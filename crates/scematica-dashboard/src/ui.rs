use crate::app::{AppState, BuilderMode, RateMode};
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
    let price_usd = *state.sol_price_usd.read();
    let scema = *state.scematica_balance.read();
    let regime = state.strategy_regime.read().clone();
    let open_pos = state.open_position_count();

    let mode_color = match mode {
        crate::app::BotMode::Idle => Color::Yellow,
        _ => COLOR_ACCENT,
    };

    let regime_indicator = match regime.as_str() {
        "aggressive" => "▲AGG",
        "conservative" => "▼CON",
        _ => "◆NEU",
    };

    let sol_display = if price_usd > 0.0 {
        format!("{:.4} (${:.2})", sol, sol * price_usd)
    } else {
        format!("{:.4}", sol)
    };

    let header_text = format!(
        " SCEMATICA  │  {}  │  Wallet: {}  │  SOL: {}  │  SCEMA: {:.0}  │  Regime: {}  │  Pos: {}",
        mode,
        if wallet.len() > 12 { &wallet[..12] } else { &wallet },
        sol_display,
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

    // Right: live positions + recent trades + sparkline + alert history
    let right_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(14), // live positions
            Constraint::Min(0),     // recent trades
            Constraint::Length(5),  // sparkline
            Constraint::Length(7),  // alert history (last 5 events)
        ])
        .split(h_chunks[1]);
    render_live_positions(f, right_chunks[0], state);
    render_recent_trades(f, right_chunks[1], state);
    render_pnl_sparkline(f, right_chunks[2], state);
    render_alert_history(f, right_chunks[3], state);
}

fn render_live_positions(f: &mut Frame, area: Rect, state: &Arc<AppState>) {
    let positions = state.live_positions.read();
    let now_unix = chrono::Utc::now().timestamp();

    // Detect a stale file: if last_check on EVERY position is >10 s ago, the
    // sniper either isn't running or its flush task is wedged. Surface that in
    // the title so the operator doesn't sit staring at frozen values.
    let max_age_since_check = positions
        .iter()
        .map(|p| now_unix.saturating_sub(p.last_check_unix_secs))
        .max()
        .unwrap_or(0);
    let title_suffix = if positions.is_empty() {
        String::new()
    } else if max_age_since_check > 10 {
        format!("  ⚠ stale ({}s)", max_age_since_check)
    } else {
        String::new()
    };
    let title = format!(" 💼 Open Positions  ({} live){} ", positions.len(), title_suffix);

    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));

    if positions.is_empty() {
        let p = Paragraph::new("no open positions")
            .style(Style::default().fg(Color::DarkGray))
            .block(block);
        f.render_widget(p, area);
        return;
    }

    // Sort by entry time descending — newest position at the top, so a fresh
    // buy appears where the operator's eye lands. Older positions fall off
    // the bottom only if there are more than panel-height rows.
    let mut sorted: Vec<_> = positions.iter().cloned().collect();
    sorted.sort_by(|a, b| b.entry_unix_secs.cmp(&a.entry_unix_secs));

    let rows: Vec<Row> = sorted
        .iter()
        .map(|p| {
            let pnl   = p.pnl_pct();
            let peak  = p.peak_pnl_pct();
            let age   = p.age_secs();
            let value_sol = p.value_sol();
            let staleness = now_unix.saturating_sub(p.last_check_unix_secs);

            let pnl_color = if pnl >= 45.0       { Color::LightGreen }
                       else if pnl >= 10.0       { Color::Green }
                       else if pnl > -5.0        { Color::Yellow }
                       else if pnl > -25.0       { Color::LightRed }
                       else                      { Color::Red };

            // Decline streak indicator — prefix on status
            let streak_pfx = match p.decline_streak {
                3..=4 => "▼ ",
                5..   => "▼▼ ",
                _     => "",
            };

            // Status: escalation > stale > momentum > normal
            let status_body = if staleness > 5 {
                format!("stale {}s", staleness)
            } else if p.escalations > 0 {
                format!("esc x{}", p.escalations)
            } else if peak >= 45.0 {
                "riding".to_string()
            } else if pnl > 10.0 {
                "green".to_string()
            } else if pnl > -5.0 {
                "watch".to_string()
            } else {
                "down".to_string()
            };
            let status = format!("{}{}", streak_pfx, status_body);
            let status_color = if staleness > 5 { Color::DarkGray }
                          else if p.decline_streak >= 5 { Color::Red }
                          else if p.decline_streak >= 3 { Color::LightRed }
                          else { pnl_color };

            // Progress bar: SL ←|→ TP, 8 chars wide
            let prog = p.progress_to_tp();
            let bar_width: usize = 8;
            let filled = ((prog * bar_width as f64).round() as usize).min(bar_width);
            let bar: String = (0..bar_width).map(|i| {
                if i < filled { '█' } else { '░' }
            }).collect();
            let bar_color = if prog >= 0.85 { Color::LightGreen }
                       else if prog >= 0.5  { Color::Green }
                       else if prog >= 0.25 { Color::Yellow }
                       else                 { Color::Red };

            // Age: <60 s → seconds, <60 min → Xm Ys, else Xh Ym
            let age_str = if age < 60 { format!("{}s", age) }
                     else if age < 3600 { format!("{}m{}s", age/60, age%60) }
                     else { format!("{}h{}m", age/3600, (age%3600)/60) };

            // SL column: show as % from entry (negative = loss floor)
            let sl_pct = p.current_sl_pct;
            let sl_str = format!("{:+.0}%", sl_pct);
            let sl_color = if sl_pct >= 0.0 { Color::Green }
                      else if sl_pct > -10.0 { Color::Yellow }
                      else { Color::LightRed };

            Row::new(vec![
                Cell::from(p.mint[..8.min(p.mint.len())].to_string()),
                Cell::from(age_str),
                Cell::from(format!("{:.4}", value_sol))
                    .style(Style::default().fg(pnl_color)),
                Cell::from(format!("{:+.1}%", pnl))
                    .style(Style::default().fg(pnl_color).add_modifier(Modifier::BOLD)),
                Cell::from(format!("{:+.1}%", peak))
                    .style(Style::default().fg(Color::Cyan)),
                Cell::from(sl_str)
                    .style(Style::default().fg(sl_color)),
                Cell::from(format!("{:.0}%", p.dynamic_tp_pct))
                    .style(Style::default().fg(Color::Magenta)),
                Cell::from(bar)
                    .style(Style::default().fg(bar_color)),
                Cell::from(status)
                    .style(Style::default().fg(status_color)),
            ])
        })
        .collect();

    let widths = [
        Constraint::Length(9),  // Mint
        Constraint::Length(7),  // Age
        Constraint::Length(8),  // Value SOL
        Constraint::Length(8),  // PnL %
        Constraint::Length(8),  // Peak %
        Constraint::Length(6),  // SL floor %
        Constraint::Length(6),  // TP target %
        Constraint::Length(10), // Progress bar
        Constraint::Min(8),     // Status
    ];
    let table = Table::new(rows, widths)
        .header(
            Row::new(vec![
                Cell::from("Mint").style(Style::default().fg(COLOR_ACCENT).add_modifier(Modifier::BOLD)),
                Cell::from("Age").style(Style::default().fg(COLOR_ACCENT).add_modifier(Modifier::BOLD)),
                Cell::from("Value").style(Style::default().fg(COLOR_ACCENT).add_modifier(Modifier::BOLD)),
                Cell::from("PnL").style(Style::default().fg(COLOR_ACCENT).add_modifier(Modifier::BOLD)),
                Cell::from("Peak").style(Style::default().fg(COLOR_ACCENT).add_modifier(Modifier::BOLD)),
                Cell::from("SL").style(Style::default().fg(COLOR_ACCENT).add_modifier(Modifier::BOLD)),
                Cell::from("TP").style(Style::default().fg(COLOR_ACCENT).add_modifier(Modifier::BOLD)),
                Cell::from("Progress").style(Style::default().fg(COLOR_ACCENT).add_modifier(Modifier::BOLD)),
                Cell::from("Status").style(Style::default().fg(COLOR_ACCENT).add_modifier(Modifier::BOLD)),
            ]),
        )
        .block(block);
    f.render_widget(table, area);
}

fn render_metrics(f: &mut Frame, area: Rect, state: &Arc<AppState>) {
    let m = state.effective_snapshot();
    let scema = *state.scematica_balance.read();
    let sol = *state.sol_balance.read();
    let price_usd = *state.sol_price_usd.read();

    let sol_usd_str = if price_usd > 0.0 {
        format!("{:.4} SOL  (${:.2})", sol, sol * price_usd)
    } else {
        format!("{:.4} SOL", sol)
    };
    let price_str = if price_usd > 0.0 {
        format!("${:.2} / SOL", price_usd)
    } else {
        "fetching…".to_string()
    };
    let pnl_usd_str = if price_usd > 0.0 {
        format!("{:.6} SOL  (${:.4})", m.total_pnl_sol(), m.total_pnl_sol() * price_usd)
    } else {
        format!("{:.6} SOL", m.total_pnl_sol())
    };

    let rows: Vec<Row> = vec![
        Row::new(vec![Cell::from("Trades Attempted"), Cell::from(m.trades_attempted.to_string())]),
        Row::new(vec![Cell::from("Trades Confirmed"), Cell::from(m.trades_confirmed.to_string())]),
        Row::new(vec![Cell::from("Trades Failed"), Cell::from(m.trades_failed.to_string())]),
        Row::new(vec![Cell::from("Win Rate"), Cell::from(format!("{:.1}%", m.win_rate()))]),
        Row::new(vec![Cell::from("Arbs Found"), Cell::from(m.arb_opportunities_found.to_string())]),
        Row::new(vec![Cell::from("Arbs Executed"), Cell::from(m.arb_executed.to_string())]),
        Row::new(vec![Cell::from("Total PnL"), Cell::from(pnl_usd_str)]),
        Row::new(vec![Cell::from("Pools Tracked"), Cell::from(m.pools_tracked.to_string())]),
        Row::new(vec![Cell::from("Uptime"), Cell::from(format!("{}s", m.uptime_secs))]),
        Row::new(vec![
            Cell::from("SOL Balance"),
            Cell::from(sol_usd_str).style(Style::default().fg(Color::Cyan)),
        ]),
        Row::new(vec![
            Cell::from("SOL Price"),
            Cell::from(price_str).style(Style::default().fg(Color::Yellow)),
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
    let open = state.open_position_count();

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

fn render_alert_history(f: &mut Frame, area: Rect, state: &Arc<AppState>) {
    let alerts = state.alert_history.read();
    let block = Block::default()
        .title(" Recent Alerts ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow));

    let items: Vec<ListItem> = if alerts.is_empty() {
        vec![ListItem::new(
            Line::from(Span::styled("No alerts yet", Style::default().fg(Color::DarkGray))),
        )]
    } else {
        alerts.iter().map(|(ts, title, body)| {
            let is_sell = title.contains("SELL");
            let is_buy  = title.contains("BUY");
            let col = if is_sell && title.contains('✓') { Color::Green }
                      else if is_buy { Color::Cyan }
                      else { Color::Yellow };
            ListItem::new(Line::from(vec![
                Span::styled(
                    format!("{} ", ts.format("%H:%M:%S")),
                    Style::default().fg(Color::DarkGray),
                ),
                Span::styled(format!("{} — {}", title, body), Style::default().fg(col)),
            ]))
        }).collect()
    };

    let list = List::new(items).block(block);
    f.render_widget(list, area);
}

fn render_nn_stats(f: &mut Frame, area: Rect, state: &Arc<AppState>) {
    let nn = state.nn_stats.read();

    // Split area: top for stats table, bottom for Q-value bar chart
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(7), Constraint::Length(4)])
        .split(area);

    let block = Block::default()
        .title(" 🧠 Deep Q* Agent ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));

    let action_labels = ["Hold", "Buy", "BuyAgg", "SellP", "SellAll"];

    if let Some(v) = &*nn {
        let epsilon  = v["epsilon"].as_f64().unwrap_or(1.0);
        let steps    = v["step_count"].as_u64().unwrap_or(0);
        let replay   = v["replay_size"].as_u64().unwrap_or(0);
        let loss     = v["avg_loss"].as_f64().unwrap_or(0.0);
        let reward   = v["total_reward"].as_f64().unwrap_or(0.0);
        let ready    = v["ready_to_advise"].as_bool().unwrap_or(false);
        let last_act = v["last_action"].as_str().unwrap_or("-").to_string();
        let ready_str = if ready { "YES" } else { "NO" };
        let ready_col = if ready { Color::Green } else { Color::Yellow };

        let rows = vec![
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
                Cell::from(format!("{:.2}", reward)),
            ]),
            Row::new(vec![
                Cell::from("Last Action").style(Style::default().fg(Color::DarkGray)),
                Cell::from(last_act),
            ]),
            Row::new(vec![
                Cell::from("Advising").style(Style::default().fg(Color::DarkGray)),
                Cell::from(ready_str).style(Style::default().fg(ready_col)),
            ]),
        ];
        let table = Table::new(rows, [Constraint::Length(16), Constraint::Min(0)])
            .block(block);
        f.render_widget(table, chunks[0]);

        // Q-value bar chart — one bar per action, width proportional to Q-value
        let q_vals: Vec<f64> = if let Some(arr) = v["last_q_values"].as_array() {
            arr.iter().filter_map(|x| x.as_f64()).collect()
        } else {
            Vec::new()
        };

        if !q_vals.is_empty() {
            let q_block = Block::default()
                .title(" Q-values ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::DarkGray));

            let q_min = q_vals.iter().cloned().fold(f64::INFINITY, f64::min);
            let q_max = q_vals.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
            let q_range = (q_max - q_min).max(1.0);
            let max_action = q_vals.iter()
                .enumerate()
                .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
                .map(|(i, _)| i)
                .unwrap_or(0);

            let bar_w = (chunks[1].width.saturating_sub(2)) as usize / action_labels.len().max(1);
            let mut spans: Vec<Span> = Vec::new();
            for (i, (&q, &label)) in q_vals.iter().zip(action_labels.iter()).enumerate() {
                let norm = ((q - q_min) / q_range * (bar_w.saturating_sub(1)) as f64) as usize;
                let bar: String = "█".repeat(norm.max(1));
                let col = if i == max_action { Color::Green } else { Color::Blue };
                spans.push(Span::styled(
                    format!("{:<width$}", format!("{} {:.1}", label, q), width = bar_w),
                    Style::default().fg(col),
                ));
                let _ = bar; // suppress unused warning; label already carries width signal
            }
            let p = Paragraph::new(Line::from(spans)).block(q_block);
            f.render_widget(p, chunks[1]);
        } else {
            let q_block = Block::default()
                .title(" Q-values ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::DarkGray));
            let p = Paragraph::new("  Waiting for first Q-value observation…")
                .style(Style::default().fg(Color::DarkGray))
                .block(q_block);
            f.render_widget(p, chunks[1]);
        }
    } else {
        let table = Table::new(
            vec![Row::new(vec![Cell::from("Waiting for NN agent...").style(Style::default().fg(Color::DarkGray))])],
            [Constraint::Min(0)],
        ).block(block);
        f.render_widget(table, chunks[0]);

        let q_block = Block::default()
            .title(" Q-values ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray));
        f.render_widget(q_block, chunks[1]);
    }
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
    let price_usd = *state.sol_price_usd.read();
    let sol_label = if price_usd > 0.0 {
        format!("{:.4} (${:.2})", sol, sol * price_usd)
    } else {
        format!("{:.4}", sol)
    };
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
            " DUMP MODE ACTIVE  |  SOL: {}  |  Force-selling ALL positions  |  [d] to deactivate ",
            sol_label
        );
        let banner = Paragraph::new(banner_text)
            .style(Style::default().fg(Color::Black).bg(Color::Yellow).add_modifier(Modifier::BOLD))
            .wrap(Wrap { trim: true })
            .block(Block::default().borders(Borders::ALL).border_style(Style::default().fg(Color::Yellow)));
        f.render_widget(banner, chunks[0]);
    }

    if sell_mode {
        let banner_text = format!(
            " SELL MODE ACTIVE  |  SOL: {}  |  Buying paused — selling all positions  |  [e] to deactivate ",
            sol_label
        );
        let banner = Paragraph::new(banner_text)
            .style(Style::default().fg(Color::Black).bg(COLOR_ACCENT).add_modifier(Modifier::BOLD))
            .wrap(Wrap { trim: true })
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
    // inner_width: subtract 2 for borders, clamp to at least 10 so we always make progress.
    let inner_width = log_area.width.saturating_sub(2).max(10) as usize;
    let max_rows = log_area.height.saturating_sub(2) as usize;
    let filter_lower = filter_text.to_lowercase();
    // Prefix for wrapped continuation lines — keeps visual structure clear.
    let cont_prefix = "  ↪ ";
    // inner_width for continuation lines (shorter by prefix len)
    let cont_width = inner_width.saturating_sub(cont_prefix.len()).max(4);

    let mut lines: Vec<Line> = Vec::new();
    'outer: for raw in logs.iter().rev() {
        if !filter_lower.is_empty() && !raw.to_lowercase().contains(&filter_lower) {
            continue;
        }
        // Classify line color by keywords that survive any prefix we prepend.
        let color = if raw.contains("DUMP MODE") || raw.contains("[DUMP]") {
            Color::Yellow
        } else if raw.contains("ERROR") || raw.contains("[ERROR]") || raw.contains("SELL MODE") {
            COLOR_ACCENT
        } else if raw.contains("WARN") || raw.contains("[WARN]") {
            Color::Yellow
        } else if raw.contains("confirmed") || raw.contains("Sell confirmed") || raw.contains("[INFO] sell") {
            Color::Green
        } else if raw.contains("[INFO]") || raw.contains("[TRADE]") {
            COLOR_TEXT
        } else {
            COLOR_TEXT
        };
        let style      = Style::default().fg(color);
        let cont_style = Style::default().fg(color).add_modifier(Modifier::DIM);

        // Safety-clip absurdly long lines before processing so we don't spend
        // microseconds chunking a 10 000-char corrupt entry.
        let raw_clipped: &str = if raw.chars().count() > 1000 {
            &raw[..raw.char_indices().nth(1000).map(|(i, _)| i).unwrap_or(raw.len())]
        } else {
            raw.as_str()
        };

        let chars: Vec<char> = raw_clipped.chars().collect();
        if chars.is_empty() {
            // blank separator — still push so timestamp gaps are visible
            lines.push(Line::from(Span::styled(String::new(), style)));
            if lines.len() >= max_rows { break; }
        } else {
            // First chunk: full inner_width
            let first_end = inner_width.min(chars.len());
            lines.push(Line::from(Span::styled(
                chars[..first_end].iter().collect::<String>(),
                style,
            )));
            if lines.len() >= max_rows { break 'outer; }

            // Continuation chunks: slightly narrower with "  ↪ " prefix
            let mut pos = first_end;
            while pos < chars.len() {
                let end = (pos + cont_width).min(chars.len());
                let chunk: String = chars[pos..end].iter().collect();
                lines.push(Line::from(vec![
                    Span::styled(cont_prefix, cont_style),
                    Span::styled(chunk, style),
                ]));
                if lines.len() >= max_rows { break 'outer; }
                pos = end;
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
    let builder_mode = *state.builder_mode.read();
    let moon_chase = *state.moon_chase.read();

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
            let active = rate_mode == RateMode::Bearish;
            Line::from(vec![
                Span::styled(if active { "▶ " } else { "  " }, Style::default().fg(if active { Color::Blue } else { Color::DarkGray })),
                Span::styled("[1] Bearish    ", Style::default().fg(if active { Color::Blue } else { COLOR_TEXT }).add_modifier(if active { Modifier::BOLD } else { Modifier::empty() })),
                Span::styled("0.3x  0.003 SOL/trade  TP: 45%  SL:  8%", Style::default().fg(if active { Color::Blue } else { Color::DarkGray })),
            ])
        },
        {
            let active = rate_mode == RateMode::Micro;
            Line::from(vec![
                Span::styled(if active { "▶ " } else { "  " }, Style::default().fg(if active { Color::Cyan } else { Color::DarkGray })),
                Span::styled("[2] Micro      ", Style::default().fg(if active { Color::Cyan } else { COLOR_TEXT }).add_modifier(if active { Modifier::BOLD } else { Modifier::empty() })),
                Span::styled("0.1x  0.001 SOL/trade  TP: 60%  SL: 10%  (≈$1–2 wallets)", Style::default().fg(if active { Color::Cyan } else { Color::DarkGray })),
            ])
        },
        {
            let active = rate_mode == RateMode::Safe;
            Line::from(vec![
                Span::styled(if active { "▶ " } else { "  " }, Style::default().fg(if active { Color::Green } else { Color::DarkGray })),
                Span::styled("[3] Safe       ", Style::default().fg(if active { Color::Green } else { COLOR_TEXT }).add_modifier(if active { Modifier::BOLD } else { Modifier::empty() })),
                Span::styled("0.5x  0.005 SOL/trade  TP: 75%  SL: 10%", Style::default().fg(if active { Color::Green } else { Color::DarkGray })),
            ])
        },
        {
            let active = rate_mode == RateMode::Balanced;
            Line::from(vec![
                Span::styled(if active { "▶ " } else { "  " }, Style::default().fg(if active { Color::Green } else { Color::DarkGray })),
                Span::styled("[4] Balanced   ", Style::default().fg(if active { Color::Green } else { COLOR_TEXT }).add_modifier(if active { Modifier::BOLD } else { Modifier::empty() })),
                Span::styled("1.0x  0.010 SOL/trade  TP:150%  SL: 15%", Style::default().fg(if active { Color::Green } else { Color::DarkGray })),
            ])
        },
        {
            let active = rate_mode == RateMode::Aggressive;
            Line::from(vec![
                Span::styled(if active { "▶ " } else { "  " }, Style::default().fg(if active { Color::Yellow } else { Color::DarkGray })),
                Span::styled("[5] Aggressive ", Style::default().fg(if active { Color::Yellow } else { COLOR_TEXT }).add_modifier(if active { Modifier::BOLD } else { Modifier::empty() })),
                Span::styled("2.0x  0.020 SOL/trade  TP:300%  SL: 25%", Style::default().fg(if active { Color::Yellow } else { Color::DarkGray })),
            ])
        },
        {
            let active = rate_mode == RateMode::Degen;
            Line::from(vec![
                Span::styled(if active { "▶ " } else { "  " }, Style::default().fg(if active { COLOR_ACCENT } else { Color::DarkGray })),
                Span::styled("[6] Degen      ", Style::default().fg(if active { COLOR_ACCENT } else { COLOR_TEXT }).add_modifier(if active { Modifier::BOLD } else { Modifier::empty() })),
                Span::styled("4.0x  0.040 SOL/trade  TP:450%  SL: 40%", Style::default().fg(if active { COLOR_ACCENT } else { Color::DarkGray })),
            ])
        },
        {
            let active = rate_mode == RateMode::Bullish;
            Line::from(vec![
                Span::styled(if active { "▶ " } else { "  " }, Style::default().fg(if active { Color::Magenta } else { Color::DarkGray })),
                Span::styled("[7] Bullish    ", Style::default().fg(if active { Color::Magenta } else { COLOR_TEXT }).add_modifier(if active { Modifier::BOLD } else { Modifier::empty() })),
                Span::styled("6.0x  0.060 SOL/trade  TP:750%  SL: 50%", Style::default().fg(if active { Color::Magenta } else { Color::DarkGray })),
            ])
        },
        {
            let active = rate_mode == RateMode::Moon;
            Line::from(vec![
                Span::styled(if active { "▶ " } else { "  " }, Style::default().fg(if active { Color::LightMagenta } else { Color::DarkGray })),
                Span::styled("[8] 🌙 Moon    ", Style::default().fg(if active { Color::LightMagenta } else { COLOR_TEXT }).add_modifier(if active { Modifier::BOLD } else { Modifier::empty() })),
                Span::styled("8.0x  0.080 SOL/trade  TP:1200% SL: 60%  (parabolic chase)", Style::default().fg(if active { Color::LightMagenta } else { Color::DarkGray })),
            ])
        },
        Line::from(""),
        {
            let mc_color = if moon_chase { Color::LightMagenta } else { Color::DarkGray };
            let mc_text  = if moon_chase {
                "🌙 ENGAGED  —  8 escalations × 1.75×  |  pullback 25%  |  threshold 3%/check"
            } else {
                "disengaged  —  press [m] to enable parabolic-greedy escalation"
            };
            Line::from(vec![
                Span::styled("[m] Moon Chase: ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                Span::styled(mc_text, Style::default().fg(mc_color).add_modifier(if moon_chase { Modifier::BOLD } else { Modifier::empty() })),
            ])
        },
        Line::from(""),
        Line::from(vec![
            Span::styled("Builder Mode", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            Span::raw("  (compounding algorithm — live size/TP/SL from wallet progress)"),
        ]),
        {
            let active = builder_mode == BuilderMode::Off;
            Line::from(vec![
                Span::styled(if active { "▶ " } else { "  " }, Style::default().fg(Color::DarkGray)),
                Span::styled("[o] Off          ", Style::default().fg(if active { Color::White } else { COLOR_TEXT }).add_modifier(if active { Modifier::BOLD } else { Modifier::empty() })),
                Span::styled(BuilderMode::Off.algo_description(), Style::default().fg(Color::DarkGray)),
            ])
        },
        {
            let active = builder_mode == BuilderMode::Growth;
            Line::from(vec![
                Span::styled(if active { "▶ " } else { "  " }, Style::default().fg(if active { Color::Green } else { Color::DarkGray })),
                Span::styled("[g] Growth 0.2   ", Style::default().fg(if active { Color::Green } else { COLOR_TEXT }).add_modifier(if active { Modifier::BOLD } else { Modifier::empty() })),
                Span::styled(BuilderMode::Growth.algo_description(), Style::default().fg(if active { Color::Green } else { Color::DarkGray })),
            ])
        },
        {
            let active = builder_mode == BuilderMode::Builder;
            Line::from(vec![
                Span::styled(if active { "▶ " } else { "  " }, Style::default().fg(if active { Color::Yellow } else { Color::DarkGray })),
                Span::styled("[j] Builder 1.0  ", Style::default().fg(if active { Color::Yellow } else { COLOR_TEXT }).add_modifier(if active { Modifier::BOLD } else { Modifier::empty() })),
                Span::styled(BuilderMode::Builder.algo_description(), Style::default().fg(if active { Color::Yellow } else { Color::DarkGray })),
            ])
        },
        {
            let active = builder_mode == BuilderMode::SuperBuilder;
            Line::from(vec![
                Span::styled(if active { "▶ " } else { "  " }, Style::default().fg(if active { Color::Magenta } else { Color::DarkGray })),
                Span::styled("[k] SuperBld 3.0 ", Style::default().fg(if active { Color::Magenta } else { COLOR_TEXT }).add_modifier(if active { Modifier::BOLD } else { Modifier::empty() })),
                Span::styled(BuilderMode::SuperBuilder.algo_description(), Style::default().fg(if active { Color::Magenta } else { Color::DarkGray })),
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

    let scroll = *state.config_scroll.read();
    let para = Paragraph::new(all_lines)
        .block(
            Block::default()
                .title(" ⚙️  Configuration  (↑/↓ scroll) ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(COLOR_ACCENT)),
        )
        .scroll((scroll, 0));
    f.render_widget(para, area);
}

fn render_footer(f: &mut Frame, area: Rect, current_tab: usize) {
    let hint = match current_tab {
        1 => " [x] Export CSV  [R] Reset Positions  [Tab] Switch  [q] Quit  [←/→] Navigate ",
        2 => " [/] Filter  [e] Sell  [b] Buy  [h] High-Speed  [d] DUMP ALL  [Tab] Switch  [q] Quit ",
        3 => " [s/a/b/x] Bot  [1-8] Rate  [g] Growth  [j] Builder  [k] SuperBld  [o] Off  [↑↓] Scroll  [Tab] Switch  [q] Quit ",
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

    // X-axis: seconds since the sniper observed each pool (now - timestamp).
    // The on-disk `age_secs` field is the pool's age at evaluation time, but
    // `pool.open_time` is rarely populated by Raydium for new pools so most
    // entries report 0 there — which would stack every dot at x=0 and render
    // a useless radar. Using `now - timestamp` instead gives a proper temporal
    // spread regardless of open_time availability.
    let now = chrono::Utc::now().timestamp();
    const X_MAX: f64 = 300.0;
    let to_x = |ts: i64| ((now - ts).max(0) as f64).min(X_MAX - 1.0);

    // Y-axis: log10(size_sol + 1) so the scale covers the full range of pools we
    // actually see (0.1 SOL pump.fun → 1000+ SOL whale pools) without crushing
    // small ones into the bottom row or clamping large ones.
    //   1 SOL  → y ≈ 0.30
    //   10 SOL → y ≈ 1.04
    //   100 SOL → y ≈ 2.00
    //   1000 SOL → y ≈ 3.00
    const Y_MAX: f64 = 3.5;
    let to_y = |sol: f64| (sol.max(0.0) + 1.0).log10().clamp(0.0, Y_MAX);

    // Separate passed / rejected pools for color-coded Points
    let passed_coords: Vec<(f64, f64)> = pools
        .iter()
        .filter(|p| p.passed_filters)
        .map(|p| (to_x(p.timestamp), to_y(p.size_sol)))
        .collect();
    let rejected_coords: Vec<(f64, f64)> = pools
        .iter()
        .filter(|p| !p.passed_filters)
        .map(|p| (to_x(p.timestamp), to_y(p.size_sol)))
        .collect();

    let total = pools.len();
    let passed = pools.iter().filter(|p| p.passed_filters).count();
    let rejected = total - passed;
    let canvas_title = format!(
        " Pool Radar — Seen vs Size (log)  |  total: {}  passed: {} (green)  rejected: {} (red) ",
        total, passed, rejected,
    );

    let canvas = Canvas::default()
        .block(
            Block::default()
                .title(canvas_title)
                .borders(Borders::ALL)
                .border_style(Style::default().fg(COLOR_ACCENT)),
        )
        .x_bounds([0.0, X_MAX])
        .y_bounds([0.0, Y_MAX])
        .paint(move |ctx| {
            // X = seconds since sniper observed the pool; Y = log10(size_sol+1).
            ctx.print(0.0,   -0.15, "now");
            ctx.print(150.0, -0.15, "2.5m ago");
            ctx.print(285.0, -0.15, "5m ago");
            ctx.print(-18.0, 0.30, "1");
            ctx.print(-22.0, 1.04, "10");
            ctx.print(-26.0, 2.00, "100");
            ctx.print(-30.0, 3.00, "1k SOL");

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

    // Table: 10 most recent pools (sorted by timestamp descending — newest first).
    // Sorting matters when entries arrive out of order from concurrent evaluators.
    let mut sorted = pools.clone();
    sorted.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
    let rows: Vec<Row> = sorted
        .iter()
        .take(10)
        .map(|p| {
            let seen_secs_ago = (now - p.timestamp).max(0);
            let pass_str = if p.passed_filters { "PASS" } else { "FAIL" };
            let pass_color = if p.passed_filters { Color::Green } else { Color::Red };
            let mint_short = p.mint[..8.min(p.mint.len())].to_string();
            // Prefer the on-disk pool age when populated, otherwise fall back to "seen"
            let age_cell = if p.age_secs > 0.0 {
                format!("{:.0}s", p.age_secs)
            } else {
                format!("~{}s", seen_secs_ago)
            };
            Row::new(vec![
                Cell::from(mint_short),
                Cell::from(age_cell),
                Cell::from(format!("{:.2}", p.size_sol)),
                Cell::from(pass_str).style(Style::default().fg(pass_color)),
                Cell::from(format!("{:.1}", p.score)),
                Cell::from(format!("{}s ago", seen_secs_ago)),
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
