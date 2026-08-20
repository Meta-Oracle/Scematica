//! # `scema` — Scematica Omni from the terminal
//!
//! One binary over the whole loop. The verbs are the stages, so what the runtime does is
//! legible from `--help` rather than from a diagram:
//!
//! ```text
//!   scema observe .                          perceive an environment
//!   scema simulate "add tests to scema-cli"  rank branches, write nothing
//!   scema decide   "add tests to scema-cli"  rank branches, seal a record
//!   scema explain  8f92a1c4                  why that decision came out that way
//!   scema verify   8f92a1c4                  recompute the commitment
//!   scema remember --stats                   what the agent has retained
//!   scema policy                             the weights and the specialists
//! ```
//!
//! ## The verbs that exist and refuse
//!
//! `execute`, `delegate`, `discover` and `pay` are registered and exit non-zero with a
//! statement of what is missing. They are in the help text on purpose: the shape of the
//! runtime includes an action path, an agent-to-agent path and a payment path, and an
//! operator should be able to find out from the tool itself that those are not built rather
//! than from a README they may not read. A verb that silently did not exist would be
//! indistinguishable from one that failed.
//!
//! ## `simulate` versus `decide`
//!
//! `simulate` never persists. It is a counterfactual — "what would this look like" — and a
//! record it left behind would later read as a decision the agent made. `decide` seals a
//! record and appends memory. Both compute exactly the same thing; only the side effects
//! differ, which is why they share one code path with a flag rather than being two.

use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{anyhow, Context, Result};
use clap::{Parser, Subcommand};
use scema_agent::{Agent, Cycle};
use scema_memory::{MemoryKind, Recall};
// The render rule lives with the types it protects, not with each front end. See
// `scema_policy::render`.
use scema_policy::render;
use scema_verify::{verify, RecordStore};
use scema_world::{Constraint, Goal};

/// Default state directory, relative to the working directory.
const DEFAULT_ROOT: &str = ".scema";

#[derive(Parser)]
#[command(
    name = "scema",
    version,
    about = "Scematica Omni — an agent runtime with a world model, counterfactual simulation and verifiable decisions",
    long_about = None
)]
struct Cli {
    /// State directory for decision records and memory.
    #[arg(long, global = true, default_value = DEFAULT_ROOT)]
    root: PathBuf,

    /// Deep Q* checkpoint (the sniper's `scematica-nn-agent.json`), for trading worlds.
    #[arg(long, global = true)]
    dqstar: Option<String>,

    /// Emit JSON instead of a rendered report.
    #[arg(long, global = true)]
    json: bool,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Perceive an environment and print the world state.
    Observe {
        /// Path to observe.
        #[arg(default_value = ".")]
        locator: String,
    },
    /// Rank competing branches against a goal. Writes nothing.
    Simulate {
        goal: String,
        #[arg(long, default_value = ".")]
        path: String,
        /// A thing the agent must not touch. Repeatable. Format: `subject[:detail]`.
        #[arg(long = "must-not")]
        must_not: Vec<String>,
        /// Assert that this goal addresses a counted signal, by id. Repeatable.
        ///
        /// Nothing infers this. Without it the goal branch is ungrounded, scores at or
        /// below zero, and the agent abstains — which is the honest answer to an
        /// instruction the observed world says nothing about. Run `scema observe` to see
        /// the signal ids.
        #[arg(long = "ground")]
        ground: Vec<String>,
        /// Show the failure modes of the top branch.
        #[arg(long)]
        failures: bool,
    },
    /// Rank branches, choose or abstain, and seal a decision record.
    Decide {
        goal: String,
        #[arg(long, default_value = ".")]
        path: String,
        #[arg(long = "must-not")]
        must_not: Vec<String>,
        /// Assert that this goal addresses a counted signal, by id. Repeatable.
        #[arg(long = "ground")]
        ground: Vec<String>,
    },
    /// Everything `decide` does, with the full narration.
    Mission {
        goal: String,
        #[arg(long, default_value = ".")]
        path: String,
        #[arg(long = "must-not")]
        must_not: Vec<String>,
        /// Assert that this goal addresses a counted signal, by id. Repeatable.
        #[arg(long = "ground")]
        ground: Vec<String>,
    },
    /// Re-read a sealed decision.
    Explain {
        /// Record id, or any unique prefix.
        id: Option<String>,
        /// List known records instead.
        #[arg(long)]
        list: bool,
    },
    /// Recompute a record's commitment and report what moved.
    Verify {
        /// Record id or unique prefix.
        id: Option<String>,
        /// Verify a record file directly, wherever it is.
        #[arg(long)]
        file: Option<PathBuf>,
        /// Verify every record in the store.
        #[arg(long)]
        all: bool,
    },
    /// What the agent has retained.
    Remember {
        /// Per-kind counts and projection calibration.
        #[arg(long)]
        stats: bool,
        /// Recall records whose subject contains this.
        #[arg(long)]
        about: Option<String>,
        #[arg(long, default_value = "10")]
        limit: usize,
    },
    /// The utility weights and the registered specialists.
    Policy,
    /// Not implemented: carry out a chosen action.
    Execute,
    /// Not implemented: hire another agent.
    Delegate,
    /// Not implemented: find purchasable capabilities.
    Discover,
    /// Not implemented: settle for a capability over x402.
    Pay,
}

fn parse_constraints(specs: &[String]) -> Vec<Constraint> {
    specs
        .iter()
        .filter_map(|s| {
            let (subject, detail) = match s.split_once(':') {
                Some((a, b)) => (a.trim(), b.trim()),
                None => (s.trim(), "declared on the command line"),
            };
            // An empty subject would forbid everything by substring match. Dropping it is
            // safer than the alternative, and the warning says so rather than failing
            // silently.
            if subject.is_empty() {
                eprintln!("scema: ignoring an empty --must-not (an empty subject would forbid every branch)");
                return None;
            }
            Some(Constraint::must_not(subject, detail))
        })
        .collect()
}

fn build_goal(statement: &str, must_not: &[String], ground: &[String]) -> Goal {
    let mut g = Goal::new("goal", statement);
    for c in parse_constraints(must_not) {
        g = g.with_constraint(c);
    }
    for id in ground {
        g = g.grounded(id.trim());
    }
    g
}

/// Warn about `--ground` ids that name no signal in the observed world.
///
/// The simulator drops them, so this is not a correctness issue — it is a usability one.
/// A typo in a signal id otherwise produces a silent abstention that looks like the agent
/// disagreeing rather than the operator mistyping.
fn warn_dangling_grounds(world: &scema_world::WorldState, goal: &Goal) {
    for id in &goal.grounded_in {
        if !world.signals.iter().any(|s| &s.id == id) {
            eprintln!(
                "scema: --ground `{id}` names no signal in this world; it will be ignored.                  Run `scema observe` for the ids."
            );
        }
    }
}

fn print_cycle(c: &Cycle, json: bool, failures: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(&c.record)?);
        return Ok(());
    }
    println!("{}\n", render::world_header(&c.world));
    println!("{}\n", render::signals(&c.world));
    print!("{}", render::matrix(&c.decision, &c.projections));
    println!();
    println!("{}\n", render::evaluators(&c.decision));
    println!("{}", render::verdict(&c.decision));

    if failures {
        if let Some(top) = c.decision.ranked.first() {
            if let Some(p) = c.projections.iter().find(|p| p.hypothesis == top.hypothesis) {
                let text = render::failure_modes(p);
                if !text.is_empty() {
                    println!("\n{text}");
                }
            }
        }
    }

    match &c.record_path {
        Some(p) => println!(
            "\nRECORD    {}  ({})\n          {} memory record(s) appended",
            c.record.id,
            p.display(),
            c.remembered
        ),
        None => println!(
            "\nRECORD    not written — `simulate` is a counterfactual and leaves no trace.\n          Run `scema decide` to seal this as {}.",
            c.record.id
        ),
    }
    Ok(())
}

fn run(cli: Cli) -> Result<ExitCode> {
    let agent_for = |persist: bool| {
        let mut a = Agent::new(cli.root.clone(), cli.dqstar.clone());
        a.persist = persist;
        a
    };

    match &cli.command {
        Command::Observe { locator } => {
            let agent = agent_for(false);
            let w = agent.observe(locator)?;
            if cli.json {
                println!("{}", serde_json::to_string_pretty(&w)?);
            } else {
                println!("{}\n", render::world_header(&w));
                println!("{}\n", render::signals(&w));
                println!("OBJECTS  {}", w.objects.len());
                for o in w.objects.iter().take(20) {
                    let attrs: Vec<String> = o
                        .attrs
                        .iter()
                        .map(|(k, v)| format!("{k}={}", v.render()))
                        .collect();
                    println!(
                        "  {:<10} {:<24} {}",
                        o.provenance.label(),
                        o.label,
                        if attrs.is_empty() {
                            "(no values — unseen, not empty)".to_string()
                        } else {
                            attrs.join(" ")
                        }
                    );
                }
                if w.objects.len() > 20 {
                    println!("  … {} more", w.objects.len() - 20);
                }
            }
            Ok(ExitCode::SUCCESS)
        }

        Command::Simulate { goal, path, must_not, ground, failures } => {
            let agent = agent_for(false);
            let world = agent.observe(path)?;
            let goal = build_goal(goal, must_not, ground);
            warn_dangling_grounds(&world, &goal);
            let c = agent.cycle_over(world, goal)?;
            print_cycle(&c, cli.json, *failures)?;
            Ok(ExitCode::SUCCESS)
        }

        Command::Decide { goal, path, must_not, ground }
        | Command::Mission { goal, path, must_not, ground } => {
            let agent = agent_for(true);
            let world = agent.observe(path)?;
            let goal = build_goal(goal, must_not, ground);
            warn_dangling_grounds(&world, &goal);
            let c = agent.cycle_over(world, goal)?;
            let narrate = matches!(cli.command, Command::Mission { .. });
            print_cycle(&c, cli.json, narrate)?;
            // A decision that abstained is not a failure of the program, and must not exit
            // non-zero: a script that treats "the agent declined" as a crash will be
            // rewritten to ignore the exit code, and then it will ignore real crashes too.
            Ok(ExitCode::SUCCESS)
        }

        Command::Explain { id, list } => {
            let store = RecordStore::new(cli.root.clone());
            if *list || id.is_none() {
                let ids = store.ids()?;
                if ids.is_empty() {
                    println!("No decision records under {}.", cli.root.display());
                    println!("Run `scema decide \"<goal>\"` to seal one.");
                    return Ok(ExitCode::SUCCESS);
                }
                println!("{} record(s), newest first:", ids.len());
                for id in ids {
                    match store.load(&id) {
                        Ok(r) => println!(
                            "  {}  {:<40}  {}",
                            r.id,
                            {
                                let s = r.goal.statement.clone();
                                if s.chars().count() > 40 {
                                    s.chars().take(39).collect::<String>() + "…"
                                } else {
                                    s
                                }
                            },
                            match (&r.decision.chosen, &r.decision.abstention) {
                                (Some(c), _) => format!("chose {c}"),
                                (None, Some(a)) => format!("abstained — {}", a.headline()),
                                _ => "—".into(),
                            }
                        ),
                        // An unreadable record still gets a line. Hiding it would make a
                        // corrupt store look like a smaller one.
                        Err(e) => println!("  {id}  <unreadable: {e}>"),
                    }
                }
                return Ok(ExitCode::SUCCESS);
            }

            let record = store.load(id.as_ref().unwrap())?;
            if cli.json {
                println!("{}", serde_json::to_string_pretty(&record)?);
                return Ok(ExitCode::SUCCESS);
            }
            println!("RECORD    {}  runtime {}", record.id, record.runtime);
            println!("GOAL      {}", record.goal.statement);
            for c in &record.goal.constraints {
                println!("          constraint {:?} `{}` — {}", c.kind, c.subject, c.detail);
            }
            println!();
            println!("{}\n", render::world_header(&record.world));
            print!("{}", render::matrix(&record.decision, &record.projections));
            println!();
            println!("{}\n", render::evaluators(&record.decision));
            println!("{}", render::verdict(&record.decision));
            let v = verify(&record);
            println!(
                "\nCOMMITMENT {}\n           root {}",
                if v.valid { "VALID — the record matches its commitment" } else { "INVALID" },
                record.commitment.root
            );
            Ok(ExitCode::SUCCESS)
        }

        Command::Verify { id, file, all } => {
            let store = RecordStore::new(cli.root.clone());
            let records = if let Some(f) = file {
                vec![RecordStore::load_path(f).with_context(|| format!("reading {}", f.display()))?]
            } else if *all {
                store.ids()?.iter().filter_map(|i| store.load(i).ok()).collect()
            } else {
                let id = id
                    .as_ref()
                    .ok_or_else(|| anyhow!("give a record id, --file, or --all"))?;
                vec![store.load(id)?]
            };
            if records.is_empty() {
                println!("Nothing to verify under {}.", cli.root.display());
                return Ok(ExitCode::SUCCESS);
            }

            let results: Vec<_> = records.iter().map(verify).collect();
            if cli.json {
                println!("{}", serde_json::to_string_pretty(&results)?);
            } else {
                for v in &results {
                    println!("{}  {}", v.id, if v.valid { "VALID" } else { "INVALID" });
                    for m in &v.mismatches {
                        println!(
                            "    {:<12} committed {}…  recomputed {}…",
                            m.field,
                            &m.committed[..m.committed.len().min(12)],
                            &m.recomputed[..m.recomputed.len().min(12)]
                        );
                    }
                    if v.root_only {
                        println!("    every part verifies but the root does not — the root was edited on its own");
                    }
                }
                println!(
                    "\nThis proves the record was not edited after sealing. It does NOT prove the\nworld was as described — provenance carries that, not the digest."
                );
            }
            if results.iter().all(|v| v.valid) {
                Ok(ExitCode::SUCCESS)
            } else {
                Ok(ExitCode::FAILURE)
            }
        }

        Command::Remember { stats, about, limit } => {
            let agent = agent_for(false);
            let mem = agent.memory();
            if *stats || about.is_none() {
                let counts = mem.counts()?;
                println!("MEMORY   {}", mem.root().join("memory").display());
                for (kind, n, corrupt) in counts {
                    println!(
                        "  {:<16} {:>6} record(s){}",
                        format!("{kind:?}"),
                        n,
                        if corrupt > 0 { format!("   {corrupt} unreadable line(s)") } else { String::new() }
                    );
                }
                let c = mem.calibration()?;
                println!("\nCALIBRATION");
                println!("  branches not taken, recorded   {}", c.recorded);
                println!("  of those, later resolved       {}", c.resolved);
                println!("  unresolved                     {}", c.unresolved);
                match c.mean_abs_error {
                    Some(e) => println!("  mean |projected − realised|    {e:.3}"),
                    None => println!(
                        "  mean |projected − realised|    — (nothing resolved; a branch nobody ran has no outcome)"
                    ),
                }
                return Ok(ExitCode::SUCCESS);
            }
            let query = Recall {
                subject: about.clone(),
                limit: Some(*limit),
                ..Default::default()
            };
            for kind in MemoryKind::all() {
                let hits = mem.recall(kind, &query)?;
                if hits.is_empty() {
                    continue;
                }
                println!("{kind:?}");
                for h in hits {
                    println!("  {}  {}  {}", h.id, h.subject, serde_json::to_string(&h.body)?);
                }
            }
            Ok(ExitCode::SUCCESS)
        }

        Command::Policy => {
            let agent = agent_for(false);
            let w = agent.config.weights;
            println!("UTILITY   U = R − λ₁K − λ₂C − λ₃U + λ₄V");
            println!("  λ₁ risk           {:.2}", w.risk);
            println!("  λ₂ cost           {:.2}", w.cost);
            println!("  λ₃ uncertainty    {:.2}", w.uncertainty);
            println!("  λ₄ reversibility  {:.2}", w.reversibility);
            println!("\n  These are a stated preference, not a fitted parameter. They are hashed");
            println!("  into every record so a ranking can be re-read against them later.");
            println!("\nGATES");
            println!("  min measured fraction  {:.0}%", agent.config.min_coverage * 100.0);
            println!("  specialist veto at     ≤ {:.2}", agent.config.veto_at_or_below);
            println!("\nOBSERVERS");
            for o in agent.observers() {
                println!("  {:<10} {}", o.name(), o.about());
            }
            println!("\nEVALUATORS");
            for e in agent.evaluators() {
                println!("  {:<10} {}", e.name(), e.about());
            }
            Ok(ExitCode::SUCCESS)
        }

        Command::Execute => not_built(
            "execute",
            "Nothing in this workspace writes to an environment it observed. An action path \
             needs the approval model from `alchem-link` — risk declared per tool, no \
             terminal means deny, secrets refused before the prompt — wired in front of it.",
        ),
        Command::Delegate => not_built(
            "delegate",
            "Agent-to-agent hiring runs over the ScemaDEX relay and needs a bonded result \
             format, so a specialist that answers badly can be slashed rather than merely \
             disbelieved.",
        ),
        Command::Discover => not_built(
            "discover",
            "Capability discovery needs the relay's catalogue endpoint and a policy for \
             which capabilities this agent is allowed to want.",
        ),
        Command::Pay => not_built(
            "pay",
            "x402 settlement exists in `scematica-protocol`, but paying on the agent's own \
             initiative needs a spend policy first. A runtime that can spend without one is \
             a runtime nobody should install.",
        ),
    }
}

fn not_built(verb: &str, why: &str) -> Result<ExitCode> {
    eprintln!("scema {verb}: not built yet.\n");
    eprintln!("  {why}");
    eprintln!("\n  It is listed in `--help` on purpose: the shape of this runtime includes");
    eprintln!("  this verb, and finding that out from the tool beats finding it out later.");
    Ok(ExitCode::from(2))
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(cli) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("scema: {e:#}");
            ExitCode::FAILURE
        }
    }
}
