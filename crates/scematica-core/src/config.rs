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
}

impl Default for SniperConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            quote_mint: "WSOL".into(),
            quote_amount: 0.1,
            buy_delay_ms: 0,
            max_buy_retries: 3,
            max_sell_retries: 3,
            auto_sell: true,
            auto_sell_delay_ms: 0,
            take_profit_pct: 50.0,
            stop_loss_pct: 20.0,
            buy_slippage_pct: 1.0,
            sell_slippage_pct: 1.0,
            price_check_interval_ms: 1000,
            price_check_duration_ms: 60_000,
            one_token_at_a_time: true,
            use_snipe_list: false,
            snipe_list_path: "snipe-list.txt".into(),
            max_buys: 0,
            trailing_stop_loss_pct: 0.0,
            partial_tp_pct: 0.0,
            partial_tp_trigger: 0.0,
            max_concurrent_positions: 0,
            cooldown_after_losses: 0,
            cooldown_minutes: 30,
            daily_loss_limit_sol: 0.0,
            max_drawdown_pct: 0.0,
            blacklist_path: "blacklist.txt".into(),
            copy_wallets: vec![],
            filters: FilterConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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
}

impl Default for FilterConfig {
    fn default() -> Self {
        Self {
            check_interval_ms: 2000,
            check_duration_ms: 20_000,
            consecutive_matches: 2,
            check_mint_renounced: true,
            check_freezable: true,
            check_burned: true,
            check_mutable: true,
            check_socials: false,
            min_pool_size: 5.0,
            max_pool_size: 0.0,
            check_name: true,
            check_volume: false,
            min_volume_txns: 3,
            check_liquidity_depth: true,
            max_price_impact_pct: 5.0,
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
