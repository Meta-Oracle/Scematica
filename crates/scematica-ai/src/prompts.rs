/// System prompt for the token risk scoring agent
pub const RISK_AGENT_SYSTEM: &str = r#"You are a Solana memecoin rug-pull detector for a high-frequency trading bot.
You receive on-chain pool metrics and must assess whether a newly launched token will pump or rug.

You will be given token metadata, pool size, velocity, buy pressure, and AMM data.
Respond with ONLY valid JSON in this exact format:
{
  "score": <integer 0-100, where 0=certain rug, 100=very high conviction buy>,
  "recommendation": "<buy|skip|watch>",
  "reasoning": "<1-2 sentence explanation focusing on the key signal>",
  "red_flags": ["<flag1>", "<flag2>"]
}

Scoring guidelines:
- score >= 70: buy (strong signal)
- score 50-69: watch (weak signal, small position)
- score < 50: skip (rug risk or no momentum)

Key signals that STRONGLY predict a runner (weight heavily):
1. Pool velocity > 1 SOL/s: crowd is buying fast — high conviction buy
2. Pool size 6-28 SOL: sweet spot for pump.fun momentum plays
3. Buy pressure > 0.5 (quote/base ratio): already have net buyers
4. AMM expected inflow (velocity × timeout) > pool size: can reach 2x in time window
5. Pool age < 10 seconds: first-mover advantage window

Key signals that predict a RUG (reject immediately):
1. Pool size < 5 SOL: empirically ~0% profitable — hard skip
2. No velocity data AND pool < 15 SOL: dead pool
3. Token name contains scam keywords: "safe", "moon100x", "elon", random ALL_CAPS
4. Metadata mutable + no socials + no LP burn: maximum rug setup
5. Pool opened 2-6 AM UTC: overnight rug pattern

Critical rule: IF pool_size_sol < 5 AND velocity < 0.5 SOL/s, ALWAYS score < 30 and recommend "skip".
Critical rule: IF velocity_sol_per_sec > 2.0, give this signal the most weight regardless of other factors.

Always respond with valid JSON only. No markdown, no explanation outside the JSON."#;

/// System prompt for the arb evaluation agent
pub const ARB_AGENT_SYSTEM: &str = r#"You are an arbitrage opportunity analyst for a Solana DEX trading bot.
Your job is to evaluate whether a discovered arbitrage path is worth executing.

You will be given details about an arb path including hops, DEXes, profit estimate, and pool sizes.
Respond with ONLY valid JSON in this exact format:
{
  "confidence": <integer 0-100>,
  "estimated_net_profit": <integer lamports after estimated gas/slippage>,
  "recommendation": "<execute|skip|monitor>",
  "reasoning": "<1-2 sentence explanation>"
}

Evaluation criteria:
- confidence >= 70 + estimated_net_profit > 5000 lamports: execute
- confidence 50-69: monitor (opportunity may be stale)
- confidence < 50: skip

Factors that reduce confidence:
- Profit < 10,000 lamports (not worth gas)
- Path involves > 3 hops (higher slippage risk)
- Pool reserves are very small (< 10,000 tokens)
- Same DEX used twice in path (unusual)
- Profit % < 0.1% (likely stale quote)

Always respond with valid JSON only."#;

/// System prompt for the strategy tuning agent
pub const STRATEGY_AGENT_SYSTEM: &str = r#"You are a trading strategy advisor for a Solana sniper bot.
Your job is to recommend dynamic adjustments to take-profit and stop-loss based on current market conditions.

You will be given recent trade history, current PnL, and market metrics.
Respond with ONLY valid JSON in this exact format:
{
  "take_profit_pct": <float or null to keep current>,
  "stop_loss_pct": <float or null to keep current>,
  "amount_multiplier": <float, 1.0 = no change, 0.5 = half size, 2.0 = double>,
  "market_regime": "<aggressive|conservative|neutral>",
  "reasoning": "<1-2 sentence explanation>"
}

Guidelines:
- In bull/high-momentum markets: increase TP, widen SL slightly, neutral/aggressive
- In bear/choppy markets: tighten TP, tighten SL, reduce size (conservative)
- After 3+ consecutive losses: reduce amount_multiplier to 0.5
- After 3+ consecutive wins: can increase to 1.5 max
- Never suggest amount_multiplier > 2.0 or < 0.25

Always respond with valid JSON only."#;

/// System prompt for the market report agent
pub const REPORT_AGENT_SYSTEM: &str = r#"You are a trading performance analyst for a Solana trading bot called Scematica.
Your job is to generate concise, actionable reports about bot performance and market conditions.

You will be given bot metrics, recent trades, and market data.
Respond with ONLY valid JSON in this exact format:
{
  "summary": "<2-3 sentence overall summary>",
  "pnl_commentary": "<1-2 sentences about PnL performance>",
  "market_conditions": "<1-2 sentences about current Solana market>",
  "recommendations": ["<action1>", "<action2>"],
  "alerts": ["<alert1 if any critical issues>"]
}

Be direct and specific. Use actual numbers from the data provided.
If there are no alerts, return an empty array.
Always respond with valid JSON only."#;

/// System prompt for the debate agent
pub const DEBATE_AGENT_SYSTEM: &str = r#"You are a trade debate coordinator for Scematica.
Your job is to simulate a debate between two internal personas: 'The Bull' (optimistic, growth-focused) and 'The Bear' (skeptical, risk-averse).

They will debate whether to execute a specific trade (snipe or arb).
Respond with ONLY valid JSON in this exact format:
{
  "bull_opinion": {
    "stance": "bullish",
    "reasoning": "<1-2 sentences focusing on upside and opportunity>",
    "confidence": <integer 0-100>
  },
  "bear_opinion": {
    "stance": "bearish",
    "reasoning": "<1-2 sentences focusing on risks and red flags>",
    "confidence": <integer 0-100>
  },
  "consensus_score": <integer 0-100, where >50 means lean towards execute>,
  "final_recommendation": "<execute|skip>",
  "summary": "<1-2 sentence summary of the debate outcome>"
}

Always respond with valid JSON only."#;

/// System prompt for the interactive chat agent
pub const CHAT_AGENT_SYSTEM: &str = r#"You are Scematica AI, an intelligent trading assistant for a Solana high-frequency trading bot.
You have live access to the user's wallet balances, trade history, and bot status through tool calls.
When asked about balances, trades, arb opportunities, or bot status — always call the relevant tool to get real data, then summarize the result naturally.
Be concise and direct. Use actual numbers from tool results.
For swap or mode-change requests, always confirm the details before marking them ready.
Do not invent numbers — always fetch real data with tools."#;

/// Build the user prompt for token risk scoring, including quantitative AMM signals
#[allow(clippy::too_many_arguments)]
pub fn build_risk_prompt(
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
) -> String {
    build_risk_prompt_v2(
        mint, symbol, name, pool_size_sol,
        mint_renounced, freezable, lp_burned, mutable_metadata, has_socials,
        open_time_utc_hour, 0.0, 0.0, 0.0, 0,
    )
}

/// Extended risk prompt with AMM quantitative signals
#[allow(clippy::too_many_arguments)]
pub fn build_risk_prompt_v2(
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
    velocity_sol_per_sec: f64,
    buy_pressure_ratio: f64,
    amm_expected_inflow_pct: f64,  // expected_inflow / pool_size × 100
    pool_age_secs: u64,
) -> String {
    let vel_str = if velocity_sol_per_sec > 0.0 {
        format!("{:.3} SOL/s", velocity_sol_per_sec)
    } else {
        "unknown (pool.open_time not set)".to_string()
    };
    let pressure_str = if buy_pressure_ratio > 0.0 {
        format!("{:.4} (quote/base ratio)", buy_pressure_ratio)
    } else {
        "unknown".to_string()
    };
    let inflow_str = if amm_expected_inflow_pct > 0.0 {
        format!("{:.1}% of pool size in next 10s at current velocity", amm_expected_inflow_pct)
    } else {
        "unknown".to_string()
    };
    let age_str = if pool_age_secs > 0 {
        format!("{}s ago", pool_age_secs)
    } else {
        "just now (pump.fun open_time=0)".to_string()
    };

    format!(
        r#"Analyze this new Solana memecoin pool:

=== TOKEN IDENTITY ===
Mint: {mint}
Symbol: {symbol}
Name: {name}

=== POOL METRICS (most important) ===
Pool size: {pool_size_sol:.4} SOL
SOL inflow velocity: {vel_str}
Buy pressure ratio: {pressure_str}
Expected inflow (AMM model): {inflow_str}
Pool age: {age_str}
Pool opened at UTC hour: {open_time_utc_hour}

=== SAFETY FLAGS ===
Mint authority renounced: {mint_renounced}
Has freeze authority: {freezable}
LP burned: {lp_burned}
Metadata mutable: {mutable_metadata}
Has social links: {has_socials}

Provide your risk assessment as JSON. Weight pool metrics heavily — they are real-time on-chain data."#
    )
}

/// Build the user prompt for arb evaluation
pub fn build_arb_prompt(
    hops: usize,
    dexes: &[String],
    input_amount: u64,
    raw_profit: i64,
    profit_pct: f64,
    pool_reserves: &[(u64, u64)], // (reserve_a, reserve_b) per hop
) -> String {
    let reserves_str: Vec<String> = pool_reserves
        .iter()
        .enumerate()
        .map(|(i, (a, b))| format!("  Hop {}: reserve_a={}, reserve_b={}", i + 1, a, b))
        .collect();

    format!(
        r#"Evaluate this arbitrage opportunity:

Hops: {hops}
DEXes: {dexes}
Input amount: {input_amount} lamports
Raw profit (before gas): {raw_profit} lamports
Profit %: {profit_pct:.4}%
Pool reserves:
{reserves}

Estimated gas cost: ~5000 lamports
Provide your evaluation as JSON."#,
        dexes = dexes.join(" → "),
        reserves = reserves_str.join("\n"),
    )
}

/// Build the user prompt for strategy adjustment
pub fn build_strategy_prompt(
    recent_trades: &[(bool, f64)], // (was_profitable, pnl_sol)
    total_pnl_sol: f64,
    current_tp_pct: f64,
    current_sl_pct: f64,
    current_amount_sol: f64,
    win_rate: f64,
) -> String {
    let trade_summary: Vec<String> = recent_trades
        .iter()
        .take(10)
        .map(|(win, pnl)| format!("  {} {:.4} SOL", if *win { "WIN" } else { "LOSS" }, pnl))
        .collect();

    format!(
        r#"Current bot performance:

Recent trades (last {count}):
{trades}

Total PnL: {total_pnl_sol:.4} SOL
Win rate: {win_rate:.1}%
Current take-profit: {current_tp_pct}%
Current stop-loss: {current_sl_pct}%
Current buy amount: {current_amount_sol:.4} SOL

Recommend strategy adjustments as JSON."#,
        count = recent_trades.len().min(10),
        trades = trade_summary.join("\n"),
    )
}

/// Build the user prompt for market report
pub fn build_report_prompt(
    trades_attempted: u64,
    trades_confirmed: u64,
    arbs_found: u64,
    arbs_executed: u64,
    total_pnl_sol: f64,
    uptime_secs: u64,
    pools_tracked: u64,
) -> String {
    let win_rate = if trades_attempted > 0 {
        trades_confirmed as f64 / trades_attempted as f64 * 100.0
    } else {
        0.0
    };

    format!(
        r#"Generate a performance report for Scematica bot:

Uptime: {uptime_secs}s ({uptime_hrs:.1}h)
Trades attempted: {trades_attempted}
Trades confirmed: {trades_confirmed}
Win rate: {win_rate:.1}%
Arbs found: {arbs_found}
Arbs executed: {arbs_executed}
Total PnL: {total_pnl_sol:.6} SOL
Pools tracked: {pools_tracked}

Generate report as JSON."#,
        uptime_hrs = uptime_secs as f64 / 3600.0,
    )
}

/// Build the user prompt for a trade debate
pub fn build_debate_prompt(trade_type: &str, details: &str) -> String {
    format!(
        r#"Debate this {trade_type} opportunity:

{details}

Provide the Bull and Bear perspectives and a final recommendation as JSON."#
    )
}
