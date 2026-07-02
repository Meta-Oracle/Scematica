use anyhow::Result;
use clap::Parser;
use scematica_core::{
    config::BotConfig,
    metrics::{
        artifact_dir, artifact_path, artifact_path_string, ensure_artifact_file, BotMetrics,
        BUILDER_MODE_FILE, DUMP_MODE_FILE, HIGH_SPEED_FILE, LOCK_FILE, LOG_FILE, MOON_CHASE_FILE,
        NN_ADVICE_FILE, NN_AGENT_FILE, NN_STATS_FILE, POOL_DECISIONS_FILE, POSITIONS_FILE,
        RATE_MODE_FILE, SELL_MODE_FILE, TRADES_FILE, TX_TELEMETRY_FILE,
    },
    token::raw_to_ui,
    types::known_tokens,
    wallet::Wallet,
};
use scematica_nn::{AgentStats, DQNAgent, TradeAction, TradeState as NNState};
use scematica_sniper::{
    alerts::AlertManager,
    listener::{ListenerEvent, PoolListener},
    sniper::Sniper,
};
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::{commitment_config::CommitmentConfig, signer::Signer};
use std::sync::{Arc, Mutex};
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

/// Returns true if a process with `pid` is currently running.
fn is_process_alive(pid: u32) -> bool {
    #[cfg(windows)]
    {
        let out = std::process::Command::new("tasklist")
            .args(["/FI", &format!("PID eq {}", pid), "/NH"])
            .output();
        match out {
            Ok(o) => {
                let s = String::from_utf8_lossy(&o.stdout);
                s.lines().any(|l| l.contains(&pid.to_string()))
            }
            Err(_) => false,
        }
    }
    #[cfg(not(windows))]
    {
        std::path::Path::new(&format!("/proc/{}", pid)).exists()
    }
}

/// Best-effort removal of the lock file on graceful shutdown.
struct LockGuard;
impl Drop for LockGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(artifact_path(LOCK_FILE));
    }
}

fn event_f64(v: &serde_json::Value, key: &str) -> f64 {
    v.get(key).and_then(|x| x.as_f64()).unwrap_or(0.0)
}

fn event_confirmed(v: &serde_json::Value) -> bool {
    v.get("status")
        .and_then(|x| x.as_str())
        .map(|status| status == "\u{2713}" || status.eq_ignore_ascii_case("confirmed"))
        .unwrap_or(false)
}

fn trade_state_from_event(
    v: &serde_json::Value,
    daily_pnl_sol: f64,
    consecutive_wins: i32,
    consecutive_losses: i32,
) -> NNState {
    use chrono::Timelike;

    let pnl_pct = event_f64(v, "pnl_pct").clamp(-200.0, 500.0);
    let pool_score = event_f64(v, "pool_score");
    let inflow_rate = event_f64(v, "inflow_rate_sol_per_sec");
    let velocity = event_f64(v, "velocity_sol_per_sec");
    NNState {
        pool_age_secs: event_f64(v, "pool_age_secs"),
        initial_liquidity_sol: event_f64(v, "pool_size_sol"),
        price_change_pct: pnl_pct / 100.0,
        volume_5min_sol: (inflow_rate.max(0.0) * 300.0).min(100.0),
        buy_sell_ratio: event_f64(v, "buy_pressure_ratio").max(0.0).min(5.0),
        lp_burned: true,
        mint_renounced: true,
        current_pnl_pct: pnl_pct / 100.0,
        position_age_secs: event_f64(v, "position_age_secs"),
        daily_pnl_sol,
        consecutive_wins,
        consecutive_losses,
        sol_balance_sol: 0.0,
        regime: 0,
        volatility: 0.0,
        spread_pct: 0.0,
        time_of_day_norm: chrono::Utc::now().hour() as f64 / 24.0,
        open_positions: 0,
        peak_pnl_pct: (pnl_pct / 100.0).max(0.0),
        pool_score_norm: (pool_score / 100.0).clamp(0.0, 1.0),
        deployer_rug_rate: 0.5,
        volume_velocity: (inflow_rate / 5.0).clamp(-1.0, 1.0),
        price_velocity: (velocity / 5.0).clamp(-1.0, 1.0),
        price_acceleration: 0.0,
    }
}

fn infer_buy_action(v: &serde_json::Value) -> TradeAction {
    let pool_score = event_f64(v, "pool_score");
    let pumpfun_score = event_f64(v, "pumpfun_score");
    let inflow_rate = event_f64(v, "inflow_rate_sol_per_sec");
    let velocity = event_f64(v, "velocity_sol_per_sec");
    if pool_score >= 90.0 || pumpfun_score >= 90.0 || inflow_rate >= 1.5 || velocity >= 2.618 {
        TradeAction::BuyAggressive
    } else {
        TradeAction::BuyStandard
    }
}

fn train_nn_transition(
    agent: &Arc<Mutex<DQNAgent>>,
    state: NNState,
    action: TradeAction,
    reward: f64,
    next_state: NNState,
    done: bool,
) {
    if let Ok(mut ag) = agent.lock() {
        ag.observe(state, action, reward, next_state, done);
        for _ in 0..4 {
            let Some(loss) = ag.train_step() else {
                break;
            };
            debug!("NN train loss={:.6}", loss);
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    // Init tracing — write to stderr AND scematica-sniper.log so the dashboard can tail it
    {
        use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
        let filter =
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(&args.log_level));
        let stderr_layer = tracing_subscriber::fmt::layer().with_writer(std::io::stderr);
        let file_appender = tracing_appender::rolling::never(artifact_dir(), LOG_FILE);
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
    info!(
        "║     SCEMATICA SNIPER  v{}          ║",
        env!("CARGO_PKG_VERSION")
    );
    info!("╚══════════════════════════════════════╝");

    // ── Single-instance guard ───────────────────────────────────────────────
    // Two sniper processes sharing the same Helius WebSocket rate-limit each
    // other into uselessness (we observed ~1 pool/15 min instead of 1 pool/min).
    // Refuse to start if another instance is already alive.
    let lock_file = artifact_path(LOCK_FILE);
    if let Ok(prev) = std::fs::read_to_string(&lock_file) {
        if let Some(pid_str) = prev.lines().next() {
            if let Ok(pid) = pid_str.trim().parse::<u32>() {
                if is_process_alive(pid) {
                    error!(
                        "Another sniper is already running (PID {}). Refusing to start a duplicate. \
                         Stop the existing instance or delete {} if you know it's stale.",
                        pid, LOCK_FILE,
                    );
                    return Err(anyhow::anyhow!(
                        "duplicate sniper instance detected (PID {})",
                        pid
                    ));
                }
                warn!("Stale {} from dead PID {} — overwriting", LOCK_FILE, pid);
            }
        }
    }
    let _ = std::fs::write(&lock_file, format!("{}\n", std::process::id()));
    // Best-effort cleanup on graceful shutdown (Ctrl+C). Crashes leave the file
    // behind, but the next start's is_process_alive() check handles that.
    let _lock_guard = LockGuard;

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
            match rpc
                .get_token_accounts_by_owner(
                    &wallet_kp.pubkey(),
                    TokenAccountsFilter::Mint(known_tokens::SCEMATICA_MINT),
                )
                .await
            {
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
                            MIN_SCEMA_REQUIRED,
                            held
                        ));
                    }
                    info!(
                        "✅ SCEMA gate passed: {:.0} SCEMA held (required: {:.0})",
                        held, MIN_SCEMA_REQUIRED
                    );
                    gate_passed = true;
                    break;
                }
                Err(e) => {
                    warn!(
                        "SCEMA gate attempt {}/5 failed: {} — retrying in 3s...",
                        attempt, e
                    );
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

    ensure_artifact_file(TRADES_FILE);
    ensure_artifact_file(POOL_DECISIONS_FILE);
    ensure_artifact_file(TX_TELEMETRY_FILE);

    let nn_agent_path = artifact_path_string(NN_AGENT_FILE);
    // Opt-in upgrades (v1.12): QR-DQN distributional returns + a Dreamer-style
    // latent world model for Dyna imagination. Both default OFF so existing
    // deployments and their scalar checkpoints are untouched. The distributional
    // policy can only be applied to a *fresh* agent (weights are shape-different);
    // the world model can be attached to any agent, including a loaded scalar one.
    let want_distributional = std::env::var("SCEMATICA_NN_DISTRIBUTIONAL")
        .map(|v| v != "0")
        .unwrap_or(false);
    let want_world_model = std::env::var("SCEMATICA_NN_WORLD_MODEL")
        .map(|v| v != "0")
        .unwrap_or(false);
    let nn_agent: Arc<Mutex<DQNAgent>> =
        Arc::new(Mutex::new(match DQNAgent::load(&nn_agent_path) {
            Ok(mut a) => {
                info!(
                    "NN agent loaded from checkpoint (distributional={}, world_model={})",
                    a.is_distributional(),
                    a.has_world_model()
                );
                if want_world_model && !a.has_world_model() {
                    a.enable_world_model();
                    info!("🧠 World model attached to loaded agent");
                }
                a
            }
            Err(_) => {
                let mut a = if want_distributional {
                    info!(
                        "NN agent initialised fresh — QR-DQN distributional (STATE_DIM={}, ACTIONS=5, quantiles={})",
                        scematica_nn::STATE_DIM,
                        scematica_nn::N_QUANTILES
                    );
                    DQNAgent::new_distributional()
                } else {
                    info!(
                        "NN agent initialised fresh (scalar Double-DQN, STATE_DIM={}, ACTIONS=5)",
                        scematica_nn::STATE_DIM
                    );
                    DQNAgent::new()
                };
                if want_world_model {
                    a.enable_world_model();
                    info!("🧠 World model enabled");
                }
                a
            }
        }));

    // Create sniper
    let sniper = Arc::new(Sniper::new(
        config.sniper.clone(),
        wallet_kp.clone(),
        rpc.clone(),
        metrics.clone(),
        alerts.clone(),
        Some(Arc::clone(&nn_agent)),
        config.execution.clone(),
    ));

    // Record session start balance for drawdown tracking
    if let Ok(start_bal) = rpc.get_balance(&wallet_kp.pubkey()).await {
        use std::sync::atomic::Ordering;
        sniper
            .session_start_lamports
            .store(start_bal, Ordering::Relaxed);
        info!(
            "Session start balance: {:.4} SOL ({} lamports)",
            start_bal as f64 / 1e9,
            start_bal
        );
    }

    // Load persisted pool cache so previous-run tokens can be sold immediately
    sniper.load_pool_cache("pool-cache.json");

    // Surface any stale lock files from a prior session so the operator knows why
    // buys might be paused. Without this banner the dashboard just silently rejects
    // every pool with no obvious reason — exactly the "still gets hung up on sell
    // mode" footgun.
    {
        if let Ok(contents) = std::fs::read_to_string(artifact_path(SELL_MODE_FILE)) {
            let reason = serde_json::from_str::<serde_json::Value>(&contents)
                .ok()
                .and_then(|v| v.get("reason").and_then(|r| r.as_str().map(String::from)))
                .unwrap_or_else(|| "unknown".to_string());
            warn!(
                "🚨 STARTUP: scematica-sell-mode.json exists (reason: {}). Buying is PAUSED. \
                 Press [b] in the dashboard Logs tab to override, or delete the file manually.",
                reason,
            );
        }
        if artifact_path(DUMP_MODE_FILE).exists() {
            warn!(
                "🚨 STARTUP: scematica-dump-mode.json exists. DUMP MODE is engaged — all positions \
                 will be force-sold with zero slippage. Delete the file or press [d] to clear."
            );
        }
    }

    // Scan for tokens already held from a previous run and spawn sell monitors
    {
        let sniper_scan = sniper.clone();
        tokio::spawn(async move {
            sniper_scan.scan_existing_positions().await;
        });
    }

    // High-speed-mode file watcher — checks scematica-highspeed-mode.json every 2 s
    // (faster than the other watchers so the operator's toggle takes effect quickly).
    {
        use std::sync::atomic::Ordering;
        let sniper_hs = sniper.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(2));
            let mut was_active = false;
            loop {
                interval.tick().await;
                let active = artifact_path(HIGH_SPEED_FILE).exists();
                sniper_hs.high_speed_mode.store(active, Ordering::Relaxed);
                if active && !was_active {
                    warn!("⚡ HIGH-SPEED MODE engaged — filters/AI/scorer bypassed, fee escalated, parallel buys enabled. Expect 429s and failed buys.");
                } else if !active && was_active {
                    info!("⚡ High-speed mode disengaged — normal filter pipeline restored");
                }
                was_active = active;
            }
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
                let active = artifact_path(SELL_MODE_FILE).exists();
                sniper_sm.sell_mode.store(active, Ordering::Relaxed);
                if active && !was_active {
                    warn!("🚨 SELL MODE activated — pausing buys and force-selling all positions");
                    let sniper_ref = sniper_sm.clone();
                    tokio::spawn(async move {
                        sniper_ref.scan_existing_positions().await;
                    });
                } else if !active && was_active {
                    // Reset buy counter so the next batch starts fresh
                    sniper_sm
                        .buy_count
                        .store(0, std::sync::atomic::Ordering::Relaxed);
                    info!(
                        "✅ Sell mode deactivated — buy counter reset, resuming normal operation"
                    );
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
                let active = artifact_path(DUMP_MODE_FILE).exists();
                sniper_dm.dump_mode.store(active, Ordering::Relaxed);
                if active {
                    active_ticks += 1;
                    let first = !was_active;
                    // Fire immediately on activation, then every 6 ticks (30 s)
                    if first || active_ticks % 6 == 0 {
                        if first {
                            warn!("💥 DUMP MODE activated — force-selling ALL positions with zero slippage");
                        } else {
                            warn!(
                                "AUTO DUMP: retrying unsold positions (tick {})",
                                active_ticks
                            );
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
    // When the dashboard sets a new rate mode the sniper picks it up and applies
    // ALL fields from the matching RateMode in config (single source of truth).
    // amount_multiplier is reset to 1.0 on every mode switch so strategy-agent
    // values from a prior mode don't compound onto a fresh mode's sizing.
    {
        let sniper_rm = sniper.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(5));
            let mut last_mode = String::new();
            loop {
                interval.tick().await;
                let Ok(data) = std::fs::read_to_string(artifact_path(RATE_MODE_FILE)) else {
                    continue;
                };
                let Ok(v) = serde_json::from_str::<serde_json::Value>(&data) else {
                    continue;
                };
                let mode_str = v["mode"].as_str().unwrap_or("").to_string();
                if mode_str.is_empty() {
                    continue;
                }
                // Dedup: skip if nothing changed (mode name AND wallet_pct both stable)
                let file_wp = v["wallet_pct"].as_f64().unwrap_or(0.0);
                let dedup_key = format!("{}:{:.3}", mode_str, file_wp);
                if dedup_key == last_mode {
                    continue;
                }
                last_mode = dedup_key;

                // Look up the mode in config.toml (case-insensitive).
                // config is the single source of truth for TP/SL/sizing — not the file,
                // which may carry stale dashboard enum values that diverge from config.toml.
                let config_mode = sniper_rm
                    .config
                    .rate_modes
                    .iter()
                    .find(|m| m.name.to_lowercase() == mode_str.to_lowercase() && m.enabled)
                    .cloned();

                let mut params = sniper_rm.live_params.write();
                if let Some(m) = config_mode {
                    // Apply every field from config — this is the authoritative definition.
                    params.take_profit_pct = m.take_profit_pct;
                    params.stop_loss_pct = m.stop_loss_pct;
                    params.take_profit_pct_mode = m.take_profit_pct;
                    params.stop_loss_pct_mode = m.stop_loss_pct;
                    params.quote_amount_mode = m.quote_amount;
                    params.wallet_pct = m.wallet_pct;
                    params.momentum_max_escalations_mode = m.momentum_max_escalations;
                    params.active_mode_name = m.name.clone();
                    // Reset multiplier to 1.0 so strategy-agent adjustments start
                    // from a clean baseline rather than stacking on the prior mode's value.
                    params.amount_multiplier = 1.0;
                    info!(
                        "⚡ Rate mode → {} (from config)  |  {:.1}% wallet  {:.4} SOL base  TP {:.0}%  SL {:.0}%  esc {}",
                        m.name, m.wallet_pct, m.quote_amount, m.take_profit_pct, m.stop_loss_pct, m.momentum_max_escalations
                    );
                } else {
                    // Mode not found in config — fall back to file values
                    if let Some(tp) = v["tp_pct"].as_f64() {
                        params.take_profit_pct = tp;
                        params.take_profit_pct_mode = tp;
                    }
                    if let Some(sl) = v["sl_pct"].as_f64() {
                        params.stop_loss_pct = sl;
                        params.stop_loss_pct_mode = sl;
                    }
                    if let Some(qa) = v["quote_amount"].as_f64() {
                        params.quote_amount_mode = qa;
                    }
                    if let Some(wp) = v["wallet_pct"].as_f64() {
                        params.wallet_pct = wp;
                    }
                    params.amount_multiplier = 1.0;
                    info!(
                        "⚡ Rate mode → {} (file fallback — not in config)  |  {:.1}% wallet  TP {:.0}%  SL {:.0}%",
                        mode_str, params.wallet_pct, params.take_profit_pct, params.stop_loss_pct
                    );
                }
            }
        });
    }

    // ── Weekend mode auto-switch ──────────────────────────────────────────────
    // Checks day-of-week every 10 min UTC. If config.weekend_mode is set, writes
    // scematica-rate-mode.json to trigger the rate-mode watcher above.
    // Live data: Saturday = 0% WR, Friday = 22% WR vs Monday = 32% WR.
    if !sniper.config.weekend_mode.is_empty() {
        let sniper_wm = sniper.clone();
        tokio::spawn(async move {
            use chrono::{Datelike, Weekday};
            let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(600));
            let mut last_applied = String::new();
            loop {
                interval.tick().await;
                let weekday = chrono::Utc::now().weekday();
                let is_weekend = matches!(weekday, Weekday::Sat | Weekday::Sun);
                let target_mode = if is_weekend {
                    sniper_wm.config.weekend_mode.clone()
                } else {
                    let wm = &sniper_wm.config.weekday_mode;
                    if wm.is_empty() {
                        "Balanced".to_string()
                    } else {
                        wm.clone()
                    }
                };
                if target_mode == last_applied {
                    continue;
                }
                // Only auto-switch if the mode exists in config
                let known = sniper_wm
                    .config
                    .rate_modes
                    .iter()
                    .any(|m| m.name.to_lowercase() == target_mode.to_lowercase() && m.enabled);
                if !known {
                    continue;
                }
                let json = serde_json::json!({ "mode": target_mode });
                if let Ok(s) = serde_json::to_string(&json) {
                    let path = artifact_path(RATE_MODE_FILE);
                    let tmp = artifact_path(format!("{}.tmp", RATE_MODE_FILE));
                    if std::fs::write(&tmp, &s).is_ok() {
                        let _ = std::fs::rename(&tmp, path);
                        info!(
                            "📅 Weekend auto-switch: {} → {} ({})",
                            last_applied, target_mode, weekday
                        );
                        last_applied = target_mode;
                    }
                }
            }
        });
    }

    // ── Builder-mode watcher ──────────────────────────────────────────────────
    // Polls `scematica-builder-mode.json` every 5 s (written by dashboard [g/j/k/o]).
    //
    // Beyond simply setting wallet_target_lamports_override, this watcher runs the
    // two compounding algorithms that drive the Builder (1 SOL) and SuperBuilder
    // (3 SOL) modes. On every tick it reads the current approximate wallet balance,
    // computes progress toward the target, and writes optimal live_params so that
    // position size, TP, and SL all evolve continuously as the wallet grows.
    //
    // ── Builder (1 SOL) — Geometric Compounding ──────────────────────────────
    //   p = wallet_sol / 1.0  (0..1)
    //   size_mult  = clamp(1.5 + 2.0 × p^0.65, 1.5, 3.5)
    //     → 0%: 1.5×  |  25%: 2.26×  |  50%: 2.82×  |  100%: 3.5×
    //   take_profit = base_tp × max(1.0, 1.5 − 0.5 × p)
    //     → far from target: TP at 1.5× base (bigger wins compound faster)
    //     → near target:     TP at base (accept moderate wins to lock in gains)
    //   stop_loss  = base_sl × (1.2 − 0.2 × p)   (slightly wider when far)
    //
    // ── SuperBuilder (3 SOL) — Parabolic Compounding ─────────────────────────
    //   p = wallet_sol / 3.0  (0..1)
    //   size_mult  = clamp(2.0 + 6.0 × p^0.35, 2.0, 8.0)
    //     → 0%: 2.0×  |  10%: 4.7×  |  25%: 5.6×  |  50%: 6.7×  |  100%: 8.0×
    //   take_profit = base_tp × max(1.0, 2.0 − p)
    //     → 0%: 2× base TP  |  50%: 1.5× base  |  100%: base
    //   stop_loss  = base_sl × 1.4  (fixed wider floor; profit_first handles rugs)
    //   moon_chase  = ON when p < 0.25 (early aggressive), OFF when p > 0.60
    {
        use std::sync::atomic::Ordering;
        let sniper_bm = sniper.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(5));
            let mut last_label = String::new();
            let mut last_mult = 0.0f64;
            loop {
                interval.tick().await;
                let path = artifact_path(BUILDER_MODE_FILE);
                if !path.exists() {
                    if !last_label.is_empty() {
                        sniper_bm
                            .wallet_target_lamports_override
                            .store(0, Ordering::Relaxed);
                        sniper_bm
                            .progressive_scaling
                            .store(false, Ordering::Relaxed);
                        sniper_bm.moon_chase.store(false, Ordering::Relaxed);
                        {
                            let mut lp = sniper_bm.live_params.write();
                            lp.amount_multiplier = 1.0;
                        }
                        info!("🏗️  Builder mode cleared — config.wallet_target_sol restored");
                        last_label.clear();
                        last_mult = 0.0;
                    }
                    continue;
                }
                let Ok(data) = std::fs::read_to_string(&path) else {
                    continue;
                };
                let Ok(v) = serde_json::from_str::<serde_json::Value>(&data) else {
                    continue;
                };
                let label = v["mode"].as_str().unwrap_or("").to_string();
                let target_sol = v["target_sol"].as_f64().unwrap_or(0.0);

                // On label change: set target and log mode switch
                if label != last_label {
                    let target_lam = (target_sol * 1e9) as u64;
                    sniper_bm
                        .wallet_target_lamports_override
                        .store(target_lam, Ordering::Relaxed);
                    // Both Builder and SuperBuilder use live_params for sizing,
                    // not the buy() progressive scaler (which is additive and
                    // would double-multiply with live_params.amount_multiplier).
                    sniper_bm
                        .progressive_scaling
                        .store(false, Ordering::Relaxed);
                    info!(
                        "🏗️  Builder mode → {} | target {:.2} SOL",
                        label, target_sol
                    );
                    last_label = label.clone();
                }

                // Every tick: recompute optimal live_params from current progress
                let wallet_sol = sniper_bm.approx_wallet_sol();
                let base_tp = sniper_bm.base_take_profit_pct();
                let base_sl = sniper_bm.base_stop_loss_pct();
                let progress = if target_sol > 0.0 {
                    (wallet_sol / target_sol).clamp(0.0, 1.0)
                } else {
                    0.0
                };

                let (mult, tp, sl, moon_on) = match label.as_str() {
                    "builder" => {
                        // Geometric Compounding — 1 SOL target
                        let m = (1.5 + 2.0 * progress.powf(0.65)).clamp(1.5, 3.5);
                        let t = base_tp * (1.5 - 0.5 * progress).max(1.0);
                        let s = base_sl * (1.2 - 0.2 * progress);
                        (m, t, s, false)
                    }
                    "super_builder" => {
                        // Parabolic Compounding — 3 SOL target
                        let m = (2.0 + 6.0 * progress.powf(0.35)).clamp(2.0, 8.0);
                        let t = base_tp * (2.0 - progress).max(1.0);
                        let s = base_sl * 1.4;
                        // Auto moon-chase: on when early, off when protecting gains
                        let mc = progress < 0.25;
                        (m, t, s, mc)
                    }
                    "growth" => {
                        // Growth — conservative 0.2 SOL target, mild scaling
                        let m = (1.0 + 1.0 * progress.powf(0.8)).clamp(1.0, 2.0);
                        let t = base_tp;
                        let s = base_sl;
                        (m, t, s, false)
                    }
                    _ => continue,
                };

                // Only log + write when multiplier changed by >5% (avoids log spam)
                if (mult - last_mult).abs() / last_mult.max(1.0) > 0.05 || last_mult == 0.0 {
                    info!(
                        "🏗️  {} | progress {:.1}% | size {:.2}× | TP {:.0}% | SL {:.1}% | moon_chase {}",
                        label, progress * 100.0, mult, tp, sl,
                        if moon_on { "ON" } else { "OFF" }
                    );
                    last_mult = mult;
                }

                sniper_bm.moon_chase.store(moon_on, Ordering::Relaxed);
                {
                    let mut lp = sniper_bm.live_params.write();
                    lp.amount_multiplier = mult;
                    lp.take_profit_pct = tp;
                    lp.stop_loss_pct = sl;
                }
            }
        });
    }

    // Live positions flush task — serialises the in-memory position registry
    // to scematica-positions.json every 1 s. The dashboard reads this on its
    // tick poll. Atomic write via tmp + rename so the dashboard never reads a
    // half-written file mid-flush.
    {
        let sniper_lp = sniper.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(1));
            loop {
                interval.tick().await;
                let snap: Vec<_> = sniper_lp
                    .live_positions
                    .iter()
                    .map(|e| e.value().clone())
                    .collect();
                if let Ok(json) = serde_json::to_string(&snap) {
                    let path = artifact_path(POSITIONS_FILE);
                    let tmp = artifact_path(format!("{}.tmp", POSITIONS_FILE));
                    if std::fs::write(&tmp, &json).is_ok() {
                        let _ = std::fs::rename(&tmp, path);
                    }
                }
            }
        });
    }

    // Moon Chase file watcher — checks scematica-moon-chase.json every 5 s.
    // Presence of the file = engaged; absence = disengaged. The sell monitor
    // reads sniper.moon_chase via Atomic on every check, so toggling is hot.
    {
        use std::sync::atomic::Ordering;
        let sniper_mc = sniper.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(5));
            let mut was_active = false;
            loop {
                interval.tick().await;
                let active = artifact_path(MOON_CHASE_FILE).exists();
                sniper_mc.moon_chase.store(active, Ordering::Relaxed);
                if active && !was_active {
                    warn!("🌙 MOON CHASE engaged — 8 escalations × 1.75×, pullback 25%, threshold 3%/check");
                } else if !active && was_active {
                    info!("🌙 Moon Chase disengaged — momentum-hold back to EV-optimal params");
                }
                was_active = active;
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

    // Max drawdown guard — activates sell mode if wallet drops > max_drawdown_pct from
    // session start. Also self-clears when the wallet recovers above the threshold
    // and there are no open positions — otherwise a single dip pinned the bot in sell
    // mode forever (the "still hung up on sell mode" footgun).
    {
        use std::sync::atomic::Ordering;
        let sniper_dd = sniper.clone();
        let rpc_dd = rpc.clone();
        let wallet_pk = wallet_kp.pubkey();
        let max_dd = config.sniper.max_drawdown_pct;
        tokio::spawn(async move {
            if max_dd <= 0.0 {
                return;
            }
            let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(30));
            loop {
                interval.tick().await;
                let start = sniper_dd.session_start_lamports.load(Ordering::Relaxed);
                if start == 0 {
                    continue;
                }
                let open_positions = sniper_dd.open_positions.load(Ordering::Relaxed);
                if open_positions > 0 {
                    tracing::debug!(
                        open_positions,
                        "Max drawdown guard deferred while positions are open"
                    );
                    continue;
                }
                let current = match rpc_dd.get_balance(&wallet_pk).await {
                    Ok(b) => b,
                    Err(_) => continue,
                };

                let drawdown_pct = if current < start {
                    (start - current) as f64 / start as f64 * 100.0
                } else {
                    0.0
                };

                // Trip condition — set sell-mode with reason=max_drawdown.
                if drawdown_pct >= max_dd {
                    let already_set = std::fs::read_to_string("scematica-sell-mode.json")
                        .map(|s| s.contains("max_drawdown"))
                        .unwrap_or(false);
                    if !already_set {
                        warn!(
                            "🔴 Max drawdown {:.1}% reached (start={:.4} SOL, now={:.4} SOL) — activating sell mode",
                            drawdown_pct, start as f64 / 1e9, current as f64 / 1e9,
                        );
                    }
                    sniper_dd.sell_mode.store(true, Ordering::Relaxed);
                    let _ = std::fs::write(
                        "scematica-sell-mode.json",
                        r#"{"active":true,"reason":"max_drawdown"}"#,
                    );
                    continue;
                }

                // Recovery condition — clear ONLY if the active sell-mode file is our
                // own drawdown trigger (reason=="max_drawdown") AND all positions have
                // closed. We never clobber dashboard / buy_limit / dump triggers.
                if let Ok(contents) = std::fs::read_to_string("scematica-sell-mode.json") {
                    let is_drawdown_trigger = contents.contains("max_drawdown");
                    let no_open = sniper_dd.open_positions.load(Ordering::Relaxed) == 0;
                    // Add 1% hysteresis so we don't flap right at the boundary.
                    let recovered = drawdown_pct + 1.0 < max_dd;
                    if is_drawdown_trigger && no_open && recovered {
                        sniper_dd.sell_mode.store(false, Ordering::Relaxed);
                        let _ = std::fs::remove_file("scematica-sell-mode.json");
                        // Reset the drawdown baseline to the current wallet balance so the
                        // next drawdown measurement starts from now, not from the original
                        // session start. Without this reset the guard re-trips immediately
                        // on the next buy (wallet drops briefly below an already-low baseline).
                        sniper_dd
                            .session_start_lamports
                            .store(current, Ordering::Relaxed);
                        info!(
                            "🟢 Drawdown recovered ({:.1}% < {:.1}% threshold) — baseline reset to {:.4} SOL, sell mode lifted",
                            drawdown_pct, max_dd, current as f64 / 1e9,
                        );
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
                let Ok(meta) = std::fs::metadata(&config_path) else {
                    continue;
                };
                let Ok(mtime) = meta.modified() else { continue };
                if last_mtime == Some(mtime) {
                    continue;
                }
                last_mtime = Some(mtime);
                // Reload and update only live parameters (no restart required)
                match scematica_core::config::BotConfig::from_file(&config_path) {
                    Ok(new_cfg) => {
                        let mut params = sniper_hr.live_params.write();
                        params.take_profit_pct = new_cfg.sniper.take_profit_pct;
                        params.stop_loss_pct = new_cfg.sniper.stop_loss_pct;
                        // amount_multiplier intentionally not reset — rate mode owns it
                        info!(
                            "🔄 Config hot-reloaded: TP {:.1}%  SL {:.1}%",
                            params.take_profit_pct, params.stop_loss_pct
                        );
                    }
                    Err(e) => warn!(
                        "Config hot-reload parse error: {} — keeping current params",
                        e
                    ),
                }
            }
        });
    }

    // ── Deep Q* Neural Network agent ────────────────────────────────────────
    // Learns from every trade row and shares the live agent used by buy advice.
    // Trained checkpoints advise immediately; fresh agents advise after training starts.
    {
        let nn_agent = Arc::clone(&nn_agent);

        // Initial stats write — without this, the dashboard's NN panel sits blank
        // for the first 5 s after launch (until the flush task ticks for the first
        // time). Writing immediately gives the operator visible signal that the
        // agent loaded and is alive even before any trades have happened.
        {
            if let Ok(ag) = nn_agent.lock() {
                let stats: AgentStats = ag.stats();
                if let Ok(s) = serde_json::to_string(&stats) {
                    let _ = std::fs::write(artifact_path(NN_STATS_FILE), s);
                }
                ag.write_explanation(&NNState::default(), &artifact_path_string(NN_ADVICE_FILE));
            }
        }

        // Observer task: polls scematica-trades.jsonl and trains from BUY, SELL,
        // and ARB rows. BUY rows carry pool context; paired SELL rows provide the
        // outcome reward that teaches entry selection.
        {
            let agent = Arc::clone(&nn_agent);
            tokio::spawn(async move {
                let mut last_seen: usize = 0;
                let mut pending_buys: std::collections::HashMap<String, (NNState, TradeAction)> =
                    std::collections::HashMap::new();
                let mut daily_pnl_sol: f64 = 0.0;
                let mut consecutive_wins: i32 = 0;
                let mut consecutive_losses: i32 = 0;
                let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(5));
                loop {
                    interval.tick().await;
                    let Ok(raw) = std::fs::read_to_string(artifact_path(TRADES_FILE)) else {
                        continue;
                    };
                    let lines: Vec<&str> = raw.lines().collect();
                    if lines.len() < last_seen {
                        last_seen = 0;
                        pending_buys.clear();
                    }
                    if lines.len() == last_seen {
                        continue;
                    }

                    for line in &lines[last_seen..] {
                        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
                            continue;
                        };
                        let kind = v["kind"].as_str().unwrap_or("");
                        let mint = v["mint"].as_str().unwrap_or("").to_string();
                        if kind == "BUY" {
                            let state = trade_state_from_event(
                                &v,
                                daily_pnl_sol,
                                consecutive_wins,
                                consecutive_losses,
                            );
                            let action = infer_buy_action(&v);
                            if event_confirmed(&v) {
                                if !mint.is_empty() {
                                    pending_buys.insert(mint, (state, action));
                                }
                            } else {
                                train_nn_transition(
                                    &agent,
                                    state,
                                    action,
                                    -0.05,
                                    NNState::default(),
                                    true,
                                );
                            }
                            continue;
                        }
                        if kind == "ARB" {
                            let pnl_sol = event_f64(&v, "pnl");
                            let amount = event_f64(&v, "amount");
                            daily_pnl_sol += pnl_sol;
                            let mut arb_state = trade_state_from_event(
                                &v,
                                daily_pnl_sol,
                                consecutive_wins,
                                consecutive_losses,
                            );
                            if amount > 0.0 && arb_state.current_pnl_pct == 0.0 {
                                arb_state.current_pnl_pct = pnl_sol / amount;
                                arb_state.price_change_pct = pnl_sol / amount;
                            }
                            train_nn_transition(
                                &agent,
                                arb_state,
                                TradeAction::Hold,
                                (pnl_sol * 100.0).clamp(-0.05, 0.05),
                                NNState::default(),
                                true,
                            );
                            continue;
                        }
                        if kind != "SELL" {
                            continue;
                        }
                        let confirmed = event_confirmed(&v);

                        let pnl_sol = v["pnl"].as_f64().unwrap_or(0.0);
                        let pnl_pct = v["pnl_pct"]
                            .as_f64()
                            .filter(|&p| p != 0.0) // treat missing/zero as absent
                            .unwrap_or_else(|| {
                                // Old entries lack pnl_pct. Skip them (use 0 = neutral)
                                // rather than backfilling pnl_sol*10000 which diverged Q-net.
                                0.0
                            })
                            .clamp(-200.0, 500.0); // hard cap regardless of source
                        let age_secs = v["position_age_secs"].as_f64().unwrap_or(0.0);
                        daily_pnl_sol += pnl_sol;

                        // Update streak tracking
                        if pnl_sol > 0.0 {
                            consecutive_wins += 1;
                            consecutive_losses = 0;
                        } else {
                            consecutive_losses += 1;
                            consecutive_wins = 0;
                        }

                        let state = NNState::from_trade_fields(
                            pnl_pct / 100.0,
                            age_secs,
                            daily_pnl_sol,
                            consecutive_wins,
                            consecutive_losses,
                            0.0,
                            0,
                        );
                        let action = TradeAction::SellAll;
                        let reward = if confirmed {
                            // Divide by 100 to keep TD targets in [-3, +9] range so SGD
                            // gradients stay bounded (raw shape_reward reaches ±900, blowing
                            // the 1e-3 lr network into saturation within a few batches).
                            DQNAgent::shape_reward(pnl_pct, (age_secs / 60.0) as u32) / 100.0
                        } else {
                            // Penalise hard-failed sells lightly so the agent learns to avoid pools
                            // where exits jam without flooding the signal with synthetic -100% events.
                            -0.01
                        };
                        let next_state = NNState::from_trade_fields(
                            0.0,
                            0.0,
                            0.0,
                            consecutive_wins,
                            consecutive_losses,
                            0.0,
                            0,
                        );

                        let sell_state = trade_state_from_event(
                            &v,
                            daily_pnl_sol,
                            consecutive_wins,
                            consecutive_losses,
                        );
                        if let Some((entry_state, entry_action)) = pending_buys.remove(&mint) {
                            train_nn_transition(
                                &agent,
                                entry_state,
                                entry_action,
                                reward,
                                sell_state,
                                true,
                            );
                        }
                        train_nn_transition(&agent, state, action, reward * 0.25, next_state, true);
                    }
                    last_seen = lines.len();
                }
            });
        }

        // Stats flush task: writes scematica-nn-stats.json every 5 s for the dashboard.
        // 5 s matches the dashboard's poll cadence on the file — slower than that and the
        // panel feels frozen between updates (the previous 30 s interval is what made it
        // look like the agent wasn't running at all).
        {
            let agent = Arc::clone(&nn_agent);
            tokio::spawn(async move {
                let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(5));
                loop {
                    interval.tick().await;
                    if let Ok(ag) = agent.lock() {
                        let stats: AgentStats = ag.stats();
                        if let Ok(s) = serde_json::to_string(&stats) {
                            // Atomic write: tmp → rename so the dashboard never reads a
                            // half-written file mid-flush.
                            let path = artifact_path(NN_STATS_FILE);
                            let tmp = artifact_path(format!("{}.tmp", NN_STATS_FILE));
                            if std::fs::write(&tmp, &s).is_ok() {
                                let _ = std::fs::rename(&tmp, path);
                            }
                        }
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
                        match ag.save(&artifact_path_string(NN_AGENT_FILE)) {
                            Ok(_) => info!("🧠 NN agent checkpoint saved"),
                            Err(e) => tracing::warn!("NN checkpoint save failed: {}", e),
                        }
                    }
                }
            });
        }

        // World-model task: when a Dreamer-style world model is attached, train it
        // on real transitions and periodically "dream" imagined trajectories into
        // the replay buffer (Dyna planning). This amortises scarce live trades into
        // many synthetic training samples. No-op when no world model is present.
        {
            let agent = Arc::clone(&nn_agent);
            tokio::spawn(async move {
                let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(15));
                loop {
                    interval.tick().await;
                    if let Ok(mut ag) = agent.lock() {
                        if !ag.has_world_model() {
                            continue;
                        }
                        // A few model-learning steps per tick.
                        let mut last = None;
                        for _ in 0..4 {
                            last = ag.train_world_model_step();
                            if last.is_none() {
                                break;
                            }
                        }
                        // Then dream: 8 rollouts × 4-step horizon → up to 32 synthetic
                        // transitions folded back into replay for the policy learner.
                        let injected = ag.imagine_into_replay(8, 4);
                        if let Some(l) = last {
                            debug!(
                                "🌙 world-model: recon={:.4} dyn={:.4} rew={:.4} dreamed={}",
                                l.reconstruction, l.dynamics, l.reward, injected
                            );
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
                    if DQNAgent::poll_regime_shift_file(&artifact_path_string(
                        "scematica-regime-shift.json",
                    )) {
                        if let Ok(mut ag) = agent.lock() {
                            ag.notify_regime_shift();
                        }
                    }
                }
            });
        }
    }
    // ─────────────────────────────────────────────────────────────────────────

    // Trade log rotation task: archive scematica-trades.jsonl when it exceeds 10,000 lines.
    // Archives to scematica-trades.jsonl.bak-<timestamp> and starts fresh so the NN
    // observer and dashboard don't read GB-sized files on long-running sessions.
    {
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(60));
            const MAX_LINES: usize = 10_000;
            loop {
                interval.tick().await;
                let trades_path = artifact_path(TRADES_FILE);
                if let Ok(contents) = std::fs::read_to_string(&trades_path) {
                    let line_count = contents.lines().count();
                    if line_count >= MAX_LINES {
                        let ts = chrono::Utc::now().format("%Y%m%d-%H%M%S");
                        let archive = artifact_path(format!("{}.bak-{}", TRADES_FILE, ts));
                        if std::fs::rename(&trades_path, &archive).is_ok() {
                            info!(
                                line_count,
                                archive = %archive.display(),
                                "Trade log rotated — archived and starting fresh"
                            );
                        }
                    }
                }
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
        info!(
            "👁 Whale copy listener started ({} wallets)",
            config.sniper.copy_wallets.len()
        );
    }

    // Spawn listener
    let ws_url = config.rpc.ws_endpoint.clone();
    let wallet_pubkey = wallet_kp.pubkey();
    let event_tx_clone = event_tx.clone();
    tokio::spawn(async move {
        loop {
            let listener =
                PoolListener::new(&ws_url, wallet_pubkey, quote_mint, event_tx_clone.clone());
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
                            ath as f64 / 1e9,
                            ath
                        );
                        last_ath = ath;
                    }
                    let dd = sniper_ath.ath_tracker.drawdown_pct(balance);
                    if dd > 5.0 {
                        debug!(
                            "ATH drawdown: {:.1}% (current={:.4} SOL, ATH={:.4} SOL)",
                            dd,
                            balance as f64 / 1e9,
                            ath as f64 / 1e9
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
            if sniper_gb.grief_breaker.is_none() {
                return;
            }
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

    // ── Pump.fun graduation monitor (legacy poll-based) ──────────────────────
    // Enabled when PUMPFUN_MONITOR=1 env var is set.
    // Superseded by pumpfun_trending_enabled in config; kept for fallback.
    if std::env::var("PUMPFUN_MONITOR").as_deref() == Ok("1")
        && !config.sniper.pumpfun_trending_enabled
    {
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
        info!("Pump.fun graduation monitor started (threshold=82 SOL)");
    }

    // ── Pump.fun trending monitor (PumpPortal WebSocket) ─────────────────────
    // Real-time buy/sell velocity tracking per bonding curve token.
    // Emits CachedPool immediately on graduation when token was pre-screened as
    // trending — typically 0.5–3 s ahead of the Raydium AMM V4 listener.
    if config.sniper.pumpfun_trending_enabled {
        use scematica_sniper::pumpfun_trending::{PumpFunTrendingConfig, PumpFunTrendingMonitor};
        let pf_tx = event_tx.clone();
        let rpc_url = config.rpc.endpoint.clone();
        let pf_cfg = PumpFunTrendingConfig {
            min_trending_score: config.sniper.pumpfun_trending_score,
            min_curve_pct: config.sniper.pumpfun_min_curve_pct,
            track_window_secs: config.sniper.pumpfun_window_secs,
            max_migration_age_secs: config.sniper.pumpfun_max_migration_age_secs,
            min_recent_buys: config.sniper.pumpfun_min_recent_buys,
            min_net_buy_sol: config.sniper.pumpfun_min_net_buy_sol,
            max_last_buy_age_secs: config.sniper.pumpfun_max_last_buy_age_secs,
            max_tracked_tokens: 300,
        };
        info!(
            "PumpFun trending monitor starting (min_score={:.0}, min_curve={:.0}%, min_buys={}, min_net_buy={:.2} SOL)",
            pf_cfg.min_trending_score,
            pf_cfg.min_curve_pct,
            pf_cfg.min_recent_buys,
            pf_cfg.min_net_buy_sol,
        );
        tokio::spawn(async move {
            let monitor = PumpFunTrendingMonitor::new(rpc_url, pf_tx, pf_cfg);
            if let Err(e) = monitor.run().await {
                warn!("PumpFun trending monitor exited: {}", e);
            }
        });
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

                    let start = sniper_pe
                        .session_start_lamports
                        .load(std::sync::atomic::Ordering::Relaxed);
                    if start == 0 {
                        continue;
                    }

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
                extract_pct,
                extract_threshold,
                &extract_wallet_log[..8.min(extract_wallet_log.len())]
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
