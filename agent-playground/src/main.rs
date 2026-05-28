use anyhow::Result;
use clap::Parser;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::io;
use tokio::sync::mpsc;

mod agent;
mod app;
mod arena;
mod config;
mod llm;
mod ui;

use app::App;
use arena::{ArenaEvent, PlaygroundCommand};
use config::PlaygroundConfig;

#[derive(Parser, Debug)]
#[command(
    name = "playground",
    about = "Multi-LLM Agent-to-Agent Arena — free backends: Ollama, Groq, OpenRouter, LM Studio, Cerebras"
)]
struct Cli {
    /// Path to config file (JSON or TOML)
    #[arg(long, default_value = "playground.json")]
    config: String,

    /// Start with a demo conversation (3 philosophers debate AGI using Ollama/phi3)
    #[arg(long)]
    demo: bool,

    /// Save an example config file and exit
    #[arg(long)]
    init: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    if cli.init {
        PlaygroundConfig::save_example("playground.json")?;
        println!("Example config written to playground.json");
        println!("Edit it and run: cargo run --bin playground");
        return Ok(());
    }

    let config = if std::path::Path::new(&cli.config).exists() {
        match PlaygroundConfig::from_file(&cli.config) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("Warning: could not load config ({}), using defaults.", e);
                PlaygroundConfig::default()
            }
        }
    } else {
        PlaygroundConfig::default()
    };

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = run(&mut terminal, config, cli.demo).await;

    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    result
}

async fn run<B: ratatui::backend::Backend>(
    terminal: &mut Terminal<B>,
    config: PlaygroundConfig,
    demo: bool,
) -> Result<()> {
    let (cmd_tx, cmd_rx) = mpsc::channel::<PlaygroundCommand>(128);
    let (event_tx, event_rx) = mpsc::channel::<ArenaEvent>(512);

    let mut app = App::new(config, cmd_tx.clone(), event_rx);

    if demo {
        app.load_demo();
    }

    // Sync the arena's initial state from app (agents may have been changed by load_demo)
    let arena_config = app.arena_config();

    // Arena runs in a background task; sends ArenaEvents back, receives PlaygroundCommands
    let bg_event_tx = event_tx.clone();
    tokio::spawn(async move {
        if let Err(e) = arena::run_arena(arena_config, cmd_rx, bg_event_tx.clone()).await {
            let _ = bg_event_tx
                .send(ArenaEvent::StatusUpdate(format!("Arena stopped: {}", e)))
                .await;
        }
    });

    loop {
        app.poll_events();
        terminal.draw(|f| ui::render(f, &app))?;

        // Non-blocking key poll at ~20 fps
        if event::poll(std::time::Duration::from_millis(50))? {
            if let Event::Key(key) = event::read()? {
                app.handle_key(key).await?;
            }
        }

        if app.should_quit {
            break;
        }
    }

    Ok(())
}
