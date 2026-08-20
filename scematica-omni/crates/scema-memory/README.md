# scema-memory

**Four memories, one of them mostly unanswerable.**

Part of [Scematica Omni](https://github.com/Meta-Oracle/Scematica/tree/main/scematica-omni) —
an agent runtime that perceives an environment, projects competing futures, ranks them under
a stated preference, decides *or refuses to*, and seals a verifiable record of what it did.

The organising idea across every crate: **each layer can say "I don't know", and saying it
costs nothing.** An agent that cannot express ignorance expresses a number of the right shape
instead, and nothing downstream can tell it from a measurement.

---

Episodic (what happened), semantic (what is believed), procedural (how things are done), and
**counterfactual** (what the branch we declined would have done). Append-only JSONL.

The fourth is the point. A counterfactual records a branch the agent *did not take*, so its
projected utility is known and its realised outcome almost never is — nobody ran it. The rule
that falls out:

> **Unresolved counterfactuals are counted, never scored.**

`Calibration::mean_abs_error` is `None`, not `0.0`, when nothing resolved. Imputing outcomes
for untaken branches — from a model, a neighbour, a prior — would mean the system generating
its own training signal, and every later decision tuning itself to a fiction.

A corrupt line is skipped *and counted*, never fatal: one half-written record from a killed
process must not make the agent amnesiac, and must not be swallowed either.

---

Licensed MIT.
