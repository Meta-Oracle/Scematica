use anyhow::{Context, Result};
use clap::Parser;
use serde::{Deserialize, Serialize};
use std::path::Path;
use tracing::{info, warn};

#[derive(Parser)]
#[command(name = "pool-seeder", about = "Fetch live pool metadata for the Scematica arb engine")]
struct Args {
    /// Output directory for pool JSON files
    #[arg(short, long, default_value = "pools")]
    output: String,

    /// Minimum pool liquidity in USD to include
    #[arg(long, default_value = "50000")]
    min_liquidity: f64,

    /// Maximum pools per DEX to fetch
    #[arg(long, default_value = "500")]
    limit: usize,
}

/// The pool JSON format consumed by scematica-arb's pools.rs
#[derive(Debug, Serialize, Deserialize)]
struct PoolJson {
    address: String,
    dex: String,
    token_a_mint: String,
    token_b_mint: String,
    token_a_vault: String,
    token_b_vault: String,
    fee_numerator: u64,
    fee_denominator: u64,
}

// ─── Raydium ─────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct RaydiumPoolsResponse {
    data: RaydiumPoolsData,
}

#[derive(Debug, Deserialize)]
struct RaydiumPoolsData {
    data: Vec<RaydiumPool>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RaydiumPool {
    id: String,
    base_mint: String,
    quote_mint: String,
    lp_vault: String,
    #[serde(default)]
    tvl: f64,
}

async fn fetch_raydium(client: &reqwest::Client, limit: usize, min_liquidity: f64) -> Result<Vec<PoolJson>> {
    // Raydium V3 pools API — returns AMM V4 pools sorted by liquidity
    let url = format!(
        "https://api-v3.raydium.io/pools/info/list?poolType=standard&poolSortField=liquidity&sortType=desc&pageSize={}&page=1",
        limit
    );

    let resp: serde_json::Value = client
        .get(&url)
        .send()
        .await
        .context("Raydium API request failed")?
        .json()
        .await
        .context("Raydium API JSON parse failed")?;

    let pools = resp["data"]["data"]
        .as_array()
        .cloned()
        .unwrap_or_default();

    let mut result = Vec::new();
    for p in pools {
        let tvl = p["tvl"].as_f64().unwrap_or(0.0);
        if tvl < min_liquidity {
            continue;
        }

        let id = p["id"].as_str().unwrap_or("").to_string();
        let base_mint = p["mintA"]["address"].as_str().unwrap_or("").to_string();
        let quote_mint = p["mintB"]["address"].as_str().unwrap_or("").to_string();
        // Raydium V3 API provides vault addresses directly
        let vault_a = p["vaultA"]["address"].as_str().unwrap_or("").to_string();
        let vault_b = p["vaultB"]["address"].as_str().unwrap_or("").to_string();

        if id.is_empty() || base_mint.is_empty() || quote_mint.is_empty()
            || vault_a.is_empty() || vault_b.is_empty()
        {
            continue;
        }

        result.push(PoolJson {
            address: id,
            dex: "Raydium".into(),
            token_a_mint: base_mint,
            token_b_mint: quote_mint,
            token_a_vault: vault_a,
            token_b_vault: vault_b,
            fee_numerator: 25,       // Raydium standard: 0.25%
            fee_denominator: 10_000,
        });
    }

    Ok(result)
}

// ─── Orca ─────────────────────────────────────────────────────────────────────

async fn fetch_orca(client: &reqwest::Client, limit: usize, min_liquidity: f64) -> Result<Vec<PoolJson>> {
    // Orca Whirlpool API
    let url = "https://api.mainnet.orca.so/v1/whirlpool/list";

    let resp: serde_json::Value = client
        .get(url)
        .send()
        .await
        .context("Orca API request failed")?
        .json()
        .await
        .context("Orca API JSON parse failed")?;

    let pools = resp["whirlpools"]
        .as_array()
        .cloned()
        .unwrap_or_default();

    // Sort by tvl descending, take top `limit`
    let mut pools_with_tvl: Vec<(f64, &serde_json::Value)> = pools
        .iter()
        .map(|p| (p["tvl"].as_f64().unwrap_or(0.0), p))
        .collect();
    pools_with_tvl.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

    let mut result = Vec::new();
    for (tvl, p) in pools_with_tvl.into_iter().take(limit) {
        if tvl < min_liquidity {
            continue;
        }

        let address = p["address"].as_str().unwrap_or("").to_string();
        let token_a = p["tokenA"]["mint"].as_str().unwrap_or("").to_string();
        let token_b = p["tokenB"]["mint"].as_str().unwrap_or("").to_string();
        let vault_a = p["tokenVaultA"].as_str().unwrap_or("").to_string();
        let vault_b = p["tokenVaultB"].as_str().unwrap_or("").to_string();
        // fee_rate is in hundredths of a basis point (e.g. 3000 = 0.3%)
        let fee_rate = p["feeRate"].as_u64().unwrap_or(3000);

        if address.is_empty() || token_a.is_empty() || token_b.is_empty()
            || vault_a.is_empty() || vault_b.is_empty()
        {
            continue;
        }

        result.push(PoolJson {
            address,
            dex: "Orca".into(),
            token_a_mint: token_a,
            token_b_mint: token_b,
            token_a_vault: vault_a,
            token_b_vault: vault_b,
            fee_numerator: fee_rate,
            fee_denominator: 1_000_000,
        });
    }

    Ok(result)
}

// ─── Meteora ──────────────────────────────────────────────────────────────────

async fn fetch_meteora(client: &reqwest::Client, limit: usize, min_liquidity: f64) -> Result<Vec<PoolJson>> {
    // Meteora DLMM pairs API
    let url = format!(
        "https://dlmm-api.meteora.ag/pair/all_with_pagination?limit={}&sort_key=liquidity&order_by=desc",
        limit
    );

    let resp: serde_json::Value = client
        .get(&url)
        .send()
        .await
        .context("Meteora API request failed")?
        .json()
        .await
        .context("Meteora API JSON parse failed")?;

    let pairs = resp["pairs"]
        .as_array()
        .cloned()
        .unwrap_or_default();

    let mut result = Vec::new();
    for p in pairs {
        let liquidity = p["liquidity"].as_f64().unwrap_or(0.0);
        if liquidity < min_liquidity {
            continue;
        }

        let address = p["address"].as_str().unwrap_or("").to_string();
        let token_x = p["mint_x"].as_str().unwrap_or("").to_string();
        let token_y = p["mint_y"].as_str().unwrap_or("").to_string();
        let reserve_x = p["reserve_x"].as_str().unwrap_or("").to_string();
        let reserve_y = p["reserve_y"].as_str().unwrap_or("").to_string();
        // base_fee_percentage is a string like "0.1" (= 0.1%)
        let fee_pct: f64 = p["base_fee_percentage"]
            .as_str()
            .and_then(|s| s.parse().ok())
            .unwrap_or(0.1);
        let fee_numerator = (fee_pct * 100.0) as u64; // e.g. 0.1% → 10 / 10_000

        if address.is_empty() || token_x.is_empty() || token_y.is_empty()
            || reserve_x.is_empty() || reserve_y.is_empty()
        {
            continue;
        }

        result.push(PoolJson {
            address,
            dex: "Meteora".into(),
            token_a_mint: token_x,
            token_b_mint: token_y,
            token_a_vault: reserve_x,
            token_b_vault: reserve_y,
            fee_numerator,
            fee_denominator: 10_000,
        });
    }

    Ok(result)
}

// ─── Writer ───────────────────────────────────────────────────────────────────

fn write_pools(pools: &[PoolJson], dir: &str, dex: &str) -> Result<usize> {
    let dex_dir = Path::new(dir).join(dex.to_lowercase());
    std::fs::create_dir_all(&dex_dir)?;

    // Write one JSON file per pool, named by pool address
    for pool in pools {
        let path = dex_dir.join(format!("{}.json", pool.address));
        let json = serde_json::to_string_pretty(pool)?;
        std::fs::write(&path, json)?;
    }

    Ok(pools.len())
}

// ─── Main ─────────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::new("info"))
        .init();

    let args = Args::parse();

    std::fs::create_dir_all(&args.output)?;

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .user_agent("scematica-pool-seeder/0.1")
        .build()?;

    info!("Fetching Raydium pools (min_liquidity=${}, limit={})...", args.min_liquidity, args.limit);
    match fetch_raydium(&client, args.limit, args.min_liquidity).await {
        Ok(pools) => {
            let n = write_pools(&pools, &args.output, "Raydium")?;
            info!("✓ Raydium: wrote {} pools", n);
        }
        Err(e) => warn!("✗ Raydium fetch failed: {}", e),
    }

    info!("Fetching Orca pools...");
    match fetch_orca(&client, args.limit, args.min_liquidity).await {
        Ok(pools) => {
            let n = write_pools(&pools, &args.output, "Orca")?;
            info!("✓ Orca: wrote {} pools", n);
        }
        Err(e) => warn!("✗ Orca fetch failed: {}", e),
    }

    info!("Fetching Meteora pools...");
    match fetch_meteora(&client, args.limit, args.min_liquidity).await {
        Ok(pools) => {
            let n = write_pools(&pools, &args.output, "Meteora")?;
            info!("✓ Meteora: wrote {} pools", n);
        }
        Err(e) => warn!("✗ Meteora fetch failed: {}", e),
    }

    info!("Pool seeding complete. Output: {}", args.output);
    Ok(())
}
