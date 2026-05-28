use crate::agent::AGENT_COLORS;
use crate::app::{App, InputMode};
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{
        Block, BorderType, Borders, List, ListItem, Paragraph, Wrap,
    },
    Frame,
};

const HELP_TEXT: &[&str] = &[
    "  Keyboard shortcuts:",
    "  [s]         Start / Pause conversation",
    "  [n]         Step one turn (manual)",
    "  [c]         Clear history",
    "  [m]         List Ollama models",
    "  [j/k]       Scroll messages",
    "  [PgUp/Dn]   Scroll fast",
    "  [G]         Jump to latest (follow mode)",
    "  [g]         Jump to top",
    "  [i] or [:]  Open command input",
    "  [?]         Toggle this help",
    "  [q]         Quit",
    "",
    "  Commands (in input mode):",
    "  /topic <text>              Set debate topic",
    "  /add <name> ollama <model> Add Ollama agent",
    "  /add <name> groq <model> <key>",
    "  /add <name> openrouter <model> <key>",
    "  /add <name> lmstudio <model> [<url>]",
    "  /add <name> cerebras <model> <key>",
    "  /remove <name>             Remove agent",
    "  /delay <ms>                Set turn delay",
    "  /agents                    List agents",
    "  /clear                     Clear history",
    "  /models                    List Ollama models",
    "  /start  /pause  /step",
];

pub fn render(f: &mut Frame, app: &App) {
    let area = f.size();

    let outer = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .border_style(Style::default().fg(Color::Rgb(0, 180, 220)))
        .title(Span::styled(
            "  AGENT PLAYGROUND  ",
            Style::default()
                .fg(Color::Rgb(0, 240, 255))
                .add_modifier(Modifier::BOLD),
        ))
        .title_alignment(Alignment::Center);

    let inner = outer.inner(area);
    f.render_widget(outer, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // header bar
            Constraint::Min(0),    // body
            Constraint::Length(1), // stats bar
            Constraint::Length(3), // input
        ])
        .split(inner);

    render_header(f, app, chunks[0]);
    render_body(f, app, chunks[1]);
    render_stats(f, app, chunks[2]);
    render_input(f, app, chunks[3]);
}

fn render_header(f: &mut Frame, app: &App, area: Rect) {
    let (status_sym, status_color) = if app.is_running {
        ("◉ LIVE", Color::Rgb(0, 255, 100))
    } else {
        ("◎ IDLE", Color::Rgb(180, 180, 60))
    };

    let follow_tag = if app.follow_mode {
        Span::styled(" [follow]", Style::default().fg(Color::Rgb(60, 180, 60)))
    } else {
        Span::styled(" [scroll]", Style::default().fg(Color::DarkGray))
    };

    let topic_short = if app.topic.len() > 55 {
        format!("{}...", &app.topic[..55])
    } else {
        app.topic.clone()
    };

    let line = Line::from(vec![
        Span::styled(status_sym, Style::default().fg(status_color).add_modifier(Modifier::BOLD)),
        Span::styled(
            format!("  |  Topic: {}  |  Round: {}  |  Delay: {}ms", topic_short, app.round, app.turn_delay_ms),
            Style::default().fg(Color::Rgb(180, 180, 180)),
        ),
        follow_tag,
    ]);
    f.render_widget(Paragraph::new(line), area);
}

fn render_body(f: &mut Frame, app: &App, area: Rect) {
    if app.help_visible {
        render_help(f, area);
        return;
    }

    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(22), Constraint::Percentage(78)])
        .split(area);

    render_agents_panel(f, app, chunks[0]);
    render_arena(f, app, chunks[1]);
}

fn render_help(f: &mut Frame, area: Rect) {
    let lines: Vec<Line> = HELP_TEXT
        .iter()
        .map(|l| {
            if l.trim_start().starts_with('/') || l.trim_start().starts_with('[') {
                Line::from(Span::styled(*l, Style::default().fg(Color::Rgb(0, 220, 180))))
            } else if l.is_empty() {
                Line::from("")
            } else {
                Line::from(Span::styled(*l, Style::default().fg(Color::Rgb(200, 200, 200))))
            }
        })
        .collect();

    let block = Block::default()
        .title(" Help — press [?] to close ")
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Color::Rgb(0, 220, 180)));

    let para = Paragraph::new(lines)
        .block(block)
        .wrap(Wrap { trim: false });

    f.render_widget(para, area);
}

fn render_agents_panel(f: &mut Frame, app: &App, area: Rect) {
    let items: Vec<ListItem> = app
        .agents
        .iter()
        .enumerate()
        .map(|(i, agent)| {
            let color = AGENT_COLORS[i % AGENT_COLORS.len()];
            let status = app.agent_statuses.get(&agent.name);
            let thinking = status.map(|s| s.is_thinking).unwrap_or(false);
            let online = status.map(|s| s.is_online).unwrap_or(true);
            let msgs = status.map(|s| s.messages_sent).unwrap_or(0);
            let tokens = status.map(|s| s.total_tokens).unwrap_or(0);
            let latency = status.map(|s| s.last_latency_ms).unwrap_or(0);

            let (sym, sym_color) = if !online {
                ("✗", Color::Red)
            } else if thinking {
                ("~", Color::Yellow)
            } else {
                ("●", color)
            };

            let name_style = if thinking {
                Style::default().fg(color).add_modifier(Modifier::BOLD | Modifier::SLOW_BLINK)
            } else {
                Style::default().fg(color).add_modifier(Modifier::BOLD)
            };

            let mut lines = vec![
                Line::from(vec![
                    Span::styled(format!("{} ", sym), Style::default().fg(sym_color)),
                    Span::styled(agent.name.clone(), name_style),
                ]),
                Line::from(Span::styled(
                    format!("  {}", agent.model_label()),
                    Style::default().fg(Color::DarkGray),
                )),
                Line::from(Span::styled(
                    format!("  msgs:{} tok:{}", msgs, tokens),
                    Style::default().fg(Color::Rgb(100, 100, 100)),
                )),
            ];

            if latency > 0 {
                lines.push(Line::from(Span::styled(
                    format!("  {}ms", latency),
                    Style::default().fg(Color::Rgb(80, 80, 80)),
                )));
            }

            if let Some(err) = status.and_then(|s| s.last_error.as_deref()) {
                let short = if err.len() > 18 { &err[..18] } else { err };
                lines.push(Line::from(Span::styled(
                    format!("  ERR:{}", short),
                    Style::default().fg(Color::Red),
                )));
            }

            lines.push(Line::from("")); // spacer
            ListItem::new(lines)
        })
        .collect();

    let list = List::new(items).block(
        Block::default()
            .title(" Agents ")
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(Color::Rgb(60, 100, 180))),
    );

    f.render_widget(list, area);
}

fn render_arena(f: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .title(format!(" Arena  {} messages ", app.messages.len()))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Color::Rgb(130, 0, 180)));

    let inner = block.inner(area);
    f.render_widget(block, area);

    if app.messages.is_empty() {
        // Show idle splash
        let splash = vec![
            Line::from(""),
            Line::from(""),
            Line::from(Span::styled(
                "        No messages yet.",
                Style::default().fg(Color::DarkGray),
            )),
            Line::from(""),
            Line::from(Span::styled(
                "        Add agents and set a topic, then press [s] to start.",
                Style::default().fg(Color::Rgb(80, 80, 80)),
            )),
            Line::from(""),
            Line::from(Span::styled(
                "        Run with --demo for a quick 3-agent debate.",
                Style::default().fg(Color::Rgb(60, 60, 60)),
            )),
        ];
        f.render_widget(Paragraph::new(splash), inner);

        // Show thinking indicator even when history is empty
        render_thinking_overlay(f, app, inner);
        return;
    }

    let mut all_lines: Vec<Line> = Vec::new();

    for msg in &app.messages {
        let color = app.agent_color(&msg.from_agent);
        let ts = msg.timestamp.format("%H:%M:%S").to_string();
        let tok_str = msg
            .tokens
            .map(|t| format!("  [{} tok  {}ms]", t, msg.latency_ms))
            .unwrap_or_default();

        // Message header
        all_lines.push(Line::from(vec![
            Span::styled(ts, Style::default().fg(Color::Rgb(80, 80, 80))),
            Span::raw("  "),
            Span::styled(
                msg.from_agent.clone(),
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            ),
            Span::styled(tok_str, Style::default().fg(Color::Rgb(60, 60, 60))),
        ]));

        // Separator under name
        all_lines.push(Line::from(Span::styled(
            "  ─────────────────────────────────────",
            Style::default().fg(Color::Rgb(50, 50, 50)),
        )));

        // Message body — each source line becomes a rendered line
        for text_line in msg.content.lines() {
            all_lines.push(Line::from(vec![
                Span::raw("  "),
                Span::styled(text_line.to_string(), Style::default().fg(Color::Rgb(220, 220, 220))),
            ]));
        }

        all_lines.push(Line::from("")); // gap between messages
    }

    // Thinking indicators appended at bottom
    for (name, status) in &app.agent_statuses {
        if status.is_thinking {
            let color = app.agent_color(name);
            all_lines.push(Line::from(vec![
                Span::styled("  ~ ", Style::default().fg(Color::Yellow)),
                Span::styled(name.clone(), Style::default().fg(color).add_modifier(Modifier::BOLD)),
                Span::styled(
                    " is thinking...",
                    Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC),
                ),
            ]));
        }
    }

    let total_lines = all_lines.len() as u16;
    let visible = inner.height;
    let max_scroll = total_lines.saturating_sub(visible);
    let scroll = app.scroll_offset.min(max_scroll);

    let para = Paragraph::new(all_lines)
        .scroll((scroll, 0))
        .wrap(Wrap { trim: false });

    f.render_widget(para, inner);
}

fn render_thinking_overlay(f: &mut Frame, app: &App, area: Rect) {
    for (name, status) in &app.agent_statuses {
        if status.is_thinking {
            let color = app.agent_color(name);
            let line = Line::from(vec![
                Span::styled("  ~ ", Style::default().fg(Color::Yellow)),
                Span::styled(name.clone(), Style::default().fg(color).add_modifier(Modifier::BOLD)),
                Span::styled(" is thinking...", Style::default().fg(Color::DarkGray)),
            ]);
            f.render_widget(Paragraph::new(vec![line]), area);
        }
    }
}

fn render_stats(f: &mut Frame, app: &App, area: Rect) {
    let active_count = app
        .agent_statuses
        .values()
        .filter(|s| s.is_online)
        .count();

    let thinking_names: Vec<&str> = app
        .agent_statuses
        .values()
        .filter(|s| s.is_thinking)
        .map(|s| s.name.as_str())
        .collect();

    let thinking_str = if thinking_names.is_empty() {
        String::new()
    } else {
        format!("  |  thinking: {}", thinking_names.join(", "))
    };

    let bar = format!(
        " msgs={}  tokens={}  agents={}/{}  rounds={}{}  |  {}",
        app.messages.len(),
        app.total_tokens,
        active_count,
        app.agents.len(),
        app.round,
        thinking_str,
        app.status_msg,
    );

    let style = if app.status_msg.starts_with("[ERR]") {
        Style::default().fg(Color::Red)
    } else {
        Style::default().fg(Color::Rgb(100, 100, 100))
    };

    f.render_widget(Paragraph::new(bar).style(style), area);
}

fn render_input(f: &mut Frame, app: &App, area: Rect) {
    let (border_color, title, content_line) = match app.input_mode {
        InputMode::Normal => (
            Color::Rgb(60, 60, 60),
            " Command ",
            Line::from(Span::styled(
                "[i] type command  |  [s] start/pause  |  [n] step  |  [?] help  |  [q] quit",
                Style::default().fg(Color::Rgb(80, 80, 80)),
            )),
        ),
        InputMode::Editing => (
            Color::Rgb(200, 160, 0),
            " Command (Enter=send  Esc=cancel) ",
            Line::from(vec![
                Span::styled("> ", Style::default().fg(Color::Rgb(200, 160, 0))),
                Span::styled(app.input.clone(), Style::default().fg(Color::White)),
                Span::styled("_", Style::default().fg(Color::Rgb(200, 160, 0)).add_modifier(Modifier::SLOW_BLINK)),
            ]),
        ),
    };

    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(border_color));

    let inner = block.inner(area);
    f.render_widget(block, area);
    f.render_widget(Paragraph::new(content_line), inner);
}
