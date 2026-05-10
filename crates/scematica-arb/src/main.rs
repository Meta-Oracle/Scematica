use anyhow::Result;
use clap::Parser;
use scematica_arb::{
    executor::ArbExecutor,
    graph::ArbGraph,
    pools::load_pools_from_dir,
    searcher::ArbSearcher,
};
use scematica_core::{
    config::BotConfig,
    metrics::BotMetrics,
    token::{resolve_mint, ui_to_raw},
    types::known_tokens,
    wallet::Wallet,
};
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::commitment_config::CommitmentConfig;
use std::sync::Arc;
use tracing::{error, info};
use tracing_subscriber::EnvFilter;

#[derive(Parser, Debug)]
#[command(name = "scematica-arb", about = "Scematica Cross-DEX Arbitrage Bot")]
struct Args {
    #[arg(short, long)]
    config: Option<String>,

    #[arg(short, long, default_value = "info")]
    log_level: String,

    /// Run in dry-run mode (find arbs but don't execute)
    #[arg(long, default_value = "false")]
    dry_run: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new(&args.log_level)),
        )
        .init();

    info!("╔══════════════════════════════════════╗");
    info!("║    SCEMATICA ARB ENGINE  v{}       ║", env!("CARGO_PKG_VERSION"));
    info!("╚══════════════════════════════════════╝");

    let config = match &args.config {
        Some(path) => BotConfig::from_file(path)?,
        None => BotConfig::from_env()?,
    };

    if !config.arb.enabled {
        info!("Arb engine is disabled in config. Exiting.");
        return Ok(());
    }

    let wallet = Wallet::from_source(&config.wallet.keypair_path)?;
    let wallet_kp = Arc::new(wallet.keypair);
    info!("Wallet: {}", wallet_kp.pubkey());

    let commitment = CommitmentConfig::confirmed();
    let rpc = Arc::new(RpcClient::new_with_commitment(
        config.rpc.endpoint.clone(),
        commitment,
    ));

    let metrics = BotMetrics::new();

    // Build pool graph
    let graph = Arc::new(ArbGraph::new());
    let pool_count = load_pools_from_dir(&config.arb.pool_dir, &graph, &rpc).await?;
    info!("Graph built: {} pools, {} mints", pool_count, graph.mint_count());

    // Resolve start mint
    let start_mint = resolve_mint(&config.arb.start_mint)
        .unwrap_or(known_tokens::USDC_MINT);
    let start_decimals = if config.arb.start_mint.to_uppercase() == "USDC" { 6 } else { 9 };
    let start_amount = ui_to_raw(config.arb.start_amount, start_decimals) as u128;

    info!(
        "Starting arb search: {} ({}) | max_hops={} | min_profit={}",
        config.arb.start_mint,
        start_mint,
        config.arb.max_hops,
        config.arb.min_profit_lamports,
    );

    // Swap program ID (deployed scematica-swap program)
    let swap_program_id = std::env::var("SWAP_PROGRAM_ID")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(solana_sdk::pubkey::Pubkey::default());

    let executor = Arc::new(ArbExecutor::new(
        rpc.clone(),
        wallet_kp.clone(),
        metrics.clone(),
        swap_program_id,
        config.arb.min_profit_lamports,
    ));

    // Metrics reporter
    let metrics_clone = metrics.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(30));
        loop {
            interval.tick().await;
            let snap = metrics_clone.snapshot();
            info!(
                "📊 Arb | Found: {} | Executed: {} | PnL: {:.4} SOL | Uptime: {}s",
                snap.arb_opportunities_found,
                snap.arb_executed,
                snap.total_pnl_sol(),
                snap.uptime_secs,
            );
        }
    });

    info!("🔍 Arb searcher running. Press Ctrl+C to stop.");

    // Main search loop
    let mut interval = tokio::time::interval(tokio::time::Duration::from_millis(500));
    loop {
        interval.tick().await;

        let searcher = ArbSearcher::new((*graph).clone(), config.arb.clone());
        let paths = searcher.search(&start_mint, start_amount);

        if paths.is_empty() {
            continue;
        }

        info!("Found {} arb opportunities", paths.len());

        for path in &paths {
            if args.dry_run {
                info!("[DRY RUN] {}", path);
                continue;
            }

            let executor_clone = executor.clone();
            let path_clone = path.clone();
            tokio::spawn(async move {
                if let Err(e) = executor_clone.execute(&path_clone).await {
                    error!("Arb execution error: {}", e);
                }
            });
        }
    }
}
