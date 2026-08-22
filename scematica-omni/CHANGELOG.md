# Changelog

## 0.5.0 — beta

The release that makes the "universal" claim structural rather than aspirational, and makes
the runtime survivable for somebody who has not read the README.

Two breaking changes to the JSON world contract, both deliberately taken **now**, before
public testing pins the format in other people's code.

### The contract is versioned

`WorldState` carries a `schema` field — `"scema.world/1"` — and a world that does not
declare one is refused on import.

The contract is a JSON shape implemented in four languages, only one of which can link
`scema-world`. There is no compiler anywhere between a producer and this format. Without a
version, the next change to it degrades into a silent misread; with one, an importer can say
which side is out of date and stop. `scema check` distinguishes **missing**, **malformed**,
a **foreign** contract, and a major that is **newer** or **older** than the runtime, because
"you are out of date" and "I am out of date" are opposite instructions to the reader.

**Records sealed before this release still verify.** The field is `Option` and is omitted
from the JSON when absent, so the digest of an existing record does not move. A verifier
that cried tamper on untouched history would be worse than no verifier — the first thing
anybody does with one is stop believing it. Pinned by
`a_record_sealed_before_the_world_contract_was_versioned_still_verifies`.

### `Domain` and `EntityKind` are open

Both were closed enums. `Domain` had four arms, which meant a perceived web page and a set
of Chainlink oracle feeds **both had to report `unknown`** — two entirely different worlds,
indistinguishable to every specialist downstream. Anyone observing a cluster, a dataset, a
document corpus or a CI pipeline had no way to say so at all, and no way to add one without
a release of this crate.

Now: known arms plus `Other(String)`, carried verbatim so it round-trips through a decision
record byte for byte. Parsing normalises case and padding; it does not guess synonyms, so
`k8s` and `kubernetes` stay two domains and the known names are published rather than
guessed at (`scema check --vocabulary`).

- New domains: `web`, `data`, `document`, alongside `software` / `infrastructure` /
  `trading` / `unknown`.
- New entity kinds: `dataset`, `document`, `cluster`.
- The browser extension now reports `web`; `alchem-link` reports `data`. An unfamiliar
  domain is a **warning**, never a failure — a check that failed on one would push authors
  back onto `unknown`, which is the thing opening the enum was meant to stop.

### Reversibility moved onto `Domain`, and trading was wrong

`edit_reversibility` was a `match` in the hypothesiser with a `_ => Unknown` fallback: fine
with four domains, quietly wrong the moment a producer could name its own, since every new
domain would land on the fallback without anyone reading that function again.

It is now a table on `Domain`, and it corrects a real understatement: **`Trading` is
`Irreversible`**, not `Unknown`. A filled order cannot be unfilled, and that was the one
domain here where irreversibility is certain rather than undetermined. `Infrastructure` and
`Data` are `Costly`. An `Other` domain stays `Unknown`, which propagates to an *unmeasured*
term and shows up in the coverage — an optimistic default here is exactly how an agent talks
itself into an irreversible action.

### `scema check` — conformance for producer authors

```console
$ my-tool --world | scema check -
$ scema check --vocabulary
```

Runs the importer's **own** rules from the new `scema_tools::conform`, not a friendlier
restatement of them. A checker that disagreed with the importer in either direction is worse
than no checker: it teaches an author that the tooling is unreliable and to route around it.

It reports **every** problem at once. The importer used to bail on the first violation,
which is fine for a guard and hostile as a development loop — an author with four problems
learned about them one release at a time. Findings carry a stable `code`, a message, and a
`fix`; exit 1 on failure, 0 on warnings, so it drops into CI.

See the new **[docs/PRODUCERS.md](docs/PRODUCERS.md)**.

### The two places a first run read as a malfunction

Both were correct behaviour rendered as silence, which makes them rendering bugs.

**The grounding cliff.** `scema simulate "fix the flaky tests"` produced an abstention and
nothing else. Every step of that is right — the goal branch is ungrounded, so no expected
gain can be measured, so it scores at or below zero — but the output read as the tool
disagreeing or being broken, when what was being asked for was one flag nobody had heard of.
`render::next_steps` now renders each of the five abstention reasons as a *different* next
command, and an ungrounded one lists the counted signal ids with the exact line to re-run.

It suggests and never acts. Grounding is asserted by a human; a runtime that filled in
`--ground` because it looked plausible would be the keyword-overlap bug again with a
friendlier face.

**"I asked for X and it did Y."** The quieter one, because it looks like success. When a
grounded signal branch outranks the operator's own goal, the runtime now says so and
explains why the goal lost, instead of printing an unrelated decision without comment.

### `scema quickstart`

The loop, narrated, over a directory you already have. Walks observe → simulate → ground →
decide, explaining each stage's output as it appears, and demonstrating that `--ground`
changes the answer rather than merely asserting it.

It **writes nothing** and stops before sealing, printing the command instead. A tutorial
that seals a record on someone's behalf has taught them the wrong thing about the one
command in this runtime that leaves a trace.

`scema --help` now leads with it, and closes with the four questions a newcomer actually
has.

### Docs

- **[GETTING-STARTED.md](GETTING-STARTED.md)** — the four words (a signal is a count; an em
  dash is not a zero; grounding is asserted; abstention is an answer), what a record proves
  and what it does not, and what is deliberately not built.
- **[docs/PRODUCERS.md](docs/PRODUCERS.md)** — the contract, the five rules, and why you
  should restate the validation on your own side.

---

## What is stable in beta, and what is not

Beta means you can build on this. It does not mean nothing moves. Concretely:

**Stable — a breaking change here gets a major bump and a migration note:**

- The `scema.world/1` JSON contract: field names, the `Provenance` arms, tagged scalars, the
  `measured` + `evidence` rule, `extent.total = null` for an unknown denominator.
- The canonical encoding and the 1e-9 float binding. A record sealed by 0.5.0 must verify
  under every later 0.x, and the `/omni` browser verifier and `scema verify` must continue
  to agree.
- `scema verify` and `scema explain` semantics, and the sealed record's field layout.
- Abstention being exit **0**, and `scema check` being exit 1 only on a failure.
- The daemon's four guards: loopback-only bind, `Host` check, bearer token, workspace
  confinement. No CORS header will start being emitted.

**Not stable yet — expect movement inside 0.x:**

- Rust API surface below the CLI. The crates are published so the loop can be embedded, but
  trait shapes (`Observer`, `Hypothesizer`, `Evaluator`, `Simulator`) may change.
- The exact λ weights and the `TooLittleMeasured` floor. They are a stated preference, and
  they are hashed into every record precisely so a ranking can be re-read against the
  preferences that produced it.
- The open vocabulary's *known* lists. Names get added; nothing gets removed, and an
  unlisted name has always been legal.
- Rendered text — the wording of findings and next-step blocks. Assert on `Finding::code`,
  which exists for that, not on the message.
- The MCP tool names and their JSON, and the daemon's route shapes.

**Still deliberately absent**, and not a beta gap: the action path (`execute`), agent-to-
agent (`delegate` / `discover`), payment (`pay`), and a model-backed hypothesiser. Nothing
in this workspace writes to an environment it observed. Each of those verbs is registered
and exits non-zero saying what is missing, because a verb that silently did not exist would
be indistinguishable from one that failed.

---

## 0.2.0

The console, universal perception, and assistant plugins.

- `scema-tui` — the console. Five tabs, black and violet, azure reserved for a claim.
- `ImportObserver` and the four-producer contract: a repository, a running Scematica system,
  one network's oracle feeds, and a DOM, all as `WorldState`.
- `scema-mcp`, `plugins/claude-code`, and `scema connect`.
- `plugins/scema-web` — MV3 browser extension, no build step.
- `/omni` in `web/` — an offline record verifier that runs entirely in the reader's browser.

## 0.1.0

First release. The loop, the six invariants, proof-carrying decision records.
