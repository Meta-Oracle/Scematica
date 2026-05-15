use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use std::time::Duration;
use tokio::sync::mpsc;

#[derive(Debug, Clone)]
pub enum AppEvent {
    Key(KeyEvent),
    Tick,
    Quit,
}

/// Spawn a background task that reads terminal events and sends them on a channel
pub fn spawn_event_reader(tx: mpsc::Sender<AppEvent>, tick_rate_ms: u64) {
    std::thread::spawn(move || {
        let tick = Duration::from_millis(tick_rate_ms);
        loop {
            if event::poll(tick).unwrap_or(false) {
                if let Ok(Event::Key(key)) = event::read() {
                    let _ = tx.blocking_send(AppEvent::Key(key));
                }
            } else {
                let _ = tx.blocking_send(AppEvent::Tick);
            }
        }
    });
}

/// Handle a key event and return a dashboard action.
/// `current_tab` routes chat-specific keys.
/// `has_pending` controls whether `y`/`n` route to confirm/reject (vs typed into input).
pub fn handle_key(key: KeyEvent, current_tab: usize, has_pending: bool) -> Option<DashboardAction> {
    // Ctrl+C always quits
    if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
        return Some(DashboardAction::Quit);
    }

    if current_tab == 4 {
        match key.code {
            KeyCode::Esc => Some(DashboardAction::Quit),
            KeyCode::Tab => Some(DashboardAction::NextTab),
            KeyCode::BackTab => Some(DashboardAction::PrevTab),
            KeyCode::Enter => Some(DashboardAction::ChatSend),
            KeyCode::Backspace => Some(DashboardAction::ChatBackspace),
            // Only route y/n to confirm/reject when there is an active pending tool call
            KeyCode::Char('y') if has_pending && key.modifiers.is_empty() => Some(DashboardAction::ChatConfirm),
            KeyCode::Char('n') if has_pending && key.modifiers.is_empty() => Some(DashboardAction::ChatReject),
            KeyCode::Char(c) => Some(DashboardAction::ChatChar(c)),
            _ => None,
        }
    } else {
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => Some(DashboardAction::Quit),
            KeyCode::Tab | KeyCode::Right => Some(DashboardAction::NextTab),
            KeyCode::BackTab | KeyCode::Left => Some(DashboardAction::PrevTab),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub enum DashboardAction {
    Quit,
    NextTab,
    PrevTab,
    ChatChar(char),
    ChatBackspace,
    ChatSend,
    ChatConfirm,
    ChatReject,
}
