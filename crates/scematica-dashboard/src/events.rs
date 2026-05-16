use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use std::time::Duration;
use tokio::sync::mpsc;
use crate::app::{BotMode, RateMode};

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
                    if key.kind == KeyEventKind::Press {
                        let _ = tx.blocking_send(AppEvent::Key(key));
                    }
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
/// `log_filter_active` controls whether typing goes to the log filter bar.
pub fn handle_key(key: KeyEvent, current_tab: usize, has_pending: bool, log_filter_active: bool) -> Option<DashboardAction> {
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
            // Log filter Esc must come before global Esc→Quit to avoid shadowing
            KeyCode::Esc if current_tab == 2 && log_filter_active => Some(DashboardAction::LogFilterClear),
            KeyCode::Char('q') | KeyCode::Esc => Some(DashboardAction::Quit),
            KeyCode::Tab | KeyCode::Right => Some(DashboardAction::NextTab),
            KeyCode::BackTab | KeyCode::Left => Some(DashboardAction::PrevTab),
            // Bot process controls — active on the Config tab (index 3)
            KeyCode::Char('s') if current_tab == 3 => Some(DashboardAction::StartBot(BotMode::Sniper)),
            KeyCode::Char('a') if current_tab == 3 => Some(DashboardAction::StartBot(BotMode::Arb)),
            KeyCode::Char('b') if current_tab == 3 => Some(DashboardAction::StartBot(BotMode::Both)),
            KeyCode::Char('x') if current_tab == 3 => Some(DashboardAction::StopBot),
            // Rate mode presets — [1][2][3][4] on Config tab
            KeyCode::Char('1') if current_tab == 3 => Some(DashboardAction::SetRateMode(RateMode::Safe)),
            KeyCode::Char('2') if current_tab == 3 => Some(DashboardAction::SetRateMode(RateMode::Balanced)),
            KeyCode::Char('3') if current_tab == 3 => Some(DashboardAction::SetRateMode(RateMode::Aggressive)),
            KeyCode::Char('4') if current_tab == 3 => Some(DashboardAction::SetRateMode(RateMode::Degen)),
            // [e] on the Logs tab toggles emergency sell mode
            KeyCode::Char('e') if current_tab == 2 && !log_filter_active => Some(DashboardAction::ToggleSellMode),
            // [d] on the Logs tab triggers auto dump — force-sells all positions immediately
            KeyCode::Char('d') if current_tab == 2 && !log_filter_active => Some(DashboardAction::AutoDump),
            // [/] on the Logs tab activates the filter bar
            KeyCode::Char('/') if current_tab == 2 => Some(DashboardAction::LogFilterActivate),
            // Typing into the filter bar
            KeyCode::Char(c) if current_tab == 2 && log_filter_active => Some(DashboardAction::LogFilterChar(c)),
            KeyCode::Backspace if current_tab == 2 && log_filter_active => Some(DashboardAction::LogFilterBackspace),
            // [x] on the Trades tab exports history to CSV
            KeyCode::Char('x') if current_tab == 1 => Some(DashboardAction::ExportCsv),
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
    StartBot(BotMode),
    StopBot,
    ToggleSellMode,
    AutoDump,
    SetRateMode(RateMode),
    ExportCsv,
    LogFilterActivate,
    LogFilterChar(char),
    LogFilterBackspace,
    LogFilterClear,
}
