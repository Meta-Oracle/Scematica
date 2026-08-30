# The compatibility corpus

Real sealed records, kept so that a change to Omni which would stop them verifying fails
CI instead of reaching anybody.

This is the mechanism behind the 1.0 promise. `docs/COMPATIBILITY.md` states it in words;
this directory is what makes the words checkable. Every file here was produced by a real
`scema decide` or `scema execute` at the runtime version in its name, and
`cargo test -p scema-effect --test corpus` re-verifies all of them on every commit.

| File | Runtime | Why it is here |
|---|---|---|
| `0.1.0-before-the-schema-field.json` | 0.1.0 | Sealed before `WorldState::schema` existed. Proves the field's `Option` + `skip_serializing_if` is load-bearing: make it required, or serialise it as `null`, and this record stops verifying. |
| `0.2.0-before-the-schema-field.json` | 0.2.0 | A second pre-schema record, from a different world shape. One file could pass by accident. |
| `0.6.0-decision.json` | 0.6.0 | The current decision shape, with `schema: scema.world/1` present. |
| `0.6.0-effect.json` | 0.6.0 | An `EffectRecord` — a different commitment over different fields, so the corpus covers both record types rather than assuming one generalises. |

## Adding to it

Add a record when a release changes anything a commitment covers. Name it for the runtime
that sealed it.

**Never regenerate a file here.** The whole value is that these were sealed by code that no
longer exists; re-sealing one with today's build makes it agree with today's build by
construction and the test stops detecting anything. If a corpus record fails, the answer is
almost always that the change is wrong — not the record.

The one legitimate reason to remove a record is a deliberate, announced break, and that is a
major version.
