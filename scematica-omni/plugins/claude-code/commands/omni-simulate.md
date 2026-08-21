---
description: Rank branches against a goal without writing anything, and read the matrix correctly
argument-hint: "<goal> [--ground <signal-id> ...]"
allowed-tools: mcp__scema__omni_observe, mcp__scema__omni_simulate
---

Run the omni loop against this goal: **$ARGUMENTS**

Procedure, and the first step is not optional:

1. Call `omni_observe` first and read the signal ids. You cannot ground a goal in a signal
   you have not seen, and guessing an id produces a silent abstention that looks like
   disagreement rather than a typo.
2. Decide which counted signals — if any — this goal actually addresses, and pass their ids
   in `ground`. **Do not ground a goal in a signal merely because a word matches.** That
   exact inference was tried in this runtime and removed: it grounded "add tests to the
   scema-cli crate" in a marker backlog in a *different* crate, because `scema` is a
   substring of every unit name in the workspace. If nothing observed supports the goal,
   ground it in nothing and let it score at or below zero.
3. Call `omni_simulate`. It writes nothing.

When you report the result:

- An **em dash is not a zero.** It means nobody measured that term and it contributed
  nothing to the utility beside it. Never restate one as `0.00`, and never average over it.
- Always give the **measured fraction** with any utility you quote. A utility of `0.91`
  computed on two terms out of nine is a statement about ignorance.
- If the agent **abstained**, the reason is the useful part. Each of the five reasons sends
  the reader somewhere different — report which one, and what it implies.
- If a specialist **declined**, say whether it was `out_of_domain` (permanent, fine) or
  `insufficient` (its domain, missing inputs — something the reader can go and supply).

Do not recommend acting on the top branch because it is the top branch. A ranking always
exists; sort five numbers and something comes first.
