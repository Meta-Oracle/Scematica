//! `scema-ddqn` — the live viewer shipped with the `scematica-nn` crate.
//!
//! After `cargo install scematica-nn`, run `scema-ddqn` to watch the pure-Rust
//! Double/Dueling Deep Q* agent **learn a task in real time**. The viewer wires
//! the published [`DQNAgent`] to a small synthetic trading environment — each
//! step the agent sees a pool's features, picks one of five actions, and is
//! rewarded by the realised (simulated) outcome — then trains on its own replay
//! buffer. You can watch epsilon anneal, the loss fall, the Q-values separate,
//! and the policy accuracy climb as the network discovers the optimal policy.
//!
//! No network, no GPU, no ML framework — just the crate's own agent.
//!
//! Keys: `space` pause/resume · `s` single-step · `+`/`-` speed · `q`/`Esc` quit.

use std::collections::VecDeque;
use std::io::{self, Stdout};
use std::time::{Duration, Instant};

use anyhow::Result;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{BarChart, Block, Borders, Gauge, Paragraph, Sparkline},
    Frame, Terminal,
};

use scematica_nn::{DQNAgent, TradeAction, TradeState, ACTION_DIM};

const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Per-action friction so the agent has to learn *when not to act* (HOLD).
const COST: f64 = 6.0;
/// Rewards are scaled down before training to keep them in a sane range.
const REWARD_SCALE: f64 = 10.0;

/// Reward for each action given a realised pnl% (index = `TradeAction::index`).
fn rewards_for(pnl: f64) -> [f64; ACTION_DIM] {
    [
        0.0,                       // Hold — no exposure, no cost
        pnl - COST,                // BuyStandard — 1× exposure
        1.5 * pnl - 1.5 * COST,    // BuyAggressive — 1.5× exposure
        -0.5 * pnl - 0.5 * COST,   // SellPartial — half short
        -pnl - COST,               // SellAll — full short / exit
    ]
}

fn argmax(xs: &[f64]) -> usize {
    xs.iter()
        .enumerate()
        .fold((0usize, f64::NEG_INFINITY), |(bi, bv), (i, &v)| {
            if v > bv { (i, v) } else { (bi, bv) }
        })
        .0
}

/// Sample one synthetic pool + its realised pnl%. The pnl is correlated with the
/// pool's *quality*, which is encoded into the state features the agent sees — so
/// there is a learnable mapping from state to best action.
fn sample_episode(rng: &mut StdRng) -> (TradeState, f64) {
    let lp_burned = rng.gen_bool(0.5);
    let mint_renounced = rng.gen_bool(0.5);
    let pool_score = rng.gen::<f64>(); // [0,1]
    let buy_sell_ratio = rng.gen_range(0.0..5.0);
    let rug_rate = rng.gen::<f64>();

    let quality = 0.25 * if lp_burned { 1.0 } else { 0.0 }
        + 0.20 * if mint_renounced { 1.0 } else { 0.0 }
        + 0.25 * pool_score
        + 0.15 * (buy_sell_ratio / 5.0)
        + 0.15 * (1.0 - rug_rate);

    // pnl%: quality 0→-80%, 0.5→0%, 1→+80%, plus noise.
    let pnl = (quality - 0.5) * 160.0 + rng.gen_range(-12.0..12.0);

    let state = TradeState {
        initial_liquidity_sol: rng.gen_range(5.0..120.0),
        price_change_pct: rng.gen_range(-0.5..2.0),
        volume_5min_sol: rng.gen_range(0.0..50.0),
        buy_sell_ratio,
        lp_burned,
        mint_renounced,
        pool_score_norm: pool_score,
        deployer_rug_rate: rug_rate,
        volatility: rng.gen_range(0.0..1.0),
        ..Default::default()
    };
    (state, pnl)
}

struct App {
    agent: DQNAgent,
    rng: StdRng,
    /// Steps simulated per UI tick (speed control).
    speed: usize,
    paused: bool,
    /// Rolling window of greedy-correct (1.0) / wrong (0.0) decisions.
    acc_window: VecDeque<f64>,
    /// History of policy accuracy (%) for the sparkline.
    acc_history: VecDeque<u64>,
    /// Per-action chosen counts (lifetime).
    action_counts: [u64; ACTION_DIM],
    /// Q-values + decision for the most recent state shown in the centre panel.
    last_q: Vec<f64>,
    last_greedy: usize,
    last_optimal: usize,
    last_reward: f64,
}

impl App {
    fn new() -> Self {
        Self {
            agent: DQNAgent::new(),
            rng: StdRng::seed_from_u64(0xDD9_42),
            speed: 8,
            paused: false,
            acc_window: VecDeque::with_capacity(512),
            acc_history: VecDeque::with_capacity(256),
            action_counts: [0; ACTION_DIM],
            last_q: vec![0.0; ACTION_DIM],
            last_greedy: 0,
            last_optimal: 0,
            last_reward: 0.0,
        }
    }

    /// One environment step + one training step.
    fn step_once(&mut self) {
        let (state, pnl) = sample_episode(&mut self.rng);
        let rewards = rewards_for(pnl);
        let optimal = argmax(&rewards);

        // The agent picks epsilon-greedily; we also read its *greedy* choice +
        // Q-values to measure policy accuracy independent of exploration.
        let (greedy_action, q) = self.agent.greedy_action(&state);
        let action = self.agent.select_action(&state);
        let reward = rewards[action.index()];

        // Single-step terminal transition, then learn.
        let terminal = TradeState::default();
        self.agent
            .observe(state, action, reward / REWARD_SCALE, terminal, true);
        let _ = self.agent.train_step();

        self.action_counts[action.index()] += 1;
        let correct = if greedy_action.index() == optimal { 1.0 } else { 0.0 };
        if self.acc_window.len() == 400 {
            self.acc_window.pop_front();
        }
        self.acc_window.push_back(correct);

        self.last_q = q;
        self.last_greedy = greedy_action.index();
        self.last_optimal = optimal;
        self.last_reward = reward;
    }

    /// Advance one UI tick = `speed` environment steps.
    fn tick(&mut self) {
        for _ in 0..self.speed {
            self.step_once();
        }
        let acc = self.accuracy();
        if self.acc_history.len() == 200 {
            self.acc_history.pop_front();
        }
        self.acc_history.push_back((acc * 100.0).round() as u64);
    }

    fn accuracy(&self) -> f64 {
        if self.acc_window.is_empty() {
            return 0.0;
        }
        self.acc_window.iter().sum::<f64>() / self.acc_window.len() as f64
    }
}

fn main() -> Result<()> {
    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "-h" | "--help" => {
                print_help();
                return Ok(());
            }
            "-V" | "--version" => {
                println!("scema-ddqn {VERSION}");
                return Ok(());
            }
            other => {
                eprintln!("scema-ddqn: unrecognized argument `{other}` (try --help)");
                std::process::exit(2);
            }
        }
    }

    let mut terminal = setup_terminal()?;
    let res = run(&mut terminal);
    restore_terminal(&mut terminal)?;
    res
}

fn print_help() {
    println!("scema-ddqn {VERSION} — Scematica Deep Q* live training viewer\n");
    println!("Watches the pure-Rust Double/Dueling DQN agent learn a synthetic");
    println!("trading task in real time: epsilon anneal, loss, Q-values, accuracy.\n");
    println!("USAGE:\n  scema-ddqn [OPTIONS]\n");
    println!("OPTIONS:\n  -h, --help       Print this help");
    println!("  -V, --version    Print version\n");
    println!("KEYS (in the viewer):");
    println!("  space  pause/resume   s  single-step   +/-  speed   q/Esc  quit");
}

fn run(terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> Result<()> {
    let mut app = App::new();
    let tick = Duration::from_millis(120);
    let mut last_tick = Instant::now();

    loop {
        terminal.draw(|f| draw(f, &app))?;

        let timeout = tick.saturating_sub(last_tick.elapsed());
        if event::poll(timeout)? {
            if let Event::Key(key) = event::read()? {
                match key.code {
                    KeyCode::Char('q') | KeyCode::Esc => break,
                    KeyCode::Char(' ') => app.paused = !app.paused,
                    KeyCode::Char('s') => app.tick(),
                    KeyCode::Char('+') | KeyCode::Char('=') => {
                        app.speed = (app.speed * 2).min(256)
                    }
                    KeyCode::Char('-') | KeyCode::Char('_') => {
                        app.speed = (app.speed / 2).max(1)
                    }
                    _ => {}
                }
            }
        }
        if last_tick.elapsed() >= tick {
            if !app.paused {
                app.tick();
            }
            last_tick = Instant::now();
        }
    }
    Ok(())
}

fn draw(f: &mut Frame, app: &App) {
    let root = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(0), Constraint::Length(3)])
        .split(f.size());

    draw_title(f, root[0]);

    let body = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(34), Constraint::Percentage(34), Constraint::Percentage(32)])
        .split(root[1]);

    draw_stats(f, body[0], app);
    draw_qvalues(f, body[1], app);
    draw_right(f, body[2], app);
    draw_footer(f, root[2], app);
}

fn draw_title(f: &mut Frame, area: Rect) {
    let p = Paragraph::new(Line::from(vec![
        Span::styled("Scematica DQ*", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        Span::raw("  — Double/Dueling Deep Q* agent learning a synthetic trading task, live"),
    ]))
    .block(Block::default().borders(Borders::ALL));
    f.render_widget(p, area);
}

fn draw_stats(f: &mut Frame, area: Rect, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(3), Constraint::Length(3)])
        .split(area);

    let s = app.agent.stats();
    let lines = vec![
        Line::from(vec![
            Span::styled("env steps   ", Style::default().fg(Color::DarkGray)),
            Span::styled(format!("{}", s.step_count), Style::default().add_modifier(Modifier::BOLD)),
        ]),
        Line::from(format!("train steps {}", s.train_steps)),
        Line::from(format!("replay size {}", s.replay_size)),
        Line::from(format!("target syncs {}", s.target_updates)),
        Line::from(format!("avg loss    {:.4}", s.avg_loss)),
        Line::from(format!("total reward {:.1}", s.total_reward)),
        Line::from(vec![
            Span::raw("ready to advise "),
            if s.ready_to_advise {
                Span::styled("YES", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD))
            } else {
                Span::styled("no", Style::default().fg(Color::DarkGray))
            },
        ]),
    ];
    let p = Paragraph::new(lines)
        .block(Block::default().borders(Borders::ALL).title(" Training "));
    f.render_widget(p, chunks[0]);

    let eps = Gauge::default()
        .block(Block::default().borders(Borders::ALL).title(" Exploration ε "))
        .gauge_style(Style::default().fg(Color::Yellow))
        .ratio(s.epsilon.clamp(0.0, 1.0))
        .label(format!("{:.3}", s.epsilon));
    f.render_widget(eps, chunks[1]);

    let acc = app.accuracy();
    let acc_color = if acc > 0.8 { Color::Green } else if acc > 0.5 { Color::Yellow } else { Color::Red };
    let acc_gauge = Gauge::default()
        .block(Block::default().borders(Borders::ALL).title(" Policy accuracy "))
        .gauge_style(Style::default().fg(acc_color))
        .ratio(acc.clamp(0.0, 1.0))
        .label(format!("{:.0}%", acc * 100.0));
    f.render_widget(acc_gauge, chunks[2]);
}

fn draw_qvalues(f: &mut Frame, area: Rect, app: &App) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Q-values · latest state ");
    let inner = block.inner(area);
    f.render_widget(block, area);

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints(
            std::iter::repeat(Constraint::Length(2))
                .take(ACTION_DIM)
                .chain(std::iter::once(Constraint::Min(0)))
                .collect::<Vec<_>>(),
        )
        .split(inner);

    let min_q = app.last_q.iter().cloned().fold(f64::INFINITY, f64::min);
    let max_q = app.last_q.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let span = (max_q - min_q).max(1e-6);

    for i in 0..ACTION_DIM {
        let q = app.last_q.get(i).copied().unwrap_or(0.0);
        let ratio = ((q - min_q) / span).clamp(0.0, 1.0);
        let label = TradeAction::from_index(i).label();
        let mut marks = String::new();
        if i == app.last_greedy {
            marks.push_str(" ◄greedy");
        }
        if i == app.last_optimal {
            marks.push_str(" ★optimal");
        }
        let color = if i == app.last_greedy {
            Color::Cyan
        } else if i == app.last_optimal {
            Color::Green
        } else {
            Color::DarkGray
        };
        let g = Gauge::default()
            .block(Block::default().title(Span::styled(
                format!("{label}{marks}"),
                Style::default().fg(color),
            )))
            .gauge_style(Style::default().fg(color))
            .ratio(ratio)
            .label(format!("{q:+.3}"));
        f.render_widget(g, rows[i]);
    }

    let hit = app.last_greedy == app.last_optimal;
    let verdict = if hit {
        Span::styled("greedy = optimal ✓", Style::default().fg(Color::Green))
    } else {
        Span::styled("greedy ≠ optimal ✗", Style::default().fg(Color::Red))
    };
    let p = Paragraph::new(vec![
        Line::from(format!("last reward {:+.2}", app.last_reward)),
        Line::from(verdict),
    ]);
    f.render_widget(p, rows[ACTION_DIM]);
}

fn draw_right(f: &mut Frame, area: Rect, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
        .split(area);

    let labels: [&str; ACTION_DIM] = ["HOLD", "BUY", "BUYagg", "SELLp", "SELLall"];
    let data: Vec<(&str, u64)> = labels
        .iter()
        .enumerate()
        .map(|(i, l)| (*l, app.action_counts[i]))
        .collect();
    let chart = BarChart::default()
        .block(Block::default().borders(Borders::ALL).title(" Actions chosen "))
        .data(&data)
        .bar_width(6)
        .bar_gap(1)
        .bar_style(Style::default().fg(Color::Magenta))
        .value_style(Style::default().fg(Color::Black).bg(Color::Magenta))
        .label_style(Style::default().fg(Color::Gray));
    f.render_widget(chart, chunks[0]);

    let hist: Vec<u64> = app.acc_history.iter().copied().collect();
    let spark = Sparkline::default()
        .block(Block::default().borders(Borders::ALL).title(" Accuracy trend (%) "))
        .data(&hist)
        .max(100)
        .style(Style::default().fg(Color::Green));
    f.render_widget(spark, chunks[1]);
}

fn draw_footer(f: &mut Frame, area: Rect, app: &App) {
    let state = if app.paused { "PAUSED" } else { "RUNNING" };
    let color = if app.paused { Color::Yellow } else { Color::Green };
    let spans = vec![
        Span::styled(format!(" {state} "), Style::default().fg(color).add_modifier(Modifier::BOLD)),
        Span::styled(format!("· {}× speed ", app.speed), Style::default().fg(Color::Cyan)),
        Span::styled(
            "· [space] pause  [s] step  [+/-] speed  [q] quit",
            Style::default().fg(Color::DarkGray),
        ),
    ];
    let p = Paragraph::new(Line::from(spans)).block(Block::default().borders(Borders::ALL));
    f.render_widget(p, area);
}

fn setup_terminal() -> Result<Terminal<CrosstermBackend<Stdout>>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    Ok(Terminal::new(CrosstermBackend::new(stdout))?)
}

fn restore_terminal(terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> Result<()> {
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen, DisableMouseCapture)?;
    terminal.show_cursor()?;
    Ok(())
}
