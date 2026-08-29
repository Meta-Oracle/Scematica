# scema-trust

Whether an action may happen. The gate Scematica Omni needs before it can act.

```rust
use scema_trust::{Approver, DenyApprover, Request, Risk, TrustPolicy};

let mut policy = TrustPolicy::new();   // reads allowed, writes prompt, execution refused
let mut approver = DenyApprover;       // the default when stdin is not a terminal

let request = Request::new("write_file", Risk::Write).at("docs/plan.md");
match approver.decide(&mut policy, &request) {
    scema_trust::Outcome::Allowed => { /* … */ }
    scema_trust::Outcome::Refused(why) => { /* Policy or Declined — not the same thing */ }
}
```

No dependencies. This is the gate in front of every action the runtime will ever take, and
it is meant to be readable end to end by somebody deciding whether to trust it.

## Two gates, kept apart

`scema_tools::Workspace` answers **where** an action may reach. This crate answers
**whether** it may happen at all. Merging them is how a grant for one silently becomes a
grant for the other: approving "write to `docs/`" says nothing about whether `~/.ssh` is
inside the workspace, and a workspace containing a path says nothing about whether writing
is allowed today.

## The preflight order is the point

```
1. hard refusals        read_only ∧ mutating          → Deny
                        execute ∧ ¬allow_execute      → Deny
2. explicit rules       first matching rule wins
3. session grants       a previous "always" for this grant key
4. standing config      non-mutating                  → Allow
                        write ∧ allow_writes          → Allow
                        otherwise                     → ask
```

**A refusal must never be reversible by a grant given for something else.** Hard refusals
come first for exactly that reason. Rules precede grants because a rule is the deployment's
stated policy and a grant is one person's convenience during one session.

`preflight` is a pure function from a policy and a request to a decision or "ask", which is
what makes it checkable against a file.

## Four rules worth not breaking

- **Risk is declared per tool, never inferred.** A scheme that guesses from a name will
  eventually guess low on the one tool where it matters, silently.
- **Grants are session-scoped and never persisted**, and keyed by *directory*, not file.
  A permission that survives the process turns one keystroke into standing authorisation.
  Prompting per file is how people learn to approve without reading.
- **No terminal means deny.** `DenyApprover` is the default in a non-interactive process.
  Piped input and CI must not treat silence as consent.
- **A refusal says which kind it is.** `Refusal::Policy` means no prompt was shown;
  `Refusal::Declined` means one was and the answer was no. Reporting "the user declined"
  when nothing was shown describes a decision nobody made.

## Conformance

Python is the reference implementation (`alchem_link.approvals`). The specification is
`alchem-link/docs/TRUST-MODEL.md`; the cases are `alchem-link/vectors/trust-model.json`, and
both implementations run them. Whichever side fails is the wrong one.

```
cargo test -p scema-trust                     # 11 unit tests + 20 shared vectors
cargo test -p scema-tools --test protected_vectors   # the secrets list, same file
```

The vector tests skip when the sibling tree is absent — a published crate does not carry it
— and say so, because a conformance suite that quietly runs zero cases is worse than one
that fails.

## What this does not do

It does not act, touch the filesystem, or prompt on its own. It does not decide *where*.
And it is not yet wired to `scema execute`, which still exits 2: the policy exists now, the
action path is the next step.
