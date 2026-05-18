use crate::{
    client::AiClient,
    prompts,
    types::{ArbScore, AiProvider, DebateOpinion, DebateResult, MarketReport, StrategyAdjustment, TokenRiskScore},
};
use chrono::Utc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;
use tracing::{debug, warn};

// ─── Rate-limit cache ─────────────────────────────────────────────────────────
// When the primary AI provider returns HTTP 429, we cache the retry-after
// expiry here and skip all calls until it clears. This eliminates per-pool HTTP
// round-trips that add latency without adding signal during rate-limited periods.

static RATE_LIMIT_UNTIL_SECS: AtomicU64 = AtomicU64::new(0);
static FALLBACK_CLIENT: OnceLock<Option<AiClient>> = OnceLock::new();

fn get_fallback() -> Option<&'static AiClient> {
    FALLBACK_CLIENT.get_or_init(|| {
        dotenv::dotenv().ok();
        // Try OpenRouter as the fallback provider (free tier available)
        if std::env::var("OPENROUTER_API_KEY").is_ok() {
            AiClient::new(AiProvider::OpenRouter).ok()
        } else if std::env::var("OLLAMA_HOST").is_ok() {
            AiClient::new(AiProvider::Ollama).ok()
        } else {
            None
        }
    }).as_ref()
}

fn is_rate_limited() -> bool {
    let until = RATE_LIMIT_UNTIL_SECS.load(Ordering::Relaxed);
    if until == 0 { return false; }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    now < until
}

/// Parse "Please try again in 1m23.456s" or "Please try again in 30s" from Groq 429 body.
fn parse_retry_after(err: &str) -> u64 {
    if let Some(start) = err.find("try again in ") {
        let rest = &err[start + "try again in ".len()..];
        let end = rest.find(['.', '\n', '"']).unwrap_or(rest.len());
        let segment = rest[..end].trim();
        if let Some(m_pos) = segment.find('m') {
            let mins: u64 = segment[..m_pos].parse().unwrap_or(0);
            let secs_str = segment.get(m_pos + 1..).unwrap_or("0").trim_end_matches('s');
            let secs: u64 = secs_str.parse().unwrap_or(0);
            return mins * 60 + secs + 10; // +10s buffer
        }
        let s = segment.trim_end_matches('s');
        if let Ok(secs) = s.parse::<u64>() {
            return secs + 10;
        }
    }
    300 // default 5-minute blackout if unparseable
}

fn handle_rate_limit(err: &str) {
    let delay = parse_retry_after(err);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    RATE_LIMIT_UNTIL_SECS.store(now + delay, Ordering::Relaxed);
    warn!("AI rate-limited — suppressing calls for {}s (until cached expiry)", delay);
}

fn is_rate_limit_err(e: &str) -> bool {
    e.contains("429") || e.contains("rate_limit") || e.contains("Rate limit")
        || e.contains("quota") || e.contains("try again in ")
}

// ─── Risk Agent ───────────────────────────────────────────────────────────────

/// Scores a new token for rug/honeypot risk before the sniper buys
pub struct RiskAgent {
    client: AiClient,
}

impl RiskAgent {
    pub fn new(client: AiClient) -> Self {
        Self { client }
    }

    /// Assess a token and return a risk score.
    /// Returns a default "skip" score if the AI call fails (fail-safe).
    /// Rate-limit aware: skips the HTTP call during cached blackout windows
    /// and automatically retries with the fallback provider on 429 errors.
    pub async fn score_token(
        &self,
        mint: &str,
        symbol: &str,
        name: &str,
        pool_size_sol: f64,
        mint_renounced: bool,
        freezable: bool,
        lp_burned: bool,
        mutable_metadata: bool,
        has_socials: bool,
        open_time_utc_hour: u8,
    ) -> TokenRiskScore {
        // Skip the HTTP round-trip entirely if we know the primary is rate-limited
        if is_rate_limited() {
            // Try fallback before giving up
            if get_fallback().is_none() {
                return Self::default_skip(mint, "rate-limited (cached)");
            }
        }

        let prompt = prompts::build_risk_prompt(
            mint,
            symbol,
            name,
            pool_size_sol,
            mint_renounced,
            freezable,
            lp_burned,
            mutable_metadata,
            has_socials,
            open_time_utc_hour,
        );

        // Choose primary or fallback client based on rate-limit state
        let result = if is_rate_limited() {
            if let Some(fb) = get_fallback() {
                fb.ask_json(prompts::RISK_AGENT_SYSTEM, &prompt).await
            } else {
                return Self::default_skip(mint, "rate-limited, no fallback");
            }
        } else {
            let res = self.client.ask_json(prompts::RISK_AGENT_SYSTEM, &prompt).await;
            // On 429: cache the blackout, then try fallback in-band
            if let Err(ref e) = res {
                let err_str = e.to_string();
                if is_rate_limit_err(&err_str) {
                    handle_rate_limit(&err_str);
                    if let Some(fb) = get_fallback() {
                        warn!(mint = %mint, "Primary AI rate-limited — trying fallback provider");
                        fb.ask_json(prompts::RISK_AGENT_SYSTEM, &prompt).await
                    } else {
                        return Self::default_skip(mint, "rate-limited, no fallback configured");
                    }
                } else {
                    res
                }
            } else {
                res
            }
        };

        match result {
            Ok(json_str) => {
                match serde_json::from_str::<TokenRiskScore>(&json_str) {
                    Ok(mut score) => {
                        score.timestamp = Utc::now();
                        debug!(
                            mint = %mint,
                            score = score.score,
                            recommendation = %score.recommendation,
                            "AI risk score"
                        );
                        score
                    }
                    Err(e) => {
                        warn!("Failed to parse AI risk response: {} | raw: {}", e, json_str);
                        Self::default_skip(mint, "Failed to parse AI response")
                    }
                }
            }
            Err(e) => {
                let err_str = e.to_string();
                if is_rate_limit_err(&err_str) {
                    handle_rate_limit(&err_str);
                }
                warn!("AI risk agent error: {}", e);
                Self::default_skip(mint, &err_str)
            }
        }
    }

    fn default_skip(_mint: &str, reason: &str) -> TokenRiskScore {
        TokenRiskScore {
            score: 0,
            recommendation: "skip".into(),
            reasoning: format!("AI unavailable: {}", reason),
            red_flags: vec!["AI assessment failed — defaulting to skip".into()],
            timestamp: Utc::now(),
        }
    }
}

// ─── Arb Agent ────────────────────────────────────────────────────────────────

/// Evaluates arb paths before execution
pub struct ArbAgent {
    client: AiClient,
}

impl ArbAgent {
    pub fn new(client: AiClient) -> Self {
        Self { client }
    }

    pub async fn score_arb(
        &self,
        hops: usize,
        dexes: &[String],
        input_amount: u64,
        raw_profit: i64,
        profit_pct: f64,
        pool_reserves: &[(u64, u64)],
    ) -> ArbScore {
        // Fast pre-filter: skip AI call for obviously bad arbs
        if raw_profit < 5_000 {
            return ArbScore {
                confidence: 0,
                estimated_net_profit: raw_profit - 5_000,
                recommendation: "skip".into(),
                reasoning: "Profit below minimum threshold (5000 lamports)".into(),
            };
        }

        let prompt = prompts::build_arb_prompt(
            hops,
            dexes,
            input_amount,
            raw_profit,
            profit_pct,
            pool_reserves,
        );

        match self.client.ask_json(prompts::ARB_AGENT_SYSTEM, &prompt).await {
            Ok(json_str) => {
                match serde_json::from_str::<ArbScore>(&json_str) {
                    Ok(score) => {
                        debug!(
                            confidence = score.confidence,
                            net_profit = score.estimated_net_profit,
                            recommendation = %score.recommendation,
                            "AI arb score"
                        );
                        score
                    }
                    Err(e) => {
                        warn!("Failed to parse AI arb response: {} | raw: {}", e, json_str);
                        // On parse failure, use raw profit estimate
                        ArbScore {
                            confidence: 50,
                            estimated_net_profit: raw_profit - 5_000,
                            recommendation: "monitor".into(),
                            reasoning: "AI parse failed — using raw estimate".into(),
                        }
                    }
                }
            }
            Err(e) => {
                warn!("AI arb agent error: {}", e);
                // On AI failure, fall back to simple threshold check
                let net = raw_profit - 5_000;
                ArbScore {
                    confidence: if net > 10_000 { 60 } else { 30 },
                    estimated_net_profit: net,
                    recommendation: if net > 10_000 { "execute".into() } else { "skip".into() },
                    reasoning: format!("AI unavailable — threshold check: net={}", net),
                }
            }
        }
    }
}

// ─── Strategy Agent ───────────────────────────────────────────────────────────

/// Dynamically adjusts trading strategy parameters
pub struct StrategyAgent {
    client: AiClient,
}

impl StrategyAgent {
    pub fn new(client: AiClient) -> Self {
        Self { client }
    }

    pub async fn get_adjustment(
        &self,
        recent_trades: &[(bool, f64)],
        total_pnl_sol: f64,
        current_tp_pct: f64,
        current_sl_pct: f64,
        current_amount_sol: f64,
        win_rate: f64,
    ) -> StrategyAdjustment {
        // Need at least 5 trades to make a meaningful adjustment
        if recent_trades.len() < 5 {
            return StrategyAdjustment::default();
        }

        let prompt = prompts::build_strategy_prompt(
            recent_trades,
            total_pnl_sol,
            current_tp_pct,
            current_sl_pct,
            current_amount_sol,
            win_rate,
        );

        match self.client.ask_json(prompts::STRATEGY_AGENT_SYSTEM, &prompt).await {
            Ok(json_str) => {
                match serde_json::from_str::<StrategyAdjustment>(&json_str) {
                    Ok(adj) => {
                        // Safety clamp: never let AI suggest extreme values
                        let adj = StrategyAdjustment {
                            take_profit_pct: adj.take_profit_pct.map(|v| v.clamp(5.0, 500.0)),
                            stop_loss_pct: adj.stop_loss_pct.map(|v| v.clamp(2.0, 50.0)),
                            amount_multiplier: adj.amount_multiplier.clamp(0.25, 2.0),
                            ..adj
                        };
                        debug!(
                            regime = %adj.market_regime,
                            tp = ?adj.take_profit_pct,
                            sl = ?adj.stop_loss_pct,
                            multiplier = adj.amount_multiplier,
                            "AI strategy adjustment"
                        );
                        adj
                    }
                    Err(e) => {
                        warn!("Failed to parse AI strategy response: {}", e);
                        StrategyAdjustment::default()
                    }
                }
            }
            Err(e) => {
                warn!("AI strategy agent error: {}", e);
                StrategyAdjustment::default()
            }
        }
    }
}

// ─── Report Agent ─────────────────────────────────────────────────────────────

/// Generates periodic natural language performance reports
pub struct ReportAgent {
    client: AiClient,
}

impl ReportAgent {
    pub fn new(client: AiClient) -> Self {
        Self { client }
    }

    pub async fn generate_report(
        &self,
        trades_attempted: u64,
        trades_confirmed: u64,
        arbs_found: u64,
        arbs_executed: u64,
        total_pnl_sol: f64,
        uptime_secs: u64,
        pools_tracked: u64,
    ) -> MarketReport {
        let prompt = prompts::build_report_prompt(
            trades_attempted,
            trades_confirmed,
            arbs_found,
            arbs_executed,
            total_pnl_sol,
            uptime_secs,
            pools_tracked,
        );

        match self.client.ask_json(prompts::REPORT_AGENT_SYSTEM, &prompt).await {
            Ok(json_str) => {
                match serde_json::from_str::<MarketReport>(&json_str) {
                    Ok(mut report) => {
                        report.timestamp = Utc::now();
                        report
                    }
                    Err(e) => {
                        warn!("Failed to parse AI report: {}", e);
                        Self::fallback_report(total_pnl_sol, trades_confirmed, trades_attempted)
                    }
                }
            }
            Err(e) => {
                warn!("AI report agent error: {}", e);
                Self::fallback_report(total_pnl_sol, trades_confirmed, trades_attempted)
            }
        }
    }

    fn fallback_report(pnl: f64, confirmed: u64, attempted: u64) -> MarketReport {
        let win_rate = if attempted > 0 {
            confirmed as f64 / attempted as f64 * 100.0
        } else {
            0.0
        };
        MarketReport {
            summary: format!(
                "Bot running. {} trades confirmed of {} attempted ({:.1}% win rate).",
                confirmed, attempted, win_rate
            ),
            pnl_commentary: format!("Total PnL: {:.6} SOL", pnl),
            market_conditions: "AI report unavailable.".into(),
            recommendations: vec![],
            alerts: vec!["AI reporting agent offline".into()],
            timestamp: Utc::now(),
        }
    }
}

// ─── Debate Agent ─────────────────────────────────────────────────────────────

/// Simulates a debate between Bull and Bear personas
pub struct DebateAgent {
    client: AiClient,
}

impl DebateAgent {
    pub fn new(client: AiClient) -> Self {
        Self { client }
    }

    pub async fn debate(&self, trade_type: &str, details: &str) -> DebateResult {
        let prompt = prompts::build_debate_prompt(trade_type, details);

        match self.client.ask_json(prompts::DEBATE_AGENT_SYSTEM, &prompt).await {
            Ok(json_str) => {
                match serde_json::from_str::<DebateResult>(&json_str) {
                    Ok(result) => {
                        debug!(
                            consensus = result.consensus_score,
                            recommendation = %result.final_recommendation,
                            "AI trade debate finished"
                        );
                        result
                    }
                    Err(e) => {
                        warn!("Failed to parse AI debate response: {}", e);
                        Self::fallback_result("AI parse failed")
                    }
                }
            }
            Err(e) => {
                warn!("AI debate agent error: {}", e);
                Self::fallback_result(&e.to_string())
            }
        }
    }

    fn fallback_result(reason: &str) -> DebateResult {
        DebateResult {
            bull_opinion: DebateOpinion {
                stance: "neutral".into(),
                reasoning: format!("Debate failed: {}", reason),
                confidence: 50,
            },
            bear_opinion: DebateOpinion {
                stance: "neutral".into(),
                reasoning: format!("Debate failed: {}", reason),
                confidence: 50,
            },
            consensus_score: 50,
            final_recommendation: "skip".into(),
            summary: "AI debate unavailable — defaulting to safety".into(),
        }
    }
}

// ─── Coordinator ─────────────────────────────────────────────────────────────

/// Coordinates all AI agents — single entry point for the rest of the system
pub struct AiCoordinator {
    pub risk: RiskAgent,
    pub arb: ArbAgent,
    pub strategy: StrategyAgent,
    pub report: ReportAgent,
    pub debate: DebateAgent,
}

impl AiCoordinator {
    pub fn new(client: AiClient) -> Self {
        Self {
            risk: RiskAgent::new(client.clone()),
            arb: ArbAgent::new(client.clone()),
            strategy: StrategyAgent::new(client.clone()),
            report: ReportAgent::new(client.clone()),
            debate: DebateAgent::new(client),
        }
    }

    /// Try to create from environment. Returns None if no AI key is configured
    /// (allows the bot to run without AI if no key is set).
    pub fn from_env_optional() -> Option<Self> {
        match AiClient::from_env() {
            Ok(client) => {
                tracing::info!(
                    provider = %client.provider_name(),
                    model = %client.model,
                    "AI agent layer initialized"
                );
                Some(Self::new(client))
            }
            Err(e) => {
                tracing::warn!(
                    "AI layer disabled: {}. Set GROQ_API_KEY for free AI features.",
                    e
                );
                None
            }
        }
    }
}
