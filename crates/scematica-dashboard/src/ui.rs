use crate::app::{AppState, RateMode};
use crate::chat::ChatLine;
use crate::components::{COLOR_BG, COLOR_ACCENT, COLOR_TEXT, LoaderSpinner};
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style, Stylize},
    text::{Line, Span},
    widgets::{
        canvas::{Canvas, Points},
        Block, Borders, Cell, List, ListItem, Paragraph, Row, Sparkline, Table, Tabs, Wrap,
    },
    Frame,
};
use scematica_core::types::known_tokens;
use std::sync::{Arc, Mutex, OnceLock};

#[allow(dead_code)]
const SCEMATICA_LOGO: &str = r#"
 ███████╗ ██████╗███████╗███╗   ███╗ █████╗ ████████╗██╗ ██████╗ █████╗
 ██╔════╝██╔════╝██╔════╝████╗ ████║██╔══██╗╚══██╔══╝██║██╔════╝██╔══██╗
 ███████╗██║     █████╗  ██╔████╔██║███████║   ██║   ██║██║     ███████║
 ╚════██║██║     ██╔══╝  ██║╚██╔╝██║██╔══██║   ██║   ██║██║     ██╔══██║
 ███████║╚██████╗███████╗██║ ╚═╝ ██║██║  ██║   ██║   ██║╚██████╗██║  ██║
 ╚══════╝ ╚═════╝╚══════╝╚═╝     ╚═╝╚═╝  ╚═╝   ╚═╝   ╚═╝ ╚═════╝╚═╝  ╚═╝
"#;

static SPINNER: OnceLock<Mutex<LoaderSpinner>> = OnceLock::new();

pub fn render(f: &mut Frame, state: &Arc<AppState>) {
    let size = f.size();
    SPINNER.get_or_init(|| Mutex::new(LoaderSpinner::new()));
    
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
        4 => render_chat(f, content_area, state),
        5 => render_radar(f, content_area, state),
        _ => {}
    }

    // Only show the AI spinner on the Chat tab
    if tab == 4 && *state.is_ai_loading.read() {
        let spinner_w = 18u16;
        let spinner_h = 3u16;
        let sx = content_area.x + content_area.width.saturating_sub(spinner_w + 2);
        let sy = content_area.y + content_area.height.saturating_sub(spinner_h + 4);
        let loader_area = Rect::new(sx, sy, spinner_w, spinner_h);
        if let Some(spinner_lock) = SPINNER.get() {
            if let Ok(mut s) = spinner_lock.lock() {
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

    render_footer(f, chunks[3], tab);
}

fn render_onboarding(f: &mut Frame, area: Rect, _state: &Arc<AppState>, step: crate::onboarding::OnboardingStep) {
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
        
    f.render_widget(Block::default().bg(COLOR_BG), area);
    f.render_widget(p, area);
}


fn render_header(f: &mut Frame, area: Rect, state: &Arc<AppState>) {
    let mode = *state.active_mode.read();
    let wallet = state.wallet_address.read().clone();
    let sol = *state.sol_balance.read();
    let scema = *state.scematica_balance.read();
    let regime = state.strategy_regime.read().clone();
    let open_pos = state.open_position_mints().len();

    let mode_color = match mode {
        crate::app::BotMode::Idle => Color::Yellow,
        _ => COLOR_ACCENT,
    };

    let regime_indicator = match regime.as_str() {
        "aggressive" => "▲AGG",
        "conservative" => "▼CON",
        _ => "◆NEU",
    };

    let header_text = format!(
        " SCEMATICA  │  {}  │  Wallet: {}  │  SOL: {:.4}  │  SCEMA: {:.0}  │  Regime: {}  │  Pos: {}",
        mode,
        if wallet.len() > 12 { &wallet[..12] } else { &wallet },
        sol,
        scema,
        regime_indicator,
        open_pos,
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
    let titles = vec!["Overview", "Trades", "Logs", "Config", "Chat", "Radar"];
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
    let h_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(area);

    // Left: metrics + session stats + NN stats
    let left_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(7), Constraint::Length(7)])
        .split(h_chunks[0]);
    render_metrics(f, left_chunks[0], state);
    render_session_stats(f, left_chunks[1], state);
    render_nn_stats(f, left_chunks[2], state);

    // Right: recent trades + sparkline
    let right_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(5)])
        .split(h_chunks[1]);
    render_recent_trades(f, right_chunks[0], state);
    render_pnl_sparkline(f, right_chunks[1], state);
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

fn render_session_stats(f: &mut Frame, area: Rect, state: &Arc<AppState>) {
    let best = *state.best_trade_pnl.read();
    let worst = *state.worst_trade_pnl.read();
    let streak = *state.trade_streak.read();
    let open = state.open_position_mints().len();

    let (streak_label, streak_color) = if streak > 0 {
        (format!("W{}", streak), Color::Green)
    } else if streak < 0 {
        (format!("L{}", streak.abs()), Color::Red)
    } else {
        ("—".to_string(), Color::DarkGray)
    };

    let rows = vec![
        Row::new(vec![
            Cell::from("Best Trade"),
            Cell::from(format!("{:+.4} SOL", best))
                .style(Style::default().fg(if best >= 0.0 { Color::Green } else { COLOR_ACCENT })),
        ]),
        Row::new(vec![
            Cell::from("Worst Trade"),
            Cell::from(format!("{:+.4} SOL", worst))
                .style(Style::default().fg(if worst >= 0.0 { Color::Green } else { COLOR_ACCENT })),
        ]),
        Row::new(vec![
            Cell::from("Streak"),
            Cell::from(streak_label).style(Style::default().fg(streak_color)),
        ]),
        Row::new(vec![
            Cell::from("Open Pos"),
            Cell::from(open.to_string()).style(Style::default().fg(Color::Cyan)),
        ]),
    ];

    let widths = [Constraint::Percentage(50), Constraint::Percentage(50)];
    let table = Table::new(rows, widths)
        .block(Block::default().title(" 🏆 Session Stats ").borders(Borders::ALL).border_style(Style::default().fg(COLOR_ACCENT)));
    f.render_widget(table, area);
}

fn render_pnl_sparkline(f: &mut Frame, area: Rect, state: &Arc<AppState>) {
    let spark_data: Vec<u64> = state.pnl_sparkline.read().iter().copied().collect();
    let sparkline = Sparkline::default()
        .block(Block::default().title(" PnL History ").borders(Borders::ALL).border_style(Style::default().fg(COLOR_ACCENT)))
        .data(&spark_data)
        .style(Style::default().fg(Color::Green));
    f.render_widget(sparkline, area);
}

fn render_nn_stats(f: &mut Frame, area: Rect, state: &Arc<AppState>) {
    let nn = state.nn_stats.read();
    let block = Block::default()
        .title(" 🧠 Deep Q* Agent ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));

    let rows = if let Some(v) = &*nn {
        let epsilon    = v["epsilon"].as_f64().unwrap_or(1.0);
        let steps      = v["step_count"].as_u64().unwrap_or(0);
        let replay     = v["replay_size"].as_u64().unwrap_or(0);
        let loss       = v["avg_loss"].as_f64().unwrap_or(0.0);
        let reward     = v["total_reward"].as_f64().unwrap_or(0.0);
        let ready      = v["ready_to_advise"].as_bool().unwrap_or(false);
        let last_act   = v["last_action"].as_str().unwrap_or("-").to_string();
        let ready_str  = if ready { "YES" } else { "NO" };
        let ready_col  = if ready { Color::Green } else { Color::Yellow };
        vec![
            Row::new(vec![
                Cell::from("ε / Steps").style(Style::default().fg(Color::DarkGray)),
                Cell::from(format!("{:.4}  /  {}", epsilon, steps)),
            ]),
            Row::new(vec![
                Cell::from("Replay / Loss").style(Style::default().fg(Color::DarkGray)),
                Cell::from(format!("{}  /  {:.6}", replay, loss)),
            ]),
            Row::new(vec![
                Cell::from("Total Reward").style(Style::default().fg(Color::DarkGray)),
                Cell::from(format!("{:.4}", reward)),
            ]),
            Row::new(vec![
                Cell::from("Last Action").style(Style::default().fg(Color::DarkGray)),
                Cell::from(last_act),
            ]),
            Row::new(vec![
                Cell::from("Advising").style(Style::default().fg(Color::DarkGray)),
                Cell::from(ready_str).style(Style::default().fg(ready_col)),
            ]),
        ]
    } else {
        vec![Row::new(vec![Cell::from("Waiting for NN agent...").style(Style::default().fg(Color::DarkGray))])]
    };

    let table = Table::new(rows, [Constraint::Length(16), Constraint::Min(0)])
        .block(block);
    f.render_widget(table, area);
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
    let sell_mode = *state.sell_mode_active.read();
    let dump_mode = *state.dump_mode_active.read();
    let sol = *state.sol_balance.read();
    let filter_active = *state.log_filter_active.read();
    let filter_text = state.log_filter.read().clone();

    // Build vertical layout dynamically based on active banners + filter bar
    let dump_h: u16 = if dump_mode { 3 } else { 0 };
    let sell_h: u16 = if sell_mode { 3 } else { 0 };
    let filter_h: u16 = if filter_active || !filter_text.is_empty() { 3 } else { 0 };
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(dump_h),
            Constraint::Length(sell_h),
            Constraint::Length(filter_h),
            Constraint::Min(0),
        ])
        .split(area);

    if dump_mode {
        let banner_text = format!(
            " DUMP MODE ACTIVE  |  SOL: {:.4}  |  Force-selling ALL positions (zero slippage)  |  [d] to deactivate ",
            sol
        );
        let banner = Paragraph::new(banner_text)
            .style(Style::default().fg(Color::Black).bg(Color::Yellow).add_modifier(Modifier::BOLD))
            .block(Block::default().borders(Borders::ALL).border_style(Style::default().fg(Color::Yellow)));
        f.render_widget(banner, chunks[0]);
    }

    if sell_mode {
        let banner_text = format!(
            " SELL MODE ACTIVE  |  SOL: {:.4}  |  Buying paused — selling all positions  |  [e] to deactivate ",
            sol
        );
        let banner = Paragraph::new(banner_text)
            .style(Style::default().fg(Color::Black).bg(COLOR_ACCENT).add_modifier(Modifier::BOLD))
            .block(Block::default().borders(Borders::ALL).border_style(Style::default().fg(COLOR_ACCENT)));
        f.render_widget(banner, chunks[1]);
    }

    if filter_h > 0 {
        let cursor = if filter_active { "_" } else { "" };
        let filter_display = format!(" Filter: {}{}", filter_text, cursor);
        let filter_bar = Paragraph::new(filter_display)
            .style(Style::default().fg(Color::White))
            .block(Block::default()
                .title(if filter_active { " [/] Filter (Esc to clear) " } else { " Filter active " })
                .borders(Borders::ALL)
                .border_style(Style::default().fg(if filter_active { Color::Cyan } else { Color::DarkGray })));
        f.render_widget(filter_bar, chunks[2]);
    }

    let log_area = chunks[3];
    let logs = state.log_lines.read();
    let inner_width = log_area.width.saturating_sub(2).max(1) as usize;
    let max_rows = log_area.height.saturating_sub(2) as usize;
    let filter_lower = filter_text.to_lowercase();

    let mut lines: Vec<Line> = Vec::new();
    'outer: for raw in logs.iter().rev() {
        if !filter_lower.is_empty() && !raw.to_lowercase().contains(&filter_lower) {
            continue;
        }
        let color = if raw.contains("DUMP MODE") {
            Color::Yellow
        } else if raw.contains("ERROR") || raw.contains("SELL MODE") {
            COLOR_ACCENT
        } else if raw.contains("WARN") {
            Color::Yellow
        } else if raw.contains("confirmed") || raw.contains("Sell confirmed") {
            Color::Green
        } else {
            COLOR_TEXT
        };
        let style = Style::default().fg(color);
        let chars: Vec<char> = raw.chars().collect();
        if chars.is_empty() {
            lines.push(Line::from(Span::styled(String::new(), style)));
            if lines.len() >= max_rows { break; }
        } else {
            for chunk in chars.chunks(inner_width) {
                lines.push(Line::from(Span::styled(chunk.iter().collect::<String>(), style)));
                if lines.len() >= max_rows { break 'outer; }
            }
        }
    }

    let high_speed = *state.high_speed_active.read();
    let title = if dump_mode {
        " Logs — DUMP MODE ACTIVE (newest first)  [b] Force Buy Mode ".to_string()
    } else if sell_mode {
        " Logs — SELL MODE (newest first)  [b] Force Buy Mode  [d] Dump All ".to_string()
    } else if high_speed {
        " Logs — ⚡ HIGH-SPEED (newest first)  [h] Disable  [e] Sell Mode  [d] Dump ".to_string()
    } else {
        " Logs (newest first)  [e] Sell Mode  [b] Buy Mode  [h] High-Speed  [d] Dump All ".to_string()
    };

    let border_color = if dump_mode {
        Color::Yellow
    } else if high_speed {
        Color::LightMagenta
    } else {
        COLOR_ACCENT
    };

    let paragraph = Paragraph::new(lines).block(
        Block::default()
            .title(title)
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border_color)),
    );
    f.render_widget(paragraph, log_area);
}

fn render_config(f: &mut Frame, area: Rect, state: &Arc<AppState>) {
    let scema = *state.scematica_balance.read();
    let wallet = state.wallet_address.read().clone();
    let tp = *state.strategy_tp_pct.read();
    let sl = *state.strategy_sl_pct.read();
    let mult = *state.strategy_multiplier.read();
    let regime = state.strategy_regime.read().clone();
    let mode = *state.active_mode.read();
    let rate_mode = *state.rate_mode.read();

    let regime_color = match regime.as_str() {
        "aggressive" => Color::Green,
        "conservative" => COLOR_ACCENT,
        _ => Color::Yellow,
    };

    let (mode_dot, mode_color) = match mode {
        crate::app::BotMode::Idle    => ("● IDLE",    Color::DarkGray),
        crate::app::BotMode::Sniper  => ("● SNIPER",  Color::Green),
        crate::app::BotMode::Arb     => ("● ARB",     Color::Green),
        crate::app::BotMode::Both    => ("● BOTH",    Color::Green),
    };

    let text = vec![
        Line::from(vec![
            Span::styled("Bot Controls", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        ]),
        Line::from(vec![
            Span::styled("  Status: ", Style::default().fg(COLOR_ACCENT)),
            Span::styled(mode_dot, Style::default().fg(mode_color).add_modifier(Modifier::BOLD)),
        ]),
        Line::from(vec![
            Span::styled("  ", Style::default()),
            Span::styled("[s] Sniper", Style::default().fg(Color::Cyan)),
            Span::raw("  "),
            Span::styled("[a] Arb", Style::default().fg(Color::Cyan)),
            Span::raw("  "),
            Span::styled("[b] Both", Style::default().fg(Color::Cyan)),
            Span::raw("  "),
            Span::styled("[x] Stop All", Style::default().fg(COLOR_ACCENT)),
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
            Span::styled(format!(" {:.1}%", tp), Style::default().fg(Color::Green)),
        ]),
        Line::from(vec![
            Span::styled("  Stop Loss:  ", Style::default().fg(COLOR_ACCENT)),
            Span::styled(format!(" {:.1}%", sl), Style::default().fg(COLOR_ACCENT)),
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
            Span::styled("Rate Mode", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            Span::raw("  (press key to switch)"),
        ]),
        {
            let active = rate_mode == RateMode::Safe;
            Line::from(vec![
                Span::styled(if active { "▶ " } else { "  " }, Style::default().fg(if active { Color::Green } else { Color::DarkGray })),
                Span::styled("[1] Safe       ", Style::default().fg(if active { Color::Green } else { COLOR_TEXT }).add_modifier(if active { Modifier::BOLD } else { Modifier::empty() })),
                Span::styled("0.5x  0.005 SOL/trade  TP: 50%  SL: 10%", Style::default().fg(if active { Color::Green } else { Color::DarkGray })),
            ])
        },
        {
            let active = rate_mode == RateMode::Balanced;
            Line::from(vec![
                Span::styled(if active { "▶ " } else { "  " }, Style::default().fg(if active { Color::Green } else { Color::DarkGray })),
                Span::styled("[2] Balanced   ", Style::default().fg(if active { Color::Green } else { COLOR_TEXT }).add_modifier(if active { Modifier::BOLD } else { Modifier::empty() })),
                Span::styled("1.0x  0.010 SOL/trade  TP:100%  SL: 15%", Style::default().fg(if active { Color::Green } else { Color::DarkGray })),
            ])
        },
        {
            let active = rate_mode == RateMode::Aggressive;
            Line::from(vec![
                Span::styled(if active { "▶ " } else { "  " }, Style::default().fg(if active { Color::Yellow } else { Color::DarkGray })),
                Span::styled("[3] Aggressive ", Style::default().fg(if active { Color::Yellow } else { COLOR_TEXT }).add_modifier(if active { Modifier::BOLD } else { Modifier::empty() })),
                Span::styled("2.0x  0.020 SOL/trade  TP:200%  SL: 25%", Style::default().fg(if active { Color::Yellow } else { Color::DarkGray })),
            ])
        },
        {
            let active = rate_mode == RateMode::Degen;
            Line::from(vec![
                Span::styled(if active { "▶ " } else { "  " }, Style::default().fg(if active { COLOR_ACCENT } else { Color::DarkGray })),
                Span::styled("[4] Degen      ", Style::default().fg(if active { COLOR_ACCENT } else { COLOR_TEXT }).add_modifier(if active { Modifier::BOLD } else { Modifier::empty() })),
                Span::styled("4.0x  0.040 SOL/trade  TP:300%  SL: 40%", Style::default().fg(if active { COLOR_ACCENT } else { Color::DarkGray })),
            ])
        },
        Line::from(""),
        Line::from(vec![
            Span::styled("SCEMATICA Token", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        ]),
        Line::from(vec![
            Span::styled("  Mint:    ", Style::default().fg(COLOR_ACCENT)),
            Span::styled("AbKiP2Jc6nM7937jTDfqoJC1bsg5FQ24Buk2iqRFpump", Style::default().fg(COLOR_TEXT)),
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

    // Append filter rejection stats if available
    let filter_stats_opt = state.filter_stats.read().clone();
    let mut extra_lines: Vec<Line> = Vec::new();
    if let Some(stats) = filter_stats_opt {
        extra_lines.push(Line::from(""));
        extra_lines.push(Line::from(vec![
            Span::styled("Filter Stats", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        ]));
        if let Some(obj) = stats.as_object() {
            if let Some(seen) = obj.get("pools_seen").and_then(|v| v.as_u64()) {
                extra_lines.push(Line::from(vec![
                    Span::styled("  Seen:   ", Style::default().fg(COLOR_ACCENT)),
                    Span::styled(seen.to_string(), Style::default().fg(COLOR_TEXT)),
                ]));
            }
            if let Some(passed) = obj.get("pools_passed").and_then(|v| v.as_u64()) {
                extra_lines.push(Line::from(vec![
                    Span::styled("  Passed: ", Style::default().fg(COLOR_ACCENT)),
                    Span::styled(passed.to_string(), Style::default().fg(Color::Green)),
                ]));
            }
            if let Some(rejections) = obj.get("rejections").and_then(|v| v.as_object()) {
                for (filter, count) in rejections.iter().take(6) {
                    let n = count.as_u64().unwrap_or(0);
                    if n > 0 {
                        extra_lines.push(Line::from(vec![
                            Span::styled(format!("  {:16}", filter), Style::default().fg(COLOR_ACCENT)),
                            Span::styled(format!("{} rejected", n), Style::default().fg(Color::Yellow)),
                        ]));
                    }
                }
            }
        }
    }

    let mut all_lines = text;
    all_lines.extend(extra_lines);

    let para = Paragraph::new(all_lines)
        .block(
            Block::default()
                .title(" ⚙️  Configuration ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(COLOR_ACCENT)),
        )
        .wrap(Wrap { trim: true });
    f.render_widget(para, area);
}

fn render_footer(f: &mut Frame, area: Rect, current_tab: usize) {
    let hint = match current_tab {
        1 => " [x] Export CSV  [R] Reset Positions  [Tab] Switch  [q] Quit  [←/→] Navigate ",
        2 => " [/] Filter  [e] Sell  [b] Buy  [h] High-Speed  [d] DUMP ALL  [Tab] Switch  [q] Quit ",
        3 => " [s] Sniper  [a] Arb  [b] Both  [x] Stop  [1-4] Rate Mode  [Tab] Switch tab  [q] Quit ",
        4 => " [Enter] Send  [Backspace] Delete  [y/n] Confirm/Reject  [Tab] Switch tab  [Esc] Quit ",
        5 => " Pool Radar — live scatter of evaluated pools (last 5 min)  [Tab] Switch tab  [q] Quit ",
        _ => " [Tab] Switch tab  [q] Quit  [←/→] Navigate ",
    };
    let footer = Paragraph::new(hint)
        .style(Style::default().fg(Color::DarkGray))
        .alignment(Alignment::Center);
    f.render_widget(footer, area);
}

fn render_radar(f: &mut Frame, area: Rect, state: &Arc<AppState>) {
    // Split: canvas on top, table of recent pools below
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(13)])
        .split(area);

    let pools = state.radar_pools.read().clone();

    // Y-axis: 0 → 10 SOL, log-ish via min/clamp. Pump.fun graduates land at
    // 0.4–3 SOL — the old 0–100 SOL scale crushed them all into the bottom row.
    // We pin the displayed value into [0, 10] and add a marker above for any pool
    // bigger than that.
    const Y_MAX: f64 = 10.0;
    let to_y = |sol: f64| sol.clamp(0.0, Y_MAX);

    // Separate passed / rejected pools for color-coded Points
    let passed_coords: Vec<(f64, f64)> = pools
        .iter()
        .filter(|p| p.passed_filters)
        .map(|p| (p.age_secs.min(299.0), to_y(p.size_sol)))
        .collect();
    let rejected_coords: Vec<(f64, f64)> = pools
        .iter()
        .filter(|p| !p.passed_filters)
        .map(|p| (p.age_secs.min(299.0), to_y(p.size_sol)))
        .collect();

    let total = pools.len();
    let passed = pools.iter().filter(|p| p.passed_filters).count();
    let canvas_title = format!(
        " Pool Radar — Age vs Size  |  pools: {} (passed {} red rejected) ",
        total, passed,
    );

    let canvas = Canvas::default()
        .block(
            Block::default()
                .title(canvas_title)
                .borders(Borders::ALL)
                .border_style(Style::default().fg(COLOR_ACCENT)),
        )
        .x_bounds([0.0, 300.0])
        .y_bounds([0.0, Y_MAX])
        .paint(move |ctx| {
            // Axis labels: X = pool age in seconds, Y = quote-vault SOL.
            ctx.print(0.0, -0.5, "0s");
            ctx.print(150.0, -0.5, "150s");
            ctx.print(292.0, -0.5, "300s");
            ctx.print(-12.0, 0.0,   "0");
            ctx.print(-15.0, 5.0,   "5");
            ctx.print(-18.0, 9.8,   "10 SOL");

            if !rejected_coords.is_empty() {
                ctx.draw(&Points {
                    coords: &rejected_coords,
                    color: Color::Red,
                });
            }
            if !passed_coords.is_empty() {
                ctx.draw(&Points {
                    coords: &passed_coords,
                    color: Color::Green,
                });
            }
        });

    f.render_widget(canvas, chunks[0]);

    // Table: 10 most recent pools
    let now = chrono::Utc::now().timestamp();
    let rows: Vec<Row> = pools
        .iter()
        .rev()
        .take(10)
        .map(|p| {
            let age_display = now - p.timestamp;
            let pass_str = if p.passed_filters { "PASS" } else { "FAIL" };
            let pass_color = if p.passed_filters { Color::Green } else { Color::Red };
            let mint_short = p.mint[..8.min(p.mint.len())].to_string();
            Row::new(vec![
                Cell::from(mint_short),
                Cell::from(format!("{:.0}s", p.age_secs)),
                Cell::from(format!("{:.2}", p.size_sol)),
                Cell::from(pass_str).style(Style::default().fg(pass_color)),
                Cell::from(format!("{:.1}", p.score)),
                Cell::from(format!("{}s ago", age_display)),
            ])
        })
        .collect();

    let widths = [
        Constraint::Length(10),
        Constraint::Length(8),
        Constraint::Length(10),
        Constraint::Length(6),
        Constraint::Length(7),
        Constraint::Min(0),
    ];
    let table = Table::new(rows, widths)
        .header(
            Row::new(vec![
                Cell::from("Mint").style(Style::default().fg(COLOR_ACCENT).add_modifier(Modifier::BOLD)),
                Cell::from("Age").style(Style::default().fg(COLOR_ACCENT).add_modifier(Modifier::BOLD)),
                Cell::from("Size SOL").style(Style::default().fg(COLOR_ACCENT).add_modifier(Modifier::BOLD)),
                Cell::from("Result").style(Style::default().fg(COLOR_ACCENT).add_modifier(Modifier::BOLD)),
                Cell::from("Score").style(Style::default().fg(COLOR_ACCENT).add_modifier(Modifier::BOLD)),
                Cell::from("Seen").style(Style::default().fg(COLOR_ACCENT).add_modifier(Modifier::BOLD)),
            ]),
        )
        .block(
            Block::default()
                .title(" Recent Pools ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(COLOR_ACCENT)),
        );

    f.render_widget(table, chunks[1]);
}

fn render_chat(f: &mut Frame, area: Rect, state: &Arc<AppState>) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(3)])
        .split(area);

    let history = state.chat_history.read();
    let has_pending = state.chat_pending.read().is_some();
    let loading = *state.is_ai_loading.read();

    // Inner height minus borders — each item is 1 line
    let max_visible = chunks[0].height.saturating_sub(2) as usize;
    let skip = history.len().saturating_sub(max_visible);

    let items: Vec<ListItem> = history
        .iter()
        .skip(skip)
        .map(|line| match line {
            ChatLine::User(s) => ListItem::new(format!("You: {}", s))
                .style(Style::default().fg(Color::Cyan)),
            ChatLine::Bot(s) => ListItem::new(format!(" AI: {}", s))
                .style(Style::default().fg(COLOR_TEXT)),
            ChatLine::ToolResult(s) => ListItem::new(format!("  > {}", s))
                .style(Style::default().fg(Color::DarkGray)),
            ChatLine::Error(s) => ListItem::new(format!("[ERR] {}", s))
                .style(Style::default().fg(COLOR_ACCENT)),
            ChatLine::Pending { summary, risk } => ListItem::new(format!(
                "[Confirm? y/n] {} (Risk: {})",
                summary, risk
            ))
            .style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
        })
        .collect();

    let title = if loading {
        " 🤖 Chat — Thinking... "
    } else if has_pending {
        " 🤖 Chat — Awaiting confirmation [y] yes  [n] no "
    } else {
        " 🤖 Chat — Ask about your wallet & trades "
    };

    let border_color = if has_pending { Color::Yellow } else { COLOR_ACCENT };

    let list = List::new(items).block(
        Block::default()
            .title(title)
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border_color)),
    );
    f.render_widget(list, chunks[0]);

    // Input box
    let input = state.chat_input.read().clone();
    let (prompt_text, prompt_color) = if loading {
        ("  Thinking...".to_string(), Color::Yellow)
    } else if has_pending {
        (format!("  [y] confirm  [n] reject"), Color::Yellow)
    } else {
        (format!("  > {}_", input), Color::White)
    };

    let input_box = Paragraph::new(prompt_text)
        .style(Style::default().fg(prompt_color))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(border_color)),
        );
    f.render_widget(input_box, chunks[1]);
}
