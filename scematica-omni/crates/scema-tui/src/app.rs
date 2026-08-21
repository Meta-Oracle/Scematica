//! Application state, and the one thread that is allowed to think.
//!
//! ## Why there is a worker thread at all
//!
//! Observing a repository walks a tree, and running a cycle observes, hypothesises,
//! projects and hashes. On a monorepo that is comfortably longer than a frame. A TUI that
//! did it on the draw thread would freeze mid-keystroke, and the operator's reasonable
//! conclusion — that it had hung — would be indistinguishable from it actually having hung.
//! So every call into `scema-agent` goes through [`Worker`], and the UI stays responsive
//! with an explicit [`App::busy`] marker rather than an implicit freeze.
//!
//! ## Simulate is the default; decide is not
//!
//! `Enter` runs a **simulation**, which writes nothing. Sealing a record is a separate key
//! and a confirmation, and that asymmetry is deliberate: `scema simulate` and `scema
//! decide` compute exactly the same thing and differ only in whether they leave a trace, so
//! the only protection against a counterfactual becoming a record is that the two are not
//! the same keystroke. A TUI where the obvious button persists would quietly fill `.scema/`
//! with decisions nobody made.
//!
//! ## Grounding is ticked, never inferred
//!
//! The signal list is a multi-select, and what it selects is `Goal::grounded_in`. Nothing
//! here reads the goal text looking for a matching signal — an earlier version of the CLI
//! inferred grounding by keyword overlap and immediately grounded "add tests to the
//! scema-cli crate" in a marker backlog in a different crate, because `scema` is a
//! substring of every unit name in this workspace. **An instruction is not evidence**, and
//! the checkbox is where the operator supplies the evidence instead.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;

use scema_agent::{Agent, Cycle};
use scema_memory::{Calibration, MemoryKind};
use scema_policy::DecisionConfig;
use scema_verify::{verify, DecisionRecord, RecordStore, Verification};
use scema_world::{Constraint, Goal, WorldState};

use crate::theme::Theme;

/// The five things this console can be showing.
///
/// One tab per stage of the loop that produces something a human needs to look at.
/// `hypothesise` and `score` have no tab because their output is only legible as the
/// matrix, which belongs to `Simulate`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab {
    /// What was perceived, and what could not be.
    World,
    /// The goal, the grounding, and the ranking that came out.
    Simulate,
    /// Sealed decision records, and whether their commitments still hold.
    Records,
    /// The four memories, and the calibration that is mostly unresolved.
    Memory,
    /// The λ weights, the gates, the observers and the specialists.
    Policy,
}

impl Tab {
    pub const ALL: [Tab; 5] = [Tab::World, Tab::Simulate, Tab::Records, Tab::Memory, Tab::Policy];

    pub fn title(self) -> &'static str {
        match self {
            Tab::World => "WORLD",
            Tab::Simulate => "SIMULATE",
            Tab::Records => "RECORDS",
            Tab::Memory => "MEMORY",
            Tab::Policy => "POLICY",
        }
    }

    /// The question the tab answers. Shown in the header so a new operator does not have to
    /// guess what they are looking at.
    pub fn question(self) -> &'static str {
        match self {
            Tab::World => "what is out there, and what could not be seen?",
            Tab::Simulate => "which branch wins, and does anything measured support it?",
            Tab::Records => "what was decided, and does the record still verify?",
            Tab::Memory => "what has been retained, and how well did it project?",
            Tab::Policy => "under whose preferences, and with which specialists?",
        }
    }

    pub fn next(self) -> Tab {
        let i = Tab::ALL.iter().position(|t| *t == self).unwrap_or(0);
        Tab::ALL[(i + 1) % Tab::ALL.len()]
    }

    pub fn prev(self) -> Tab {
        let i = Tab::ALL.iter().position(|t| *t == self).unwrap_or(0);
        Tab::ALL[(i + Tab::ALL.len() - 1) % Tab::ALL.len()]
    }
}

/// Which pane inside the current tab takes the arrow keys.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Focus {
    Left,
    Right,
}

impl Focus {
    pub fn flip(self) -> Focus {
        match self {
            Focus::Left => Focus::Right,
            Focus::Right => Focus::Left,
        }
    }
}

/// What typing does right now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// Keys are commands.
    Normal,
    /// Keys go into the goal line.
    EditGoal,
    /// Keys go into a new `must-not` constraint.
    EditConstraint,
    /// Waiting for a yes/no on sealing a record.
    ConfirmDecide,
}

/// A line in the status bar, with how alarming it is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Status {
    Idle,
    Note(String),
    Warn(String),
    Error(String),
}

impl Status {
    pub fn text(&self) -> &str {
        match self {
            Status::Idle => "",
            Status::Note(s) | Status::Warn(s) | Status::Error(s) => s,
        }
    }
}

/// One row of the record list.
#[derive(Clone, Debug)]
pub struct RecordRow {
    pub id: String,
    pub at: i64,
    pub goal: String,
    pub entity: String,
    /// `None` when the record abstained; the headline is in `outcome`.
    pub chosen: Option<String>,
    pub outcome: String,
    /// `Some(false)` is the alarming case; `None` means the record could not be read at
    /// all, which is a third state and must not render as "invalid".
    pub valid: Option<bool>,
    /// Set when the record itself is unreadable. A corrupt store must not look like a
    /// smaller one — the same rule `scema explain --list` follows.
    pub unreadable: Option<String>,
}

/// Per-kind counts plus calibration.
#[derive(Clone, Debug)]
pub struct MemoryView {
    pub counts: Vec<(MemoryKind, usize, usize)>,
    pub calibration: Calibration,
    pub root: PathBuf,
}

/// Work handed to the [`Worker`].
pub enum Job {
    Observe { path: String },
    Cycle { world: Box<WorldState>, goal: Box<Goal>, persist: bool },
    LoadRecords,
    OpenRecord(String),
    LoadMemory,
}

/// Work that came back.
pub enum Done {
    Observed(Result<Box<WorldState>, String>),
    Cycled(Result<Box<Cycle>, String>),
    Records(Result<Vec<RecordRow>, String>),
    Opened(Result<Box<(DecisionRecord, Verification)>, String>),
    Memory(Result<Box<MemoryView>, String>),
}

/// The thinking thread.
///
/// One thread, not a pool. Every job here reads the same `.scema/` directory and the same
/// tree; running two observations concurrently would double the I/O to produce one screen,
/// and running a cycle concurrently with the record list would let a `decide` land
/// half-way through the listing it is about to change.
pub struct Worker {
    tx: Sender<Job>,
    pub rx: Receiver<Done>,
}

impl Worker {
    pub fn spawn(agent: Arc<Agent>, root: PathBuf) -> Worker {
        let (tx, jobs) = mpsc::channel::<Job>();
        let (results, rx) = mpsc::channel::<Done>();
        std::thread::spawn(move || {
            for job in jobs {
                let out = match job {
                    Job::Observe { path } => Done::Observed(
                        agent.observe(&path).map(Box::new).map_err(|e| format!("{e:#}")),
                    ),
                    Job::Cycle { world, goal, persist } => {
                        // `Agent::persist` is a field on a shared `Arc`, so the simulate
                        // path cannot flip it — two jobs would race and a simulation could
                        // seal a record. Building a second agent for the dry run is cheap
                        // and it is the path that must never write.
                        let result = if persist {
                            agent.cycle_over(*world, *goal)
                        } else {
                            let mut dry = Agent::new(root.clone(), None);
                            dry.persist = false;
                            dry.config = agent.config;
                            dry.cycle_over(*world, *goal)
                        };
                        Done::Cycled(result.map(Box::new).map_err(|e| format!("{e:#}")))
                    }
                    Job::LoadRecords => Done::Records(load_records(&root)),
                    Job::OpenRecord(id) => Done::Opened(
                        RecordStore::new(root.clone())
                            .load(&id)
                            .map(|r| {
                                let v = verify(&r);
                                Box::new((r, v))
                            })
                            .map_err(|e| format!("{e:#}")),
                    ),
                    Job::LoadMemory => Done::Memory(load_memory(&agent).map(Box::new)),
                };
                if results.send(out).is_err() {
                    break; // the UI is gone
                }
            }
        });
        Worker { tx, rx }
    }

    pub fn send(&self, job: Job) {
        // A dead worker means the process is shutting down. Losing a job at that point is
        // correct; panicking would turn a clean exit into a crash report.
        let _ = self.tx.send(job);
    }
}

fn load_records(root: &Path) -> Result<Vec<RecordRow>, String> {
    let store = RecordStore::new(root.to_path_buf());
    let ids = store.ids().map_err(|e| format!("{e:#}"))?;
    Ok(ids
        .iter()
        .map(|id| match store.load(id) {
            Ok(r) => {
                let v = verify(&r);
                RecordRow {
                    id: r.id.clone(),
                    at: r.at,
                    goal: r.goal.statement.clone(),
                    entity: r.world.entity.locator.clone(),
                    chosen: r.decision.chosen.clone(),
                    outcome: match (&r.decision.chosen, &r.decision.abstention) {
                        (Some(c), _) => format!("chose {c}"),
                        (None, Some(a)) => a.headline(),
                        _ => "no decision and no reason".into(),
                    },
                    valid: Some(v.valid),
                    unreadable: None,
                }
            }
            // Three states, not two. "Could not read the record" and "the record does not
            // verify" are different claims and only one of them is an accusation.
            Err(e) => RecordRow {
                id: id.clone(),
                at: 0,
                goal: String::new(),
                entity: String::new(),
                chosen: None,
                outcome: String::new(),
                valid: None,
                unreadable: Some(format!("{e:#}")),
            },
        })
        .collect())
}

fn load_memory(agent: &Agent) -> Result<MemoryView, String> {
    let mem = agent.memory();
    Ok(MemoryView {
        counts: mem.counts().map_err(|e| format!("{e:#}"))?,
        calibration: mem.calibration().map_err(|e| format!("{e:#}"))?,
        root: mem.root().join("memory"),
    })
}

/// Everything on screen.
pub struct App {
    pub theme: Theme,
    pub root: PathBuf,
    pub path: String,
    pub runtime: &'static str,
    pub config: DecisionConfig,
    pub observers: Vec<(String, String)>,
    pub evaluators: Vec<(String, String)>,

    pub tab: Tab,
    pub focus: Focus,
    pub mode: Mode,
    pub should_quit: bool,
    pub tick: u64,
    pub status: Status,
    /// What the worker is doing, for the header. `None` means idle.
    pub busy: Option<&'static str>,
    /// Toggles the help overlay.
    pub help: bool,

    pub world: Option<WorldState>,
    pub object_sel: usize,
    pub signal_sel: usize,
    pub grounded: BTreeSet<String>,

    pub goal: String,
    pub constraint_draft: String,
    pub must_not: Vec<String>,
    pub cycle: Option<Cycle>,
    /// True when `cycle` came from a `decide` rather than a `simulate`. Drives the banner
    /// that says whether anything was written.
    pub cycle_persisted: bool,
    pub matrix_sel: usize,

    pub records: Vec<RecordRow>,
    pub record_sel: usize,
    pub open_record: Option<Box<(DecisionRecord, Verification)>>,

    pub memory: Option<MemoryView>,
}

impl App {
    pub fn new(theme: Theme, root: PathBuf, path: String, agent: &Agent) -> App {
        App {
            theme,
            root,
            path,
            runtime: scema_agent::RUNTIME,
            config: agent.config,
            observers: agent
                .observers()
                .iter()
                .map(|o| (o.name().to_string(), o.about().to_string()))
                .collect(),
            evaluators: agent
                .evaluators()
                .iter()
                .map(|e| (e.name().to_string(), e.about().to_string()))
                .collect(),
            tab: Tab::World,
            focus: Focus::Left,
            mode: Mode::Normal,
            should_quit: false,
            tick: 0,
            status: Status::Idle,
            busy: None,
            help: false,
            world: None,
            object_sel: 0,
            signal_sel: 0,
            grounded: BTreeSet::new(),
            goal: String::new(),
            constraint_draft: String::new(),
            must_not: Vec::new(),
            cycle: None,
            cycle_persisted: false,
            matrix_sel: 0,
            records: Vec::new(),
            record_sel: 0,
            open_record: None,
            memory: None,
        }
    }

    /// The goal as the loop will see it: statement, constraints, and the ticked grounds.
    ///
    /// Note what is absent — anything that reads `self.goal` looking for a signal to ground
    /// it in. Grounding comes from [`App::grounded`], which is a set the operator ticked.
    pub fn build_goal(&self) -> Goal {
        let mut g = Goal::new("goal", self.goal.trim());
        for spec in &self.must_not {
            let (subject, detail) = match spec.split_once(':') {
                Some((a, b)) => (a.trim(), b.trim()),
                None => (spec.trim(), "declared in the console"),
            };
            // An empty subject forbids every branch by substring match. Dropped, exactly as
            // the CLI and the daemon drop it.
            if !subject.is_empty() {
                g = g.with_constraint(Constraint::must_not(subject, detail));
            }
        }
        for id in &self.grounded {
            g = g.grounded(id);
        }
        g
    }

    /// Ground ids that name no signal in the observed world.
    ///
    /// The simulator drops them silently, so a client that never sees the list cannot tell
    /// a typo from a disagreement. In this console they can only arrive by observing a new
    /// world while an old selection is still held, which is exactly the case worth warning
    /// about.
    pub fn dangling_grounds(&self) -> Vec<String> {
        let Some(w) = &self.world else {
            return self.grounded.iter().cloned().collect();
        };
        self.grounded
            .iter()
            .filter(|id| !w.signals.iter().any(|s| &&s.id == id))
            .cloned()
            .collect()
    }

    /// Can a cycle be run at all? Returns the reason when it cannot.
    pub fn cycle_blocked(&self) -> Option<&'static str> {
        if self.world.is_none() {
            return Some("nothing has been observed yet — press `o` on the WORLD tab");
        }
        if self.goal.trim().is_empty() {
            return Some("no goal — press `g` and type one");
        }
        if self.busy.is_some() {
            return Some("already working");
        }
        None
    }

    /// Fold a completed job back into the state.
    pub fn absorb(&mut self, done: Done) {
        self.busy = None;
        match done {
            Done::Observed(Ok(w)) => {
                let signals = w.signals.len();
                let blind = w.blind_spots.len();
                // Grounds are kept across a re-observation rather than cleared, because the
                // common case is re-observing the same tree after an edit and the operator
                // would have to re-tick everything. `dangling_grounds` is what catches the
                // uncommon case.
                self.world = Some(*w);
                self.object_sel = 0;
                self.signal_sel = 0;
                self.status = Status::Note(format!(
                    "observed: {signals} counted signal(s), {blind} blind spot(s)"
                ));
            }
            Done::Observed(Err(e)) => {
                self.world = None;
                self.status = Status::Error(format!("observe failed: {e}"));
            }
            Done::Cycled(Ok(c)) => {
                self.cycle_persisted = c.record_path.is_some();
                self.status = if self.cycle_persisted {
                    Status::Note(format!(
                        "sealed {} · {} memory record(s) appended",
                        c.record.id, c.remembered
                    ))
                } else {
                    Status::Note(format!(
                        "simulated — nothing written. Would seal as {}.",
                        c.record.id
                    ))
                };
                self.cycle = Some(*c);
                self.matrix_sel = 0;
                self.tab = Tab::Simulate;
            }
            Done::Cycled(Err(e)) => self.status = Status::Error(format!("cycle failed: {e}")),
            Done::Records(Ok(rows)) => {
                let bad = rows.iter().filter(|r| r.valid == Some(false)).count();
                let unreadable = rows.iter().filter(|r| r.unreadable.is_some()).count();
                self.record_sel = self.record_sel.min(rows.len().saturating_sub(1));
                self.records = rows;
                self.status = match (bad, unreadable) {
                    (0, 0) => Status::Note(format!("{} record(s), all verify", self.records.len())),
                    (b, 0) => Status::Error(format!("{b} record(s) DO NOT VERIFY")),
                    (0, u) => Status::Warn(format!("{u} record(s) could not be read")),
                    (b, u) => Status::Error(format!("{b} invalid, {u} unreadable")),
                };
            }
            Done::Records(Err(e)) => self.status = Status::Error(format!("record store: {e}")),
            Done::Opened(Ok(pair)) => {
                self.status = if pair.1.valid {
                    Status::Note(format!("{} verifies", pair.0.id))
                } else {
                    Status::Error(format!(
                        "{} DOES NOT VERIFY — {} field(s) moved",
                        pair.0.id,
                        pair.1.mismatches.len()
                    ))
                };
                self.open_record = Some(pair);
            }
            Done::Opened(Err(e)) => self.status = Status::Error(format!("record: {e}")),
            Done::Memory(Ok(m)) => self.memory = Some(*m),
            Done::Memory(Err(e)) => self.status = Status::Error(format!("memory: {e}")),
        }
    }

    /// Move a selection index, clamped, wrapping at neither end.
    ///
    /// Not wrapping is a deliberate choice for lists that carry alarming rows: an operator
    /// holding `down` to reach the bottom of a record list should stop at the bottom, not
    /// silently arrive back at the top and start re-reading.
    pub fn step(index: &mut usize, len: usize, delta: isize) {
        if len == 0 {
            *index = 0;
            return;
        }
        let next = (*index as isize + delta).clamp(0, len as isize - 1);
        *index = next as usize;
    }

    pub fn toggle_ground(&mut self) {
        let Some(w) = &self.world else { return };
        let Some(s) = w.signals.get(self.signal_sel) else { return };
        if !self.grounded.remove(&s.id) {
            self.grounded.insert(s.id.clone());
        }
        self.status = Status::Note(format!(
            "{} ground(s) asserted — an instruction is not evidence, a ticked signal is",
            self.grounded.len()
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use scema_world::{Domain, Entity, EntityKind, Extent, Polarity, Signal};

    fn world(signal_ids: &[&str]) -> WorldState {
        WorldState {
            observer: "test".into(),
            entity: Entity {
                kind: EntityKind::Repository,
                locator: "/r".into(),
                label: "r".into(),
            },
            domain: Domain::Software,
            observed_at: 0,
            objects: vec![],
            facts: vec![],
            signals: signal_ids
                .iter()
                .map(|id| Signal {
                    id: (*id).into(),
                    polarity: Polarity::Risk,
                    label: (*id).into(),
                    detail: String::new(),
                    magnitude: 0.5,
                    measured: true,
                    targets: vec![],
                    evidence: vec![],
                })
                .collect(),
            extent: Extent::complete(0, "t"),
            blind_spots: vec![],
        }
    }

    fn app() -> App {
        let dir = std::env::temp_dir().join("scema-tui-test");
        let agent = Agent::new(&dir, None);
        App::new(Theme::default(), dir, ".".into(), &agent)
    }

    #[test]
    fn grounding_comes_only_from_the_ticked_set() {
        // The regression this console must never reintroduce. The goal text names a signal
        // id verbatim and it still does not ground the branch, because nothing reads the
        // goal text looking for one.
        let mut a = app();
        a.world = Some(world(&["untested:scema-cli"]));
        a.goal = "add tests to untested:scema-cli".into();
        assert!(a.build_goal().grounded_in.is_empty());

        a.signal_sel = 0;
        a.toggle_ground();
        assert_eq!(a.build_goal().grounded_in, vec!["untested:scema-cli".to_string()]);
    }

    #[test]
    fn a_ground_left_over_from_an_earlier_world_is_reported_not_dropped() {
        let mut a = app();
        a.world = Some(world(&["a"]));
        a.grounded.insert("a".into());
        a.grounded.insert("gone".into());
        assert_eq!(a.dangling_grounds(), vec!["gone".to_string()]);
    }

    #[test]
    fn an_empty_constraint_subject_is_dropped_rather_than_forbidding_everything() {
        // A `must_not` with an empty subject matches every target by substring and would
        // exclude the whole matrix. Same drop the CLI and daemon make.
        let mut a = app();
        a.must_not.push("  ".into());
        a.must_not.push("crates/x:frozen".into());
        let g = a.build_goal();
        assert_eq!(g.constraints.len(), 1);
        assert_eq!(g.constraints[0].subject, "crates/x");
    }

    #[test]
    fn a_cycle_is_blocked_with_a_reason_rather_than_silently_doing_nothing() {
        let mut a = app();
        assert!(a.cycle_blocked().unwrap().contains("observed"));
        a.world = Some(world(&["a"]));
        assert!(a.cycle_blocked().unwrap().contains("goal"));
        a.goal = "do the thing".into();
        assert!(a.cycle_blocked().is_none());
        a.busy = Some("observing");
        assert!(a.cycle_blocked().is_some());
    }

    #[test]
    fn selection_clamps_and_does_not_wrap() {
        let mut i = 0usize;
        App::step(&mut i, 3, -1);
        assert_eq!(i, 0);
        App::step(&mut i, 3, 5);
        assert_eq!(i, 2);
        App::step(&mut i, 0, 1);
        assert_eq!(i, 0);
    }

    #[test]
    fn an_unreadable_record_is_a_third_state_not_an_invalid_one() {
        // "could not read this record" and "this record does not verify" are different
        // claims and only one is an accusation.
        let row = RecordRow {
            id: "x".into(),
            at: 0,
            goal: String::new(),
            entity: String::new(),
            chosen: None,
            outcome: String::new(),
            valid: None,
            unreadable: Some("bad json".into()),
        };
        assert!(row.valid.is_none());
        assert_ne!(row.valid, Some(false));
    }

    #[test]
    fn tabs_cycle_in_both_directions() {
        assert_eq!(Tab::World.next(), Tab::Simulate);
        assert_eq!(Tab::World.prev(), Tab::Policy);
        assert_eq!(Tab::Policy.next(), Tab::World);
    }

    #[test]
    fn every_tab_states_the_question_it_answers() {
        let mut qs: Vec<&str> = Tab::ALL.iter().map(|t| t.question()).collect();
        qs.sort_unstable();
        let before = qs.len();
        qs.dedup();
        assert_eq!(before, qs.len(), "two tabs claim to answer the same question");
    }
}
