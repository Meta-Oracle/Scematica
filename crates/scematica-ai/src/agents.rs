use crate::{
    client::AiClient,
    prompts,
    types::{ArbScore, MarketReport, StrategyAdjustment, TokenRiskScore},
};
use anyhow::Result;
use chrono::Utc;
use tracing::{debug, warn};

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

        match self.client.ask_json(prompts::RISK_AGENT_SYSTEM, &prompt).await {
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
                warn!("AI risk agent error: {}", e);
                Self::default_skip(mint, &e.to_string())
            }
        }
    }

    fn default_skip(mint: &str, reason: &str) -> TokenRiskScore {
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

// ─── Coordinator ─────────────────────────────────────────────────────────────

/// Coordinates all AI agents — single entry point for the rest of the system
pub struct AiCoordinator {
    pub risk: RiskAgent,
    pub arb: ArbAgent,
    pub strategy: StrategyAgent,
    pub report: ReportAgent,
}

impl AiCoordinator {
    pub fn new(client: AiClient) -> Self {
        Self {
            risk: RiskAgent::new(client.clone()),
            arb: ArbAgent::new(client.clone()),
            strategy: StrategyAgent::new(client.clone()),
            report: ReportAgent::new(client),
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
