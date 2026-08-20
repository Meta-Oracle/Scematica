//! # scema-agent — the loop
//!
//! ```text
//!   observe ─▶ hypothesise ─▶ simulate ─▶ score ─▶ decide ─▶ record ─▶ remember
//!      ▲                                                                  │
//!      └──────────────────────────────────────────────────────────────────┘
//! ```
//!
//! Every stage is a trait with at least one real implementation, and the whole pass is
//! deterministic: the same world and goal produce the same [`DecisionRecord`] id. That is
//! not an aesthetic property — it is what makes the record verifiable by somebody who was
//! not there.
//!
//! ## What the loop does *not* do
//!
//! It does not execute. [`Cycle`] ends at a decision and a record; nothing in this
//! workspace writes to the environment it observed. The [`scema_world::Action`] values in a
//! chosen hypothesis are a *declaration of intent* that has been risk-classified and
//! constraint-checked, and turning one into a side effect is a separate crate with a
//! separate approval model — the one `alchem-link` already worked out, where risk class is
//! declared per tool and no terminal means deny.
//!
//! Saying that plainly matters more than shipping it. An agent runtime that quietly gained
//! a write path would invalidate every claim the rest of these crates make about being safe
//! to point at a live system.
//!
//! ## Memory is written on every pass, including the abstentions
//!
//! The rejected branches are the interesting ones. Each becomes a
//! [`scema_memory::MemoryBody::Counterfactual`] holding what was projected for it and why
//! it lost — and, per that crate's rule, it stays unresolved forever unless somebody
//! actually runs it. An abstention is recorded as an episode with
//! [`scema_memory::Outcome::Unobserved`], because "the agent declined" is a fact about the
//! agent and not an outcome in the world.

pub mod hypothesize;

use anyhow::{anyhow, Result};
use scema_memory::{MemoryBody, MemoryKind, MemoryRecord, MemoryStore, Outcome};
use scema_policy::{decide, Decision, DecisionConfig, Evaluator};
use scema_sim::{Projection, Simulator, StructuralSimulator};
use scema_tools::{Observer, RepoObserver};
use scema_verify::{DecisionRecord, RecordStore};
use scema_world::{now_secs, Goal, Hypothesis, WorldState};
use std::path::PathBuf;

use crate::hypothesize::{GoalHypothesizer, Hypothesizer, MemoryHypothesizer, SignalHypothesizer};

/// Runtime identifier stamped into every record.
pub const RUNTIME: &str = concat!("scema-omni/", env!("CARGO_PKG_VERSION"));

/// One complete pass.
pub struct Cycle {
    pub world: WorldState,
    pub hypotheses: Vec<Hypothesis>,
    pub projections: Vec<Projection>,
    pub decision: Decision,
    pub record: DecisionRecord,
    /// Where the record was written, when it was. `None` in dry-run.
    pub record_path: Option<PathBuf>,
    /// Memory records appended by this pass.
    pub remembered: usize,
}

/// The orchestrator.
///
/// The trait objects carry `Send + Sync` because `scema-daemon` shares one `Arc<Agent>`
/// across connection threads. Constructing an agent per request would reload the Deep Q*
/// checkpoint every time, and a `Mutex` would serialise every observation behind whichever
/// request is currently walking a large tree. Every implementation in this workspace is
/// already thread-safe — they hold plain data and take no locks.
pub struct Agent {
    observers: Vec<Box<dyn Observer + Send + Sync>>,
    simulator: Box<dyn Simulator + Send + Sync>,
    evaluators: Vec<Box<dyn Evaluator + Send + Sync>>,
    memory: MemoryStore,
    records: RecordStore,
    pub config: DecisionConfig,
    /// When false, `cycle` computes and returns everything but writes nothing. The default
    /// for `scema simulate`, which is explicitly a counterfactual and must not leave a
    /// trace that reads like a decision the agent made.
    pub persist: bool,
}

impl Agent {
    /// An agent rooted at a state directory, with the default observers and evaluators.
    ///
    /// `dqstar_checkpoint` is the sniper's `scematica-nn-agent.json` when there is one. A
    /// missing file is not an error — the evaluator reports it through its applicability,
    /// which is where an operator will actually see it.
    pub fn new(root: impl Into<PathBuf>, dqstar_checkpoint: Option<String>) -> Self {
        let root: PathBuf = root.into();
        let mut evaluators: Vec<Box<dyn Evaluator + Send + Sync>> = Vec::new();

        {
            use scema_policy::dqstar::DqStarEvaluator;
            evaluators.push(Box::new(match dqstar_checkpoint {
                Some(p) => DqStarEvaluator::from_checkpoint(p),
                None => DqStarEvaluator::unloaded(),
            }));
        }

        Agent {
            observers: vec![Box::new(RepoObserver::new())],
            simulator: Box::new(StructuralSimulator::new()),
            evaluators,
            memory: MemoryStore::new(root.clone()),
            records: RecordStore::new(root),
            config: DecisionConfig::default(),
            persist: true,
        }
    }

    pub fn memory(&self) -> &MemoryStore {
        &self.memory
    }

    pub fn records(&self) -> &RecordStore {
        &self.records
    }

    pub fn observers(&self) -> &[Box<dyn Observer + Send + Sync>] {
        &self.observers
    }

    pub fn evaluators(&self) -> &[Box<dyn Evaluator + Send + Sync>] {
        &self.evaluators
    }

    /// Perceive an environment.
    pub fn observe(&self, locator: &str) -> Result<WorldState> {
        let observer = self
            .observers
            .iter()
            .find(|o| o.handles(locator))
            .ok_or_else(|| anyhow!("no observer in this build handles `{locator}`"))?;
        observer.observe(locator)
    }

    /// Propose branches from every hypothesiser, in a stable order.
    ///
    /// Duplicate ids are dropped, first proposer wins. Two hypothesisers arriving at the
    /// same branch is a real occurrence — a memory of a procedure that a signal rule would
    /// also propose — and ranking the same branch twice would inflate its apparent support.
    pub fn hypothesize(&self, world: &WorldState, goal: &Goal) -> Vec<Hypothesis> {
        let memory_hypothesizer = MemoryHypothesizer::new(&self.memory);
        let sources: Vec<&dyn Hypothesizer> = vec![
            &GoalHypothesizer,
            &SignalHypothesizer,
            &memory_hypothesizer,
        ];
        let mut out: Vec<Hypothesis> = Vec::new();
        for s in sources {
            for h in s.propose(world, goal) {
                if !out.iter().any(|existing| existing.id == h.id) {
                    out.push(h);
                }
            }
        }
        out
    }

    /// Run one full pass.
    pub fn cycle(&self, locator: &str, goal: Goal) -> Result<Cycle> {
        let world = self.observe(locator)?;
        self.cycle_over(world, goal)
    }

    /// Run a pass over a world that was already observed.
    ///
    /// Split out so the CLI can observe once and simulate several goals against the same
    /// world, and so a test can drive the loop over a constructed world without a
    /// filesystem.
    pub fn cycle_over(&self, world: WorldState, goal: Goal) -> Result<Cycle> {
        let hypotheses = self.hypothesize(&world, &goal);
        let projections = self.simulator.project_all(&world, &goal, &hypotheses);

        // The explicit cast is load-bearing: `&(dyn Evaluator + Send + Sync)` coerces to
        // `&dyn Evaluator`, but inference will not do it inside `collect`.
        let evaluator_refs: Vec<&dyn Evaluator> = self
            .evaluators
            .iter()
            .map(|b| b.as_ref() as &dyn Evaluator)
            .collect();
        let decision = decide(
            &world,
            &goal,
            &hypotheses,
            &projections,
            &evaluator_refs,
            self.config,
        );

        let record = DecisionRecord::seal(
            RUNTIME,
            now_secs(),
            world.clone(),
            goal.clone(),
            hypotheses.clone(),
            projections.clone(),
            decision.clone(),
        );

        let (record_path, remembered) = if self.persist {
            let path = self.records.save(&record)?;
            let n = self.write_memory(&record)?;
            (Some(path), n)
        } else {
            (None, 0)
        };

        Ok(Cycle { world, hypotheses, projections, decision, record, record_path, remembered })
    }

    /// Append this pass to memory: one episode, one counterfactual per branch not taken.
    fn write_memory(&self, record: &DecisionRecord) -> Result<usize> {
        let mut n = 0usize;
        let d = &record.decision;
        let subject = record.world.entity.locator.clone();

        let (what, outcome) = match (&d.chosen, &d.abstention) {
            (Some(id), _) => (format!("chose `{id}` for goal `{}`", record.goal.statement), Outcome::Unobserved),
            (None, Some(a)) => (format!("abstained: {}", a.headline()), Outcome::Unobserved),
            (None, None) => ("no decision and no stated reason".into(), Outcome::Unobserved),
        };
        // `Unobserved` in both arms and deliberately so. The agent knows what it decided; it
        // does not know whether that was right, and `Succeeded` here would be the loop
        // grading its own homework before anything happened.
        self.memory.remember(
            &MemoryRecord::new(
                record.id.clone(),
                MemoryKind::Episodic,
                record.at,
                subject.clone(),
                MemoryBody::Episode {
                    what,
                    outcome,
                    evidence: vec![format!("decision record {}", record.id)],
                },
                RUNTIME,
            )
            .tagged("cycle"),
        )?;
        n += 1;

        for (i, r) in d.ranked.iter().enumerate() {
            if Some(&r.hypothesis) == d.chosen.as_ref() {
                continue;
            }
            let reason = match &d.abstention {
                Some(a) => a.headline(),
                None => format!("ranked #{} of {}", i + 1, d.ranked.len()),
            };
            self.memory.remember(&MemoryRecord::new(
                format!("{}-{}", record.id, r.hypothesis),
                MemoryKind::Counterfactual,
                record.at,
                subject.clone(),
                MemoryBody::Counterfactual {
                    decision: record.id.clone(),
                    hypothesis: r.hypothesis.clone(),
                    statement: r.statement.clone(),
                    projected: r.utility.value,
                    reason,
                },
                RUNTIME,
            ))?;
            n += 1;
        }

        for e in &d.excluded {
            self.memory.remember(&MemoryRecord::new(
                format!("{}-{}", record.id, e.hypothesis),
                MemoryKind::Counterfactual,
                record.at,
                subject.clone(),
                MemoryBody::Counterfactual {
                    decision: record.id.clone(),
                    hypothesis: e.hypothesis.clone(),
                    statement: e.statement.clone(),
                    // A forbidden branch was never projected — it was removed before
                    // ranking. `f64::NAN` would poison the calibration arithmetic, so it
                    // is recorded as 0.0 with the reason carrying the truth.
                    projected: 0.0,
                    reason: format!("forbidden: {}", e.reason),
                },
                RUNTIME,
            ))?;
            n += 1;
        }

        Ok(n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use scema_world::{
        Constraint, Domain, Entity, EntityKind, Extent, Object, Polarity, Provenance, Signal,
    };
    use std::fs;

    fn scratch() -> PathBuf {
        let p = std::env::temp_dir().join(format!(
            "scema-omni-agent-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&p).unwrap();
        p
    }

    fn world_with(signals: Vec<Signal>) -> WorldState {
        WorldState {
            observer: "test".into(),
            entity: Entity {
                kind: EntityKind::Repository,
                locator: "/repo".into(),
                label: "repo".into(),
            },
            domain: Domain::Software,
            observed_at: 1_700_000_000,
            objects: vec![Object::new("o", "file", "o", Provenance::Live { age_secs: 0 })],
            facts: vec![],
            signals,
            extent: Extent::complete(1, "walked"),
            blind_spots: vec![],
        }
    }

    fn untested() -> Signal {
        Signal {
            id: "untested:x".into(),
            polarity: Polarity::Risk,
            label: "`x` has no tests".into(),
            detail: "3 files, 900 lines, zero test attributes".into(),
            magnitude: 0.9,
            measured: true,
            targets: vec!["unit:crates/x".into()],
            evidence: vec!["counted 0".into()],
        }
    }

    #[test]
    fn a_full_pass_produces_a_record_that_verifies() {
        let dir = scratch();
        let agent = Agent::new(&dir, None);
        let c = agent
            .cycle_over(world_with(vec![untested()]), Goal::new("g", "raise confidence"))
            .unwrap();
        assert!(scema_verify::verify(&c.record).valid);
        assert!(c.record_path.unwrap().exists());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn the_same_world_and_goal_produce_the_same_record_id() {
        // Determinism is the precondition for a third party verifying anything.
        let dir = scratch();
        let agent = Agent::new(&dir, None);
        let w = world_with(vec![untested()]);
        let g = Goal::new("g", "raise confidence");
        let a = DecisionRecord::seal(
            RUNTIME,
            0,
            w.clone(),
            g.clone(),
            agent.hypothesize(&w, &g),
            vec![],
            decide(&w, &g, &[], &[], &[], agent.config),
        );
        let b = DecisionRecord::seal(
            RUNTIME,
            0,
            w.clone(),
            g.clone(),
            agent.hypothesize(&w, &g),
            vec![],
            decide(&w, &g, &[], &[], &[], agent.config),
        );
        assert_eq!(a.id, b.id);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn rejected_branches_become_unresolved_counterfactuals() {
        let dir = scratch();
        let agent = Agent::new(&dir, None);
        agent
            .cycle_over(world_with(vec![untested()]), Goal::new("g", "raise confidence"))
            .unwrap();
        let cal = agent.memory().calibration().unwrap();
        assert!(cal.recorded > 0, "the branches not taken must be remembered");
        assert_eq!(cal.resolved, 0);
        assert_eq!(cal.mean_abs_error, None);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_record_from_a_real_observation_survives_the_json_transport() {
        // The regression behind `scema_verify::canonical`'s fixed-point float encoding. A
        // record sealed here and verified after a `GET` reported INVALID on a byte nobody
        // had touched, because `serde_json` parsed one of its own emitted floats one ULP
        // low. Synthetic worlds never hit it — their magnitudes are round numbers — so the
        // pin has to run against a real walk.
        let dir = scratch();
        fs::write(dir.join("Cargo.toml"), "[package]
name = \"t\"
").unwrap();
        fs::create_dir_all(dir.join("src")).unwrap();
        fs::write(dir.join("src/lib.rs"), "fn a() {}
// TODO: x
// FIXME: y
").unwrap();

        let agent = Agent::new(&dir, None);
        let c = agent.cycle(dir.to_str().unwrap(), Goal::new("g", "tidy up")).unwrap();
        assert!(scema_verify::verify(&c.record).valid, "sealed record must verify in memory");

        let text = serde_json::to_string(&c.record).unwrap();
        let back: DecisionRecord = serde_json::from_str(&text).unwrap();
        let v = scema_verify::verify(&back);
        assert!(v.valid, "after JSON transport: {:?}", v.mismatches);

        // And through the store, which is how `scema verify` reads it.
        let reloaded = agent.records().load(&c.record.id).unwrap();
        assert!(scema_verify::verify(&reloaded).valid);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_dry_run_writes_nothing() {
        // `scema simulate` is a counterfactual and must not leave a trace that later reads
        // like a decision the agent made.
        let dir = scratch();
        let mut agent = Agent::new(&dir, None);
        agent.persist = false;
        let c = agent
            .cycle_over(world_with(vec![untested()]), Goal::new("g", "raise confidence"))
            .unwrap();
        assert!(c.record_path.is_none());
        assert_eq!(c.remembered, 0);
        assert!(agent.records().ids().unwrap().is_empty());
        assert_eq!(agent.memory().calibration().unwrap().recorded, 0);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_world_with_nothing_counted_abstains() {
        let dir = scratch();
        let agent = Agent::new(&dir, None);
        let c = agent
            .cycle_over(world_with(vec![]), Goal::new("g", "make it better somehow"))
            .unwrap();
        assert!(c.decision.chosen.is_none());
        assert!(c.decision.abstention.is_some());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_constraint_removes_a_branch_and_the_record_still_shows_it() {
        let dir = scratch();
        let agent = Agent::new(&dir, None);
        let goal = Goal::new("g", "raise confidence")
            .with_constraint(Constraint::must_not("unit:crates/x", "frozen for the release"));
        let c = agent.cycle_over(world_with(vec![untested()]), goal).unwrap();
        assert!(!c.decision.excluded.is_empty());
        assert!(c.decision.excluded.iter().any(|e| e.reason.contains("frozen")));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn duplicate_branch_ids_are_proposed_once() {
        let dir = scratch();
        let agent = Agent::new(&dir, None);
        let w = world_with(vec![untested()]);
        let hs = agent.hypothesize(&w, &Goal::new("g", "x"));
        let mut ids: Vec<&str> = hs.iter().map(|h| h.id.as_str()).collect();
        ids.sort_unstable();
        let before = ids.len();
        ids.dedup();
        assert_eq!(before, ids.len(), "a branch counted twice looks twice as supported");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn the_dqstar_evaluator_declines_on_a_software_world() {
        // The relationship between this runtime and the trading bot, asserted rather than
        // described: the DQN is a specialist that says nothing here.
        let dir = scratch();
        let agent = Agent::new(&dir, None);
        let c = agent
            .cycle_over(world_with(vec![untested()]), Goal::new("g", "raise confidence"))
            .unwrap();
        let status = c
            .decision
            .evaluator_status
            .iter()
            .find(|s| s.evaluator == "dqstar")
            .expect("the evaluator must be listed even when it declines");
        assert!(!status.applicability.is_applicable());
        assert!(c.decision.ranked.iter().all(|r| r.evaluations.is_empty()));
        fs::remove_dir_all(&dir).ok();
    }
}
