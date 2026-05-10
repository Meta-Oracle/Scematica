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

/// Handle a key event and return true if the app should quit
pub fn handle_key(key: KeyEvent) -> Option<DashboardAction> {
    match key.code {
        KeyCode::Char('q') | KeyCode::Esc => Some(DashboardAction::Quit),
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            Some(DashboardAction::Quit)
        }
        KeyCode::Tab | KeyCode::Right => Some(DashboardAction::NextTab),
        KeyCode::BackTab | KeyCode::Left => Some(DashboardAction::PrevTab),
        _ => None,
    }
}

#[derive(Debug, Clone)]
pub enum DashboardAction {
    Quit,
    NextTab,
    PrevTab,
}
