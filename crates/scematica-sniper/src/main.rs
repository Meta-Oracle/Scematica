use anyhow::Result;
use clap::Parser;
use scematica_core::{
    config::BotConfig,
    metrics::BotMetrics,
    token::raw_to_ui,
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
const MIN_SCEMA_REQUIRED: f64 = 250_000.0;

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

    // Init tracing — write to stderr AND scematica-sniper.log so the dashboard can tail it
    {
        use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
        let filter = EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| EnvFilter::new(&args.log_level));
        let stderr_layer = tracing_subscriber::fmt::layer()
            .with_writer(std::io::stderr);
        let file_appender = tracing_appender::rolling::never(".", "scematica-sniper.log");
        let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);
        let file_layer = tracing_subscriber::fmt::layer()
            .with_ansi(false)
            .with_writer(non_blocking);
        tracing_subscriber::registry()
            .with(filter)
            .with(stderr_layer)
            .with(file_layer)
            .init();
        // _guard must live for the process lifetime — bind it to a local that isn't dropped
        std::mem::forget(_guard);
    }

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
    // Uses get_token_accounts_by_owner to find the SCEMA account regardless of
    // which token program (legacy vs Token-2022) owns it — avoids ATA address
    // derivation issues with Token-2022 mints like SCEMA.
    if MIN_SCEMA_REQUIRED > 0.0 {
        use solana_client::rpc_request::TokenAccountsFilter;

        let mut gate_passed = false;
        for attempt in 1..=5 {
            match rpc.get_token_accounts_by_owner(
                &wallet_kp.pubkey(),
                TokenAccountsFilter::Mint(known_tokens::SCEMATICA_MINT),
            ).await {
                Ok(accounts) => {
                    let mut held = 0.0f64;
                    for keyed in &accounts {
                        if let Ok(pk) = keyed.pubkey.parse::<solana_sdk::pubkey::Pubkey>() {
                            if let Ok(bal) = rpc.get_token_account_balance(&pk).await {
                                held += raw_to_ui(
                                    bal.amount.parse().unwrap_or(0),
                                    known_tokens::SCEMATICA_DECIMALS,
                                );
                            }
                        }
                    }

                    if held < MIN_SCEMA_REQUIRED {
                        error!(
                            "Insufficient SCEMA: {:.0} held, {:.0} required.",
                            held, MIN_SCEMA_REQUIRED
                        );
                        return Err(anyhow::anyhow!(
                            "SCEMA balance gate: need {:.0}, have {:.0}",
                            MIN_SCEMA_REQUIRED, held
                        ));
                    }
                    info!("✅ SCEMA gate passed: {:.0} SCEMA held (required: {:.0})", held, MIN_SCEMA_REQUIRED);
                    gate_passed = true;
                    break;
                }
                Err(e) => {
                    warn!("SCEMA gate attempt {}/5 failed: {} — retrying in 3s...", attempt, e);
                    tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;
                }
            }
        }
        if !gate_passed {
            error!(
                "SCEMA gate: could not verify after 5 attempts. \
                 Set SCEMATICA_SKIP_GATE=1 to bypass if RPC is degraded."
            );
            if std::env::var("SCEMATICA_SKIP_GATE").as_deref() != Ok("1") {
                return Err(anyhow::anyhow!("SCEMA gate: RPC verification failed"));
            }
            warn!("SCEMATICA_SKIP_GATE=1 set — proceeding without SCEMA verification");
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

    // Scan for tokens already held from a previous run and spawn sell monitors
    {
        let sniper_scan = sniper.clone();
        tokio::spawn(async move {
            sniper_scan.scan_existing_positions().await;
        });
    }

    // Sell-mode file watcher — checks for scematica-sell-mode.json every 5 s.
    // When the file appears (written by the dashboard or manually), pauses all buys
    // and triggers an immediate sell scan for every token in the wallet.
    {
        use std::sync::atomic::Ordering;
        let sniper_sm = sniper.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(5));
            let mut was_active = false;
            loop {
                interval.tick().await;
                let active = std::path::Path::new("scematica-sell-mode.json").exists();
                sniper_sm.sell_mode.store(active, Ordering::Relaxed);
                if active && !was_active {
                    warn!("🚨 SELL MODE activated — pausing buys and force-selling all positions");
                    let sniper_ref = sniper_sm.clone();
                    tokio::spawn(async move {
                        sniper_ref.scan_existing_positions().await;
                    });
                } else if !active && was_active {
                    info!("✅ Sell mode deactivated — resuming normal operation");
                }
                was_active = active;
            }
        });
    }

    // Dump-mode file watcher — checks for scematica-dump-mode.json every 5 s.
    // When active: sets dump_mode flag (min_out=0 on all sells) and calls auto_dump()
    // to immediately force-sell every token position in the wallet.
    {
        use std::sync::atomic::Ordering;
        let sniper_dm = sniper.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(5));
            let mut was_active = false;
            loop {
                interval.tick().await;
                let active = std::path::Path::new("scematica-dump-mode.json").exists();
                sniper_dm.dump_mode.store(active, Ordering::Relaxed);
                if active && !was_active {
                    warn!("💥 DUMP MODE activated — force-selling ALL positions with zero slippage");
                    let sniper_ref = sniper_dm.clone();
                    tokio::spawn(async move {
                        sniper_ref.auto_dump().await;
                    });
                } else if !active && was_active {
                    info!("✅ Dump mode deactivated");
                }
                was_active = active;
            }
        });
    }

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
        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(5));
        let mut last_log = std::time::Instant::now();
        loop {
            interval.tick().await;
            metrics_clone.flush_to_file(scematica_core::metrics::METRICS_FILE);
            // Log summary every 30s to avoid spam, but flush the file every 5s
            if last_log.elapsed().as_secs() >= 30 {
                let snap = metrics_clone.snapshot();
                info!(
                    "📊 Metrics | Trades: {}/{} confirmed | PnL: {:.4} SOL | Uptime: {}s",
                    snap.trades_confirmed,
                    snap.trades_attempted,
                    snap.total_pnl_sol(),
                    snap.uptime_secs,
                );
                last_log = std::time::Instant::now();
            }
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
