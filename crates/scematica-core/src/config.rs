use serde::{Deserialize, Serialize};
use std::path::Path;
use anyhow::Result;

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
}

impl Default for SniperConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            quote_mint: "WSOL".into(),
            quote_amount: 0.1,
            buy_delay_ms: 0,
            max_buy_retries: 3,
            max_sell_retries: 5,
            auto_sell: true,
            auto_sell_delay_ms: 0,
            take_profit_pct: 50.0,
            stop_loss_pct: 15.0,
            buy_slippage_pct: 1.5,
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
            trailing_stop_loss_pct: 15.0,
            partial_tp_pct: 50.0,
            partial_tp_trigger: 30.0,
            max_concurrent_positions: 0,
            cooldown_after_losses: 3,
            cooldown_minutes: 15,
            daily_loss_limit_sol: 0.0,
            max_drawdown_pct: 0.0,
            blacklist_path: "blacklist.txt".into(),
            copy_wallets: vec![],
            filters: FilterConfig::default(),
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
            // v0.9.2 sweet-spot: dropped 45 → 25. The PoolScorer's sharp bands
            // mean a pool below 25 truly is junk (no fresh-age bonus AND
            // sub-1-SOL liquidity). Above 25 we hand the decision to the AI
            // gates: Grok risk scorer + dQ* observer. Those two evolve with
            // training data; the static score gate doesn't.
            min_pool_score: 25.0,
            extra_rpc_endpoints: vec![],
            adaptive_slippage: false,
            sandwich_shield: false,
            // Profit-first defaults: ON, build toward 0.2 SOL, rug-only floor at -50%.
            // Operators who want classic SL behavior can set `profit_first_mode = false`.
            profit_first_mode: true,
            wallet_target_sol: 0.2,
            profit_first_floor_pct: 50.0,
            // Momentum-aware long-term sniping: ON by default. The escalation +
            // pullback exit replaces fixed-TP greed with "ride strong winners,
            // lock when they cool". See the field comments above for tuning.
            momentum_hold: true,
            momentum_window_checks: 5,
            momentum_escalation_threshold_pct: 5.0,
            momentum_escalation_factor: 1.5,
            momentum_max_escalations: 4,
            momentum_min_peak_pct: 20.0,
            momentum_pullback_exit_pct: 15.0,
            // v0.9.6 perfect-exit defaults
            adaptive_pullback: true,
            velocity_decay_exit: true,
            velocity_decay_window: 3,
            // v1.0.0: lowered 10→7 so the inflection signal fires earlier while
            // still above typical noise floor for memecoin price action.
            velocity_decay_min_pnl_pct: 7.0,
            // v1.0.0: tightened 2.0→1.5 — catches deceleration a tick sooner
            // without false-positive risk on healthy pauses.
            velocity_decay_drop_threshold: 1.5,
            tiered_partial_tp: true,
            tiered_partial_tp_levels: vec![
                (30.0, 25.0),
                (75.0, 25.0),
                (150.0, 25.0),
            ],
            // v1.0.0 reliability defaults
            flash_crash_pct: 22.0,
            profit_lock_checks: 8,
            close_ata_on_sell: true,
            max_position_hold_mins: 90,
        }
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
            min_pool_size: 0.25,
            max_pool_size: 0.0,
            check_name: false,
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
            max_deployer_rugs_24h: 3,
            check_jupiter_discrepancy: false,
            jupiter_min_premium_pct: 5.0,
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
