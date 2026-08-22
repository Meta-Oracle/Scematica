//! `scema check` — does this JSON conform to the world contract, and if not, exactly why.
//!
//! The surface a producer author works against. Omni's claim to be domain-agnostic rests on
//! the observed thing describing itself in `scema-world`'s vocabulary, which means anybody
//! can write a producer, in any language, and the only feedback they will ever get is what
//! this prints. Three of the four producers in this repository are already hand-written
//! against a JSON shape with no compiler between them and it; going public multiplies that
//! by however many people try.
//!
//! Two properties do the work:
//!
//! * **It is the importer's own rules**, from `scema_tools::conform`, not a friendlier
//!   restatement of them. A checker that disagreed with the importer in either direction —
//!   passing something that is then refused, or refusing something that would import — is
//!   worse than no checker, because it teaches an author that the tooling is unreliable and
//!   to route around it.
//! * **It reports everything at once.** Fixing four problems should take one run, not four
//!   releases.
//!
//! It deliberately does **not** stamp the `imported:` prefix or otherwise touch the world:
//! this is a lint over the producer's own bytes, and rewriting them before printing a
//! verdict would mean reporting on something the producer did not emit.

use std::io::Read;
use std::path::Path;
use std::process::ExitCode;

use anyhow::{bail, Context, Result};
use scema_tools::conform::{conform, Finding, Level};
use scema_world::{Domain, EntityKind, WorldState, WORLD_SCHEMA};

/// Read a world from a path or `-`, without validating it.
///
/// Separate from `ImportObserver` on purpose. The importer refuses a non-conforming world,
/// which is right for the read path and useless here: the whole job is to *describe* what
/// is wrong with one.
fn read(locator: &str) -> Result<(String, String)> {
    if locator == "-" || locator.eq_ignore_ascii_case("stdin") {
        let mut text = String::new();
        std::io::stdin().read_to_string(&mut text).context("reading a world from stdin")?;
        if text.trim().is_empty() {
            bail!(
                "nothing arrived on stdin. A producer that printed its help text or failed \
                 silently looks exactly like this — check its exit code."
            );
        }
        return Ok((text, "stdin".into()));
    }
    let p = Path::new(locator);
    let text = std::fs::read_to_string(p)
        .with_context(|| format!("reading {}", p.display()))?;
    Ok((text, p.display().to_string()))
}

/// Print the vocabulary this build knows.
///
/// Both lists are open, so this is a menu rather than a constraint — but an author who can
/// read the menu picks an existing name instead of coining a synonym for it, and synonyms
/// are the one drift an open vocabulary cannot repair.
pub fn vocabulary() -> ExitCode {
    println!("CONTRACT  {WORLD_SCHEMA}\n");
    println!("  Both vocabularies are open: a name that is not listed is legal and is carried");
    println!("  through untouched. Prefer a listed name where one fits — this build cannot tell");
    println!("  `k8s` from `kubernetes`, and nothing downstream can either.\n");
    println!("DOMAINS");
    for d in Domain::KNOWN {
        let rev = Domain::parse(d).edit_reversibility();
        println!("  {d:<16}  undoing an edit here: {rev:?}");
    }
    println!("\n  Anything else            undoing an edit here: Unknown (an unmeasured term)");
    println!("\nENTITY KINDS");
    for k in EntityKind::KNOWN {
        println!("  {k}");
    }
    ExitCode::SUCCESS
}

fn print_finding(f: &Finding) {
    println!("  {:<5} {:<28} {}", f.level.as_str(), f.code, f.message);
    if let Some(fix) = &f.fix {
        println!("        {:<28} fix: {fix}", "");
    }
}

/// Check one world and report.
///
/// Exit code 1 on any failure, so this drops into a producer's CI without needing its
/// output parsed. Warnings never fail the run: an unfamiliar domain is the open vocabulary
/// working as designed, and a check that failed on it would push authors back onto
/// `unknown`, which is the thing opening it was meant to stop.
pub fn run(locator: &str, json: bool) -> Result<ExitCode> {
    let (text, source) = read(locator)?;

    let world: WorldState = match serde_json::from_str(&text) {
        Ok(w) => w,
        Err(e) => {
            // A parse failure is not a conformance finding — nothing was read well enough
            // to have findings about. Say which it is rather than blurring the two.
            if json {
                println!(
                    "{}",
                    serde_json::json!({
                        "source": source,
                        "parsed": false,
                        "error": e.to_string(),
                    })
                );
            } else {
                println!("{source}\n");
                println!("  FAIL  parse                        {e}");
                println!(
                    "        {:<28} fix: the document must be a scema-world WorldState; run \
                     `scema check --vocabulary`",
                    ""
                );
                println!("\n  Not a world. Nothing else could be checked.");
            }
            return Ok(ExitCode::FAILURE);
        }
    };

    let findings = conform(&world);
    let fails = findings.iter().filter(|f| f.level == Level::Fail).count();
    let warns = findings.iter().filter(|f| f.level == Level::Warn).count();

    if json {
        let items: Vec<_> = findings
            .iter()
            .map(|f| {
                serde_json::json!({
                    "level": f.level.as_str(),
                    "code": f.code,
                    "message": f.message,
                    "fix": f.fix,
                })
            })
            .collect();
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "source": source,
                "parsed": true,
                "conforms": fails == 0,
                "failures": fails,
                "warnings": warns,
                "findings": items,
            }))?
        );
        return Ok(if fails == 0 { ExitCode::SUCCESS } else { ExitCode::FAILURE });
    }

    println!("{source}\n");
    // Failures first: the reader's next action is at the top rather than under a page of
    // notes about a world that is not going to import.
    for level in [Level::Fail, Level::Warn, Level::Note] {
        for f in findings.iter().filter(|f| f.level == level) {
            print_finding(f);
        }
    }

    println!();
    match (fails, warns) {
        (0, 0) => println!("  Conforms to {WORLD_SCHEMA}. This world would import."),
        (0, w) => println!(
            "  Conforms to {WORLD_SCHEMA}, with {w} warning(s). This world would import."
        ),
        (f, _) => println!(
            "  {f} failure(s). This world would be refused on import.\n\n  \
             Nothing here checks the *claims* — a producer reporting a stale reading as \
             live,\n  or counting something it did not count, passes every rule above. That \
             is what\n  the `imported:` prefix on the observer is for."
        ),
    }

    Ok(if fails == 0 { ExitCode::SUCCESS } else { ExitCode::FAILURE })
}
