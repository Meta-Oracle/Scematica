# scematica-mesh

The running system's own topology, collected from the files it leaves on disk.

```powershell
cargo run -p scematica-mesh --example dump           # text
cargo run -p scematica-mesh --example dump -- --json
cargo test -p scematica-mesh
```

## What it is

Scematica is already a multi-agent system and has never been visible as one. In one
sniper process there are three DQN variants competing in a tournament, regime nets that
swap in below ε 0.3, five LLM agents, six independent risk breakers that can each halt
buys, a filter pipeline where every stage is a decision, and a Ψ gate over all of it.
Until now, asking "why did nothing trade?" meant grepping JSON.

This crate reads those files and returns a graph: the decision-making units, the edges
between them, what each last decided, and — before any of that — **whether each can be
seen at all**.

Run against a real working directory it answers the question directly:

```
diagnosis  DQ* agent was blocking when it last wrote 4d ago: suppressing buys —
           bearish Q leads the best buy by 3.4x (threshold is +15%) — this reading is
           stale, so treat it as the last known state rather than the current one
```

## The rules

1. **A node exists because its source exists.** No file means a dark node, never an
   invented one and never a zero. `0 trades` and `cannot see the executor` are different
   claims and only one accuses the system of idleness.
2. **Provenance is per node.** One live/stale banner is useless on a graph where one unit
   is fresh, one is three months stale, and five write nothing at all — which is the real
   state of this system today.
3. **Freshness budgets are per source**, derived from each writer's own cadence. The
   sniper rewrites metrics every 5s; the LLM strategy agent may not write for an hour.
4. **Rust is authoritative.** `web/lib/mesh/` renders; it does not decide.

## The agentic gate

`cognition.rs` implements §16, §17, §20, §22, §31, §32 and §33 of the Agentic Neural
Architecture spec over the observed mesh — the mesh being the only vantage point from
which subsystem coherence can honestly be measured, since no subsystem can measure its
own agreement with the others.

```
Ψ = C · K · (1 − R)
```

Every term carries `measured: bool` and cites its spec section. **An unmeasured dimension
is not a limiting factor** — it contributes the neutral element, and the result reports
`measured_fraction` so a Ψ of 0.95 computed on two inputs out of nine looks like what it
is. Implemented literally, §17's sigmoid and §34's product both pin the gate low or shut
on subsystems nobody has built; this repository already paid for that once, when an
unmeasured channel scored `0` jammed the sentience Ψ at `0` permanently.

Ω (§33) returns `None` until at least one of its five subsystems exists. Emitting a number
for an unbuilt architecture is the one thing this module will not do.

## Not to be confused with

`scema-bot-mesh`, which lives in its own workspace and solves a different problem
(deterministic, challengeable neural inference for BOT Chain). This crate makes no
cryptographic claim and runs no inference. It observes.

## v0.0.2

`EdgeKind::Experience` exists and is always inactive, because the tournament variants do
not share experience even though `scemadex_sdk::mesh::ExperienceBatch` and `PeerMarket`
were designed for exactly that. Lighting that edge up for real is a capability addition.
Drawing it lit before then is not.
