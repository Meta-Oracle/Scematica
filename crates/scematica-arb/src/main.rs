use anyhow::Result;
use clap::Parser;
use scematica_ai::agents::AiCoordinator;
use scematica_arb::{
    executor::ArbExecutor,
    graph::ArbGraph,
    pools::load_pools_from_dir,
    searcher::ArbSearcher,
};
use scematica_core::{
    config::BotConfig,
    metrics::BotMetrics,
    token::{get_scema_ata, raw_to_ui, resolve_mint, ui_to_raw},
    types::known_tokens,
    wallet::Wallet,
};
use solana_sdk::{commitment_config::CommitmentConfig, signature::Signer};
use std::sync::Arc;
use tracing::{error, info, warn};
use tracing_subscriber::EnvFilter;

/// Minimum SCEMA balance required to run the arb engine.
const MIN_SCEMA_REQUIRED: f64 = 250_000.0;

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
    let rpc = Arc::new(scematica_core::rpc::RpcConnection::new(
        &config.rpc.endpoint,
        commitment,
    ));

    let metrics = BotMetrics::new();

    // ── SCEMA balance gate ────────────────────────────────────────────────────
    if MIN_SCEMA_REQUIRED > 0.0 {
        let rpc_client = solana_client::nonblocking::rpc_client::RpcClient::new_with_commitment(
            config.rpc.endpoint.clone(),
            CommitmentConfig::confirmed(),
        );
        let scema_ata = get_scema_ata(&wallet_kp.pubkey()); // Token-2022 ATA derivation
        match rpc_client.get_token_account_balance(&scema_ata).await {
            Ok(balance) => {
                let raw: u64 = balance.amount.parse().unwrap_or(0);
                let held = raw_to_ui(raw, known_tokens::SCEMATICA_DECIMALS);
                if held < MIN_SCEMA_REQUIRED {
                    error!(
                        "Insufficient SCEMA balance: {:.2} held, {:.2} required. \
                         Acquire SCEMA (AbKiP2Jc6nM7937jTDfqoJC1bsg5FQ24Buk2iqRFpump) to run the arb engine.",
                        held, MIN_SCEMA_REQUIRED
                    );
                    return Err(anyhow::anyhow!(
                        "SCEMA balance gate: need {:.0} SCEMA, have {:.2}",
                        MIN_SCEMA_REQUIRED, held
                    ));
                }
                info!("✅ SCEMA gate passed: {:.2} SCEMA held (required: {:.0})", held, MIN_SCEMA_REQUIRED);
            }
            Err(_) => {
                warn!(
                    "No SCEMA token account found. \
                     Acquire SCEMA (AbKiP2Jc6nM7937jTDfqoJC1bsg5FQ24Buk2iqRFpump) to run the arb engine."
                );
                return Err(anyhow::anyhow!("SCEMA balance gate: no token account found"));
            }
        }
    }
    // ─────────────────────────────────────────────────────────────────────────

    // AI coordinator
    let ai = AiCoordinator::from_env_optional().map(Arc::new);

    // Build pool graph
    let graph = Arc::new(ArbGraph::new());
    let pool_count = load_pools_from_dir(&config.arb.pool_dir, &graph, &rpc).await?;
    info!("Graph built: {} pools, {} mints", pool_count, graph.mint_count());

    // Graph refresher
    let fetcher = Arc::new(scematica_core::rpc::DexFetcher::new(rpc.clone()));
    let refresher = scematica_arb::refresher::GraphRefresher::new(
        graph.clone(),
        fetcher,
        5000, // 5s refresh
    );
    tokio::spawn(async move {
        refresher.run().await;
    });

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
    let default_program_id: solana_sdk::pubkey::Pubkey =
        "7ycLhn5WsodcbYwV9ecQDd3qWQhKgGzgMK5pc4CYXkEc".parse().unwrap();
    let swap_program_id = std::env::var("SWAP_PROGRAM_ID")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(default_program_id);

    let executor = Arc::new(ArbExecutor::new(
        rpc.clone(),
        wallet_kp.clone(),
        metrics.clone(),
        ai,
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
            metrics_clone.flush_to_file(scematica_core::metrics::METRICS_FILE);
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
