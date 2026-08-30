//! Carrying out an effect, behind both gates, and recording what actually happened.
//!
//! The order is fixed and each step can only ever narrow what follows:
//!
//! 1. **Where** — `scema_tools::Workspace` resolves the path, or refuses it.
//! 2. **Whether** — `scema_trust::TrustPolicy` preflights; if it has no answer, the
//!    [`Approver`] is asked.
//! 3. **Do it** — and then *observe the result*, which is a separate step and the one that
//!    can fail on its own.
//!
//! ## A dry run goes through the gates too
//!
//! [`Mode::DryRun`] is not "skip everything and print". It resolves the path and preflights
//! the policy, so it answers the question an operator actually has — *would this be allowed,
//! and what exactly would it do* — and it answers it before anything is touched.
//!
//! What a dry run will not do is **prompt**. Asking somebody to approve an act that is not
//! going to happen teaches them that the prompt is a formality, and the next one they see
//! for real gets the same reflex. So a dry run whose policy says "ask" reports that it would
//! have asked, and stops.
//!
//! ## Doing and observing are not the same step
//!
//! Every arm here writes and then *checks*. A write that returns `Ok` and a file that is
//! then unreadable is [`Outcome::Unknown`], not a success — the tempting collapse is to
//! trust the return value, and a record that claims success for an unverified write is a
//! false statement carrying a valid commitment. `Unknown` is the honest arm and it is meant
//! to be reachable.

use std::path::Path;

use scema_tools::Workspace;
use scema_trust::{Approver, Request, TrustPolicy};

use crate::{Effect, Outcome, RefusedBy};

/// Whether to actually do it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// Run the gates, touch nothing, report what would happen.
    DryRun,
    /// Run the gates and carry it out.
    Commit,
}

/// Run one effect and report the outcome.
///
/// Never panics and never propagates an error: every failure path is an [`Outcome`],
/// because the point of this function is to produce something recordable. A caller that had
/// to handle `Result` as well as `Outcome` would have two ways to describe a failure and
/// would eventually record only one of them.
pub fn run(
    effect: &Effect,
    workspace: &Workspace,
    policy: &mut TrustPolicy,
    approver: &mut dyn Approver,
    mode: Mode,
) -> Outcome {
    // 1. Where.
    //
    // Resolving the *parent* for a path that does not exist yet: `Workspace::resolve`
    // canonicalises, which fails on a file that has not been created. Confining the parent
    // is the same guarantee — a path cannot escape the roots by having a name that is not
    // there yet — and it is the only form of the check that works for a create.
    let target = effect.path();
    let confined = match confine(workspace, target) {
        Ok(p) => p,
        Err(reason) => {
            return Outcome::Refused { by: RefusedBy::Workspace, reason };
        }
    };

    // 2. Whether.
    let request = Request::new(tool_name(effect), effect.risk())
        .at(target)
        .describing(effect.summary());

    match policy.preflight(&request) {
        Some(d) if !d.allowed() => {
            return Outcome::Refused {
                by: RefusedBy::Policy,
                reason: format!("policy refused `{}` at {}", request.tool, target),
            };
        }
        Some(_) => {}
        None => {
            if mode == Mode::DryRun {
                // Deliberately not prompting. See the module note.
                return Outcome::Simulated;
            }
            let answer = approver.prompt(&request);
            policy.remember(&request, answer);
            if !answer.allowed() {
                return Outcome::Refused {
                    by: RefusedBy::Operator,
                    reason: approver.why_refused().to_string(),
                };
            }
        }
    }

    if mode == Mode::DryRun {
        return Outcome::Simulated;
    }

    // 3. Do it, then observe it.
    match effect {
        Effect::WriteFile { contents, .. } => write_file(&confined, contents),
        Effect::CreateDir { .. } => create_dir(&confined),
        Effect::Run { argv, .. } => run_command(argv, &confined),
    }
}

/// Confine a path that may not exist yet.
///
/// `Workspace::resolve` canonicalises, which fails on anything not yet created — so every
/// create would be refused if that were the only check. The rule here is to confine the
/// **deepest ancestor that does exist** and rebuild the rest onto it. A path cannot escape
/// the roots by naming directories that are not there.
///
/// Two things this deliberately refuses rather than guesses at.
///
/// A non-existent path containing `..` cannot be canonicalised, so there is no safe way to
/// know where it points — `a/../../b` is only resolvable once `a` exists. Refusing is the
/// difference between "I collapsed this correctly" and "I assumed". This is the case a
/// string-scan confinement check gets wrong in the dangerous direction.
///
/// And the rebuilt leaf is re-checked against the protected names, because it skipped the
/// `resolve` call that would normally have applied them.
fn confine(workspace: &Workspace, target: &str) -> Result<std::path::PathBuf, String> {
    if let Ok(p) = workspace.resolve(target) {
        return Ok(p);
    }

    let raw = Path::new(target);
    if raw.components().any(|c| matches!(c, std::path::Component::ParentDir)) {
        return Err(format!(
            "`{target}` contains `..` and does not exist, so where it points cannot be \
             established; refusing rather than guessing"
        ));
    }

    // Relative paths mean "inside the workspace", never "inside the process working
    // directory" — a daemon's cwd is not something the caller can see.
    let absolute = if raw.is_absolute() {
        raw.to_path_buf()
    } else {
        match workspace.roots().first() {
            Some(r) => r.join(raw),
            None => return Err("workspace has no roots".into()),
        }
    };

    let mut existing = absolute.clone();
    let mut tail: Vec<std::ffi::OsString> = Vec::new();
    while !existing.exists() {
        let Some(name) = existing.file_name().map(|n| n.to_owned()) else {
            return Err(format!("`{target}` has no existing ancestor inside the workspace"));
        };
        tail.push(name);
        match existing.parent() {
            Some(p) if !p.as_os_str().is_empty() => existing = p.to_path_buf(),
            _ => return Err(format!("`{target}` has no existing ancestor inside the workspace")),
        }
    }

    let base = workspace
        .resolve(&existing.to_string_lossy())
        .map_err(|e| e.to_string())?;
    tail.reverse();
    let joined = tail.iter().fold(base, |acc, n| acc.join(n));

    if scema_tools::workspace::is_protected(&joined) {
        return Err(format!("`{target}` is a protected path and is never written"));
    }
    Ok(joined)
}

/// The tool name a policy rule matches on.
///
/// Stable strings, not `Debug` output: a rule written as `write_file` in somebody's config
/// must keep matching after an enum variant is renamed.
fn tool_name(effect: &Effect) -> &'static str {
    match effect {
        Effect::WriteFile { .. } => "write_file",
        Effect::CreateDir { .. } => "create_dir",
        Effect::Run { .. } => "run",
    }
}

fn write_file(path: &Path, contents: &str) -> Outcome {
    if let Some(parent) = path.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            return Outcome::Failed { reason: format!("creating {}: {e}", parent.display()) };
        }
    }
    if let Err(e) = std::fs::write(path, contents) {
        return Outcome::Failed { reason: format!("writing {}: {e}", path.display()) };
    }
    // Observe. A write that returned Ok and a file that is then unreadable is not a success.
    match std::fs::metadata(path) {
        Ok(m) if m.len() as usize == contents.len() => {
            Outcome::Succeeded { detail: format!("{} bytes", m.len()) }
        }
        Ok(m) => Outcome::Unknown {
            why: format!(
                "wrote {} bytes but {} reports {} — something else is writing here",
                contents.len(),
                path.display(),
                m.len()
            ),
        },
        Err(e) => Outcome::Unknown {
            why: format!("wrote {}, but could not read it back: {e}", path.display()),
        },
    }
}

fn create_dir(path: &Path) -> Outcome {
    if let Err(e) = std::fs::create_dir_all(path) {
        return Outcome::Failed { reason: format!("creating {}: {e}", path.display()) };
    }
    match std::fs::metadata(path) {
        Ok(m) if m.is_dir() => Outcome::Succeeded { detail: path.display().to_string() },
        Ok(_) => Outcome::Unknown {
            why: format!("{} exists but is not a directory", path.display()),
        },
        Err(e) => Outcome::Unknown {
            why: format!("created {}, but could not stat it: {e}", path.display()),
        },
    }
}

fn run_command(argv: &[String], cwd: &Path) -> Outcome {
    let Some((program, rest)) = argv.split_first() else {
        return Outcome::Failed { reason: "empty argv".into() };
    };
    // No shell. The argv is what was approved and the argv is what runs.
    match std::process::Command::new(program).args(rest).current_dir(cwd).output() {
        Ok(out) => match out.status.code() {
            Some(0) => Outcome::Succeeded {
                detail: format!("exit 0, {} byte(s) of output", out.stdout.len()),
            },
            Some(code) => Outcome::Failed { reason: format!("exit {code}") },
            // Killed by a signal: it ran, and what it did is not knowable from here.
            None => Outcome::Unknown {
                why: "terminated by a signal; whether it completed its work is unknown".into(),
            },
        },
        Err(e) => Outcome::Failed { reason: format!("could not start `{program}`: {e}") },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use scema_trust::{AutoApprover, DenyApprover, Decision, Risk, Rule};

    fn scratch(tag: &str) -> std::path::PathBuf {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let p = std::env::temp_dir().join(format!(
            "scema-effect-{}-{}-{}",
            std::process::id(),
            tag,
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&p).unwrap();
        std::fs::canonicalize(&p).unwrap()
    }

    fn ws(root: &Path) -> Workspace {
        Workspace::new([root]).unwrap()
    }

    #[test]
    fn a_dry_run_touches_nothing_but_still_reports_a_policy_refusal() {
        // The value of a dry run is that it answers "would this be allowed", not just
        // "what would it write".
        let root = scratch("dry");
        let e = Effect::Run { argv: vec!["echo".into()], cwd: root.display().to_string() };
        let mut p = TrustPolicy::new(); // execution off
        let out = run(&e, &ws(&root), &mut p, &mut DenyApprover, Mode::DryRun);
        assert!(matches!(out, Outcome::Refused { by: RefusedBy::Policy, .. }), "{out:?}");
    }

    #[test]
    fn a_dry_run_never_prompts() {
        // Asking somebody to approve an act that is not going to happen teaches them the
        // prompt is a formality. A prompting approver here would return Allow; Simulated
        // proves it was never consulted.
        let root = scratch("noprompt");
        let e = Effect::WriteFile {
            path: root.join("a.txt").display().to_string(),
            contents: "x".into(),
        };
        let mut p = TrustPolicy::new(); // writes prompt
        let out = run(&e, &ws(&root), &mut p, &mut AutoApprover, Mode::DryRun);
        assert_eq!(out, Outcome::Simulated);
        assert!(!root.join("a.txt").exists(), "a dry run must touch nothing");
        assert!(p.grants().is_empty(), "a dry run must not record a grant");
    }

    #[test]
    fn a_write_outside_the_workspace_is_refused_by_the_first_gate() {
        let root = scratch("outside");
        let other = scratch("elsewhere");
        let e = Effect::WriteFile {
            path: other.join("a.txt").display().to_string(),
            contents: "x".into(),
        };
        let mut p = TrustPolicy::new().writing();
        let out = run(&e, &ws(&root), &mut p, &mut AutoApprover, Mode::Commit);
        assert!(matches!(out, Outcome::Refused { by: RefusedBy::Workspace, .. }), "{out:?}");
        assert!(!other.join("a.txt").exists());
    }

    #[test]
    fn a_nonexistent_path_containing_dot_dot_is_refused_rather_than_guessed_at() {
        // The dangerous direction. `a/../../b` cannot be canonicalised until `a` exists, so
        // there is no honest way to say where it points — and a string-scan confinement
        // check is exactly the thing that gets this wrong while looking careful.
        let root = scratch("dotdot");
        let e = Effect::WriteFile {
            path: root.join("nope").join("..").join("..").join("escaped.txt").display().to_string(),
            contents: "x".into(),
        };
        let mut p = TrustPolicy::new().writing();
        let out = run(&e, &ws(&root), &mut p, &mut AutoApprover, Mode::Commit);
        assert!(matches!(out, Outcome::Refused { by: RefusedBy::Workspace, .. }), "{out:?}");
        assert!(!root.parent().unwrap().join("escaped.txt").exists());
    }

    #[test]
    fn a_deeply_nested_new_path_is_confined_by_its_deepest_existing_ancestor() {
        // Confining only the immediate parent refuses every nested create, which is how a
        // confinement check ends up switched off.
        let root = scratch("nested");
        let path = root.join("a").join("b").join("c").join("f.txt");
        let e = Effect::WriteFile { path: path.display().to_string(), contents: "x".into() };
        let mut p = TrustPolicy::new().writing();
        let out = run(&e, &ws(&root), &mut p, &mut AutoApprover, Mode::Commit);
        assert!(matches!(out, Outcome::Succeeded { .. }), "{out:?}");
        assert!(path.exists());
    }

    #[test]
    fn a_relative_path_means_inside_the_workspace_not_the_process_cwd() {
        // A daemon's working directory is not something the caller can see, so resolving
        // against it would make the same request mean different things.
        let root = scratch("relative");
        let e = Effect::WriteFile { path: "rel.txt".into(), contents: "x".into() };
        let mut p = TrustPolicy::new().writing();
        let out = run(&e, &ws(&root), &mut p, &mut AutoApprover, Mode::Commit);
        assert!(matches!(out, Outcome::Succeeded { .. }), "{out:?}");
        assert!(root.join("rel.txt").exists());
    }

    #[test]
    fn a_protected_name_is_refused_even_inside_the_workspace() {
        // The leaf does not exist yet, so confinement goes via the parent — and the
        // protected-name check has to be applied to the leaf explicitly on that path.
        let root = scratch("protected");
        let e = Effect::WriteFile {
            path: root.join(".env").display().to_string(),
            contents: "SECRET=1".into(),
        };
        let mut p = TrustPolicy::new().writing();
        let out = run(&e, &ws(&root), &mut p, &mut AutoApprover, Mode::Commit);
        assert!(matches!(out, Outcome::Refused { by: RefusedBy::Workspace, .. }), "{out:?}");
        assert!(!root.join(".env").exists());
    }

    #[test]
    fn a_refusal_for_want_of_a_terminal_does_not_claim_somebody_declined() {
        // The specification's own rule, and the first end-to-end run of `scema execute`
        // broke it: `DenyApprover` refuses *without asking anyone*, and recording that as
        // "declined at the prompt" describes a decision nobody made — sending an operator
        // to look for a prompt they never saw.
        let root = scratch("noterminal");
        let e = Effect::WriteFile {
            path: root.join("a.txt").display().to_string(),
            contents: "x".into(),
        };
        let mut p = TrustPolicy::new();
        let out = run(&e, &ws(&root), &mut p, &mut DenyApprover, Mode::Commit);
        match out {
            Outcome::Refused { by: RefusedBy::Operator, reason } => {
                assert!(reason.contains("not a terminal"), "reason was: {reason}");
                assert!(!reason.contains("declined at the prompt"));
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_declined_prompt_is_recorded_as_the_operator_not_the_policy() {
        let root = scratch("declined");
        let e = Effect::WriteFile {
            path: root.join("a.txt").display().to_string(),
            contents: "x".into(),
        };
        let mut p = TrustPolicy::new();
        let out = run(&e, &ws(&root), &mut p, &mut DenyApprover, Mode::Commit);
        assert!(matches!(out, Outcome::Refused { by: RefusedBy::Operator, .. }), "{out:?}");
    }

    #[test]
    fn a_committed_write_lands_and_is_verified_by_reading_it_back() {
        let root = scratch("write");
        let path = root.join("sub").join("a.txt");
        let e = Effect::WriteFile {
            path: path.display().to_string(),
            contents: "hello".into(),
        };
        let mut p = TrustPolicy::new().writing();
        let out = run(&e, &ws(&root), &mut p, &mut DenyApprover, Mode::Commit);
        assert!(matches!(out, Outcome::Succeeded { .. }), "{out:?}");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "hello");
    }

    #[test]
    fn a_rule_can_refuse_a_specific_directory() {
        let root = scratch("rule");
        std::fs::create_dir_all(root.join("locked")).unwrap();
        let e = Effect::WriteFile {
            path: root.join("locked").join("a.txt").display().to_string(),
            contents: "x".into(),
        };
        let mut p = TrustPolicy::new().writing();
        p.rules.push(Rule::new("write_file", Decision::Deny).at("*locked*"));
        let out = run(&e, &ws(&root), &mut p, &mut AutoApprover, Mode::Commit);
        assert!(matches!(out, Outcome::Refused { by: RefusedBy::Policy, .. }), "{out:?}");
    }

    #[test]
    fn the_tool_name_is_stable_and_not_debug_output() {
        // A rule written as `write_file` in somebody's config must keep matching after an
        // enum variant is renamed.
        assert_eq!(tool_name(&Effect::CreateDir { path: "x".into() }), "create_dir");
        assert_eq!(
            tool_name(&Effect::Run { argv: vec![], cwd: ".".into() }),
            "run"
        );
    }

    #[test]
    fn an_empty_argv_fails_rather_than_starting_something() {
        let root = scratch("emptyargv");
        let e = Effect::Run { argv: vec![], cwd: root.display().to_string() };
        let mut p = TrustPolicy::new().executing();
        let out = run(&e, &ws(&root), &mut p, &mut AutoApprover, Mode::Commit);
        assert!(matches!(out, Outcome::Failed { .. }), "{out:?}");
    }

    #[test]
    fn a_command_that_cannot_start_is_a_failure_not_an_unknown() {
        // It definitely did not run. Reserving Unknown for cases where the world may have
        // changed is what keeps the arm meaningful.
        let root = scratch("nostart");
        let e = Effect::Run {
            argv: vec!["scema-definitely-not-a-real-binary".into()],
            cwd: root.display().to_string(),
        };
        let mut p = TrustPolicy::new().executing();
        let out = run(&e, &ws(&root), &mut p, &mut AutoApprover, Mode::Commit);
        assert!(matches!(out, Outcome::Failed { .. }), "{out:?}");
    }

    #[test]
    fn writes_stay_refused_under_read_only_whatever_else_is_set() {
        let root = scratch("readonly");
        let e = Effect::WriteFile {
            path: root.join("a.txt").display().to_string(),
            contents: "x".into(),
        };
        let mut p = TrustPolicy::read_only().writing();
        let out = run(&e, &ws(&root), &mut p, &mut AutoApprover, Mode::Commit);
        assert!(matches!(out, Outcome::Refused { by: RefusedBy::Policy, .. }), "{out:?}");
        assert!(!root.join("a.txt").exists());
    }

    #[test]
    fn a_create_dir_is_confirmed_by_stat_not_by_the_call_returning_ok() {
        let root = scratch("mkdir");
        let e = Effect::CreateDir { path: root.join("made").display().to_string() };
        let mut p = TrustPolicy::new().writing();
        let out = run(&e, &ws(&root), &mut p, &mut AutoApprover, Mode::Commit);
        assert!(matches!(out, Outcome::Succeeded { .. }), "{out:?}");
        assert!(root.join("made").is_dir());
    }

    #[test]
    fn risk_is_carried_into_the_request_so_execution_is_gated_separately() {
        // A write policy must not authorise a command.
        let root = scratch("risksplit");
        let e = Effect::Run {
            argv: vec!["echo".into()],
            cwd: root.display().to_string(),
        };
        let mut p = TrustPolicy::new().writing();
        let out = run(&e, &ws(&root), &mut p, &mut AutoApprover, Mode::Commit);
        assert!(matches!(out, Outcome::Refused { by: RefusedBy::Policy, .. }), "{out:?}");
        assert_eq!(Risk::Execute, e.risk());
    }
}
