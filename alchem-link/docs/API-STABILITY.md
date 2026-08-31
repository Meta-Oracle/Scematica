# API stability

`alchem-link` is 1.0.0. This document says what that buys you and, more usefully, what it
does not — because a version number on its own is a promise nobody can check.

## What 1.0 means here

**Everything named in `alchem_link.__all__` is public and follows semantic versioning.**
That list is the surface. A name in it will not be removed or change its signature in a
1.x release; new names may be added.

Anything not in that list is internal, including every leading-underscore function and
every submodule attribute you reach past the package root. `alchem_link.omni._check` is a
real function with a real job and it is not part of this promise.

## The three things that are *not* covered, and why

These are the parts people most want pinned, and pinning them would mean lying.

### 1. The feed registry is data, not API

`FEEDS`, `NETWORKS` and the 66 verified feeds in them describe the world outside this
package. Chainlink deprecates aggregators, deploys new ones, and changes heartbeats;
networks come and go. Those tables **will** change in patch releases, and that is the
package working correctly rather than breaking compatibility.

What *is* stable is the shape: `Feed` and `Network` keep their fields, `list_feeds` keeps
returning `List[Feed]`, and `verify_registry` keeps being the thing that asks the chain
rather than the table. If you have pinned a specific address, pin it in your code — and
run `alchem-link verify` in CI, which is what catches the table and the chain disagreeing.

Heartbeats specifically: `heartbeat_measured=False` marks a conservative *bound*, not a
measurement. A later release turning that bound into a measured value will change the
number, and a consumer that hardcoded the old one will call a feed fresh that its
publisher considers late.

### 2. Statistics may gain `None` where they used to return a number

This already happened once, in 1.0.0: `Stats.max_drawdown_pct` and `Stats.largest_move_bps`
were `float` defaulting to `0.0` and are now `Optional[float]` defaulting to `None`. A
window with fewer than two prints cannot contain a decline, and `0.0` there was a claim
that the price held through a span nobody observed.

That direction of change — a number becoming `None` when it turns out nothing measured it —
is **not** treated as a breaking change under this policy, and you should write consuming
code that expects it. The reverse (a `None` becoming a number) would be, because it would
mean the package started inventing something.

This is the one place where the project's central rule outranks the version contract. Every
other guarantee here exists to make the package predictable; this one exists to stop it
being confidently wrong, and the second is worth more.

### 3. The `WorldState` shape follows `scema.world/1`, not this version

`alchem_link.omni` emits a Scematica Omni world. That contract is versioned separately and
declared in the payload itself (`schema: "scema.world/1"`). When omni's contract moves,
this producer moves with it and declares the new version — which may be a minor release
here even though it changes bytes on the wire, because the consumer is the thing that
validates it and the consumer is told which version it is reading.

`world()` and `windowed_world()` are pure and take `now` for exactly this reason: their
output is reproducible, so a fixture in `scematica-omni/crates/scema-tools/fixtures/` can
pin what this package actually emits against what omni actually accepts.

## Command line

The verbs, their flags, and their **exit codes** are covered. Exit codes especially: a
script branching on `EXIT_UNUSABLE` is depending on a decision this package makes about
whether a read can be trusted, and changing that silently would turn a working guard into
a no-op.

Human-readable output is **not** covered. Text layout, colour, table widths and wording
will change. If you are parsing terminal output, use `--format json` — that is what it is
for, and `--format` is covered.

## Python versions

3.9 and up. Stdlib only, no optional extras: no `requests`, no `web3`, and a bundled
Keccak-256 because `hashlib` ships SHA3-256 with different padding and function selectors
have to be computed rather than trusted. A future release adding a mandatory third-party
dependency would be 2.0.

## Deprecation

A public name being removed gets one minor release emitting `DeprecationWarning` first,
naming its replacement. Something that has no replacement is not deprecated — it is
removed in a major release, and the changelog says why.
