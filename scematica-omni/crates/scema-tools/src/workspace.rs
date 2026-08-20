//! [`Workspace`]: the answer to *where*, and the only thing allowed to answer it.
//!
//! The CLI has an operator typing paths, which needs no confinement — they can already read
//! their own disk. The daemon and the MCP server do not: their callers are a browser
//! extension and a language model, and "observe this directory" from either of those is an
//! instruction from somewhere the operator is not looking. A model asked to audit a project
//! will cheerfully propose observing `~/.ssh`, and the observer would do it.
//!
//! So both of those front ends resolve every path through this type and nothing else. It is
//! the same split `alchem-link` settled on, and the same two rules:
//!
//! * **Resolve first, compare second.** Paths are fully canonicalised — symlinks followed,
//!   `..` collapsed — and *then* checked against the roots. A string scan for `..` passes a
//!   symlink that points at `/`, which is the whole attack.
//! * **A tool that opens a path directly bypasses the model.** There is no partial
//!   application of this; if a front end resolves one path itself, the confinement is
//!   decorative.
//!
//! Note what this does *not* do. It says nothing about *whether* an action is allowed —
//! that is `Goal`'s constraints and, when there is ever a write path, an approval policy.
//! Where and whether are different questions and the failure mode of merging them is that a
//! grant for one silently becomes a grant for the other.

use std::path::{Path, PathBuf};

use anyhow::{anyhow, Result};

/// A set of directories a front end may look inside.
#[derive(Clone, Debug)]
pub struct Workspace {
    roots: Vec<PathBuf>,
}

impl Workspace {
    /// Build from candidate roots, dropping any that do not resolve.
    ///
    /// A root that does not exist is dropped rather than fatal, but an empty result *is*
    /// fatal: a workspace with no roots would either confine nothing or confine everything
    /// depending on how the check is written, and neither is a state to start a daemon in.
    pub fn new<I, P>(roots: I) -> Result<Self>
    where
        I: IntoIterator<Item = P>,
        P: AsRef<Path>,
    {
        let resolved: Vec<PathBuf> = roots
            .into_iter()
            .filter_map(|r| std::fs::canonicalize(r.as_ref()).ok())
            .collect();
        if resolved.is_empty() {
            return Err(anyhow!(
                "no readable workspace root; refusing to start with nothing to confine to"
            ));
        }
        Ok(Workspace { roots: resolved })
    }

    /// The current working directory as the sole root.
    pub fn cwd() -> Result<Self> {
        Workspace::new([std::env::current_dir()?])
    }

    pub fn roots(&self) -> &[PathBuf] {
        &self.roots
    }

    /// Human-readable roots, for a `/policy` response or an MCP tool description.
    pub fn root_labels(&self) -> Vec<String> {
        self.roots.iter().map(|p| strip_verbatim(p)).collect()
    }

    /// Resolve a caller-supplied locator, or refuse.
    ///
    /// The error deliberately names the roots. A confinement failure the caller cannot
    /// diagnose gets worked around by turning confinement off.
    pub fn resolve(&self, locator: &str) -> Result<PathBuf> {
        if locator.trim().is_empty() {
            return Err(anyhow!("empty path"));
        }
        let candidate = Path::new(locator);
        // Relative paths resolve against the first root, not the process working directory.
        // A daemon's cwd is not something the caller can see, so resolving against it would
        // make the same request mean different things depending on how it was started.
        let joined = if candidate.is_absolute() {
            candidate.to_path_buf()
        } else {
            self.roots[0].join(candidate)
        };
        let resolved = std::fs::canonicalize(&joined)
            .map_err(|e| anyhow!("cannot resolve `{locator}`: {e}"))?;

        if self.roots.iter().any(|r| resolved.starts_with(r)) {
            Ok(resolved)
        } else {
            Err(anyhow!(
                "`{locator}` resolves to `{}`, which is outside this workspace ({})",
                strip_verbatim(&resolved),
                self.root_labels().join(", ")
            ))
        }
    }
}

/// Strip the Windows extended-length prefix for display. See `repo::display_path`.
fn strip_verbatim(p: &Path) -> String {
    let s = p.to_string_lossy().to_string();
    match s.strip_prefix(r"\\?\") {
        Some(rest) => rest.to_string(),
        None => s,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn scratch() -> PathBuf {
        let p = std::env::temp_dir().join(format!(
            "scema-omni-ws-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&p).unwrap();
        fs::canonicalize(&p).unwrap()
    }

    #[test]
    fn a_path_inside_a_root_resolves() {
        let root = scratch();
        fs::create_dir_all(root.join("sub")).unwrap();
        let ws = Workspace::new([&root]).unwrap();
        assert!(ws.resolve("sub").is_ok());
        assert!(ws.resolve(root.join("sub").to_str().unwrap()).is_ok());
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn dot_dot_cannot_climb_out() {
        let root = scratch();
        fs::create_dir_all(root.join("sub")).unwrap();
        let ws = Workspace::new([root.join("sub")]).unwrap();
        let err = ws.resolve("..").unwrap_err().to_string();
        assert!(err.contains("outside this workspace"), "got {err}");
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn an_absolute_path_elsewhere_is_refused_and_the_error_names_the_roots() {
        // An error the caller cannot diagnose gets worked around by disabling confinement.
        let root = scratch();
        let ws = Workspace::new([&root]).unwrap();
        let outside = std::env::temp_dir();
        let err = ws.resolve(outside.to_str().unwrap()).unwrap_err().to_string();
        assert!(err.contains("outside this workspace"));
        assert!(err.contains(&strip_verbatim(&root)), "got {err}");
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    #[cfg(unix)]
    fn a_symlink_pointing_out_is_refused() {
        // The case a string scan for `..` misses entirely, which is why resolution happens
        // before the comparison.
        let root = scratch();
        let inside = root.join("inside");
        fs::create_dir_all(&inside).unwrap();
        let target = scratch();
        std::os::unix::fs::symlink(&target, inside.join("escape")).unwrap();
        let ws = Workspace::new([&inside]).unwrap();
        assert!(ws.resolve("escape").is_err(), "a symlink out is still out");
        fs::remove_dir_all(&root).ok();
        fs::remove_dir_all(&target).ok();
    }

    #[test]
    fn a_relative_path_resolves_against_the_root_not_the_process_cwd() {
        // The daemon's cwd is invisible to the caller; resolving against it would make one
        // request mean different things depending on how the daemon was launched.
        let root = scratch();
        fs::create_dir_all(root.join("marker")).unwrap();
        let ws = Workspace::new([&root]).unwrap();
        let got = ws.resolve("marker").unwrap();
        assert!(got.starts_with(&root));
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_workspace_with_no_readable_root_refuses_to_exist() {
        assert!(Workspace::new(["definitely-not-here-4f2a"]).is_err());
    }

    #[test]
    fn a_missing_path_inside_a_root_is_an_error_not_a_silent_pass() {
        let root = scratch();
        let ws = Workspace::new([&root]).unwrap();
        let err = ws.resolve("no-such-dir").unwrap_err().to_string();
        assert!(err.contains("cannot resolve"), "got {err}");
        fs::remove_dir_all(&root).ok();
    }
}
