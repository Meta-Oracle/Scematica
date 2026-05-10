use crate::types::{DexKind, SwapQuote, TokenInfo};
use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use solana_sdk::pubkey::Pubkey;

/// Metadata about a liquidity pool
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PoolInfo {
    pub address: Pubkey,
    pub dex: DexKind,
    pub token_a: TokenInfo,
    pub token_b: TokenInfo,
    pub token_a_vault: Pubkey,
    pub token_b_vault: Pubkey,
    pub token_a_reserve: u64,
    pub token_b_reserve: u64,
    pub fee_numerator: u64,
    pub fee_denominator: u64,
    pub open_time: u64,
}

impl PoolInfo {
    /// Constant-product AMM quote: given input amount, compute output
    /// Uses the standard xy=k formula with fee deduction
    pub fn get_quote(&self, input_mint: &Pubkey, input_amount: u64) -> Option<u64> {
        let (reserve_in, reserve_out) = if input_mint == &self.token_a.mint {
            (self.token_a_reserve, self.token_b_reserve)
        } else if input_mint == &self.token_b.mint {
            (self.token_b_reserve, self.token_a_reserve)
        } else {
            return None;
        };

        if reserve_in == 0 || reserve_out == 0 {
            return None;
        }

        // Apply fee: amount_in_with_fee = amount_in * (fee_denom - fee_num)
        let fee_denom = self.fee_denominator;
        let fee_num = self.fee_numerator;
        let amount_in_with_fee = (input_amount as u128)
            .checked_mul((fee_denom - fee_num) as u128)?;

        // xy=k: out = (reserve_out * amount_in_with_fee) / (reserve_in * fee_denom + amount_in_with_fee)
        let numerator = (reserve_out as u128).checked_mul(amount_in_with_fee)?;
        let denominator = (reserve_in as u128)
            .checked_mul(fee_denom as u128)?
            .checked_add(amount_in_with_fee)?;

        let output = numerator.checked_div(denominator)?;
        Some(output as u64)
    }

    /// Returns the other token mint given one side
    pub fn other_mint(&self, mint: &Pubkey) -> Option<&Pubkey> {
        if mint == &self.token_a.mint {
            Some(&self.token_b.mint)
        } else if mint == &self.token_b.mint {
            Some(&self.token_a.mint)
        } else {
            None
        }
    }

    pub fn contains_mint(&self, mint: &Pubkey) -> bool {
        &self.token_a.mint == mint || &self.token_b.mint == mint
    }
}

/// Trait that all DEX pool adapters must implement
#[async_trait]
pub trait PoolAdapter: Send + Sync {
    fn pool_info(&self) -> &PoolInfo;

    /// Fetch latest on-chain reserves and update internal state
    async fn refresh(&mut self, rpc: &crate::rpc::RpcConnection) -> Result<()>;

    /// Build a swap quote (off-chain estimate)
    fn quote(&self, input_mint: &Pubkey, input_amount: u64) -> Option<SwapQuote>;

    /// Build the swap instruction(s) for this pool
    async fn build_swap_ix(
        &self,
        owner: &Pubkey,
        input_mint: &Pubkey,
        input_amount: u64,
        min_output: u64,
    ) -> Result<Vec<solana_sdk::instruction::Instruction>>;
}

/// Pool graph node: maps mint → list of pools that trade that mint
#[derive(Debug, Default)]
pub struct PoolGraph {
    /// mint_index → (mint_index → Vec<pool_address>)
    pub edges: dashmap::DashMap<usize, dashmap::DashMap<usize, Vec<Pubkey>>>,
    pub mint_to_idx: dashmap::DashMap<Pubkey, usize>,
    pub idx_to_mint: dashmap::DashMap<usize, Pubkey>,
}

impl PoolGraph {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_mint(&self, mint: Pubkey) -> usize {
        if let Some(idx) = self.mint_to_idx.get(&mint) {
            return *idx;
        }
        let idx = self.mint_to_idx.len();
        self.mint_to_idx.insert(mint, idx);
        self.idx_to_mint.insert(idx, mint);
        idx
    }

    pub fn add_edge(&self, mint_a: Pubkey, mint_b: Pubkey, pool: Pubkey) {
        let idx_a = self.add_mint(mint_a);
        let idx_b = self.add_mint(mint_b);

        self.edges
            .entry(idx_a)
            .or_default()
            .entry(idx_b)
            .or_default()
            .push(pool);

        self.edges
            .entry(idx_b)
            .or_default()
            .entry(idx_a)
            .or_default()
            .push(pool);
    }

    pub fn mint_count(&self) -> usize {
        self.mint_to_idx.len()
    }

    pub fn get_idx(&self, mint: &Pubkey) -> Option<usize> {
        self.mint_to_idx.get(mint).map(|v| *v)
    }

    pub fn get_mint(&self, idx: usize) -> Option<Pubkey> {
        self.idx_to_mint.get(&idx).map(|v| *v)
    }

    pub fn neighbors(&self, idx: usize) -> Vec<usize> {
        self.edges
            .get(&idx)
            .map(|m| m.iter().map(|e| *e.key()).collect())
            .unwrap_or_default()
    }

    pub fn pools_between(&self, idx_a: usize, idx_b: usize) -> Vec<Pubkey> {
        self.edges
            .get(&idx_a)
            .and_then(|m| m.get(&idx_b).map(|v| v.clone()))
            .unwrap_or_default()
    }
}
