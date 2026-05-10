use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use solana_sdk::pubkey::Pubkey;
use std::sync::Arc;

/// A pool edge in the arbitrage graph
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PoolEdge {
    pub pool_address: Pubkey,
    pub dex: scematica_core::types::DexKind,
    /// Current reserve of token A (input side)
    pub reserve_a: u64,
    /// Current reserve of token B (output side)
    pub reserve_b: u64,
    /// Fee numerator (e.g. 25 for 0.25%)
    pub fee_numerator: u64,
    /// Fee denominator (e.g. 10000)
    pub fee_denominator: u64,
}

impl PoolEdge {
    /// Constant-product AMM quote with fee
    pub fn get_quote(&self, amount_in: u128) -> u128 {
        if self.reserve_a == 0 || self.reserve_b == 0 {
            return 0;
        }
        let fee_denom = self.fee_denominator as u128;
        let fee_num = self.fee_numerator as u128;
        let amount_with_fee = amount_in * (fee_denom - fee_num);
        let numerator = (self.reserve_b as u128) * amount_with_fee;
        let denominator = (self.reserve_a as u128) * fee_denom + amount_with_fee;
        if denominator == 0 {
            return 0;
        }
        numerator / denominator
    }
}

/// Arbitrage graph: maps mint_index → mint_index → Vec<PoolEdge>
#[derive(Debug, Default, Clone)]
pub struct ArbGraph {
    /// mint pubkey → index
    pub mint_to_idx: Arc<DashMap<Pubkey, usize>>,
    /// index → mint pubkey
    pub idx_to_mint: Arc<DashMap<usize, Pubkey>>,
    /// adjacency: from_idx → to_idx → edges
    pub edges: Arc<DashMap<usize, DashMap<usize, Vec<PoolEdge>>>>,
}

impl ArbGraph {
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

    pub fn add_pool(
        &self,
        mint_a: Pubkey,
        mint_b: Pubkey,
        edge: PoolEdge,
    ) {
        let idx_a = self.add_mint(mint_a);
        let idx_b = self.add_mint(mint_b);

        // A → B
        self.edges
            .entry(idx_a)
            .or_default()
            .entry(idx_b)
            .or_default()
            .push(edge.clone());

        // B → A (reversed reserves)
        let reversed = PoolEdge {
            reserve_a: edge.reserve_b,
            reserve_b: edge.reserve_a,
            ..edge
        };
        self.edges
            .entry(idx_b)
            .or_default()
            .entry(idx_a)
            .or_default()
            .push(reversed);
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

    pub fn edges_between(&self, from: usize, to: usize) -> Vec<PoolEdge> {
        self.edges
            .get(&from)
            .and_then(|m| m.get(&to).map(|v| v.clone()))
            .unwrap_or_default()
    }

    /// Update reserves for a specific pool
    pub fn update_pool_reserves(
        &self,
        pool_address: &Pubkey,
        new_reserve_a: u64,
        new_reserve_b: u64,
    ) {
        for edge_map in self.edges.iter() {
            for mut edges in edge_map.iter_mut() {
                for edge in edges.iter_mut() {
                    if &edge.pool_address == pool_address {
                        edge.reserve_a = new_reserve_a;
                        edge.reserve_b = new_reserve_b;
                    }
                }
            }
        }
    }
}
