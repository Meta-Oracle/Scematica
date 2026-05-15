use std::path::PathBuf;
use std::sync::Arc;
use tokio::process::{Child, Command};
use tokio::sync::mpsc;
use crate::app::{AppState, BotMode};

pub enum BotCommand {
    Start(BotMode),
    Stop,
}

/// Long-running task that owns child process handles and responds to start/stop commands.
/// Updates `AppState.active_mode` as children come up or exit.
pub async fn run_process_manager(
    mut cmd_rx: mpsc::Receiver<BotCommand>,
    state: Arc<AppState>,
) {
    let mut sniper: Option<Child> = None;
    let mut arb: Option<Child> = None;

    loop {
        // Poll for exits on every iteration (non-blocking)
        poll_exit(&mut sniper, "Sniper", &state);
        poll_exit(&mut arb, "Arb", &state);
        sync_mode(&state, sniper.is_some(), arb.is_some());

        // Wait up to 500 ms for a command, then loop to re-check children
        match tokio::time::timeout(
            tokio::time::Duration::from_millis(500),
            cmd_rx.recv(),
        )
        .await
        {
            Ok(Some(BotCommand::Start(mode))) => {
                // Kill whatever is currently running
                kill(&mut sniper, "Sniper", &state).await;
                kill(&mut arb, "Arb", &state).await;

                if matches!(mode, BotMode::Sniper | BotMode::Both) {
                    match launch("sniper", &state) {
                        Ok(child) => { sniper = Some(child); }
                        Err(e) => state.push_log(format!("[ERROR] {}", e)),
                    }
                }
                if matches!(mode, BotMode::Arb | BotMode::Both) {
                    match launch("arb", &state) {
                        Ok(child) => { arb = Some(child); }
                        Err(e) => state.push_log(format!("[ERROR] {}", e)),
                    }
                }
                sync_mode(&state, sniper.is_some(), arb.is_some());
            }
            Ok(Some(BotCommand::Stop)) => {
                kill(&mut sniper, "Sniper", &state).await;
                kill(&mut arb, "Arb", &state).await;
                state.push_log("[BOT] All bots stopped");
                sync_mode(&state, false, false);
            }
            Ok(None) => break, // sender dropped — dashboard is shutting down
            Err(_) => {}       // timeout, just loop
        }
    }

    // Clean up on shutdown
    kill(&mut sniper, "Sniper", &state).await;
    kill(&mut arb, "Arb", &state).await;
}

fn poll_exit(child: &mut Option<Child>, label: &str, state: &Arc<AppState>) {
    let done = if let Some(c) = child.as_mut() {
        match c.try_wait() {
            Ok(Some(status)) => {
                if status.success() {
                    state.push_log(format!("[BOT] {} stopped", label));
                } else {
                    state.push_log(format!("[BOT] {} exited with {}", label, status));
                }
                true
            }
            Ok(None) => false,
            Err(e) => {
                state.push_log(format!("[BOT] {} monitor error: {}", label, e));
                true
            }
        }
    } else {
        false
    };
    if done {
        *child = None;
    }
}

async fn kill(child: &mut Option<Child>, label: &str, state: &Arc<AppState>) {
    if let Some(mut c) = child.take() {
        match c.kill().await {
            Ok(_) => state.push_log(format!("[BOT] {} killed", label)),
            Err(e) => state.push_log(format!("[BOT] {} kill error: {}", label, e)),
        }
    }
}

fn sync_mode(state: &Arc<AppState>, sniper_up: bool, arb_up: bool) {
    let mode = match (sniper_up, arb_up) {
        (true, true)  => BotMode::Both,
        (true, false) => BotMode::Sniper,
        (false, true) => BotMode::Arb,
        _             => BotMode::Idle,
    };
    *state.active_mode.write() = mode;
}

fn launch(name: &str, state: &Arc<AppState>) -> anyhow::Result<Child> {
    let bin = find_binary(name)?;
    state.push_log(format!("[BOT] Starting {} ({:?})", name, bin));
    let mut child = Command::new(&bin)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn()?;

    // Stream child stderr into the dashboard log tab in real time
    if let Some(stderr) = child.stderr.take() {
        let state_clone = state.clone();
        let label = name.to_uppercase();
        tokio::spawn(async move {
            use tokio::io::{AsyncBufReadExt, BufReader};
            let mut lines = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                state_clone.push_log(format!("[{}] {}", label, line));
            }
            state_clone.push_log(format!("[{}] Process ended", label));
        });
    }

    Ok(child)
}

fn find_binary(name: &str) -> anyhow::Result<PathBuf> {
    let bin = format!("{}{}", name, if cfg!(windows) { ".exe" } else { "" });

    // Env var override: SCEMATICA_SNIPER_BIN / SCEMATICA_ARB_BIN
    let env_key = format!("SCEMATICA_{}_BIN", name.to_uppercase());
    if let Ok(p) = std::env::var(&env_key) {
        let path = PathBuf::from(&p);
        if path.exists() { return Ok(path); }
    }

    for dir in &["target/release", "target/debug"] {
        let path = PathBuf::from(dir).join(&bin);
        if path.exists() { return Ok(path); }
    }

    anyhow::bail!(
        "Cannot find '{}' binary. Build it first:\n  cargo build --bin {}",
        bin, name
    )
}
