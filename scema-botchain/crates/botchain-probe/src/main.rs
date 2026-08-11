//! `botchain-probe` — measure whether a sniper is viable on BOT Chain, before writing one.
//!
//! Two questions the port hangs on, neither answerable from documentation:
//!
//! 1. **Can we reach the chain at all**, and from which endpoint? The official RPC was
//!    unreachable from some networks during testing while the explorer proxy answered.
//! 2. **Is there any pool-creation flow?** A new-pool sniper with no new pools is an
//!    expensive way to idle. This counts every event emitted by each DEX factory over a
//!    recent window — by address, not by a guessed event signature — and extrapolates a
//!    daily rate.
//!
//! It reports what it measured and refuses to round zero up to "promising".
//!
//!   cargo run -p botchain-probe -- --blocks 5000
//!
//!   BOTCHAIN_NETWORK=testnet   pick the network (default mainnet)
//!   BOTCHAIN_RPC_URL=https://… try a private node first

use anyhow::Result;
use botchain_core::{chain::MAINNET_VENUES, client_from_env};
use std::collections::BTreeMap;

/// `eth_getLogs` window. Public nodes commonly reject anything much wider.
const CHUNK: u64 = 500;

/// Blocks scanned by default. At ~0.67s that is roughly an hour of history.
const DEFAULT_BLOCKS: u64 = 5_000;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "botchain_probe=info,botchain_core=warn".into()),
        )
        .without_time()
        .init();

    let blocks: u64 = std::env::args()
        .skip_while(|a| a != "--blocks")
        .nth(1)
        .and_then(|v| v.parse().ok())
        .unwrap_or(DEFAULT_BLOCKS);

    let client = client_from_env()?;
    let net = client.network();

    println!("── {} (chain {}) ──────────────────────────────", net.name, net.chain_id);

    // Verify before trusting anything else. Chain ids are not unique; the check that
    // means something is "this endpoint reports the id I expect".
    let verified = client.verify().await?;
    println!("  reachable via : {}", verified.endpoint);
    println!("  endpoint kind : {:?}  ({} ms)", verified.kind, verified.elapsed.as_millis());

    let head = client.block_number().await?;
    let gas = client.gas_price().await?;
    println!("  head block    : {head}");
    println!("  gas price     : {:.2} gwei", gas as f64 / 1e9);

    // ── pool-creation flow ────────────────────────────────────────────────────
    //
    // Filtered by **factory address with no topic filter**, deliberately. Filtering by a
    // guessed event signature answers "does this fork emit the event I assumed", not
    // "does anything get created here" — and the first version of this probe reported a
    // confident zero for exactly that reason, because the venues are V3-style and emit
    // PoolCreated rather than the V2 PairCreated it was watching for. Address-only
    // filtering cannot be wrong about the fork.
    let from = head.saturating_sub(blocks);
    println!("
  scanning blocks {from}..{head} across {} venues …", MAINNET_VENUES.len());

    let mut scanned = 0u64;
    let mut refused = 0u32;
    let mut per_venue: BTreeMap<&str, usize> = BTreeMap::new();
    let mut topics: BTreeMap<String, usize> = BTreeMap::new();

    for venue in MAINNET_VENUES {
        let mut start = from;
        let mut venue_scanned = 0u64;
        while start < head {
            let end = (start + CHUNK).min(head);
            match client.logs_for_address(venue.factory, start, end).await {
                Ok(logs) => {
                    venue_scanned += end - start;
                    *per_venue.entry(venue.name).or_insert(0) += logs.len();
                    for log in &logs {
                        if let Some(t) = log
                            .get("topics")
                            .and_then(|t| t.as_array())
                            .and_then(|a| a.first())
                            .and_then(|t| t.as_str())
                        {
                            *topics.entry(t.to_string()).or_insert(0) += 1;
                        }
                    }
                }
                Err(e) => {
                    refused += 1;
                    if refused <= 3 {
                        eprintln!("    {} range {start}..{end} refused: {e}", venue.name);
                    }
                }
            }
            start = end;
        }
        scanned = scanned.max(venue_scanned);
    }

    let total: usize = per_venue.values().sum();

    println!("
── result ──────────────────────────────────────────────");
    println!("  blocks scanned    : {scanned} (of {blocks} requested, {refused} ranges refused)");
    println!("  factory events    : {total}");
    for (name, n) in &per_venue {
        println!("    {n:>5}  {name}");
    }
    if !topics.is_empty() {
        println!("  event signatures seen:");
        for (t, n) in &topics {
            println!("    {n:>5}  {t}");
        }
    }

    if scanned == 0 {
        println!("
  Nothing was scanned — every range was refused. That is a measurement");
        println!("  failure, not a finding about the chain. Try a smaller --blocks.");
        return Ok(());
    }

    // ~0.67s blocks, measured.
    let per_day = total as f64 / scanned as f64 * (86_400.0 / 0.67);
    println!("  implied rate      : {per_day:.2} pool creations/day");

    println!();
    if total == 0 {
        println!("  No pool creation at all in this window, on any venue. A new-pool sniper");
        println!("  has nothing to act on. Measured August 2026: 2 events across ~1,000,000");
        println!("  blocks (~8 days), and zero over the most recent 200,000. Re-run this");
        println!("  before deciding the port is worth building — it is the whole point of");
        println!("  this binary, and the answer is allowed to change.");
    } else if per_day < 5.0 {
        println!("  Still far below the flow the Solana side trades, where the edge is");
        println!("  measured. Treat these as something to study, not to trade.");
    }

    Ok(())
}
