//! `scema execute` — carry out one declared effect, and seal what happened.
//!
//! ```text
//! scema execute effect.json                     # dry run: gates only, touches nothing
//! scema execute effect.json --commit            # actually do it, and seal a record
//! scema execute - --commit --intent <record-id> # from stdin, naming the decision
//! ```
//!
//! ## Dry run is the default, and that is the whole design
//!
//! The two paths compute exactly the same thing up to the last step, which is precisely why
//! they must not be the same keystroke — the same reasoning that puts `enter` and `D` on
//! different keys in the console, and `simulate` and `decide` on different verbs. Somebody
//! who has to type `--commit` has said something; somebody who merely ran the command has
//! not.
//!
//! A dry run still runs both gates, so it answers the question an operator actually has:
//! *would this be allowed, and what exactly would it do*. What it will not do is prompt —
//! asking for approval of an act that is not going to happen teaches people the prompt is a
//! formality.
//!
//! ## The effect is declared, never inferred
//!
//! This reads an `Effect` from a file. It does not derive one from a decision record, and
//! that is deliberate rather than unfinished: omni's hypothesisers produce branches like
//! "11 markers in `scema-tools`", which is a *description of work*, not a machine-executable
//! action. Turning one into the other automatically would be the keyword-overlap bug with a
//! much larger blast radius — inference that writes to a disk instead of merely mis-ranking
//! a branch.
//!
//! `--intent` records **which decision this claims to carry out**. It is asserted by the
//! operator, exactly as `--ground` is, and nothing checks that the effect is a sensible way
//! to carry out that decision, because nothing could.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{bail, Context, Result};
use scema_effect::{
    exec::{self, Mode},
    Effect, EffectRecord, Outcome,
};
use scema_tools::Workspace;
use scema_trust::{Approver, AutoApprover, DenyApprover, TrustPolicy};

const RUNTIME: &str = concat!("scema-omni/", env!("CARGO_PKG_VERSION"));

fn read_effect(locator: &str) -> Result<Effect> {
    let text = if locator == "-" {
        let mut s = String::new();
        std::io::stdin().read_to_string(&mut s).context("reading an effect from stdin")?;
        s
    } else {
        std::fs::read_to_string(locator).with_context(|| format!("reading {locator}"))?
    };
    if text.trim().is_empty() {
        bail!("nothing to execute — the effect was empty");
    }
    serde_json::from_str(&text).with_context(|| {
        "parsing an effect. Expected one of: \
         {\"kind\":\"write_file\",\"path\":\"…\",\"contents\":\"…\"}, \
         {\"kind\":\"create_dir\",\"path\":\"…\"}, \
         {\"kind\":\"run\",\"argv\":[\"…\"],\"cwd\":\"…\"}"
    })
}

#[allow(clippy::too_many_arguments)]
pub fn run(
    locator: &str,
    root: &Path,
    allow: Option<&PathBuf>,
    commit: bool,
    intent: Option<&str>,
    allow_writes: bool,
    allow_execute: bool,
    yes: bool,
) -> Result<ExitCode> {
    let effect = read_effect(locator)?;

    let allow_root = match allow {
        Some(p) => p.clone(),
        None => std::env::current_dir().context("resolving the working directory")?,
    };
    let workspace = Workspace::new([&allow_root])?;

    let mut policy = TrustPolicy::new();
    if allow_writes {
        policy = policy.writing();
    }
    if allow_execute {
        policy = policy.executing();
    }

    // No terminal means deny. `--yes` is the explicit opt-out and has to be typed by
    // somebody who meant it; piped input and CI must not read silence as consent.
    let interactive = std::io::IsTerminal::is_terminal(&std::io::stdin());
    let mut deny = DenyApprover;
    let mut auto = AutoApprover;
    let approver: &mut dyn Approver = if yes { &mut auto } else { &mut deny };

    let mode = if commit { Mode::Commit } else { Mode::DryRun };

    println!("EFFECT   {}", effect.summary());
    println!("  risk       {}", effect.risk().as_str());
    println!("  workspace  {}", allow_root.display());
    println!("  mode       {}", if commit { "COMMIT" } else { "dry run" });
    if !commit {
        println!("             nothing will be touched — add --commit to carry it out");
    }
    if !interactive && !yes && !commit {
        // Worth saying up front rather than as a surprise at the end.
        println!("             stdin is not a terminal, so a prompt would be a refusal");
    }

    let outcome = exec::run(&effect, &workspace, &mut policy, approver, mode);

    println!("\nOUTCOME  {}", outcome.label());
    match &outcome {
        Outcome::Succeeded { detail } => println!("  {detail}"),
        Outcome::Failed { reason } => println!("  {reason}"),
        Outcome::Unknown { why } => {
            println!("  {why}");
            println!(
                "\n  Not a failure. The effect was attempted and its result could not be\n  \
                 observed, so the world may or may not have changed — check before retrying."
            );
        }
        Outcome::Refused { by, reason } => println!("  refused by {by:?}: {reason}"),
        Outcome::Simulated => println!("  the gates allowed it; nothing was done"),
    }

    // A dry run seals nothing. A record of an act that did not happen is a record somebody
    // will later read as one that did.
    if !commit {
        return Ok(ExitCode::SUCCESS);
    }

    let at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or_default();
    let record = EffectRecord::seal(RUNTIME, at, intent.unwrap_or(""), effect, outcome.clone());

    let dir = root.join("effects");
    std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
    let path = dir.join(format!("{}.json", record.id));
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_string_pretty(&record)?)?;
    std::fs::rename(&tmp, &path)?;

    println!("\nSEALED   {}", record.id);
    println!("  {}", path.display());
    if intent.is_none() {
        println!(
            "  no --intent: this record does not name a decision it carries out, which is\n  \
             honest but unlinkable. Pass --intent <record-id> to bind them."
        );
    }

    // A refusal is not a crash: a script that treats "the policy said no" as a failure gets
    // rewritten to ignore the exit code, and then it ignores real failures too. An
    // *unobserved* result does exit non-zero, because continuing a sequence past one is the
    // thing that must not happen quietly.
    Ok(match outcome {
        Outcome::Unknown { .. } => ExitCode::from(3),
        Outcome::Failed { .. } => ExitCode::from(1),
        _ => ExitCode::SUCCESS,
    })
}
