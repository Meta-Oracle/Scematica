---
name: omni-decision-records
description: How to read Scematica Omni output without misreporting it — em dashes are not zeros, coverage never leaves the score it qualifies, abstention is an answer, and a verified commitment proves less than it looks like it proves. Use whenever reading or summarising anything from the scema MCP server, a .scema/decisions/*.json record, or `scema` CLI output.
---

# Reading Scematica Omni output

Omni's whole design is that **every layer can say "I don't know", and saying it costs
nothing**. That only works if the last layer — you, writing a summary — preserves the
distinction. A model that reports an em dash as a zero has undone the entire type system
underneath it, in one sentence, and nothing downstream can tell.

Five rules. They are short because each one is a single thing not to do.

## 1. An em dash is not a zero

`—` in any column means **nobody measured that term**. It contributed the additive neutral
element (`0.0`) to the sum, which is not the same claim as "this was measured and it was
zero" — and a measured zero prints as `0.00`, precisely so the two can be told apart.

```
   1 take: 11 marker(s) in `scema-tools`   0.22   0.40   0.10   0.00   0.70    0.125   5/5
   2 add tests to the scema-cli crate         —   0.40   0.10   0.00   0.70   -0.095   4/5
```

Branch 2 has no measured expected gain. Correct: "nothing observed supports this branch."
Wrong: "this branch has zero expected gain" — that asserts a measurement that was not made.

Never average over an em dash, never sum it as zero in prose, and never fill it in from
context.

## 2. Coverage never leaves the score it qualifies

Every utility, every Ψ-style aggregate, every ranking ships a `measured` fraction like
`2/5` or a meter like `▰▰▱▱▱`. Quote it whenever you quote the number. `0.91` on two terms
out of nine and `0.91` on nine out of nine are different claims that print identically.

If the fraction is below the configured floor the agent abstains, and the abstention says so
— that is not the agent failing to decide, it is the agent reporting how little was observed.

## 3. Abstention is an answer, and which one matters

Five reasons, five different instructions to the reader:

| Reason | What it means | What to tell the reader |
|---|---|---|
| `no_candidates` | nothing was proposed | the world produced no counted signals to build a branch from |
| `all_forbidden` | every branch hits a constraint | the goal is unsatisfiable as stated; relax a `must_not` or restate it |
| `no_positive_utility` | the best branch scores ≤ 0 | acting is worse than not acting — accept it, or lower the bar deliberately |
| `too_little_measured` | coverage under the floor | this is about how little was observed, **not** about the branches; go and observe more |
| `contested` | a qualified specialist disagrees | read the specialist's note before overriding it |

Never report an abstention as "the tool did not work" or "no result". `scema decide` exits
**0** when it abstains, on purpose: a script that treated declining as a crash would get
rewritten to ignore the exit code, and then it would ignore real crashes too.

## 4. Grounding is asserted, never inferred

A goal is an **instruction**, and an instruction is not evidence. A branch gets a measured
expected gain only when the operator asserts, via `ground`, that it addresses a signal the
observer actually counted.

Do not pick a `ground` id because a word in the goal matches a word in the signal. That
inference existed once and was removed the first day it ran: it grounded "add tests to the
scema-cli crate" in a marker backlog in a *different* crate, because `scema` is a substring
of every unit name in the workspace. The branch then inherited a measured gain from evidence
that had nothing to do with it — exactly the laundering the simulator refuses to do.

If nothing observed supports the goal, ground it in nothing. The abstention that follows is
the true answer.

## 5. What a verified commitment proves — and the two things it does not

`omni_verify` / `scema verify` recomputes six digests and a root over them.

**Proves:** the record was not edited after it was sealed. On a mismatch it names *every*
field that moved.

**Does not prove:** that the world was as described. Provenance carries that. A record whose
world is full of `Absent` objects and blind spots verifies perfectly and describes a world
nobody could see.

**Does not prove:** that this is the original record. It is tamper-**evident**, not
tamper-proof, until the root is anchored somewhere the author does not control.

State all three. Compressing them into "the record is valid" teaches the reader to
over-trust the verifier, which is the failure mode that makes a verifier worse than none.

Also: **unreadable is a third state.** A record that could not be parsed is not an invalid
one. One is a gap; the other is an accusation.

## Provenance, when you meet it

`Live` / `Stale` / `Absent` / `Simulated`, and the ordering rule is that you answer *can
this be seen?* before you report the value.

- `Stale` is **not** actionable. A value that was true an hour ago looks exactly like one
  that is true now, and that resemblance is the hazard. Never present a stale reading as
  current.
- `Absent` carries **no value at all**. Rendering it as `0` turns "we could not read this"
  into "this is empty" — an observation becomes an accusation.
- `Simulated` is labelled at every point it surfaces, without exception.

## The tools

| Tool | Does | Writes |
|---|---|---|
| `omni_observe` | perceive a path → world state, signals, blind spots | no |
| `omni_simulate` | rank branches against a goal | **no** |
| `omni_decide` | same computation, seals a record | yes — usually not advertised |
| `omni_records` | list sealed records | no |
| `omni_explain` | re-read one record | no |
| `omni_verify` | recompute a commitment | no |
| `omni_policy` | λ weights, gates, observers, specialists | no |
| `omni_memory` | per-kind counts and calibration | no |

`omni_simulate` and `omni_decide` compute **exactly** the same thing and differ only in
whether they leave a trace. Prefer `simulate`. If `omni_decide` is not in the tool list, the
server was started without `--allow-decide` and that is the default — do not work around it.

Paths resolve through a workspace root. A refused path comes back as a tool *result* with
`isError`, not a protocol error, and it names the root: correct the path rather than
concluding the server is broken.

## Calibration reads as mostly unresolved, and that is right

`omni_memory` reports counterfactuals `recorded`, `resolved` and `unresolved` separately, and
`mean_abs_error` is `None` — not `0.0` — when nothing resolved. A branch nobody ran has no
outcome. Imputing one would mean the loop generating its own training signal, and every later
decision would be tuned to a fiction. Report `unresolved` as the expected state, not as
missing data.
