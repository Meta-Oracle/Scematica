use anyhow::Result;
use dashmap::DashMap;
use futures::{SinkExt, StreamExt};
use serde::Deserialize;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::pubkey::Pubkey;
use std::collections::VecDeque;
use std::str::FromStr;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use tokio_tungstenite::{connect_async, tungstenite::Message};
use tracing::{debug, info, warn};

use crate::cache::CachedPool;
use crate::listener::ListenerEvent;

// ── PumpPortal WebSocket endpoint ────────────────────────────────────────────
const PUMPPORTAL_WS: &str = "wss://pumpportal.fun/api/data";

// Pump.fun bonding curve graduation threshold (~69 SOL in curve)
const GRADUATION_SOL: f64 = 69.0;

// Subscription messages sent on connect
const SUB_NEW_TOKEN:  &str = r#"{"method":"subscribeNewToken"}"#;
const SUB_MIGRATIONS: &str = r#"{"method":"subscribeRaydiumMigrations"}"#;

// Raydium AMM V4 pool state layout offsets (same as listener.rs)
const BASE_MINT_OFFSET:   usize = 400;
const QUOTE_MINT_OFFSET:  usize = 432;
const BASE_VAULT_OFFSET:  usize = 336;
const QUOTE_VAULT_OFFSET: usize = 368;
const MARKET_ID_OFFSET:   usize = 464;
const OPEN_TIME_OFFSET:   usize = 200;
const POOL_STATE_MIN_SIZE: usize = 496;

/// Configuration for the Pump.fun trending monitor.
/// All fields are optional — defaults are tuned for real-money sniping.
#[derive(Debug, Clone)]
pub struct PumpFunTrendingConfig {
    /// Minimum trending score (0-100) to pre-flag a token on graduation.
    /// Score = buy_pressure*40 + volume_velocity*30 + curve_fill*30.
    /// Default: 55.0 — requires moderate buy pressure + meaningful volume.
    pub min_trending_score: f64,
    /// Minimum bonding curve fill % before considering a token trending.
    /// Prevents sniping tokens that just launched with zero traction.
    /// Default: 40.0 (28 SOL of the 69 SOL graduation threshold).
    pub min_curve_pct: f64,
    /// How long to track a token's trades in the sliding window (seconds).
    /// Default: 120 — 2 minutes of data for the momentum signal.
    pub track_window_secs: u64,
    /// Maximum number of tokens tracked simultaneously.
    /// Oldest-first eviction when limit is hit.
    /// Default: 300.
    pub max_tracked_tokens: usize,
}

impl Default for PumpFunTrendingConfig {
    fn default() -> Self {
        Self {
            min_trending_score: 55.0,
            min_curve_pct: 40.0,
            track_window_secs: 120,
            max_tracked_tokens: 300,
        }
    }
}

/// A single trade event broadcast by PumpPortal
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct PumpEvent {
    /// "create" | "buy" | "sell" | "migration"
    tx_type: String,
    mint: String,
    /// SOL amount traded (buys/sells only)
    sol_amount: f64,
    is_buy: bool,
    /// Current SOL in the bonding curve (updated on every trade)
    #[serde(rename = "vSolInBondingCurve")]
    v_sol_in_curve: f64,
    /// Raydium pool pubkey — migration events only
    pool: String,
    name: String,
    symbol: String,
}

/// Sliding-window state for one token
struct TokenWindow {
    /// (wall_time, is_buy, sol_amount) for the last `track_window_secs`
    trades: VecDeque<(Instant, bool, f64)>,
    /// Most recent bonding curve SOL balance
    curve_sol: f64,
    /// Unix timestamp of first observation
    first_seen_secs: u64,
    /// Whether we've already emitted a pre-graduation signal for this mint
    pre_flagged: bool,
}

impl TokenWindow {
    fn new(curve_sol: f64) -> Self {
        let first_seen_secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        Self {
            trades: VecDeque::new(),
            curve_sol,
            first_seen_secs,
            pre_flagged: false,
        }
    }

    fn add_trade(&mut self, is_buy: bool, sol_amount: f64, curve_sol: f64, window_secs: u64) {
        let now = Instant::now();
        self.trades.push_back((now, is_buy, sol_amount));
        if curve_sol > 0.0 { self.curve_sol = curve_sol; }
        // Prune entries outside the sliding window
        let cutoff = now - Duration::from_secs(window_secs);
        while self.trades.front().map(|(t, ..)| *t < cutoff).unwrap_or(false) {
            self.trades.pop_front();
        }
    }

    /// Compute trending score 0–100.
    ///
    /// Components:
    ///   Buy pressure (0-40): buy_count / total_count, scaled 40%→80% → 0→40 pts
    ///   Volume velocity (0-30): capped at 2 SOL/min for full score
    ///   Curve fill (0-30): capped at graduation threshold
    fn trending_score(&self) -> f64 {
        if self.trades.len() < 3 {
            return 0.0;
        }

        let (buy_n, buy_vol, sell_vol) = self.trades.iter().fold(
            (0u32, 0f64, 0f64),
            |(bn, bv, sv), (_, is_buy, sol)| {
                if *is_buy { (bn + 1, bv + sol, sv) }
                else       { (bn, bv, sv + sol) }
            },
        );
        let total_n  = self.trades.len() as f64;
        let buy_ratio = buy_n as f64 / total_n;
        let total_vol = buy_vol + sell_vol;

        // Buy pressure: floor at 40%, ceiling at 80% for full points
        let buy_score = ((buy_ratio - 0.40) / 0.40).clamp(0.0, 1.0) * 40.0;

        // Volume velocity: 2 SOL/min → full 30 pts
        // Span is at most window_secs; use actual elapsed from first trade
        let span_secs = self.trades.front()
            .map(|(t, ..)| t.elapsed().as_secs_f64())
            .unwrap_or(1.0)
            .max(1.0);
        let sol_per_min = total_vol / span_secs * 60.0;
        let vel_score = (sol_per_min / 2.0).clamp(0.0, 1.0) * 30.0;

        // Curve fill: cap at graduation
        let fill_score = (self.curve_sol / GRADUATION_SOL).clamp(0.0, 1.0) * 30.0;

        buy_score + vel_score + fill_score
    }

    fn curve_fill_pct(&self) -> f64 {
        self.curve_sol / GRADUATION_SOL * 100.0
    }
}

/// Pump.fun trending monitor.
///
/// Connects to the PumpPortal WebSocket, subscribes to all new-token creates
/// and Raydium migration events.  For each new token it immediately also
/// subscribes to its individual trade stream.  A sliding-window trending score
/// is maintained per token so that when a highly-trending token graduates we
/// can emit it through the sniper pipeline ahead of the standard Raydium
/// AMM V4 listener (which typically lags PumpPortal by 0.5–3 s).
///
/// Architecture note: this monitor emits `ListenerEvent::NewPool` — exactly
/// the same type as the Raydium listener — so the sniper's dedup guard
/// (seen_pool_ids) prevents double-processing.
pub struct PumpFunTrendingMonitor {
    rpc: Arc<RpcClient>,
    tx: mpsc::Sender<ListenerEvent>,
    config: PumpFunTrendingConfig,
}

impl PumpFunTrendingMonitor {
    pub fn new(
        rpc_url: String,
        tx: mpsc::Sender<ListenerEvent>,
        config: PumpFunTrendingConfig,
    ) -> Self {
        Self {
            rpc: Arc::new(RpcClient::new(rpc_url)),
            tx,
            config,
        }
    }

    /// Run the trending monitor.  Reconnects indefinitely on WebSocket drops.
    pub async fn run(&self) -> Result<()> {
        loop {
            match self.run_inner().await {
                Ok(()) => {
                    warn!("PumpFun trending: WS connection closed — reconnecting in 3 s");
                }
                Err(e) => {
                    warn!("PumpFun trending: WS error: {} — reconnecting in 3 s", e);
                }
            }
            tokio::time::sleep(Duration::from_secs(3)).await;
        }
    }

    async fn run_inner(&self) -> Result<()> {
        let (ws, _) = connect_async(PUMPPORTAL_WS).await?;
        let (mut write, mut read) = ws.split();
        info!("PumpFun trending: connected to PumpPortal");

        // Subscribe to all new-token creates and Raydium migrations
        write.send(Message::Text(SUB_NEW_TOKEN.to_string())).await?;
        write.send(Message::Text(SUB_MIGRATIONS.to_string())).await?;

        // Channel: main loop sends subscription messages to the write task
        let (ws_tx, mut ws_rx) = mpsc::channel::<String>(256);

        // Write task: drains ws_rx and sends each message over the WS write half.
        // Runs independently so the read loop is never blocked on a write.
        let write_task = tokio::spawn(async move {
            while let Some(msg) = ws_rx.recv().await {
                if write.send(Message::Text(msg)).await.is_err() { break; }
            }
        });

        // Per-token sliding-window state
        let windows: DashMap<String, TokenWindow> = DashMap::new();

        let cfg       = &self.config;
        let rpc       = Arc::clone(&self.rpc);
        let event_tx  = self.tx.clone();

        while let Some(msg) = read.next().await {
            let raw = match msg? {
                Message::Text(s) => s,
                Message::Close(_) => break,
                _ => continue,
            };

            // PumpPortal occasionally sends arrays; handle both forms
            let events: Vec<PumpEvent> = if raw.trim_start().starts_with('[') {
                serde_json::from_str(&raw).unwrap_or_default()
            } else {
                match serde_json::from_str::<PumpEvent>(&raw) {
                    Ok(e)  => vec![e],
                    Err(_) => continue,
                }
            };

            for ev in events {
                if ev.mint.is_empty() { continue; }

                match ev.tx_type.as_str() {

                    // ── New token created on bonding curve ────────────────────
                    "create" => {
                        debug!(mint = %ev.mint, name = %ev.name, symbol = %ev.symbol,
                               "PumpFun: new token");

                        windows.entry(ev.mint.clone())
                            .or_insert_with(|| TokenWindow::new(ev.v_sol_in_curve));

                        // Evict oldest entry if over the cap
                        if windows.len() > cfg.max_tracked_tokens {
                            if let Some(stale) = windows.iter()
                                .min_by_key(|e| e.value().first_seen_secs)
                                .map(|e| e.key().clone())
                            {
                                windows.remove(&stale);
                            }
                        }

                        // Subscribe to per-token trade events
                        let sub_msg = serde_json::json!({
                            "method": "subscribeTokenTrade",
                            "keys": [ev.mint],
                        }).to_string();
                        let _ = ws_tx.send(sub_msg).await;
                    }

                    // ── Buy / Sell on bonding curve ───────────────────────────
                    "buy" | "sell" => {
                        let mut entry = windows
                            .entry(ev.mint.clone())
                            .or_insert_with(|| TokenWindow::new(ev.v_sol_in_curve));

                        entry.add_trade(
                            ev.is_buy, ev.sol_amount,
                            ev.v_sol_in_curve, cfg.track_window_secs,
                        );

                        let score       = entry.trending_score();
                        let curve_pct   = entry.curve_fill_pct();
                        let pre_flagged = entry.pre_flagged;

                        // Log when a token first crosses the trending threshold
                        if score >= cfg.min_trending_score
                            && curve_pct >= cfg.min_curve_pct
                            && !pre_flagged
                        {
                            entry.pre_flagged = true;
                            info!(
                                mint      = %ev.mint,
                                score     = %format!("{:.1}", score),
                                curve_pct = %format!("{:.1}%", curve_pct),
                                "📈 PumpFun trending: pre-flagged for graduation snipe"
                            );
                        }

                        debug!(
                            mint      = %ev.mint,
                            score     = %format!("{:.1}", score),
                            curve_pct = %format!("{:.1}%", curve_pct),
                            is_buy    = ev.is_buy,
                            sol       = ev.sol_amount,
                        );
                    }

                    // ── Token graduated to Raydium ────────────────────────────
                    "migration" => {
                        // Read trending state then immediately remove from map
                        let trending_state = windows.remove(&ev.mint)
                            .map(|(_, w)| (w.trending_score(), w.curve_fill_pct(), w.pre_flagged));
                        let (score, curve_pct, was_pre_flagged) =
                            trending_state.unwrap_or((0.0, 0.0, false));

                        info!(
                            mint            = %ev.mint,
                            pool            = %ev.pool,
                            score           = %format!("{:.1}", score),
                            curve_pct       = %format!("{:.1}%", curve_pct),
                            was_pre_flagged,
                            "🎓 PumpFun migration → Raydium"
                        );

                        if ev.pool.is_empty() {
                            warn!(mint = %ev.mint, "PumpFun migration: no pool pubkey in event");
                            continue;
                        }

                        // Spawn pool fetch so the read loop isn't blocked
                        let pool_pk_str = ev.pool.clone();
                        let mint_str    = ev.mint.clone();
                        let rpc2        = Arc::clone(&rpc);
                        let tx2         = event_tx.clone();
                        let min_score   = cfg.min_trending_score;
                        let min_curve   = cfg.min_curve_pct;

                        tokio::spawn(async move {
                            if let Some(pool) = Self::fetch_and_decode_pool(
                                &rpc2, &pool_pk_str, &mint_str,
                            ).await {
                                let trending = score >= min_score && curve_pct >= min_curve;
                                if trending {
                                    info!(
                                        mint  = %mint_str,
                                        pool  = %pool_pk_str,
                                        score = %format!("{:.1}", score),
                                        "🚀 PumpFun trending graduation — emitting to sniper"
                                    );
                                }
                                let _ = tx2.send(ListenerEvent::NewPool(pool)).await;
                            } else {
                                warn!(
                                    mint = %mint_str,
                                    pool = %pool_pk_str,
                                    "PumpFun migration: Raydium pool decode failed"
                                );
                            }
                        });
                    }

                    other => {
                        debug!("PumpFun: unknown tx_type {:?}", other);
                    }
                }
            }
        }

        write_task.abort();
        Ok(())
    }

    /// Fetch a Raydium AMM V4 pool account and decode it into a CachedPool.
    ///
    /// Uses a 3-attempt loop with a 500 ms back-off — newly-created pool accounts
    /// can take 1-2 slots to propagate to the RPC endpoint.
    async fn fetch_and_decode_pool(
        rpc: &RpcClient,
        pool_pubkey: &str,
        base_mint_hint: &str,
    ) -> Option<CachedPool> {
        let pool_pk = Pubkey::from_str(pool_pubkey).ok()?;

        for attempt in 0..3u32 {
            if attempt > 0 {
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
            let account = match rpc.get_account(&pool_pk).await {
                Ok(a) => a,
                Err(e) => {
                    debug!("PumpFun pool fetch attempt {}: {}", attempt + 1, e);
                    continue;
                }
            };
            if let Some(pool) = decode_pool_account(&pool_pk, &account.data) {
                return Some(pool);
            }
        }

        // Last resort: if we couldn't decode the pool account, try to build a
        // minimal pool using the base_mint_hint so the sniper can at least attempt
        // to look up the pool itself.
        let base_mint = Pubkey::from_str(base_mint_hint).ok()?;
        debug!(
            pool = %pool_pubkey,
            "PumpFun: pool decode failed after 3 attempts — emitting hint pool"
        );
        Some(CachedPool {
            id: pool_pk,
            base_mint,
            quote_mint: solana_sdk::pubkey!("So11111111111111111111111111111111111111112"),
            base_vault: pool_pk,  // sniper will re-fetch from pool state
            quote_vault: pool_pk,
            market_id: pool_pk,
            open_time: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            base_decimals: 6,
            quote_decimals: 9,
        })
    }
}

/// Decode a Raydium AMM V4 pool account directly from its raw data bytes.
/// Mirrors the offsets in `listener.rs::decode_raydium_v4_pool`.
fn decode_pool_account(pool_pk: &Pubkey, data: &[u8]) -> Option<CachedPool> {
    if data.len() < POOL_STATE_MIN_SIZE {
        return None;
    }

    let base_mint  = Pubkey::try_from(&data[BASE_MINT_OFFSET  ..BASE_MINT_OFFSET   + 32]).ok()?;
    let quote_mint = Pubkey::try_from(&data[QUOTE_MINT_OFFSET ..QUOTE_MINT_OFFSET  + 32]).ok()?;
    let base_vault = Pubkey::try_from(&data[BASE_VAULT_OFFSET ..BASE_VAULT_OFFSET  + 32]).ok()?;
    let quote_vault= Pubkey::try_from(&data[QUOTE_VAULT_OFFSET..QUOTE_VAULT_OFFSET + 32]).ok()?;
    let market_id  = Pubkey::try_from(&data[MARKET_ID_OFFSET  ..MARKET_ID_OFFSET   + 32]).ok()?;
    let open_time  = u64::from_le_bytes(
        data[OPEN_TIME_OFFSET..OPEN_TIME_OFFSET + 8].try_into().ok()?
    );

    // Reject zero pubkeys — pool not fully initialised yet
    if base_mint  == Pubkey::default() || quote_mint   == Pubkey::default()
    || base_vault == Pubkey::default() || quote_vault  == Pubkey::default()
    || market_id  == Pubkey::default()
    {
        return None;
    }

    Some(CachedPool {
        id: *pool_pk,
        base_mint,
        quote_mint,
        base_vault,
        quote_vault,
        market_id,
        open_time,
        base_decimals: 6,  // Pump.fun tokens are 6-decimal
        quote_decimals: 9, // WSOL is 9-decimal
    })
}
