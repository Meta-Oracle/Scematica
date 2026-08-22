//! # scema-tools — perception
//!
//! The only crate in the read path allowed to touch the outside world. [`Observer`] is the
//! interface and [`RepoObserver`] is the first implementation.
//!
//! [`ImportObserver`] is the second, and it is what makes omni's domain-agnosticism
//! operational rather than merely stated. A source tree can be perceived here because it is
//! a filesystem walk in Rust. A running Solana bot, a set of Chainlink oracle feeds and a
//! DOM cannot be — they live behind another lockfile, a Python package and a browser — and
//! linking any of them would make this crate a hub of domain dependencies, which is exactly
//! what the workspace note forbids. So the thing being observed **describes itself in
//! `scema-world`'s vocabulary**, and this crate reads that. There are four producers on that
//! contract now and only one of them is written in a language this crate can link.
//!
//! [`Workspace`] lives here too, and it belongs to the *read* path for a reason that is easy
//! to miss: the CLI has an operator typing paths and needs no confinement, but the daemon
//! and the MCP server take paths from a browser extension and a language model. "Observe
//! this directory" from either of those is an instruction from somewhere the operator is
//! not looking.
//!
//! Actuators (the write path) are not here yet. That is deliberate rather than unfinished:
//! the loop is worth trusting with a keyboard only after the decision layer above it has
//! been watched abstaining on real inputs for a while, and `scema execute` says so rather
//! than pretending.

pub mod conform;
pub mod import;
pub mod observer;
pub mod repo;
pub mod workspace;

pub use conform::{conform, has_failure, Finding, Level};
pub use import::ImportObserver;
pub use observer::{resolve, Observer};
pub use repo::RepoObserver;
pub use workspace::Workspace;

/// The observers compiled into this build, in resolution order.
///
/// Ordered, not scored: [`resolve`] takes the first that claims a locator. `RepoObserver`
/// is last because it accepts almost any non-URL string, so a more specific observer added
/// later must go in front of it.
///
/// [`ImportObserver`] is therefore first. Its grammar is deliberately narrow — `-` and a
/// `.json` suffix — precisely because being first means anything it claims, the repo
/// observer never sees.
pub fn default_observers() -> Vec<&'static dyn Observer> {
    static IMPORT: ImportObserver = ImportObserver;
    static REPO: RepoObserver = RepoObserver;
    vec![&IMPORT, &REPO]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_import_observer_is_ahead_of_the_repo_observer() {
        // First-match resolution, so order is the whole registry. If `RepoObserver` came
        // first it would claim `mesh.json` as a directory to walk and fail with a confusing
        // error rather than importing it.
        let obs = default_observers();
        assert_eq!(obs[0].name(), "import");
        assert_eq!(resolve(&obs, "mesh.json").unwrap().name(), "import");
        assert_eq!(resolve(&obs, "-").unwrap().name(), "import");
    }

    #[test]
    fn a_directory_still_goes_to_the_repo_observer() {
        // The thing that would break if the import grammar were widened.
        let obs = default_observers();
        assert_eq!(resolve(&obs, ".").unwrap().name(), "repo");
        assert_eq!(resolve(&obs, "/some/project").unwrap().name(), "repo");
    }
}
