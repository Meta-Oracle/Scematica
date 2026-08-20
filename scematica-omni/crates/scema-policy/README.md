# scema-policy

**Deciding, and declining to decide.**

Part of [Scematica Omni](https://github.com/Meta-Oracle/Scematica/tree/main/scematica-omni) —
an agent runtime that perceives an environment, projects competing futures, ranks them under
a stated preference, decides *or refuses to*, and seals a verifiable record of what it did.

The organising idea across every crate: **each layer can say "I don't know", and saying it
costs nothing.** An agent that cannot express ignorance expresses a number of the right shape
instead, and nothing downstream can tell it from a measurement.

---

`U = R - L1*K - L2*C - L3*U + L4*V`, plus pluggable evaluators that may decline, plus the five
ways a decision can refuse to pick anything.

**The equation is additive, and that is a safety property.** A multiplicative form is more
expressive and it is a trap: an unmeasured factor is either `1.0` (the equation lies about
certainty) or `0.0` (the score is pinned shut by dimensions nobody built). Additive terms with
a `0.0` neutral make ignorance *silent rather than fatal*, and visible in the coverage rather
than smuggled into the number. The weights are a stated preference, never a fitted parameter,
and they are hashed into every record.

**`Applicability` is the whole design of the evaluator trait.** A specialist must be able to
say "not my domain" and "my domain, but I lack the inputs" as *different* answers — the first
is permanent and fine, the second is a missing file an operator can go and supply. The
optional `dqstar` feature wires in Scematica's trained Dueling Double-DQN as **one evaluator
among several**; on a software world it declines, because its 24-feature state is pool and
position data and it would otherwise emit five finite Q-values that are correctly shaped and
entirely meaningless.

Specialist opinions are *attached*, never averaged into the ranking — a general utility and a
normalised Q-value are not the same quantity. A qualified specialist's measured negative
vetoes outright. A veto is legible; a blend of incommensurable scores is not.

`render` lives here too, and is the only place in Rust a `Term` becomes a string: an unmeasured
term prints an em dash, never `0.00`.

---

Licensed MIT.
