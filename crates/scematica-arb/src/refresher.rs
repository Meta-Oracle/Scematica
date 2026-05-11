use crate::graph::ArbGraph;
use anyhow::Result;
use scematica_core::rpc::DexFetcher;
use std::sync::Arc;
use tokio::time::{interval, Duration};
use tracing::{debug, error, info};

/// Periodically refreshes the reserves of pools in the ArbGraph
pub struct GraphRefresher {
    graph: Arc<ArbGraph>,
    fetcher: Arc<DexFetcher>,
    refresh_interval: Duration,
}

impl GraphRefresher {
    pub fn new(
        graph: Arc<ArbGraph>,
        fetcher: Arc<DexFetcher>,
        refresh_interval_ms: u64,
    ) -> Self {
        Self {
            graph,
            fetcher,
            refresh_interval: Duration::from_millis(refresh_interval_ms),
        }
    }

    /// Start the background refresh loop
    pub async fn run(&self) {
        let mut interval = interval(self.refresh_interval);
        info!("Graph refresher started (interval: {:?})", self.refresh_interval);

        loop {
            interval.tick().await;
            if let Err(e) = self.refresh_all_pools().await {
                error!("Error refreshing graph: {}", e);
            }
        }
    }

    async fn refresh_all_pools(&self) -> Result<()> {
        // Collect unique pool addresses from the graph
        let mut pools = std::collections::HashSet::new();
        for edge_map in self.graph.edges.iter() {
            for inner_map in edge_map.iter() {
                for edge in inner_map.iter() {
                    pools.insert(edge.pool_address);
                }
            }
        }

        debug!("Refreshing reserves for {} pools", pools.len());

        // For now, we refresh Raydium pools since we have the decoder
        // In a full impl, we'd dispatch based on edge.dex
        for pool_addr in pools {
            match self.fetcher.fetch_raydium_pool(&pool_addr).await {
                Ok((base_vault, quote_vault)) => {
                    match self.fetcher.fetch_reserves(&base_vault, &quote_vault).await {
                        Ok((res_a, res_b)) => {
                            self.graph.update_pool_reserves(&pool_addr, res_a, res_b);
                            debug!("Updated pool {}: {} / {}", pool_addr, res_a, res_b);
                        }
                        Err(e) => debug!("Failed to fetch vaults for pool {}: {}", pool_addr, e),
                    }
                }
                Err(e) => {
                    debug!("Could not fetch Raydium pool {}: {}", pool_addr, e);
                }
            }
        }

        Ok(())
    }
}
