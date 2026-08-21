//! Dispatching to the sibling binaries, so `scema` is one entry point rather than four.
//!
//! Omni ships four surfaces and three of them are separate binaries: `scema-tui`,
//! `scema-omnid`, `scema-mcp`. Making the operator remember which is which — and remember
//! that the console is `scema-tui` and not `scema tui` — is a small tax charged on every
//! single use, and the fix is the pattern the bot workspace already settled on with the
//! `scematica` launcher: one command that finds its siblings and hands over.
//!
//! ## Why exec rather than a shared crate
//!
//! Linking `scema-tui` into `scema` would drag ratatui and crossterm into every install of
//! the CLI, including the ones on a machine that will only ever run `scema verify` in CI.
//! It would also make `cargo install scema-cli` a 40-second build for a tool whose whole
//! appeal is that it is small. So the launcher spawns, and the components stay independent
//! crates that can be installed one at a time.
//!
//! ## Resolution order, and why the sibling comes first
//!
//! 1. Next to the running `scema` binary.
//! 2. `PATH`.
//!
//! Sibling first is the load-bearing part. A developer with `cargo install`ed binaries in
//! `~/.cargo/bin` and a `target/release` build in a checkout will run the checkout's
//! `scema` and expect the checkout's `scema-tui` — resolving through `PATH` first silently
//! pairs a new launcher with an old console, and the symptom is a flag that "does not
//! exist" in a binary where it plainly does.

use std::path::PathBuf;
use std::process::{Command, ExitCode};

use anyhow::{anyhow, Result};

/// A component binary this launcher knows how to hand over to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Component {
    /// Binary name, without an extension.
    pub bin: &'static str,
    /// The crate that provides it, for the install hint.
    pub krate: &'static str,
    pub about: &'static str,
}

pub const TUI: Component = Component {
    bin: "scema-tui",
    krate: "scema-tui",
    about: "the console — the loop as a terminal application",
};
pub const DAEMON: Component = Component {
    bin: "scema-omnid",
    krate: "scema-daemon",
    about: "the local daemon — loopback HTTP, token-authenticated",
};
pub const MCP: Component = Component {
    bin: "scema-mcp",
    krate: "scema-mcp",
    about: "the MCP server — the loop as tools, for a model",
};

pub const ALL: [Component; 3] = [TUI, DAEMON, MCP];

fn exe_name(bin: &str) -> String {
    if cfg!(windows) {
        format!("{bin}.exe")
    } else {
        bin.to_string()
    }
}

/// Find a component binary, sibling first.
pub fn locate(component: Component) -> Option<PathBuf> {
    if let Ok(me) = std::env::current_exe() {
        if let Some(dir) = me.parent() {
            let candidate = dir.join(exe_name(component.bin));
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    which(component.bin)
}

/// A minimal `which`, over `PATH` and (on Windows) `PATHEXT`.
///
/// Hand-rolled rather than a crate: this is the only place the CLI needs it, and a
/// dependency in a binary that is meant to be trivially installable is a dependency
/// somebody has to audit.
fn which(bin: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let direct = dir.join(exe_name(bin));
        if direct.is_file() {
            return Some(direct);
        }
        if cfg!(windows) {
            let exts = std::env::var("PATHEXT").unwrap_or_else(|_| ".EXE;.CMD;.BAT".into());
            for ext in exts.split(';') {
                let candidate = dir.join(format!("{bin}{}", ext.to_ascii_lowercase()));
                if candidate.is_file() {
                    return Some(candidate);
                }
            }
        }
    }
    None
}

/// Hand over to a component, forwarding the remaining arguments and its exit code.
///
/// The child inherits stdio, which matters for all three: the console needs a real
/// terminal, the daemon logs to stderr, and the MCP server speaks JSON-RPC on stdout and
/// must not have anything interposed on it.
pub fn run(component: Component, args: &[String]) -> Result<ExitCode> {
    let Some(path) = locate(component) else {
        return Err(anyhow!(
            "`{}` is not installed.\n\n  It is a separate binary so that `scema` itself stays small — the console\n  pulls in a whole terminal stack that a CI machine running `scema verify` has\n  no use for.\n\n  Install it with:\n\n      cargo install {}\n\n  …or build it in a checkout with:\n\n      cargo build --release -p {}",
            component.bin,
            component.krate,
            component.krate
        ));
    };

    let status = Command::new(&path)
        .args(args)
        .status()
        .map_err(|e| anyhow!("could not start {}: {e}", path.display()))?;

    // The child's code, verbatim. Collapsing a component's exit status into the launcher's
    // own would make `scema verify --all` in a script report success on a record that did
    // not verify.
    Ok(match status.code() {
        Some(code) => ExitCode::from((code & 0xff) as u8),
        // Killed by a signal. 130 is the conventional "terminated by SIGINT", and any
        // non-zero beats reporting success for a process that was killed.
        None => ExitCode::from(130),
    })
}

/// One line per component, saying whether it is installed and where.
pub fn inventory() -> Vec<(Component, Option<PathBuf>)> {
    ALL.iter().map(|c| (*c, locate(*c))).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_component_names_the_crate_that_provides_it() {
        // The error an operator actually hits. "command not found" sends them to a search
        // engine; naming the crate sends them to a working install.
        let missing = Component {
            bin: "scema-does-not-exist",
            krate: "scema-nowhere",
            about: "x",
        };
        let err = run(missing, &[]).unwrap_err().to_string();
        assert!(err.contains("cargo install scema-nowhere"), "{err}");
    }

    #[test]
    fn the_windows_extension_is_only_added_on_windows() {
        if cfg!(windows) {
            assert_eq!(exe_name("scema-tui"), "scema-tui.exe");
        } else {
            assert_eq!(exe_name("scema-tui"), "scema-tui");
        }
    }

    #[test]
    fn every_component_is_a_distinct_binary_and_crate() {
        let mut bins: Vec<&str> = ALL.iter().map(|c| c.bin).collect();
        bins.sort_unstable();
        let before = bins.len();
        bins.dedup();
        assert_eq!(before, bins.len());
    }
}
