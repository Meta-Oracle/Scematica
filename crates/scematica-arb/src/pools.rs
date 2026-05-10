use crate::graph::{ArbGraph, PoolEdge};
use anyhow::Result;
use scematica_core::types::DexKind;
use serde::{Deserialize, Serialize};
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::pubkey::Pubkey;
use std::sync::Arc;
use tracing::{debug, info, warn};

/// Pool metadata loaded from JSON files (mirrors 0xNineteen's pool JSON approach)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PoolJson {
    pub address: String,
    pub dex: String,
    pub token_a_mint: String,
    pub token_b_mint: String,
    pub token_a_vault: String,
    pub token_b_vault: String,
    pub fee_numerator: u64,
    pub fee_denominator: u64,
}

/// Load pool JSONs from a directory and populate the graph
pub async fn load_pools_from_dir(
    dir: &str,
    graph: &ArbGraph,
    rpc: &Arc<RpcClient>,
) -> Result<usize> {
    let mut count = 0;

    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) => {
            warn!("Pool dir '{}' not found: {}. Starting with empty graph.", dir, e);
            return Ok(0);
        }
    };

    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }

        let content = std::fs::read_to_string(&path)?;
        match serde_json::from_str::<PoolJson>(&content) {
            Ok(pool_json) => {
                if let Some(edge) = pool_json_to_edge(&pool_json, rpc).await {
                    let mint_a: Pubkey = pool_json.token_a_mint.parse().unwrap_or_default();
                    let mint_b: Pubkey = pool_json.token_b_mint.parse().unwrap_or_default();
                    graph.add_pool(mint_a, mint_b, edge);
                    count += 1;
                }
            }
            Err(e) => {
                warn!("Failed to parse pool JSON {:?}: {}", path, e);
            }
        }
    }

    info!("Loaded {} pools from {}", count, dir);
    Ok(count)
}

/// Convert a PoolJson + live reserves into a PoolEdge
async fn pool_json_to_edge(pool: &PoolJson, rpc: &Arc<RpcClient>) -> Option<PoolEdge> {
    let pool_address: Pubkey = pool.address.parse().ok()?;
    let vault_a: Pubkey = pool.token_a_vault.parse().ok()?;
    let vault_b: Pubkey = pool.token_b_vault.parse().ok()?;

    let dex = match pool.dex.as_str() {
        "raydium" | "Raydium" => DexKind::Raydium,
        "orca" | "Orca" => DexKind::Orca,
        "meteora" | "Meteora" => DexKind::Meteora,
        "saber" | "Saber" => DexKind::Saber,
        "mercurial" | "Mercurial" => DexKind::Mercurial,
        _ => DexKind::Unknown,
    };

    // Fetch live reserves
    let (reserve_a, reserve_b) = fetch_reserves(rpc, &vault_a, &vault_b).await;

    Some(PoolEdge {
        pool_address,
        dex,
        reserve_a,
        reserve_b,
        fee_numerator: pool.fee_numerator,
        fee_denominator: pool.fee_denominator,
    })
}

/// Fetch token vault balances in parallel
async fn fetch_reserves(
    rpc: &Arc<RpcClient>,
    vault_a: &Pubkey,
    vault_b: &Pubkey,
) -> (u64, u64) {
    let (res_a, res_b) = tokio::join!(
        rpc.get_token_account_balance(vault_a),
        rpc.get_token_account_balance(vault_b),
    );

    let reserve_a = res_a
        .ok()
        .and_then(|b| b.amount.parse::<u64>().ok())
        .unwrap_or(0);
    let reserve_b = res_b
        .ok()
        .and_then(|b| b.amount.parse::<u64>().ok())
        .unwrap_or(0);

    (reserve_a, reserve_b)
}

/// Refresh all pool reserves in the graph by re-fetching vaults
/// Called periodically to keep quotes accurate
pub async fn refresh_graph_reserves(
    graph: &ArbGraph,
    pool_vaults: &[(Pubkey, Pubkey, Pubkey)], // (pool_addr, vault_a, vault_b)
    rpc: &Arc<RpcClient>,
) -> Result<()> {
    // Batch fetch all vault accounts
    let all_vaults: Vec<Pubkey> = pool_vaults
        .iter()
        .flat_map(|(_, va, vb)| [*va, *vb])
        .collect();

    // Chunk into batches of 100 (RPC limit)
    for chunk in all_vaults.chunks(100) {
        let accounts = rpc.get_multiple_accounts(chunk).await?;
        // Process accounts and update graph reserves
        // (simplified — full impl parses SPL token account data)
        debug!("Refreshed {} vault accounts", accounts.len());
    }

    Ok(())
}
