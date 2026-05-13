use anyhow::Result;
use clap::Parser;
use crossterm::{
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use scematica_core::metrics::BotMetrics;
use scematica_core::token::{get_ata, raw_to_ui};
use scematica_core::types::known_tokens;
use scematica_dashboard::{
    app::AppState,
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

            // SOL balance
            if let Ok(balance) = state_clone.rpc.get_sol_balance(&wallet_pubkey).await {
                *state_clone.sol_balance.write() = balance as f64 / 1_000_000_000.0;
            }

            // SCEMATICA token balance via ATA
            if let Ok(raw) = state_clone.rpc.get_token_balance(&scematica_ata).await {
                *state_clone.scematica_balance.write() =
                    raw_to_ui(raw, known_tokens::SCEMATICA_DECIMALS);
            }
        }
    });

    state.push_log(format!("[INFO] Scematica dashboard | Wallet: {}", wallet_pubkey));

    // Event channel
    let (event_tx, mut event_rx) = mpsc::channel::<AppEvent>(100);
    spawn_event_reader(event_tx, args.tick_rate);

    // Main render loop
    loop {
        terminal.draw(|f| render(f, &state))?;

        if let Some(event) = event_rx.recv().await {
            match event {
                AppEvent::Key(key) => {
                    if let Some(action) = handle_key(key) {
                        match action {
                            DashboardAction::Quit => break,
                            DashboardAction::NextTab => state.next_tab(),
                            DashboardAction::PrevTab => state.prev_tab(),
                        }
                    }
                }
                AppEvent::Tick => {
                    state.poll_metrics_file();
                    state.poll_trade_file();
                    state.poll_strategy_file();
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
