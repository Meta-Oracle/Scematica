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

## `ImportObserver` — a world perceived somewhere else

The second observer, and the one that makes omni's domain-agnosticism *operational* rather
than merely stated.

`RepoObserver` can perceive a source tree because it is a filesystem walk in Rust running in
this process. It cannot perceive a running Solana bot, a set of Chainlink oracle feeds or a
DOM — those live behind a lockfile pinned around `solana-sdk 1.18`, a stdlib-only Python
package, and a browser. Linking any of them would make this crate a hub of domain
dependencies, which is exactly what the workspace note forbids.

So the thing being observed **describes itself in `scema-world`'s vocabulary**, and this
crate reads that:

```console
$ mesh-dashboard --world | scema simulate "get it trading again" --path -
$ alchem-link omni -n base > feeds.json && scema observe feeds.json
```

Four producers sit on that contract — `RepoObserver` here, `perceive.js` in the browser
extension, `scematica_mesh::omni` in the bot workspace, `alchem_link.omni` in Python — and
only the first is written in a language this crate can link.

**The observer field is always rewritten.** Whatever the producer called itself, it is
prefixed `imported:`, exactly as the daemon prefixes a wire-supplied world with `client:`. A
decision record can therefore never claim a world that arrived as a file was observed
locally, and a reader can see in one field which it was. The rewrite is idempotent: a world
passed through two stages does not become `imported:imported:mesh`.

**It validates the shape, not the claims.** A duplicated signal id (`--ground` could not
name it), a magnitude outside `[0,1]` (it would dominate a ranking by arithmetic rather than
by importance), an extent whose numerator exceeds its denominator, and — the one that
matters — a signal claiming `measured: true` while citing no evidence, which is a guess
wearing a measurement's clothes.

It does **not** validate that the producer told the truth. A producer reporting a stale feed
as `Live` is lying and no amount of parsing catches that. The honest response is not a
deeper check; it is the `imported:` prefix, which tells a reader whose word this is.

`fixtures/` holds real captured output from all three external producers, and
`import::tests` asserts that what they actually emit is what this crate actually accepts. A
producer's own self-check catches a bug in that producer; a fixture catches the case where
both sides changed and only one of them was right.

## Three obligations on every observer

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
