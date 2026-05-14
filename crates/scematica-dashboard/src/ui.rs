use crate::app::AppState;
use crate::components::{COLOR_BG, COLOR_ACCENT, COLOR_TEXT, LoaderSpinner};
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{
        Block, Borders, Cell, List, ListItem, Paragraph, Row, Table, Tabs, Wrap,
    },
    Frame,
};
use scematica_core::types::known_tokens;
use std::sync::Arc;

#[allow(dead_code)]
const SCEMATICA_LOGO: &str = r#"
 ███████╗ ██████╗███████╗███╗   ███╗ █████╗ ████████╗██╗ ██████╗ █████╗
 ██╔════╝██╔════╝██╔════╝████╗ ████║██╔══██╗╚══██╔══╝██║██╔════╝██╔══██╗
 ███████╗██║     █████╗  ██╔████╔██║███████║   ██║   ██║██║     ███████║
 ╚════██║██║     ██╔══╝  ██║╚██╔╝██║██╔══██║   ██║   ██║██║     ██╔══██║
 ███████║╚██████╗███████╗██║ ╚═╝ ██║██║  ██║   ██║   ██║╚██████╗██║  ██║
 ╚══════╝ ╚═════╝╚══════╝╚═╝     ╚═╝╚═╝  ╚═╝   ╚═╝   ╚═╝ ╚═════╝╚═╝  ╚═╝
"#;

static mut SPINNER: Option<LoaderSpinner> = None;

pub fn render(f: &mut Frame, state: &Arc<AppState>) {
    let size = f.size();
    
    unsafe {
        if SPINNER.is_none() {
            SPINNER = Some(LoaderSpinner::new());
        }
    }
    
    // Fill background
    let bg_block = Block::default().bg(COLOR_BG);
    f.render_widget(bg_block, size);

    // Main layout
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),  // header
            Constraint::Length(3),  // tabs
            Constraint::Min(0),     // content
            Constraint::Length(1),  // footer
        ])
        .split(size);

    render_header(f, chunks[0], state);
    render_tabs(f, chunks[1], state);
    
    let content_area = chunks[2];
    let tab = *state.selected_tab.read();
    match tab {
        0 => render_overview(f, content_area, state),
        1 => render_trades(f, content_area, state),
        2 => render_logs(f, content_area, state),
        3 => render_config(f, content_area, state),
        _ => {}
    }

    if *state.is_ai_loading.read() {
        let loader_area = Rect::new(
            content_area.x + content_area.width / 2 - 8,
            content_area.y + content_area.height / 2 - 2,
            16,
            5,
        );
        unsafe {
            if let Some(ref mut s) = SPINNER {
                s.tick();
                s.render(f, loader_area);
            }
        }
    }

    // Onboarding Overlay
    let onboarding_step = state.onboarding.read().current_step;
    if onboarding_step != crate::onboarding::OnboardingStep::Completed {
        render_onboarding(f, size, state, onboarding_step);
    }

    render_footer(f, chunks[3]);
}

fn render_onboarding(f: &mut Frame, area: Rect, state: &Arc<AppState>, step: crate::onboarding::OnboardingStep) {
    let area = Rect::new(
        area.width / 4,
        area.height / 4,
        area.width / 2,
        area.height / 2,
    );

    let (title, content) = match step {
        crate::onboarding::OnboardingStep::Welcome => (
            " Welcome to Scematica ",
            "The premier high-frequency trading assistant.\n\nPress [Enter] to begin onboarding."
        ),
        crate::onboarding::OnboardingStep::VerifyWallet => (
            " Step 1: Wallet Setup ",
            "Please ensure your keypair is configured.\nUse the key-converter tool to import.\n\nPress [Enter] when ready."
        ),
        crate::onboarding::OnboardingStep::StrategyTuning => (
            " Step 2: Strategy Tuning ",
            "Let's define your risk parameters.\nAsk the AI to set your Take Profit & Stop Loss.\n\nPress [Enter] to finish."
        ),
        crate::onboarding::OnboardingStep::DemoSimulation => (
            " Step 3: Demo Mode ",
            "Simulating trades... watch the dashboard.\n\nPress [Enter] to exit demo."
        ),
        _ => ("", ""),
    };

    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(COLOR_ACCENT))
        .bg(COLOR_BG);

    let p = Paragraph::new(content)
        .alignment(Alignment::Center)
        .block(block)
        .wrap(Wrap { trim: true });
        
    f.render_widget(bg_block, area); // Dimmed background logic would be ideal here
    f.render_widget(p, area);
}


fn render_header(f: &mut Frame, area: Rect, state: &Arc<AppState>) {
    let mode = *state.active_mode.read();
    let wallet = state.wallet_address.read().clone();
    let sol = *state.sol_balance.read();
    let scema = *state.scematica_balance.read();

    let mode_color = match mode {
        crate::app::BotMode::Idle => Color::Yellow,
        _ => COLOR_ACCENT,
    };

    let header_text = format!(
        " SCEMATICA  │  Mode: {}  │  Wallet: {}  │  SOL: {:.4}  │  SCEMA: {:.2}",
        mode,
        if wallet.len() > 12 { &wallet[..12] } else { &wallet },
        sol,
        scema,
    );

    let header = Paragraph::new(header_text)
        .style(Style::default().fg(mode_color).bg(COLOR_BG).add_modifier(Modifier::BOLD))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(COLOR_ACCENT)),
        );
    f.render_widget(header, area);
}

fn render_tabs(f: &mut Frame, area: Rect, state: &Arc<AppState>) {
    let tab = *state.selected_tab.read();
    let titles = vec!["Overview", "Trades", "Logs", "Config"];
    let tabs = Tabs::new(titles)
        .select(tab)
        .style(Style::default().fg(COLOR_TEXT))
        .highlight_style(
            Style::default()
                .fg(COLOR_ACCENT)
                .add_modifier(Modifier::BOLD),
        )
        .block(Block::default().borders(Borders::ALL).border_style(Style::default().fg(COLOR_ACCENT)));
    f.render_widget(tabs, area);
}

fn render_overview(f: &mut Frame, area: Rect, state: &Arc<AppState>) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(area);

    render_metrics(f, chunks[0], state);
    render_recent_trades(f, chunks[1], state);
}

fn render_metrics(f: &mut Frame, area: Rect, state: &Arc<AppState>) {
    let m = state.effective_snapshot();
    let scema = *state.scematica_balance.read();
    let sol = *state.sol_balance.read();

    let rows: Vec<Row> = vec![
        Row::new(vec![Cell::from("Trades Attempted"), Cell::from(m.trades_attempted.to_string())]),
        Row::new(vec![Cell::from("Trades Confirmed"), Cell::from(m.trades_confirmed.to_string())]),
        Row::new(vec![Cell::from("Trades Failed"), Cell::from(m.trades_failed.to_string())]),
        Row::new(vec![Cell::from("Win Rate"), Cell::from(format!("{:.1}%", m.win_rate()))]),
        Row::new(vec![Cell::from("Arbs Found"), Cell::from(m.arb_opportunities_found.to_string())]),
        Row::new(vec![Cell::from("Arbs Executed"), Cell::from(m.arb_executed.to_string())]),
        Row::new(vec![Cell::from("Total PnL"), Cell::from(format!("{:.6} SOL", m.total_pnl_sol()))]),
        Row::new(vec![Cell::from("Pools Tracked"), Cell::from(m.pools_tracked.to_string())]),
        Row::new(vec![Cell::from("Uptime"), Cell::from(format!("{}s", m.uptime_secs))]),
        Row::new(vec![
            Cell::from("SOL Balance"),
            Cell::from(format!("{:.4} SOL", sol))
                .style(Style::default().fg(Color::Cyan)),
        ]),
        Row::new(vec![
            Cell::from("SCEMA Balance"),
            Cell::from(format!("{:.2} SCEMA", scema))
                .style(Style::default().fg(if scema > 0.0 { Color::Green } else { Color::DarkGray })),
        ]),
    ];

    let widths = [Constraint::Percentage(50), Constraint::Percentage(50)];
    let table = Table::new(rows, widths)
        .header(
            Row::new(vec![
                Cell::from("Metric").style(Style::default().fg(COLOR_ACCENT).add_modifier(Modifier::BOLD)),
                Cell::from("Value").style(Style::default().fg(COLOR_TEXT).add_modifier(Modifier::BOLD)),
            ]),
        )
        .block(
            Block::default()
                .title(" 📊 Metrics ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(COLOR_ACCENT)),
        );

    f.render_widget(table, area);
}

fn render_recent_trades(f: &mut Frame, area: Rect, state: &Arc<AppState>) {
    let trades = state.trades.read();
    let items: Vec<ListItem> = trades
        .iter()
        .take(20)
        .map(|t| {
            let color = if t.pnl >= 0.0 { Color::Green } else { COLOR_ACCENT };
            let line = format!(
                "{} {} {} {:>10.4} SOL  {}",
                t.timestamp.format("%H:%M:%S"),
                t.status,
                t.kind,
                t.pnl,
                &t.mint[..8.min(t.mint.len())],
            );
            ListItem::new(line).style(Style::default().fg(color))
        })
        .collect();

    let list = List::new(items).block(
        Block::default()
            .title(" 📈 Recent Trades ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(COLOR_ACCENT)),
    );
    f.render_widget(list, area);
}

fn render_trades(f: &mut Frame, area: Rect, state: &Arc<AppState>) {
    let trades = state.trades.read();
    let scema_mint = known_tokens::SCEMATICA_MINT.to_string();

    let rows: Vec<Row> = trades
        .iter()
        .map(|t| {
            let color = if t.mint == scema_mint {
                Color::Yellow
            } else if t.pnl >= 0.0 {
                Color::Green
            } else {
                COLOR_ACCENT
            };

            let mint_display = if t.mint == scema_mint {
                "SCEMA   ".to_string()
            } else {
                t.mint[..8.min(t.mint.len())].to_string()
            };

            Row::new(vec![
                Cell::from(t.timestamp.format("%H:%M:%S").to_string()),
                Cell::from(t.status.clone()),
                Cell::from(t.kind.clone()),
                Cell::from(mint_display),
                Cell::from(format!("{:.6}", t.amount)),
                Cell::from(format!("{:.6}", t.pnl)),
                Cell::from(t.signature[..12.min(t.signature.len())].to_string()),
            ])
            .style(Style::default().fg(color))
        })
        .collect();

    let widths = [
        Constraint::Length(10),
        Constraint::Length(3),
        Constraint::Length(6),
        Constraint::Length(10),
        Constraint::Length(12),
        Constraint::Length(12),
        Constraint::Min(0),
    ];
    let table = Table::new(rows, widths)
        .header(
            Row::new(vec![
                Cell::from("Time").style(Style::default().fg(COLOR_ACCENT).add_modifier(Modifier::BOLD)),
                Cell::from("St").style(Style::default().fg(COLOR_ACCENT).add_modifier(Modifier::BOLD)),
                Cell::from("Type").style(Style::default().fg(COLOR_ACCENT).add_modifier(Modifier::BOLD)),
                Cell::from("Mint").style(Style::default().fg(COLOR_ACCENT).add_modifier(Modifier::BOLD)),
                Cell::from("Amount").style(Style::default().fg(COLOR_ACCENT).add_modifier(Modifier::BOLD)),
                Cell::from("PnL").style(Style::default().fg(COLOR_ACCENT).add_modifier(Modifier::BOLD)),
                Cell::from("Sig").style(Style::default().fg(COLOR_ACCENT).add_modifier(Modifier::BOLD)),
            ]),
        )
        .block(
            Block::default()
                .title(" 📋 Trade History ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(COLOR_ACCENT)),
        );

    f.render_widget(table, area);
}

fn render_logs(f: &mut Frame, area: Rect, state: &Arc<AppState>) {
    let logs = state.log_lines.read();
    let items: Vec<ListItem> = logs
        .iter()
        .rev()
        .take(area.height as usize)
        .map(|line| {
            let color = if line.contains("ERROR") {
                COLOR_ACCENT
            } else if line.contains("WARN") {
                Color::Yellow
            } else if line.contains("💰") || line.contains("confirmed") {
                Color::Green
            } else {
                COLOR_TEXT
            };
            ListItem::new(line.as_str()).style(Style::default().fg(color))
        })
        .collect();

    let list = List::new(items).block(
        Block::default()
            .title(" 📝 Logs (newest first) ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(COLOR_ACCENT)),
    );
    f.render_widget(list, area);
}

fn render_config(f: &mut Frame, area: Rect, state: &Arc<AppState>) {
    let scema = *state.scematica_balance.read();
    let wallet = state.wallet_address.read().clone();
    let tp = *state.strategy_tp_pct.read();
    let sl = *state.strategy_sl_pct.read();
    let mult = *state.strategy_multiplier.read();
    let regime = state.strategy_regime.read().clone();

    let regime_color = match regime.as_str() {
        "aggressive" => Color::Green,
        "conservative" => COLOR_ACCENT,
        _ => Color::Yellow,
    };

    let text = vec![
        Line::from(vec![
            Span::styled("Mode: ", Style::default().fg(COLOR_ACCENT)),
            Span::raw(state.active_mode.read().to_string()),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("AI Strategy Agent", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        ]),
        Line::from(vec![
            Span::styled("  Regime:     ", Style::default().fg(COLOR_ACCENT)),
            Span::styled(
                regime.to_uppercase(),
                Style::default().fg(regime_color).add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            Span::styled("  Take Profit:", Style::default().fg(COLOR_ACCENT)),
            Span::styled(
                format!(" {:.1}%", tp),
                Style::default().fg(Color::Green),
            ),
        ]),
        Line::from(vec![
            Span::styled("  Stop Loss:  ", Style::default().fg(COLOR_ACCENT)),
            Span::styled(
                format!(" {:.1}%", sl),
                Style::default().fg(COLOR_ACCENT),
            ),
        ]),
        Line::from(vec![
            Span::styled("  Size Mult:  ", Style::default().fg(COLOR_ACCENT)),
            Span::styled(
                format!(" {:.2}x", mult),
                Style::default().fg(if mult >= 1.0 { Color::Green } else { COLOR_ACCENT }),
            ),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("SCEMATICA Token", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        ]),
        Line::from(vec![
            Span::styled("  Mint:    ", Style::default().fg(COLOR_ACCENT)),
            Span::styled(
                "AbKiP2Jc6nM7937jTDfqoJC1bsg5FQ24Buk2iqRFpump",
                Style::default().fg(COLOR_TEXT),
            ),
        ]),
        Line::from(vec![
            Span::styled("  Symbol:  ", Style::default().fg(COLOR_ACCENT)),
            Span::styled("SCEMA", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
        ]),
        Line::from(vec![
            Span::styled("  Balance: ", Style::default().fg(COLOR_ACCENT)),
            Span::styled(
                format!("{:.2} SCEMA", scema),
                Style::default()
                    .fg(if scema > 0.0 { Color::Green } else { COLOR_ACCENT })
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("Wallet: ", Style::default().fg(COLOR_ACCENT)),
            Span::raw(wallet),
        ]),
    ];

    let para = Paragraph::new(text)
        .block(
            Block::default()
                .title(" ⚙️  Configuration ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(COLOR_ACCENT)),
        )
        .wrap(Wrap { trim: true });
    f.render_widget(para, area);
}

fn render_footer(f: &mut Frame, area: Rect) {
    let footer = Paragraph::new(" [Tab] Switch tab  [q] Quit  [←/→] Navigate ")
        .style(Style::default().fg(Color::DarkGray))
        .alignment(Alignment::Center);
    f.render_widget(footer, area);
}
