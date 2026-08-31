# Producer fixtures

Real output from the three producers that emit a `WorldState` **without linking
`scema-world`**, captured so that `import::tests` can pin the wire contract.

One of them emits two kinds of world. `alchem-link` describes the same oracle set
either as an instant or as a window of history, and the second is pinned separately
because it is where fabrication is easiest: every figure in a price window is a real
number of a plausible shape, and a volatility invented from two prints is
indistinguishable from one measured over three hundred once it is inside a record a
third party can verify but not second-guess. Both worlds carry the same entity locator
and the same `feed:<pair>` object ids, deliberately — one subject observed two ways.

| File | Producer | Captured with |
|---|---|---|
| `mesh-world.json` | `scematica_mesh::omni` (Rust, bot workspace) | `mesh-dashboard --world <dir>` |
| `alchem-world.json` | `alchem_link.omni` (Python, stdlib only) | `alchem-link omni -n base` |
| `alchem-window-world.json` | `alchem_link.omni` (Python, stdlib only) | `alchem_link.omni.windowed_world` over a captured history |
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

`alchem-window-world.json` is the exception: `alchem-link omni --window` needs six hours of
event logs from a live archive node, and a fixture that can only be regenerated against a
paid endpoint is one nobody regenerates. It is built instead by calling the **pure**
transform `alchem_link.omni.windowed_world` over hand-built histories chosen so every branch
fires at once — a steady feed, a thin one, one that gaps, one that has run away from its own
TWAP, one that fell, one stale at the window end, one that returned nothing, one whose scan
was capped, and the rest simply absent. That is still real producer output: the transform is
the producer, and `perceive_window` only does the reading.

`page-world.json` comes from driving `perceive()` over a hand-built document; the
construction is in the capture command recorded in the git history for this directory, and
`plugins/scema-web/test/perceive.test.js` is where that observer's behaviour is really
pinned.

A fixture is a **captured observation**, not a hand-edited document. Editing one to make a
test pass is editing the evidence.
