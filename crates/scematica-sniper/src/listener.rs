use anyhow::Result;
use futures::{SinkExt, StreamExt};
use serde_json::{json, Value};
use solana_sdk::pubkey::Pubkey;
use std::collections::HashSet;
use std::str::FromStr;
use std::sync::Arc;
use parking_lot::Mutex;
use tokio::sync::mpsc;
use tokio_tungstenite::{connect_async, tungstenite::Message};
use tracing::{debug, error, info, warn};

use crate::cache::{CachedMarket, CachedPool};
use scematica_core::dex::program_ids;

/// Events emitted by the listener
#[derive(Debug, Clone)]
pub enum ListenerEvent {
    NewPool(CachedPool),
    NewMarket(CachedMarket),
    WalletUpdate { account: Pubkey, mint: Pubkey, amount: u64 },
}

/// Subscribes to Solana WebSocket for new Raydium pools and wallet changes
pub struct PoolListener {
    ws_url: String,
    wallet: Pubkey,
    quote_mint: Pubkey,
    event_tx: mpsc::Sender<ListenerEvent>,
    /// Pool IDs we've already dispatched this session. Raydium fires
    /// programNotification on every pool state change (each swap, deposit,
    /// withdrawal, etc.) so a single pool generates dozens-to-hundreds of
    /// events per minute. We dedup here so decode/channel-send/task-spawn
    /// work happens AT MOST ONCE per pool. The persistent pool-cache.json
    /// guard in on_new_pool is kept as a cross-session safety net.
    seen_pool_ids: Arc<Mutex<HashSet<Pubkey>>>,
}

impl PoolListener {
    pub fn new(
        ws_url: impl Into<String>,
        wallet: Pubkey,
        quote_mint: Pubkey,
        event_tx: mpsc::Sender<ListenerEvent>,
    ) -> Self {
        Self {
            ws_url: ws_url.into(),
            wallet,
            quote_mint,
            event_tx,
            seen_pool_ids: Arc::new(Mutex::new(HashSet::new())),
        }
    }

    /// Start listening. Runs until the channel is closed or connection drops.
    pub async fn run(&self) -> Result<()> {
        info!("Connecting to WebSocket: {}", self.ws_url);
        let (ws_stream, _) = connect_async(&self.ws_url).await?;
        let (mut write, mut read) = ws_stream.split();

        // Subscribe to Raydium AMM V4 program account changes.
        // decode_raydium_v4_pool filters out established pools by open_time.
        let pool_sub = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "programSubscribe",
            "params": [
                program_ids::RAYDIUM_AMM_V4.to_string(),
                {
                    "encoding": "base64",
                    "commitment": "processed",
                    "filters": [
                        { "dataSize": 752 }
                    ]
                }
            ]
        });
        write.send(Message::Text(pool_sub.to_string())).await?;

        // Subscribe to wallet account changes (for sell triggers)
        let wallet_sub = json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "accountSubscribe",
            "params": [
                self.wallet.to_string(),
                { "encoding": "jsonParsed", "commitment": "confirmed" }
            ]
        });
        write.send(Message::Text(wallet_sub.to_string())).await?;

        info!("WebSocket subscriptions active");

        // Stall watchdog + heartbeat. Previously, a silently-dead WS (no FIN, no
        // RST, just stops sending data — typical NAT/LB timeouts) made the bot
        // appear alive while delivering zero pool events for minutes. Now:
        //   • outbound ping every 20 s keeps NAT/LB state warm
        //   • 60 s read-timeout → drop the connection and let the outer reconnect loop fire
        //   • 30 s heartbeat log so operators can see the listener IS working
        const READ_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);
        const PING_INTERVAL: std::time::Duration = std::time::Duration::from_secs(20);
        const HEARTBEAT_INTERVAL: std::time::Duration = std::time::Duration::from_secs(30);

        let mut ping_clock = tokio::time::interval(PING_INTERVAL);
        ping_clock.tick().await; // skip the immediate-fire tick
        let mut hb_clock = tokio::time::interval(HEARTBEAT_INTERVAL);
        hb_clock.tick().await;

        let mut text_msgs: u64 = 0;
        let mut pool_events: u64 = 0;
        let mut last_event = std::time::Instant::now();

        loop {
            tokio::select! {
                msg = tokio::time::timeout(READ_TIMEOUT, read.next()) => {
                    let msg = match msg {
                        Err(_) => {
                            warn!(
                                text_msgs, pool_events,
                                idle_secs = last_event.elapsed().as_secs(),
                                "WS read stalled >60s — forcing reconnect"
                            );
                            break;
                        }
                        Ok(None) => {
                            warn!("WS stream ended");
                            break;
                        }
                        Ok(Some(m)) => m,
                    };
                    last_event = std::time::Instant::now();
                    match msg {
                        Ok(Message::Text(text)) => {
                            text_msgs += 1;
                            if text.contains("programNotification") { pool_events += 1; }
                            if let Err(e) = self.handle_message(&text).await {
                                warn!("Error handling WS message: {}", e);
                            }
                        }
                        Ok(Message::Ping(data)) => { let _ = write.send(Message::Pong(data)).await; }
                        Ok(Message::Pong(_)) => {}
                        Ok(Message::Close(_)) => { warn!("WebSocket connection closed by peer"); break; }
                        Err(e) => { error!("WebSocket error: {}", e); break; }
                        _ => {}
                    }
                }
                _ = ping_clock.tick() => {
                    if let Err(e) = write.send(Message::Ping(vec![])).await {
                        warn!("WS ping send failed: {} — reconnect", e);
                        break;
                    }
                }
                _ = hb_clock.tick() => {
                    info!(
                        text_msgs, pool_events,
                        idle_secs = last_event.elapsed().as_secs(),
                        "📡 Listener heartbeat",
                    );
                }
            }
        }

        Ok(())
    }

    async fn handle_message(&self, text: &str) -> Result<()> {
        let value: Value = serde_json::from_str(text)?;

        // Subscription confirmation
        if value.get("result").is_some() && value.get("id").is_some() {
            debug!("Subscription confirmed: id={}", value["id"]);
            return Ok(());
        }

        let method = value["method"].as_str().unwrap_or("");

        match method {
            "programNotification" => {
                self.handle_pool_notification(&value).await?;
            }
            "accountNotification" => {
                self.handle_wallet_notification(&value).await?;
            }
            _ => {}
        }

        Ok(())
    }

    async fn handle_pool_notification(&self, value: &Value) -> Result<()> {
        let params = &value["params"]["result"];
        let pubkey_str = params["value"]["pubkey"].as_str().unwrap_or("");
        let data = &params["value"]["account"]["data"];

        // Cheap dedup BEFORE any decoding. Raydium fires programNotification on
        // every pool state change (every swap, deposit, withdraw) so a single
        // pool emits 50–200 events per minute. Without this gate, each duplicate
        // event ran the base64 decoder, the layout parser, normalisation, the
        // channel send, and a fresh tokio::spawn — wasted CPU and channel slots.
        if let Ok(pool_id) = Pubkey::from_str(pubkey_str) {
            let already_seen = {
                let mut seen = self.seen_pool_ids.lock();
                if seen.contains(&pool_id) {
                    true
                } else {
                    seen.insert(pool_id);
                    // Cap the set so a long-running session can't grow unbounded.
                    // 20k pool IDs ≈ 640 KB which is fine, but anything past that
                    // is just past sessions we've already shipped to evaluator.
                    if seen.len() > 20_000 {
                        // Crude eviction: clear half the entries. The chance of
                        // re-seeing an evicted pool's events is tiny because each
                        // pool's burst of state-change events happens in the first
                        // few minutes of its life.
                        let to_drop: Vec<Pubkey> =
                            seen.iter().take(10_000).copied().collect();
                        for k in to_drop { seen.remove(&k); }
                    }
                    false
                }
            };
            if already_seen {
                return Ok(());
            }
        }

        if let Some(b64) = data[0].as_str() {
            use base64::Engine;
            let bytes = base64::engine::general_purpose::STANDARD.decode(b64)?;
            if bytes.len() >= 752 {
                if let Some(pool) = decode_raydium_v4_pool(pubkey_str, &bytes) {
                    // Raydium pools can be initialised with either token in the "coin"
                    // (base) slot. pump.fun's newer migration path puts WSOL as base
                    // and the memecoin as quote — opposite of the original convention.
                    // Normalise so downstream code always sees:
                    //   pool.base_mint  = the target memecoin
                    //   pool.quote_mint = our configured quote (e.g. WSOL)
                    // The on-chain swap builder reads vaults from chain state, so
                    // swapping our cached view doesn't affect tx correctness.
                    let mut pool = pool;
                    if pool.base_mint == self.quote_mint && pool.quote_mint != self.quote_mint {
                        std::mem::swap(&mut pool.base_mint, &mut pool.quote_mint);
                        std::mem::swap(&mut pool.base_vault, &mut pool.quote_vault);
                        std::mem::swap(&mut pool.base_decimals, &mut pool.quote_decimals);
                    }
                    info!(
                        pool = %pool.id,
                        base = %pool.base_mint,
                        quote = %pool.quote_mint,
                        open_time = pool.open_time,
                        "📥 Listener decoded pool — dispatching to evaluator",
                    );
                    let _ = self.event_tx.send(ListenerEvent::NewPool(pool)).await;
                }
            }
        }

        Ok(())
    }

    async fn handle_wallet_notification(&self, value: &Value) -> Result<()> {
        let parsed = &value["params"]["result"]["value"]["data"]["parsed"]["info"];
        if let (Some(mint_str), Some(amount_str)) = (
            parsed["mint"].as_str(),
            parsed["tokenAmount"]["amount"].as_str(),
        ) {
            if let (Ok(mint), Ok(amount)) = (
                Pubkey::from_str(mint_str),
                amount_str.parse::<u64>(),
            ) {
                // Skip quote token updates
                if mint != self.quote_mint {
                    let account_str = value["params"]["result"]["context"]["slot"]
                        .as_str()
                        .unwrap_or("");
                    let account = Pubkey::from_str(account_str).unwrap_or(self.wallet);
                    let _ = self
                        .event_tx
                        .send(ListenerEvent::WalletUpdate { account, mint, amount })
                        .await;
                }
            }
        }
        Ok(())
    }
}

/// Decode a Raydium V4 pool state from raw account bytes
fn decode_raydium_v4_pool(pubkey_str: &str, data: &[u8]) -> Option<CachedPool> {
    use scematica_core::dex::raydium_v4::*;

    if data.len() < POOL_STATE_SIZE {
        return None;
    }

    let pool_id = Pubkey::from_str(pubkey_str).ok()?;

    let base_mint = Pubkey::try_from(&data[BASE_MINT_OFFSET..BASE_MINT_OFFSET + 32]).ok()?;
    let quote_mint = Pubkey::try_from(&data[QUOTE_MINT_OFFSET..QUOTE_MINT_OFFSET + 32]).ok()?;
    let base_vault = Pubkey::try_from(&data[BASE_VAULT_OFFSET..BASE_VAULT_OFFSET + 32]).ok()?;
    let quote_vault = Pubkey::try_from(&data[QUOTE_VAULT_OFFSET..QUOTE_VAULT_OFFSET + 32]).ok()?;
    let market_id = Pubkey::try_from(&data[MARKET_ID_OFFSET..MARKET_ID_OFFSET + 32]).ok()?;

    let open_time = u64::from_le_bytes(
        data[OPEN_TIME_OFFSET..OPEN_TIME_OFFSET + 8].try_into().ok()?
    );

    // Reject partially-initialised pool accounts — all key pubkeys must be non-zero.
    // programSubscribe fires before the pool account is fully written; reading it
    // at that moment yields a zero base_mint, which causes ATA creation to fail with
    // InvalidMint (0x1).
    if base_mint == Pubkey::default()
        || quote_mint == Pubkey::default()
        || base_vault == Pubkey::default()
        || quote_vault == Pubkey::default()
        || market_id == Pubkey::default()
    {
        return None;
    }

    // Reject established pools — only pass pools whose open_time is within the last
    // 5 minutes or in the future. programSubscribe fires on every pool state change
    // (swaps, deposits, etc.); this prevents us from trying to snipe pools that have
    // been live for hours.
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    if open_time > 0 && open_time + 300 < now {
        return None; // Pool opened more than 5 minutes ago — skip
    }

    Some(CachedPool {
        id: pool_id,
        base_mint,
        quote_mint,
        base_vault,
        quote_vault,
        market_id,
        open_time,
        base_decimals: 9,
        quote_decimals: 9,
        pumpfun_score: 0.0,
    })
}
