# What 1.0 promises

**Status:** the compatibility promise for Scematica Omni 1.0.

1.0 on a verification runtime is not a maturity badge. It is one sentence:

> **A record sealed today still verifies tomorrow.**

Everything below is that sentence, made specific enough to be checkable — and
`corpus/` is what checks it, on every commit.

---

## Frozen

These cannot change in 1.x. A change to any of them moves a digest, and a digest that moves
turns a verifier into something that reports untouched history as tampered — the one failure
that teaches a reader to stop believing it.

| Frozen | Where |
|---|---|
| The canonical encoding | `scema_verify::canonical` — sorted keys, tagged types, normalised `-0.0` and NaN |
| Float quantisation at **1e-9** | `canonical::FIXED_SCALE`. Not a shortcut; see below |
| SHA-256 as the commitment hash | `scema-verify`, `scema-anchor` |
| Which fields a commitment covers | world, goal, hypotheses, projections, policy, decision — and for an effect: intent, effect, outcome |
| Which fields it deliberately does **not** cover | `id`, `at`, `runtime`. They describe the recording, not the thing recorded |
| The wire shape of `scema.world/1` | `scema-world` |
| `WorldState::schema` being optional | `Option` **and** `skip_serializing_if`. Both halves |
| Merkle construction | domain-separated leaves and nodes; odd nodes promoted, never duplicated |

### The two that look like details and are not

**`schema` is optional, and it is serialised only when present.** Records sealed before the
field existed carry no `schema` key at all. Making it required breaks their parsing; making
it serialise as `null` changes their canonical encoding and moves their digest. The corpus
holds two such records specifically so that either mistake is a red test. It has been
verified to fail: removing `skip_serializing_if` makes both of them report as tampered while
newer records pass, which is exactly the shape of the disaster.

**Floats are hashed as fixed-point, not as IEEE bits.** `serde_json`'s parser is not
correctly rounded for every 17-significant-digit input, so a record sealed in one process and
re-read in another came back one ULP low from its own identical text and reported INVALID on
a byte nobody had touched. Values bind to 1e-9: an edit at or above that resolution is
caught, one below it is not and cannot move any gate in `scema-policy`. Do not "simplify"
this back to hashing bits.

---

## Open — extensible without breaking anything

| Open | How |
|---|---|
| `Domain`, `EntityKind` | Open enums. Unknown names are held verbatim and round-trip byte for byte |
| New `WorldState` producers | The contract is JSON; `scema check` runs the importer's own rules |
| New `Effect` arms | Additive; each must declare a `Risk` |
| New `Anchor` chains | `chain` and `reference` are opaque strings on purpose |
| λ weights, policy configuration | Committed per record, so changing them cannot alter an old one |
| Everything above the CLI | Rust trait shapes, evaluator registration, TUI, daemon routes |

Adding a variant to an open enum, a producer, or an effect kind is a **minor** release. None
of them can change an existing record's digest, because an existing record does not contain
them.

---

## Not covered

Stated so nobody infers a promise that was not made.

- **Rust API stability below the CLI.** Trait shapes, module paths and constructor
  signatures may change in a minor release. If you embed the loop, pin a minor.
- **`.scema/` layout.** The directory is machine-local and full of absolute paths. Records
  are portable; the directory around them is not.
- **`scema-nft` plate bytes.** The plate is deterministic *for a given version* — that is
  what the Rust/TypeScript parity test asserts. It is not promised to be stable across
  versions, and it is not part of any commitment.
- **Anything an anchor asserts.** `--record` writes down that a root was published. It does
  not check, and says so. A reader follows the reference themselves.

---

## What `verify` proves, and the two things it does not

Unchanged by 1.0, because they are properties of the design and not of the version:

1. It **does** prove the record was not edited after sealing, and names the field that moved.
2. It does **not** prove the world was as described. Provenance carries that, which is why
   the world state is committed whole — `Absent` arms and blind spots included.
3. It does **not** prove this is the original record. Tamper-evident, not tamper-proof, until
   the root is anchored somewhere the author does not control. `scema anchor` is the half
   that batches and proves; publishing is the half that needs a chain and a key.

---

## How this is enforced

`corpus/` holds real records sealed by builds that no longer exist, including two from before
`schema` existed. `cargo test -p scema-effect --test corpus` re-verifies every one of them,
and the `omni` CI job runs it on every commit.

The corpus is **never regenerated**. A re-sealed record agrees with today's build by
construction and detects nothing. When a corpus record fails, the change under test is
almost certainly the thing that is wrong.

---

## Breaking any of this

A frozen item changes in **2.0**, with `scema.world/2`, a migration path written *before* it
is needed, and both versions readable for at least one major cycle. `parse_schema` exists so
an importer can say which side is out of date rather than misreading silently — that was the
entire reason the contract got a version in 0.5.0, and it is what makes a future break
survivable rather than a rug.
