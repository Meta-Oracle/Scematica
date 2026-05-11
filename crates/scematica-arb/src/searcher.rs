use crate::{
    graph::{ArbGraph, PoolEdge},
    opportunity::ArbPath,
};
use scematica_core::config::ArbConfig;
use solana_sdk::pubkey::Pubkey;
use std::collections::HashSet;
use tracing::{debug, info};

/// Brute-force arbitrage searcher over the pool graph.
/// Mirrors the approach from 0xNineteen/solana-arbitrage-bot:
/// - Start from a given mint
/// - DFS up to max_hops deep
/// - Try multiple input sizes (halving strategy)
/// - Collect all profitable paths
pub struct ArbSearcher {
    pub graph: ArbGraph,
    pub config: ArbConfig,
}

impl ArbSearcher {
    pub fn new(graph: ArbGraph, config: ArbConfig) -> Self {
        Self { graph, config }
    }

    /// Search for all profitable arbitrage paths starting from `start_mint`
    /// with the given `start_amount`. Returns paths sorted by profit descending.
    pub fn search(&self, start_mint: &Pubkey, start_amount: u128) -> Vec<ArbPath> {
        let start_idx = match self.graph.get_idx(start_mint) {
            Some(idx) => idx,
            None => {
                debug!("Start mint not in graph: {}", start_mint);
                return vec![];
            }
        };

        let mut all_paths = vec![];
        let mut seen_ids: HashSet<String> = HashSet::new();

        // Try multiple input sizes: start_amount, start_amount/2, start_amount/4, ...
        let mut amount = start_amount;
        let min_amount = 1_000_000u128; // 1 USDC minimum

        for _ in 0..self.config.amount_levels {
            if amount < min_amount {
                break;
            }

            let mut paths = vec![];
            self.dfs(
                start_idx,
                start_idx,
                amount,
                amount,
                vec![start_idx],
                vec![],
                vec![],
                &mut paths,
                &mut seen_ids,
            );

            all_paths.extend(paths);
            amount /= 2;
        }

        // Sort by profit descending
        all_paths.sort_by(|a, b| b.profit.cmp(&a.profit));
        all_paths
    }

    /// Recursive DFS over the graph
    fn dfs(
        &self,
        start_idx: usize,
        curr_idx: usize,
        init_amount: u128,
        curr_amount: u128,
        mint_path: Vec<usize>,
        pool_path: Vec<PoolEdge>,
        hop_amounts: Vec<u64>,
        results: &mut Vec<ArbPath>,
        seen_ids: &mut HashSet<String>,
    ) {
        // Max path length: max_hops + 1 (includes start mint repeated at end)
        if mint_path.len() > self.config.max_hops + 1 {
            return;
        }

        let neighbors = self.graph.neighbors(curr_idx);

        for next_idx in neighbors {
            // Don't revisit mints unless it's the start (closing the loop)
            if mint_path.contains(&next_idx) && next_idx != start_idx {
                continue;
            }

            let edges = self.graph.edges_between(curr_idx, next_idx);

            for edge in edges {
                let out_amount = edge.get_quote(curr_amount);
                if out_amount == 0 {
                    continue;
                }

                let mut new_mint_path = mint_path.clone();
                new_mint_path.push(next_idx);
                let mut new_pool_path = pool_path.clone();
                new_pool_path.push(edge.clone());
                let mut new_hop_amounts = hop_amounts.clone();
                new_hop_amounts.push(curr_amount as u64);

                if next_idx == start_idx {
                    // Closed the loop — check profitability
                    if out_amount > init_amount {
                        let _start_mint = self.graph.get_mint(start_idx).unwrap_or_default();
                        let mint_pubkeys: Vec<Pubkey> = new_mint_path
                            .iter()
                            .filter_map(|&idx| self.graph.get_mint(idx))
                            .collect();

                        let path = ArbPath::new(
                            mint_pubkeys,
                            new_pool_path,
                            init_amount,
                            out_amount,
                            new_hop_amounts,
                        );

                        // Dedup check
                        if !seen_ids.contains(&path.id) {
                            seen_ids.insert(path.id.clone());
                            info!(
                                "💰 Arb found: {} → profit: {} ({:.3}%)",
                                path.hops(),
                                path.profit,
                                path.profit_pct
                            );
                            results.push(path);
                        }
                    }
                } else if !mint_path.contains(&next_idx) {
                    // Continue searching deeper
                    self.dfs(
                        start_idx,
                        next_idx,
                        init_amount,
                        out_amount,
                        new_mint_path,
                        new_pool_path,
                        new_hop_amounts,
                        results,
                        seen_ids,
                    );
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::PoolEdge;
    use scematica_core::{config::ArbConfig, types::DexKind};
    use solana_sdk::pubkey::Pubkey;

    fn make_edge(reserve_a: u64, reserve_b: u64) -> PoolEdge {
        PoolEdge {
            pool_address: Pubkey::new_unique(),
            dex: DexKind::Raydium,
            reserve_a,
            reserve_b,
            fee_numerator: 25,
            fee_denominator: 10_000,
        }
    }

    #[test]
    fn test_no_arb_balanced_pools() {
        let graph = ArbGraph::new();
        let mint_a = Pubkey::new_unique();
        let mint_b = Pubkey::new_unique();
        let mint_c = Pubkey::new_unique();

        // Balanced pools: no arb opportunity
        graph.add_pool(mint_a, mint_b, make_edge(1_000_000, 1_000_000));
        graph.add_pool(mint_b, mint_c, make_edge(1_000_000, 1_000_000));
        graph.add_pool(mint_c, mint_a, make_edge(1_000_000, 1_000_000));

        let config = ArbConfig {
            max_hops: 3,
            amount_levels: 1,
            ..ArbConfig::default()
        };
        let searcher = ArbSearcher::new(graph, config);
        let paths = searcher.search(&mint_a, 100_000);

        // With balanced pools and fees, should find no profitable arb
        assert!(paths.iter().all(|p| p.profit <= 0));
    }

    #[test]
    fn test_arb_imbalanced_pools() {
        let graph = ArbGraph::new();
        let mint_a = Pubkey::new_unique();
        let mint_b = Pubkey::new_unique();

        // Imbalanced: pool1 has more B per A than pool2
        // A→B via pool1 (cheap B), B→A via pool2 (expensive B)
        let pool1 = PoolEdge {
            pool_address: Pubkey::new_unique(),
            dex: DexKind::Raydium,
            reserve_a: 1_000_000,
            reserve_b: 2_000_000, // 2:1 ratio
            fee_numerator: 0,
            fee_denominator: 10_000,
        };
        let pool2 = PoolEdge {
            pool_address: Pubkey::new_unique(),
            dex: DexKind::Orca,
            reserve_a: 2_000_000, // 1:1 ratio
            reserve_b: 2_000_000,
            fee_numerator: 0,
            fee_denominator: 10_000,
        };

        graph.add_pool(mint_a, mint_b, pool1);
        graph.add_pool(mint_b, mint_a, pool2);

        let config = ArbConfig {
            max_hops: 2,
            amount_levels: 1,
            ..ArbConfig::default()
        };
        let searcher = ArbSearcher::new(graph, config);
        let paths = searcher.search(&mint_a, 100_000);

        // Should find at least one profitable path
        assert!(!paths.is_empty(), "Expected at least one arb path");
        assert!(paths[0].profit > 0, "Expected positive profit");
    }

    #[test]
    fn test_quote_formula() {
        let edge = make_edge(1_000_000, 1_000_000);
        let out = edge.get_quote(100_000);
        // With 0.25% fee: out ≈ 90,702 (xy=k with fee)
        assert!(out > 0);
        assert!(out < 100_000); // always less due to fee + slippage
    }
}
