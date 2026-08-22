# Writing a producer

**Contract: `scema.world/1`**

Omni is domain-agnostic because it never learns to perceive new things. Instead, **the thing
being observed describes itself** in `scema-world`'s vocabulary, and omni reads that. A
repository, a running trading system, a set of Chainlink oracle feeds and a web page are all
`WorldState`, and nothing above perception can tell which it was looking at.

So teaching omni about a new kind of environment does not mean changing omni. It means
emitting one JSON document, in whatever language that environment already lives in.

```console
$ my-tool --world | scema check -          # is it valid?
$ my-tool --world | scema observe -        # what does omni see?
$ my-tool --world | scema simulate "…" --path -
```

There are four producers on this contract today and only one is written in a language
omni's crates can link, which is exactly why the contract is a JSON shape rather than a
trait:

| Producer | Language | Emits |
|---|---|---|
| `scema_tools::RepoObserver` | Rust, in-process | a source tree |
| `plugins/scema-web/src/perceive.js` | JavaScript, no build step | a DOM |
| `scematica_mesh::omni` | Rust, another workspace | a running Scematica system |
| `alchem_link.omni` | Python, stdlib only | one network's oracle feeds |

---

## The shape

```jsonc
{
  "schema": "scema.world/1",          // required
  "observer": "my-tool",              // who did the looking
  "entity": {
    "kind": "cluster",                // open vocabulary — see below
    "locator": "k8s://prod/default",  // how to find this again. required
    "label": "production namespace"
  },
  "domain": "infrastructure",         // open vocabulary
  "observed_at": 1787255266,          // unix seconds

  "objects": [ /* things, with attributes and provenance */ ],
  "facts":   [ /* subject-predicate-object, with a confidence */ ],
  "signals": [ /* counted things worth acting on */ ],

  "extent": { "observed": 18, "total": 18, "note": "walked the namespace" },
  "blind_spots": [ "kube-system: forbidden — unread, not empty" ]
}
```

Run `scema check --vocabulary` for the domains and entity kinds this build knows. Both lists
are **open**: a name that is not listed is legal and is carried through untouched. Prefer a
listed name where one fits — nothing can tell `k8s` from `kubernetes`, and a synonym is the
one kind of drift an open vocabulary cannot repair.

### Objects

```jsonc
{
  "id": "listener.pools",
  "kind": "listener",
  "label": "Pool listener",
  "attrs": { "verdict": { "t": "text", "v": "pass" } },
  "provenance": { "kind": "stale", "age_secs": 516, "budget_secs": 120 }
}
```

`provenance` is one of:

| | Meaning |
|---|---|
| `{"kind":"live","age_secs":N}` | read just now |
| `{"kind":"stale","age_secs":N,"budget_secs":M}` | read, but past its freshness budget |
| `{"kind":"absent"}` | could not be read. **Carries no attributes at all** |
| `{"kind":"simulated"}` | computed, not observed |

Scalars are tagged: `{"t":"int"|"num"|"text"|"bool","v":…}`.

### Signals

A signal is the only thing that can produce a measured expected gain, so it is the part with
teeth.

```jsonc
{
  "id": "unseen-units",
  "polarity": "risk",                       // "risk" | "opportunity"
  "label": "14 unit(s) with no source on disk",
  "detail": "…",
  "magnitude": 0.78,                        // in [0,1]
  "measured": true,                         // did you COUNT this?
  "targets": ["learner.dqstar"],
  "evidence": ["counted 14 of 18 units"]    // required when measured
}
```

The `id` is what an operator types after `--ground`, so it has to be unique and stable.

---

## The five rules

Every one of these has been paid for at least once in this repository. They are enforced by
`scema check`, which runs the importer's own code — not a friendlier restatement of it.

### 1. An unreadable thing is a blind spot, never a zero

"We could not see this" and "this is empty" are different claims and only one of them is an
accusation. A `blind_spots` entry becomes *measured uncertainty* downstream, so the agent
ends up less sure and can say so with a number. A zero just quietly becomes a fact.

**A deliberate exclusion is not a blind spot.** Skipping `target/` is a decision. Filing it
as ignorance buries the paths that genuinely could not be read; say it in `extent.note`.

### 2. `measured: true` means somebody counted something

This is the rule the whole runtime is built on. A counted signal produces a real expected
gain, and nothing downstream can distinguish a fabricated count from a real one — so a
signal claiming `measured: true` with an empty `evidence` array is **refused on import**.

If you are estimating, say `measured: false`. It costs you nothing: an estimate is legal,
carries no citation requirement, and simply cannot manufacture a gain. Never invent a
"health score" or an "overall rating" — that is a hallucination with a decimal point on it,
laundered into a verifiable record.

### 3. Stale is not fresh, and it keeps its value

A reading past its freshness budget is `Stale` with the age and the budget attached. Do not
drop it, and do not present it as current. A veto from a stale source is history, not an
alarm.

### 4. An unknown denominator stays unknown

`extent.total` must be `null` when you capped the read or do not know the population.
Reporting a numerator over a smaller total claims over 100% coverage; reporting
`total == observed` when you were truncated claims completeness you cannot support.

### 5. Declare the schema

`"schema": "scema.world/1"`. A world with no declared contract version is refused, because
without it the next change to the format is a silent misread rather than an error message.
`scema check` distinguishes four cases — missing, malformed, a foreign contract, and a major
that is newer or older than the runtime — because "you are out of date" and "I am out of
date" are opposite instructions.

---

## Validate on your own side too

`scema check` catches a producer that is wrong. It cannot catch the case where the producer
*and* the importer were both changed and only one of them was right.

So every producer in this repository **restates the importer's validation in its own
language and fails its own test suite** — `alchem_link.omni._check`, the extension's
`perceive.test.js`, `scematica_mesh::omni::assert_importable`. And
`crates/scema-tools/fixtures/` holds real captured output from three of them, asserted
against the importer on every build. A self-check catches a bug in one producer; a fixture
catches the two-sided case.

`alchem_link.omni._check` caught a real extent bug the first time it ran. It is worth the
twenty lines.

```console
$ my-tool --world | scema check -
  stdin

  note  schema.ok                    contract scema.world/1
  note  signal.counts                14 signal(s), 12 counted
  warn  domain.unknown               `kubernetes` is not a domain this build knows. That is
                                     legal — the vocabulary is open — but every specialist
                                     will decline on it.

  Conforms to scema.world/1, with 1 warning(s). This world would import.
```

`scema check` exits 1 if the world would be refused, and 0 on warnings — so it drops
straight into CI. A warning never fails the run: an unfamiliar domain is the open vocabulary
working as designed, and failing on it would push authors back onto `unknown`, which is what
opening it was meant to stop.

---

## What omni does *not* check

The **shape**, always. The **claims**, never.

A producer that reports a stale feed as `Live`, or counts a signal it did not count, is
lying, and no parser catches that. The honest response is not a deeper check — it is the
`imported:` prefix omni stamps onto your `observer` field, which tells a reader of the
sealed record exactly whose word this is. A record can never claim that a world which
arrived on stdin was observed locally.

That prefix is the reason omni can afford to accept your document at all.

---

## Reference

- `crates/scema-world/src/world.rs` — the authoritative types.
- `crates/scema-tools/src/conform.rs` — every rule, with the reason it exists.
- `crates/scema-tools/fixtures/` — three real producers' captured output.
- `alchem-link/src/alchem_link/omni.py` — the shortest complete producer to copy, in Python.
- `plugins/scema-web/src/perceive.js` — the same, in dependency-free JavaScript.
