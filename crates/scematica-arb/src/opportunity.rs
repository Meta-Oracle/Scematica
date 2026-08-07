use crate::graph::PoolEdge;
use scematica_core::types::DexKind;
use serde::{Deserialize, Serialize};
use solana_sdk::pubkey::Pubkey;

/// A discovered arbitrage path
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArbPath {
    /// Unique ID for deduplication
    pub id: String,
    /// Token mint path: [A, B, C, A]
    pub mint_path: Vec<Pubkey>,
    /// Pool edges used at each hop
    pub pool_path: Vec<PoolEdge>,
    /// Input amount (in start token raw units)
    pub input_amount: u128,
    /// Expected output amount after all swaps
    pub output_amount: u128,
    /// Per-hop input amounts: hop_amounts[i] is the input to pool_path[i]
    pub hop_amounts: Vec<u64>,
    /// Profit = output - input
    pub profit: i128,
    /// Profit as a percentage of input
    pub profit_pct: f64,
    /// Unix millisecond timestamp when reserves were fetched — used for staleness check
    #[serde(default)]
    pub fetched_at_ms: u64,
}

impl ArbPath {
    pub fn new(
        mint_path: Vec<Pubkey>,
        pool_path: Vec<PoolEdge>,
        input_amount: u128,
        output_amount: u128,
        hop_amounts: Vec<u64>,
    ) -> Self {
        let profit = output_amount as i128 - input_amount as i128;
        let profit_pct = if input_amount > 0 {
            profit as f64 / input_amount as f64 * 100.0
        } else {
            0.0
        };

        // Dedup key: mint path + pool addresses
        let mint_keys: Vec<String> = mint_path.iter().map(|m| m.to_string()).collect();
        let pool_keys: Vec<String> = pool_path.iter().map(|p| p.pool_address.to_string()).collect();
        let id = format!("{}{}", mint_keys.join(""), pool_keys.join(""));

        let fetched_at_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        Self {
            id,
            mint_path,
            pool_path,
            input_amount,
            output_amount,
            hop_amounts,
            profit,
            profit_pct,
            fetched_at_ms,
        }
    }

    pub fn is_profitable(&self) -> bool {
        self.profit > 0
    }

    pub fn hops(&self) -> usize {
        self.pool_path.len()
    }

    pub fn dexes(&self) -> Vec<DexKind> {
        self.pool_path.iter().map(|p| p.dex).collect()
    }
}

impl std::fmt::Display for ArbPath {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mints: Vec<String> = self.mint_path.iter()
            .map(|m| m.to_string()[..8].to_string())
            .collect();
        write!(
            f,
            "{} | profit: {} ({:.3}%) | input: {}",
            mints.join(" → "),
            self.profit,
            self.profit_pct,
            self.input_amount,
        )
    }
}
