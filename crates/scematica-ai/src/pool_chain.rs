// 3-layer LLM chain for pool selection.
//
//   L1  Groq / llama-3.1-8b-instant      ~200ms   fast binary screener
//   L2  OpenRouter / gemma-2-9b-it:free  ~600ms   deep analyst (score + sizing)
//   L3  Cerebras / llama-3.1-8b          ~400ms   risk judge (final decision)
//
// Decision path:
//   1. L1 runs alone. If it fails → SKIP (fast rejection, no more calls).
//   2. L2 + L3 run in parallel after L1 passes.
//   3. BUY verdict requires L2.score >= min_l2_score AND L3.decision == "BUY".
//   4. Any LLM error/timeout → return ChainVerdict::fallback() so the caller
//      falls back to deterministic pool_scorer gating.

use crate::{client::AiClient, types::AiProvider};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tracing::{debug, warn};

/// On-chain + contextual signals available at filter time.
#[derive(Debug, Clone, Serialize)]
pub struct PoolSignals {
    /// Pool age in seconds at detection time (0 if unknown)
    pub pool_age_secs: u64,
    /// Quote vault balance in SOL at detection time
    pub pool_size_sol: f64,
    /// Deterministic pool_scorer result (0–100) — grounding signal for LLMs
    pub scorer_score: f64,
    /// SOL flowing into pool per second (pool_size_sol / age)
    pub velocity_sol_per_sec: f64,
    /// quote_vault / base_vault ratio; > 1 = accumulated buying
    pub buy_pressure_ratio: f64,
    /// Deployer EMA rug score from reputation ledger (0.0 = clean, 1.0 = known rugger)
    pub deployer_rug_score: f64,
    /// UTC hour of detection (0–23) for time-of-day context
    pub time_of_day_utc_hour: u8,
    /// Recent trade wins (last 20 trades)
    pub recent_wins: u32,
    /// Recent trade losses (last 20 trades)
    pub recent_losses: u32,
    /// Currently open positions
    pub open_positions: u32,
    /// Accumulated PnL this session in SOL (negative = losing)
    pub daily_pnl_sol: f64,
}

// ── per-layer output types (only used internally) ─────────────────────────────

#[derive(Debug, Deserialize)]
struct L1Screen {
    pass: bool,
    confidence: u8,
    reason: String,
}

#[derive(Debug, Deserialize)]
struct L2Analysis {
    score: u8,
    buy: bool,
    #[serde(default = "default_size_mult")]
    size_mult: f64,
    #[serde(default)]
    thesis: String,
}

fn default_size_mult() -> f64 {
    1.0
}

#[derive(Debug, Deserialize)]
struct L3Decision {
    decision: String, // "BUY" | "SKIP"
    #[serde(default = "default_size_sol")]
    size_sol: f64,
    #[serde(default = "default_hold")]
    max_hold_secs: u32,
    #[serde(default = "default_sl")]
    stop_loss_pct: f64,
}

fn default_size_sol() -> f64 {
    0.01
}
fn default_hold() -> u32 {
    20
}
fn default_sl() -> f64 {
    8.0
}

// ── public output type ────────────────────────────────────────────────────────

/// Final verdict from the 3-layer chain.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainVerdict {
    /// Whether the chain recommends buying this pool
    pub buy: bool,
    /// Recommended position size in SOL (already adjusted by L2 size_mult)
    pub size_sol: f64,
    /// Recommended max hold time in seconds
    pub max_hold_secs: u32,
    /// Recommended stop-loss %
    pub stop_loss_pct: f64,
    /// L1 confidence score (0–100)
    pub l1_confidence: u8,
    /// L2 analyst score (0–100)
    pub l2_score: u8,
    /// Human-readable reasoning chain
    pub reasoning: String,
    /// true if chain was unavailable and caller should use pool_scorer fallback
    pub fallback: bool,
}

impl ChainVerdict {
    pub fn skip(reason: impl Into<String>) -> Self {
        Self {
            buy: false,
            size_sol: 0.0,
            max_hold_secs: 0,
            stop_loss_pct: 0.0,
            l1_confidence: 0,
            l2_score: 0,
            reasoning: reason.into(),
            fallback: false,
        }
    }

    /// Chain was unavailable — caller should use pool_scorer logic.
    pub fn fallback() -> Self {
        Self {
            buy: true,
            size_sol: 0.0,
            max_hold_secs: 20,
            stop_loss_pct: 8.0,
            l1_confidence: 0,
            l2_score: 0,
            reasoning: "chain-unavailable: falling back to pool_scorer".into(),
            fallback: true,
        }
    }
}

// ── judge ─────────────────────────────────────────────────────────────────────

pub struct PoolChainJudge {
    l1: AiClient,
    l2: AiClient,
    l3: AiClient,
    min_l2_score: u8,
    l1_timeout: Duration,
    l2l3_timeout: Duration,
}

impl PoolChainJudge {
    /// Try to construct the judge. Returns None if any required API key is missing.
    pub fn try_new(
        l1_model: &str,
        l2_model: &str,
        l3_model: &str,
        min_l2_score: u8,
        l1_timeout_ms: u64,
        l2l3_timeout_ms: u64,
    ) -> Option<Self> {
        dotenv::dotenv().ok();

        let l1 = match AiClient::new(AiProvider::Groq) {
            Ok(c) => c.with_model(l1_model),
            Err(e) => {
                warn!(error=%e, "PoolChain L1 (Groq) unavailable");
                return None;
            }
        };
        let l2 = match AiClient::new(AiProvider::OpenRouter) {
            Ok(c) => c.with_model(l2_model),
            Err(e) => {
                warn!(error=%e, "PoolChain L2 (OpenRouter) unavailable");
                return None;
            }
        };
        let l3 = match AiClient::new(AiProvider::Cerebras) {
            Ok(c) => c.with_model(l3_model),
            Err(e) => {
                warn!(error=%e, "PoolChain L3 (Cerebras) unavailable");
                return None;
            }
        };

        Some(Self {
            l1,
            l2,
            l3,
            min_l2_score,
            l1_timeout: Duration::from_millis(l1_timeout_ms),
            l2l3_timeout: Duration::from_millis(l2l3_timeout_ms),
        })
    }

    /// Run the 3-layer chain on a pool. Never panics — errors produce fallback verdicts.
    pub async fn judge(&self, signals: &PoolSignals, base_size_sol: f64) -> ChainVerdict {
        // ── Layer 1: fast screener ────────────────────────────────────────────
        let l1 = match tokio::time::timeout(self.l1_timeout, self.run_l1(signals)).await {
            Err(_) => {
                warn!("PoolChain L1 timed out — fallback to scorer");
                return ChainVerdict::fallback();
            }
            Ok(Err(e)) => {
                warn!(error=%e, "PoolChain L1 error — fallback to scorer");
                return ChainVerdict::fallback();
            }
            Ok(Ok(r)) => r,
        };

        debug!(
            pass = l1.pass,
            confidence = l1.confidence,
            reason = %l1.reason,
            "PoolChain L1 screener"
        );

        if !l1.pass {
            return ChainVerdict::skip(format!("L1 reject: {}", l1.reason));
        }

        // ── Layer 2 + 3 in parallel ───────────────────────────────────────────
        let l1_reason = l1.reason.clone();
        let (l2_res, l3_res) = tokio::join!(
            tokio::time::timeout(self.l2l3_timeout, self.run_l2(signals, &l1_reason)),
            tokio::time::timeout(
                self.l2l3_timeout,
                self.run_l3(signals, &l1_reason, base_size_sol)
            ),
        );

        let l2 = match l2_res {
            Err(_) => {
                warn!("PoolChain L2 timed out — fallback to scorer");
                return ChainVerdict::fallback();
            }
            Ok(Err(e)) => {
                warn!(error=%e, "PoolChain L2 error — fallback to scorer");
                return ChainVerdict::fallback();
            }
            Ok(Ok(r)) => r,
        };

        let l3 = match l3_res {
            Err(_) => {
                warn!("PoolChain L3 timed out — fallback to scorer");
                return ChainVerdict::fallback();
            }
            Ok(Err(e)) => {
                warn!(error=%e, "PoolChain L3 error — fallback to scorer");
                return ChainVerdict::fallback();
            }
            Ok(Ok(r)) => r,
        };

        debug!(
            l2_score = l2.score,
            l2_buy = l2.buy,
            l2_size_mult = l2.size_mult,
            l3_decision = %l3.decision,
            l3_size_sol = l3.size_sol,
            "PoolChain L2+L3"
        );

        let consensus_buy =
            l2.buy && l2.score >= self.min_l2_score && l3.decision.to_uppercase() == "BUY";

        let final_size = if consensus_buy {
            (l3.size_sol * l2.size_mult.clamp(0.5, 2.5)).max(base_size_sol * 0.5)
        } else {
            0.0
        };

        ChainVerdict {
            buy: consensus_buy,
            size_sol: final_size,
            max_hold_secs: l3.max_hold_secs,
            stop_loss_pct: l3.stop_loss_pct,
            l1_confidence: l1.confidence,
            l2_score: l2.score,
            reasoning: format!(
                "L1({}) | L2(score={}, {}) | L3({}): {}",
                l1.reason,
                l2.score,
                l2.thesis,
                l3.decision,
                if consensus_buy { "BUY" } else { "SKIP" },
            ),
            fallback: false,
        }
    }

    // ── individual layer calls ────────────────────────────────────────────────

    async fn run_l1(&self, s: &PoolSignals) -> Result<L1Screen> {
        let signals = serde_json::to_string(s)?;
        let resp = self.l1.ask_json(
            "You are a Solana memecoin pool screener. Given on-chain pool signals, decide if the pool warrants deeper analysis. Respond with valid JSON only — no markdown, no explanation.",
            &format!(
                "Pool signals: {}\n\n\
                Key criteria for passing: \
                pool_age_secs <= 7 (ultra-fresh), \
                pool_size_sol between 6.5 and 28 (sweet spot), \
                scorer_score >= 88, \
                deployer_rug_score < 0.5, \
                velocity_sol_per_sec > 0. \
                Ghost pools (pool_size_sol = 0) always fail. \
                Large pools > 100 SOL almost never pump from this entry point.\n\n\
                Respond exactly: {{\"pass\":bool,\"confidence\":int,\"reason\":string}}",
                signals,
            ),
        ).await?;
        Ok(serde_json::from_str(&extract_json(&resp))?)
    }

    async fn run_l2(&self, s: &PoolSignals, l1_reason: &str) -> Result<L2Analysis> {
        let signals = serde_json::to_string(s)?;
        let resp = self.l2.ask_json(
            "You are a Solana trading analyst. Analyze memecoin pool signals and score pump potential 0-100. Respond with valid JSON only.",
            &format!(
                "Pool signals: {}\n\
                L1 screener finding: {}\n\n\
                Score this pool 0-100 for pump potential. Consider: \
                freshness (younger = better, <7s is best), \
                liquidity sweet spot (6.5-28 SOL = highest pump probability), \
                velocity (fast SOL inflow = crowd momentum), \
                buy pressure ratio (>1 = accumulation), \
                deployer reputation (near 0 = clean). \
                The bot is currently bleeding funds — be conservative, score < 65 should be SKIP.\n\n\
                Respond exactly: {{\"score\":int,\"buy\":bool,\"size_mult\":float,\"thesis\":string}}",
                signals, l1_reason,
            ),
        ).await?;
        Ok(serde_json::from_str(&extract_json(&resp))?)
    }

    async fn run_l3(
        &self,
        s: &PoolSignals,
        l1_reason: &str,
        base_size_sol: f64,
    ) -> Result<L3Decision> {
        let signals = serde_json::to_string(s)?;
        let win_rate = if s.recent_wins + s.recent_losses > 0 {
            s.recent_wins as f64 / (s.recent_wins + s.recent_losses) as f64
        } else {
            0.5
        };
        let resp = self.l3.ask_json(
            "You are a risk manager for a Solana trading bot. Make final buy/skip decisions with tight risk parameters. Respond with valid JSON only.",
            &format!(
                "Pool signals: {}\n\
                Screener summary: {}\n\
                Session state: {}/{} wins/losses ({:.0}% win rate), \
                {} open positions, {:.4} SOL daily PnL.\n\
                Base position size: {:.4} SOL.\n\n\
                The bot has been losing money. Be conservative: \
                only BUY if the pool is clearly ultra-fresh + sweet-spot + active. \
                Recommend tight stop-loss (6-10%) and short max hold (15-25s for runners). \
                size_sol should be between {:.4} and {:.4} SOL.\n\n\
                Respond exactly: {{\"decision\":\"BUY\" or \"SKIP\",\"size_sol\":float,\"max_hold_secs\":int,\"stop_loss_pct\":float}}",
                signals, l1_reason,
                s.recent_wins, s.recent_losses, win_rate * 100.0,
                s.open_positions, s.daily_pnl_sol,
                base_size_sol,
                base_size_sol * 0.5,
                base_size_sol * 2.5,
            ),
        ).await?;
        Ok(serde_json::from_str(&extract_json(&resp))?)
    }
}

// ── JSON extraction helper ────────────────────────────────────────────────────

/// Strip markdown fences and extract the first JSON object from a string.
fn extract_json(s: &str) -> String {
    let s = s.trim();
    // Strip ```json ... ``` fences
    let s = if let Some(inner) = s.strip_prefix("```") {
        let inner = inner.trim_start_matches("json").trim_start_matches('\n');
        if let Some(pos) = inner.rfind("```") {
            inner[..pos].trim()
        } else {
            inner.trim()
        }
    } else {
        s
    };
    // Extract just the JSON object
    if let (Some(start), Some(end)) = (s.find('{'), s.rfind('}')) {
        s[start..=end].to_string()
    } else {
        s.to_string()
    }
}
