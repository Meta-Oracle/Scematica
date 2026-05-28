use anyhow::Result;
use clap::Parser;
use crossterm::{
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use scematica_ai::{AiClient, ChatAgent};
use scematica_ai::chat_types::AgentOutput;
use scematica_ai::prompts::CHAT_AGENT_SYSTEM;
use scematica_ai::tool_dispatcher::ToolDispatcher;
use scematica_ai::types::ChatMessage;
use scematica_core::metrics::BotMetrics;
use scematica_core::token::raw_to_ui;
use scematica_core::types::known_tokens;
use scematica_dashboard::{
    app::{AppState, BotMode},
    chat::{ChatLine, ChatUpdate},
    demo::run_demo,
    events::{handle_key, spawn_event_reader, AppEvent, DashboardAction},
    onboarding::OnboardingStep,
    process::{run_process_manager, BotCommand},
    ui::render,
};
use solana_sdk::{commitment_config::CommitmentConfig, signature::Signer};
use std::{io, sync::Arc};
use tokio::sync::mpsc;

#[derive(Parser, Debug)]
#[command(name = "scematica-dashboard", about = "Scematica Terminal Dashboard")]
struct Args {
    /// Tick rate in milliseconds
    #[arg(long, default_value = "250")]
    tick_rate: u64,

    /// Run in demo mode — simulates trading without real RPC calls or a wallet keypair
    #[arg(long)]
    demo: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let metrics = Arc::new(BotMetrics::new());

    let state = if args.demo {
        // Demo mode: no keypair or RPC required
        let rpc = Arc::new(scematica_core::rpc::RpcConnection::new(
            "https://api.mainnet-beta.solana.com",
            CommitmentConfig::confirmed(),
        ));
        let s = AppState::new((*metrics).clone(), rpc);
        s.onboarding.write().current_step = OnboardingStep::Completed;

        let demo_state = s.clone();
        tokio::spawn(async move { run_demo(demo_state).await });

        s
    } else {
        let config = scematica_core::config::BotConfig::from_env()?;
        let wallet = scematica_core::wallet::Wallet::from_source(&config.wallet.keypair_path)?;
        let wallet_pubkey = wallet.keypair.pubkey();

        let rpc = Arc::new(scematica_core::rpc::RpcConnection::new(
            &config.rpc.endpoint,
            CommitmentConfig::confirmed(),
        ));
        let s = AppState::new((*metrics).clone(), rpc);
        s.onboarding.write().current_step = OnboardingStep::Completed;

        *s.wallet_address.write() = wallet_pubkey.to_string();
        let state_clone = s.clone();
        tokio::spawn(async move {
            use solana_client::rpc_request::TokenAccountsFilter;
            let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(5));
            loop {
                interval.tick().await;
                if let Ok(balance) = state_clone.rpc.get_sol_balance(&wallet_pubkey).await {
                    *state_clone.sol_balance.write() = balance as f64 / 1_000_000_000.0;
                }
                // SCEMA is Token-2022 — ATA address differs from legacy SPL.
                // Use owner query so we find it regardless of token program.
                if let Ok(accounts) = state_clone.rpc.client
                    .get_token_accounts_by_owner(
                        &wallet_pubkey,
                        TokenAccountsFilter::Mint(known_tokens::SCEMATICA_MINT),
                    )
                    .await
                {
                    let mut total = 0.0f64;
                    for keyed in &accounts {
                        if let Ok(pk) = keyed.pubkey.parse::<solana_sdk::pubkey::Pubkey>() {
                            if let Ok(raw) = state_clone.rpc.get_token_balance(&pk).await {
                                total += raw_to_ui(raw, known_tokens::SCEMATICA_DECIMALS);
                            }
                        }
                    }
                    *state_clone.scematica_balance.write() = total;
                }
            }
        });

        // SOL/USD price feed — polls CoinGecko simple-price endpoint every 60 s.
        // Runs in a separate task so a slow/failing API never stalls the UI tick.
        {
            let price_state = s.clone();
            tokio::spawn(async move {
                let client = reqwest::Client::builder()
                    .timeout(std::time::Duration::from_secs(10))
                    .build()
                    .unwrap_or_default();
                let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(60));
                loop {
                    interval.tick().await;
                    let result = client
                        .get("https://api.coingecko.com/api/v3/simple/price?ids=solana&vs_currencies=usd")
                        .send()
                        .await;
                    if let Ok(resp) = result {
                        if let Ok(json) = resp.json::<serde_json::Value>().await {
                            if let Some(price) = json["solana"]["usd"].as_f64() {
                                *price_state.sol_price_usd.write() = price;
                            }
                        }
                    }
                }
            });
        }

        s.push_log(format!("[INFO] Scematica dashboard | Wallet: {}", wallet_pubkey));

        // Process manager — owns child process handles, responds to BotCommand via mpsc
        let (bot_tx, bot_rx) = mpsc::channel::<BotCommand>(16);
        *s.bot_cmd_tx.write() = Some(bot_tx);
        let pm_state = s.clone();
        tokio::spawn(async move {
            run_process_manager(bot_rx, pm_state).await;
        });

        s
    };

    // AI worker — receives ChatUpdate commands, runs the agent, writes results back to state
    let (chat_tx, mut chat_rx) = mpsc::channel::<ChatUpdate>(32);
    *state.chat_tx.write() = Some(chat_tx);

    let live_data = Arc::clone(&state.live_data);
    let ai_state = state.clone();
    tokio::spawn(async move {
        let client = match AiClient::from_env() {
            Ok(c) => c,
            Err(e) => {
                ai_state.push_chat_line(ChatLine::Error(format!(
                    "AI unavailable: {}. Set ANTHROPIC_API_KEY in .env",
                    e
                )));
                return;
            }
        };

        ai_state.push_chat_line(ChatLine::Bot(format!(
            "Connected to {} ({}). Ask me about your wallet, trades, or bot status.",
            client.provider_name(),
            client.model
        )));

        let system = ChatMessage::system(CHAT_AGENT_SYSTEM);
        let history = scematica_ai::conversation::ConversationHistory::new(system, 50);
        let dispatcher = ToolDispatcher::with_live_data(Arc::clone(&live_data));
        let mut agent = ChatAgent::new(client, history, dispatcher);

        while let Some(update) = chat_rx.recv().await {
            match update {
                ChatUpdate::Send(text) => {
                    *ai_state.is_ai_loading.write() = true;
                    match agent.process(&text).await {
                        Ok(AgentOutput::Reply(r)) => {
                            *ai_state.is_ai_loading.write() = false;
                            ai_state.push_chat_line(ChatLine::Bot(r.message));
                        }
                        Ok(AgentOutput::NeedsConfirmation(p)) => {
                            *ai_state.is_ai_loading.write() = false;
                            let risk_str = format!("{:?}", p.risk);
                            ai_state.push_chat_line(ChatLine::Pending {
                                summary: p.summary.clone(),
                                risk: risk_str,
                            });
                            *ai_state.chat_pending.write() = Some(p);
                        }
                        Err(e) => {
                            *ai_state.is_ai_loading.write() = false;
                            ai_state.push_chat_line(ChatLine::Error(e.to_string()));
                        }
                    }
                }
                ChatUpdate::Confirm => {
                    *ai_state.chat_pending.write() = None;
                    *ai_state.is_ai_loading.write() = true;
                    match agent.confirm_pending().await {
                        Ok(AgentOutput::Reply(r)) => {
                            *ai_state.is_ai_loading.write() = false;
                            ai_state.push_chat_line(ChatLine::Bot(r.message));
                        }
                        Ok(AgentOutput::NeedsConfirmation(p)) => {
                            *ai_state.is_ai_loading.write() = false;
                            let risk_str = format!("{:?}", p.risk);
                            ai_state.push_chat_line(ChatLine::Pending {
                                summary: p.summary.clone(),
                                risk: risk_str,
                            });
                            *ai_state.chat_pending.write() = Some(p);
                        }
                        Err(e) => {
                            *ai_state.is_ai_loading.write() = false;
                            ai_state.push_chat_line(ChatLine::Error(e.to_string()));
                        }
                    }
                }
                ChatUpdate::Reject => {
                    *ai_state.chat_pending.write() = None;
                    let msg = agent.reject_pending();
                    ai_state.push_chat_line(ChatLine::Bot(msg));
                }
            }
        }
    });

    // Event channel
    let (event_tx, mut event_rx) = mpsc::channel::<AppEvent>(100);
    spawn_event_reader(event_tx, args.tick_rate);

    // Main render loop
    loop {
        terminal.draw(|f| render(f, &state))?;

        let current_tab = *state.selected_tab.read();

        if let Some(event) = event_rx.recv().await {
            match event {
                AppEvent::Key(key) => {
                    // If onboarding is still showing, Enter advances it and other keys are blocked
                    let onboarding_done = state.onboarding.read().current_step == OnboardingStep::Completed;
                    if !onboarding_done {
                        match key.code {
                            crossterm::event::KeyCode::Enter => {
                                state.onboarding.write().next();
                            }
                            crossterm::event::KeyCode::Esc | crossterm::event::KeyCode::Char('q') => {
                                // Skip straight to done
                                let mut ob = state.onboarding.write();
                                ob.current_step = OnboardingStep::Completed;
                            }
                            _ => {}
                        }
                        continue;
                    }

                    let has_pending = state.chat_pending.read().is_some();
                    let log_filter_active = *state.log_filter_active.read();
                    if let Some(action) = handle_key(key, current_tab, has_pending, log_filter_active) {
                        match action {
                            DashboardAction::Quit => break,
                            DashboardAction::NextTab => state.next_tab(),
                            DashboardAction::PrevTab => state.prev_tab(),
                            DashboardAction::ChatChar(c) => {
                                state.chat_input.write().push(c);
                            }
                            DashboardAction::ChatBackspace => {
                                state.chat_input.write().pop();
                            }
                            DashboardAction::ChatSend => {
                                let input = {
                                    let mut lock = state.chat_input.write();
                                    let text = lock.trim().to_string();
                                    lock.clear();
                                    text
                                };
                                if !input.is_empty() {
                                    state.push_chat_line(ChatLine::User(input.clone()));
                                    if let Some(tx) = state.chat_tx.read().as_ref() {
                                        let _ = tx.try_send(ChatUpdate::Send(input));
                                    }
                                }
                            }
                            DashboardAction::ChatConfirm => {
                                if state.chat_pending.read().is_some() {
                                    if let Some(tx) = state.chat_tx.read().as_ref() {
                                        let _ = tx.try_send(ChatUpdate::Confirm);
                                    }
                                }
                            }
                            DashboardAction::ChatReject => {
                                if state.chat_pending.read().is_some() {
                                    if let Some(tx) = state.chat_tx.read().as_ref() {
                                        let _ = tx.try_send(ChatUpdate::Reject);
                                    }
                                }
                            }
                            DashboardAction::StartBot(mode) => {
                                if let Some(tx) = state.bot_cmd_tx.read().as_ref() {
                                    let _ = tx.try_send(BotCommand::Start(mode));
                                }
                            }
                            DashboardAction::StopBot => {
                                if let Some(tx) = state.bot_cmd_tx.read().as_ref() {
                                    let _ = tx.try_send(BotCommand::Stop);
                                }
                            }
                            DashboardAction::ToggleSellMode => {
                                let currently = *state.sell_mode_active.read();
                                let next = !currently;
                                *state.sell_mode_active.write() = next;
                                const SELL_MODE_FILE: &str = "scematica-sell-mode.json";
                                if next {
                                    let _ = std::fs::write(SELL_MODE_FILE, r#"{"active":true}"#);
                                    state.push_log(
                                        "[SELL MODE] Emergency sell mode ACTIVATED — buying paused, selling all positions".to_string()
                                    );
                                } else {
                                    let _ = std::fs::remove_file(SELL_MODE_FILE);
                                    state.push_log(
                                        "[SELL MODE] Sell mode DEACTIVATED — resuming normal operation".to_string()
                                    );
                                }
                            }
                            DashboardAction::BuyMode => {
                                // Force-clear sell mode regardless of which subsystem set it.
                                // Inspect the file (if any) so the log entry tells the operator
                                // what they just overrode — drawdown? buy_limit? manual?
                                const SELL_MODE_FILE: &str = "scematica-sell-mode.json";
                                let prior_reason = std::fs::read_to_string(SELL_MODE_FILE)
                                    .ok()
                                    .and_then(|s| {
                                        serde_json::from_str::<serde_json::Value>(&s).ok()
                                    })
                                    .and_then(|v| v.get("reason").and_then(|r| r.as_str().map(String::from)))
                                    .unwrap_or_else(|| "manual".to_string());
                                let existed = std::path::Path::new(SELL_MODE_FILE).exists();
                                let _ = std::fs::remove_file(SELL_MODE_FILE);
                                *state.sell_mode_active.write() = false;
                                if existed {
                                    state.push_log(format!(
                                        "[BUY MODE] Sell-mode cleared (was: {}) — buying re-enabled",
                                        prior_reason,
                                    ));
                                } else {
                                    state.push_log(
                                        "[BUY MODE] No sell-mode file present; buying already enabled".to_string()
                                    );
                                }
                            }
                            DashboardAction::ToggleHighSpeed => {
                                const HS_FILE: &str = "scematica-highspeed-mode.json";
                                let currently = *state.high_speed_active.read();
                                let next = !currently;
                                *state.high_speed_active.write() = next;
                                if next {
                                    let _ = std::fs::write(HS_FILE, r#"{"active":true}"#);
                                    state.push_log(
                                        "[HIGH-SPEED] ⚡ ENGAGED — filters/AI/scorer bypassed, fee escalated, parallel buys. Expect 429s.".to_string()
                                    );
                                } else {
                                    let _ = std::fs::remove_file(HS_FILE);
                                    state.push_log(
                                        "[HIGH-SPEED] Disengaged — normal filter pipeline restored".to_string()
                                    );
                                }
                            }
                            DashboardAction::AutoDump => {
                                let currently = *state.dump_mode_active.read();
                                let next = !currently;
                                *state.dump_mode_active.write() = next;
                                const DUMP_MODE_FILE: &str = "scematica-dump-mode.json";
                                if next {
                                    let _ = std::fs::write(DUMP_MODE_FILE, r#"{"active":true}"#);
                                    state.push_log(
                                        "[DUMP MODE] AUTO DUMP ACTIVATED — force-selling ALL positions with zero slippage".to_string()
                                    );
                                } else {
                                    let _ = std::fs::remove_file(DUMP_MODE_FILE);
                                    state.push_log(
                                        "[DUMP MODE] Auto dump DEACTIVATED".to_string()
                                    );
                                }
                            }
                            DashboardAction::ExportCsv => {
                                match state.export_trades_csv() {
                                    Ok(path) => state.push_log(format!("[EXPORT] Trades saved to {}", path)),
                                    Err(e) => state.push_log(format!("[EXPORT] Failed: {}", e)),
                                }
                            }
                            DashboardAction::ResetPositions => {
                                // Back up the trades file, truncate it, and clear the in-memory
                                // deque + offset. Next poll will be a no-op (file is empty).
                                use std::fs;
                                let ts = chrono::Utc::now().format("%Y%m%d-%H%M%S");
                                let backup = format!("scematica-trades.jsonl.bak-{}", ts);
                                let backup_ok = fs::rename("scematica-trades.jsonl", &backup).is_ok();
                                let trunc_ok = fs::write("scematica-trades.jsonl", []).is_ok();
                                state.trades.write().clear();
                                *state.trade_file_offset.write() = 0;
                                state.push_log(format!(
                                    "[RESET] Position counter cleared (backup: {}, ok={})",
                                    if backup_ok { backup } else { "skipped".to_string() },
                                    trunc_ok,
                                ));
                            }
                            DashboardAction::LogFilterActivate => {
                                *state.log_filter_active.write() = true;
                            }
                            DashboardAction::LogFilterChar(c) => {
                                state.log_filter.write().push(c);
                            }
                            DashboardAction::LogFilterBackspace => {
                                let mut f = state.log_filter.write();
                                if f.pop().is_none() {
                                    drop(f);
                                    *state.log_filter_active.write() = false;
                                }
                            }
                            DashboardAction::LogFilterClear => {
                                state.log_filter.write().clear();
                                *state.log_filter_active.write() = false;
                            }
                            DashboardAction::ConfigScrollUp => {
                                let mut s = state.config_scroll.write();
                                *s = s.saturating_sub(2);
                            }
                            DashboardAction::ConfigScrollDown => {
                                let mut s = state.config_scroll.write();
                                *s = s.saturating_add(2);
                            }
                            DashboardAction::SetRateMode(mode) => {
                                *state.rate_mode.write() = mode;
                                let json = serde_json::json!({
                                    "mode": mode.as_str(),
                                    "multiplier": mode.multiplier(),
                                    "tp_pct": mode.tp_pct(),
                                    "sl_pct": mode.sl_pct(),
                                    "quote_amount": mode.buy_sol(),
                                    "wallet_pct": mode.wallet_pct(),
                                });
                                let _ = std::fs::write("scematica-rate-mode.json", json.to_string());
                                state.push_log(format!(
                                    "[RATE] Mode → {}  |  {:.1}% wallet ({:.3} SOL base)  |  TP {:.0}%  SL {:.0}%",
                                    mode.label(), mode.wallet_pct(), mode.buy_sol(), mode.tp_pct(), mode.sl_pct()
                                ));
                            }
                            DashboardAction::ToggleMoonChase => {
                                let new_val = !*state.moon_chase.read();
                                *state.moon_chase.write() = new_val;
                                if new_val {
                                    let json = serde_json::json!({
                                        "active": true,
                                        "max_escalations": 8,
                                        "escalation_factor": 1.75,
                                        "pullback_exit_pct": 25.0,
                                        "escalation_threshold_pct": 3.0,
                                    });
                                    let _ = std::fs::write("scematica-moon-chase.json", json.to_string());
                                    state.push_log("[MOON CHASE] 🌙 ENGAGED — 8 escalations × 1.75×, pullback 25%, threshold 3%/check");
                                } else {
                                    let _ = std::fs::remove_file("scematica-moon-chase.json");
                                    state.push_log("[MOON CHASE] disengaged — momentum-hold back to EV-optimal params");
                                }
                            }
                            DashboardAction::SetBuilderMode(bm) => {
                                *state.builder_mode.write() = bm;
                                if bm == scematica_dashboard::app::BuilderMode::Off {
                                    let _ = std::fs::remove_file("scematica-builder-mode.json");
                                    state.push_log("[BUILDER] Mode → Off (sniper uses config.toml wallet_target_sol)");
                                } else {
                                    let json = serde_json::json!({
                                        "mode": bm.as_str(),
                                        "target_sol": bm.target_sol(),
                                        "progressive_scaling": bm.progressive(),
                                    });
                                    let _ = std::fs::write("scematica-builder-mode.json", json.to_string());
                                    state.push_log(format!(
                                        "[BUILDER] Mode → {}  |  target {:.1} SOL  |  progressive scaling: {}",
                                        bm.label(), bm.target_sol(), if bm.progressive() { "ON" } else { "OFF" },
                                    ));
                                }
                            }
                        }
                    }
                }
                AppEvent::Tick => {
                    state.poll_metrics_file();
                    state.poll_trade_file();
                    state.poll_strategy_file();
                    // Only tail scematica-sniper.log when the sniper is NOT a child of
                    // this dashboard. When dashboard-managed, the process manager pipes
                    // the child's stderr straight into the log buffer (see process.rs).
                    // Polling the file as well would push every line twice — that was the
                    // "stuck displaying the metric over and over" footgun.
                    if matches!(*state.active_mode.read(), BotMode::Idle) {
                        state.poll_log_file();
                    }
                    state.poll_filter_stats_file();
                    state.poll_nn_stats_file();
                    state.poll_nn_advice_file();
                    state.poll_tournament_file();
                    state.poll_deployer_reputation_file();
                    state.poll_pool_decision_file();
                    state.poll_tx_telemetry_file();
                    state.poll_radar_file();
                    state.poll_live_positions_file();
                    state.sync_live_data();

                    // Mirror the high-speed-mode file into local state so the UI label
                    // reflects external changes (e.g. user deletes the file by hand,
                    // or another tool toggles it).
                    let hs_now = std::path::Path::new("scematica-highspeed-mode.json").exists();
                    if *state.high_speed_active.read() != hs_now {
                        *state.high_speed_active.write() = hs_now;
                    }

                    // (Removed) auto-sell-mode trigger on low SOL balance.
                    // The sniper's own buy gate (sniper.rs::buy) already refuses to buy
                    // when native_balance < quote_amount + 6_000_000 lamports. Re-writing
                    // the sell-mode file every ~250 ms here would (and did) overwrite Buy
                    // Mode the instant the user pressed [b], creating an unbreakable loop.
                }
                AppEvent::Quit => break,
            }
        }

        if state.is_quitting() {
            break;
        }
    }

    // Restore terminal
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    Ok(())
}
