# The trust model

**Status:** specification, alchem-link 1.0. Python is the reference implementation
(`alchem_link.approvals`, `alchem_link.workspace`); this document is what a second
implementation has to match, and `vectors/trust-model.json` is what both of them run.

This exists because Scematica Omni cannot grow an action path until it has this, and
writing it twice from memory is how two implementations end up disagreeing about which
refusals are overridable. The same arrangement keeps `canonical.rs` and `canonical.ts`
honest: one stated rule, shared vectors, and whichever side fails is wrong.

---

## The shape of it

**Two gates, asked in order, answering different questions. Keep them separate.**

| Gate | Question | Refuses when |
|---|---|---|
| `Workspace` | **Where** may this act? | the resolved path is outside the root, or is protected |
| `TrustPolicy` + `Approver` | **Whether** may it act at all? | policy refuses, or the person says no |

Merging them is how a grant for one silently becomes a grant for the other. A user who
approves "write to `docs/`" has said nothing about whether `~/.ssh` is inside the
workspace, and a workspace that contains a path has said nothing about whether writing is
allowed today.

---

## 1. Risk is declared per tool, never inferred

Every tool carries exactly one `Risk`:

| Risk | Meaning | Rank | Mutating |
|---|---|---|---|
| `read` | reads local files or directory structure | 0 | no |
| `network` | reads a chain or an HTTP endpoint | 0 | no |
| `write` | creates, modifies, moves or deletes inside the workspace | 2 | yes |
| `execute` | runs an arbitrary command | 3 | yes |

Two things are deliberate.

`read` and `network` share rank 0 but are **not** the same risk, and neither is free.
A read's result goes to a third-party model, so reading a file is a disclosure, not an
inspection. Keeping them distinct lets a deployment refuse one without the other.

A new tool **cannot arrive unclassified**. Risk is a required field on the tool, not
something computed from its name or arguments — a tool called `fetch_and_apply_patch`
would be classified by whoever wrote it, and any scheme that guesses will eventually guess
low on the one tool where it matters.

---

## 2. Preflight order, and why a refusal is not a grant's business

`preflight(request)` returns a decision that needs no prompt, or "ask". The order is
fixed:

```
1. hard refusals        read_only ∧ mutating  →  DENY
                        execute ∧ ¬allow_execute  →  DENY
2. explicit rules       first matching rule wins
3. session grants       a previous "always" for this grant key
4. standing config      non-mutating → ALLOW
                        write ∧ allow_writes → ALLOW
                        otherwise → ask
```

**A refusal must never be reversible by a grant given for something else.** Hard refusals
come first for exactly that reason: a user who approved "always allow writes to `docs/`"
has not consented to shell execution, and no ordering that consults grants before refusals
can promise that.

Rules precede grants because rules are the deployment's stated policy and grants are one
person's convenience during one session.

---

## 3. Grants are session-scoped and never persisted

A sticky decision (`allow_always` / `deny_always`) is remembered under a **grant key**:

```
grant_key = tool                       when the request has no path
          = "{tool}:{dirname(path)}"   otherwise
```

The **directory**, not the file. Approving one write into a directory covers the rest of
that directory — which is what makes a session usable — while still not covering the whole
workspace.

Nothing is written to disk, ever. A permission that survives the process turns one
keystroke into standing authorisation, and the user who granted it will not remember
doing so.

---

## 4. No terminal means deny

When standard input is not a terminal, the default approver is `DenyApprover` and every
request that reaches a prompt is refused.

Piped input and CI must not treat silence as consent. `--yes` is the explicit opt-out and
must be typed by someone who meant it.

---

## 5. Secrets are refused before the prompt, and the refusal is not overridable

`PROTECTED_PATTERNS` matches case-insensitively against both the file name and its path
relative to the root: environment files, PEM and SSH keys, `.npmrc`, cloud credentials,
and Solana keypairs.

Protected paths are refused for **reads as well as writes**, and the refusal happens in
`Workspace` — before any approval prompt is shown.

> A user cannot meaningfully consent to a disclosure they have not been seen.

They are also **omitted** from directory listings, walks and searches: absent, not merely
unreadable. A listing that shows `.env` and refuses to read it has already told the model
the file exists and what it is called.

---

## 6. Paths resolve fully, then compare

A path is resolved — symlinks followed, `..` collapsed — and *then* compared against the
root.

A string scan for `..` passes a symlink pointing at `/`. This is the single most common
way a confinement check is wrong, and it is wrong in the direction that matters.

---

## 7. Execution runs without a shell

Execution is off until explicitly enabled, and when enabled it produces an **argv**, not a
command line. No pipes, no `;`, no second parsing layer between what the approval prompt
displayed and what runs.

The prompt shows a preview. If the string shown and the thing executed can differ, the
prompt is decorative.

---

## 8. A refusal says why, accurately

Three outcomes are distinct and must not be collapsed:

| Outcome | Means |
|---|---|
| refused by policy | a rule or a hard refusal fired; **no prompt was shown** |
| declined by the user | a prompt was shown and the answer was no |
| refused by workspace | the path was outside the root, or protected |

Reporting "the user declined" when no prompt was shown makes the assistant describe a
decision nobody made — and sends the user looking for a prompt they never saw.

---

## Conformance

`vectors/trust-model.json` holds cases as `(policy, request) → expected`, where `expected`
is `allow`, `deny`, or `ask`. Every implementation runs them.

The vectors deliberately over-represent the cases where a wrong implementation still looks
plausible: a grant that must not survive a hard refusal, a `deny_always` that must outrank
a permissive configuration, a rule that matches on tool but not path, and a protected path
that must be refused for a *read*.

A vector file is a claim about behaviour, so a case may only be changed when the behaviour
is meant to change — not to make an implementation pass.
