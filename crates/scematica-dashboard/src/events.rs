use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use std::time::Duration;
use tokio::sync::mpsc;
use crate::app::{BotMode, BuilderMode, RateMode};

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
            // Rate mode presets — [1]–[7] on Config tab (least → most aggressive)
            KeyCode::Char('1') if current_tab == 3 => Some(DashboardAction::SetRateMode(RateMode::Bearish)),
            KeyCode::Char('2') if current_tab == 3 => Some(DashboardAction::SetRateMode(RateMode::Micro)),
            KeyCode::Char('3') if current_tab == 3 => Some(DashboardAction::SetRateMode(RateMode::Safe)),
            KeyCode::Char('4') if current_tab == 3 => Some(DashboardAction::SetRateMode(RateMode::Balanced)),
            KeyCode::Char('5') if current_tab == 3 => Some(DashboardAction::SetRateMode(RateMode::Aggressive)),
            KeyCode::Char('6') if current_tab == 3 => Some(DashboardAction::SetRateMode(RateMode::Degen)),
            KeyCode::Char('7') if current_tab == 3 => Some(DashboardAction::SetRateMode(RateMode::Bullish)),
            KeyCode::Char('8') if current_tab == 3 => Some(DashboardAction::SetRateMode(RateMode::Moon)),
            // Builder modes — [g] Growth (0.2), [j] Builder (1.0), [k] Super Builder (3.0), [o] Off
            KeyCode::Char('g') if current_tab == 3 => Some(DashboardAction::SetBuilderMode(BuilderMode::Growth)),
            KeyCode::Char('j') if current_tab == 3 => Some(DashboardAction::SetBuilderMode(BuilderMode::Builder)),
            KeyCode::Char('k') if current_tab == 3 => Some(DashboardAction::SetBuilderMode(BuilderMode::SuperBuilder)),
            KeyCode::Char('o') if current_tab == 3 => Some(DashboardAction::SetBuilderMode(BuilderMode::Off)),
            // [m] toggles Moon Chase — momentum-hold escalator goes parabolic-greedy
            KeyCode::Char('m') if current_tab == 3 => Some(DashboardAction::ToggleMoonChase),
            // [e] on the Logs tab toggles emergency sell mode
            KeyCode::Char('e') if current_tab == 2 && !log_filter_active => Some(DashboardAction::ToggleSellMode),
            // [b] on the Logs tab force-clears sell mode (resume buying). Deletes the
            // sell-mode file regardless of which subsystem set it (drawdown / buy_limit
            // / dashboard) — the operator is explicitly overriding the safety.
            KeyCode::Char('b') if current_tab == 2 && !log_filter_active => Some(DashboardAction::BuyMode),
            // [h] on the Logs tab toggles HIGH-SPEED MODE — bypasses filters/AI/scorer
            // and races for entries. Accepts higher 429 / fail rate as the trade-off.
            KeyCode::Char('h') if current_tab == 2 && !log_filter_active => Some(DashboardAction::ToggleHighSpeed),
            // [d] on the Logs tab triggers auto dump — force-sells all positions immediately
            KeyCode::Char('d') if current_tab == 2 && !log_filter_active => Some(DashboardAction::AutoDump),
            // [/] on the Logs tab activates the filter bar
            KeyCode::Char('/') if current_tab == 2 => Some(DashboardAction::LogFilterActivate),
            // Typing into the filter bar
            KeyCode::Char(c) if current_tab == 2 && log_filter_active => Some(DashboardAction::LogFilterChar(c)),
            KeyCode::Backspace if current_tab == 2 && log_filter_active => Some(DashboardAction::LogFilterBackspace),
            // [x] on the Trades tab exports history to CSV
            KeyCode::Char('x') if current_tab == 1 => Some(DashboardAction::ExportCsv),
            // [R] on the Trades tab resets the position counter — backs up + truncates
            // scematica-trades.jsonl and clears the in-memory deque. Use this when the
            // displayed open-position count is stale (e.g., wallet was emptied externally).
            KeyCode::Char('R') if current_tab == 1 => Some(DashboardAction::ResetPositions),
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
    BuyMode,
    ToggleHighSpeed,
    AutoDump,
    SetRateMode(RateMode),
    SetBuilderMode(BuilderMode),
    ToggleMoonChase,
    ExportCsv,
    ResetPositions,
    LogFilterActivate,
    LogFilterChar(char),
    LogFilterBackspace,
    LogFilterClear,
}
