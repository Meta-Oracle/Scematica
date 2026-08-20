# scema-world

**The universal world-state representation.**

Part of [Scematica Omni](https://github.com/Meta-Oracle/Scematica/tree/main/scematica-omni) —
an agent runtime that perceives an environment, projects competing futures, ranks them under
a stated preference, decides *or refuses to*, and seals a verifiable record of what it did.

The organising idea across every crate: **each layer can say "I don't know", and saying it
costs nothing.** An agent that cannot express ignorance expresses a number of the right shape
instead, and nothing downstream can tell it from a measurement.

---

The bottom of the runtime. `WorldState`, `Goal`, `Hypothesis`, `Term`, `Provenance` — the
types every layer above is written against, so one agent loop serves a git repository, a web
page and a market without a branch per domain.

**Pure**: no I/O, no clock, no network. Two dependencies (`serde`, `serde_json`), because its
JSON shape is a wire format other implementations have to match — the browser extension emits
exactly this.

Three rules it exists to enforce:

1. **`Provenance` before value.** An unreadable object is `Absent` and carries *no value at
   all*. Rendering it as `0` turns "we could not see this" into "this is empty" — an
   accusation rather than an observation.
2. **`Term` carries its own evidence.** An unmeasured quantity contributes the neutral element
   and is flagged, so a score can never quietly stand on nothing. Every aggregate ships a
   `Coverage` beside it.
3. **Ordered containers only.** `scema-verify` hashes these structures; a `HashMap` would give
   one world two digests and make every decision record unverifiable.

---

Licensed MIT.
