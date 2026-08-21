//! # `scema-tui` — the Scematica Omni console
//!
//! The agent loop as a terminal application: perceive a world, tick the signals that ground
//! a goal, watch branches compete, and read back the records that were sealed.
//!
//! ```console
//! $ scema-tui                  # the current directory
//! $ scema-tui /path/to/project
//! $ scema-tui --once           # one pass as plain text, pipeable
//! $ scema-tui --palette        # the colour roles, to check a terminal
//! ```
//!
//! ## What this binary does not do
//!
//! It does not act, and it does not talk to a daemon. The whole workspace ends at a
//! decision and a record — see the note at the top of `scema-agent` — and a console that
//! quietly gained a write path would invalidate the claim every other crate here makes
//! about being safe to point at a live system. It also holds no token and opens no socket:
//! `scema-omnid` exists for the case where something remote needs to drive the loop, and
//! folding that into a TUI would mean putting the whole pairing story on screen for no gain.
//!
//! ## Terminal lifecycle
//!
//! Raw mode and the alternate screen are entered once and restored on **every** exit path,
//! including a panic. A panic in raw mode leaves the operator with a shell that does not
//! echo, and the fact that the error message scrolled past is the least of their problems —
//! so the hook is installed before the terminal is touched.

mod app;
mod render;
mod theme;
mod view;

use std::io::{self, Stdout, Write};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use clap::Parser;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen};
use crossterm::{execute, ExecutableCommand};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;

use scema_agent::Agent;
use scema_world::Goal;

use crate::app::{App, Focus, Job, Mode, Status, Tab, Worker};
use crate::theme::{Depth, Role, Theme};

/// Frame budget. 100 ms rather than 16: nothing here animates, and a console that wakes
/// sixty times a second to redraw a static table is a console that costs battery for
/// nothing. Input is not polled on this interval — `event::poll` returns as soon as a key
/// arrives, so keystrokes stay immediate.
const TICK: Duration = Duration::from_millis(100);

#[derive(Parser)]
#[command(
    name = "scema-tui",
    version,
    about = "Scematica Omni — the agent loop as a terminal console",
    long_about = None
)]
struct Cli {
    /// The path to perceive.
    #[arg(default_value = ".")]
    path: String,

    /// State directory for decision records and memory.
    #[arg(long, default_value = ".scema")]
    root: PathBuf,

    /// Deep Q* checkpoint (the sniper's `scematica-nn-agent.json`), for trading worlds.
    #[arg(long)]
    dqstar: Option<String>,

    /// Run one pass and print it as plain text. No terminal required.
    #[arg(long)]
    once: bool,

    /// Goal for `--once`. Without one, `--once` observes and prints the world only.
    #[arg(long)]
    goal: Option<String>,

    /// Assert that the goal addresses a counted signal, by id. Repeatable.
    ///
    /// Nothing infers this, here or anywhere else in the runtime. Without it the goal
    /// branch is ungrounded, scores at or below zero, and the agent abstains.
    #[arg(long = "ground")]
    ground: Vec<String>,

    /// Draw no colour at all. `NO_COLOR` in the environment does the same thing.
    #[arg(long)]
    no_color: bool,

    /// Print every colour role and exit, to check what a terminal can carry.
    #[arg(long)]
    palette: bool,

    /// Render one frame into an off-screen buffer and print it as plain text.
    ///
    /// This is the console's own regression test, exposed. A TUI is usually untestable —
    /// its output is a terminal — so layout bugs are found by a human noticing a column is
    /// wrong. Drawing into a `TestBackend` makes a frame into a string that CI can assert
    /// on, which is how `check` below pins that an unmeasured term still renders as an em
    /// dash after somebody rearranges a panel.
    #[arg(long, value_name = "WIDTHxHEIGHT")]
    snapshot: Option<String>,
}

/// Parse `120x40`. Defaults are the smallest size the five-tab layout is designed for.
fn parse_size(spec: &str) -> (u16, u16) {
    let (w, h) = spec.split_once(['x', 'X']).unwrap_or(("120", "40"));
    (
        w.trim().parse().unwrap_or(120),
        h.trim().parse().unwrap_or(40),
    )
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let theme = Theme::new(Depth::detect(cli.no_color));

    if cli.palette {
        return print_palette(theme);
    }

    let agent = Agent::new(cli.root.clone(), cli.dqstar.clone());

    if cli.once {
        return run_once(&cli, &agent);
    }

    if let Some(spec) = cli.snapshot.clone() {
        return run_snapshot(&cli, agent, theme, &spec);
    }

    run_tui(cli, agent, theme)
}

/// One pass, as text. Uses the CLI's own formatter rather than scraping the screen buffer,
/// so a piped run and a rendered run cannot disagree about a number.
fn run_once(cli: &Cli, agent: &Agent) -> Result<()> {
    let world = agent
        .observe(&cli.path)
        .with_context(|| format!("observing {}", cli.path))?;

    let Some(statement) = &cli.goal else {
        println!("{}\n", scema_policy::render::world_header(&world));
        println!("{}", scema_policy::render::signals(&world));
        return Ok(());
    };

    for id in &cli.ground {
        if !world.signals.iter().any(|s| &s.id == id) {
            eprintln!(
                "scema-tui: --ground `{id}` names no signal in this world; it will be ignored."
            );
        }
    }

    let mut goal = Goal::new("goal", statement.trim());
    for id in &cli.ground {
        goal = goal.grounded(id.trim());
    }

    // `--once` never persists. It is the pipeable form of `simulate`, and a record left
    // behind by a shell pipeline would later read as a decision somebody made.
    let mut dry = Agent::new(cli.root.clone(), cli.dqstar.clone());
    dry.persist = false;
    let cycle = dry.cycle_over(world, goal)?;
    print!("{}", render::plain_matrix(&cycle));
    println!(
        "\nRECORD    not written — `--once` is a counterfactual. Would seal as {}.",
        cycle.record.id
    );
    Ok(())
}

fn print_palette(theme: Theme) -> Result<()> {
    // Not a decoration. The point of this flag is that an operator on an unfamiliar
    // terminal can confirm that "measured" and "unmeasured" are actually distinguishable
    // there before trusting a screen full of them.
    //
    // Deliberately not built on a `Terminal`: entering and leaving one emits cursor and
    // screen escapes, and a diagnostic whose own output is full of control sequences is a
    // diagnostic nobody can paste into a bug report.
    let mut out = io::stdout();
    writeln!(out, "depth: {:?}\n", theme.depth)?;
    for role in Role::ALL {
        let style = theme.style(role);
        writeln!(
            out,
            "  {:<16} fg={:<22} modifiers={:?}",
            format!("{role:?}"),
            format!("{:?}", style.fg),
            style.add_modifier
        )?;
    }
    writeln!(
        out,
        "\nIf `Measured` and `Unmeasured` look the same on this terminal, use --no-color:\nthe text still distinguishes them (a number versus an em dash) and the colour is\nnot carrying the message."
    )?;
    Ok(())
}

/// Draw one frame off-screen and print it.
///
/// Everything is done synchronously here rather than through the [`Worker`] — the point is
/// a deterministic frame, and a snapshot that raced a background thread would be a snapshot
/// that differed between runs.
fn run_snapshot(cli: &Cli, agent: Agent, theme: Theme, spec: &str) -> Result<()> {
    use ratatui::backend::TestBackend;

    let (w, h) = parse_size(spec);
    let mut app = App::new(theme, cli.root.clone(), cli.path.clone(), &agent);
    if let Ok(world) = agent.observe(&cli.path) {
        app.world = Some(world);
    }
    for id in &cli.ground {
        app.grounded.insert(id.clone());
    }
    if let Some(g) = &cli.goal {
        app.goal = g.clone();
        app.tab = Tab::Simulate;
        let mut dry = Agent::new(cli.root.clone(), cli.dqstar.clone());
        dry.persist = false;
        if let Some(world) = app.world.clone() {
            if let Ok(c) = dry.cycle_over(world, app.build_goal()) {
                app.cycle = Some(c);
            }
        }
    }

    let mut terminal = Terminal::new(TestBackend::new(w, h))?;
    terminal.draw(|f| render::draw(f, &app))?;
    print!("{}", frame_to_text(terminal.backend().buffer()));
    Ok(())
}

/// A `Buffer` as lines of text, trailing spaces trimmed.
///
/// Symbols only — no styles. A snapshot that embedded escape sequences would change every
/// time the palette moved, and the thing worth pinning is the *layout and the text*, which
/// is exactly what has to survive without colour anyway.
fn frame_to_text(buffer: &ratatui::buffer::Buffer) -> String {
    let area = buffer.area();
    let mut out = String::new();
    for y in 0..area.height {
        let mut line = String::new();
        for x in 0..area.width {
            line.push_str(buffer.get(x, y).symbol());
        }
        out.push_str(line.trim_end());
        out.push('\n');
    }
    out
}

fn run_tui(cli: Cli, agent: Agent, theme: Theme) -> Result<()> {
    // Installed before raw mode is entered, so a panic between here and the guard still
    // lands on a terminal that echoes.
    install_panic_hook();

    let mut terminal = enter()?;
    let agent = Arc::new(agent);
    let mut app = App::new(theme, cli.root.clone(), cli.path.clone(), &agent);
    let worker = Worker::spawn(Arc::clone(&agent), cli.root.clone());

    // Prime every tab that reads from disk, so the console is populated before the operator
    // finds the tab rather than after.
    app.busy = Some("observing");
    worker.send(Job::Observe { path: cli.path.clone() });
    worker.send(Job::LoadRecords);
    worker.send(Job::LoadMemory);

    let result = event_loop(&mut terminal, &mut app, &worker);
    leave(&mut terminal)?;
    result
}

fn event_loop(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    app: &mut App,
    worker: &Worker,
) -> Result<()> {
    let mut last = Instant::now();
    loop {
        terminal.draw(|f| render::draw(f, app))?;

        // Drain everything the worker finished. Draining rather than taking one keeps a
        // burst of three startup jobs from costing three frames.
        while let Ok(done) = worker.rx.try_recv() {
            app.absorb(done);
        }

        let timeout = TICK.saturating_sub(last.elapsed());
        if event::poll(timeout)? {
            match event::read()? {
                Event::Key(key) if key.kind == KeyEventKind::Press => {
                    handle_key(app, worker, key);
                }
                Event::Resize(_, _) => {}
                _ => {}
            }
        }
        if last.elapsed() >= TICK {
            app.tick = app.tick.wrapping_add(1);
            last = Instant::now();
        }
        if app.should_quit {
            return Ok(());
        }
    }
}

fn handle_key(app: &mut App, worker: &Worker, key: KeyEvent) {
    // Text entry first: while a line is being edited every printable key is text, and a
    // console where typing "quit" in a goal field quits is a console nobody types in.
    match app.mode {
        Mode::EditGoal => return edit_line(app, key, true),
        Mode::EditConstraint => return edit_line(app, key, false),
        Mode::ConfirmDecide => return confirm(app, worker, key),
        Mode::Normal => {}
    }

    if key.modifiers.contains(KeyModifiers::CONTROL) && matches!(key.code, KeyCode::Char('c')) {
        app.should_quit = true;
        return;
    }

    if app.help && !matches!(key.code, KeyCode::Char('?')) {
        app.help = false;
        return;
    }

    match key.code {
        KeyCode::Char('q') | KeyCode::Esc => app.should_quit = true,
        KeyCode::Char('?') => app.help = !app.help,

        KeyCode::Char('1') => app.tab = Tab::World,
        KeyCode::Char('2') => app.tab = Tab::Simulate,
        KeyCode::Char('3') => app.tab = Tab::Records,
        KeyCode::Char('4') => app.tab = Tab::Memory,
        KeyCode::Char('5') => app.tab = Tab::Policy,
        KeyCode::Right => app.tab = app.tab.next(),
        KeyCode::Left => app.tab = app.tab.prev(),
        KeyCode::Tab => app.focus = app.focus.flip(),

        KeyCode::Up | KeyCode::Char('k') => move_selection(app, -1),
        KeyCode::Down | KeyCode::Char('j') => move_selection(app, 1),
        KeyCode::PageUp => move_selection(app, -10),
        KeyCode::PageDown => move_selection(app, 10),

        KeyCode::Char('o') => {
            app.busy = Some("observing");
            app.status = Status::Note(format!("observing {}", app.path));
            worker.send(Job::Observe { path: app.path.clone() });
        }
        KeyCode::Char(' ') if app.tab == Tab::World && app.focus == Focus::Right => {
            app.toggle_ground()
        }
        KeyCode::Char('g') => {
            app.mode = Mode::EditGoal;
            app.tab = Tab::Simulate;
        }
        KeyCode::Char('m') => {
            app.mode = Mode::EditConstraint;
            app.constraint_draft.clear();
            app.tab = Tab::Simulate;
        }
        KeyCode::Char('s') | KeyCode::Enter if app.tab != Tab::Records => run_cycle(app, worker, false),
        KeyCode::Enter if app.tab == Tab::Records => open_record(app, worker),
        // Capital D, and never lower case. `d` next to `s` on a keyboard where one of them
        // writes to disk is a footgun; the shift is the smallest possible speed bump and it
        // is backed by a confirmation anyway.
        KeyCode::Char('D') => {
            if let Some(reason) = app.cycle_blocked() {
                app.status = Status::Warn(reason.to_string());
            } else {
                app.mode = Mode::ConfirmDecide;
            }
        }
        KeyCode::Char('r') => {
            app.busy = Some("reading .scema");
            worker.send(Job::LoadRecords);
            worker.send(Job::LoadMemory);
        }
        KeyCode::Char('v') => open_record(app, worker),
        _ => {}
    }
}

fn move_selection(app: &mut App, delta: isize) {
    match (app.tab, app.focus) {
        (Tab::World, Focus::Left) => {
            let n = app.world.as_ref().map(|w| w.objects.len()).unwrap_or(0);
            App::step(&mut app.object_sel, n, delta);
        }
        (Tab::World, Focus::Right) => {
            let n = app.world.as_ref().map(|w| w.signals.len()).unwrap_or(0);
            App::step(&mut app.signal_sel, n, delta);
        }
        (Tab::Simulate, _) => {
            let n = app.cycle.as_ref().map(|c| c.decision.ranked.len()).unwrap_or(0);
            App::step(&mut app.matrix_sel, n, delta);
        }
        (Tab::Records, _) => {
            let n = app.records.len();
            App::step(&mut app.record_sel, n, delta);
        }
        _ => {}
    }
}

fn run_cycle(app: &mut App, worker: &Worker, persist: bool) {
    if let Some(reason) = app.cycle_blocked() {
        app.status = Status::Warn(reason.to_string());
        return;
    }
    let Some(world) = app.world.clone() else { return };
    let goal = app.build_goal();
    app.busy = Some(if persist { "deciding" } else { "simulating" });
    app.status = Status::Note(if persist {
        "running the loop — this will seal a record".into()
    } else {
        "running the loop — nothing will be written".into()
    });
    worker.send(Job::Cycle { world: Box::new(world), goal: Box::new(goal), persist });
}

fn open_record(app: &mut App, worker: &Worker) {
    let Some(row) = app.records.get(app.record_sel) else {
        app.status = Status::Warn("no record selected".into());
        return;
    };
    app.busy = Some("verifying");
    worker.send(Job::OpenRecord(row.id.clone()));
}

fn edit_line(app: &mut App, key: KeyEvent, goal: bool) {
    let buffer = if goal { &mut app.goal } else { &mut app.constraint_draft };
    match key.code {
        KeyCode::Esc => {
            if !goal {
                app.constraint_draft.clear();
            }
            app.mode = Mode::Normal;
        }
        KeyCode::Enter => {
            if !goal {
                let draft = app.constraint_draft.trim().to_string();
                if draft.is_empty() {
                    // An empty subject matches every target by substring and would forbid
                    // the entire matrix. Refused here rather than dropped silently later.
                    app.status =
                        Status::Warn("empty constraint ignored — it would forbid every branch".into());
                } else {
                    app.must_not.push(draft);
                }
                app.constraint_draft.clear();
            }
            app.mode = Mode::Normal;
        }
        KeyCode::Backspace => {
            buffer.pop();
        }
        KeyCode::Char(c) => buffer.push(c),
        _ => {}
    }
}

fn confirm(app: &mut App, worker: &Worker, key: KeyEvent) {
    match key.code {
        KeyCode::Char('y') | KeyCode::Char('Y') => {
            app.mode = Mode::Normal;
            run_cycle(app, worker, true);
            worker.send(Job::LoadRecords);
        }
        _ => {
            app.mode = Mode::Normal;
            app.status = Status::Note("not sealed".into());
        }
    }
}

// ── terminal lifecycle ────────────────────────────────────────────────────────

fn enter() -> Result<Terminal<CrosstermBackend<Stdout>>> {
    enable_raw_mode()?;
    let mut out = io::stdout();
    execute!(out, EnterAlternateScreen, crossterm::cursor::Hide)?;
    let terminal = Terminal::new(CrosstermBackend::new(out))?;
    Ok(terminal)
}

fn leave(terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> Result<()> {
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        crossterm::cursor::Show
    )?;
    terminal.show_cursor()?;
    Ok(())
}

/// Restore the terminal before the default hook prints anything.
///
/// A panic in raw mode leaves a shell that does not echo and does not process newlines. The
/// operator's next problem is not reading the backtrace, it is getting their terminal back,
/// and `reset` is not obvious when you cannot see what you are typing.
fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = disable_raw_mode();
        let _ = io::stdout().execute(LeaveAlternateScreen);
        let _ = io::stdout().execute(crossterm::cursor::Show);
        previous(info);
    }));
}
