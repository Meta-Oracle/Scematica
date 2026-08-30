# scema-effect

What the agent actually did, as opposed to what it decided.

```console
$ scema execute effect.json                  # dry run: both gates, touches nothing
$ scema execute effect.json --commit --intent 8f92a1c4
```

A `DecisionRecord` proves what was **chosen**. The moment the runtime acts there is a second
thing to prove — what was **done**, and whether it matched. Two records, two commitments:
one record covering both would let a failed action silently rewrite the history of the
decision that ordered it.

## The arm that matters

```rust
pub enum Outcome {
    Succeeded { detail: String },
    Failed { reason: String },
    Unknown { why: String },          // attempted; result could not be observed
    Refused { by: RefusedBy, reason: String },
    Simulated,                        // dry run
}
```

`Unknown` is why this crate is shaped the way it is. An effect whose result could not be
observed is not a success and not a failure — the process was killed between the write and
the confirmation, the command was terminated by a signal, the file was replaced in between.
The tempting collapse is to trust the return value, and a record claiming success for an
unverified write is worse than no record: **a false statement carrying a valid commitment.**

So every arm here writes and then *checks*, and doing and observing are separate steps.

`Refused` names which gate said no, because "policy refused" and "the operator declined" are
different claims and only one is about a person. A refusal for want of a terminal says so
rather than claiming somebody declined — the first end-to-end run of `scema execute` got
that wrong, which is why it is now pinned by a test.

## Two gates, in order

1. **Where** — `scema_tools::Workspace`, which also refuses protected names.
2. **Whether** — `scema_trust::TrustPolicy`, then an `Approver` if it has no answer.
3. **Do it**, then observe.

Nothing here decides whether an effect is permitted. Keeping the recorder ignorant of the
policy is deliberate: a recorder that could also authorise would eventually be asked to.

### Confining a path that does not exist yet

`resolve` canonicalises, which fails on anything not yet created — so a naive check refuses
every create. This confines the **deepest ancestor that does exist** and rebuilds the rest
onto it. A path containing `..` that does not exist is **refused rather than guessed at**:
`a/../../b` is only resolvable once `a` exists, and this is the case a string-scan
confinement check gets wrong in the dangerous direction.

## Dry run is the default

The two paths compute the same thing up to the last step, which is exactly why they are not
the same keystroke — the same reasoning that separates `simulate` from `decide`, and `enter`
from `D` in the console.

A dry run still runs both gates, so it answers *would this be allowed, and what exactly
would it do*. It will not **prompt**: asking somebody to approve an act that is not going to
happen teaches them the prompt is a formality. And it seals nothing — a record of an act
that did not happen is one somebody will later read as one that did.

## Exit codes

`0` success, refusal, or dry run · `1` failed · `3` **unknown**.

A refusal is not a crash: a script that treats "the policy said no" as failure gets
rewritten to ignore the exit code, and then it ignores real failures too. An *unobserved*
result does exit non-zero, because continuing a sequence past one is the thing that must not
happen quietly.
