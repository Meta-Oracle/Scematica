use serde::{Deserialize, Serialize};
use std::path::Path;
use anyhow::Result;

/// Rate mode profile: defines entry size, TP%, SL% for a specific risk posture
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateMode {
    /// Human-readable name: "Micro", "Bearish", "Safe", "Balanced", "Aggressive", "Degen", "Moon"
    pub name: String,
    /// Menu order (1–7)
    pub order: u32,
    /// Entry amount in quote token (e.g. 0.01 SOL)
    pub quote_amount: f64,
    /// Take profit % for this mode
    pub take_profit_pct: f64,
    /// Stop loss % for this mode
    pub stop_loss_pct: f64,
    /// Max TP escalations allowed in momentum mode
    pub momentum_max_escalations: u32,
    /// Is this mode enabled?
    pub enabled: bool,
    /// Wallet % to use per trade (0.0 = use quote_amount instead)
    #[serde(default)]
    pub wallet_pct: f64,
}

/// Top-level bot configuration loaded from .env / config file
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BotConfig {
    pub rpc: RpcConfig,
    pub wallet: WalletConfig,
    pub sniper: SniperConfig,
    pub arb: ArbConfig,
    pub execution: ExecutionConfig,
    #[serde(default)]
    pub alerts: AlertsConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcConfig {
    /// HTTPS RPC endpoint
    pub endpoint: String,
    /// WebSocket RPC endpoint
    pub ws_endpoint: String,
    /// Commitment level: "processed" | "confirmed" | "finalized"
    pub commitment: String,
}

impl Default for RpcConfig {
    fn default() -> Self {
        Self {
            endpoint: "https://api.mainnet-beta.solana.com".into(),
            ws_endpoint: "wss://api.mainnet-beta.solana.com".into(),
            commitment: "confirmed".into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WalletConfig {
    /// Path to keypair JSON file, or base58-encoded private key
    pub keypair_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SniperConfig {
    pub enabled: bool,
    /// Quote token: "WSOL" or "USDC"
    pub quote_mint: String,
    /// Amount to spend per snipe (in quote token UI units, e.g. 0.1 SOL)
    pub quote_amount: f64,
    /// Delay in ms before buying after pool detection
    pub buy_delay_ms: u64,
    /// Max buy retries
    pub max_buy_retries: u32,
    /// Max sell retries
    pub max_sell_retries: u32,
    /// Auto-sell after buy
    pub auto_sell: bool,
    /// Delay in ms before auto-selling
    pub auto_sell_delay_ms: u64,
    /// Take profit % (e.g. 50.0 = 50%)
    pub take_profit_pct: f64,
    /// Stop loss % (e.g. 20.0 = 20%)
    pub stop_loss_pct: f64,
    /// Buy slippage %
    pub buy_slippage_pct: f64,
    /// Sell slippage %
    pub sell_slippage_pct: f64,
    /// Price check interval ms
    pub price_check_interval_ms: u64,
    /// Price check duration ms (how long to monitor before force-sell)
    pub price_check_duration_ms: u64,
    /// Only process one token at a time
    pub one_token_at_a_time: bool,
    /// Use snipe list (whitelist mode)
    pub use_snipe_list: bool,
    /// Path to snipe list file
    pub snipe_list_path: String,
    /// Auto-activate sell mode after this many successful buys (0 = unlimited)
    pub max_buys: u32,
    /// Trailing stop loss % from peak price (0 = disabled, uses fixed stop_loss_pct instead)
    pub trailing_stop_loss_pct: f64,
    /// Sell this % of position at partial_tp_trigger (0 = disabled)
    pub partial_tp_pct: f64,
    /// Price gain % that triggers partial take profit
    pub partial_tp_trigger: f64,
    /// Max open positions at once (0 = unlimited)
    pub max_concurrent_positions: u32,
    /// Activate cooldown after N consecutive losses (0 = disabled)
    pub cooldown_after_losses: u32,
    /// Cooldown duration in minutes
    pub cooldown_minutes: u32,
    /// Halt buying if daily losses exceed this SOL amount (0.0 = disabled)
    pub daily_loss_limit_sol: f64,
    /// Activate sell mode if wallet drops below this % of session starting balance (0.0 = disabled)
    pub max_drawdown_pct: f64,
    /// Path to dev wallet blacklist file (one pubkey per line)
    pub blacklist_path: String,
    /// Wallet addresses to copy-trade (buy same tokens they buy)
    pub copy_wallets: Vec<String>,
    pub filters: FilterConfig,

    // ── Rate Modes: 7 trading profiles with individual entry sizes and TP/SL ──
    /// 7 rate mode profiles: Micro, Bearish, Safe, Balanced, Aggressive, Degen, Moon
    pub rate_modes: Vec<RateMode>,
    /// Currently active rate mode name (e.g., "Balanced")
    pub active_mode_name: String,

    // ── Kelly position sizing ─────────────────────────────────────────────────
    /// Enable Kelly Criterion position sizing
    pub kelly_sizing: bool,
    /// Fraction of Kelly formula to apply (0.25 = quarter-Kelly)
    pub kelly_fraction: f64,
    /// Number of recent trades to use for Kelly estimate
    pub kelly_lookback: usize,

    // ── Gas war mode ─────────────────────────────────────────────────────────
    /// Escalate compute unit price when multiple pools are detected rapidly
    pub gas_war_mode: bool,
    /// Maximum compute unit price in micro-lamports during gas wars
    pub gas_war_max_cu_price: u64,

    // ── ATH drawdown watermark ────────────────────────────────────────────────
    /// Pause buying if wallet drops this % below all-time-high balance (0.0 = disabled)
    pub ath_drawdown_pct: f64,

    // ── Grief-loss circuit breaker ────────────────────────────────────────────
    /// Sliding window size in seconds for grief-loss calculation
    pub grief_loss_window_secs: u64,
    /// Halt buying if losses in the window exceed this SOL amount (0.0 = disabled)
    pub grief_loss_limit_sol: f64,

    // ── Time-of-day weighting ─────────────────────────────────────────────────
    /// Scale position size based on UTC trading hour activity
    pub time_of_day_weighting: bool,

    // ── Profit extraction ─────────────────────────────────────────────────────
    /// Automatically extract profits when session PnL exceeds this SOL (0.0 = disabled)
    pub profit_extraction_threshold_sol: f64,
    /// Percentage of profit to extract on each sweep (0.0 = disabled)
    pub profit_extraction_pct: f64,
    /// Cold wallet address to receive extracted profits
    pub profit_extraction_wallet: String,

    // ── Pool predictive scoring ────────────────────────────────────────────────
    /// Minimum pool score (0–100) required to proceed with a buy (0.0 = disabled)
    pub min_pool_score: f64,

    // ── Multi-RPC endpoints ────────────────────────────────────────────────────
    /// Additional RPC endpoints for automatic failover
    pub extra_rpc_endpoints: Vec<String>,

    // ── Adaptive slippage ─────────────────────────────────────────────────────
    /// Automatically adjust slippage based on recent sell success rates
    pub adaptive_slippage: bool,

    // ── Sandwich shield ───────────────────────────────────────────────────────
    /// Switch to Jito bundle routing when front-running is detected
    pub sandwich_shield: bool,

    // ── Momentum-aware long-term sniping ──────────────────────────────────────
    /// Enable momentum-driven TP escalation + pullback-from-peak exit. When ON,
    /// the sell monitor tracks the velocity of recent price checks: positions
    /// that hit their TP target with strong upward momentum get their TP raised
    /// instead of exiting, and positions in significant profit exit on a sharp
    /// pullback from peak rather than waiting for the SL trigger. This biases
    /// the bot toward "let winners run" — small wins get bumped into big ones
    /// when the market cooperates. Default ON.
    pub momentum_hold: bool,
    /// Number of recent price-check deltas averaged into the velocity signal.
    /// Default 5 — small enough that recent momentum dominates, large enough
    /// that single jittery checks don't flip the decision.
    pub momentum_window_checks: u32,
    /// Minimum average velocity (% of entry per check) required to trigger a TP
    /// escalation. Higher = more conservative (only the strongest movers ride).
    /// Default 5.0 — works out to roughly +5 % per ~250 ms tick during a real pump.
    pub momentum_escalation_threshold_pct: f64,
    /// Multiplier applied to the current TP target each time momentum escalates.
    /// 1.5 means TP grows by 50 % per escalation: 100 % → 150 % → 225 % → 337 %.
    /// Default 1.5.
    pub momentum_escalation_factor: f64,
    /// Maximum number of TP escalations allowed for a single position — caps the
    /// "let it ride" greed so the bot still books eventually. Default 4 (= TP
    /// can grow up to 1.5^4 = 5.06× the configured target).
    pub momentum_max_escalations: u32,
    /// Position only exits via pullback-from-peak when realised peak gain has
    /// exceeded this percentage of entry. Below this, normal TP/SL logic runs.
    /// Default 20.0 — avoids triggering on noise before the trade has run.
    pub momentum_min_peak_pct: f64,
    /// Pullback from peak that fires the lock-in exit. 15 % = "you were up 200 %,
    /// now you're up 170 %, get out before this round-trips." Default 15.0.
    pub momentum_pullback_exit_pct: f64,

    // ── Perfect-exit timing (v0.9.6) ──────────────────────────────────────────
    /// Adaptive pullback threshold scales with peak height — bigger winners
    /// get more room to breathe before the pullback exit fires. Formula:
    ///   pullback_θ(peak) = base × sqrt(1 + peak/100)
    /// At peak=20% → 1.10×.  At peak=100% → 1.41×.  At peak=500% → 2.45×.
    /// Default ON. Disable to use the flat threshold for all positions.
    pub adaptive_pullback: bool,

    /// Velocity-decay exit catches the *inflection point* — the moment momentum
    /// starts dying — before price actually reverses. Compares the average
    /// velocity over the most recent N checks vs the previous N checks; if the
    /// signal has flipped from positive-accelerating to positive-decelerating
    /// AND we're already in profit, exit. This is the "perfect exit" trigger.
    /// Default ON.
    pub velocity_decay_exit: bool,
    /// Number of checks per half-window for the decay comparison. Total memory
    /// = 2N. Default 3 (compares last 3 vs previous 3 = 6 checks of history,
    /// ~1.5 s of price action at 250 ms polling).
    pub velocity_decay_window: u32,
    /// Velocity decay only fires when current pnl is above this %. Avoids
    /// exiting on noise during the early "filling out" phase of a position.
    /// Default 10.0 — by the time we're up 10 %, momentum signal is meaningful.
    pub velocity_decay_min_pnl_pct: f64,
    /// Minimum velocity-drop magnitude (in % of entry per check) required to
    /// trigger the decay exit. Filters out micro-flutters. Default 2.0 = the
    /// velocity must have fallen by at least 2 percentage points across the
    /// window for the exit to fire.
    pub velocity_decay_drop_threshold: f64,

    /// Tiered partial-TP ladder: instead of one partial sell at
    /// `partial_tp_trigger`, sell incrementally at multiple gain levels. Locks
    /// the median win automatically while leaving capital for the escalator.
    /// Default ON; disable to fall back to the single partial_tp_pct.
    pub tiered_partial_tp: bool,
    /// Ladder of (trigger_pct, sell_pct) tuples. Sells `sell_pct` of REMAINING
    /// position when current_pnl_pct first crosses `trigger_pct`. Default:
    ///   +30 % → sell 25 %    (capture median win)
    ///   +75 % → sell 25 % more   (lock first multiple)
    ///   +150 % → sell 25 % more  (lock 2x return)
    ///   remainder rides to escalator / pullback exit
    pub tiered_partial_tp_levels: Vec<(f64, f64)>,

    // ── Profit-first growth mode ──────────────────────────────────────────────
    /// While the wallet is below `wallet_target_sol`, refuse stop-loss exits
    /// (using `profit_first_floor_pct` as a rug-only safety net) and only
    /// close positions in profit. Once the wallet reaches the target, normal
    /// SL behavior resumes. This is the "establish profits first" doctrine —
    /// it accepts longer drawdowns in individual positions to bias toward
    /// realising wins before tolerating any losses.
    pub profit_first_mode: bool,
    /// Wallet size in SOL we're trying to build to. Below this, profit-first
    /// is active; at/above, normal SL applies. Default 0.2 SOL.
    pub wallet_target_sol: f64,
    /// Rug-only safety net in profit-first mode: still exit if value drops
    /// this far below entry, even though regular SL is gated. Default 50%.
    pub profit_first_floor_pct: f64,

    // ── Flash-crash protection ────────────────────────────────────────────────
    /// If the position value drops more than this % from its peak in a SINGLE
    /// price-check interval (≥75 ms), exit immediately without waiting for the
    /// 3-consecutive-decline counter. Catches vertical dumps that the streak
    /// detector would miss during the slow normal-interval phase.
    /// Default 22.0.  Set 0.0 to disable.
    pub flash_crash_pct: f64,

    // ── Profit lock ───────────────────────────────────────────────────────────
    /// After the position stays above breakeven for this many CONSECUTIVE price
    /// checks, raise the stop-loss floor to near-breakeven (entry × 0.98) to
    /// ensure a winning position never turns into a significant loss. Activates
    /// independently of partial-TP — useful for slow steady movers that never
    /// hit the first tier quickly but stay green for many checks. Default 8.
    pub profit_lock_checks: u32,

    // ── ATA cleanup ───────────────────────────────────────────────────────────
    /// Close the base-token ATA after a full sell to reclaim the 0.002 SOL rent.
    /// Sent as a separate fire-and-forget transaction after the sell confirms.
    /// Default true.
    pub close_ata_on_sell: bool,

    // ── Hard position time cap ────────────────────────────────────────────────
    /// Force-sell any position that has been open longer than this many minutes,
    /// regardless of profit-first extension logic. Prevents capital from being
    /// permanently locked in a dead pool. 0 = disabled. Default 90.
    pub max_position_hold_mins: u32,

    // ── Absolute SOL floor ────────────────────────────────────────────────────
    /// Never enter a trade if the wallet's SOL balance would drop below this
    /// after the buy. Reserves gas money so sells never fail due to empty wallet.
    /// Default 0.02 SOL.
    pub min_sol_reserve: f64,

    // ── Confirmation window ───────────────────────────────────────────────────
    /// After detecting a pool, wait this many ms and re-check vault balance.
    /// If price has already moved >15% from creation price, skip the buy — we
    /// are entering after the initial pump. 0 = disabled. Default 0.
    pub confirmation_window_ms: u64,

    // ── Session heat cooldown ─────────────────────────────────────────────────
    /// Pause buying for `session_heat_cooldown_mins` after this many losses in
    /// `session_heat_window_secs`. Distinct from grief_breaker (which is based on
    /// SOL magnitude); this fires on *frequency*. 0 = disabled. Default 3.
    pub session_heat_losses: u32,
    /// Rolling window in seconds for session heat tracking. Default 3600.
    pub session_heat_window_secs: u64,
    /// Pause duration in minutes when session heat is tripped. Default 15.
    pub session_heat_cooldown_mins: u32,

    // ── Volume exhaustion exit ────────────────────────────────────────────────
    /// Exit the position when observed volume drops to this % of the entry-time
    /// volume. The pump is over when volume dries up. 0.0 = disabled.
    pub volume_exhaustion_pct: f64,

    // ── Buy/sell ratio inversion exit ─────────────────────────────────────────
    /// Exit immediately when the buy/sell ratio drops below this threshold after
    /// having been above it at entry. Net sellers dominating = distribution phase.
    /// 0.0 = disabled. Default 0.0.
    pub buy_sell_ratio_exit: f64,

    // ── Check interval acceleration ───────────────────────────────────────────
    /// When 3 consecutive price checks are all down, halve the check interval
    /// (minimum 25 ms). Re-expands to normal when price stabilises. Default true.
    pub check_interval_acceleration: bool,

    // ── Whale exit detector ───────────────────────────────────────────────────
    /// If the pool's quote vault drops by more than this % in a single price-check
    /// cycle, trigger an immediate sell — a large holder exited. 0.0 = disabled.
    pub whale_exit_vault_drop_pct: f64,

    // ── Pool-quality buy sizing ───────────────────────────────────────────────
    /// Scale buy size by pool_score/100. Score 95 → full size, score 55 → 55%.
    /// Requires min_pool_score > 0 to be meaningful. Default false.
    pub pool_quality_sizing: bool,

    // ── Kelly warm-up ─────────────────────────────────────────────────────────
    /// Minimum number of historical trades before Kelly sizing activates. Below
    /// this the multiplier is 0.5× (half base) to avoid wild early sizing.
    /// Default 10.
    pub kelly_min_trades: usize,

    // ── Deployer wallet age filter ────────────────────────────────────────────
    /// Reject pools from deployer wallets younger than this many hours.
    /// Fresh wallets almost always belong to rug setups. 0 = disabled.
    pub deployer_wallet_age_min_hours: u64,

    // ── Filter TTL cache ──────────────────────────────────────────────────────
    /// Cache filter pipeline results per pool pubkey for this many seconds to
    /// avoid repeated RPC calls when the same pool fires multiple events.
    /// Default 30.
    pub filter_cache_ttl_secs: u64,

    // ── Dead-zone early exit ──────────────────────────────────────────────────
    /// Exit a position if no meaningful upward momentum has been observed within
    /// this many seconds of entry. Live data: winning pump.fun trades exit within
    /// 6 s; positions still flat after 45 s are statistically dead capital.
    /// 0 = disabled. Default 45.
    pub no_pump_timeout_secs: u64,
    /// Minimum peak gain % that must have been seen to suppress the dead-zone
    /// exit. If the position's best price ever seen is below this threshold after
    /// `no_pump_timeout_secs`, exit at market to recycle capital.
    /// Default 3.0.
    pub no_pump_min_gain_pct: f64,

    // ── Dump-mode fresh-position protection ──────────────────────────────────
    /// If dump_mode fires but NOT sell_mode, protect positions younger than this
    /// many seconds — let them run through normal TP/SL instead of force-selling
    /// at min_out=0. Prevents dump mode from destroying a freshly-entered position
    /// mid-pump. 0 = no protection (dump all immediately). Default 0.
    pub min_dump_hold_secs: u64,

    // ── Mint re-entry cooldown ────────────────────────────────────────────────
    /// After buying a mint, skip it for this many seconds on any subsequent pool
    /// event. Persisted to `scematica-mint-cooldown.json` so it survives restarts.
    /// Live data: same losing pools were re-entered 3× in 26 min after restarts.
    /// Default 1800 (30 min).
    pub mint_cooldown_secs: u64,

    // ── AI chain pool selection ────────────────────────────────────────────────
    /// 3-layer LLM chain for pool screening (Groq → OpenRouter → Cerebras).
    /// When enabled, pools must pass all 3 layers to proceed to buy.
    /// Falls back to pool_scorer if any API key is missing or times out.
    #[serde(default)]
    pub ai_chain: AiChainConfig,

    // ── Peak stagnation exit ──────────────────────────────────────────────────
    /// Exit if peak gain hasn't improved in this many seconds AND current pnl
    /// exceeds `peak_stagnation_min_pnl_pct`. Catches flat pools that pumped once
    /// then stopped — they bleed slowly back while capital sits idle.
    /// 0 = disabled. Default 90.
    pub peak_stagnation_secs: u64,
    /// Minimum current pnl % required to arm the stagnation exit.
    /// Prevents exiting breakeven positions too early during normal chop.
    /// Default 20.0.
    pub peak_stagnation_min_pnl_pct: f64,

    // ── Runner detection ──────────────────────────────────────────────────────
    /// When true, pools scoring ≥ 98 (ultra-fresh ≤7 s AND sweet-spot 6.5–28 SOL)
    /// get a position sized at `runner_scale_in_sol` instead of `quote_amount`.
    /// Score=98 is the highest-conviction pool profile in live data — the bot should
    /// bet more on these than on borderline-qualifying pools (score 95–97). Default false.
    pub runner_mode: bool,
    /// Buy size in SOL for max-conviction (score ≥ 98) pools when runner_mode is on.
    /// Typically 2–4× quote_amount. Default 0.02.
    pub runner_scale_in_sol: f64,
    /// If the pool's quote vault falls below this % of its balance at position entry,
    /// exit immediately — the pool is actively draining (rug in progress).
    /// 0.0 = disabled. Default 15.0 (pool retains < 15 % of entry liquidity = drain).
    pub pool_drain_exit_pct: f64,
}

impl Default for SniperConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            quote_mint: "WSOL".into(),
            quote_amount: 0.1,
            buy_delay_ms: 0,
            max_buy_retries: 3,
            // 3 inner retries × 4 outer rounds = 12 total tx attempts per position.
            // Was 5 (= 20 total, up to 30s per round on timeouts).
            max_sell_retries: 3,
            auto_sell: true,
            auto_sell_delay_ms: 0,
            // v1.4.0: raised 80→175 — at 80% the TP fired on the FIRST price check
            // (~75ms) before the 5-sample momentum window filled, so the escalation
            // system never ran. At 175% the base TP is a fallback; the pullback and
            // velocity-decay exits are the primary signals. Data: most wins at 99-198%
            // were first-check exits, not momentum-held exits. Higher base = escalation
            // runs = bot rides parabolic moves instead of capping at initial pump.
            take_profit_pct: 175.0,
            // v1.4.0: widened 15→18 — profit_first_floor (25%) already handles rugs;
            // the SL here catches legitimate reversals without stopping out on the
            // normal initial volatility of new pools.
            stop_loss_pct: 18.0,
            // v1.1.1: widened 1.5→3.0 — thin memecoin pools move 3-5% between pool
            // detection and tx landing. 1.5% caused frequent buy rejections with 0x26
            // errors on the first attempt, burning all 3 retries on pools that were
            // still buyable. 3.0% accepts the same buy in one shot.
            buy_slippage_pct: 3.0,
            // v1.0.0: widened 2.0→2.5 — memecoin pools are thin; a 2% slippage
            // floor causes frequent sell rejections during high-volatility moves.
            sell_slippage_pct: 2.5,
            // Tighter price polling — the sell monitor now fans out the 3 balance reads
            // concurrently, so 250 ms is achievable without overloading a paid RPC.
            price_check_interval_ms: 250,
            // v0.9.0: extended from 120s → 900s (15 min) so the momentum-hold
            // escalator has room to let strong winners run. The profit-first hold
            // extension can stretch this up to 6× if the position recovers from a
            // shallow loss.
            price_check_duration_ms: 900_000,
            one_token_at_a_time: true,
            use_snipe_list: false,
            snipe_list_path: "snipe-list.txt".into(),
            max_buys: 0,
            // v1.4.0: kept at 12 — trailing stop is the backstop for tokens that
            // keep climbing past the pullback threshold. Tightening below 12 would
            // conflict with the adaptive pullback formula at modest peak heights.
            trailing_stop_loss_pct: 12.0,
            partial_tp_pct: 50.0,
            // Legacy single partial — unused when tiered_partial_tp is on.
            partial_tp_trigger: 100.0,
            max_concurrent_positions: 0,
            cooldown_after_losses: 3,
            cooldown_minutes: 15,
            daily_loss_limit_sol: 0.0,
            max_drawdown_pct: 0.0,
            blacklist_path: "blacklist.txt".into(),
            copy_wallets: vec![],
            filters: FilterConfig::default(),
            // v1.6.0: Rate modes — 7 profiles from Micro (0.001 SOL) to Moon (0.1 SOL)
            // Default to Balanced (0.01 SOL, 40% win rate from Phase 1 analysis)
            rate_modes: vec![
                RateMode { name: "Micro".into(),      order: 1, quote_amount: 0.001, wallet_pct: 0.3,  take_profit_pct: 50.0,     stop_loss_pct: 8.0,  momentum_max_escalations: 3,  enabled: true },
                RateMode { name: "Bearish".into(),    order: 2, quote_amount: 0.003, wallet_pct: 0.5,  take_profit_pct: 75.0,     stop_loss_pct: 10.0, momentum_max_escalations: 4,  enabled: true },
                RateMode { name: "Safe".into(),       order: 3, quote_amount: 0.005, wallet_pct: 0.8,  take_profit_pct: 100.0,    stop_loss_pct: 12.0, momentum_max_escalations: 5,  enabled: true },
                RateMode { name: "Balanced".into(),   order: 4, quote_amount: 0.01,  wallet_pct: 1.5,  take_profit_pct: 175.0,    stop_loss_pct: 12.0, momentum_max_escalations: 7,  enabled: true },
                RateMode { name: "Aggressive".into(), order: 5, quote_amount: 0.02,  wallet_pct: 3.0,  take_profit_pct: 300.0,    stop_loss_pct: 15.0, momentum_max_escalations: 7,  enabled: true },
                RateMode { name: "Degen".into(),      order: 6, quote_amount: 0.04,  wallet_pct: 6.0,  take_profit_pct: 450.0,    stop_loss_pct: 25.0, momentum_max_escalations: 7,  enabled: true },
                RateMode { name: "Moon".into(),       order: 7, quote_amount: 0.1,   wallet_pct: 12.0, take_profit_pct: 100000.0, stop_loss_pct: 60.0, momentum_max_escalations: 12, enabled: true },
            ],
            active_mode_name: "Balanced".to_string(),
            kelly_sizing: false,
            kelly_fraction: 0.25,
            kelly_lookback: 20,
            gas_war_mode: false,
            gas_war_max_cu_price: 2_000_000,
            ath_drawdown_pct: 0.0,
            grief_loss_window_secs: 300,
            grief_loss_limit_sol: 0.0,
            time_of_day_weighting: false,
            profit_extraction_threshold_sol: 0.0,
            profit_extraction_pct: 0.0,
            profit_extraction_wallet: String::new(),
            // v1.2.0: raised 25→35. The 2 SOL min_pool_size and check_name filter
            // already eliminated the worst pools; raising the score gate here
            // focuses on pools with both fresh age AND sufficient liquidity.
            // Moderate: operators willing to take more micro-cap risk can lower
            // to 25 in config.toml.
            min_pool_score: 35.0,
            extra_rpc_endpoints: vec![],
            adaptive_slippage: false,
            sandwich_shield: false,
            // Profit-first defaults: ON, build toward 0.15 SOL, rug-only floor at -25%.
            // Operators who want classic SL behavior can set `profit_first_mode = false`.
            // v1.3.0: lowered target 0.2→0.15 so the mode disengages faster and
            // normal SL resumes sooner, protecting the accumulated gains.
            // v1.3.1: tightened floor 50→25% — exit rugs faster instead of holding
            // a near-zero token hoping for a recovery that statistically won't come.
            profit_first_mode: true,
            wallet_target_sol: 0.15,
            profit_first_floor_pct: 25.0,
            // Momentum-aware long-term sniping: ON by default. The escalation +
            // pullback exit replaces fixed-TP greed with "ride strong winners,
            // lock when they cool". See the field comments above for tuning.
            momentum_hold: true,
            momentum_window_checks: 5,
            // v1.4.0: lowered 5→3%/check — easier to trigger escalation so the
            // bot rides moderate momentum, not just parabolic outliers.
            momentum_escalation_threshold_pct: 3.0,
            // v1.4.0: raised 1.6→1.8 — more aggressive TP target per escalation.
            // Ladder: 175 → 315 → 567 → 1020 → 1836 → 3305 → 5949% (7 rounds max).
            momentum_escalation_factor: 1.8,
            // v1.4.0: raised 5→7 rounds — lets truly parabolic movers run further
            // without a forced TP exit; the pullback exit locks gains on reversal.
            momentum_max_escalations: 7,
            // v1.4.0: raised 25→60 — require a real peak before the pullback exit
            // fires. At 25% the pullback was triggering on normal early-position
            // volatility before the position had time to develop into a winner.
            momentum_min_peak_pct: 60.0,
            // v1.4.0: tightened 18→8 (base). Despite the lower base, the adaptive
            // formula θ_eff = 8 × √(1 + peak/100) scales up with peak height:
            //   peak=60%  → 10.1 PnL pts → exits at 49.9%  (was 20.1 → exits at 4.9%)
            //   peak=100% → 11.3 PnL pts → exits at 88.7%  (was 25.5 → exits at 74.5%)
            //   peak=200% → 13.9 PnL pts → exits at 186.1% (was 31.2 → exits at 168.8%)
            //   peak=500% → 19.6 PnL pts → exits at 480.4% (was 49.2 → exits at 450.8%)
            // Net effect: locks in gains at HIGHER levels than before on every peak size.
            momentum_pullback_exit_pct: 8.0,
            // v0.9.6 perfect-exit defaults
            adaptive_pullback: true,
            velocity_decay_exit: true,
            velocity_decay_window: 3,
            // v1.4.0: raised 7→25 — at 7% the velocity-decay exit was triggering
            // on tokens that barely covered fees. Requires a genuine 25% gain before
            // the momentum-inflection signal can fire an exit.
            velocity_decay_min_pnl_pct: 25.0,
            // v1.4.0: loosened 1.2→1.5 — 1.2 was too sensitive on thin pools where
            // normal price chop looks like velocity decay. 1.5 requires a sharper
            // inflection before exiting.
            velocity_decay_drop_threshold: 1.5,
            tiered_partial_tp: true,
            // v1.4.0: All levels shifted up; fractions reduced to stay invested longer.
            // Old: (45%,20%) (100%,25%) (200%,25%) — first partial too early at 45%.
            // Data shows tokens commonly pump 100-300% on launch; selling 20% at 45%
            // was giving up upside on every strong winner.
            tiered_partial_tp_levels: vec![
                (100.0, 15.0),  // first partial at +100% — sell 15% (was 45%→20%)
                (300.0, 20.0),  // second at +300% — sell 20% (was 100%→25%)
                (600.0, 25.0),  // third at +600% — sell 25% (was 200%→25%)
            ],
            // v1.0.0 reliability defaults
            flash_crash_pct: 22.0,
            // v1.2.0: lowered 8→6 — lock profits sooner once we're consistently
            // above entry (prevents round-trip losses on sustained winners)
            profit_lock_checks: 6,
            close_ata_on_sell: true,
            // v1.3.0: lowered 90→60 — free capital faster; profit-first extension
            // logic still holds genuinely recovering positions beyond this limit.
            max_position_hold_mins: 60,
            // v1.1.0 new risk + sizing defaults
            min_sol_reserve: 0.02,
            // v1.1.1: enabled 0→200ms — waits 200ms then re-checks the quote vault.
            // If the vault drained >15% (someone already front-ran the pump), skip.
            // This costs one extra RPC call per pool but eliminates the class of trades
            // where we buy into a pool that was already >15% pumped by the time our
            // tx builds — those trades are near-guaranteed immediate losses.
            confirmation_window_ms: 200,
            // v1.3.1: disabled session heat (0 = off) — the drawdown guard is a
            // better circuit breaker. Heat was miscounting forced sell-mode exits
            // (-0.499% AMM spread) as "losses", triggering 15-min buy pauses
            // after normal operation and causing the bot to miss profitable pools.
            session_heat_losses: 0,
            session_heat_window_secs: 3600,
            session_heat_cooldown_mins: 15,
            // v1.2.0: exit when in profit and volume drops >65% from entry vault —
            // signals the pump is exhausted and smart money has already exited.
            volume_exhaustion_pct: 65.0,
            buy_sell_ratio_exit: 0.0,
            check_interval_acceleration: true,
            // v1.2.0: exit instantly when a single check shows the vault drops >22%
            // in one tick — strong rug/whale-exit signal; acts faster than the
            // 3-consecutive-decline detector.
            whale_exit_vault_drop_pct: 22.0,
            // v1.3.0: ON — scales buy size by pool_score/100 so the best pools
            // get full capital while marginal ones get partial exposure.
            pool_quality_sizing: true,
            kelly_min_trades: 10,
            deployer_wallet_age_min_hours: 0,
            filter_cache_ttl_secs: 30,
            // v1.5.0: live data shows all profitable pump.fun trades exit within 6 s.
            // Positions flat beyond 45 s are dead — exit to redeploy capital.
            no_pump_timeout_secs: 45,
            no_pump_min_gain_pct: 3.0,
            min_dump_hold_secs: 0,
            peak_stagnation_secs: 90,
            peak_stagnation_min_pnl_pct: 20.0,
            runner_mode: false,
            runner_scale_in_sol: 0.02,
            pool_drain_exit_pct: 15.0,
            mint_cooldown_secs: 1800,
            ai_chain: AiChainConfig::default(),
        }
    }
}

/// Configuration for the 3-layer LLM pool-selection chain.
///
/// Layer assignment:
///   L1 = Groq (llama-3.1-8b-instant) — fast binary screener, GROQ_API_KEY
///   L2 = OpenRouter (gemma-2-9b-it:free) — deep analyst, OPENROUTER_API_KEY
///   L3 = Cerebras (llama-3.1-8b) — risk judge, CEREBRAS_API_KEY
///
/// All three providers have free tiers. Missing keys = chain disabled, falls
/// back to pool_scorer gating only.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AiChainConfig {
    /// Enable the 3-layer LLM chain. Requires all three API keys.
    pub enabled: bool,
    /// Groq model for Layer 1 screener. Default: llama-3.1-8b-instant
    pub l1_model: String,
    /// OpenRouter model for Layer 2 analyst. Default: google/gemma-2-9b-it:free
    pub l2_model: String,
    /// Cerebras model for Layer 3 risk judge. Default: llama-3.1-8b
    pub l3_model: String,
    /// L1 timeout in ms. Default 600.
    pub l1_timeout_ms: u64,
    /// L2+L3 parallel timeout in ms. Default 1800.
    pub l2l3_timeout_ms: u64,
    /// Minimum L2 analyst score (0-100) required for a BUY. Default 65.
    pub min_l2_score: u8,
}

impl Default for AiChainConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            l1_model: "llama-3.1-8b-instant".into(),
            l2_model: "google/gemma-2-9b-it:free".into(),
            l3_model: "llama-3.1-8b".into(),
            l1_timeout_ms: 600,
            l2l3_timeout_ms: 1800,
            min_l2_score: 65,
        }
    }
}

impl SniperConfig {
    /// Retrieve the active rate mode by name, or None if not found
    pub fn get_active_rate_mode(&self) -> Option<&RateMode> {
        self.rate_modes
            .iter()
            .find(|m| m.name == self.active_mode_name && m.enabled)
    }

    /// List all enabled rate modes sorted by order
    pub fn enabled_rate_modes(&self) -> Vec<&RateMode> {
        let mut modes: Vec<_> = self.rate_modes
            .iter()
            .filter(|m| m.enabled)
            .collect();
        modes.sort_by_key(|m| m.order);
        modes
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct FilterConfig {
    /// Interval ms between filter checks
    pub check_interval_ms: u64,
    /// Total duration ms to wait for filters to pass
    pub check_duration_ms: u64,
    /// How many consecutive passes required
    pub consecutive_matches: u32,
    pub check_mint_renounced: bool,
    pub check_freezable: bool,
    pub check_burned: bool,
    pub check_mutable: bool,
    pub check_socials: bool,
    /// Min pool size in quote token (0 = disabled)
    pub min_pool_size: f64,
    /// Max pool size in quote token (0 = disabled)
    pub max_pool_size: f64,
    /// Reject tokens whose name/symbol contains known scam words
    pub check_name: bool,
    /// Require minimum recent transaction activity before buying
    pub check_volume: bool,
    /// Minimum number of transactions on the pool in the last 60s
    pub min_volume_txns: u32,
    /// Check if our position size causes >X% price impact
    pub check_liquidity_depth: bool,
    /// Max acceptable price impact % for our buy size (0 = disabled)
    pub max_price_impact_pct: f64,
    /// Reject tokens where top-10 holders own more than this % of supply
    pub check_holder_concentration: bool,
    /// Maximum allowed top-10 holder concentration % (e.g. 70.0)
    pub max_top10_holder_pct: f64,

    // ── Liquidity momentum ────────────────────────────────────────────────────
    /// Reject pools whose quote vault hasn't grown enough between two checks
    pub check_liquidity_momentum: bool,
    /// Minimum required liquidity growth % in the momentum check window
    pub liquidity_momentum_pct: f64,

    // ── Cross-pool correlation rug guard ──────────────────────────────────────
    /// Reject tokens from deployers with too many recent rugs
    pub check_cross_pool_correlation: bool,
    /// Maximum allowable rug count in the last 24 h for a deployer
    pub max_deployer_rugs_24h: u32,

    // ── Jupiter price discrepancy ─────────────────────────────────────────────
    /// Only buy when Jupiter price is higher than AMM price by at least this %
    pub check_jupiter_discrepancy: bool,
    /// Minimum Jupiter premium % to treat as a buy signal
    pub jupiter_min_premium_pct: f64,

    // ── Deployer wallet age ───────────────────────────────────────────────────
    /// Reject pools from wallets younger than `deployer_min_age_hours`. Fresh
    /// wallets are a strong rug signal. 0 = disabled.
    pub check_deployer_wallet_age: bool,
    pub deployer_min_age_hours: u64,
    /// Cache filter results per pool pubkey for this many seconds.
    /// Avoids redundant RPC calls when duplicate pool events arrive. Default 30.
    pub filter_cache_ttl_secs: u64,
}

impl Default for FilterConfig {
    fn default() -> Self {
        // v0.9.2 sweet-spot tuning: v0.8.0 tightened filters 30% but the new
        // ON-by-default gates (liquidity_momentum, holder_concentration via the
        // 67% cap) rejected essentially all fresh pump.fun graduates because
        // memecoin reality is: top-heavy at launch, vault non-monotonic during
        // first few seconds, dev holds 30–60 % until first wave of buys.
        //
        // The new doctrine: cheap dumb filters loose, smart AI gates (Grok risk
        // scorer + dQ* observer) carry the discrimination weight. The dQ* agent
        // learns the pool-quality signal from realised PnL, so as it trains it
        // gets sharper — better than fixed thresholds.
        //
        // Kept ON because they're truly cheap and sharp:
        //   - check_freezable (real honeypot signal)
        //   - check_burned (empty vault is a real rug)
        //   - check_cross_pool_correlation (rejects known-rug deployers)
        Self {
            check_interval_ms: 500,
            check_duration_ms: 8_000,
            consecutive_matches: 1,
            check_mint_renounced: false,
            check_freezable: true,
            check_burned: true,
            check_mutable: true,
            check_socials: false,
            // v1.2.0: raised 1.0→2.0 SOL. The 1 SOL floor reduced the worst rug
            // rate but 1-2 SOL pools still drain within the first 500ms. 2 SOL gives
            // the pool enough liquidity cushion to survive initial bot traffic.
            // Ultra-low-cap operators can lower to 1.0 in config.toml.
            min_pool_size: 2.0,
            max_pool_size: 0.0,
            // v1.2.0: enabled — cheap name/symbol check filters the most obvious
            // scam tokens (test, fake, honeypot etc.) with zero RPC cost.
            check_name: true,
            check_volume: false,
            min_volume_txns: 3,
            check_liquidity_depth: true,
            max_price_impact_pct: 5.0,
            check_holder_concentration: false,
            // Soft guard if check_holder_concentration is enabled — 90 % means
            // we reject only the most extreme top-heavy pools. Fresh memecoins
            // are inherently top-heavy until early buyers diversify.
            max_top10_holder_pct: 90.0,
            // OFF by default — vault doesn't always grow during the first few
            // ticks of a pump.fun launch (early sells, fee burn). Enable
            // explicitly in config.toml only after backtesting against your
            // pool stream.
            check_liquidity_momentum: false,
            liquidity_momentum_pct: 5.0,
            // KEEP ON — sharp, cheap, and the dQ* agent's reward signal also
            // depends on the deployer ledger.
            check_cross_pool_correlation: true,
            // v1.2.0: tightened 3→2 — a deployer with 2+ rugs in 24h is a repeat
            // offender; blocking at 2 reduces exposure without excluding genuinely
            // new deployers (who have 0 rug history).
            max_deployer_rugs_24h: 2,
            check_jupiter_discrepancy: false,
            jupiter_min_premium_pct: 5.0,
            // v1.3.0: ON — reject deployers whose wallet is younger than 24h.
            // Fresh wallets are near-universally rug setups. 24h is enough
            // to eliminate day-1 throwaway wallets without being too strict.
            check_deployer_wallet_age: true,
            deployer_min_age_hours: 24,
            filter_cache_ttl_secs: 30,
        }
    }
}

/// Notification / alert configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AlertsConfig {
    /// Telegram bot token (leave empty to disable)
    pub telegram_bot_token: String,
    /// Telegram chat ID (user or group)
    pub telegram_chat_id: String,
    /// Discord webhook URL (leave empty to disable)
    pub discord_webhook_url: String,
    /// Fire Windows desktop toast notifications
    pub desktop_notifications: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArbConfig {
    pub enabled: bool,
    /// Starting token mint (default: USDC)
    pub start_mint: String,
    /// Starting capital in UI units
    pub start_amount: f64,
    /// Minimum profit in lamports to execute
    pub min_profit_lamports: u64,
    /// Max hops in arb path (2 or 3)
    pub max_hops: usize,
    /// DEXes to include in graph
    pub dexes: Vec<String>,
    /// Pool metadata directory
    pub pool_dir: String,
    /// How many parallel amount sizes to try (halving strategy)
    pub amount_levels: u32,
}

impl Default for ArbConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            start_mint: "USDC".into(),
            start_amount: 100.0,
            min_profit_lamports: 10_000,
            max_hops: 3,
            dexes: vec!["Raydium".into(), "Orca".into(), "Meteora".into()],
            pool_dir: "pools".into(),
            amount_levels: 4,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionConfig {
    /// "default" | "jito" | "warp"
    pub executor: String,
    /// Custom fee in SOL (used by jito/warp)
    pub custom_fee_sol: f64,
    /// Compute unit limit (default executor)
    pub compute_unit_limit: u32,
    /// Compute unit price in micro-lamports (default executor)
    pub compute_unit_price: u64,
    /// Skip preflight simulation
    pub skip_preflight: bool,
    /// Jito block engine URL
    pub jito_url: String,
}

impl Default for ExecutionConfig {
    fn default() -> Self {
        Self {
            executor: "default".into(),
            custom_fee_sol: 0.006,
            compute_unit_limit: 200_000,
            compute_unit_price: 100_000,
            skip_preflight: true,
            jito_url: "https://mainnet.block-engine.jito.wtf".into(),
        }
    }
}

impl BotConfig {
    /// Load config from a TOML file
    pub fn from_file(path: impl AsRef<Path>) -> Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let config: BotConfig = toml::from_str(&content)?;
        Ok(config)
    }

    /// Load config from environment variables (dotenv) and overrides from config file
    pub fn from_env() -> Result<Self> {
        dotenv::dotenv().ok();
        
        // Try loading from config.toml first, then fall back to env
        let config_path = std::env::var("CONFIG_PATH").unwrap_or_else(|_| "config.toml".into());
        if std::path::Path::new(&config_path).exists() {
            return Self::from_file(config_path);
        }

        Ok(Self {
            rpc: RpcConfig::default(),
            wallet: WalletConfig {
                keypair_path: std::env::var("KEYPAIR_PATH")
                    .unwrap_or_else(|_| "~/.config/solana/id.json".into()),
            },
            sniper: SniperConfig::default(),
            arb: ArbConfig::default(),
            execution: ExecutionConfig::default(),
            alerts: AlertsConfig::default(),
        })
    }
}
