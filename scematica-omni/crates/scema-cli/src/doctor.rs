//! `scema doctor` — what is installed, what is wired up, and what is quietly broken.
//!
//! Omni is four surfaces over one loop, and the failure modes people actually hit are all
//! in the seams: a console that was never installed, a `.mcp.json` pointing at a binary
//! that has moved, a `.scema/` directory holding a record that no longer verifies, a
//! terminal that cannot tell violet from grey. None of those produce a good error at the
//! moment they bite. This command asks all of them at once.
//!
//! ## Every check reports one of three things, never two
//!
//! [`Verdict::Ok`], [`Verdict::Warn`], [`Verdict::Fail`] — and, importantly,
//! [`Verdict::Unknown`], for a check that could not be run at all. "The record store does
//! not verify" and "the record store could not be read" are different claims and only one
//! of them is an accusation; collapsing them is exactly the mistake `/mesh` made when every
//! failure rendered as "No instance paired".
//!
//! ## It changes nothing
//!
//! A doctor that repaired things would need the whole approval story in front of it, and an
//! operator running a diagnostic does not expect their editor configuration to be edited.
//! Every finding names the command that would fix it and stops there.

use std::path::{Path, PathBuf};

use scema_memory::MemoryStore;
use scema_verify::{verify, RecordStore};

use crate::connect::{self, Scope};
use crate::launch;

/// How a single check came out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    Ok,
    /// Works, but something will bite later.
    Warn,
    /// Broken now.
    Fail,
    /// The check itself could not be run. Not a pass and not a failure.
    Unknown,
}

impl Verdict {
    pub fn glyph(self) -> &'static str {
        match self {
            Verdict::Ok => "ok  ",
            Verdict::Warn => "warn",
            Verdict::Fail => "FAIL",
            Verdict::Unknown => "?   ",
        }
    }
}

/// One line of the report.
pub struct Finding {
    pub verdict: Verdict,
    pub check: String,
    pub detail: String,
    /// The command that would fix it, when there is one. Empty otherwise — a hint that says
    /// "investigate" is worse than no hint.
    pub fix: String,
}

impl Finding {
    fn new(verdict: Verdict, check: impl Into<String>, detail: impl Into<String>) -> Self {
        Finding {
            verdict,
            check: check.into(),
            detail: detail.into(),
            fix: String::new(),
        }
    }

    fn fix(mut self, f: impl Into<String>) -> Self {
        self.fix = f.into();
        self
    }
}

/// Run every check.
pub fn run(root: &Path, project: &Path) -> Vec<Finding> {
    let mut out = Vec::new();
    out.extend(components());
    out.extend(state(root));
    out.extend(records(root));
    out.extend(memory(root));
    out.extend(assistants(project));
    out.extend(terminal());
    out
}

fn components() -> Vec<Finding> {
    launch::inventory()
        .into_iter()
        .map(|(c, path)| match path {
            Some(p) => Finding::new(
                Verdict::Ok,
                format!("component `{}`", c.bin),
                p.display().to_string(),
            ),
            // A missing surface is a warning and not a failure: `scema verify` in CI needs
            // none of them, and reporting a broken installation to somebody who deliberately
            // installed one binary would train them to ignore this whole report.
            None => Finding::new(
                Verdict::Warn,
                format!("component `{}`", c.bin),
                format!("not installed — {}", c.about),
            )
            .fix(format!("cargo install {}", c.krate)),
        })
        .collect()
}

fn state(root: &Path) -> Vec<Finding> {
    if !root.exists() {
        return vec![Finding::new(
            Verdict::Warn,
            "state directory",
            format!("{} does not exist yet — nothing has been decided here", root.display()),
        )
        .fix("scema init")];
    }
    // Probing writability by writing is the only honest way to ask. A permissions check
    // computed from metadata is wrong on Windows ACLs, on read-only mounts, and inside a
    // container with a squashed filesystem — three places this actually runs.
    let probe = root.join(".scema-doctor-probe");
    match std::fs::write(&probe, b"") {
        Ok(()) => {
            std::fs::remove_file(&probe).ok();
            vec![Finding::new(Verdict::Ok, "state directory", root.display().to_string())]
        }
        Err(e) => vec![Finding::new(
            Verdict::Fail,
            "state directory",
            format!("{} is not writable: {e}", root.display()),
        )],
    }
}

fn records(root: &Path) -> Vec<Finding> {
    let store = RecordStore::new(root.to_path_buf());
    let ids = match store.ids() {
        Ok(i) => i,
        Err(e) => {
            return vec![Finding::new(
                Verdict::Unknown,
                "decision records",
                format!("could not list: {e}"),
            )]
        }
    };
    if ids.is_empty() {
        return vec![Finding::new(Verdict::Ok, "decision records", "none sealed yet")];
    }

    let mut valid = 0usize;
    let mut invalid = Vec::new();
    let mut unreadable = Vec::new();
    for id in &ids {
        match store.load(id) {
            Ok(r) => {
                if verify(&r).valid {
                    valid += 1;
                } else {
                    invalid.push(id.clone());
                }
            }
            Err(_) => unreadable.push(id.clone()),
        }
    }

    let mut out = vec![];
    if invalid.is_empty() {
        out.push(Finding::new(
            Verdict::Ok,
            "decision records",
            format!("{valid} of {} verify", ids.len()),
        ));
    } else {
        out.push(
            Finding::new(
                Verdict::Fail,
                "decision records",
                format!(
                    "{} record(s) DO NOT VERIFY: {}",
                    invalid.len(),
                    invalid.join(", ")
                ),
            )
            .fix("scema verify --all   # names the field that moved"),
        );
    }
    if !unreadable.is_empty() {
        // Third state, kept apart from the second on purpose.
        out.push(Finding::new(
            Verdict::Unknown,
            "decision records",
            format!(
                "{} record(s) could not be read at all: {}",
                unreadable.len(),
                unreadable.join(", ")
            ),
        ));
    }
    out
}

fn memory(root: &Path) -> Vec<Finding> {
    let mem = MemoryStore::new(root.to_path_buf());
    let counts = match mem.counts() {
        Ok(c) => c,
        Err(e) => {
            return vec![Finding::new(Verdict::Unknown, "memory", format!("unreadable: {e}"))]
        }
    };
    let total: usize = counts.iter().map(|(_, n, _)| n).sum();
    let corrupt: usize = counts.iter().map(|(_, _, c)| c).sum();
    let mut out = vec![Finding::new(
        Verdict::Ok,
        "memory",
        format!("{total} record(s) across {} log(s)", counts.len()),
    )];
    if corrupt > 0 {
        // An append-only log with a torn line is not a shorter log. Counting the bad lines
        // separately is what keeps a truncated write from looking like a smaller history.
        out.push(Finding::new(
            Verdict::Warn,
            "memory",
            format!("{corrupt} unreadable line(s) — a torn append, most likely a kill mid-write"),
        ));
    }
    match mem.calibration() {
        Ok(c) if c.recorded > 0 && c.resolved == 0 => out.push(Finding::new(
            Verdict::Ok,
            "calibration",
            format!(
                "{} counterfactual(s), none resolved — expected: a branch nobody ran has no outcome",
                c.recorded
            ),
        )),
        Ok(c) => out.push(Finding::new(
            Verdict::Ok,
            "calibration",
            format!("{} recorded, {} resolved", c.recorded, c.resolved),
        )),
        Err(e) => out.push(Finding::new(Verdict::Unknown, "calibration", format!("{e}"))),
    }
    out
}

/// Which assistants in this project are wired to the MCP server, and whether the wiring
/// still points at something.
fn assistants(project: &Path) -> Vec<Finding> {
    let mut out = Vec::new();
    let mut any = false;
    for h in connect::HOSTS.iter().filter(|h| h.scope == Scope::Project) {
        let path = project.join(h.project_path);
        if !path.exists() {
            continue;
        }
        any = true;
        let text = match std::fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) => {
                out.push(Finding::new(
                    Verdict::Unknown,
                    format!("assistant `{}`", h.key),
                    format!("{} unreadable: {e}", path.display()),
                ));
                continue;
            }
        };
        let parsed: Result<serde_json::Value, _> = serde_json::from_str(&text);
        match parsed {
            Ok(v) => {
                let container = match h.shape {
                    connect::Shape::VsCodeServers => "servers",
                    _ => "mcpServers",
                };
                if v.get(container).and_then(|c| c.get("scema")).is_some() {
                    out.push(Finding::new(
                        Verdict::Ok,
                        format!("assistant `{}`", h.key),
                        format!("{} names `scema`", path.display()),
                    ));
                } else {
                    out.push(
                        Finding::new(
                            Verdict::Warn,
                            format!("assistant `{}`", h.key),
                            format!("{} exists but does not name `scema`", path.display()),
                        )
                        .fix(format!("scema connect {} --write", h.key)),
                    );
                }
            }
            Err(e) => out.push(Finding::new(
                Verdict::Fail,
                format!("assistant `{}`", h.key),
                format!("{} is not valid JSON: {e}", path.display()),
            )),
        }
    }
    if !any {
        out.push(
            Finding::new(
                Verdict::Warn,
                "assistants",
                "no project-local MCP configuration found",
            )
            .fix("scema connect --list   # then `scema connect <host> --write`"),
        );
    }
    // The wiring can be present and still point at nothing.
    if launch::locate(launch::MCP).is_none() && any {
        out.push(
            Finding::new(
                Verdict::Fail,
                "assistants",
                "a host is configured to run `scema-mcp`, which is not on PATH — the server will fail to start with no message the host can show you",
            )
            .fix("cargo install scema-mcp"),
        );
    }
    out
}

fn terminal() -> Vec<Finding> {
    // Not vanity. The palette degrades truecolor -> 256 -> 16 -> none, and on a 16-colour
    // terminal an operator should know before they trust a screen full of violet that the
    // distinction they are relying on is being carried by the *text*.
    let no_color = std::env::var_os("NO_COLOR").is_some();
    let colorterm = std::env::var("COLORTERM").unwrap_or_default();
    let term = std::env::var("TERM").unwrap_or_default();
    let detail = format!(
        "TERM={} COLORTERM={} NO_COLOR={}",
        if term.is_empty() { "(unset)" } else { &term },
        if colorterm.is_empty() { "(unset)" } else { &colorterm },
        if no_color { "set" } else { "unset" }
    );
    vec![Finding::new(
        if no_color { Verdict::Warn } else { Verdict::Ok },
        "terminal",
        if no_color {
            format!("{detail} — the console will draw without colour, which it is designed to survive")
        } else {
            detail
        },
    )]
}

/// The exit code for a whole report.
///
/// A failure exits non-zero so CI can run this; a warning does not, because warnings here
/// are mostly "you did not install the optional console", and a diagnostic that fails a
/// build over an optional component is a diagnostic people delete from the pipeline.
pub fn worst(findings: &[Finding]) -> Verdict {
    if findings.iter().any(|f| f.verdict == Verdict::Fail) {
        Verdict::Fail
    } else if findings.iter().any(|f| f.verdict == Verdict::Unknown) {
        Verdict::Unknown
    } else if findings.iter().any(|f| f.verdict == Verdict::Warn) {
        Verdict::Warn
    } else {
        Verdict::Ok
    }
}

/// The default project root: the working directory.
pub fn cwd() -> PathBuf {
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn scratch() -> PathBuf {
        let p = std::env::temp_dir().join(format!(
            "scema-doctor-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn a_missing_state_directory_is_a_warning_with_a_fix_not_a_failure() {
        // A fresh checkout has no `.scema/`. Reporting that as broken would make the first
        // run of the tool look like a broken install.
        let dir = scratch();
        let f = state(&dir.join("nope"));
        assert_eq!(f[0].verdict, Verdict::Warn);
        assert_eq!(f[0].fix, "scema init");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn an_unreadable_record_is_unknown_and_an_invalid_one_is_a_failure() {
        // The distinction the whole report is built around: one is a gap, the other is an
        // accusation.
        let dir = scratch();
        let decisions = dir.join("decisions");
        fs::create_dir_all(&decisions).unwrap();
        fs::write(decisions.join("deadbeef.json"), "{ not a record").unwrap();
        let f = records(&dir);
        assert!(
            f.iter().any(|x| x.verdict == Verdict::Unknown),
            "{:?}",
            f.iter().map(|x| (x.verdict, x.detail.clone())).collect::<Vec<_>>()
        );
        assert!(f.iter().all(|x| x.verdict != Verdict::Fail));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_project_with_no_assistant_config_says_how_to_add_one() {
        let dir = scratch();
        let f = assistants(&dir);
        assert!(f.iter().any(|x| x.fix.contains("scema connect")));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_config_that_does_not_name_scema_is_a_warning_not_a_pass() {
        // Somebody else's `.mcp.json`. Reporting it green would tell an operator the wiring
        // is done when the server is not in it.
        let dir = scratch();
        fs::write(dir.join(".mcp.json"), r#"{"mcpServers":{"other":{"command":"x"}}}"#).unwrap();
        let f = assistants(&dir);
        let row = f.iter().find(|x| x.check.contains("claude-code")).unwrap();
        assert_eq!(row.verdict, Verdict::Warn);
        assert!(row.fix.contains("--write"));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_malformed_assistant_config_is_a_failure_and_names_the_file() {
        let dir = scratch();
        fs::write(dir.join(".mcp.json"), "{ nope").unwrap();
        let f = assistants(&dir);
        let row = f.iter().find(|x| x.check.contains("claude-code")).unwrap();
        assert_eq!(row.verdict, Verdict::Fail);
        assert!(row.detail.contains(".mcp.json"));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn unknown_outranks_warn_but_not_fail() {
        // So a report full of "could not check" never exits 0 pretending everything passed,
        // and never exits as though something had actually failed either.
        let f = |v: Verdict| Finding::new(v, "c", "d");
        assert_eq!(worst(&[f(Verdict::Ok)]), Verdict::Ok);
        assert_eq!(worst(&[f(Verdict::Ok), f(Verdict::Warn)]), Verdict::Warn);
        assert_eq!(worst(&[f(Verdict::Warn), f(Verdict::Unknown)]), Verdict::Unknown);
        assert_eq!(worst(&[f(Verdict::Unknown), f(Verdict::Fail)]), Verdict::Fail);
    }
}
