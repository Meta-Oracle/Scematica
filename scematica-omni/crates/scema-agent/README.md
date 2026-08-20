# scema-agent

**The orchestration loop.**

Part of [Scematica Omni](https://github.com/Meta-Oracle/Scematica/tree/main/scematica-omni) —
an agent runtime that perceives an environment, projects competing futures, ranks them under
a stated preference, decides *or refuses to*, and seals a verifiable record of what it did.

The organising idea across every crate: **each layer can say "I don't know", and saying it
costs nothing.** An agent that cannot express ignorance expresses a number of the right shape
instead, and nothing downstream can tell it from a measurement.

---

```text
  observe -> hypothesise -> simulate -> score -> decide -> record -> remember
     ^                                                                  |
     +------------------------------------------------------------------+
```

Every stage is a trait with a real implementation, and a pass is deterministic: the same world
and goal produce the same record id. That is what makes a record verifiable by somebody who
was not there.

**It does not execute.** A cycle ends at a decision and a record; nothing here writes to the
environment it observed. The actions in a chosen hypothesis are a risk-classified,
constraint-checked *declaration of intent*, and turning one into a side effect needs a
separate approval model.

**An instruction is not evidence.** The goal branch is grounded only by `Goal::grounded_in`,
which the operator sets deliberately. An earlier version inferred it from wording and
immediately grounded "add tests to the scema-cli crate" in a marker backlog in a *different*
crate, because `scema` is a substring of every unit name in its host repository.

---

Licensed MIT.
