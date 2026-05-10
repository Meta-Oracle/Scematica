use crate::app::{AppState, BotMode};
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{
        Block, Borders, Cell, List, ListItem, Paragraph, Row, Table, Tabs, Wrap,
    },
    Frame,
};
use std::sync::Arc;

const SCEMATICA_LOGO: &str = r#"
 ███████╗ ██████╗███████╗███╗   ███╗ █████╗ ████████╗██╗ ██████╗ █████╗
 ██╔════╝██╔════╝██╔════╝████╗ ████║██╔══██╗╚══██╔══╝██║██╔════╝██╔══██╗
 ███████╗██║     █████╗  ██╔████╔██║███████║   ██║   ██║██║     ███████║
 ╚════██║██║     ██╔══╝  ██║╚██╔╝██║██╔══██║   ██║   ██║██║     ██╔══██║
 ███████║╚██████╗███████╗██║ ╚═╝ ██║██║  ██║   ██║   ██║╚██████╗██║  ██║
 ╚══════╝ ╚═════╝╚══════╝╚═╝     ╚═╝╚═╝  ╚═╝   ╚═╝   ╚═╝ ╚═════╝╚═╝  ╚═╝
"#;

pub fn render(f: &mut Frame, state: &Arc<AppState>) {
    let size = f.size();

    // Main layout: header | tabs | content | footer
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

    let tab = *state.selected_tab.read();
    match tab {
        0 => render_overview(f, chunks[2], state),
        1 => render_trades(f, chunks[2], state),
        2 => render_logs(f, chunks[2], state),
        3 => render_config(f, chunks[2], state),
        _ => {}
    }

    render_footer(f, chunks[3]);
}

fn render_header(f: &mut Frame, area: Rect, state: &Arc<AppState>) {
    let mode = *state.active_mode.read();
    let wallet = state.wallet_address.read().clone();
    let sol = *state.sol_balance.read();

    let header_text = format!(
        " SCEMATICA  │  Mode: {}  │  Wallet: {}  │  SOL: {:.4}",
        mode,
        if wallet.len() > 12 { &wallet[..12] } else { &wallet },
        sol
    );

    let header = Paragraph::new(header_text)
        .style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan)),
        );
    f.render_widget(header, area);
}

fn render_tabs(f: &mut Frame, area: Rect, state: &Arc<AppState>) {
    let tab = *state.selected_tab.read();
    let titles = vec!["Overview", "Trades", "Logs", "Config"];
    let tabs = Tabs::new(titles)
        .select(tab)
        .style(Style::default().fg(Color::White))
        .highlight_style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )
        .block(Block::default().borders(Borders::ALL));
    f.render_widget(tabs, area);
}

fn render_overview(f: &mut Frame, area: Rect, state: &Arc<AppState>) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(area);

    // Left: metrics
    render_metrics(f, chunks[0], state);
    // Right: recent trades summary
    render_recent_trades(f, chunks[1], state);
}

fn render_metrics(f: &mut Frame, area: Rect, state: &Arc<AppState>) {
    let metrics = state.metrics.read().clone();

    let rows: Vec<Row> = if let Some(m) = metrics {
        vec![
            Row::new(vec![Cell::from("Trades Attempted"), Cell::from(m.trades_attempted.to_string())]),
            Row::new(vec![Cell::from("Trades Confirmed"), Cell::from(m.trades_confirmed.to_string())]),
            Row::new(vec![Cell::from("Trades Failed"), Cell::from(m.trades_failed.to_string())]),
            Row::new(vec![Cell::from("Win Rate"), Cell::from(format!("{:.1}%", m.win_rate()))]),
            Row::new(vec![Cell::from("Arbs Found"), Cell::from(m.arb_opportunities_found.to_string())]),
            Row::new(vec![Cell::from("Arbs Executed"), Cell::from(m.arb_executed.to_string())]),
            Row::new(vec![Cell::from("Total PnL"), Cell::from(format!("{:.6} SOL", m.total_pnl_sol()))]),
            Row::new(vec![Cell::from("Pools Tracked"), Cell::from(m.pools_tracked.to_string())]),
            Row::new(vec![Cell::from("Uptime"), Cell::from(format!("{}s", m.uptime_secs))]),
        ]
    } else {
        vec![Row::new(vec![Cell::from("No data yet"), Cell::from("—")])]
    };

    let widths = [Constraint::Percentage(50), Constraint::Percentage(50)];
    let table = Table::new(rows, widths)
        .header(
            Row::new(vec![
                Cell::from("Metric").style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                Cell::from("Value").style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            ]),
        )
        .block(
            Block::default()
                .title(" 📊 Metrics ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Green)),
        );

    f.render_widget(table, area);
}

fn render_recent_trades(f: &mut Frame, area: Rect, state: &Arc<AppState>) {
    let trades = state.trades.read();
    let items: Vec<ListItem> = trades
        .iter()
        .take(20)
        .map(|t| {
            let color = if t.pnl >= 0.0 { Color::Green } else { Color::Red };
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
            .border_style(Style::default().fg(Color::Blue)),
    );
    f.render_widget(list, area);
}

fn render_trades(f: &mut Frame, area: Rect, state: &Arc<AppState>) {
    let trades = state.trades.read();
    let rows: Vec<Row> = trades
        .iter()
        .map(|t| {
            let color = if t.pnl >= 0.0 { Color::Green } else { Color::Red };
            Row::new(vec![
                Cell::from(t.timestamp.format("%H:%M:%S").to_string()),
                Cell::from(t.status.clone()),
                Cell::from(t.kind.clone()),
                Cell::from(t.mint[..8.min(t.mint.len())].to_string()),
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
                Cell::from("Time").style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                Cell::from("St").style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                Cell::from("Type").style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                Cell::from("Mint").style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                Cell::from("Amount").style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                Cell::from("PnL").style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                Cell::from("Signature").style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            ]),
        )
        .block(
            Block::default()
                .title(" 📋 Trade History ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Blue)),
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
                Color::Red
            } else if line.contains("WARN") {
                Color::Yellow
            } else if line.contains("💰") || line.contains("confirmed") {
                Color::Green
            } else {
                Color::White
            };
            ListItem::new(line.as_str()).style(Style::default().fg(color))
        })
        .collect();

    let list = List::new(items).block(
        Block::default()
            .title(" 📝 Logs (newest first) ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Magenta)),
    );
    f.render_widget(list, area);
}

fn render_config(f: &mut Frame, area: Rect, state: &Arc<AppState>) {
    let text = vec![
        Line::from(vec![
            Span::styled("Mode: ", Style::default().fg(Color::Yellow)),
            Span::raw(state.active_mode.read().to_string()),
        ]),
        Line::from(""),
        Line::from(vec![Span::styled(
            "Edit config.toml and restart to change settings.",
            Style::default().fg(Color::Gray),
        )]),
    ];

    let para = Paragraph::new(text)
        .block(
            Block::default()
                .title(" ⚙️  Configuration ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan)),
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
