//! `scemadex` — the live viewer shipped with the `scemadex-sdk` crate.
//!
//! After `cargo install scemadex-sdk`, run `scemadex` to watch the agentic
//! liquidity layer drive its real pipeline end to end:
//!
//!   intent → solve → **escrow conviction bond** → execute → settle (honor/slash)
//!
//! and seed a peer marketplace with the resulting bonded inferences. It runs
//! fully offline against the lean core (no RPC, no keypair, no solana-sdk), so
//! it doubles as a visual smoke test of the published SDK surface.
//!
//! Keys: `space` pause/resume · `s` single-step · `q`/`Esc` quit.

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
    widgets::{Block, Borders, Gauge, List, ListItem, Paragraph},
    Frame, Terminal,
};

use scemadex_sdk::{
    Address, Amount, BondEngine, BondOutcome, Constraints, Conviction, EscrowBondEngine,
    ExperienceBatch, Fill, InferenceOffer, Intent, LocalPeerMarket, Objective, PeerMarket, Route,
    RouteLeg, Side, Solution, Usdc, Venue,
};

const VERSION: &str = env!("CARGO_PKG_VERSION");

/// A demo token leg: display ticker + a valid base58 mint + decimals.
struct Token {
    sym: &'static str,
    mint: &'static str,
    dec: u8,
}

const TOKENS: &[Token] = &[
    Token { sym: "WSOL", mint: "So11111111111111111111111111111111111111112", dec: 9 },
    Token { sym: "USDC", mint: "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v", dec: 6 },
    Token { sym: "BONK", mint: "DezXAZ8z7PnrnRJjz3wXBoRgixCa6xjnB7YaB1pPB263", dec: 5 },
    Token { sym: "RAY", mint: "4k3Dyjzvzp8eMZWUXbBCjEvwSkkk59S5iCNLY3QrkX6R", dec: 6 },
    Token { sym: "JUP", mint: "JUPyiwrYJFskUPiHa7hkeR8VUtAeFoSYbKedZNsDvCN", dec: 6 },
];

const VENUES: &[Venue] = &[Venue::Raydium, Venue::Orca, Venue::Meteora, Venue::Jupiter];

/// One completed pipeline run, kept for the settlement log.
struct LogEntry {
    pair: String,
    conviction: f64,
    bond_usdc: f64,
    outcome: BondOutcome,
}

struct App {
    engine: EscrowBondEngine,
    market: LocalPeerMarket,
    rng: StdRng,
    log: VecDeque<LogEntry>,
    last: Option<(String, Solution, f64, BondOutcome)>, // pair, solution, fill_ui, outcome
    fees_earned: f64,
    paused: bool,
    ticks: u64,
}

impl App {
    fn new() -> Self {
        Self {
            engine: EscrowBondEngine::with_defaults(),
            market: LocalPeerMarket::new(),
            rng: StdRng::seed_from_u64(0x5CE_4DE),
            log: VecDeque::with_capacity(16),
            last: None,
            fees_earned: 0.0,
            paused: false,
            ticks: 0,
        }
    }

    fn addr(mint: &str) -> Address {
        Address::new(mint).expect("static demo mint is valid base58")
    }

    /// Run one synthetic intent end-to-end through the real bond engine + mesh.
    async fn step(&mut self) {
        self.ticks += 1;

        // 1. Pick a random pair + size and build the intent.
        let i = self.rng.gen_range(0..TOKENS.len());
        let mut j = self.rng.gen_range(0..TOKENS.len());
        while j == i {
            j = self.rng.gen_range(0..TOKENS.len());
        }
        let (tin, tout) = (&TOKENS[i], &TOKENS[j]);
        let amount_ui = self.rng.gen_range(0.5_f64..4.0);
        let amount_in = Amount::new((amount_ui * 10f64.powi(tin.dec as i32)) as u64, tin.dec);
        let intent = Intent {
            input_mint: Self::addr(tin.mint),
            output_mint: Self::addr(tout.mint),
            amount_in,
            side: Side::Sell,
            objective: Objective::Price,
            constraints: Constraints { max_slippage_bps: 150, deadline_unix: 0, max_legs: Some(2) },
        };

        // 2. "Solve": build a 1–2 leg route with a conviction the agent would emit.
        let conviction = self.rng.gen_range(0.30_f64..0.95);
        let expected_raw =
            (amount_ui * self.rng.gen_range(0.8..1.2) * 10f64.powi(tout.dec as i32)) as u64;
        let expected_out = Amount::new(expected_raw.max(1), tout.dec);
        let route = self.build_route(&intent, expected_out);
        let solution = Solution {
            intent_digest: intent.digest(),
            route,
            conviction: Conviction::clamped(conviction),
            rationale: format!("objective=Price; conviction={conviction:.2}"),
        };

        // 3. Escrow a conviction-weighted bond (the defining primitive).
        let bond = match self.engine.escrow(&solution).await {
            Ok(b) => b,
            Err(_) => return,
        };

        // 4. Execute with a noisy fill so bonds actually honor *and* slash.
        let factor = self.rng.gen_range(0.95_f64..1.03);
        let filled_raw = (expected_out.raw as f64 * factor) as u64;
        let fill = Fill { amount_out: Amount::new(filled_raw, tout.dec), executed_unix: 0 };

        // 5. Settle the bond against the realized fill.
        let outcome = self
            .engine
            .settle(&bond, &fill)
            .await
            .unwrap_or(BondOutcome::Slashed);

        // 6. Seed / churn the peer marketplace.
        let fee = self.engine.quote_fee(solution.conviction);
        let _ = self
            .market
            .sell_inference(InferenceOffer {
                solution: solution.clone(),
                price: fee,
                peer_id: format!("peer-{:02}", self.rng.gen_range(0..8)),
            })
            .await;
        self.fees_earned += fee.as_usdc();
        if self.ticks.is_multiple_of(3) {
            let _ = self
                .market
                .sell_experience(ExperienceBatch {
                    peer_id: format!("peer-{:02}", self.rng.gen_range(0..8)),
                    transitions: self.rng.gen_range(32..256),
                    price: Usdc::from_usdc(self.rng.gen_range(0.01..0.20)),
                    payload: Vec::new(),
                })
                .await;
        }
        // Occasionally a buyer consumes the cheapest matching offer.
        if self.ticks.is_multiple_of(4) {
            let _ = self.market.buy_inference(&intent).await;
        }

        let pair = format!("{}→{}", tin.sym, tout.sym);
        if self.log.len() == 12 {
            self.log.pop_back();
        }
        self.log.push_front(LogEntry {
            pair: pair.clone(),
            conviction,
            bond_usdc: bond.amount.as_usdc(),
            outcome,
        });
        self.last = Some((pair, solution, fill.amount_out.ui(), outcome));
    }

    fn build_route(&mut self, intent: &Intent, expected_out: Amount) -> Route {
        let two_leg = self.rng.gen_bool(0.5);
        let legs = if two_leg {
            let a = self.rng.gen_range(3000..7000);
            let half = Amount::new(expected_out.raw / 2, expected_out.decimals);
            vec![
                RouteLeg {
                    venue: VENUES[self.rng.gen_range(0..VENUES.len())],
                    input_mint: intent.input_mint.clone(),
                    output_mint: intent.output_mint.clone(),
                    split_bps: a,
                    expected_out: half,
                },
                RouteLeg {
                    venue: VENUES[self.rng.gen_range(0..VENUES.len())],
                    input_mint: intent.input_mint.clone(),
                    output_mint: intent.output_mint.clone(),
                    split_bps: 10_000 - a,
                    expected_out: half,
                },
            ]
        } else {
            vec![RouteLeg {
                venue: VENUES[self.rng.gen_range(0..VENUES.len())],
                input_mint: intent.input_mint.clone(),
                output_mint: intent.output_mint.clone(),
                split_bps: 10_000,
                expected_out,
            }]
        };
        Route { legs, expected_out }
    }
}

fn main() -> Result<()> {
    // Minimal arg handling — no clap dependency.
    if let Some(arg) = std::env::args().nth(1) {
        match arg.as_str() {
            "-h" | "--help" => {
                print_help();
                return Ok(());
            }
            "-V" | "--version" => {
                println!("scemadex {VERSION}");
                return Ok(());
            }
            other => {
                eprintln!("scemadex: unrecognized argument `{other}` (try --help)");
                std::process::exit(2);
            }
        }
    }

    let rt = tokio::runtime::Runtime::new()?;
    let mut terminal = setup_terminal()?;
    let res = run(&mut terminal, &rt);
    restore_terminal(&mut terminal)?;
    res
}

fn print_help() {
    println!("scemadex {VERSION} — ScemaDEX agentic-liquidity live viewer\n");
    println!("A terminal viewer that drives the lean SDK pipeline offline:");
    println!("  intent -> solve -> conviction bond -> settle (honor/slash) -> peer market\n");
    println!("USAGE:\n  scemadex [OPTIONS]\n");
    println!("OPTIONS:\n  -h, --help       Print this help");
    println!("  -V, --version    Print version\n");
    println!("KEYS (in the viewer):");
    println!("  space  pause/resume    s  single-step    q/Esc  quit");
}

fn run(terminal: &mut Terminal<CrosstermBackend<Stdout>>, rt: &tokio::runtime::Runtime) -> Result<()> {
    let mut app = App::new();
    let tick = Duration::from_millis(850);
    let mut last_tick = Instant::now();

    loop {
        terminal.draw(|f| draw(f, &app))?;

        let timeout = tick.saturating_sub(last_tick.elapsed());
        if event::poll(timeout)? {
            if let Event::Key(key) = event::read()? {
                match key.code {
                    KeyCode::Char('q') | KeyCode::Esc => break,
                    KeyCode::Char(' ') => app.paused = !app.paused,
                    KeyCode::Char('s') => rt.block_on(app.step()),
                    _ => {}
                }
            }
        }
        if last_tick.elapsed() >= tick {
            if !app.paused {
                rt.block_on(app.step());
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
        .constraints([Constraint::Percentage(36), Constraint::Percentage(32), Constraint::Percentage(32)])
        .split(root[1]);

    draw_intent(f, body[0], app);
    draw_ledger(f, body[1], app);
    draw_mesh(f, body[2], app);
    draw_footer(f, root[2], app);
}

fn draw_title(f: &mut Frame, area: Rect) {
    let p = Paragraph::new(Line::from(vec![
        Span::styled("ScemaDEX", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        Span::raw("  — agentic liquidity layer · intent → conviction-bonded routing → settlement"),
    ]))
    .block(Block::default().borders(Borders::ALL));
    f.render_widget(p, area);
}

fn draw_intent(f: &mut Frame, area: Rect, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(3)])
        .split(area);

    let mut lines: Vec<Line> = Vec::new();
    if let Some((pair, sol, fill_ui, outcome)) = &app.last {
        lines.push(Line::from(vec![
            Span::styled("intent  ", Style::default().fg(Color::DarkGray)),
            Span::styled(pair.clone(), Style::default().add_modifier(Modifier::BOLD)),
        ]));
        lines.push(Line::from(format!("digest  {}", sol.intent_digest)));
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled("route", Style::default().fg(Color::Yellow))));
        for leg in &sol.route.legs {
            lines.push(Line::from(format!(
                "  {:>4}.{:02}%  {:?}",
                leg.split_bps / 100,
                leg.split_bps % 100,
                leg.venue
            )));
        }
        lines.push(Line::from(format!("  splits ok: {}", sol.route.splits_valid())));
        lines.push(Line::from(""));
        lines.push(Line::from(format!("expected {:.4}", sol.route.expected_out.ui())));
        lines.push(Line::from(format!("filled   {fill_ui:.4}")));
        let (label, color) = match outcome {
            BondOutcome::Honored => ("HONORED", Color::Green),
            BondOutcome::Slashed => ("SLASHED", Color::Red),
        };
        lines.push(Line::from(vec![
            Span::raw("bond     "),
            Span::styled(label, Style::default().fg(color).add_modifier(Modifier::BOLD)),
        ]));
    } else {
        lines.push(Line::from("warming up…"));
    }

    let p = Paragraph::new(lines)
        .block(Block::default().borders(Borders::ALL).title(" Intent · Route "));
    f.render_widget(p, chunks[0]);

    let conviction = app.last.as_ref().map(|(_, s, _, _)| s.conviction.0).unwrap_or(0.0);
    let gauge = Gauge::default()
        .block(Block::default().borders(Borders::ALL).title(" Conviction "))
        .gauge_style(Style::default().fg(Color::Magenta))
        .ratio(conviction.clamp(0.0, 1.0))
        .label(format!("{:.0}%", conviction * 100.0));
    f.render_widget(gauge, chunks[1]);
}

fn draw_ledger(f: &mut Frame, area: Rect, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(6), Constraint::Min(0)])
        .split(area);

    let l = app.engine.ledger();
    let summary = Paragraph::new(vec![
        Line::from(vec![
            Span::raw("honored "),
            Span::styled(format!("{:>4}", l.honored), Style::default().fg(Color::Green)),
            Span::raw("   slashed "),
            Span::styled(format!("{:>4}", l.slashed), Style::default().fg(Color::Red)),
        ]),
        Line::from(format!("honor rate  {:.1}%", l.honor_rate() * 100.0)),
        Line::from(format!("open bonds  {}", app.engine.open_bonds())),
        Line::from(format!("fees earned {:.4} USDC", app.fees_earned)),
    ])
    .block(Block::default().borders(Borders::ALL).title(" Bond Ledger "));
    f.render_widget(summary, chunks[0]);

    let items: Vec<ListItem> = app
        .log
        .iter()
        .map(|e| {
            let (mark, color) = match e.outcome {
                BondOutcome::Honored => ("✓", Color::Green),
                BondOutcome::Slashed => ("✗", Color::Red),
            };
            ListItem::new(Line::from(vec![
                Span::styled(format!("{mark} "), Style::default().fg(color)),
                Span::raw(format!("{:<10} c={:.2} bond={:.3}", e.pair, e.conviction, e.bond_usdc)),
            ]))
        })
        .collect();
    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title(" Settlements "));
    f.render_widget(list, chunks[1]);
}

fn draw_mesh(f: &mut Frame, area: Rect, app: &App) {
    let lines = vec![
        Line::from(Span::styled(
            "Peer Marketplace",
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(format!("inference offers   {}", app.market.offer_count())),
        Line::from(format!("experience batches {}", app.market.experience_count())),
        Line::from(""),
        Line::from(Span::styled(
            "Agents sell bonded inferences and",
            Style::default().fg(Color::DarkGray),
        )),
        Line::from(Span::styled(
            "learned experience; buyers inherit",
            Style::default().fg(Color::DarkGray),
        )),
        Line::from(Span::styled(
            "the seller's slashable bond.",
            Style::default().fg(Color::DarkGray),
        )),
    ];
    let p = Paragraph::new(lines)
        .block(Block::default().borders(Borders::ALL).title(" Mesh "));
    f.render_widget(p, area);
}

fn draw_footer(f: &mut Frame, area: Rect, app: &App) {
    let state = if app.paused { "PAUSED" } else { "RUNNING" };
    let color = if app.paused { Color::Yellow } else { Color::Green };
    let spans = vec![
        Span::styled(format!(" {state} "), Style::default().fg(color).add_modifier(Modifier::BOLD)),
        Span::styled("· SIM ", Style::default().fg(Color::Cyan)),
        Span::raw(format!("· tick {} ", app.ticks)),
        Span::styled("· [space] pause  [s] step  [q] quit", Style::default().fg(Color::DarkGray)),
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
