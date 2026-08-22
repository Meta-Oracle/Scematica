//! `scema quickstart` — the loop, narrated, over a directory you already have.
//!
//! ## Why a guided run rather than more documentation
//!
//! The README explains the design well and it is long, because the design is the product.
//! But the thing a newcomer needs first is not the argument for `Provenance` — it is
//! twenty seconds of evidence that this does something, on their own code, with the four
//! pieces of vocabulary that make the output readable.
//!
//! Those four are the ones that make every other line make sense, and each of them is a
//! place where the honest output looks like a malfunction if you do not have the word for
//! it yet: a signal is a **count**, `—` means **unmeasured** and never zero, **grounding**
//! is asserted and never inferred, and **abstention is an answer**.
//!
//! ## It writes nothing
//!
//! Deliberately, and the last step says so and stops. A tutorial that seals a decision
//! record on someone's behalf has taught them that `decide` is a thing that happens *to*
//! them, on the one command in this runtime that leaves a trace. The whole design rests on
//! `simulate` and `decide` computing exactly the same thing and differing only in whether
//! they wrote — the same reason the console needs two different keystrokes for them.
//!
//! So this runs the read-only half and prints the command for the other half.

use std::io::IsTerminal;
use std::process::ExitCode;

use anyhow::Result;
use scema_agent::Agent;
use scema_policy::render;
use scema_world::{Goal, WorldState};

/// Whether to emit the two escape sequences this module uses.
///
/// Colour is decoration and never the message, the same rule the console and
/// `alchem_link.theme` hold to: piping this into a file has to produce the same words.
fn styled() -> bool {
    std::env::var_os("NO_COLOR").is_none() && std::io::stdout().is_terminal()
}

fn rule(n: u8, title: &str) {
    if styled() {
        println!("\n\x1b[1m{n}. {title}\x1b[0m");
    } else {
        println!("\n{n}. {title}");
    }
    println!("{}", "─".repeat(78));
}

fn bold(text: &str) {
    if styled() {
        println!("\x1b[1m{text}\x1b[0m");
    } else {
        println!("{text}");
    }
}

/// Print an indented block, preserving the shape the source literal has.
///
/// Dedents by the block's common leading whitespace rather than trimming each line, so a
/// hanging indent under a bullet survives. Trimming line by line collapsed the three-bullet
/// list in step 1 into a wall of text, which undoes the only reason it is a list.
fn para(text: &str) {
    let lines: Vec<&str> = text.trim_matches('\n').lines().collect();
    let indent = lines
        .iter()
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.len() - l.trim_start().len())
        .min()
        .unwrap_or(0);
    for line in lines {
        if line.trim().is_empty() {
            println!();
        } else {
            println!("  {}", &line[indent..]);
        }
    }
}

/// Pick the signal a newcomer should ground against.
///
/// The largest counted one — not because magnitude is importance, but because it is the
/// example most likely to produce a visibly different ranking, and the point here is to
/// show that `--ground` *changes the answer* rather than to recommend a course of action.
fn best_signal(w: &WorldState) -> Option<&scema_world::Signal> {
    w.signals
        .iter()
        .filter(|s| s.measured)
        .max_by(|a, b| a.magnitude.total_cmp(&b.magnitude))
}

pub fn run(path: &str, root: &std::path::Path) -> Result<ExitCode> {
    let agent = {
        let mut a = Agent::new(root.to_path_buf(), None);
        a.persist = false; // Nothing here writes. See the module note.
        a
    };

    bold("SCEMA OMNI — QUICKSTART");
    para(
        "
        An agent runtime: it perceives an environment, proposes competing futures, projects
        each one, ranks them under a stated preference, and decides — or refuses to.

        This runs the read-only half of that loop and explains each stage as it goes.
        It writes nothing.
        ",
    );
    println!("\n  path: {path}");

    // ── 1 ───────────────────────────────────────────────────────────────────────────
    rule(1, "OBSERVE — what is actually there, and what could not be read");
    let world = match agent.observe(path) {
        Ok(w) => w,
        Err(e) => {
            println!("\n  Could not observe `{path}`:\n    {e:#}\n");
            para(
                "
                Try a directory you can read, e.g. `scema quickstart .` from inside a
                project. A world can also be produced by something else entirely and piped
                in — see `scema check --vocabulary`.
                ",
            );
            return Ok(ExitCode::FAILURE);
        }
    };
    println!();
    println!("{}", render::world_header(&world));
    println!();
    println!("{}", render::signals_capped(&world, 8));
    println!();
    para(
        "
        A `WorldState`. Three things about it are the whole design:

        · A signal is a COUNT, not an opinion. `counted` means something was actually
          tallied, and the evidence line says what. A producer that claims a count it
          did not take is refused on import.

        · What could not be read is a BLIND SPOT, never a zero. \"We could not see this\"
          and \"this is empty\" are different claims, and only one is an accusation.

        · The extent says how much was seen. An unknown denominator stays unknown rather
          than being rounded up to \"all of it\".
        ",
    );
    if !world.blind_spots.is_empty() {
        println!("\n  blind spots here: {}", world.blind_spots.len());
    }

    // ── 2 ───────────────────────────────────────────────────────────────────────────
    rule(2, "SIMULATE — rank competing futures against a goal");
    let goal_text = "reduce risk in this project";
    println!("\n  $ scema simulate \"{goal_text}\"\n");
    let plain = agent.cycle_over(world.clone(), Goal::new("goal", goal_text))?;
    print!("{}", render::matrix(&plain.decision, &plain.projections));
    println!();
    println!("{}", render::verdict(&plain.decision));
    let next = render::next_steps(&plain.world, &plain.record.goal, &plain.decision);
    if !next.is_empty() {
        println!("\n{next}");
    }
    println!();
    para(
        "
        Read the MEASURED column before the UTILITY column. A score computed over two terms
        out of nine is a statement about ignorance, and an em dash is not a zero — it is a
        term that contributed nothing because nothing was observed to put in it.
        ",
    );

    // ── 3 ───────────────────────────────────────────────────────────────────────────
    rule(3, "GROUND — an instruction is not evidence");
    match best_signal(&world) {
        Some(sig) => {
            let id = sig.id.clone();
            para(
                "
                The branch above that is just \"what you typed\" has no expected gain, because
                nothing observed supports it. That is not the runtime being unhelpful. It is
                the one rule everything else rests on: a goal is an instruction, and an
                instruction is not evidence.

                Nothing infers the link. An earlier version did, by keyword overlap, and
                immediately grounded a goal in an unrelated crate that shared a substring of
                its name. You assert it, by signal id:
                ",
            );
            println!("\n  $ scema simulate \"{goal_text}\" --ground {id}\n");
            let mut g = Goal::new("goal", goal_text);
            g = g.grounded(id.clone());
            let grounded = agent.cycle_over(world.clone(), g)?;
            print!("{}", render::matrix(&grounded.decision, &grounded.projections));
            println!();
            println!("{}", render::verdict(&grounded.decision));
            println!();
            para(
                "
                Same world, same weights, different answer — because the claim is now
                attached to something that was counted. You made that claim, and it is
                recorded as yours.
                ",
            );
        }
        None => {
            para(
                "
                Nothing counted was observed here, so there is nothing to ground a goal in
                and every branch abstains. That is the correct answer for this directory
                rather than a failure — try a source tree with tests and TODOs in it.
                ",
            );
        }
    }

    // ── 4 ───────────────────────────────────────────────────────────────────────────
    rule(4, "DECIDE — the one command that leaves a trace");
    para(
        "
        `simulate` and `decide` compute exactly the same thing. The only difference is that
        `decide` seals a decision record: the world it saw (blind spots included), the
        branches, the projections, the weights, the outcome, and six SHA-256 digests over
        all of it. Anybody can re-check it later without having been there.

        Quickstart stops here on purpose, because writing on your behalf is the one thing a
        tutorial should not do. When you want the record:
        ",
    );
    println!("\n  $ scema decide \"{goal_text}\" --ground <signal-id>");
    println!("  $ scema explain --list");
    println!("  $ scema verify --all");
    println!();
    para(
        "
        What `verify` proves: the record was not edited after sealing, and it names the
        field that moved. What it does NOT prove: that the world was really like that
        (provenance carries that), or that this is the original record (tamper-evident, not
        tamper-proof, until the root is anchored somewhere you do not control).
        ",
    );

    // ── 5 ───────────────────────────────────────────────────────────────────────────
    rule(5, "WHERE TO GO NEXT");
    println!();
    println!("  scema tui                    the console — the same loop, five tabs");
    println!("  scema observe <path>         the world on its own, with every signal id");
    println!("  scema policy                 the weights, and which specialists are loaded");
    println!("  scema doctor                 what is installed, wired, or quietly broken");
    println!("  scema connect --list         wire it into Claude Code, Cursor, VS Code, Zed");
    println!("  scema check --vocabulary     write your own producer, in any language");
    println!();
    para(
        "
        A repository is only one kind of world. A running system, a set of oracle feeds and
        a web page are all `WorldState` too, and nothing above perception can tell which it
        was looking at — that is what `scema check` is for.
        ",
    );
    println!();

    Ok(ExitCode::SUCCESS)
}
