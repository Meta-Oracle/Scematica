use anyhow::Result;
use clap::Parser;
use scematica_nn::{AgentStats, DQNAgent, TradeAction, TradeState as NNState};
use scematica_core::{
    config::BotConfig,
    metrics::BotMetrics,
    token::raw_to_ui,
    types::known_tokens,
    wallet::Wallet,
};
use scematica_sniper::{
    alerts::AlertManager,
    listener::{ListenerEvent, PoolListener},
    sniper::Sniper,
};
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::{commitment_config::CommitmentConfig, signer::Signer};
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};
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

    // Ensure the quote token ATA exists. For WSOL we create it automatically —
    // each buy already funds it via transfer+SyncNative, so the account just needs
    // to exist. For other quote mints (USDC) a missing ATA is a real config error.
    let quote_ata = scematica_core::token::get_ata(&wallet_kp.pubkey(), &quote_mint);
    match rpc.get_token_account_balance(&quote_ata).await {
        Ok(balance) => {
            info!(
                "Quote token ({}) balance: {}",
                config.sniper.quote_mint, balance.ui_amount_string
            );
        }
        Err(_) => {
            use scematica_core::types::known_tokens;
            if quote_mint == known_tokens::WSOL_MINT {
                // Auto-create the WSOL ATA — the buy flow wraps SOL into it on demand.
                info!("WSOL ATA not found — creating it now");
                let create_ix =
                    spl_associated_token_account::instruction::create_associated_token_account_idempotent(
                        &wallet_kp.pubkey(),
                        &wallet_kp.pubkey(),
                        &known_tokens::WSOL_MINT,
                        &spl_token::id(),
                    );
                let blockhash = rpc.get_latest_blockhash().await?;
                let tx = solana_sdk::transaction::Transaction::new_signed_with_payer(
                    &[create_ix],
                    Some(&wallet_kp.pubkey()),
                    &[wallet_kp.as_ref()],
                    blockhash,
                );
                match rpc.send_and_confirm_transaction(&tx).await {
                    Ok(sig) => info!("WSOL ATA created: {}", sig),
                    Err(e) => warn!("WSOL ATA creation failed (may already exist): {}", e),
                }
            } else {
                error!(
                    "No {} token account found. Please create one first.",
                    config.sniper.quote_mint
                );
                return Err(anyhow::anyhow!("Missing quote token account"));
            }
        }
    }

    // Init metrics
    let metrics = BotMetrics::new();

    // Build alert manager from config (Telegram / Discord / desktop toast)
    let alerts = Arc::new(AlertManager::new(config.alerts.clone()));

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
        alerts.clone(),
    ));

    // Record session start balance for drawdown tracking
    if let Ok(start_bal) = rpc.get_balance(&wallet_kp.pubkey()).await {
        use std::sync::atomic::Ordering;
        sniper.session_start_lamports.store(start_bal, Ordering::Relaxed);
        info!("Session start balance: {:.4} SOL ({} lamports)", start_bal as f64 / 1e9, start_bal);
    }

    // Load persisted pool cache so previous-run tokens can be sold immediately
    sniper.load_pool_cache("pool-cache.json");

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
                    // Reset buy counter so the next batch starts fresh
                    sniper_sm.buy_count.store(0, std::sync::atomic::Ordering::Relaxed);
                    info!("✅ Sell mode deactivated — buy counter reset, resuming normal operation");
                }
                was_active = active;
            }
        });
    }

    // Dump-mode file watcher — checks scematica-dump-mode.json every 5 s.
    // On activation: sets dump_mode (min_out=0) and calls auto_dump immediately.
    // While still active: re-calls auto_dump every 30 s so positions that failed
    // the first time (e.g. pool lookup timeout) are retried automatically.
    {
        use std::sync::atomic::Ordering;
        let sniper_dm = sniper.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(5));
            let mut was_active = false;
            let mut active_ticks = 0u32;
            loop {
                interval.tick().await;
                let active = std::path::Path::new("scematica-dump-mode.json").exists();
                sniper_dm.dump_mode.store(active, Ordering::Relaxed);
                if active {
                    active_ticks += 1;
                    let first = !was_active;
                    // Fire immediately on activation, then every 6 ticks (30 s)
                    if first || active_ticks % 6 == 0 {
                        if first {
                            warn!("💥 DUMP MODE activated — force-selling ALL positions with zero slippage");
                        } else {
                            warn!("AUTO DUMP: retrying unsold positions (tick {})", active_ticks);
                        }
                        let sniper_ref = sniper_dm.clone();
                        tokio::spawn(async move {
                            sniper_ref.auto_dump().await;
                        });
                    }
                } else {
                    if was_active {
                        info!("✅ Dump mode deactivated");
                    }
                    active_ticks = 0;
                }
                was_active = active;
            }
        });
    }

    // Rate-mode file watcher — checks scematica-rate-mode.json every 5 s.
    // When the dashboard sets a new rate mode the sniper picks it up and overrides
    // live_params immediately, letting the strategy agent continue from there.
    {
        let sniper_rm = sniper.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(5));
            let mut last_mode = String::new();
            loop {
                interval.tick().await;
                let Ok(data) = std::fs::read_to_string("scematica-rate-mode.json") else { continue };
                let Ok(v) = serde_json::from_str::<serde_json::Value>(&data) else { continue };
                let mode_str = v["mode"].as_str().unwrap_or("").to_string();
                if mode_str == last_mode { continue; }
                last_mode = mode_str.clone();
                let mut params = sniper_rm.live_params.write();
                if let Some(tp)   = v["tp_pct"].as_f64()    { params.take_profit_pct  = tp; }
                if let Some(sl)   = v["sl_pct"].as_f64()    { params.stop_loss_pct    = sl; }
                if let Some(mult) = v["multiplier"].as_f64() { params.amount_multiplier = mult; }
                info!(
                    "⚡ Rate mode → {}  |  {:.1}x  TP {:.0}%  SL {:.0}%",
                    mode_str, params.amount_multiplier, params.take_profit_pct, params.stop_loss_pct
                );
            }
        });
    }

    // Pool cache persistence — flush to disk every 60 s so sell lookups survive crashes
    {
        let sniper_pc = sniper.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(60));
            loop {
                interval.tick().await;
                sniper_pc.persist_pool_cache("pool-cache.json");
            }
        });
    }

    // Max drawdown guard — activates sell mode if wallet drops > max_drawdown_pct from session start
    {
        use std::sync::atomic::Ordering;
        let sniper_dd = sniper.clone();
        let rpc_dd = rpc.clone();
        let wallet_pk = wallet_kp.pubkey();
        let max_dd = config.sniper.max_drawdown_pct;
        tokio::spawn(async move {
            if max_dd <= 0.0 { return; }
            let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(30));
            loop {
                interval.tick().await;
                let start = sniper_dd.session_start_lamports.load(Ordering::Relaxed);
                if start == 0 { continue; }
                if let Ok(current) = rpc_dd.get_balance(&wallet_pk).await {
                    if current < start {
                        let drawdown = (start - current) as f64 / start as f64 * 100.0;
                        if drawdown >= max_dd {
                            warn!(
                                "🔴 Max drawdown {:.1}% reached (start={:.4} SOL, now={:.4} SOL) — activating sell mode",
                                drawdown, start as f64 / 1e9, current as f64 / 1e9
                            );
                            sniper_dd.sell_mode.store(true, Ordering::Relaxed);
                            let _ = std::fs::write(
                                "scematica-sell-mode.json",
                                r#"{"active":true,"reason":"max_drawdown"}"#,
                            );
                        }
                    }
                }
            }
        });
    }

    // Daily PnL midnight reset — zeroes the daily loss accumulator each UTC midnight
    {
        tokio::spawn({
            let sniper_pr = sniper.clone();
            async move {
                loop {
                    let now = chrono::Utc::now();
                    let tomorrow = now.date_naive().succ_opt().unwrap_or(now.date_naive());
                    let next_midnight = tomorrow
                        .and_hms_opt(0, 0, 1)
                        .map(|ndt| ndt.and_utc())
                        .unwrap_or_else(|| now + chrono::Duration::hours(24));
                    let secs = (next_midnight - now).num_seconds().max(60) as u64;
                    tokio::time::sleep(tokio::time::Duration::from_secs(secs)).await;
                    *sniper_pr.daily_pnl_lamports.lock() = 0;
                    info!("📅 Daily PnL accumulator reset at UTC midnight");
                }
            }
        });
    }

    // Config hot-reload — polls config.toml mtime every 30 s; reloads TP/SL/buy_amount into live_params
    {
        let sniper_hr = sniper.clone();
        let config_path = args.config.clone().unwrap_or_else(|| "config.toml".into());
        tokio::spawn(async move {
            let mut last_mtime: Option<std::time::SystemTime> = None;
            let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(30));
            loop {
                interval.tick().await;
                let Ok(meta) = std::fs::metadata(&config_path) else { continue };
                let Ok(mtime) = meta.modified() else { continue };
                if last_mtime == Some(mtime) { continue; }
                last_mtime = Some(mtime);
                // Reload and update only live parameters (no restart required)
                match scematica_core::config::BotConfig::from_file(&config_path) {
                    Ok(new_cfg) => {
                        let mut params = sniper_hr.live_params.write();
                        params.take_profit_pct  = new_cfg.sniper.take_profit_pct;
                        params.stop_loss_pct    = new_cfg.sniper.stop_loss_pct;
                        // amount_multiplier intentionally not reset — rate mode owns it
                        info!(
                            "🔄 Config hot-reloaded: TP {:.1}%  SL {:.1}%",
                            params.take_profit_pct, params.stop_loss_pct
                        );
                    }
                    Err(e) => warn!("Config hot-reload parse error: {} — keeping current params", e),
                }
            }
        });
    }

    // ── Deep Q* Neural Network agent ────────────────────────────────────────
    // Observer mode: learns from completed trades by polling scematica-trades.jsonl.
    // Once epsilon < 0.5 (ready_to_advise=true) it can gate buy decisions.
    {
        use std::sync::{Arc, Mutex};
        let nn_agent: Arc<Mutex<DQNAgent>> = Arc::new(Mutex::new(
            match DQNAgent::load("scematica-nn-agent.json") {
                Ok(a) => { info!("🧠 NN agent loaded from checkpoint"); a }
                Err(_) => { info!("🧠 NN agent initialised fresh (STATE_DIM={}, ACTIONS=5)", scematica_nn::STATE_DIM); DQNAgent::new() }
            }
        ));

        // Observer task: polls scematica-trades.jsonl for new SELL events, extracts
        // reward from PnL, constructs (state, action, reward, next_state) tuples, trains.
        {
            let agent = Arc::clone(&nn_agent);
            tokio::spawn(async move {
                let mut last_seen: usize = 0;
                let mut consecutive_wins: i32  = 0;
                let mut consecutive_losses: i32 = 0;
                let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(5));
                loop {
                    interval.tick().await;
                    let Ok(raw) = std::fs::read_to_string("scematica-trades.jsonl") else { continue };
                    let lines: Vec<&str> = raw.lines().collect();
                    if lines.len() <= last_seen { continue; }

                    for line in &lines[last_seen..] {
                        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else { continue };
                        if v["action"].as_str() != Some("SELL") { continue; }

                        let pnl_sol = v["pnl_sol"].as_f64().unwrap_or(0.0);
                        let pnl_pct = v["pnl_pct"].as_f64().unwrap_or(0.0);
                        let age_secs = v["position_age_secs"].as_f64().unwrap_or(0.0);

                        // Update streak tracking
                        if pnl_sol > 0.0 {
                            consecutive_wins  += 1;
                            consecutive_losses = 0;
                        } else {
                            consecutive_losses += 1;
                            consecutive_wins   = 0;
                        }

                        let state = NNState::from_trade_fields(
                            pnl_pct, age_secs, 0.0, consecutive_wins, consecutive_losses, 0.0, 0,
                        );
                        let action = if v["action"].as_str() == Some("SELL") {
                            TradeAction::SellAll
                        } else {
                            TradeAction::BuyStandard
                        };
                        let reward = DQNAgent::shape_reward(pnl_pct, (age_secs / 60.0) as u32);
                        let next_state = NNState::from_trade_fields(
                            0.0, 0.0, 0.0, consecutive_wins, consecutive_losses, 0.0, 0,
                        );

                        if let Ok(mut ag) = agent.lock() {
                            ag.observe(state, action, reward, next_state, true);
                            if let Some(loss) = ag.train_step() {
                                debug!("NN train loss={:.6}", loss);
                            }
                        }
                    }
                    last_seen = lines.len();
                }
            });
        }

        // Stats flush task: writes scematica-nn-stats.json every 30s for the dashboard.
        {
            let agent = Arc::clone(&nn_agent);
            tokio::spawn(async move {
                let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(30));
                loop {
                    interval.tick().await;
                    if let Ok(ag) = agent.lock() {
                        let stats: AgentStats = ag.stats();
                        let _ = std::fs::write(
                            "scematica-nn-stats.json",
                            serde_json::to_string(&stats).unwrap(),
                        );
                    }
                }
            });
        }

        // Checkpoint save task: persists weights every 10 minutes.
        {
            let agent = Arc::clone(&nn_agent);
            tokio::spawn(async move {
                let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(600));
                loop {
                    interval.tick().await;
                    if let Ok(ag) = agent.lock() {
                        match ag.save("scematica-nn-agent.json") {
                            Ok(_)  => info!("🧠 NN agent checkpoint saved"),
                            Err(e) => tracing::warn!("NN checkpoint save failed: {}", e),
                        }
                    }
                }
            });
        }

        // Regime-shift polling: checks for the signal file written by run_strategy_loop,
        // spikes epsilon on the agent so it re-explores in the new market regime.
        {
            let agent = Arc::clone(&nn_agent);
            tokio::spawn(async move {
                let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(10));
                loop {
                    interval.tick().await;
                    if DQNAgent::poll_regime_shift_file("scematica-regime-shift.json") {
                        if let Ok(mut ag) = agent.lock() {
                            ag.notify_regime_shift();
                        }
                    }
                }
            });
        }
    }
    // ─────────────────────────────────────────────────────────────────────────

    // Spawn Strategy Agent loop — adjusts TP/SL/amount every 5 minutes
    {
        let sniper_clone = sniper.clone();
        tokio::spawn(async move {
            sniper_clone.run_strategy_loop(300).await;
        });
    }

    // Event channel
    let (event_tx, mut event_rx) = mpsc::channel::<ListenerEvent>(1000);

    // Copy-trade whale listener — subscribes to configured copy_wallets over WebSocket
    // and emits NewPool events whenever they buy into a Raydium pool
    if !config.sniper.copy_wallets.is_empty() {
        use scematica_sniper::whale_copy::WhaleCopyListener;
        let wc_tx = event_tx.clone();
        let wc_wallets = config.sniper.copy_wallets.clone();
        let wc_ws = config.rpc.ws_endpoint.clone();
        tokio::spawn(async move {
            let listener = WhaleCopyListener::new(wc_ws, wc_wallets, wc_tx);
            loop {
                if let Err(e) = listener.run().await {
                    warn!("Whale copy listener error: {} — reconnecting in 10s", e);
                    tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;
                }
            }
        });
        info!("👁 Whale copy listener started ({} wallets)", config.sniper.copy_wallets.len());
    }

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

    // ── ATH tracker: update every 30s, log new ATH ───────────────────────────
    {
        let sniper_ath = sniper.clone();
        let rpc_ath = rpc.clone();
        let wallet_pk_ath = wallet_kp.pubkey();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(30));
            let mut last_ath: u64 = 0;
            loop {
                interval.tick().await;
                if let Ok(balance) = rpc_ath.get_balance(&wallet_pk_ath).await {
                    sniper_ath.ath_tracker.update(balance);
                    let ath = sniper_ath.ath_tracker.ath();
                    if ath > last_ath {
                        info!(
                            "🏆 New ATH balance: {:.4} SOL ({} lamports)",
                            ath as f64 / 1e9, ath
                        );
                        last_ath = ath;
                    }
                    let dd = sniper_ath.ath_tracker.drawdown_pct(balance);
                    if dd > 5.0 {
                        debug!(
                            "ATH drawdown: {:.1}% (current={:.4} SOL, ATH={:.4} SOL)",
                            dd, balance as f64 / 1e9, ath as f64 / 1e9
                        );
                    }
                }
            }
        });
    }

    // ── Grief breaker logging: every 60s, log window loss if > 0 ─────────────
    {
        let sniper_gb = sniper.clone();
        tokio::spawn(async move {
            if sniper_gb.grief_breaker.is_none() { return; }
            let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(60));
            loop {
                interval.tick().await;
                if let Some(ref gb) = sniper_gb.grief_breaker {
                    let loss = gb.window_loss_sol();
                    if loss > 0.0 {
                        if gb.is_tripped() {
                            warn!(
                                "🛑 Grief-loss breaker TRIPPED: {:.4} SOL lost in window",
                                loss
                            );
                        } else {
                            info!("⚠ Grief-loss window: {:.4} SOL", loss);
                        }
                    }
                }
            }
        });
    }

    // ── Pump.fun graduation monitor ───────────────────────────────────────────
    // Enabled when PUMPFUN_MONITOR=1 env var is set
    if std::env::var("PUMPFUN_MONITOR").as_deref() == Ok("1") {
        use scematica_sniper::pumpfun::PumpFunMonitor;
        let pf_tx = event_tx.clone();
        let pf_ws = config.rpc.ws_endpoint.clone();
        let pf_threshold = std::env::var("PUMPFUN_THRESHOLD_SOL")
            .ok()
            .and_then(|s| s.parse::<f64>().ok())
            .unwrap_or(82.0);
        tokio::spawn(async move {
            loop {
                let monitor = PumpFunMonitor::new(pf_ws.clone(), pf_tx.clone(), pf_threshold);
                if let Err(e) = monitor.run().await {
                    warn!("Pump.fun monitor error: {} — restarting in 30s", e);
                    tokio::time::sleep(tokio::time::Duration::from_secs(30)).await;
                }
            }
        });
        info!("Pump.fun graduation monitor started (threshold={:.0} SOL)", pf_threshold);
    }

    // ── Profit extraction scheduler: every 60s check session PnL ─────────────
    {
        let sniper_pe = sniper.clone();
        let rpc_pe = rpc.clone();
        let wallet_kp_pe = wallet_kp.clone();
        let extract_threshold = config.sniper.profit_extraction_threshold_sol;
        let extract_pct = config.sniper.profit_extraction_pct;
        let extract_wallet = config.sniper.profit_extraction_wallet.clone();

        if extract_threshold > 0.0 && extract_pct > 0.0 && !extract_wallet.is_empty() {
            let extract_wallet_log = extract_wallet.clone();
            tokio::spawn(async move {
                let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(60));
                loop {
                    interval.tick().await;

                    let start = sniper_pe.session_start_lamports.load(std::sync::atomic::Ordering::Relaxed);
                    if start == 0 { continue; }

                    let current = match rpc_pe.get_balance(&wallet_kp_pe.pubkey()).await {
                        Ok(b) => b,
                        Err(_) => continue,
                    };

                    let pnl_lamports = current as i64 - start as i64;
                    let _pnl_sol = pnl_lamports as f64 / 1e9;

                    // Check PnL vs baseline
                    let baseline = *sniper_pe.session_pnl_baseline_lamports.lock();
                    let pnl_above_baseline = (pnl_lamports - baseline) as f64 / 1e9;

                    if pnl_above_baseline < extract_threshold {
                        continue;
                    }

                    let extract_amount_sol = pnl_above_baseline * extract_pct / 100.0;
                    let extract_lamports = (extract_amount_sol * 1e9) as u64;

                    if extract_lamports < 5_000_000 {
                        // Below 0.005 SOL — not worth the tx fee
                        continue;
                    }

                    let cold_wallet = match extract_wallet.parse::<solana_sdk::pubkey::Pubkey>() {
                        Ok(pk) => pk,
                        Err(e) => {
                            warn!("Profit extraction: invalid wallet address: {}", e);
                            continue;
                        }
                    };

                    info!(
                        "💰 Profit extraction: transferring {:.4} SOL ({:.0}% of {:.4} SOL profit) to {}",
                        extract_amount_sol, extract_pct, pnl_above_baseline,
                        &extract_wallet[..8.min(extract_wallet.len())]
                    );

                    let transfer_ix = solana_sdk::system_instruction::transfer(
                        &wallet_kp_pe.pubkey(),
                        &cold_wallet,
                        extract_lamports,
                    );

                    match rpc_pe.get_latest_blockhash().await {
                        Ok(blockhash) => {
                            use solana_sdk::signer::Signer;
                            let tx = solana_sdk::transaction::Transaction::new_signed_with_payer(
                                &[transfer_ix],
                                Some(&wallet_kp_pe.pubkey()),
                                &[wallet_kp_pe.as_ref()],
                                blockhash,
                            );
                            match rpc_pe.send_and_confirm_transaction(&tx).await {
                                Ok(sig) => {
                                    info!("💰 Profit extraction confirmed: {}", sig);
                                    // Reset the PnL baseline so we don't extract the same profit twice
                                    *sniper_pe.session_pnl_baseline_lamports.lock() = pnl_lamports;
                                }
                                Err(e) => warn!("Profit extraction tx failed: {}", e),
                            }
                        }
                        Err(e) => warn!("Profit extraction: blockhash fetch failed: {}", e),
                    }
                }
            });
            info!(
                "💰 Profit extraction enabled: {:.0}% of profit when session PnL > {:.4} SOL → {}",
                extract_pct, extract_threshold, &extract_wallet_log[..8.min(extract_wallet_log.len())]
            );
        }
    }

    // ── Multi-RPC latency updater: every 5 minutes ────────────────────────────
    if !config.sniper.extra_rpc_endpoints.is_empty() {
        use scematica_sniper::multi_rpc::MultiRpc;
        use solana_sdk::commitment_config::CommitmentConfig;

        let commitment = match config.rpc.commitment.as_str() {
            "finalized" => CommitmentConfig::finalized(),
            "processed" => CommitmentConfig::processed(),
            _ => CommitmentConfig::confirmed(),
        };

        let mut all_endpoints = vec![config.rpc.endpoint.clone()];
        all_endpoints.extend(config.sniper.extra_rpc_endpoints.clone());
        let multi_rpc = Arc::new(MultiRpc::new(&all_endpoints, commitment));
        info!(
            "🌐 Multi-RPC pool: {} endpoints configured",
            multi_rpc.endpoint_count()
        );

        let multi_rpc_bg = multi_rpc.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(300));
            loop {
                interval.tick().await;
                multi_rpc_bg.update_latencies().await;
            }
        });
    }

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
