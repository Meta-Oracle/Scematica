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

## Reasoning over it: `--world`

The mesh answers *what is this system doing, and can each part of it be seen at all*. It
has never answered *so what should be done about it* — that has always been the operator's
job, and on a page showing three armed breakers and a vetoing DQ* it is not an easy one.

`scematica_mesh::omni` emits the topology as a Scematica Omni `WorldState`, so a live bot
becomes an environment the agent loop can rank branches against:

```console
$ mesh-dashboard --world | scema simulate "get the pipeline trading again" --path -
```

```
WORLD    .
         Service · Trading · observer `imported:mesh`
         20 observed (20 unit(s) in the roster; 0% currently visible) · legibility 0%
         BLIND SPOTS (9):
           · breaker.coherence (Coherence (Ψ)): no source on disk — unseen, not idle

SIGNALS
  RISK         counted      0.45  9 of 20 unit(s) cannot be seen at all
  RISK         counted      0.55  11 unit(s) were last written past their own budget
  RISK         counted      0.67  the gate stands on 33% measured terms
```

Four rules carry across the boundary unchanged, and they are the reason this is worth
having rather than a novelty:

- **An absent node becomes a blind spot, never an `activity: 0`.** `scema-sim` turns a
  blind spot into *measured* uncertainty, so an agent reasoning about a half-visible bot is
  less confident and can say so with a number. That is the same claim this crate has always
  made, now with arithmetic behind it.
- **A veto from a stale source is not counted as blocking.** "The DQ* is suppressing buys"
  and "the DQ* was suppressing buys when it last wrote, three months ago" are different
  sentences and only the first justifies acting — the distinction `MeshSummary` already
  draws between `blocking` and `blocking_stale`.
- **Ψ is only reported when something measured it.** A gate computed entirely on neutral
  elements says nothing about the system, and emitting it as a counted signal would launder
  "nobody checked" into "checked and fine". Unmeasured, it becomes a blind spot instead.
- **Every signal is a count.** Nothing here estimates a severity. A "system health score"
  invented in this module would be a hallucination with a decimal point on it, laundered
  into a decision record a third party can verify but cannot second-guess.

It is hand-built JSON rather than a dependency on `scema-world`, because the wire format is
the contract and three other producers are on it that could not take the dependency either
— the browser extension in JavaScript, `alchem_link.omni` in stdlib Python. What keeps that
honest is a test on each side plus a captured fixture in
`scematica-omni/crates/scema-tools/fixtures/`, which asserts that what this crate really
emits is what the importer really accepts.

## The observer, observed

The roster now includes `agent.omni`, read from `.scema/decisions/`. A topology that could
not see its own observer would be an odd kind of topology.

It is the only node in the mesh with **no edges at all**, and that is the claim rather than
an omission: nothing in the omni workspace writes to an environment it observed, so there
is no wire from it into the buy path. Drawing one would assert coordination that is not
happening, which is the same thing this crate refuses to do with `EdgeKind::Experience`.

It also **counts records without verifying them**. Verifying a commitment means recomputing
six SHA-256 digests under omni's canonical encoding, and this crate has neither the encoder
nor any business owning a second copy of it — a copy that drifts is worse than no copy, and
a verifier that reports an untampered record as INVALID teaches its reader to stop
believing it. The detail line says so:

```
  records      12
  unreadable   —
  newest       8994bb03
  measured     19/20
  verified     not checked here — run `scema verify --all`
```

An abstention renders as `Idle`, never `Veto`. Omni declining to choose is not the same act
as a breaker halting the buy path; it blocks nothing, and painting it as a veto would put
the page in alarm over an agent behaving exactly as designed — and inflate
`MeshSummary::blocking`, which is the number an operator reads to answer "why is nothing
trading".

## Not to be confused with

`scema-bot-mesh`, which lives in its own workspace and solves a different problem
(deterministic, challengeable neural inference for BOT Chain). This crate makes no
cryptographic claim and runs no inference. It observes.

## v0.0.2

`EdgeKind::Experience` exists and is always inactive, because the tournament variants do
not share experience even though `scemadex_sdk::mesh::ExperienceBatch` and `PeerMarket`
were designed for exactly that. Lighting that edge up for real is a capability addition.
Drawing it lit before then is not.
