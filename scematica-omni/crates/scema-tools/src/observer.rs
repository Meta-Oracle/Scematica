//! [`Observer`]: the only interface between the agent and a real environment.
//!
//! Everything above this trait reasons over a [`WorldState`] and cannot tell whether it
//! came from a filesystem, a browser tab or an HTTP API. That is what makes one loop serve
//! every domain, and it is why the trait is deliberately narrow: an observer *looks*. It
//! does not act, it does not decide, and it does not summarise on the agent's behalf.
//!
//! ## Three obligations on every implementation
//!
//! 1. **Report what could not be read.** A directory that raised a permission error belongs
//!    in [`WorldState::blind_spots`], not in a log the agent never sees. Ignorance the
//!    observer knows about is the single most useful thing it can pass upward.
//! 2. **Never round an unread thing to zero.** An object whose attributes could not be
//!    recovered is [`scema_world::Provenance::Absent`] with no attributes, not an object
//!    with zeroes.
//! 3. **Say whether the walk was complete.** [`scema_world::Extent`] with `total: None`
//!    when a cap or a depth limit was hit. An observer that silently truncates makes the
//!    agent confident about a fraction of a system.
//!
//! A deliberate exclusion is *not* a blind spot. Skipping `target/` and `node_modules/` is
//! a decision the observer made, not a failure it suffered, and filing it as ignorance
//! would drown the real unreadable paths in noise.

use anyhow::Result;
use scema_world::WorldState;

/// Something that can turn a locator into a world state.
pub trait Observer {
    /// Stable name; recorded in `WorldState::observer` and hashed into the decision record.
    fn name(&self) -> &str;

    /// One sentence for `scema observe --list`.
    fn about(&self) -> &str;

    /// Could this observer handle the locator? Cheap and syntactic — a `true` here is a
    /// claim about the shape of the string, not a promise the target exists.
    fn handles(&self, locator: &str) -> bool;

    fn observe(&self, locator: &str) -> Result<WorldState>;
}

/// Pick the first observer that claims a locator.
///
/// First match rather than best match: the registry is small and ordered by the caller, and
/// a scoring contest between observers would be a second policy layer with no way to
/// explain itself.
pub fn resolve<'a>(observers: &'a [&'a dyn Observer], locator: &str) -> Option<&'a dyn Observer> {
    observers.iter().copied().find(|o| o.handles(locator))
}
