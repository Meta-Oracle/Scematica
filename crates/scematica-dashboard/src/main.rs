use anyhow::Result;
use clap::Parser;
use crossterm::{
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use scematica_core::metrics::BotMetrics;
use scematica_dashboard::{
    app::{AppState, BotMode},
    events::{handle_key, spawn_event_reader, AppEvent, DashboardAction},
    ui::render,
};
use std::{io, sync::Arc};
use tokio::sync::mpsc;
use tracing::info;

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

    let metrics = Arc::new(BotMetrics::new());
    let state = AppState::new(metrics);

    // Seed some demo data
    *state.wallet_address.write() = "7gm6BPQrSBaTAYaJheuRevBNXcmKsgbkfBCVSjBnt9aP".into();
    *state.sol_balance.write() = 1.234;
    *state.active_mode.write() = BotMode::Both;
    state.push_log("[INFO] Scematica dashboard started");
    state.push_log("[INFO] Connecting to RPC...");
    state.push_log("[INFO] Wallet loaded");

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
                    // Refresh metrics from shared state (in production, read from Arc<BotMetrics>)
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
