use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use solana_sdk::pubkey::Pubkey;
use chrono::{DateTime, Utc};

/// A token with its mint address and decimals
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TokenInfo {
    pub mint: Pubkey,
    pub symbol: String,
    pub decimals: u8,
}

impl TokenInfo {
    pub fn new(mint: Pubkey, symbol: impl Into<String>, decimals: u8) -> Self {
        Self {
            mint,
            symbol: symbol.into(),
            decimals,
        }
    }

    /// Convert raw u64 amount to human-readable decimal
    pub fn to_ui_amount(&self, raw: u64) -> Decimal {
        let divisor = Decimal::from(10u64.pow(self.decimals as u32));
        Decimal::from(raw) / divisor
    }

    /// Convert human-readable decimal to raw u64
    pub fn to_raw_amount(&self, ui: Decimal) -> u64 {
        let multiplier = Decimal::from(10u64.pow(self.decimals as u32));
        (ui * multiplier).to_u64_saturating()
    }
}

/// Well-known token mints on Solana mainnet
pub mod known_tokens {
    use solana_sdk::pubkey;
    use solana_sdk::pubkey::Pubkey;

    pub const WSOL_MINT: Pubkey = pubkey!("So11111111111111111111111111111111111111112");
    pub const USDC_MINT: Pubkey = pubkey!("EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v");
    pub const USDT_MINT: Pubkey = pubkey!("Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB");
    pub const RAY_MINT: Pubkey = pubkey!("4k3Dyjzvzp8eMZWUXbBCjEvwSkkk59S5iCNLY3QrkX6R");
    pub const BONK_MINT: Pubkey = pubkey!("DezXAZ8z7PnrnRJjz3wXBoRgixCa6xjnB7YaB1pPB263");
}

/// Direction of a swap
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SwapDirection {
    Buy,
    Sell,
}

/// Result of a swap quote
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwapQuote {
    pub input_mint: Pubkey,
    pub output_mint: Pubkey,
    pub input_amount: u64,
    pub output_amount: u64,
    pub min_output_amount: u64, // after slippage
    pub price_impact_pct: Decimal,
    pub fee_amount: u64,
    pub dex: DexKind,
    pub pool_address: Pubkey,
}

impl SwapQuote {
    pub fn effective_price(&self) -> Decimal {
        if self.input_amount == 0 {
            return Decimal::ZERO;
        }
        Decimal::from(self.output_amount) / Decimal::from(self.input_amount)
    }
}

/// Supported DEX types
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DexKind {
    Raydium,
    Orca,
    Meteora,
    Jupiter,
    Saber,
    Mercurial,
    PumpFun,
    Unknown,
}

impl std::fmt::Display for DexKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DexKind::Raydium => write!(f, "Raydium"),
            DexKind::Orca => write!(f, "Orca"),
            DexKind::Meteora => write!(f, "Meteora"),
            DexKind::Jupiter => write!(f, "Jupiter"),
            DexKind::Saber => write!(f, "Saber"),
            DexKind::Mercurial => write!(f, "Mercurial"),
            DexKind::PumpFun => write!(f, "PumpFun"),
            DexKind::Unknown => write!(f, "Unknown"),
        }
    }
}

/// A trade record (executed or pending)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Trade {
    pub id: String,
    pub timestamp: DateTime<Utc>,
    pub direction: SwapDirection,
    pub input_token: TokenInfo,
    pub output_token: TokenInfo,
    pub input_amount: u64,
    pub output_amount: u64,
    pub dex: DexKind,
    pub pool_address: Pubkey,
    pub signature: Option<String>,
    pub status: TradeStatus,
    pub pnl_lamports: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TradeStatus {
    Pending,
    Submitted,
    Confirmed,
    Failed(String),
}

/// An arbitrage opportunity across multiple hops
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArbOpportunity {
    pub id: Uuid,
    pub timestamp: DateTime<Utc>,
    pub path: Vec<Pubkey>,          // token mint path: A -> B -> C -> A
    pub pools: Vec<Pubkey>,         // pool addresses used
    pub dexes: Vec<DexKind>,
    pub input_amount: u64,
    pub expected_output: u64,
    pub profit_lamports: i64,
    pub profit_pct: Decimal,
}

impl ArbOpportunity {
    pub fn is_profitable(&self) -> bool {
        self.profit_lamports > 0
    }
}

/// Trait for decimal saturation conversion
trait ToU64Saturating {
    fn to_u64_saturating(self) -> u64;
}

impl ToU64Saturating for Decimal {
    fn to_u64_saturating(self) -> u64 {
        use num_traits::ToPrimitive;
        self.to_u64().unwrap_or(u64::MAX)
    }
}
