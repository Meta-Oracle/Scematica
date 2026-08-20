# scema-sim

**Counterfactual projection.**

Part of [Scematica Omni](https://github.com/Meta-Oracle/Scematica/tree/main/scematica-omni) —
an agent runtime that perceives an environment, projects competing futures, ranks them under
a stated preference, decides *or refuses to*, and seals a verifiable record of what it did.

The organising idea across every crate: **each layer can say "I don't know", and saying it
costs nothing.** An agent that cannot express ignorance expresses a number of the right shape
instead, and nothing downstream can tell it from a measurement.

---

Takes a world, a goal and a set of hypotheses; returns expected gain, risk, cost, uncertainty
and reversibility per branch — each a `Term` that says whether anybody measured it.

> **A projection may not invent a number.**

A simulator that outputs `+31% predicted performance` for a refactor nobody benchmarked has
produced a hallucination with a decimal point on it, and the decimal point is what makes it
dangerous: it survives into a ranking, a report and a record looking exactly like a
measurement. So `StructuralSimulator` scores an expected gain **only** from signals the
observer actually counted.

The consequence is uncomfortable and correct: on a barely-perceived world most branches
project exactly zero and the agent abstains. That is the true answer.

---

Licensed MIT.
