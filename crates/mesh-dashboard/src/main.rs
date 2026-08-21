//! `mesh-dashboard` — Scematica Mesh as a live terminal graph.
//!
//! ```text
//!   mesh-dashboard                     # watch the current directory
//!   mesh-dashboard /path/to/botdir     # watch somewhere else
//!   mesh-dashboard --interval 2        # re-collect every 2s
//!   mesh-dashboard --once              # render one frame as text and exit
//!   mesh-dashboard --json              # the raw mesh, for piping
//! ```
//!
//! Reads the state files the sniper leaves in a working directory (see the File-Based IPC
//! table in CLAUDE.md) and draws the decision-making units, what each last decided, and —
//! first — whether each can be seen at all.
//!
//! **Read-only.** It opens files, writes nothing, and takes no locks, so it is safe to run
//! against a live bot. That is a property of `scematica_mesh::Collector`, and nothing here
//! is allowed to add a write path.
//!
//! ## Why `--once` exists
//!
//! The same canvas the TUI blits is rendered to plain rows and printed. So the answer to
//! "why did nothing trade" is pipeable, greppable, and pasteable into an issue — and it is
//! drawn by the code the unit tests already cover, rather than by a second text-mode
//! formatter that could disagree with the picture.

mod canvas;
mod render;
mod view;
mod vortex;

use std::io::{self, Stdout, Write};
use std::path::Path;
use std::time::{Duration, Instant};

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;

use scematica_mesh::{Collector, Mesh};

use crate::render::View;
use crate::view::{GraphLayout, Trace};
use crate::vortex::Camera;

/// Rows the chrome takes: header 2 + diagnosis 3 + panels 11 + footer 1.
/// Mirrors the constraints in `render::draw`; used to keep the selected node scrolled
/// into view without threading the drawn `Rect` back out of the render pass.
const CHROME_ROWS: u16 = 2 + 3 + 11 + 1;

const DEFAULT_INTERVAL_SECS: u64 = 4;

struct Args {
    root: String,
    interval: u64,
    once: bool,
    json: bool,
    /// Emit the mesh as a Scematica Omni `WorldState` and exit.
    world: bool,
    view: View,
}

fn parse_args() -> Result<Option<Args>> {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let mut root = ".".to_string();
    let mut interval = DEFAULT_INTERVAL_SECS;
    let mut once = false;
    let mut json = false;
    let mut world = false;
    let mut view = View::Graph;
    let mut saw_root = false;

    let mut i = 0;
    while i < argv.len() {
        match argv[i].as_str() {
            "-h" | "--help" => {
                print_help();
                return Ok(None);
            }
            "--once" => once = true,
            "--json" => json = true,
            "--world" => world = true,
            "--view" => {
                i += 1;
                let v = argv.get(i).ok_or_else(|| anyhow::anyhow!("--view needs a value"))?;
                view = View::parse(v)
                    .ok_or_else(|| anyhow::anyhow!("unknown view `{v}` (try graph|tornado|coherence|roster)"))?;
            }
            "--interval" => {
                i += 1;
                let v = argv.get(i).ok_or_else(|| anyhow::anyhow!("--interval needs a value"))?;
                interval = v.parse().map_err(|_| anyhow::anyhow!("--interval: not a number: {v}"))?;
                if interval == 0 {
                    anyhow::bail!("--interval must be at least 1 second");
                }
            }
            a if a.starts_with("--") => anyhow::bail!("unknown flag `{a}` (try --help)"),
            a => {
                if saw_root {
                    anyhow::bail!("unexpected extra argument `{a}`");
                }
                root = a.to_string();
                saw_root = true;
            }
        }
        i += 1;
    }

    if !Path::new(&root).is_dir() {
        anyhow::bail!("`{root}` is not a directory");
    }

    Ok(Some(Args { root, interval, once, json, world, view }))
}

fn print_help() {
    println!("mesh-dashboard — Scematica Mesh as a live terminal graph\n");
    println!("USAGE:\n  mesh-dashboard [DIR] [OPTIONS]\n");
    println!("ARGS:\n  DIR                 bot working directory to read (default: .)\n");
    println!("OPTIONS:");
    println!("  --interval <SECS>   re-collect every SECS seconds (default: {DEFAULT_INTERVAL_SECS})");
    println!("  --once              render one frame as plain text and exit");
    println!("  --view <VIEW>       start in graph|tornado|coherence|roster (default: graph)");
    println!("  --json              print the raw mesh as JSON and exit");
    println!("  --world             print the mesh as a Scematica Omni WorldState and exit");
    println!("  -h, --help          this help\n");
    println!("KEYS (interactive):");
    println!("  1/2/3/4 or Tab       switch view (graph / tornado / coherence / roster)");
    println!("  q / Esc              quit            r    re-collect now");
    println!("  ↑↓←→ or hjkl         move selection  t    trace to/from the selection");
    println!("  ← → (in tornado)     rotate the vortex");
    println!("  Tab / Shift-Tab      cycle units     g    expand the gate's terms");
    println!("  c                    clear selection < >  scroll the graph horizontally\n");
    println!("Reads only. It never writes a file or takes a lock, so it is safe against a");
    println!("live bot. A dark node means no source on disk — unseen, not idle.
");
    println!("REASONING OVER IT:");
    println!("  mesh-dashboard --world | scema simulate \"get it trading again\" --path -");
    println!("  Turns this topology into a world the omni agent can rank branches against.");
    println!("  Every signal it emits is a count; an unseen unit becomes a blind spot, never");
    println!("  a zero. See scematica_mesh::omni.");
}

fn main() -> Result<()> {
    let Some(args) = parse_args()? else { return Ok(()) };

    let mesh = Collector::new(&args.root).collect();

    if args.json {
        println!("{}", serde_json::to_string_pretty(&mesh)?);
        return Ok(());
    }
    if args.world {
        // The same topology, in Scematica Omni's vocabulary, so a live bot becomes an
        // environment the agent loop can reason over. `scematica_mesh::omni` owns the
        // mapping; this is a pipe, not a second implementation.
        println!("{}", scematica_mesh::omni::world_string_now(&mesh, &args.root));
        return Ok(());
    }
    if args.once {
        print_once(&mesh, &args.root, args.view);
        return Ok(());
    }

    run_tui(args, mesh)
}

/// One static frame, as text. Same canvas the TUI draws — so the answer to
/// "why did nothing trade" is pipeable, greppable, and drawn by the code the unit
/// tests already cover. The tornado renders as a plain-text depth proxy when asked.
fn print_once(mesh: &Mesh, root: &str, view: View) {
    let layout = view::layout(mesh);
    let c = &mesh.cognition;

    println!("SCEMATICA MESH   {}   [{} view]", mesh.generated_at, view.as_str());
    println!("root             {root}");
    println!("visibility       {}", view::visibility_label(mesh));
    println!("diagnosis        {}", mesh.summary.diagnosis);
    println!();

    match view {
        View::Graph | View::Coherence | View::Roster => {
            let canvas = render::paint_graph(mesh, &layout, None, None);
            for y in 0..canvas.height {
                println!("{}", canvas.row(y).trim_end());
            }
        }
        View::Tornado => {
            let camera = Camera::default();
            let mut grid = crate::vortex::DepthGrid::new(120, 40);
            grid.frame(0, 0, 120, 40, "ACTIVE NODE TORNADO", crate::vortex::FRAME);
            let inner = (1, 1, 118, 38);
            let clip = grid.set_clip(inner.0, inner.1, inner.2, inner.3);
            let vortex = crate::vortex::Vortex::from_mesh(mesh);
            vortex.draw_fitted(&mut grid, &camera, 60.0, 20.0, inner.2, inner.3, 1.2);
            grid.restore_clip(clip);
            println!("{}", grid.render_plain());
        }
    }

    println!();
    println!(
        "GATE §32   Ψ {:.3}  {:?}   C {:.3}  K {:.3}  R {:.3}  Ω {}   ({:.0}% of terms measured)",
        c.psi,
        c.verdict,
        c.confidence,
        c.coherence.value,
        c.risk.value,
        c.omega.map(|o| format!("{o:.3}")).unwrap_or_else(|| "—".into()),
        c.measured_fraction * 100.0
    );
    println!("           {}", c.reading);

    let problems = mesh.validate();
    if !problems.is_empty() {
        println!("\nSTRUCTURAL PROBLEMS:");
        for p in problems {
            println!("  {p}");
        }
    }
}

// ── TUI ──────────────────────────────────────────────────────────────────────

struct App {
    root: String,
    mesh: Mesh,
    layout: GraphLayout,
    interval: Duration,
    last_collect: Instant,
    selected: Option<String>,
    tracing: bool,
    show_terms: bool,
    scroll_x: u16,
    scroll_y: u16,
    problems: Option<String>,
    view: View,
    camera: Camera,
    started: Instant,
}

impl App {
    fn new(root: String, mesh: Mesh, interval: Duration, view: View) -> Self {
        let layout = view::layout(&mesh);
        let problems = structural_problems(&mesh);
        App {
            root,
            mesh,
            layout,
            interval,
            last_collect: Instant::now(),
            selected: None,
            tracing: false,
            show_terms: false,
            scroll_x: 0,
            scroll_y: 0,
            problems,
            view,
            camera: Camera::default(),
            started: Instant::now(),
        }
    }

    fn recollect(&mut self) {
        // The directory can disappear under a long-running session (a bot moved, a mount
        // dropped). Say so rather than silently redrawing the last good picture as if it
        // were current — that is the same error the whole tool exists to prevent.
        if !Path::new(&self.root).is_dir() {
            self.problems = Some(format!("`{}` is no longer a directory", self.root));
            self.last_collect = Instant::now();
            return;
        }
        self.mesh = Collector::new(&self.root).collect();
        self.layout = view::layout(&self.mesh);
        self.problems = structural_problems(&self.mesh);
        self.last_collect = Instant::now();
        // A node can vanish between polls; a selection pointing at nothing would render a
        // blank panel with no explanation.
        if let Some(id) = &self.selected {
            if !self.mesh.nodes.iter().any(|n| &n.id == id) {
                self.selected = None;
                self.tracing = false;
            }
        }
    }

    fn trace(&self) -> Option<Trace> {
        if !self.tracing {
            return None;
        }
        self.selected.as_ref().map(|id| view::trace(&self.mesh, id))
    }

    /// Move the selection by one column (`dx`) or one row within a column (`dy`).
    fn move_selection(&mut self, dx: i32, dy: i32) {
        if self.layout.placed.is_empty() {
            return;
        }
        let Some(cur) = self.selected.clone().and_then(|id| self.layout.find(&id).cloned()) else {
            // Nothing selected yet: start at the top of the leftmost column, which is
            // where the pipeline starts.
            let first = self
                .layout
                .placed
                .iter()
                .min_by_key(|p| (p.x, p.y))
                .map(|p| p.id.clone());
            self.selected = first;
            return;
        };

        // Columns are identified by layer, not by pixel x. Same thing today, but the
        // layer is the meaning and x is a consequence of it — grouping on the coordinate
        // would quietly break the moment a column is ever offset.
        let next = if dy != 0 {
            let mut same: Vec<_> =
                self.layout.placed.iter().filter(|p| p.layer == cur.layer).collect();
            same.sort_by_key(|p| p.y);
            let idx = same.iter().position(|p| p.id == cur.id).unwrap_or(0) as i32;
            let target = (idx + dy).clamp(0, same.len() as i32 - 1) as usize;
            same.get(target).map(|p| p.id.clone())
        } else {
            // Nearest node vertically in the next column that actually contains nodes.
            // Skipping empty layers matters: with no peers collected, Mesh (layer 5) does
            // not exist and one press of → from Execution must not dead-end.
            let mut layers: Vec<u8> = self.layout.placed.iter().map(|p| p.layer).collect();
            layers.sort_unstable();
            layers.dedup();
            let ci = layers.iter().position(|l| *l == cur.layer).unwrap_or(0) as i32;
            let ti = (ci + dx).clamp(0, layers.len() as i32 - 1) as usize;
            let target_layer = layers[ti];
            self.layout
                .placed
                .iter()
                .filter(|p| p.layer == target_layer)
                .min_by_key(|p| p.cy().abs_diff(cur.cy()))
                .map(|p| p.id.clone())
        };

        if let Some(id) = next {
            self.selected = Some(id);
        }
    }

    fn cycle(&mut self, forward: bool) {
        if self.layout.placed.is_empty() {
            return;
        }
        let n = self.layout.placed.len();
        let idx = self
            .selected
            .as_ref()
            .and_then(|id| self.layout.placed.iter().position(|p| &p.id == id));
        let next = match idx {
            None => 0,
            Some(i) if forward => (i + 1) % n,
            Some(i) => (i + n - 1) % n,
        };
        self.selected = Some(self.layout.placed[next].id.clone());
    }

    /// Keep the selected box inside the viewport after a move.
    fn follow_selection(&mut self, view_w: u16, view_h: u16) {
        let Some(p) = self.selected.as_ref().and_then(|id| self.layout.find(id)) else { return };
        let (x, y, w, h) = (p.x, p.y, p.w, p.h);

        if x < self.scroll_x {
            self.scroll_x = x;
        } else if view_w > 0 && x + w > self.scroll_x + view_w {
            self.scroll_x = (x + w).saturating_sub(view_w);
        }
        if y < self.scroll_y {
            self.scroll_y = y;
        } else if view_h > 0 && y + h > self.scroll_y + view_h {
            self.scroll_y = (y + h).saturating_sub(view_h);
        }
    }

    fn clamp_scroll(&mut self, view_w: u16, view_h: u16) {
        self.scroll_x = self.scroll_x.min(self.layout.width.saturating_sub(view_w.max(1)));
        self.scroll_y = self.scroll_y.min(self.layout.height.saturating_sub(view_h.max(1)));
    }
}

fn structural_problems(mesh: &Mesh) -> Option<String> {
    let problems = mesh.validate();
    if problems.is_empty() {
        None
    } else {
        Some(problems.join("; "))
    }
}

/// Restores the terminal on the way out, including on a panic.
///
/// Leaving someone's terminal in raw mode on the alternate screen is breaking it, not
/// using it — and a panic inside a draw call is exactly when that is most likely.
struct TerminalGuard;

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = restore_terminal();
    }
}

fn restore_terminal() -> Result<()> {
    disable_raw_mode()?;
    let mut out = io::stdout();
    execute!(out, LeaveAlternateScreen)?;
    out.flush()?;
    Ok(())
}

fn setup_terminal() -> Result<Terminal<CrosstermBackend<Stdout>>> {
    enable_raw_mode()?;
    let mut out = io::stdout();
    execute!(out, EnterAlternateScreen)?;
    Ok(Terminal::new(CrosstermBackend::new(out))?)
}

fn run_tui(args: Args, mesh: Mesh) -> Result<()> {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = restore_terminal();
        default_hook(info);
    }));

    let mut terminal = setup_terminal()?;
    let _guard = TerminalGuard;

    let mut app = App::new(args.root.clone(), mesh, Duration::from_secs(args.interval), args.view);
    let tick = Duration::from_millis(120);

    loop {
        let size = terminal.size()?;
        // Graph viewport: the frame minus the chrome, minus the graph block's own borders.
        let view_w = size.width.saturating_sub(2);
        let view_h = size.height.saturating_sub(CHROME_ROWS).saturating_sub(2);
        app.clamp_scroll(view_w, view_h);

        let traced = app.trace();
        let canvas = render::paint_graph(&app.mesh, &app.layout, app.selected.as_deref(), traced.as_ref());
        let time = app.started.elapsed().as_secs_f64();

        terminal.draw(|f| {
            render::draw(
                f,
                &render::Screen {
                    mesh: &app.mesh,
                    layout: &app.layout,
                    canvas: &canvas,
                    root: &app.root,
                    selected: app.selected.as_deref(),
                    tracing: app.tracing,
                    show_terms: app.show_terms,
                    scroll_x: app.scroll_x,
                    scroll_y: app.scroll_y,
                    interval_secs: args.interval,
                    last_error: app.problems.as_deref(),
                    view: app.view,
                    time,
                    camera: &app.camera,
                },
            )
        })?;

        if event::poll(tick)? {
            if let Event::Key(key) = event::read()? {
                // Windows reports both press and release; acting on both double-steps
                // every keystroke.
                if key.kind != KeyEventKind::Press {
                    continue;
                }
                if handle_key(&mut app, key, view_w, view_h, time) {
                    break;
                }
            }
        }

        if app.last_collect.elapsed() >= app.interval {
            app.recollect();
        }
    }

    Ok(())
}

/// Returns true when the app should quit.
fn handle_key(app: &mut App, key: KeyEvent, view_w: u16, view_h: u16, _time: f64) -> bool {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    match key.code {
        KeyCode::Char('q') | KeyCode::Esc => return true,
        KeyCode::Char('c') if ctrl => return true,
        // View switching: 1-4 and the number-row keys both work.
        KeyCode::Char('1') | KeyCode::F(1) => app.view = View::Graph,
        KeyCode::Char('2') | KeyCode::F(2) => app.view = View::Tornado,
        KeyCode::Char('3') | KeyCode::F(3) => app.view = View::Coherence,
        KeyCode::Char('4') | KeyCode::F(4) => app.view = View::Roster,
        KeyCode::Char('c') => {
            app.selected = None;
            app.tracing = false;
        }
        KeyCode::Char('r') => app.recollect(),
        KeyCode::Char('g') => app.show_terms = !app.show_terms,
        KeyCode::Char('t') | KeyCode::Enter => {
            // Tracing needs something to trace from.
            if app.selected.is_none() {
                app.move_selection(0, 0);
            }
            app.tracing = !app.tracing;
        }
        KeyCode::Tab => {
            app.cycle(true);
            app.follow_selection(view_w, view_h);
        }
        KeyCode::BackTab => {
            app.cycle(false);
            app.follow_selection(view_w, view_h);
        }
        KeyCode::Up | KeyCode::Char('k') => {
            app.move_selection(0, -1);
            app.follow_selection(view_w, view_h);
        }
        KeyCode::Down | KeyCode::Char('j') => {
            app.move_selection(0, 1);
            app.follow_selection(view_w, view_h);
        }
        KeyCode::Left | KeyCode::Char('h') => {
            if app.view == View::Tornado {
                // Rotate the vortex about its vertical axis.
                app.camera.yaw += 0.12;
            } else {
                app.move_selection(-1, 0);
                app.follow_selection(view_w, view_h);
            }
        }
        KeyCode::Right | KeyCode::Char('l') => {
            if app.view == View::Tornado {
                app.camera.yaw -= 0.12;
            } else {
                app.move_selection(1, 0);
                app.follow_selection(view_w, view_h);
            }
        }
        KeyCode::Char('[') => app.camera.pitch = (app.camera.pitch + 0.08).min(1.4),
        KeyCode::Char(']') => app.camera.pitch = (app.camera.pitch - 0.08).max(-0.2),
        KeyCode::Char('<') | KeyCode::Char(',') => app.scroll_x = app.scroll_x.saturating_sub(6),
        KeyCode::Char('>') | KeyCode::Char('.') => app.scroll_x = app.scroll_x.saturating_add(6),
        KeyCode::PageUp => app.scroll_y = app.scroll_y.saturating_sub(view_h.max(1)),
        KeyCode::PageDown => app.scroll_y = app.scroll_y.saturating_add(view_h.max(1)),
        KeyCode::Home => {
            app.scroll_x = 0;
            app.scroll_y = 0;
        }
        _ => {}
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vortex::Camera;
    use scematica_mesh::{Edge, Node, NodeKind, Provenance, Verdict};

    fn node(id: &str, kind: NodeKind) -> Node {
        node_with(id, kind, Provenance::Live { age_secs: 1 }, Verdict::Pass)
    }

    fn node_with(id: &str, kind: NodeKind, p: Provenance, v: Verdict) -> Node {
        Node {
            id: id.into(),
            kind,
            label: id.into(),
            blurb: String::new(),
            provenance: p,
            verdict: v,
            activity: None,
            detail: vec![],
            reason: None,
        }
    }

    fn app() -> App {
        let mesh = Mesh::new(
            vec![
                node("l", NodeKind::Listener),
                node("f1", NodeKind::Filter),
                node("f2", NodeKind::Filter),
                node("x", NodeKind::Executor),
            ],
            vec![Edge::signal("l", "f1"), Edge::signal("f1", "x")],
            "t".into(),
        );
        App::new(".".into(), mesh, Duration::from_secs(4), View::Graph)
    }

    #[test]
    fn the_first_move_selects_the_start_of_the_pipeline() {
        let mut a = app();
        assert!(a.selected.is_none());
        a.move_selection(0, 1);
        assert_eq!(a.selected.as_deref(), Some("l"), "selection should start at Ingest");
    }

    #[test]
    fn vertical_movement_stays_inside_a_column() {
        let mut a = app();
        a.selected = Some("f1".into());
        a.move_selection(0, 1);
        assert_eq!(a.selected.as_deref(), Some("f2"));
        // Clamped at the bottom rather than wrapping into another column.
        a.move_selection(0, 1);
        assert_eq!(a.selected.as_deref(), Some("f2"));
        a.move_selection(0, -1);
        assert_eq!(a.selected.as_deref(), Some("f1"));
    }

    #[test]
    fn horizontal_movement_crosses_columns() {
        let mut a = app();
        a.selected = Some("l".into());
        a.move_selection(1, 0);
        assert!(matches!(a.selected.as_deref(), Some("f1") | Some("f2")));
        a.move_selection(-1, 0);
        assert_eq!(a.selected.as_deref(), Some("l"));
    }

    #[test]
    fn cycling_wraps_in_both_directions() {
        let mut a = app();
        a.cycle(true);
        let first = a.selected.clone();
        for _ in 0..a.layout.placed.len() {
            a.cycle(true);
        }
        assert_eq!(a.selected, first, "a full cycle returns to where it started");
        a.cycle(false);
        assert_ne!(a.selected, first);
    }

    /// Tracing with nothing selected would trace nothing and silently look broken.
    #[test]
    fn tracing_selects_something_first() {
        let mut a = app();
        assert!(!handle_key(&mut a, KeyEvent::from(KeyCode::Char('t')), 80, 20, 0.0));
        assert!(a.tracing);
        assert!(a.selected.is_some());
        assert!(a.trace().is_some());
    }

    #[test]
    fn clearing_drops_both_the_selection_and_the_trace() {
        let mut a = app();
        a.selected = Some("l".into());
        a.tracing = true;
        handle_key(&mut a, KeyEvent::from(KeyCode::Char('c')), 80, 20, 0.0);
        assert!(a.selected.is_none());
        assert!(!a.tracing);
        assert!(a.trace().is_none());
    }

    #[test]
    fn q_and_esc_quit_and_ctrl_c_does_too() {
        let mut a = app();
        assert!(handle_key(&mut a, KeyEvent::from(KeyCode::Char('q')), 80, 20, 0.0));
        assert!(handle_key(&mut a, KeyEvent::from(KeyCode::Esc), 80, 20, 0.0));
        assert!(handle_key(
            &mut a,
            KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
            80,
            20,
            0.0
        ));
    }

    #[test]
    fn the_number_keys_switch_views() {
        let mut a = app();
        handle_key(&mut a, KeyEvent::from(KeyCode::Char('2')), 80, 20, 0.0);
        assert_eq!(a.view, View::Tornado);
        handle_key(&mut a, KeyEvent::from(KeyCode::Char('4')), 80, 20, 0.0);
        assert_eq!(a.view, View::Roster);
        handle_key(&mut a, KeyEvent::from(KeyCode::Char('3')), 80, 20, 0.0);
        assert_eq!(a.view, View::Coherence);
        handle_key(&mut a, KeyEvent::from(KeyCode::Char('1')), 80, 20, 0.0);
        assert_eq!(a.view, View::Graph);
    }

    #[test]
    fn left_and_right_rotate_the_vortex_in_tornado_view() {
        let mut a = app();
        a.view = View::Tornado;
        let before = a.camera.yaw;
        handle_key(&mut a, KeyEvent::from(KeyCode::Left), 80, 20, 0.0);
        assert!(a.camera.yaw > before, "the vortex should rotate about its axis");
        handle_key(&mut a, KeyEvent::from(KeyCode::Right), 80, 20, 0.0);
        assert_eq!(a.camera.yaw, before);
    }

    /// A narrow terminal must still be able to reach the right-hand columns.
    #[test]
    fn following_the_selection_scrolls_it_into_view() {
        let mut a = app();
        a.selected = Some("x".into());
        a.follow_selection(30, 10);
        let p = a.layout.find("x").unwrap().clone();
        assert!(a.scroll_x <= p.x, "selected box starts before the viewport");
        assert!(p.x + p.w <= a.scroll_x + 30, "selected box runs past the viewport");
    }

    #[test]
    fn scroll_is_clamped_to_the_canvas() {
        let mut a = app();
        a.scroll_x = 9_000;
        a.scroll_y = 9_000;
        a.clamp_scroll(40, 10);
        assert!(a.scroll_x < a.layout.width);
        assert!(a.scroll_y < a.layout.height);
    }

    /// A selection pointing at a node that no longer exists renders a blank panel with no
    /// explanation, so it is dropped on the poll that loses it.
    #[test]
    fn a_vanished_selection_is_dropped_on_recollect() {
        let mut a = app();
        a.selected = Some("ghost".into());
        a.tracing = true;
        a.recollect();
        assert!(a.selected.is_none());
        assert!(!a.tracing);
    }

    #[test]
    fn an_empty_mesh_navigates_without_panicking() {
        let mut a = App::new(".".into(), Mesh::new(vec![], vec![], "t".into()), Duration::from_secs(4), View::Graph);
        a.move_selection(0, 1);
        a.move_selection(1, 0);
        a.cycle(true);
        a.follow_selection(40, 10);
        a.clamp_scroll(40, 10);
        assert!(a.selected.is_none());
    }

    /// Drive the whole frame through ratatui's TestBackend across every view and a couple
    /// of camera angles, so a layout/state regression in the overhaul fails here.
    #[test]
    fn the_frame_renders_in_every_view_with_a_rotated_camera() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let mesh = Mesh::new(
            vec![
                node_with("l", NodeKind::Listener, Provenance::Live { age_secs: 1 }, Verdict::Pass),
                node_with("f", NodeKind::Filter, Provenance::Stale { age_secs: 999, budget_secs: 30 }, Verdict::Veto),
                node_with("x", NodeKind::Executor, Provenance::Live { age_secs: 1 }, Verdict::Idle),
            ],
            vec![Edge::veto("f", "x").with_active(Some(true)), Edge::signal("l", "f")],
            "2026-08-16T00:00:00Z".into(),
        );
        let layout = view::layout(&mesh);
        let camera = Camera { yaw: 1.1, pitch: 0.4, distance: 9.0, focal: 14.0 };
        let canvas = render::paint_graph(&mesh, &layout, None, None);

        for view in View::ALL {
            let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
            terminal
                .draw(|f| {
                    render::draw(
                        f,
                        &render::Screen {
                            mesh: &mesh,
                            layout: &layout,
                            canvas: &canvas,
                            root: ".",
                            selected: None,
                            tracing: false,
                            show_terms: true,
                            scroll_x: 0,
                            scroll_y: 0,
                            interval_secs: 4,
                            last_error: None,
                            view,
                            time: 2.0,
                            camera: &camera,
                        },
                    )
                })
                .unwrap();
        }
    }
}
