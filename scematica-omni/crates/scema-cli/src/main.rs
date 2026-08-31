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
//! `delegate`, `discover` and `pay` are registered and exit non-zero with a statement of
//! what is missing. They are in the help text on purpose: the shape of the runtime includes
//! an agent-to-agent path and a payment path, and an operator should be able to find that
//! out from the tool itself rather than from a README they may not read. A verb that
//! silently did not exist would be indistinguishable from one that failed.
//!
//! `execute` used to be one of them, and is not any more: `scema-trust` answers *whether*
//! and `scema_tools::Workspace` answers *where*, so there is now a gate to put in front of
//! an action. It stays **dry run by default** for the same reason `simulate` and `decide`
//! are different verbs — the two paths compute the same thing up to the last step, and the
//! only thing keeping a rehearsal from reading as an act is that they are not the same
//! keystroke.
//!
//! ## `simulate` versus `decide`
//!
//! `simulate` never persists. It is a counterfactual — "what would this look like" — and a
//! record it left behind would later read as a decision the agent made. `decide` seals a
//! record and appends memory. Both compute exactly the same thing; only the side effects
//! differ, which is why they share one code path with a flag rather than being two.

mod market;
mod anchor;
mod check;
mod execute;
mod nft;
mod quickstart;
mod connect;
mod doctor;
mod launch;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{anyhow, Context, Result};
use clap::{CommandFactory, Parser, Subcommand};
use clap_complete::Shell;
use scema_agent::{Agent, Cycle};
use scema_memory::{MemoryKind, Recall};
// The render rule lives with the types it protects, not with each front end. See
// `scema_policy::render`.
use scema_policy::render;
use scema_verify::{verify, RecordStore};
use scema_world::{Constraint, Goal, WorldFeatures, WorldState};

/// Default state directory, relative to the working directory.
const DEFAULT_ROOT: &str = ".scema";

#[derive(Parser)]
#[command(
    name = "scema",
    version,
    about = "Scematica Omni — an agent runtime with a world model, counterfactual simulation and verifiable decisions",
    long_about = None,
    // The useful next command is otherwise buried in a list of seventeen verbs that all
    // sound equally plausible to somebody who has not read the README.
    after_help = concat!(
        "New here?                scema quickstart
",
        "Writing a producer?      scema check --vocabulary
",
        "Wiring up an assistant?  scema connect --list
",
        "Something not working?   scema doctor",
    ),
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
    /// The loop, narrated, over a directory you already have. Writes nothing.
    ///
    /// The first thing to run. It walks observe -> simulate -> ground -> decide, explaining
    /// each stage's output as it appears, and stops before sealing anything — a tutorial
    /// that writes a decision record on your behalf has taught you the wrong thing about
    /// the one command in here that leaves a trace.
    Quickstart {
        /// The path to walk. Defaults to the working directory.
        #[arg(default_value = ".")]
        path: String,
    },

    /// Perceive an environment and print the world state.
    Observe {
        /// Path to observe.
        #[arg(default_value = ".")]
        locator: String,
        /// Also draw the perceived world here, as a self-contained SVG.
        ///
        /// Opt-in, and it stays opt-in. `observe` writing files because it felt like it
        /// would break a guarantee this runtime states out loud in three places — quickstart
        /// "writes nothing", simulate "writes nothing", and the whole reason `decide` is a
        /// separate keystroke from `simulate`. A perception verb that leaves artefacts on
        /// disk is a perception verb somebody stops trusting.
        #[arg(long)]
        nft: Option<PathBuf>,
        /// Also rasterise it here. Implies the growth; see `scema nft --help` for sizing.
        #[arg(long)]
        nft_png: Option<PathBuf>,
        /// Edge of that PNG in pixels.
        #[arg(long, default_value = "1024")]
        nft_png_size: usize,
        /// Print the domain-agnostic feature vector and its coverage.
        ///
        /// The same twelve numbers a policy evaluator would read, so what the runtime
        /// perceives can be inspected without writing an evaluator to look at it.
        #[arg(long)]
        features: bool,
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
    /// Batch sealed records under one Merkle root, and pin it somewhere else.
    ///
    /// `verify` proves a record was not edited; it does not prove it is the original,
    /// because whoever holds the only copy can seal a different one. This is the half that
    /// batches and issues per-record inclusion proofs. Publishing the root needs a chain and
    /// a key, and nothing here does it — an anchor recorded but never submitted would be the
    /// fabrication the rest of this runtime exists to refuse.
    Anchor {
        /// List batches and where each has been published.
        #[arg(long)]
        list: bool,
        /// Print the inclusion proof for one record id.
        #[arg(long = "proof")]
        proof: Option<String>,
        /// Note a publication that happened: `<chain>=<reference>`. Asserted, not verified.
        #[arg(long)]
        record: Option<String>,
        /// Which batch `--record` refers to, by root prefix.
        #[arg(long)]
        batch: Option<String>,
        /// Verify a proof file, offline.
        #[arg(long)]
        check: Option<PathBuf>,
        /// The Merkle root `--check` should verify against.
        #[arg(long = "root-hash")]
        root_hash: Option<String>,
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
    /// Open the console — the loop as a full-screen terminal application.
    ///
    /// A separate binary (`scema-tui`), handed over to rather than linked in, so that
    /// `cargo install scema-cli` does not drag a terminal stack onto a CI machine whose
    /// only use for this is `scema verify`.
    Tui {
        /// Arguments forwarded verbatim to `scema-tui`.
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Run the local daemon (`scema-omnid`) — loopback HTTP, token-authenticated.
    Daemon {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Run the MCP server (`scema-mcp`) — the loop as tools, over stdio, for a model.
    Mcp {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },

    /// Create the state directory, so the first `decide` is not also the first write.
    Init {
        /// Overwrite an existing `.gitignore` entry rather than leaving it alone.
        #[arg(long)]
        force: bool,
    },

    /// Wire the MCP server into an assistant — Claude Code, Cursor, VS Code, Zed, Codex.
    ///
    /// Prints the exact snippet and where it goes. `--write` merges it, and only into
    /// project-local files: a user-level assistant config is shared by every project, and
    /// editing it on your behalf would mean a tool installed for one repository quietly
    /// gaining the ability to observe all of them.
    Connect {
        /// Which assistant. Omit with `--list` to see them all.
        host: Option<String>,
        /// List every host this knows about.
        #[arg(long)]
        list: bool,
        /// Merge the entry into the project-local config file.
        #[arg(long)]
        write: bool,
        /// The directory the MCP server is confined to. Defaults to the working directory.
        #[arg(long)]
        allow: Option<PathBuf>,
        /// Let the model seal decision records. Off by default, and `omni_decide` is not
        /// even advertised without it.
        #[arg(long)]
        allow_decide: bool,
    },

    /// Check that a world conforms to the contract, and say exactly why if it does not.
    ///
    /// For anyone writing a producer. Omni is domain-agnostic because the observed thing
    /// describes itself in `scema-world`'s vocabulary, which means a producer can be
    /// written in any language — and then nothing but this stands between it and a silent
    /// misread. It runs the importer's own rules, not a friendlier restatement of them, and
    /// reports every problem at once rather than one per fix-and-rerun.
    ///
    /// Exits 1 if the world would be refused, so it drops straight into a producer's CI.
    Check {
        /// A `.json` file, or `-` for stdin.
        #[arg(default_value = "-")]
        locator: String,
        /// Print the domains and entity kinds this build knows, and stop.
        #[arg(long)]
        vocabulary: bool,
    },

    /// Draw a world as a self-contained SVG plate, with token metadata.
    ///
    /// Takes a `WorldState` (what `observe --json` prints, and what every producer emits) or
    /// a sealed record, and writes an SVG that depends on nothing outside itself. The plate
    /// is deterministic — the same world always produces the same bytes, here and in the
    /// browser — so it is a derivative of the record rather than an illustration of it.
    ///
    /// It writes files and nothing else: no minting, no signing, no spend. Where the SVG
    /// goes next is your decision, not this runtime's.
    Nft {
        /// A `.json` world or record, or `-` for stdin.
        #[arg(default_value = "-")]
        locator: String,
        /// Write the SVG here instead of to stdout.
        #[arg(long)]
        out: Option<PathBuf>,
        /// Also write ERC-721 token metadata here.
        #[arg(long)]
        metadata: Option<PathBuf>,
        /// Point the metadata at this image URI instead of inlining the SVG.
        ///
        /// For a deployment that pins the image somewhere content-addressed. Without it the
        /// image is a `data:` URI and the token is complete on its own.
        #[arg(long)]
        image: Option<String>,
        /// Which drawing the metadata's `image` inlines: `svg` (default) or `png`.
        ///
        /// SVG is the default because it is two orders of magnitude smaller and is *the*
        /// drawing rather than a sampling of it. Choose `png` when the consumer cannot
        /// render SVG — several marketplaces and most preview pipelines cannot — and accept
        /// the size: base64 inflates by 4/3, so 1024px is roughly 4 MB of metadata. Trade it
        /// down with `--png-size`, or host the image and pass `--image`, which wins over
        /// both.
        #[arg(long, default_value = "svg", value_parser = ["svg", "png"])]
        image_format: String,
        /// Also write a PNG here, rasterised from the same growth.
        ///
        /// The rasteriser and the PNG encoder are written by hand for the same reason the
        /// rest of this crate is: a library rasteriser antialiases differently from a
        /// browser canvas, and an image that depends on who rendered it is not a derivative
        /// of the record.
        #[arg(long)]
        png: Option<PathBuf>,
        /// Edge of the PNG in pixels. Square, like the SVG's viewBox.
        #[arg(long, default_value = "1024")]
        png_size: usize,
        /// Draw the instrument plate instead of the fractal growth.
        ///
        /// Same data, read a different way: the plate is gauges and a coverage meter, the
        /// growth is the world's shape. Both are byte-identical between Rust and the
        /// browser; the growth is the default.
        #[arg(long)]
        plate: bool,
    },

    /// What is installed, what is wired up, and what is quietly broken. Changes nothing.
    Doctor,

    /// Emit a shell completion script.
    Completions {
        /// bash | zsh | fish | powershell | elvish
        shell: Shell,
    },

    /// Carry out one declared effect, behind both gates, and seal what happened.
    ///
    /// Dry run by default. The two paths compute the same thing up to the last step, which
    /// is exactly why they are not the same keystroke — somebody who typed `--commit` has
    /// said something; somebody who merely ran the command has not.
    ///
    /// The effect is declared in a file, never inferred from a decision: omni's branches
    /// describe work ("11 markers in `scema-tools`"), and turning one into an executable
    /// action automatically would be inference that writes to a disk.
    Execute {
        /// A `.json` effect, or `-` for stdin.
        locator: String,
        /// Carry it out. Without this, nothing is touched and nothing is sealed.
        #[arg(long)]
        commit: bool,
        /// The decision record this effect claims to carry out. Asserted, never inferred.
        #[arg(long)]
        intent: Option<String>,
        /// The directory the effect is confined to. Defaults to the working directory.
        #[arg(long)]
        allow: Option<PathBuf>,
        /// Stop prompting for writes.
        #[arg(long)]
        allow_writes: bool,
        /// Turn shell execution on at all.
        #[arg(long)]
        allow_execute: bool,
        /// Answer every prompt with yes. The explicit opt-out; never a default.
        #[arg(long)]
        yes: bool,
    },
    /// Hand a goal to another agent, on the record.
    ///
    /// Records what was handed off and under what spend authority. It is NOT a contract:
    /// bonded results live on the ScemaDEX rail, which is not in this workspace, so a
    /// specialist that answers badly can be disbelieved and not penalised.
    Delegate {
        /// What the other agent is being asked to do.
        goal: String,
        /// Who is being asked.
        #[arg(long)]
        to: String,
        /// Spend policy. Without one nothing is delegable — an absent policy permits none.
        #[arg(long)]
        policy: Option<PathBuf>,
        /// Budget for this delegation, in the smallest unit.
        #[arg(long)]
        max: Option<u128>,
        #[arg(long, default_value = "lamports")]
        asset: String,
        /// Seal the delegation. Without this nothing is written and nobody is contacted.
        #[arg(long)]
        commit: bool,
    },
    /// What capabilities are on offer, and which this agent is allowed to want.
    ///
    /// Reads a catalogue file (or `-` for stdin) rather than an endpoint, so a relay, a curl
    /// pipeline and a hand-written list are all the same input. Contacts nothing.
    Discover {
        /// Catalogue of offers, or `-` for stdin.
        #[arg(default_value = "-")]
        catalogue: PathBuf,
        /// Spend policy, to mark what may actually be bought.
        #[arg(long)]
        policy: Option<PathBuf>,
    },
    /// Decide whether a spend may happen, and record the decision.
    ///
    /// **It does not settle.** x402 settlement lives in `scematica-protocol`, which depends
    /// on solana-sdk — the pin this workspace exists to keep out. This authorises, seals a
    /// record, and emits a settlement request for something that can pay.
    Pay {
        /// Capability being bought. Matched verbatim against the policy.
        #[arg(long)]
        capability: String,
        /// Who is being paid.
        #[arg(long)]
        to: String,
        /// Amount in the smallest unit. Never a display value.
        #[arg(long)]
        units: u128,
        #[arg(long, default_value = "lamports")]
        asset: String,
        /// Spend policy. Without one nothing is payable.
        #[arg(long)]
        policy: Option<PathBuf>,
        /// Ledger of what has already settled, so the budget is cumulative.
        #[arg(long)]
        ledger: Option<PathBuf>,
        /// The decision this spend serves.
        #[arg(long)]
        intent: Option<String>,
        /// Seal a record and emit the settlement request. Dry run without it.
        #[arg(long)]
        commit: bool,
    },
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

    // An abstention is an answer, and *which* one is the actionable part. Printing the
    // verdict without it is what made a first run read as the tool being broken.
    let next = render::next_steps(&c.world, &c.record.goal, &c.decision);
    if !next.is_empty() {
        println!("
{next}");
    }

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
        Command::Quickstart { path } => quickstart::run(path, &cli.root),
        Command::Check { locator, vocabulary } => {
            if *vocabulary {
                return Ok(check::vocabulary());
            }
            check::run(locator, cli.json)
        }
        Command::Nft { locator, out, metadata, image, image_format, png, png_size, plate } => {
            nft::run(
                locator,
                out.as_ref(),
                metadata.as_ref(),
                image.as_deref(),
                image_format,
                png.as_ref(),
                *png_size,
                *plate,
            )
        }
        Command::Observe { locator, nft, nft_png, nft_png_size, features } => {
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

            if *features {
                print_features(&w);
            }
            draw_perceived(&w, nft.as_ref(), nft_png.as_ref(), *nft_png_size)?;
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

        Command::Anchor { list, proof, record, batch, check, root_hash } => anchor::run(
            &cli.root,
            *list,
            proof.as_deref(),
            record.as_deref(),
            batch.as_deref(),
            check.as_deref(),
            root_hash.as_deref(),
        ),
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

        Command::Tui { args } => launch::run(launch::TUI, args),
        Command::Daemon { args } => launch::run(launch::DAEMON, args),
        Command::Mcp { args } => launch::run(launch::MCP, args),

        Command::Init { force } => {
            let root = &cli.root;
            std::fs::create_dir_all(root.join("decisions"))
                .with_context(|| format!("creating {}", root.join("decisions").display()))?;
            std::fs::create_dir_all(root.join("memory"))
                .with_context(|| format!("creating {}", root.join("memory").display()))?;

            // `.scema/` is machine-local and full of absolute paths, so it is ignored rather
            // than committed. Writing the ignore *inside* the directory rather than editing
            // the project's root `.gitignore` is deliberate: this tool has no business
            // rewriting a file the whole repository shares, and a self-ignoring directory
            // works whatever the project's own ignore rules say.
            let ignore = root.join(".gitignore");
            if !ignore.exists() || *force {
                std::fs::write(
                    &ignore,
                    "# Machine-local. Decision records cite absolute paths and memory is a\n\
                     # per-checkout history; neither is meaningful in somebody else's clone.\n\
                     *\n",
                )
                .with_context(|| format!("writing {}", ignore.display()))?;
            }

            println!("Initialised {}", root.display());
            println!("  decisions/   sealed decision records, one JSON file each");
            println!("  memory/      four append-only JSONL logs");
            println!("  .gitignore   this directory is machine-local");
            println!();
            println!("Nothing has been decided yet. Start with:");
            println!("  scema observe .                        # what is out there");
            println!("  scema simulate \"<goal>\" --ground <id>   # rank branches, write nothing");
            println!("  scema tui                              # the same thing, interactively");
            Ok(ExitCode::SUCCESS)
        }

        Command::Connect { host, list, write, allow, allow_decide } => {
            if *list || host.is_none() {
                println!("Assistants this can wire up:\n");
                for (key, h) in connect::catalogue() {
                    println!(
                        "  {:<15} {:<32} {}",
                        key,
                        h.label,
                        match h.scope {
                            connect::Scope::Project => format!("project: {}", h.project_path),
                            connect::Scope::User =>
                                "user-level (printed, never written)".to_string(),
                        }
                    );
                }
                println!("\n  scema connect <host>            print the snippet and where it goes");
                println!("  scema connect <host> --write    merge it, project-local hosts only");
                return Ok(ExitCode::SUCCESS);
            }

            let key = host.as_deref().unwrap();
            let h = connect::host(key).ok_or_else(|| {
                anyhow!(
                    "unknown host `{key}`. Known: {}",
                    connect::catalogue().keys().cloned().collect::<Vec<_>>().join(", ")
                )
            })?;
            let project = doctor::cwd();
            let allow_path = allow.clone().unwrap_or_else(|| project.clone());
            let text = connect::snippet(h, &allow_path, *allow_decide)?;

            if *write {
                match connect::write(h, &project, &allow_path, *allow_decide) {
                    Ok(connect::Written::Created(p)) => println!("created {}", p.display()),
                    Ok(connect::Written::Merged(p)) => {
                        println!("merged the `scema` entry into {} (nothing else touched)", p.display())
                    }
                    Ok(connect::Written::Unchanged(p)) => {
                        println!("{} already has this exact entry", p.display())
                    }
                    Err(e) => {
                        // Not a hard failure: the snippet is still useful, and the whole
                        // point of refusing a user-level write is that pasting it is the
                        // correct next step rather than a workaround.
                        eprintln!("scema connect: {e:#}\n");
                        println!("{text}");
                        return Ok(ExitCode::from(2));
                    }
                }
            } else {
                match h.scope {
                    connect::Scope::Project => println!("{} — {}\n", h.label, h.project_path),
                    connect::Scope::User => println!("{}\n{}\n", h.label, h.user_hint),
                }
                println!("{text}");
            }
            println!("Then: {}", h.after);
            if !*allow_decide {
                println!(
                    "\nNote: `omni_decide` is not advertised to the model. The server can perceive,\n\
                     simulate, explain and verify; it cannot seal a record. Add --allow-decide if\n\
                     you want that, having decided you want it."
                );
            }
            Ok(ExitCode::SUCCESS)
        }

        Command::Doctor => {
            let project = doctor::cwd();
            let findings = doctor::run(&cli.root, &project);
            if cli.json {
                let rows: Vec<_> = findings
                    .iter()
                    .map(|f| {
                        serde_json::json!({
                            "verdict": format!("{:?}", f.verdict).to_lowercase(),
                            "check": f.check,
                            "detail": f.detail,
                            "fix": f.fix,
                        })
                    })
                    .collect();
                println!("{}", serde_json::to_string_pretty(&rows)?);
            } else {
                println!("scema doctor — {}\n", scema_agent::RUNTIME);
                for f in &findings {
                    println!("  [{}] {:<24} {}", f.verdict.glyph(), f.check, f.detail);
                    if !f.fix.is_empty() {
                        println!("         {:<24} → {}", "", f.fix);
                    }
                }
                println!("\nThis command changes nothing. Every finding names the fix and stops there.");
            }
            // Only a real failure is non-zero. A missing optional console must not fail a
            // pipeline, or the pipeline stops running this.
            Ok(match doctor::worst(&findings) {
                doctor::Verdict::Fail => ExitCode::FAILURE,
                _ => ExitCode::SUCCESS,
            })
        }

        Command::Completions { shell } => {
            let mut cmd = Cli::command();
            let name = cmd.get_name().to_string();
            clap_complete::generate(*shell, &mut cmd, name, &mut std::io::stdout());
            Ok(ExitCode::SUCCESS)
        }

        Command::Execute {
            locator,
            commit,
            intent,
            allow,
            allow_writes,
            allow_execute,
            yes,
        } => execute::run(
            locator,
            &cli.root,
            allow.as_ref(),
            *commit,
            intent.as_deref(),
            *allow_writes,
            *allow_execute,
            *yes,
        ),
        Command::Delegate { goal, to, policy, max, asset, commit } => market::delegate(
            Path::new(&cli.root),
            goal,
            to,
            policy.as_ref(),
            *max,
            asset,
            *commit,
        ),
        Command::Discover { catalogue, policy } => {
            market::discover(catalogue, policy.as_ref())
        }
        Command::Pay {
            capability,
            to,
            units,
            asset,
            policy,
            ledger,
            intent,
            commit,
        } => market::pay(
            Path::new(&cli.root),
            capability,
            to,
            *units,
            asset,
            policy.as_ref(),
            ledger.as_ref(),
            intent.as_deref(),
            *commit,
        ),
    }
}

/// Draw the world that was just perceived, if asked.
///
/// The link between perception and the artefact, and it is a *flag* rather than a default on
/// purpose. `observe` is one of three verbs this runtime advertises as writing nothing, and
/// the separation between "look" and "leave a trace" is the same distinction that keeps
/// `simulate` and `decide` as different keystrokes. Making perception emit files by default
/// would trade that for convenience, once, permanently.
///
/// The plate is drawn from the world as perceived, so its commitment is computed here rather
/// than taken from a record — there is no record yet. That is the honest reading: this
/// picture is of an observation, not of a decision, and `scema nft` on a sealed record is
/// what binds one to a judgement.
fn draw_perceived(
    w: &WorldState,
    svg_out: Option<&PathBuf>,
    png_out: Option<&PathBuf>,
    png_size: usize,
) -> Result<()> {
    if svg_out.is_none() && png_out.is_none() {
        return Ok(());
    }
    let digest = scema_nft::world_digest(w);

    if let Some(p) = svg_out {
        let svg = scema_nft::render_svg(w, &digest);
        std::fs::write(p, &svg).with_context(|| format!("writing {}", p.display()))?;
        eprintln!("wrote {} ({} bytes)", p.display(), svg.len());
    }
    if let Some(p) = png_out {
        let bytes = scema_nft::fractal::render_png(w, &digest, png_size);
        std::fs::write(p, &bytes).with_context(|| format!("writing {}", p.display()))?;
        eprintln!("wrote {} ({} bytes, {png_size}x{png_size})", p.display(), bytes.len());
    }
    eprintln!("world commitment {digest}");
    Ok(())
}

/// The domain-agnostic feature vector, rendered under the one rule that matters.
///
/// An unmeasured feature prints `—`, never `0.000`, and the coverage is on the same screen as
/// the numbers. Both are the same requirement `scema_policy::render::cell` enforces; this
/// prints `Term`s directly because they are not a scored aggregate.
fn print_features(w: &WorldState) {
    let f = WorldFeatures::of(w);
    let c = f.coverage();
    println!("FEATURES  {} measured of {}", c.measured, c.total);
    for (name, t) in WorldFeatures::names().iter().zip(f.terms()) {
        let shown = if t.measured { format!("{:.3}", t.value) } else { "—".to_string() };
        println!("  {name:<20} {shown:>7}   {}", t.note);
    }
    println!();
    println!("  A consumer reading only the values cannot tell a substituted neutral from");
    println!("  a measurement — which is why the coverage above is not optional, and why");
    println!("  `WorldFeatures::to_vec_with_mask` exists for anything that learns here.");
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
