use crate::graph::{ArbGraph, PoolEdge};
use anyhow::Result;
use scematica_core::{rpc::DexFetcher, types::DexKind};
use solana_sdk::pubkey::Pubkey;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::time::{interval, Duration};
use tracing::{debug, error, info, warn};

/// Vault pair for a pool — stored at graph-build time so the refresher
/// doesn't need to re-decode pool accounts on every tick.
#[derive(Debug, Clone)]
pub struct PoolVaultInfo {
    pub pool_address: Pubkey,
    pub dex: DexKind,
    pub vault_a: Pubkey,
    pub vault_b: Pubkey,
}

/// Periodically refreshes the reserves of all pools in the ArbGraph.
/// Dispatches by DEX type and batches RPC calls to minimise round-trips.
pub struct GraphRefresher {
    graph: Arc<ArbGraph>,
    fetcher: Arc<DexFetcher>,
    refresh_interval: Duration,
    /// Pre-resolved vault pairs, keyed by pool address.
    /// Populated lazily on first refresh; re-resolved when a pool is missing.
    vault_cache: tokio::sync::RwLock<HashMap<Pubkey, PoolVaultInfo>>,
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
            vault_cache: tokio::sync::RwLock::new(HashMap::new()),
        }
    }

    /// Start the background refresh loop.
    pub async fn run(&self) {
        let mut ticker = interval(self.refresh_interval);
        info!("Graph refresher started (interval: {:?})", self.refresh_interval);

        loop {
            ticker.tick().await;
            if let Err(e) = self.refresh_all_pools().await {
                error!("Error refreshing graph: {}", e);
            }
        }
    }

    async fn refresh_all_pools(&self) -> Result<()> {
        // Collect unique (pool_address, dex) pairs from the graph
        let mut pool_dex_map: HashMap<Pubkey, DexKind> = HashMap::new();
        for edge_map in self.graph.edges.iter() {
            for inner_map in edge_map.iter() {
                for edge in inner_map.iter() {
                    pool_dex_map.entry(edge.pool_address).or_insert(edge.dex);
                }
            }
        }

        debug!("Refreshing reserves for {} pools", pool_dex_map.len());

        // Ensure vault cache is populated for all known pools
        self.populate_vault_cache(&pool_dex_map).await;

        // Collect all vault pubkeys for a single batched RPC call
        let vault_infos: Vec<PoolVaultInfo> = {
            let cache = self.vault_cache.read().await;
            pool_dex_map
                .keys()
                .filter_map(|addr| cache.get(addr).cloned())
                .collect()
        };

        if vault_infos.is_empty() {
            return Ok(());
        }

        // Build flat list of all vault pubkeys (vault_a, vault_b interleaved)
        let all_vaults: Vec<Pubkey> = vault_infos
            .iter()
            .flat_map(|v| [v.vault_a, v.vault_b])
            .collect();

        // Batch fetch in chunks of 100 (RPC limit)
        let mut balances: HashMap<Pubkey, u64> = HashMap::new();
        for chunk in all_vaults.chunks(100) {
            match self.fetcher.rpc.client.get_multiple_accounts(chunk).await {
                Ok(accounts) => {
                    for (pubkey, maybe_account) in chunk.iter().zip(accounts.iter()) {
                        if let Some(account) = maybe_account {
                            // SPL token account: amount is at bytes 64..72
                            if account.data.len() >= 72 {
                                let amount = u64::from_le_bytes(
                                    account.data[64..72].try_into().unwrap_or([0u8; 8]),
                                );
                                balances.insert(*pubkey, amount);
                            }
                        }
                    }
                }
                Err(e) => {
                    warn!("Batch vault fetch failed: {}", e);
                }
            }
        }

        // Apply updated reserves to the graph
        let mut updated = 0usize;
        for info in &vault_infos {
            let res_a = balances.get(&info.vault_a).copied().unwrap_or(0);
            let res_b = balances.get(&info.vault_b).copied().unwrap_or(0);
            if res_a > 0 || res_b > 0 {
                self.graph.update_pool_reserves(&info.pool_address, res_a, res_b);
                updated += 1;
            }
        }

        debug!("Updated reserves for {}/{} pools", updated, vault_infos.len());
        Ok(())
    }

    /// Resolve vault addresses for any pools not yet in the cache.
    /// Dispatches by DEX type — Raydium uses on-chain account layout decoding,
    /// Orca/Meteora fall back to a direct vault-balance approach.
    async fn populate_vault_cache(&self, pool_dex_map: &HashMap<Pubkey, DexKind>) {
        let missing: Vec<(Pubkey, DexKind)> = {
            let cache = self.vault_cache.read().await;
            pool_dex_map
                .iter()
                .filter(|(addr, _)| !cache.contains_key(*addr))
                .map(|(addr, dex)| (*addr, *dex))
                .collect()
        };

        if missing.is_empty() {
            return;
        }

        debug!("Resolving vaults for {} new pools", missing.len());

        let mut new_entries: Vec<PoolVaultInfo> = Vec::new();

        for (pool_addr, dex) in missing {
            match dex {
                DexKind::Raydium => {
                    match self.fetcher.fetch_raydium_pool(&pool_addr).await {
                        Ok((vault_a, vault_b)) => {
                            new_entries.push(PoolVaultInfo {
                                pool_address: pool_addr,
                                dex,
                                vault_a,
                                vault_b,
                            });
                        }
                        Err(e) => {
                            debug!("Could not resolve Raydium pool {}: {}", pool_addr, e);
                        }
                    }
                }
                DexKind::Orca => {
                    // Orca Whirlpool layout: token_vault_a at offset 101, token_vault_b at 133
                    match self.fetch_orca_vaults(&pool_addr).await {
                        Ok((vault_a, vault_b)) => {
                            new_entries.push(PoolVaultInfo {
                                pool_address: pool_addr,
                                dex,
                                vault_a,
                                vault_b,
                            });
                        }
                        Err(e) => {
                            debug!("Could not resolve Orca pool {}: {}", pool_addr, e);
                        }
                    }
                }
                DexKind::Meteora => {
                    // Meteora DLMM layout: reserve_x at offset 72, reserve_y at 104
                    match self.fetch_meteora_vaults(&pool_addr).await {
                        Ok((vault_a, vault_b)) => {
                            new_entries.push(PoolVaultInfo {
                                pool_address: pool_addr,
                                dex,
                                vault_a,
                                vault_b,
                            });
                        }
                        Err(e) => {
                            debug!("Could not resolve Meteora pool {}: {}", pool_addr, e);
                        }
                    }
                }
                _ => {
                    // For unknown DEX types, skip vault resolution
                    debug!("Skipping vault resolution for unsupported DEX {:?}", dex);
                }
            }
        }

        if !new_entries.is_empty() {
            let mut cache = self.vault_cache.write().await;
            for entry in new_entries {
                cache.insert(entry.pool_address, entry);
            }
        }
    }

    /// Decode Orca Whirlpool account to extract vault pubkeys.
    /// Layout (after 8-byte Anchor discriminator):
    ///   whirlpools_config: 32, token_mint_a: 32, token_mint_b: 32,
    ///   bump: 1, tick_spacing: 2, tick_spacing_seed: 2,
    ///   fee_rate: 2, protocol_fee_rate: 2, liquidity: 16,
    ///   sqrt_price: 16, tick_current_index: 4, protocol_fee_owed_a: 8,
    ///   protocol_fee_owed_b: 8, token_vault_a: 32, token_vault_b: 32
    async fn fetch_orca_vaults(&self, pool: &Pubkey) -> Result<(Pubkey, Pubkey)> {
        const VAULT_A_OFFSET: usize = 8 + 32 + 32 + 32 + 1 + 2 + 2 + 2 + 2 + 16 + 16 + 4 + 8 + 8; // = 165
        const VAULT_B_OFFSET: usize = VAULT_A_OFFSET + 32; // = 197
        const MIN_LEN: usize = VAULT_B_OFFSET + 32;

        let data = self.fetcher.rpc.client.get_account_data(pool).await?;
        if data.len() < MIN_LEN {
            anyhow::bail!("Orca pool data too short for {}", pool);
        }
        let vault_a = Pubkey::try_from(&data[VAULT_A_OFFSET..VAULT_A_OFFSET + 32])
            .map_err(|_| anyhow::anyhow!("failed to parse Orca vault_a"))?;
        let vault_b = Pubkey::try_from(&data[VAULT_B_OFFSET..VAULT_B_OFFSET + 32])
            .map_err(|_| anyhow::anyhow!("failed to parse Orca vault_b"))?;
        Ok((vault_a, vault_b))
    }

    /// Decode Meteora DLMM LbPair account to extract reserve pubkeys.
    /// Layout (after 8-byte discriminator):
    ///   parameters: 32, v_parameters: 32, bump_seed: 1, bin_step_seed: 2,
    ///   pair_type: 1, active_id: 4, bin_step: 2, status: 1, require_base_factor_seed: 1,
    ///   base_factor_seed: 2, padding1: 2, token_x_mint: 32, token_y_mint: 32,
    ///   reserve_x: 32, reserve_y: 32
    async fn fetch_meteora_vaults(&self, pool: &Pubkey) -> Result<(Pubkey, Pubkey)> {
        const RESERVE_X_OFFSET: usize = 8 + 32 + 32 + 1 + 2 + 1 + 4 + 2 + 1 + 1 + 2 + 2 + 32 + 32; // = 152
        const RESERVE_Y_OFFSET: usize = RESERVE_X_OFFSET + 32; // = 184
        const MIN_LEN: usize = RESERVE_Y_OFFSET + 32;

        let data = self.fetcher.rpc.client.get_account_data(pool).await?;
        if data.len() < MIN_LEN {
            anyhow::bail!("Meteora pool data too short for {}", pool);
        }
        let vault_a = Pubkey::try_from(&data[RESERVE_X_OFFSET..RESERVE_X_OFFSET + 32])
            .map_err(|_| anyhow::anyhow!("failed to parse Meteora reserve_x"))?;
        let vault_b = Pubkey::try_from(&data[RESERVE_Y_OFFSET..RESERVE_Y_OFFSET + 32])
            .map_err(|_| anyhow::anyhow!("failed to parse Meteora reserve_y"))?;
        Ok((vault_a, vault_b))
    }
}
