# scema-cli

**`scema` - the agent runtime from a terminal.**

Part of [Scematica Omni](https://github.com/Meta-Oracle/Scematica/tree/main/scematica-omni) —
an agent runtime that perceives an environment, projects competing futures, ranks them under
a stated preference, decides *or refuses to*, and seals a verifiable record of what it did.

The organising idea across every crate: **each layer can say "I don't know", and saying it
costs nothing.** An agent that cannot express ignorance expresses a number of the right shape
instead, and nothing downstream can tell it from a measurement.

---

```console
$ scema observe .
$ scema simulate "clear the marker backlog" --ground markers:scema-tools   # writes nothing
$ scema decide   "clear the marker backlog" --ground markers:scema-tools   # seals a record
$ scema explain 58898030 ; scema verify --all
$ scema policy
```

`simulate` never persists — it is a counterfactual, and a record it left behind would later
read as a decision the agent made. `decide` seals a record and appends memory. Both compute
exactly the same thing.

`execute`, `delegate`, `discover` and `pay` are registered verbs that exit non-zero saying
what is missing. They are in `--help` on purpose: the shape of the runtime includes an action
path, an agent-to-agent path and a payment path, and finding that out from the tool beats
finding it out later.

An unmeasured term renders as an em dash, never `0.00`. A column of numbers is the most
persuasive thing a program can emit, and the moment a neutral element renders as a number the
distinction the type system has been protecting is gone in the last hundred lines of the
program.

---

Licensed MIT.
