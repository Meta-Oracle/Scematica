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
use scematica_core::token::{get_ata, raw_to_ui};
use scematica_core::types::known_tokens;
use scematica_dashboard::{
    app::AppState,
    chat::{ChatLine, ChatUpdate},
    events::{handle_key, spawn_event_reader, AppEvent, DashboardAction},
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

    let config = scematica_core::config::BotConfig::from_env()?;
    let wallet = scematica_core::wallet::Wallet::from_source(&config.wallet.keypair_path)?;
    let wallet_pubkey = wallet.keypair.pubkey();

    let metrics = Arc::new(BotMetrics::new());
    let rpc = Arc::new(scematica_core::rpc::RpcConnection::new(
        &config.rpc.endpoint,
        CommitmentConfig::confirmed(),
    ));
    let state = AppState::new((*metrics).clone(), rpc);

    // Live sync — SOL balance + SCEMATICA token balance every 5s
    *state.wallet_address.write() = wallet_pubkey.to_string();
    let state_clone = state.clone();
    tokio::spawn(async move {
        let scematica_ata = get_ata(&wallet_pubkey, &known_tokens::SCEMATICA_MINT);
        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(5));
        loop {
            interval.tick().await;

            if let Ok(balance) = state_clone.rpc.get_sol_balance(&wallet_pubkey).await {
                *state_clone.sol_balance.write() = balance as f64 / 1_000_000_000.0;
            }

            if let Ok(raw) = state_clone.rpc.get_token_balance(&scematica_ata).await {
                *state_clone.scematica_balance.write() =
                    raw_to_ui(raw, known_tokens::SCEMATICA_DECIMALS);
            }
        }
    });

    state.push_log(format!("[INFO] Scematica dashboard | Wallet: {}", wallet_pubkey));

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
                    "AI unavailable: {}. Set XAI_API_KEY or GROQ_API_KEY.",
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
                    if let Some(action) = handle_key(key, current_tab) {
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
                        }
                    }
                }
                AppEvent::Tick => {
                    state.poll_metrics_file();
                    state.poll_trade_file();
                    state.poll_strategy_file();
                    state.sync_live_data();
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
