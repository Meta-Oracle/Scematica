//! # scema-tools — perception
//!
//! The only crate in the read path allowed to touch the outside world. [`Observer`] is the
//! interface; [`RepoObserver`] is the first implementation, and the browser extension will
//! be the second — a content script that produces the same [`scema_world::WorldState`] JSON
//! from a page is indistinguishable to everything above it.
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

pub mod observer;
pub mod repo;
pub mod workspace;

pub use observer::{resolve, Observer};
pub use repo::RepoObserver;
pub use workspace::Workspace;

/// The observers compiled into this build, in resolution order.
///
/// Ordered, not scored: [`resolve`] takes the first that claims a locator. `RepoObserver`
/// is last because it accepts almost any non-URL string, so a more specific observer added
/// later must go in front of it.
pub fn default_observers() -> Vec<&'static dyn Observer> {
    static REPO: RepoObserver = RepoObserver;
    vec![&REPO]
}
