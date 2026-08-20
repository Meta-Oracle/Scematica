# scema-tools

**Perception, and workspace confinement.**

Part of [Scematica Omni](https://github.com/Meta-Oracle/Scematica/tree/main/scematica-omni) —
an agent runtime that perceives an environment, projects competing futures, ranks them under
a stated preference, decides *or refuses to*, and seals a verifiable record of what it did.

The organising idea across every crate: **each layer can say "I don't know", and saying it
costs nothing.** An agent that cannot express ignorance expresses a number of the right shape
instead, and nothing downstream can tell it from a measurement.

---

The only crate in the read path allowed to touch the outside world. `Observer` is the
interface; `RepoObserver` walks a source tree and counts what can be counted.

Three obligations on every observer:

1. **Report what could not be read** — into `blind_spots`, which becomes measured uncertainty
   downstream. Ignorance the observer knows about is the most useful thing it can pass up.
2. **Never round an unread thing to zero.** An unreadable unit is `Provenance::Absent` with no
   attributes, not a unit with zeroes.
3. **Say whether the walk was complete** — `Extent { total: None }` when a cap was hit.

A deliberate exclusion is *not* a blind spot: skipping `target/` is a decision, not a failure,
and filing it as ignorance buries the paths that really could not be read.

`Workspace` also lives here, and answers **where** only — resolve fully (symlinks followed,
`..` collapsed) *then* compare against roots, because a string scan for `..` passes a symlink
pointing at `/`. Used by the daemon and the MCP server, whose callers are a browser extension
and a language model.

---

Licensed MIT.
