# Producer fixtures

Real output from the three producers that emit a `WorldState` **without linking
`scema-world`**, captured so that `import::tests` can pin the wire contract.

| File | Producer | Captured with |
|---|---|---|
| `mesh-world.json` | `scematica_mesh::omni` (Rust, bot workspace) | `mesh-dashboard --world <dir>` |
| `alchem-world.json` | `alchem_link.omni` (Python, stdlib only) | `alchem-link omni -n base` |
| `page-world.json` | `plugins/scema-web/src/perceive.js` (browser, no build step) | `perceive()` over a fixture DOM |

## Why these exist

`ImportObserver` is what makes omni's domain-agnosticism operational rather than merely
stated: a running Solana bot, a set of Chainlink aggregators and a DOM are all
`WorldState`, and none of the three can take a dependency on omni's crates — one is behind
a lockfile pinned around `solana-sdk 1.18`, one is a stdlib-only Python package, one is a
browser extension with no bundler.

So the contract is a JSON shape, and a JSON shape with three hand-written producers is a
JSON shape that will drift. Each producer restates the importer's validation on its own
side and fails its own tests; these fixtures close the loop from the other direction, by
asserting that what those producers **actually emitted** is what this crate **actually
accepts**.

The two halves catch different things. A producer's self-check catches a bug in that
producer. A fixture catches the case where both sides were changed and only one of them was
right.

## Recapturing

Regenerate after changing a producer, and read the diff rather than accepting it:

```console
# from the repository root
$ cargo run -q -p mesh-dashboard -- --world /path/to/a/bot/dir \
    > scematica-omni/crates/scema-tools/fixtures/mesh-world.json

$ cd alchem-link && PYTHONPATH=src python -m alchem_link.cli omni -n base \
    > ../scematica-omni/crates/scema-tools/fixtures/alchem-world.json
```

`page-world.json` comes from driving `perceive()` over a hand-built document; the
construction is in the capture command recorded in the git history for this directory, and
`plugins/scema-web/test/perceive.test.js` is where that observer's behaviour is really
pinned.

A fixture is a **captured observation**, not a hand-edited document. Editing one to make a
test pass is editing the evidence.
