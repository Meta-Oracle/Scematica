use anyhow::Result;
use clap::Parser;
use scematica_core::{
    config::BotConfig,
    metrics::BotMetrics,
    token::{get_ata, raw_to_ui},
    types::known_tokens,
    wallet::Wallet,
};
use scematica_sniper::{
    listener::{ListenerEvent, PoolListener},
    sniper::Sniper,
};
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::{commitment_config::CommitmentConfig, signer::Signer};
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{error, info, warn};
use tracing_subscriber::EnvFilter;

/// Minimum SCEMA balance required to run the sniper.
/// Set to 0 to disable the gate entirely.
const MIN_SCEMA_REQUIRED: f64 = 1000.0;

#[derive(Parser, Debug)]
#[command(name = "scematica-sniper", about = "Scematica Solana Sniper Bot")]
struct Args {
    /// Path to config file (TOML). Falls back to .env if not provided.
    #[arg(short, long)]
    config: Option<String>,

    /// Log level override (e.g. debug, info, warn)
    #[arg(short, long, default_value = "info")]
    log_level: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    // Init tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new(&args.log_level)),
        )
        .init();

    info!("╔══════════════════════════════════════╗");
    info!("║     SCEMATICA SNIPER  v{}          ║", env!("CARGO_PKG_VERSION"));
    info!("╚══════════════════════════════════════╝");

    // Load config
    let config = match &args.config {
        Some(path) => BotConfig::from_file(path)?,
        None => BotConfig::from_env()?,
    };

    if !config.sniper.enabled {
        info!("Sniper is disabled in config. Exiting.");
        return Ok(());
    }

    // Load wallet
    let wallet = Wallet::from_source(&config.wallet.keypair_path)?;
    let wallet_kp = Arc::new(wallet.keypair);
    info!("Wallet: {}", wallet_kp.pubkey());

    // Setup RPC
    let commitment = match config.rpc.commitment.as_str() {
        "finalized" => CommitmentConfig::finalized(),
        "processed" => CommitmentConfig::processed(),
        _ => CommitmentConfig::confirmed(),
    };
    let rpc = Arc::new(RpcClient::new_with_commitment(
        config.rpc.endpoint.clone(),
        commitment,
    ));

    // Resolve quote mint
    let quote_mint = scematica_core::token::resolve_mint(&config.sniper.quote_mint)
        .ok_or_else(|| anyhow::anyhow!("Unknown quote mint: {}", config.sniper.quote_mint))?;

    // Validate wallet has quote token account
    let quote_ata = scematica_core::token::get_ata(&wallet_kp.pubkey(), &quote_mint);
    match rpc.get_token_account_balance(&quote_ata).await {
        Ok(balance) => {
            info!(
                "Quote token ({}) balance: {}",
                config.sniper.quote_mint, balance.ui_amount_string
            );
        }
        Err(_) => {
            error!(
                "No {} token account found. Please create one first.",
                config.sniper.quote_mint
            );
            return Err(anyhow::anyhow!("Missing quote token account"));
        }
    }

    // Init metrics
    let metrics = BotMetrics::new();

    // ── SCEMA balance gate ────────────────────────────────────────────────────
    // Require a minimum SCEMA holding to run the bot.
    if MIN_SCEMA_REQUIRED > 0.0 {
        let scema_ata = get_ata(&wallet_kp.pubkey(), &known_tokens::SCEMATICA_MINT);
        match rpc.get_token_account_balance(&scema_ata).await {
            Ok(balance) => {
                let raw: u64 = balance.amount.parse().unwrap_or(0);
                let held = raw_to_ui(raw, known_tokens::SCEMATICA_DECIMALS);
                if held < MIN_SCEMA_REQUIRED {
                    error!(
                        "Insufficient SCEMA balance: {:.2} held, {:.2} required. \
                         Acquire SCEMA (AbKiP2Jc6nM7937jTDfqoJC1bsg5FQ24Buk2iqRFpump) to run the bot.",
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
                    "No SCEMA token account found for this wallet. \
                     Acquire SCEMA (AbKiP2Jc6nM7937jTDfqoJC1bsg5FQ24Buk2iqRFpump) to run the bot."
                );
                return Err(anyhow::anyhow!("SCEMA balance gate: no token account found"));
            }
        }
    }
    // ─────────────────────────────────────────────────────────────────────────

    // Create sniper
    let sniper = Arc::new(Sniper::new(
        config.sniper.clone(),
        wallet_kp.clone(),
        rpc.clone(),
        metrics.clone(),
    ));

    // Spawn Strategy Agent loop — adjusts TP/SL/amount every 5 minutes
    {
        let sniper_clone = sniper.clone();
        tokio::spawn(async move {
            sniper_clone.run_strategy_loop(300).await;
        });
    }

    // Event channel
    let (event_tx, mut event_rx) = mpsc::channel::<ListenerEvent>(1000);

    // Spawn listener
    let ws_url = config.rpc.ws_endpoint.clone();
    let wallet_pubkey = wallet_kp.pubkey();
    let event_tx_clone = event_tx.clone();
    tokio::spawn(async move {
        loop {
            let listener = PoolListener::new(
                &ws_url,
                wallet_pubkey,
                quote_mint,
                event_tx_clone.clone(),
            );
            if let Err(e) = listener.run().await {
                error!("Listener error: {}. Reconnecting in 5s...", e);
                tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
            }
        }
    });

    // Spawn metrics reporter
    let metrics_clone = metrics.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(30));
        loop {
            interval.tick().await;
            let snap = metrics_clone.snapshot();
            info!(
                "📊 Metrics | Trades: {}/{} confirmed | Arbs: {} | PnL: {:.4} SOL | Uptime: {}s",
                snap.trades_confirmed,
                snap.trades_attempted,
                snap.arb_executed,
                snap.total_pnl_sol(),
                snap.uptime_secs,
            );
            metrics_clone.flush_to_file(scematica_core::metrics::METRICS_FILE);
        }
    });

    info!("🚀 Sniper is running. Press Ctrl+C to stop.");

    // Main event loop
    while let Some(event) = event_rx.recv().await {
        let sniper_clone = sniper.clone();
        tokio::spawn(async move {
            sniper_clone.handle_event(event).await;
        });
    }

    Ok(())
}
