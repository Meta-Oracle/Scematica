# Changelog

Version history for Scematica. For install, running, and architecture, see the [README](README.md).

## Unreleased

Bot workspace stays at **1.27.0** — nothing here is a release yet. Scematica Omni moves to
**0.6.0**, and that move is the point of the entry.

### `measure` — the bot audits its own decision log

First step of the Measure phase, and the reason it goes before anything else: the recurring
failure recorded in this repository's own history is tuning a threshold before checking that
the quantity it compares against is capable of varying.

`scematica measure` reads `scematica-pool-decisions.jsonl` and `scematica-trades.jsonl`,
writes nothing, and is safe against a live bot. It reports the rejection funnel, a
dead-signal audit, realised PnL, and coverage.

**`--split` is the point.** An aggregate over the whole log is a claim about *history* and
reads as a claim about the *bot*. Building this tool immediately demonstrated why: over all
9,214 records the momentum gate is the single largest cause of rejection ever recorded,
1,747 pools turned away on `inflow_rate=0.000`. Split on 2026-08-05 — the day that veto was
removed — and it is 28.3% of one window and 0.4% of the next, with buy-ready rising 1.1% →
11.3%. Both numbers are correct; only the second is about the bot as it stands. That fix had
never been measured.

**What the audit found that is still live.** Four fields are written into every decision and
have never once carried a non-zero value across all 9,214 records, in either window:
`velocity_sol_per_sec`, `pumpfun_score`, `dex_boost_usd`, `social_count` —
plus `pool_age_secs`, non-zero twice. The report deliberately says **never varied** rather
than "always zero": measured-and-zero and never-populated are different facts and the log
cannot tell them apart. Reading the producers settles it, and two consequences are already
visible — `sniper.rs` composes the DQ\* state with `price_velocity` derived from
`velocity_sol_per_sec`, so at least one of the net's 24 input features is a constant; and
`main.rs` gates on `pool_score >= 90 || pumpfun_score >= 90 || inflow_rate >= 1.5 ||
velocity >= 2.618`, an OR whose second and fourth terms can never be true.

**Coverage now accrues.** The share of RPC-bound checks that resolved was never written per
decision — the coherence breaker counts it process-globally — so for every existing record it
is unmeasured, and the report prints `—`. Never `0`, and PnL is never attributed to a
coverage band nobody recorded. The sniper now appends `scematica-coherence.jsonl` every 30s
so the question becomes answerable going forward. Timer-sampled rather than per-pool, because
the breaker keeps a rolling window rather than a monotonic counter and threading a
per-evaluation context through the buy path is not a change worth making for a diagnostic;
`measure` says so, and reports resolution *around* a decision rather than *for* it. Samples
the breaker declined to judge are counted and never averaged.

### Learn, step one: the tournament can change its mind

The DQ\* tournament runs three variants and promoted the one with the highest `total_reward`.
That number is a **lifetime sum which is never reset**, so a variant that was better in its
first thousand steps kept the primary slot forever, however badly it was doing now. A
comparison that cannot change its mind is not a tournament, and this is the criterion the
Learn phase was going to have to replace before any of the dark machinery — QR-DQN, the
Dreamer world model, the adversarial gym — could be evaluated at all. There is no point
switching a variant on if the thing that judges it cannot notice.

Promotion is now on **recent mean reward** over a 200-transition window, and three separate
things can stop it, each meaning something different: the incumbent has no recent mean, so
there is nothing to compare against; no challenger has one, because a variant below the
40-transition floor has not performed badly but has not performed; or the best challenger is
ahead by less than the margin, which is noise. Three variants on one stream produce means
that cross constantly, and a primary that changes every evaluation is not a selection — it is
noise with a promotion log.

An unmeasured variant reports `None`, never `0.0`, and is **absent** from the comparison
rather than entering it at zero — which would rank it below every losing variant on the
strength of having done nothing. The same rule as `Term`, `Coverage` and every other
aggregate here.

The margin is relative to the incumbent's `abs()`. Computed on the signed value it inverts
exactly when the agent is losing money — a challenger at −1.0 against an incumbent at −2.0 is
a real improvement, and the naive test rejects it. Pinned by a test.

The window is deliberately not serialised into the checkpoint. It is a claim about *recent*
behaviour, and restoring one written days ago would let a resumed agent be promoted on
performance it is not currently delivering, which is the defect this replaced wearing a
different hat.

One of the seven new tests failed first time and the code was right: `RECENT_WINDOW` is 200
and the test fed only 80 losing transitions, so the window still held the good history and
the recent mean was legitimately higher. The window working, not the rule failing.

467 bot tests, clippy clean.

### PNG export, rasterised by hand

`scema nft --png` writes the growth as a PNG. The rasteriser, the font and the PNG encoder
are all written here, which needs a reason, and the reason is the one the crate already
rests on: **the same world produces the same bytes.** `resvg` and a browser canvas antialias
differently, so a library PNG would depend on which runtime made it — two artefacts with one
name. The same call this crate already made for base64, the glob matcher and the sine table.

Antialiasing is a 3× supersample followed by an integer box downsample with a stated
rounding rule, so every pixel is a sum of integers over a constant — a value two runtimes
cannot disagree about. No analytic coverage, no floats.

The zlib stream uses **stored** deflate blocks. A real compressor's output depends on its
heuristics, and a heuristic is exactly what two implementations differ about; stored blocks
make the byte stream a pure function of the pixels. Files are larger — 787 KB at 512px — and
that is the right trade for an artefact whose entire value is reproducibility. Validated
against a real zlib decoder: signature, per-chunk CRCs, IHDR, and an inflated length that
matches exactly.

The legend is drawn with a 5×7 bitmap font written out by hand, including glyphs for `·`,
`°`, `—` and `∅` — the empty set especially, because that is how an unmeasured coverage is
written everywhere else here and a fallback box would turn a specific statement into a
missing-character marker. An unknown character draws a visible box rather than nothing: a
silently dropped glyph makes a label read as something it is not.

Both renderings walk one primitive list. `text_pair` produces the SVG element and the raster
primitive together so a legend line cannot reach one and not the other, and a test asserts
the counts match. The SVG output is byte-identical after the refactor, which the parity
fixture confirmed rather than my believing it.

CLI only for now. The browser still exports the SVG, so there is no two-runtime divergence to
have — the rasteriser would need porting first, and claiming parity before that would be the
kind of unearned confidence the rest of this stack exists to refuse.

402 omni tests, 37 parity checks, clippy clean.

### Arrive, step one: the pipeline can time itself

The repository's own notes call the central product problem structural — **the bot arrives
post-pump** — and no amount of filter tuning touches it. Attacking it from the latency side
needs a number that did not exist.

The execution path was already instrumented: `TxTelemetryEvent` carries `elapsed_ms`,
`blockhash_fetch_ms_total`, `send_confirm_ms_total`. Nothing measured the part *before* that,
where the filter round trips live. `decide_latency_ms` now records the span from the listener
seeing a pool to the decision being written, and `measure` reports it as a nearest-rank
distribution — every figure printed is a span that was actually measured, never one
interpolated between two that were.

`Option` and `skip_serializing_if`, for the reason every optional field here is: a build that
did not record it and a pipeline that decided in zero milliseconds are different facts. Zero
would read as *arrives instantly*, which is the most flattering possible reading of exactly
the thing under investigation, so it is the one value that must not appear by default. All
9,214 historical records honestly report `—`.

Marked through a bounded side table in `latency.rs` rather than threaded through
`write_pool_decision`'s twenty-six call sites. That is a deliberate trade: a large diff in a
live buy path is how a trading bug gets introduced by a diagnostic. The table is bounded and
prunes stale entries, because it is fed by an unbounded event stream and is not drained on
every path — a leak in a report is a leak in the process that holds the money. Reading a span
consumes it, so a pool decided twice cannot report the first decision's span both times.

This is the baseline, not the fix. Block-level listening, leader-schedule-aware submission and
bundle routing are the Arrive phase proper, and they need a live connection to evaluate —
which is precisely why the measurement lands first.

460 bot tests, clippy clean.

### The NFT is a fractal now

`scema nft` grows the world instead of gauging it. The instrument plate is still there behind
`--plate` and still tested — it is the same data read as measurements — but the default is a
form whose *shape* is the reading.

Nothing about it is decoration laid over numbers. Depth comes from `Extent`, branching from
the signal count, spread from the balance of risk against opportunity, decay from legibility,
and the exact form is seeded by the world's own commitment, so two worlds are visibly
distinct and the same world never is.

**Ignorance is a severed limb.** A blind spot does not fade a branch or tint it — it cuts it
off, leaving a stub and a void in the canopy. A world nobody could read grows visibly
mutilated, which is an accurate report and is meant to be uncomfortable.

Three attempts were needed to make that honest, and each failure is worth recording because
each was the form asserting something the data did not.

The first made severing a per-node *probability* of `blind_spots / observed`, which compounds
down the recursion: **three reported blind spots cut twenty-six limbs.** That is the same
class of error as rendering an unmeasured term as `0.00`. It is a count now, chosen up front.

The second cut at whatever depth was convenient, so a cut could sit inside another cut's
subtree and never be reached — three blind spots rendered as two limbs cut. Every cut now
lands on **one level**, where nesting is impossible and the count is exact.

The third chose the shallowest level that could *hold* the cuts, and three cuts fit exactly on
level one of an arity-3 tree — so all three level-one limbs were cut and the entire canopy
disappeared, rendering "three of six objects were unreadable" as "nothing was observed at
all". The level now needs three nodes per cut, and when even the deepest level cannot hold
them the footer says `(CAPPED)` rather than quietly under-reporting.

Determinism survived, which fractals make harder rather than easier: a recursion amplifies
any disagreement, so a one-ULP difference at the root is a visibly different tree by the
fourth level. There is no float arithmetic in the growth — integer milliunits, whole-degree
angles through the shared sine table, and a **32-bit** xorshift because `Math.imul` and
`>>> 0` reproduce u32 wrapping exactly where u64 would need `BigInt`. The recursion order is
fixed and the RNG is consumed in that order.

`web/lib/omni/fractal.ts` is the port and it matched byte for byte on the first run.
`check:omni` pins it, along with the cut count across seeds and that cutting never
annihilates the form. `/omni` renders the growth now, with a legend, because a form this
suggestive needs its terms stated or a viewer reads whatever they already believed into it.

391 omni tests, 37 parity checks, clippy clean.

### Scematica Omni 1.0 — the freeze

On a verification runtime, 1.0 is not a maturity badge. It is one sentence: **a record sealed
today still verifies tomorrow.** `docs/COMPATIBILITY.md` makes that specific enough to be
checkable, and `corpus/` is what checks it.

**Frozen:** the canonical encoding, the 1e-9 float quantisation, SHA-256 as the commitment
hash, which fields a commitment covers and which it deliberately does not (`id`, `at`,
`runtime` describe the recording, not the thing recorded), the wire shape of `scema.world/1`,
`WorldState::schema` being optional, and the Merkle construction.

**Open, and extensible without breaking anything:** `Domain` and `EntityKind` stay open enums,
new producers, new `Effect` arms, new anchor chains, λ weights. None can change an existing
record's digest, because an existing record does not contain them — so each is a minor.

**Not covered, said plainly rather than left to inference:** Rust API stability below the CLI,
the `.scema/` directory layout, `scema-nft` plate bytes across versions, and anything an
anchor asserts.

The corpus is the mechanism. Four real records sealed by builds that no longer exist —
including two from **before `WorldState::schema` existed** — re-verified by
`cargo test -p scema-effect --test corpus` on every commit, through the `omni` CI job.

That tripwire was tested rather than assumed. Removing `skip_serializing_if` from `schema`
makes both pre-schema records serialise `"schema": null`, which changes their canonical
encoding and moves their digests; the corpus reported both as tampered while the two current
records passed — exactly the shape of the disaster it exists to prevent. Reverted, and green
again.

The corpus is **never regenerated**. A re-sealed record agrees with today's build by
construction and detects nothing; when one fails, the change under test is almost certainly
what is wrong.

Two limits are unchanged by 1.0 because they are properties of the design rather than the
version: `verify` does not prove the world was as described (provenance carries that), and it
does not prove the record is the original — tamper-evident, not tamper-proof, until the root
is anchored somewhere the author does not control. `scema anchor` is the half that batches and
proves; publishing needs a chain and a key.

The browser extension and the Claude Code plugin move to 1.0.0 with the runtime they talk to.

374 omni tests, clippy clean.

### Omni 0.9 — a root somebody else can hold

`scema verify` has always ended its own limits with the same clause: tamper-evident, not
tamper-proof, *until the root is anchored somewhere the author does not control*. `scema-anchor`
is the half that batches and proves. Record roots — decisions **and** effects — go into one
Merkle tree, and any single record gets an inclusion proof a third party can check holding
neither the batch nor the other records.

**SHA-256, and that is not a compromise.** `mesh-attest` uses keccak and matching it would let
the two share a verifier, but Omni's commitments are SHA-256: changing the hash would mean every
record already sealed on disk stops verifying, and a verifier that rejects untouched history is
the one failure that teaches a reader to stop believing it. It costs nothing, because EVM
exposes SHA-256 as precompile `0x02` — a contract can check these proofs directly. The algorithm
is recorded in the batch and checked on verification rather than assumed.

Two details that are cheap to get wrong and were not. **Leaves and internal nodes are
domain-separated** (`H(0x00‖bytes)` versus `H(0x01‖left‖right)`); without the tags an internal
node is a valid leaf preimage and membership can be proven for something never submitted. And
**an odd node is promoted, never duplicated** — padding by duplication is the widespread
implementation and lets two leaf sets share a root, the CVE-2012-2459 shape, which here would
mean a batch presented as covering a record it never covered. Both are pinned by tests that fail
if the property is lost.

**Anchors are a list, and empty means unanchored — said in those words.** The plan is one chain
whose economics we control and one with an audience, each independently checkable. Nothing here
reaches a chain: `--record` writes down that a root was published and states plainly that it did
not check, because reaching a chain is a network act with a key behind it and recording an anchor
that was never submitted would be the fabrication the rest of this runtime exists to refuse. A
reader follows the reference themselves — an anchor taken on the author's word is not an anchor.

Editing a batch is caught: `--list` reports `ROOT DOES NOT MATCH ITS LEAVES`, which is the edit
that would let a record be claimed as covered by an anchor that never included it.

372 omni tests, clippy clean.

### Omni 0.8 — the loop can act

`execute` no longer exits 2. `scema-effect` records what was *done*, `scema-trust` says
whether it may be, `scema_tools::Workspace` says where — three separate things, and the
recorder is deliberately ignorant of the policy, because a recorder that could also
authorise would eventually be asked to.

**`Outcome::Unknown` is why the crate is shaped this way.** An effect attempted whose result
could not be observed is not a success and not a failure: killed between the write and the
confirmation, terminated by a signal, replaced by something else in between. Every arm
writes and then *checks*, so doing and observing are separate steps — the tempting collapse
is to trust the return value, and a record claiming success for an unverified write is worse
than no record, because it is a false statement carrying a valid commitment. It exits 3, so
a sequence cannot continue past one quietly, while a refusal exits 0: a script that treats
"the policy said no" as a crash gets rewritten to ignore the exit code, and then it ignores
real failures too.

**Dry run is the default.** The two paths compute the same thing up to the last step, which
is exactly why they are not the same keystroke — the rule that already separates `simulate`
from `decide` and `enter` from `D`. A dry run still runs both gates, so it answers *would
this be allowed*; it never prompts, because asking somebody to approve an act that is not
going to happen teaches them the prompt is a formality; and it seals nothing, because a
record of an act that did not happen is one somebody will later read as one that did.

**The effect is declared, never inferred.** Omni's branches describe work — "11 markers in
`scema-tools`" — and deriving an executable action from one would be the keyword-overlap bug
with a disk behind it. `--intent` names the decision an effect claims to carry out, asserted
by the operator exactly as `--ground` is.

Confinement had to grow up for this. `resolve` canonicalises, which fails on anything not
yet created, so a naive check refuses every create; it now confines the **deepest ancestor
that exists** and rebuilds the rest onto it. A non-existent path containing `..` is
**refused rather than guessed at** — `a/../../b` is only resolvable once `a` exists, and
this is the case a string-scan check gets wrong in the dangerous direction.

The first end-to-end run broke the specification written two commits earlier. `DenyApprover`
refuses *without asking anyone*, and the outcome read `refused by Operator: declined at the
prompt` — describing a decision nobody made, and sending an operator to look for a prompt
they never saw. `Approver::why_refused` now carries the accurate reason, and a test pins it.

357 omni tests, clippy clean.

### Omni 0.7 groundwork — the trust model, in Rust, checked against Python

The other half of the same arc. `scema-trust` is a port of `alchem_link.approvals` against
the specification written for it, and `cargo test -p scema-trust` runs the same twenty
vectors Python runs. All twenty agree.

No dependencies, deliberately: this is the gate in front of every action the runtime will
ever take, and it should be readable end to end by somebody deciding whether to trust it.
The glob matcher is thirty lines rather than a crate, and it is iterative with backtracking
because the naive recursive form overflows the stack on `*a*a*a*a*b` — and these patterns
match paths a model chose.

`preflight` is a pure function from a policy and a request to a decision or "ask", which is
what makes it checkable against a file at all. `Risk` is closed where `Domain` is open, and
for the opposite reason: an unrecognised domain should degrade to a warning so producers can
describe new worlds, but an unrecognised *risk* has no safe default — `Read` understates and
`Execute` makes the vocabulary unusable — so the caller decides. Grants live behind a
private field so one can only arrive through `grant` or `remember`, past the settling rule.
And `Refusal` distinguishes `Policy` from `Declined`, because saying "the user declined"
when no prompt was shown describes a decision nobody made.

**`scema_tools::Workspace` now refuses `PROTECTED_PATTERNS` by name**, closing a gap CLAUDE.md
had recorded as open: `RepoObserver` reads file contents to count tests and markers, and
nothing stopped it reading a keypair. It emits only counts, but a count derived from a
private key is still a read of one, and the daemon and MCP server take their paths from a
browser extension and a language model. The check runs *after* canonicalisation and *before*
the root test, so a symlink cannot launder a protected target and the refusal reads as "this
is a secret" rather than "widen your roots". `cargo test -p scema-tools --test
protected_vectors` holds that list against Python's.

Both vector tests **skip** when the sibling tree is absent — a published crate does not carry
it — and announce the skip, because a conformance suite that quietly runs zero cases is worse
than one that fails.

One bug, in the harness rather than the library, worth recording because it is the shape of
mistake these files exist to catch. Two vectors failed on first run; the library was right
and the reader was wrong. A grant key is `tool:dirname` and therefore contains a colon, so
splitting on the *first* one produced the key `"write_file` and silently installed no grant —
the case then fell through to the standing configuration, and the vector accused the library.

329 omni tests, clippy clean.

### alchem-link: the trust model becomes a specification

The first step of the alchem-link 1.0 line, and it goes first because Scematica Omni cannot
grow an action path until it exists. `approvals.py` and `workspace.py` are the only working
approval model in this repository, and omni's own docs name that model as the precondition
for `execute`. Writing it a second time from memory is how two implementations end up
disagreeing about which refusals are overridable.

`docs/TRUST-MODEL.md` states it in a language-neutral form: two gates asked in order and
kept separate (`Workspace` answers *where*, `TrustPolicy` answers *whether*), risk declared
per tool so a new tool cannot arrive unclassified, the fixed preflight order — hard refusals,
explicit rules, session grants, standing configuration — and why that order is the point.
Grants are session-scoped and never persisted, keyed by directory rather than file. No
terminal means deny. Secrets are refused before the prompt, for reads as well as writes, and
omitted from listings entirely, because a user cannot consent to a disclosure they have not
been shown.

`vectors/trust-model.json` is what actually binds two implementations: twenty cases as
(policy, request) -> allow / deny / **ask**, chosen for where a wrong implementation still
looks plausible — a grant that must not survive a hard refusal, a `deny_always` that must
outrank a permissive configuration, a rule matching on tool but not path, a rule that tries
to enable execution the operator turned off. All twenty passed against Python on the first
run. `ask` is asserted to be exercised, because a vector file where nothing asks would pass
against an implementation that never prompts.

Writing the vectors found one real gap and one bad test. `PROTECTED_PATTERNS` covered
`credentials` and `.aws/` but not a flat `aws-credentials` exported beside somebody's source;
it now covers `*-credentials` and `*_credentials`, anchored as suffixes so that a document
*about* credentials stays readable — the point is to refuse the secret, not its
documentation. And the new test asserting `Workspace` agrees with the vectors originally
wrapped `resolve` in `assertRaises(Exception)`, which passed for every path because none of
them existed in the temporary root: a missing file and a refused secret are different
outcomes, and a test that cannot tell them apart would keep passing after the protection was
removed. It asks `is_protected` directly now.

627 tests.

### One age rule, not three

The first thing `measure` found, acted on. `CachedPool::open_time` is usually absent —
pump.fun migrations and whale-copy pools set it to `0` outright and Raydium often leaves it
unset on new pools, which are the only pools this bot trades. Both scorers already knew
that and fall back to the detection timestamp, treating an unusable value as unknown rather
than as zero; `pool_scorer`'s own comment says returning `0.0` velocity there *"would read
as measured, and stalled, and penalise the pool."*

`sniper.rs` computed its own age inline with a bare `else { 0 }` and no fallback, and that
value — not the scorers' — is what reached the decision log, the Deep Q\* state and two
gates. It is now one implementation in `pool_age`, with `None` for an age nobody can
establish and for a timestamp far enough in the future to be nonsense. `pool_scorer` keeps
its own skew handling on top, deliberately: it is ranking, and a pool whose clock cannot be
believed should rank like a stale one rather than vanish from the comparison.

Two limits are recorded rather than papered over. There is **no source of true age** for an
`open_time = 0` pool — nothing stores when the pool was first seen, so the fallback yields
"observed zero seconds ago", which is honest and not an age. And the decision log still
carries `f64`, so an unknown age and a pool detected this second both write `0`. The gates
are unaffected — every one is guarded on `pool_age_secs > 0`, so an unknown velocity cannot
satisfy a threshold — but the log cannot tell the two apart, which is exactly what made this
invisible for months. Making the log carry `Option` touches every writer and is the next
change, kept separate from a behavioural one.

The Deep Q\* net has no way to express an unmeasured input: `price_velocity` takes `0.0`
when velocity is unknown, which the net reads as an observation of a stalled pool. Fixing it
means a mask feature and a `STATE_DIM` change that invalidates every checkpoint, so it
belongs to the Learn phase where that cost is acceptable.

### Omni 0.6.0 — because a published 0.5.0 could not carry the fix

`Domain` and `EntityKind` became open enums in 0.5.0, and two of the four producers depend
on it: the browser extension emits `domain: "web"` and `alchem-link` emits `domain: "data"`.
A component built before that refuses both outright —

```
unknown variant `web`, expected one of `software`, `infrastructure`, `trading`, `unknown`
```

— which is what an operator running an older `scema-omnid` beside a current extension
actually sees. The components are separate crates so `cargo install scema-cli` on a CI box
does not drag in a terminal stack, and the cost of that split is that nothing makes them
move together except the operator.

So three things changed. The family is at **0.6.0**, which is what lets the corrected CLI
and daemon be published at all — 0.5.0 is already on the registry and a published version
is a fact. `scema doctor` now **runs each component and compares its version to its own**,
reporting `FAIL` with `cargo install <crate> --force` rather than the `ok` it used to give
on existence alone; the command exists to find quietly-broken installations and this was
the one it could not see. And the README says to install the line, not a crate.

The browser extension and the Claude Code plugin move to 0.6.0 with it. They did not change,
but they version with the runtime they talk to, and the drift v1.27.0 closed was exactly
these numbers disagreeing.

### `scema-nft` — a world, drawn

New crate. A `WorldState` becomes a self-contained SVG plate plus ERC-721 metadata, via
`scema nft` or in the browser on `/omni`. The same world produces the same **bytes** in Rust
and in `web/lib/omni/nft.ts`, which is what makes the plate a derivative of the record
rather than an illustration of it — an image that depended on which runtime drew it would
mean two artefacts for one world.

That required no trigonometry (a shared integer sine table; `sin`/`cos` are not correctly
rounded by IEEE-754), no decimal formatting of floats, rounding spelled out as half away
from zero, code points rather than UTF-16 units, base64 over UTF-8 bytes rather than `btoa`,
and no clock. The same wall `canonical.rs` hit, and the same conclusion.

The plate is an instrument: the em-dash rule in vector form. A gauge nobody measured draws
its full sweep dashed; a gauge measured at zero draws nothing and prints `0.00`. Blind spots
cut visible notches, because ignorance should be a hole rather than blank space. There is no
rarity, tier or rank, and both test suites assert the absence.

### Two real bugs in the bot workspace

**`cargo clippy --workspace` did not compile.** `scema-ddqn`'s argument loop tripped
`clippy::never_loop`, which is deny-by-default, so a documented command failed outright. The
loop could never reach a second argument — every arm ends the process — and now says so.

**The API's log tail could hang, and served a fragment as a line.** `read_last_n_lines`
seeks to an arbitrary byte and then read lines with `filter_map(|l| l.ok())`. `Lines` may
yield `Err` forever, and `filter_map` skips errors and keeps asking, so a persistent read
error turned a log tail into a stuck request thread. The seek also lands mid-line — and can
land mid-UTF-8-character — so the first line was a fragment served as though it were a whole
entry. The fragment is now discarded at the byte level and the iterator is bounded.

### `npm run lint` in web/ opened an installer

ESLint was never declared or installed, so `next lint` prompted interactively — a surprise in
a terminal and a hang in CI. Replaced with `npm run typecheck` and `npm run check`, the
latter running the typecheck plus all five parity suites in one command.

## What's New in v1.27.0

### 1.26.0 shipped without a description, and that is the story of this release

1.26.0 was published to crates.io on 2026-08-21 and documented nowhere. No changelog
entry, no README line, and every version-bearing document in the tree — README,
QUICKSTART, ROADMAP, WHITEPAPER, EQUATIONS, the thesis, `web/package.json` — still said
1.25.0. That is the same drift v1.25.0 was cut to end, arriving one release later from the
opposite direction: last time the manifests ran ahead of the prose, this time a *published
artifact* did.

1.26.0 is not folded away. A published version is a fact, and the tree has moved past it
anyway — `scematica-mesh` gained the Omni world emitter after that publish. **v1.27.0 is
the release that describes both**: what 1.26.0 shipped, and everything since.

The rule that survives both incidents: the version is not the number in `Cargo.toml`, it
is that number **and every place that repeats it**.

### Scematica Mesh — the running system's own topology

`scematica-mesh` collects the File-Based IPC files into a graph of decision-making units
and serves it read-only — no writes, no locks, safe against a live bot. `mesh-dashboard`
renders it as a terminal graph; `/mesh` renders it on the web behind `GET /api/mesh`.

It also implements the agentic-architecture spec's Ψ = C·K·(1−R) over the *observed* mesh,
because no subsystem can measure its own agreement with the others. Every term carries
`measured: bool`, and **an unmeasured dimension contributes the neutral element, never
zero** — the literal reading of the spec pins the gate shut on subsystems nobody has
built, which is the same trap that once jammed the sentience Ψ at 0. Ω stays `None` until
one of its five subsystems exists.

`/mesh` has no simulation branch, for a sharper reason than the other pages: a simulated
metric is a fake number wearing a badge, but a simulated *topology* asserts that a
particular set of units exists and is wired a particular way on the operator's machine.
There is no honest way to badge that, so it 503s when no bot is paired — which is not the
same as an empty mesh, and the two render differently.

### The Escrow Market — non-custodial, and provably so

`programs/scemadex-vault` time-locks any SPL token against a reserve asset. Four
instructions and **no privileged role at all**: no admin, no sweeper, no pause. Two traps
that only appear with a Token-2022 mint, which is the product's central case: it uses
`token_interface` rather than legacy `anchor_spl::token`, and deposits credit the
**measured balance delta** rather than the requested amount, because a transfer fee
otherwise books reserve that never arrived. Each instruction carries one token program
*per leg*, so a Token-2022 mint can be backed by legacy-SPL wBTC — a single shared
`token_program` account made every such pair unconstructible.

The custody guarantee depends on a deploy step invisible in the source:
`set-upgrade-authority --final`. Until `solana program show` reports `Authority: none`, a
PDA vault is fully custodial no matter how `lib.rs` reads.

`/escrow` reads it. `balance >= recorded`, never `==` — anyone can transfer tokens into
any account, so a surplus is normal and permanently stuck, giving three verdicts
(`backed` / `donated` / `SHORTFALL`) rather than a boolean. No price, no USD, no "percent
backed": the program consults no oracle and neither does the page. Decimals come from the
mint account, never a token list, because a wrong `decimals` is a wrong *quantity of
money*, not a wrong label.

### Scematica Omni 0.5.0 — the world contract opened

Omni versions on its own track and reached 0.5.0. `WorldState` now carries
`schema: "scema.world/1"` and an undeclared version is refused on import — the contract is
JSON implemented in four languages with no compiler between a producer and it, so without
a version the next format change is a silent misread rather than an error. The field is
`Option` + `skip_serializing_if`, which is load-bearing: records sealed before it existed
must keep verifying, and a verifier that cries tamper on untouched history is the one
failure that teaches a reader to stop believing it.

`Domain` and `EntityKind` became **open** enums. Closing them was the largest limit on
universality — a perceived web page and a set of Chainlink feeds both reported `unknown`,
so two entirely different worlds were indistinguishable to every specialist.

Also: `scema quickstart`, and the fix for two correct behaviours that read as
malfunctions on a first run. An ungrounded `simulate` abstains, which is right at every
step and looked like the tool refusing; and a grounded signal branch outranking the
operator's goal looked like the goal being ignored. Both were rendering bugs, not logic
ones — `next_steps` now renders each abstention reason as a different next command, and
**suggests without ever acting**.

### Scylar moved to a reasoning model

The terminal's Groq provider is now `openai/gpt-oss-120b`. Sampling and reasoning options
moved from the route to the provider, because they are not portable — `reasoning_effort`
and `include_reasoning` exist on gpt-oss and nowhere else in the list, and an unrecognised
field is a 400 on some OpenAI-compatible servers rather than an ignored key.

Three things had to change with it. The completion cap is no longer 900: gpt-oss reasons
before it answers and those tokens bill against the same budget, so the old cap could be
spent entirely on thinking and stream back nothing — indistinguishable from an outage. The
history window dropped from 20 turns to 8 on that provider, because the free tier meters
~8k tokens/min and every tool round re-sends the whole conversation. And `pump` forwards
only `delta.content`, which is now load-bearing: the chain of thought arrives in a sibling
field, and forwarding it would put her working-out on screen, drive the mouth sprite with
it, and feed it to `recordAnswer` as something she claimed.

The system prompt was **not** cut, and that was measured rather than assumed. Squeezing it
to fit drops `metacognition` first — the layer that makes her say whether a claim was READ
or GUESSED — because `composePsyche` ranks situational data above doctrine on purpose.
~700 tokens is not worth the rule the assistant is built around.

### alchem-link 0.24.0

Emits the opened world contract: the declared `schema`, and a real `domain` of `data`
instead of collapsing to `unknown`. Two of its own tests were still asserting the closed
form and had been failing since the contract moved — one of them passing for the wrong
reason, catching a `ValueError` from the schema check while claiming to test an evidence
rule. Both fixed; 622 tests pass.

### Version surfaces reconciled

Bot workspace **1.27.0** (all `scematica-*` crates inherit it). `web/package.json`
**1.27.0**, which is also the Android `versionName`/`versionCode` source (→ 12700).
`alchem-link` **0.24.0**. Scematica Omni stays at **0.5.0** — only tests and docs changed
after its publish, and bumping a version that shipped nothing is the same sin as not
bumping one that did.

Two drifts fixed along the way: the browser extension's `package.json` said 0.1.0 while
its own `manifest.json` said 0.5.0, and the README's ScemaDEX rows were a full patch
behind what was actually on crates.io (`scemadex-sdk` 0.3.1, `scemadex-mcp` /
`scemadex-settle` 0.1.3, `scema-agent-playground` 0.1.1). The Omni family now appears in
the README's version table at all, which it did not before.

Documents whose header carries a version but whose contents were **not** re-verified now
say so — `verified against source 2026-08-11 (at v1.25.0)`. Bumping the number on an
unaudited document is how a stale claim acquires a fresh date.

`scema-botchain` and `scema-bot-mesh` remain separate workspaces at 0.1.0 on their own
track. The ScemaDEX family is unchanged since its last publish and stays independently
pre-1.0.

## What's New in v1.25.0

### One version again — 1.16 through 1.24 were never a release

The workspace number moved 1.15.0 → 1.24.0 in a single commit and then kept taking
features without moving again: the sentience crate, the coherence breaker,
counterfactual replay, calibration, the Scylar terminal, the BOT Chain port and the
neural mesh all landed *after* that bump. Nine intermediate minors were never cut, and
nothing describes them, so this release folds the whole span into **1.25.0** rather than
inventing a history for numbers that never shipped.

The drift was not only in the headline. `web/package.json` — the single source for the
web dashboard and the Android artifact's `versionName`/`versionCode` — was still on
1.15.0, and every internal crate dependency still pinned `version = "1.15.0"` against
crates that had moved to 1.24.0. Caret semver hid it (1.24.0 satisfies `^1.15.0`), so the
build never complained while the manifests said something false. Both are now on 1.25.0,
and `alchem_link.__version__` — which the CLI, the shell banner and the dashboard title
all read — is reconciled with the `0.23.24` that `pyproject.toml` and the built wheel
already carried.

The ScemaDEX SDK family (`scemadex-sdk` 0.3.0, `scemadex-mcp` / `scemadex-settle` 0.1.2)
is unchanged since its last publish and keeps versioning independently below 1.0.
`scema-botchain` and `scema-bot-mesh` are separate workspaces at 0.1.0 and version on
their own track.

### scematica-sentience — the cognitive architecture as a library

The Singularity Cognitive Architecture, previously prose, is now computable Rust: 29
modules from `perception` and `data_integrity` through `knowledge_graph`,
`meta_cognition`, `contradiction` and `truth_confidence`, converging on two equations —
`S_t = R×L×M×D` and `Ψ_t = S×I×K×MC×A_g×F` — plus the `Ω_{t+1}` recursive step. Five of
the seventeen axioms are enforced as runtime checks rather than documentation.

It is a **library only**, with no binary, so the `scematica` launcher gains no
subcommand. Nothing on the sniper's trading path depends on it for LLM gating yet; the
`overlay` module gates a model's output on integrated cognition (GO / CAUTION / HOLD) and
is consumed by the API and the coherence breaker, described below.

### The Ψ gate measures staleness, not mood

`GET /api/sentience` answers one question: can anything reading this API describe the bot
*right now*? It matters because every read endpoint serves its state file identically
whether that file was written four seconds or four hours ago, and `/api/health` only
reports that a process was once here — so a live-looking briefing can describe a session
that ended overnight. **HOLD returns 409 and the model is never called**, because a warned
model still writes a confident paragraph of stale numbers.

Two failure modes were hit building it, both of which would have made the badge worthless:

- `Perception`'s data ratio is a **product**, so a single unmeasured channel scored 0 pins
  Ψ at 0 and jams the gate shut permanently. Unmeasured dimensions are 1.0 — "not a
  limiting factor" — and only *measured* degradation moves the verdict. Otherwise a
  healthy bot sits in permanent CAUTION and operators learn to ignore the flag.
- The handler must overwrite only measured fields via `state_mut`. Calling `set_state`
  there also replaces the timestep and sentience index, which silently cancels every
  `/api/sentience/observe` on the very next gate read.

Ψ stays a pure function of measured data integrity by design: a run of coherent answers
must not be able to talk the gate into trusting stale numbers.

### Coherence breaker — the first breaker that fires before the damage

Every other breaker in `scematica-sniper` halts on money — `ath_tracker` on drawdown,
`grief_breaker` on a loss window, `kelly` on win rate — and therefore all of them fire
*after* the loss. `coherence.rs` fires on the condition that precedes it.

RPC-bound filters are capped at `RPC_CALL_TIMEOUT_SECS` and **fail open**: when a node is
slow, `check_mint_renounced`, `check_freezable` and `check_burned` return `pass()` because
they could not look, not because they looked and approved. That is right for one pool and
wrong as a state to keep trading in — past some fraction of unresolved checks the pipeline
is a pass-through wearing a filter's name, and the safety checks the operator believes are
running are silently not running. The breaker counts resolved-versus-failed-open over a
120s window, feeds the ratio to the same `scematica-sentience` master equation the API
gate uses, and stops buying on HOLD. One definition of Ψ across the system, not two.

Instrumented in the two shared RPC retry helpers in `filters.rs` rather than at each
fail-open site, so a newly added filter is counted by construction. Requires
`MIN_SAMPLES = 20` before it can return a verdict — a cold start has resolved 0 of 0
checks, and a breaker that trips on an empty sample fires hardest exactly when it knows
least. Deliberately **not** wired into the sell path: a degraded feed is a reason to stop
opening risk, never a reason to stop closing it. `coherence_breaker` in `config.toml`,
default **on** via `default_true()`, because `#[serde(default)]` yields `false` for a
missing bool and would have silently disabled a safety feature for every existing config.

### Counterfactual replay — exact where evidence exists, silent where it does not

`POST /api/replay` re-applies proposed thresholds to what the pipeline actually
*measured*. Every evaluated pool is already written to `scematica-pool-decisions.jsonl`
with the values that decided it, so a threshold change needs no RPC and no simulation.

The asymmetry is the design, not a shortcoming to paper over. Outcomes exist for pools
that were **taken** and do not exist for pools that were **rejected** — nobody bought
them, so nothing recorded what they would have done. **Tightening** therefore yields an
*exact* PnL delta against real realised SOL; **loosening** admits pools with no outcome,
and the endpoint reports how many and their measured distribution while refusing to put a
return on them. Inventing an expected value there is the most tempting move available and
would make every answer built on it worthless.

This is deliberately **not** built on `scematica_sniper::Backtester`, whose
`static_filter_check` returns `false` outright whenever `min_pool_size > 0` or any
RPC-bound filter is enabled and never looks at `pool_score` at all — under any realistic
config it answers "nothing would pass", which is a confident number that means nothing.

### Calibration — scoring the assistant against what the mints did

`GET /api/calibration` exploits an unusual property: ground truth arrives automatically,
minutes later. Scylar says a pool looks strong; `scematica-trades.jsonl` records what it
did. "Of the 40 pools I called strong, 12 rugged" is a fact about her rather than a tone.

Two limits are load-bearing. Claims are scoped to the **sentence** naming the mint, never
the whole message — a paragraph mentioning four mints does not hold four opinions, and
attributing the message's overall sentiment to each would manufacture claims she never
made and then score her on them. And only claims with an outcome are scored: bullish calls
resolve against realised PnL, bearish calls usually cannot because nobody buys what she
warns against. That gap is counted and reported, never closed with an estimate — scoring
an assistant on outcomes it caused *not* to happen is how a calibration number becomes
flattery.

### Scylar Terminal — the third product on the site

An avatar chat terminal at `/scylar-terminal`, with its own violet palette beside the
sniper's black-and-red and alchem-link's black-and-blue. It runs on whichever free LLM
tier has a key, Groq first for latency. The constraints that shaped it:

- **Provider keys are server-side, always**, and the chat route **strips client-supplied
  `system` turns** — without that, a public endpoint with a key behind it is someone
  else's free LLM proxy.
- **The model picks a tool name, never a URL.** `lib/scylar/tools.ts` hard-codes a path
  per tool, all GETs, no control routes — the same reasoning as alchem-link refusing a
  caller-supplied RPC URL. Row counts are clamped (models ask for 500) and repeated
  identical calls within a turn are answered from cache, because llama-3.3-70b re-calls
  rather than answers when a result looks thin and each round is a whole request against
  a 30/min tier.
- **Live bot state is opt-in and labelled**, tagged `SIMULATED` when it is. The per-turn
  badge is the real guarantee; the prompt instruction is a mitigation, and it was ignored
  entirely until phrased as a required output token rather than a description.
- **Voice drives the mouth.** `SpeechSynthesis` word boundaries produce one open-close per
  word. Chrome silently stops after ~15s of a single utterance, making `splitForSpeech` a
  correctness requirement, and `onend` is unreliable, so a watchdog polling
  `speechSynthesis.speaking` is what stops a missed event locking the UI. `pickVoice`
  ranks **gender before quality** — `SpeechSynthesisVoice` has no gender field, so ranking
  quality first picks "Andrew Online (Natural)" over "Zira" on stock Windows + Edge.

`npm run check:scylar` pins the pure logic — expressions, speech, markdown, commands,
session, tools and gate.

### BOT Chain — measured before ported

`scema-botchain` is the EVM (chain **677**) port, in its own cargo workspace and in the
root `exclude` list. Not tidiness: every current EVM stack wants reqwest 0.12 / rustls
0.23, which is exactly the combination the root pin comments say cannot coexist with
`solana-sdk`'s `curve25519-dalek 3`. One lockfile resurrects the conflict; two make it a
non-issue. The rule that follows is that nothing in there may depend on a crate pulling
`solana-sdk` — `scematica-nn`, `scematica-sentience` and `scemadex-sdk` are safe;
`scematica-core` and its dependents are not.

**The port stops short of a sniper, on evidence.** `botchain-probe` read the chain rather
than the documentation and found **2 pool creations in ~8 days** (0 in 200k blocks, 2 in
1M), 0.29% network utilisation, 2 swaps in a 50-transaction sample, and a token list of
four real tokens followed by 2-to-6-holder test deployments. Consensus is Parlia PoSA — a
BSC fork. There is nothing to snipe yet, and the README records the measurement so the
conclusion can be re-checked rather than believed. Solidity contracts
(`BotchainPriceFeed`, `ScemaArbExecutor`, `ScemaBondEscrow`, `BotchainNNMesh`) are
deployed and tested on 677. **The Solana bot is unaffected and stays authoritative.**

### scema-bot-mesh — inference someone else can check

A neural mesh whose decisions are verifiable by a party that did not run them, including
by a contract on chain 677. Weights are too large for on-chain storage, but a keccak256
hash of them is 32 bytes and so is a hash of an inference — so an agent commits 32 bytes,
any challenger holding the weights re-runs the forward pass, disagreement is provable, and
the bond behind the claim is slashable via `ScemaBondEscrow`.

Commit-and-challenge is old; the reason it is rarely applied to neural inference is that
the challenger's re-run must produce **the same bits**, and floating point does not
cooperate — Solidity cannot represent an `f32` at all, transcendentals are libm
implementations rather than IEEE operations, and JavaScript has no `f32`. So the
foundation is Q16.16 integer arithmetic with the usual implementation details promoted to
specification: round-half-away-from-zero (so `(-x)·y == -(x·y)` exactly), division rather
than `>>` (an arithmetic shift floors toward −∞ and breaks that symmetry — a real bug here,
caught by `multiplication_is_symmetric_under_sign`), fixed summation order, ties to lowest
index, and saturating rather than wrapping arithmetic. `FRAC_BITS`, parameter ordering and
domain tags are bound into the hash, so a future change produces a visibly different
commitment instead of a silently incompatible one.

### alchem-link v0.23.0 — the shell becomes a coding agent

*The 0.23 series shipped through **0.23.24**, which is the current published patch level.
The three sections below describe what 0.23.0 introduced.*

The chat agent could read chains and nothing else. It can now work in a directory: 28
tools covering file reads, writes and edits, directory creation and search, project
scaffolding, result export, and — only when switched on — command execution.

**Codegen goes through the generator, not the model.** Asked for a Chainlink consumer,
the agent calls `generate_consumer` rather than writing Solidity from memory, and the
system prompt says so explicitly. A model writing that contract from training data
produces something that compiles, looks right, hardcodes `3600`, and omits the sequencer
gate. The generator bakes in that feed's *measured* per-chain heartbeat and every check
`audit` looks for. A plausible-looking contract is the same failure class as a
plausible-looking price, which is the rule this agent was built around in the first place.

**Two gates, answering different questions.** `Workspace` decides *where* a tool may act:
paths are fully resolved — symlinks followed, `..` collapsed — and only *then* compared
against the root, because a string check for `..` does not catch a symlink inside the
workspace pointing at `/`. `TrustPolicy` and an `Approver` decide *whether* it acts at
all, with risk declared per tool so a new one cannot arrive unclassified.

**Reading a secret is an exfiltration, not a read.** The non-obvious one, and the reason
the denylist is not overridable by approval: tool results are sent to a third-party
model, so reading `.env` hands over `ALCHEMY_API_KEY` to whoever runs that endpoint. Env
files, PEM and SSH keys, `.npmrc`, cloud credentials and Solana keypairs are refused
before the prompt is even shown, for reads as much as writes, and are omitted from
directory listings and search results rather than merely being unreadable. A user cannot
consent to a disclosure they have not been shown.

**No terminal means deny.** Piped `chat` and CI jobs get `DenyApprover`, because a prompt
that cannot be answered must not be read as consent. `--yes` is the explicit opt-out and
has to be typed. Execution is refused outright until `--allow-exec` and then prompts per
command; it runs without a shell, so the argv in the approval prompt is exactly what
executes with no second parsing layer in between.

**Refusals say why, accurately.** "Running commands is not enabled in this session" and
"the user was asked and declined" are different facts. Reporting the second when no
prompt was shown leaves the user arguing with an assistant about a decision neither of
them made.

The approval prompt leads with the verb and the path rather than the tool name, and `v`
shows the unified diff — approving a write you have not seen is a keystroke, not consent.
Grants are session-scoped and never written to disk. New shell verbs: `:workspace`,
`:trust`, `:changes`, `:diff`. Tests: **479 → 561** at 0.23.0 and **590** as of 0.23.24,
still all offline; `test_agent_workspace.py` is the security suite and most of its cases
assert that something does *not* happen.

### alchem-link v0.23.0 — the terminal system moves in-package

v0.4.0 made the zero-dependency claim true at install time by pushing the TUI into an
optional extra. That was a technicality: the user interface was still the one part of the
product that needed somebody else's code, and anyone who ran `alchem-link-ui` installed
Textual and everything it pulls. **`alchem_link.term` replaces it** — a complete terminal
toolkit in six strictly-layered modules, and now there is no extra to install at all.

`ansi.py` negotiates colour depth and degrades rather than breaking: truecolor → xterm-256
→ the basic sixteen → none. That matters more than it sounds. Emitting `38;2;r;g;b` at a
16-colour terminal does not gracefully lose colour, it prints the digits as text across
every frame; and a Windows console ignores escape sequences entirely until
`ENABLE_VIRTUAL_TERMINAL_PROCESSING` is set, which `enable_vt` does through the Win32 API.
`screen.py` is a double-buffered cell grid that emits only the runs that changed — an idle
frame costs **zero bytes**, which is the difference between a dashboard that is usable over
SSH and one that is not. `input.py` is raw mode plus a *pure* parser from escape sequences
to named keys, including the ambiguity at the heart of terminal input: a bare `Esc` and the
first byte of an arrow key are the same byte until you wait and see.

**Black and blue before the first frame.** Drawing black rectangles gets you a black
*pane*; the columns past the last painted cell and the scrollback above the prompt stay
whatever colour the terminal was. `boot.py` repaints the terminal's **own defaults** via
OSC 11/10/12 and hands them back on exit. It runs from `alchem-link`, from
`alchem-link shell`, and from the compiled binary — the case that matters most, because a
binary launched by double-click lands in a fresh console with no `TERM` at all, which is
where colour detection has the fewest hints and where theming matters most.

Colour stayed decoration: `NO_COLOR`, a pipe, `--no-color` and a 16-colour terminal all
produce **character-identical** text to the full-colour form, asserted in tests, because
this output goes into CI logs and issue reports at least as often as onto a screen.

**Panels render to lines, not to the screen.** A panel returns `List[(text, Style)]` and
the app paints a window onto that list, so scrolling and clipping are one slice and every
renderer stays a pure function. All fifteen panels are tested in their loading, empty and
error states — which matters more here than usual, since a dashboard that crashes takes the
whole screen with it.

### alchem-link v0.23.0 — simulation, sessions and statistics

**`simulate` — would your guards have caught it?** `audit` describes a feed as it is now.
This replays *your* consumer's checks against the failure modes that have already cost
people money: the LUNA bounded-crash shape, a frozen feed, an L2 sequencer outage, carried
rounds, a flash spike, a future timestamp. The default guard — a staleness window and a
positivity check, which is what most integrations have — scores **4/8**, and
`bounded_crash` is in the miss list: every reading after the feed pins to `minAnswer` is
fresh, positive, complete and orders of magnitude wrong, and only a consumer-side bound or
a move limit sees it. There is a healthy control in the set so that "reject everything"
cannot score well, and `backtest` runs the other direction against real round history,
where a rejection is a false positive rather than a catch.

**`AlchemLink` — one object per session.** The functional API builds a client per call:
five reads means five clients, five Multicall3 probes and five sets of statistics that add
up to nothing reportable. `connect("base")` holds the network, the connection and a cache
whose TTL comes from each feed's *measured* heartbeat — 20s for a 60s Polygon feed, two
minutes for an hourly mainnet one. `price(..., strict=True)` raises `StaleFeed` instead of
returning a reading that merely says so, for the paths where forgetting to check `.stale`
is the bug.

**`analytics`** computes TWAP time-weighted rather than sample-weighted, because an oracle
publishes most often precisely when the price is moving and the mean of the answers
therefore over-weights volatile periods; and annualises volatility by the series' own
measured spacing rather than an assumed interval, which is why the same asset used to
report wildly different volatility on Polygon and Ethereum. **`logs`** reads publish
history from `AnswerUpdated` events — one `eth_getLogs` against a hundred `eth_call`s —
resolving the proxy first, since the address you consume emits nothing. **`parallel`** fans
a read across every chain concurrently with failures reported as rows rather than raised.
**`exporters`** adds CSV, NDJSON, Markdown and a **Prometheus** scrape body, so
`alchem_link_feed_stale == 1` becomes a complete alert rule. **`registry`** adds search,
fuzzy pair resolution that suggests rather than silently substitutes, and coverage
reporting. **`errors`** gives everything one hierarchy under `AlchemLinkError` while
keeping the builtins the replaced classes inherited — `UnknownNetwork` is still a
`KeyError`, `AbiError` still a `ValueError` — so no existing `except` clause breaks.

Tests: **214 → 479**, still all offline. That constraint is the point rather than a
convenience: these modules compute numbers people size positions and write guards against,
and a number that can only be checked against a live chain cannot be checked at all.

## What's New in v1.15.0

### One version for the bot stack

`scematica-nn` had drifted to 1.14.0 while the rest of the stack sat at 1.13.0, then took
changes (`equations.rs`) *after* 1.14.0 was already on crates.io — so it could not
republish under its own number, and the README's version table disagreed with the
manifests. Every `scematica-*` crate now inherits `[workspace.package] version`, which is
**1.15.0**; `scematica-nn` and `scematica-suite` no longer pin their own. A crate cannot
drift ahead of the stack again without editing one line that moves all of them.

1.13.0 and 1.14.0 were never published as a complete stack, so no downstream release is
skipped. `web/package.json` — the single source for the web dashboard and the Android
artifact's `versionName`/`versionCode` — moves to 1.15.0 with it. The ScemaDEX SDK family
(`scemadex-sdk` 0.3.0, `scemadex-mcp`/`scemadex-settle` 0.1.2) is unchanged since its last
publish and keeps versioning independently below 1.0.

### Collapse detection: dispersion, not magnitude

The DQ\* veto guard tested whether the bearish Q exceeded the best buy Q by a relative
margin. On 2026-08-05 the net returned `SellPartial` at Q\* ≈ 26.5 against a best-buy Q of
≈ 13.5 for **25 consecutive pools** and blocked every buy. The guard read that as maximum
conviction — and on its own terms it was, because a policy collapsed onto one action
produces a large, stable gap on *every* input. A margin test passes trivially against a
constant function.

Magnitude cannot distinguish conviction from collapse; only dispersion can. `equations.rs`
in `scematica-nn` makes the Scematica equations running instrumentation, supplying the
intelligence ratio `I = Var_Σ[Q*] / E_Σ[Q*]²` — a squared coefficient of variation over a
32-sample window, below `1e-4` (≈1% relative spread) meaning the valuations do not depend
on the pool. The sniper now carries an `EquationMonitor` alongside the agent: past a veto
streak the veto degrades to size-down rather than holding the gate shut, so a
non-discriminating net keeps training and keeps its sizing influence but cannot silently
kill the edge. The equation terms publish next to the agent snapshot, so a collapsed
policy is visible on the dashboard instead of only in the log.

Statements in [EQUATIONS.md](EQUATIONS.md); derivations and the collapse case study in
[EQUATIONS-ANALYSIS.md](EQUATIONS-ANALYSIS.md).

### alchem-link v0.4.0 — from a reader to an oracle auditor

v0.3.0 could read a price and tell you whether it was stale. The failure modes that
actually cost protocols money all *return successfully*, and none of them were covered.

**Keccak-256, in-package.** The old package hardcoded four function selectors because
`hashlib` ships SHA3-256, whose padding differs. `keccak.py` implements the real thing in
~100 lines of integer arithmetic, so selectors are now **computed**, pinned in tests
against the standard vectors *and* the four constants that had been verified live. That
unlocked a full ABI codec — `address`/`uint<N>`/`int<N>`/`bytes<N>`/`bytes`/`string`,
dynamic arrays and tuples, including the `(address,bool,bytes)[]` that
`Multicall3.aggregate3` needs — plus EIP-55 checksumming and revert decoding.

**Batched reads.** Multicall3 with a JSON-RPC-batch fallback and a sequential fallback
below that. Ethereum's 16 feeds went from 48 HTTP round trips (~20s) to **2 (607ms)**, and
the report says which tier ran: only Multicall3 is block-atomic, without which comparing
two feeds compares two different moments.

**`audit` — the consumer-safety lint.** Stale rounds, non-positive answers, incomplete
rounds, carried-over answers (`answeredInRound < roundId`), description/decimals
mismatches, and `BOUNDED_ANSWER` — the LUNA failure mode, where a feed pinned against its
`minAnswer` circuit breaker keeps returning the floor, fresh and well-formed and orders of
magnitude wrong. Seeing it requires resolving the proxy and reading bounds off the
implementation, which `inspect` now does.

**L2 sequencer gating.** Three uptime feeds, each verified as
`L2 Sequencer Uptime Status Feed`, with Chainlink's grace period applied — the second
check almost everyone omits, which reopens a protocol exactly during the post-outage queue
flush it was written to survive.

**Measured heartbeats.** The registry declared 3600s for every feed, inherited from
mainnet. `cadence.py` walks round history and separates heartbeat-triggered publishes from
deviation-triggered ones, recovering both parameters. The real values: **Polygon ~60s**,
Optimism/Base 1200s, Arbitrum USDC 300s. A 3600s staleness check on Polygon would not fire
until the feed had been dead for an hour. It also refuses to guess — a window with no quiet
period reports the heartbeat as *not observed* rather than inventing one from where the
window happened to end.

**Cross-chain divergence.** The same pair on every chain that carries it, in basis points,
with stale legs excluded from consensus and outliers attributed. Testnets are excluded
outright: Sepolia's feeds carry unrelated data and were showing up as the widest
"divergence" in every early run.

**Registry: 18 → 66 feeds, 6 → 11 networks** (adds Avalanche, BNB, Gnosis, Scroll, Linea).
Every address verified against its own `description()`; the check caught the Gnosis address
commonly labelled "xDAI/USD" reporting `DAI / USD`. Two candidate CCIP routers had no code
and were dropped.

**Also:** `generate` emits a consumer contract with every audited check wired in and the
per-chain measured heartbeat baked into `MAX_AGE`; `gas` prices EIP-1559 tiers in USD
through the chain's own Chainlink feed; `ccip` verifies routers as `Router 1.2.0` and
probes lanes via `isChainSupported`; `holdings` values a portfolio keylessly; `watch`
streams rounds as JSON Lines. The four prose-dict reference modules now report what the
package actually does and what the current endpoint can actually reach.

CLI moved to real subcommands. `textual` became an optional extra (`alchem-link[tui]`), so
the zero-dependency claim is now true at install time. Tests: **76 → 214**, all offline.

## What's New in v1.13.0

### Web dashboard — real data without a bot

The web dashboard no longer depends on a local sniper to show something real. Discovery now runs off a live public feed, scored by a TypeScript port of the sniper's own pipeline.

**Shared polling store** (`web/lib/store.ts`, `queries.ts`): panels used to own a `setInterval` each — 19 timers, ~251 requests/min, with `/api/health` polled by three components, `/api/positions` by two, and `/api/trades` by four. There is now one timer per endpoint key, fanned out to subscribers and refcounted, so duplicate consumers cost nothing and hidden panels stop fetching entirely. Pro mode with a bot paired: ~169 req/min. Beginner mode: ~102, because the `AdvancedOnly` panels unmount and their upstreams go quiet.

**Live mint feed** (`web/lib/feed/jupiter.ts`): reads recently-created mints from Jupiter's keyless, CORS-open token endpoint and normalises them to the shapes the panels already consume. USD liquidity is converted to SOL through a shared price cache.

**Ported pool scorer** (`web/lib/feed/scorer.ts`): the likelihood-ratio ladders and logistic from `pool_scorer.rs`, plus seven filters. Each filter declares `parity` — `port` for a faithful translation, `approx` where the Rust input does not exist in the feed. `npm run check:parity` pins the cases the Rust unit tests assert, so the two implementations cannot drift silently.

**Wallet-signed swaps** (`web/lib/swap.ts`, `components/ManualSwap.tsx`): non-custodial execution via Jupiter quote → build → wallet signature → confirm. Labelled NOT SNIPING in the UI, because a wallet prompt puts a human in the loop for seconds. Hidden in the Capacitor shell, which connects wallets by deeplink but cannot sign.

### Web dashboard — fixes

- **Dead price endpoint.** `price.jup.ag` no longer resolves; every poll produced a `net::ERR_NAME_NOT_RESOLVED`. Replaced with Jupiter Price v3, which quotes in USD, so the SOL-denominated price is derived from a combined SCEMA + WSOL request.
- **Static-export 404 storm.** The mobile/static build ships no Next server, so `/api/*` resolved against the static host — a 404 per panel per poll, each returning an HTML error page that callers then failed to parse as JSON. `apiFetch` now short-circuits to a synthetic 503 when the build has no proxy and nothing is paired, and `MobileGate` offers the pairing screen in that case instead of polling into the void.
- **`probePairing` did not validate the token.** It probed `GET /api/controls`, which is not token-gated — only the control POSTs carry `require_token` — so pairing reported success for any token, including none. It now POSTs to a gated route with a deliberately malformed body: the auth middleware runs before the `Json` extractor, so a bad token gives 401 and a good one gives 422 without the handler ever running. That matters because `params_handler` writes `scematica-rate-mode.json`; a valid probe body would have silently rewritten live TP/SL on every pairing attempt.
- `probePairing` returns a discriminated result, so the pairing screen distinguishes "unreachable" from "token rejected" rather than showing one combined message.

### alchem-link v0.3.0 — a toolkit that does something

The Python kit was a scaffold: every function returned a hardcoded dict of prose and nothing touched a network. It now reads live Chainlink price feeds and Alchemy-served chain state, with no dependencies beyond the standard library.

**Zero-dependency chain reads.** Function selectors are stored as the constants they are — the stdlib ships SHA3-256, which is not Keccak-256 — so reading an aggregator needs no hashing library and no `web3`. Each selector was verified against a live mainnet aggregator.

**Verified feed registry** (`feeds.py`): 18 feeds across Ethereum, Sepolia, Base, Arbitrum, OP and Polygon. Every address was called for `description()` and `decimals()` before being registered, and each is filed under the pair the contract itself reports. That check caught a real error: the address widely shared as Base "BTC/USD" reports `WBTC / USD`, and is registered under that name because WBTC can depeg.

**Staleness detection.** A feed that responds is not necessarily a feed that published. Readings carry age against the expected heartbeat and report FRESH, STALE or INVALID. Stablecoin feeds get a longer heartbeat than volatile pairs, matching observed behaviour.

**New commands:** `price`, `feeds --live`, `block`, `gas`, `networks`, `doctor`, `verify`. The reference commands (`blueprint`, `alchemy`, `chainlink`, `integration`, `recipes`) are unchanged. Works with no API key against a public endpoint; set `ALCHEMY_API_KEY` for real rate limits.

**Live Feeds panel** in the TUI, reading off the UI thread so an RPC round trip cannot freeze the app. `r` refreshes, `n` cycles networks.

Test suite grew from 12 to 76 cases, all offline.

---

## What's New in v1.12.0

### Fibonacci Recovery System — Live Integration

The three Fibonacci modules (`fibonacci_momentum`, `fibonacci_pool_scorer`, `fibonacci_recovery_system`) are now fully wired into the sniper execution path.

**Entry gate:** Every incoming pool is evaluated by `FibonacciRecoverySystem::evaluate_entry()` before the pool scorer. Pools scoring below 0.55 on the composite Fibonacci signal are rejected with a logged reason. The gate weighs pool size (35%), pool age (30%), inflow velocity (25%), and buy pressure (10%) against golden-ratio thresholds.

**Position sizing:** `calculate_position_size()` applies a multiplier to the configured quote amount — 2.0× for exceptional entries (score ≥ 0.90), 1.618× for strong (≥ 0.75), 1.0× baseline, 0.5× for weak patterns. The Fibonacci Runner fast-lane (all four signals at maximum strength) bypasses normal scheduling and executes immediately.

**Exit tracking:** `FibonacciMomentum` runs inside every sell monitor. The golden retracement exit (61.8% pullback from peak) is evaluated each price-check tick alongside the existing exit ladder. Stats are logged every 10 exits: win rate, average PnL, entry/exit counts.

**Stats IPC:** `fib_system` and `fib_stats` are shared via `Arc` between the sniper and all sell monitor tasks, consistent with the file-based IPC architecture.

### alchem-link v0.2.0 — TUI Dashboard

The Python developer kit ships a full terminal UI (`alchem-link-ui`) built on Textual. Sidebar navigation across Blueprint, Alchemy, Chainlink, Integration, and Recipes panels. All data rendered as formatted cards — no raw JSON. Recipe drill-in with step-by-step checklist. Standalone `.exe` buildable via PyInstaller. Published to PyPI.

---

## What's New in v1.11.0

### Intelligence Data Pipeline

The sniper, API, terminal dashboard, and web dashboard now share one runtime artifact directory. By default this resolves to the workspace root; set `SCEMATICA_DATA_DIR` to override it for deployments. This fixes live runs where one process wrote `scematica-nn-advice.json`, `scematica-pool-decisions.jsonl`, or `scematica-tx-telemetry.jsonl` in a different working directory than the API/dashboard were reading.

The sniper now creates the Intelligence artifacts at startup:

| File | Producer | Consumer |
|---|---|---|
| `scematica-nn-advice.json` | Deep Q* agent startup + entry advice path | TUI Intelligence tab, web dashboard, `/api/nn-advice` |
| `scematica-pool-decisions.jsonl` | Pool gate ledger in `sniper.rs` | TUI Intelligence tab, web dashboard, `/api/decisions` |
| `scematica-tx-telemetry.jsonl` | Transaction executor in `executor.rs` | TUI Intelligence tab, web dashboard, `/api/tx-telemetry` |

New API endpoints:

```text
GET /api/nn-advice
GET /api/intelligence?limit=80
```

`/api/intelligence` returns the latest NN stats/advice plus recent pool decisions and transaction telemetry in one response. The web dashboard's Intelligence section now renders live DQ* advice, Q-values, pool decisions, and execution-quality telemetry from these endpoints.

### Profit Claim Clarification

`scematica_analysis.md` documents the profit model, risk controls, and limits of the system. Scematica can enforce execution and risk invariants, but it cannot honestly guarantee profit in adversarial, probabilistic markets.

---

## What's New in v1.10.0

### Pump.fun Trending Monitor

A new `pumpfun_trending.rs` module connects to PumpPortal's WebSocket feed and scores bonding curves in real time, firing a `ListenerEvent::NewPool` event **0.5–3 seconds before** the standard AMM V4 `InitializeInstruction` listener sees the same pool.

**How it works:**

Each bonding curve accumulates a sliding-window trending score from three signals:

| Signal | What it measures |
|---|---|
| Buy pressure | Net buy-side delta in the observation window |
| Volume velocity | SOL/s flowing into the curve |
| Curve fill % | How far the bonding curve is toward the graduation threshold |

When `trending_score ≥ 55` (configurable) AND `curve fill ≥ 40%`, the curve is emitted as a pool candidate. Graduating tokens — those where the curve fill has crossed the 100% threshold — are pre-flagged and bypass the standard entry delay entirely.

**Config** (`[sniper]` section in `config.toml`):
```toml
pumpfun_trending_enabled = true
pumpfun_trending_score   = 55.0     # minimum score to emit as candidate
pumpfun_min_curve_pct    = 40.0     # minimum curve fill %
pumpfun_window_secs      = 120      # sliding window for score accumulation
```

The trending listener runs in parallel with the existing AMM V4 listener and Whale Copy listener — all three merge into one `ListenerEvent::NewPool` stream, so the filter pipeline and executor are unchanged.

### Exit Reason Coverage (Complete)

All 16 sell paths now populate `exit_reason` in `TradeEvent` structs, completing the work started in v1.9.0. Previously, the arb executor and `sell_with_min_out` paths were missing the field, producing blank exit_reason in the trades log. BUY events correctly carry an empty `exit_reason`. The dashboard exit breakdown analytics panel now has full coverage.

---

## What's New in v1.9.0

### Exit Reason Tracking

`exit_reason` is now populated on every sell path and written to `scematica-trades.jsonl`. Previously, all trades showed a blank exit reason, making it impossible to distinguish why a position closed.

**Exit reasons tracked:**

| Code | Trigger |
|---|---|
| `take_profit` | Hit dynamic TP level |
| `stop_loss` | Fell below hard SL floor |
| `trailing_stop` | Dropped > trailing stop % from peak |
| `velocity_decay` | Momentum second derivative negative |
| `peak_stagnation` | Peak unchanged for 90s with PnL ≥ 20% |
| `dump_detected` | 3 consecutive declining checks |
| `fibonacci` | Fibonacci golden retracement exit |
| `no_pump_timeout` | Dead-zone timeout (peak < 3% after N seconds) |
| `sell_mode` | Operator or drawdown guard triggered Sell Mode |
| `dump_mode` | Operator triggered Dump Mode |
| `volume_exhaustion` | Quote vault volume dropped below threshold |
| `tiered_tp` | Tiered partial-TP ladder completed |
| `timeout` | `price_check_duration_ms` window expired |

### Weekend Auto-Switch

Live session data from 573 trades showed dramatically lower win rates on weekends vs weekdays (0% Saturday, 22% Friday, 32% Monday). The bot now automatically adjusts its rate mode based on the UTC day of week.

**How it works:** A 10-minute watcher in `main.rs` checks `chrono::Utc::now().weekday()` and writes `scematica-rate-mode.json`. On Saturday/Sunday it switches to the configured `weekend_mode`; on Monday–Friday it restores `weekday_mode`. The change takes effect within 10 minutes of the day boundary, with no restart needed.

**Config** (`[sniper]` section):
```toml
weekend_mode  = "Bearish"    # Sat/Sun: 0.3× size, TP 30%, SL 8%
weekday_mode  = "Balanced"   # Mon-Fri: 1.0× size, TP 100%, SL 15%
```

### Time-of-Day Controls

`time_of_day_weighting` is now enabled and calibrated from 573 live trades. Low-traffic UTC hours (1am–9pm) are blocked by default.

**Config:**
```toml
time_of_day_weighting = true
blocked_hours_utc     = [1, 21]    # block UTC hours 1–21 (active window: 9pm–1am UTC)
```

### NN Reward Overflow Fix

**Root cause:** The NN observer backfilled `pnl_pct` for old trade entries using `pnl_sol / 0.01 * 100.0`. A 0.9 SOL winning trade produced `pnl_pct = 9000%`, which passed into `shape_reward()` → reward ≈ 85,000 → Q-value divergence. Live symptom: `avg_loss = 331,882` in the NN stats panel.

**Fix:** Old entries without a `pnl_pct` field now use `0.0` (neutral reward). All 18 state inputs are clamped to `(−200, 500)` before the forward pass to prevent future divergence regardless of bad data.

> **Action required after upgrading:** Delete `scematica-nn-agent.json` before restarting to reset the diverged Q-weights. The agent will retrain from scratch, reaching `ready_to_advise` again once `epsilon < 0.5`.

---

## What's New in v1.8.2

### Exit Strategy — 99% PnL Glitch + Dead Capital Fixes

**Root-cause analysis of the 99% exits (7–11 min holds):**

The live data showed 86 all-time trades exiting at ~99% gain despite a 175% take-profit target. These positions hit 175%+ early, locked in the profit-floor SL at 2.75× entry, then slowly bled back over 7–11 minutes. The `velocity_decay_exit` was supposed to catch this but was gated behind `velocity_decay_min_pnl_pct = 175%` — so once the pool dropped below 175% (while still at 100–174%), the decay exit was silently disarmed. The pool eventually hit the 2.75× floor from below, executing at 99% market price.

**Fix 1: Lower velocity decay threshold (config — no rebuild)**

`velocity_decay_min_pnl_pct = 100.0` (was 175%). Velocity decay now fires at 100%+ gain, catching bleeder pools while they're still between 100% and 175%, before they bleed through the profit floor.

**Fix 2: Peak stagnation exit (code — requires rebuild)**

New config keys `peak_stagnation_secs = 90` and `peak_stagnation_min_pnl_pct = 20.0`. If the position's all-time peak hasn't improved in 90 seconds AND current PnL is above 20%, the monitor exits at market. This catches flat pools that pumped once then stopped — previously these would hold for 7–11 minutes before hitting the SL floor. Logged as `⏱ Peak stagnation exit`.

**Fix 3: Tighter trailing stop (config — no rebuild)**

`trailing_stop_loss_pct = 25.0` (was 50%). At 300%+ peaks, the trailing stop now becomes the binding constraint (not the floor), exiting sooner on parabolic reversals.

**30–120s dead zone (data pattern):**

156 trades (27% of all) in the 30–120s hold bucket with only 6% win rate contributed +0.054 SOL total. These are pools that gained >5% early (suppressing the 20s no-pump timeout), then oscillated. The peak stagnation exit with `peak_stagnation_secs=90` captures most of these.

**Config changes:**
```toml
trailing_stop_loss_pct = 25.0
velocity_decay_min_pnl_pct = 100.0
peak_stagnation_secs = 90
peak_stagnation_min_pnl_pct = 20.0
```

---

## What's New in v1.8.1

### Exit Strategy — Stuck-Position Fixes

Two bugs were preventing positions from exiting cleanly in the 175–315% gain window.

**Bug 1: Adaptive pullback formula made exits impossible (198% glitch)**

With `adaptive_pullback = true` and `momentum_pullback_exit_pct = 40.0`, the effective pullback threshold at a 198% peak was `40 × √(1 + 198/100) = 69.1%`. The pullback exit required `current ≤ 129%`, but `exit_gate_met` required `current ≥ 175%` (the profit floor). These two conditions can never both be true — the position held indefinitely between 175% and 315% (the next escalation level).

**Fix:** `adaptive_pullback = false` + `momentum_pullback_exit_pct = 15.0` + `momentum_min_peak_pct = 200.0`.

At 200% peak, the pullback fires at 185% — above the 175% profit floor (satisfiable). The invariant `momentum_min_peak_pct > initial_tp_pct + momentum_pullback_exit_pct` (200 > 175 + 15) must always hold when changing these values.

**Bug 2: Position tracking — tokens falling through the cracks**

The sell monitor exited immediately on the first zero-balance read after a buy confirmation. Solana RPC nodes can lag 1–3 checks behind a confirmed transaction, so the monitor would see `amount = 0`, silently quit, and leave tokens unmonitored in the wallet (recovered only on next bot restart via the startup scan).

**Fix:** Zero-balance grace period — the monitor now requires 5 consecutive zero-balance reads before exiting. Single-check RPC lag no longer loses a position.

**Config changes** (`config.toml` — no rebuild needed):
```toml
adaptive_pullback = false
momentum_pullback_exit_pct = 15.0   # was 40.0
momentum_min_peak_pct = 200.0       # unchanged; satisfies the invariant with pullback=15
velocity_decay_min_pnl_pct = 175.0  # was 200.0 — arms decay exit at initial TP
```

---

## What's New in v1.8.0

### Exit Strategy Overhaul — Escalation Ladder Working

Three bugs were blocking the momentum escalation ladder entirely, causing trades to cluster at discrete TP thresholds (99%, 298%, 398%) rather than riding the full 175→315→567→1021→1837% ladder.

**Bug 1: Stale `target_profit` (critical)**

`target_profit` was computed once per loop iteration at the top. When escalation fired and raised `dynamic_tp_pct` (e.g., 175→315%), the TP check at the bottom of the same iteration still used the old `target_profit`. The bot escalated AND immediately sold at the old threshold in the same tick.

**Fix:** Made `target_profit` `mut` and refreshed it in-place after every escalation.

**Bug 2: Velocity window blocked fast pumps**

Escalation required `velocity_window.len() >= 5` (1.25s of samples). 87 of 176 winners exit in <2s — they pump in <500ms and the window never fills before TP is already hit.

**Fix:** Require only 1 sample (`!velocity_window.is_empty()`). Added `single_jump` override: if the pool gains 50%+ past TP in one check, escalate unconditionally.

**Bug 3: PnL used pre-swap AMM estimate**

`do_sell` logged the AMM `estimated_out` rather than actual received tokens. A 2× pool shows 99% gain (not 100%) due to the 0.25% swap fee applied against pre-swap reserves.

**Fix:** After sell confirms, fetch the quote ATA balance for actual received amount.

---

## What's New in v1.7.0

### DexScreener Paid Boost — Guaranteed Buy Override

A new `dexscreener.rs` module queries the DexScreener API for each incoming pool's base mint. If the token has an active **paid boost** (non-zero `boostAmount`), it is treated as a guaranteed buy signal and skips both the Fibonacci entry gate and the Bayesian pool score gate.

**Why this works:** A project that has purchased DexScreener advertising has spent verifiable USD on marketing. Rug teams do not buy ads before rugging. Boosted tokens have real visitor traffic and demonstrated team commitment — empirically the strongest pre-launch signal available off-chain.

**How it's implemented:**
- `DexScreenerCache` caches results per-mint for 5 minutes (one HTTP call per token, not per pool event)
- API call has a hard 1.5 s timeout; any failure is fail-open (normal evaluation continues)
- When boost is detected: `🚀 DEXSCREENER PAID BOOST — skipping Fibonacci + pool score gates (guaranteed buy)` is logged with the USD boost amount
- All on-chain fraud filters (freeze authority, vault drained, LP burned) still apply — only the scoring gates are bypassed

**Config:** No config change needed. The boost check runs automatically in normal mode (not high-speed).

### Pool Evaluation — Calibrated Loosening

Three filter thresholds were tightened too aggressively in previous versions, causing good pools to be rejected:

| Parameter | Old value | New value | Reason |
|---|---|---|---|
| `min_pool_score` | 60 | **45** | Score-60 required near-ideal conditions; score-45 still rejects dead pools while accepting moderate runners |
| Fibonacci `min_entry_score` | 0.75 | **0.55** | A 12 SOL pool at 10 s with 0.8 SOL/s inflow scored 0.53 and was always rejected — now accepted |
| `max_top10_holder_pct` | 75% | **90%** | Brand-new pools always have high initial concentration (LP vault + deployer); 75% was rejecting legitimate launches |

**What "not too broad" means in practice:** The Fibonacci gate still rejects pools older than 13 seconds, pools with zero velocity, and pools outside the 3–55 SOL band. The Bayesian score gate still rejects pools scoring below 45 (roughly: sub-3 SOL, completely stale age, or ghost pools).

### Fibonacci Protocol Whitepaper

See [FIBONACCI_PROTOCOL_WHITEPAPER.md](docs/FIBONACCI_PROTOCOL_WHITEPAPER.md) for the full mathematical specification of the scoring model, entry gate, position sizing ladder, exit strategy, and live data calibration.

---

## What's New in v1.6.0

### Fibonacci Protocol — Entry/Exit Framework

A new mathematical entry/exit framework built on the golden ratio (φ ≈ 1.618) and Fibonacci sequence applied to AMM pool dynamics. See the full spec in [FIBONACCI_PROTOCOL_WHITEPAPER.md](docs/FIBONACCI_PROTOCOL_WHITEPAPER.md).

**New modules:**
- `fibonacci_momentum.rs` — per-position momentum tracker with Fibonacci TP levels, golden retracement, and velocity-collapse detection
- `fibonacci_pool_scorer.rs` — combines the existing Bayesian scorer with Fibonacci pattern bonuses (+0 to +15 points additive)
- `fibonacci_recovery_system.rs` — entry gate + position sizing + exit ladder coordinator

**Entry gate (composite Fibonacci score, threshold 0.55):**

| Signal | Weight | Key thresholds |
|---|---|---|
| Pool size | 35% | Sweet spot: 8–21 SOL (F₆–F₈) |
| Pool age | 30% | Peak: ≤3 s (F₄); acceptable: ≤13 s (F₇) |
| Inflow velocity | 25% | Strong: ≥φ SOL/s (1.618); exceptional: ≥φ² SOL/s (2.618) |
| Buy pressure | 10% | Golden: quote/base ratio ≥ φ |

**Fibonacci Runner fast-lane:** pools that hit all four criteria at maximum strength (`8–21 SOL`, `≤5 s`, `≥2.618 SOL/s`, `ratio ≥ 1.618`) skip normal scheduling and execute immediately.

**Position sizing multipliers:** 2.0× for score ≥ 0.90 (exceptional), 1.618× for ≥ 0.75 (strong), 1.0× baseline, down to 0.5× for weak patterns.

**Fibonacci exit ladder:**
- Dead-pool exit: no movement after 3 s with < 5% peak gain → immediate sell
- TP₁: 61.8% gain (sell 30%)
- TP₂: 161.8% gain (sell 40%)
- TP₃: 261.8% gain (sell 30%)
- Golden retracement: 61.8% pullback from peak → exit

### Guaranteed ≥0.05 SOL Exits — Swell-Based Exit Gate

All momentum/timing exits (trailing stop, adaptive pullback, velocity decay, volume exhaustion, whale exit, flash crash, 3-consecutive-decline dump detection) are now **gated behind the initial take-profit level (500%)**.

**What this fixes:** Previous behavior allowed the trailing stop (5%), pullback exit (15%), velocity decay, and dump detection to fire at +50–300% gains, returning sub-0.05 SOL profit on a 0.01 SOL buy. With the exit gate, the bot holds through all market noise below the 500% target and only activates timing exits once the position has reached ≥500% gain.

**Live swell signal:** The sell monitor now tracks a 6-check sliding window of quote vault deltas (net SOL flow). When the vault is actively draining (pool is selling off) AND the position is at/above TP, the trailing stop tightens to 2% (from the configured value) to lock gains before the reversal completes.

**Profit floor:** Once the position first hits the TP price (500% gain), the stop-loss floor is raised to exactly that level. Any subsequent exit — whether from trailing stop, pullback, or time-cap — is guaranteed to return ≥0.05 SOL profit.

**Hard SL and no-pump timeout are exempt** — they still fire at their configured levels to protect against rugs and dead positions.

### Social Link Enrichment — "Biggest Hitters" Pool Selection

Every pool now runs through a new `SocialLinksFilter` that:

1. **Reads Metaplex on-chain metadata** — extracts real name and symbol (instead of "UNKNOWN" in logs)
2. **Fetches off-chain URI JSON** (1.5s timeout) — checks for Twitter, Telegram, website, Discord links in pump.fun and Metaplex extension format
3. **Populates `FilterPipeline::metadata`** cache with enriched token info for downstream use

**Pool scorer boost:** `score_with_socials()` applies additive score adjustments based on social count (−4 for zero socials → +10 for all four platforms). Anonymous tokens with zero social presence are penalised; well-connected projects are promoted.

**AI enrichment:** The risk-scoring AI now receives real token name and symbol instead of "UNKNOWN", producing more meaningful context-aware analysis.

**Social rejection (opt-in):** Enable `check_socials = true` in `config.toml` to hard-reject tokens with zero social links. Currently off by default to avoid false-positives on legitimate projects that haven't set their URI yet at pool creation time.

**New config fields** (in `[sniper]` section):
```toml
momentum_min_peak_pct = 500.0       # Pullback exit only fires after peak >= 500%
velocity_decay_min_pnl_pct = 500.0  # Decay exit only fires when PnL >= 500%
volume_exhaustion_pct = 0.0         # Disabled — swell gate handles vault drain
whale_exit_vault_drop_pct = 0.0     # Disabled in profit zone
flash_crash_pct = 0.0               # Disabled in profit zone; SL handles crashes
profit_lock_checks = 0              # Disabled — profit floor in code locks 0.05 SOL
```

Enable `check_socials = true` in `[sniper.filters]` to require social presence.

---

### Guaranteed ≥0.05 SOL Exits — Swell-Based Exit Gate

All momentum/timing exits (trailing stop, adaptive pullback, velocity decay, volume exhaustion, whale exit, flash crash, 3-consecutive-decline dump detection) are now **gated behind the initial take-profit level (500%)**.

**What this fixes:** Previous behavior allowed the trailing stop (5%), pullback exit (15%), velocity decay, and dump detection to fire at +50–300% gains, returning sub-0.05 SOL profit on a 0.01 SOL buy. With the exit gate, the bot holds through all market noise below the 500% target and only activates timing exits once the position has reached ≥500% gain.

**Live swell signal:** The sell monitor now tracks a 6-check sliding window of quote vault deltas (net SOL flow). When the vault is actively draining (pool is selling off) AND the position is at/above TP, the trailing stop tightens to 2% (from the configured value) to lock gains before the reversal completes.

**Profit floor:** Once the position first hits the TP price (500% gain), the stop-loss floor is raised to exactly that level. Any subsequent exit — whether from trailing stop, pullback, or time-cap — is guaranteed to return ≥0.05 SOL profit.

**Hard SL and no-pump timeout are exempt** — they still fire at their configured levels to protect against rugs and dead positions.

### Social Link Enrichment — "Biggest Hitters" Pool Selection

Every pool now runs through a new `SocialLinksFilter` that:

1. **Reads Metaplex on-chain metadata** — extracts real name and symbol (instead of "UNKNOWN" in logs)
2. **Fetches off-chain URI JSON** (1.5s timeout) — checks for Twitter, Telegram, website, Discord links in pump.fun and Metaplex extension format
3. **Populates `FilterPipeline::metadata`** cache with enriched token info for downstream use

**Pool scorer boost:** `score_with_socials()` applies additive score adjustments based on social count (−4 for zero socials → +10 for all four platforms). Anonymous tokens with zero social presence are penalised; well-connected projects are promoted.

**AI enrichment:** The risk-scoring AI now receives real token name and symbol instead of "UNKNOWN", producing more meaningful context-aware analysis.

**Social rejection (opt-in):** Enable `check_socials = true` in `config.toml` to hard-reject tokens with zero social links. Currently off by default to avoid false-positives on legitimate projects that haven't set their URI yet at pool creation time.

**New config fields** (in `[sniper]` section):
```toml
momentum_min_peak_pct = 500.0       # Pullback exit only fires after peak >= 500%
velocity_decay_min_pnl_pct = 500.0  # Decay exit only fires when PnL >= 500%
volume_exhaustion_pct = 0.0         # Disabled — swell gate handles vault drain
whale_exit_vault_drop_pct = 0.0     # Disabled in profit zone
flash_crash_pct = 0.0               # Disabled in profit zone; SL handles crashes
profit_lock_checks = 0              # Disabled — profit floor in code locks 0.05 SOL
```

Enable `check_socials = true` in `[sniper.filters]` to require social presence.

---

## What's New in v1.5.2

### Live-Data PnL Improvements — Overnight Session Analysis

Four targeted improvements driven by overnight session data (628 trades, +77% ROI on 0.1597 SOL start):

**DeployerWalletAge filter disabled by default** — Pump.fun ALWAYS creates fresh deployer wallets (0 hours old at pool creation). The 24h `deployer_min_age_hours` default was rejecting 100% of pump.fun pools at the current session start (3/3 rejections observed). Disabled in `config.toml`; deployer quality is now handled by the reputation scoring system (`scematica-deployer-reputation.json`) which uses EMA-blended rug history instead of wallet age.

**`min_pool_score` raised 35 → 65** — Score-47 thin pools (≤0.9 SOL liquidity) caused -72% to -90% slippage losses from early sessions. The pool sweet spot confirmed by overnight data is 15–28 SOL (score 98). Setting `min_pool_score = 65` in `config.toml` blocks these thin pools while passing all high-conviction targets.

**`no_pump_timeout_secs` reduced 45 → 30** — Overnight data showed zero profitable trades held past 30 seconds (all wins exited within 6 s via TP or fast-poll sell monitor). Reducing the dead-zone exit timeout from 45 → 30 s recycles capital ~33% faster with no effect on winning trades.

**Dump-mode fresh-position protection (`min_dump_hold_secs`)** — New config field (default 0, set to 90 in `config.toml`). When `dump_mode` fires without `sell_mode`, positions younger than `min_dump_hold_secs` are held through normal TP/SL instead of being force-sold at `min_out=0`. Prevents dump mode from destroying a freshly-entered position mid-pump (observed: -60% loss on a 61-second position at session end). Full `sell_mode` still clears all positions immediately regardless of age.

---

## What's New in v1.5.1

### Extended-Session Reliability — Bug Fixes

Four bugs identified from live-session diagnostics that could cause the bot to silently stop buying or produce incorrect behavior after hours of runtime:

**`open_positions` underflow on restart with existing positions** — Critical: `scan_existing_positions` spawned sell monitors for pre-existing wallet tokens WITHOUT incrementing `open_positions`. When those monitors closed they called `fetch_sub(1)` on a zero counter, wrapping to `u32::MAX`. This corrupted the buy-limit sell-mode auto-clear logic for the entire session (the `prev_open == 1` trigger could never fire). Fixed: the startup scan now increments `open_positions` before spawning each monitor, matching the behavior of the buy path.

**Pool-cache.json unbounded growth** — After days of running, `pool-cache.json` could accumulate thousands of entries (2,367+ in one session). On persist (every 60 s), the JSON writer would serialize the full map, producing MB-sized files and slow atomic renames. Fixed: `persist_to_file` now caps at 1,000 entries, preventing multi-MB cache files over long multi-day sessions. Load is unchanged (all existing entries are still loaded at startup for cross-session dedup).

**Buy-limit gate silent at INFO log level** — The `max_buys` gate at the top of `on_new_pool` used `debug!`, making it invisible at the default INFO log level. If `buy_count` was not reset correctly after a sell-mode cycle, every pool would be silently skipped with no log output. Changed to `warn!` so the gate is always visible when active.

**Sell-mode skip message misleading** — The "press [b] on dashboard to clear" message fired for buy-limit-triggered sell mode, which actually auto-clears when all positions close. Updated to show `open_positions` count and explain the two clearing paths (auto-clear for buy_limit vs manual [b] for external triggers).

---

## What's New in v1.5.0

### Sell Reliability — Token-2022 Positions Now Visible

The sniper's `scan_existing_positions` startup scan previously only queried the legacy SPL Token program. All pump.fun mints use the **Token-2022** program (`TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb`). After a restart, every pump.fun position was invisible to the scanner, so no sell monitor was attached and positions would hold for the full 15-minute window (or until a manual drawdown sell). Fix: startup now scans both programs in sequence and merges results, so all open positions — SPL Token and Token-2022 — are picked up immediately.

### Sell Reliability — Drain Guard Raised + Retry Rounds Accelerated

**Drain threshold raised 10k → 500k lamports** — the previous 10k lamport floor passed severely drained pools that still returned zero output from the swap. The new threshold (0.0005 SOL) correctly detects near-empty pools and immediately writes a `pool_drained` loss event instead of exhausting retry rounds.

**`sell_with_retry` rounds tightened from 12s → 3s total:**

| Round | Old delay | New delay | Slippage |
|---|---|---|---|
| 1 | 0s | 0s | Normal |
| 2 | 3s | 0s | 2× |
| 3 | 3s | 1s | `min_out=0` |
| 4 | 6s | 2s | `min_out=0` |

On pump.fun rugs — which typically complete in under 3 seconds — the old schedule was reaching `min_out=0` only after the pool was already drained. The new schedule hits all four retry variants in ≤3 seconds, matching rug cadence.

**`max_sell_retries` reduced 5 → 3** — 3 inner retries × 4 outer rounds = 12 total transaction attempts per position (was 20). Each confirmation timeout costs up to 30 seconds; fewer retries on confirmed failures frees the executor faster for live positions.

### Dead-Zone Early Exit — Recycle Capital Fast

**Root cause identified from live data**: winning trades exit at +99–397% within 0.1–6 seconds. Positions that don't pump sit flat at −0.499% (the AMM spread) for 148–322 seconds with zero exit signals — none of SL, trailing stop, flash-crash, or dump detection ever fires because the price never moves.

**Fix**: new `no_pump_timeout_secs` (default 45) / `no_pump_min_gain_pct` (default 3.0) gate in the sell monitor. If the position's best price seen (peak) is below +3% after 45 seconds, exit immediately. Suppressed if any upward momentum was observed — a token that hit +3% at any point continues through the normal TP/SL/pullback/escalation path as before.

**Effect**: dead positions recycled in ~45 seconds instead of 148–322 seconds, freeing capital for the next buy. Profitable trades are unaffected (they exit in <6 seconds, long before the gate fires).

Config:
```toml
no_pump_timeout_secs = 45   # seconds before dead-zone exit fires (0 = disabled)
no_pump_min_gain_pct = 3.0  # peak gain % required to suppress the exit
```

### Momentum / Volume Scoring — PoolScorer v0.8.1

Two new signals added to the 0–100 pool score alongside existing pool-age and pool-size components:

**Velocity bonus (up to +22)** — `quote_vault_SOL / age_seconds`. High SOL-per-second inflow means buyers are piling in fast, indicating a runner candidate.

| Velocity | Bonus |
|---|---|
| ≥ 15 SOL/s | +22 — crowd piling in |
| ≥ 5 SOL/s | +14 — strong inflow |
| ≥ 1.5 SOL/s | +7 — moderate |
| ≥ 0.4 SOL/s | +2 — mild |
| < 0.4 SOL/s | 0 |

**Buy-pressure ratio bonus (up to +12)** — `quote_vault / base_vault`. On a Raydium AMM, as buyers accumulate tokens the SOL side grows and the token side shrinks, pushing the ratio above the launch baseline (~0.001). A ratio ≥ 0.5 confirms heavy buying already in progress.

| Ratio | Bonus |
|---|---|
| ≥ 0.5 | +12 — heavily bought up |
| ≥ 0.05 | +6 |
| ≥ 0.005 | +2 |
| < 0.005 | 0 |

Pools with both high velocity and a rising buy-pressure ratio now score near the ceiling, giving the `min_pool_score` gate a much sharper signal for catching runners at detection time — before the price chart shows anything.

**Detection-time freshness fallback**: Pump.fun always sets `open_time=0` meaning "open immediately", which caused the +30 ultra-fresh age bonus to never fire — all pump.fun pools scored 50/56/68 with no differentiation. The scorer now accepts a `detected_at_secs` timestamp; when `open_time=0` and the pool was detected within the last 60 seconds, the detection time is used as the effective open time. Result: fresh pump.fun pools now score 95–100 (50 base + 30 fresh + 18 sweet spot) rather than a flat 68.

**Size bands retuned** (v1.5.0): sweet spot extended from 6.5–22 → 6.5–28 SOL based on live winner data (18–28 SOL). Pools over 100 SOL now penalised: −8 for 100–400 SOL (likely established/pumped), −20 for >400 SOL.

### Log Panel — Visual Glitch Fixes

- **Compact log format** — verbose tracing timestamps (`2026-05-18T05:17:50.123456Z  INFO target::module: msg`) are reformatted to `[SNIPER] HH:MM:SS [LEVEL] message`. Log panel no longer shows raw tracing prefixes.
- **Exact byte-offset tracking** — replaced `BufReader::lines()` + approximate offset arithmetic with `read_line()` which returns the exact byte count including line terminators. Eliminates the offset drift that caused duplicate or skipped lines on re-reads after the file was partially consumed.
- **Blank line suppression** — empty lines and lines that consist only of whitespace are filtered before pushing to the display buffer.
- **Continuation markers** — lines wider than the panel are split at character boundaries with a `↪` prefix (dimmed) on each continuation chunk. Previously, ratatui's text wrapping could leave unformatted overflow.
- **Banner wrapping** — `Wrap { trim: true }` added to status banner paragraphs; previously the "NO LOG FILE" message was truncated on narrow terminals.

---

## What's New in v1.4.0

### Hold Longer — Exit at Peak, Not at First Pump

Data from live sessions showed the bot exiting at 99–198% on every launch because `take_profit_pct = 80` fired on the **first price check** (< 250 ms), before the 5-sample momentum window could fill. The escalation system never ran. Changes to let winners run:

**Base TP raised 80% → 175%** — acts as a floor for the escalation ladder, not an early exit. The pullback exit and velocity-decay exit are the primary exit signals; TP fires only if the token keeps pumping continuously with no reversal.

**Momentum escalation tuned for more aggressive riding:**

| Parameter | v1.3.x | v1.4.0 | Effect |
|---|---|---|---|
| `take_profit_pct` | 80% | **175%** | TP ladder now starts above the typical initial pump |
| `momentum_escalation_factor` | 1.6× | **1.8×** | TP target grows faster per round |
| `momentum_max_escalations` | 5 | **7** | 7-round ladder: 175→315→567→1020→1836→3305→5949% |
| `momentum_escalation_threshold_pct` | 5%/check | **3%/check** | Lower velocity bar to trigger escalation |
| `momentum_min_peak_pct` | 25% | **60%** | Pullback exit only fires after a real 60%+ peak |
| `momentum_pullback_exit_pct` | 18% base | **8% base** | Tighter adaptive pullback = higher exit PnL at every peak height |
| `velocity_decay_min_pnl_pct` | 7% | **25%** | Velocity-decay exit only fires with ≥ 25% gain (was 7%, barely above fees) |
| `velocity_decay_drop_threshold` | 1.2 | **1.5** | Less sensitive to thin-pool noise |

**Adaptive pullback improvement:** The formula `θ_eff = 8 × √(1 + peak/100)` now locks in at higher PnL than the old `18 × √(1 + peak/100)` at every realistic peak level:

| Peak | Old exit PnL | New exit PnL |
|---|---|---|
| 60% | 4.9% | **49.9%** |
| 100% | 74.5% | **88.7%** |
| 200% | 168.8% | **186.1%** |
| 500% | 450.8% | **480.4%** |

**Tiered partial-TP ladder shifted up:**

| Level | v1.3.x | v1.4.0 |
|---|---|---|
| First partial | +45%, sell 20% | **+100%, sell 15%** |
| Second partial | +100%, sell 25% | **+300%, sell 20%** |
| Third partial | +200%, sell 25% | **+600%, sell 25%** |

At 45% many positions were selling their first chunk before the initial pump leg finished. Now the first lock-in only fires once we're at 100%+, preserving upside on the full position during the run-up.

### Builder & SuperBuilder — Compounding Growth Algorithms

Both modes target specific SOL milestones using live compounding equations that recompute every 5 seconds from the current wallet balance. Size, TP, and SL all evolve autonomously as the wallet grows — no manual adjustment needed.

**Builder (1 SOL target) — Geometric Compounding:**

| Progress | Size mult | TP | SL |
|---|---|---|---|
| 0% | 1.50× | 1.5× base | 1.2× base |
| 25% | 2.26× | 1.38× base | 1.15× base |
| 50% | 2.82× | 1.25× base | 1.10× base |
| 100% | 3.50× | 1.0× base | 1.0× base |

Formula: `size = 1.5 + 2.0 × progress^0.65` · `tp = base × max(1, 1.5 − 0.5 × p)` · `sl = base × (1.2 − 0.2 × p)`

**SuperBuilder (3 SOL target) — Parabolic Compounding:**

| Progress | Size mult | TP | Notes |
|---|---|---|---|
| 0% | 2.0× | 2.0× base | Moon Chase auto-ON |
| 10% | 4.7× | 1.9× base | Moon Chase ON |
| 25% | 5.6× | 1.75× base | Moon Chase ON |
| 50% | 6.7× | 1.5× base | Moon Chase OFF |
| 100% | 8.0× (cap) | 1.0× base | Moon Chase OFF |

Formula: `size = 2.0 + 6.0 × progress^0.35` · `tp = base × max(1, 2.0 − p)` · `sl = base × 1.4` · Moon Chase auto-engages when `progress < 25%` and disengages when `progress > 60%`

The key property: **position size compounds geometrically with wallet growth**, so each winning trade funds larger subsequent positions, accelerating the path to the SOL target.

### Bug Fixes

- **PnL always showed 0.0000 SOL** — `sell_with_retry` was calling `record_trade_confirmed(0)` with a hardcoded zero. `record_sell_outcome` now calls `record_trade_confirmed(pnl_lamports)` with the real value. Session PnL display is now accurate.
- **Duplicate buys from multiple listeners** — Added `recently_bought: DashMap<Pubkey, Instant>` dedup guard. Same mint cannot be bought again within 5 minutes of a confirmed buy, regardless of how many listener sources fire for the same pool event.
- **Session heat miscounting forced exits** — Sell-mode forced exits (`-0.499%` AMM spread) were being counted as losses, triggering 15-min buy pauses. `session_heat_losses` set to 0 (disabled). The drawdown guard is the correct circuit breaker.
- **Drawdown baseline never reset** — After a drawdown recovery, `session_start_lamports` was stale so the guard re-tripped immediately on the next buy. Baseline now resets to current wallet balance on recovery.
- **Desktop notifications minimizing TUI window** — `send_desktop` used `-WindowStyle Hidden` which creates a console handle first and briefly steals foreground. Fixed with `CREATE_NO_WINDOW` Win32 flag — no window ever created, TUI stays focused.
- **Profit-first rug floor tightened** — `profit_first_floor_pct` 50% → 25%. Bot now exits rugged tokens at -25% instead of holding to -50%.

---

## What's New in v1.3.0

### Multi-Position Trading — Unlimited Concurrent Positions

- **Lock architecture rewrite** — the `ProcessingSlot` buy lock previously handed off to the sell monitor on buy confirmation, blocking all new buys for the entire monitoring window (up to 90 minutes per position). The lock now releases immediately after the buy transaction confirms (~2 s), so the next qualifying pool can be sniped without waiting for any open position to close.
- **Unlimited concurrent positions** (`max_concurrent_positions = 0`) — the bot buys into every qualifying pool without a cap. The only serialisation is a brief buy-tx lock preventing two purchases from racing on the same WSOL ATA simultaneously.
- **Parallel sell execution** — sell semaphore raised from 1 → 5 concurrent sell transactions. With many open positions all hitting stop-loss simultaneously (e.g. dump mode), exits now run 5-at-a-time rather than fully serialised, cutting mass-exit latency from minutes to seconds.
- **Accurate metrics** — `record_trade_attempt()` was being called before the lock check, inflating the "failed buy" counter with pools that were correctly skipped (never actually attempted). Counter now increments only after the lock is secured and a real transaction is submitted.

### Position Display — Real-Time Stats

New columns replace the static Entry SOL field:

| Column | Description |
|---|---|
| **SL%** | Live stop-loss floor as % from entry. Updates every 250 ms with trailing stop, profit-lock, and tiered-TP adjustments. Green = above breakeven, yellow = small loss floor, red = deep loss floor. |
| **Progress** | 8-char `░░░███░░` bar: left edge = SL, right edge = TP, fill = current position. Instantly shows how close to exit in either direction. |
| **Status** | Adds `▼ ` prefix on 3-tick decline streak, `▼▼ ` on 5-tick (rug warning), color-coded red. |

The `LivePositionSnapshot` now carries `current_sl_lamports`, `current_sl_pct`, and `decline_streak` — all flushed to the positions file every price-check tick.

---

## What's New in v1.2.0

### Execution — 100% Sell Rate

- **Pool drain detection** — `sell_with_retry` and `do_sell` now pre-check the quote vault balance before building swap instructions. If `< 10,000 lamports`, the pool is considered drained: a `pool_drained` total-loss SELL event is written immediately and the processing lock is freed. Previously, drained-pool positions would exhaust all 4 retry rounds × 5 executor retries (~20 doomed transactions) before unlocking — blocking all future buys for 30+ seconds.

### AI — Rate-Limit Cache + Provider Fallback

- **Groq 429 blackout cache** — parses the `"Please try again in Xs"` delay from 429 responses and stores it in a module-level atomic. Subsequent pool evaluations skip the HTTP call entirely during the blackout window, eliminating per-pool latency during rate-limited periods.
- **Automatic fallback provider** — when the primary AI provider (Groq/xAI) is rate-limited, the system transparently retries with OpenRouter or local Ollama (whichever key is set in `.env`). No interruption to pool scoring.

### Profit Margins

- TP raised 50% → 80%; momentum escalation factor 1.5× → 1.6×; max escalations 4 → 5 (up to ~10.5× TP target)
- Tiered partial-TP levels shifted up: 30/75/150% → 45/100/200% — hold longer before each partial
- Trailing stop tightened 15% → 12% from peak; profit-lock engages after 6 checks (was 8)
- Pullback exit requires 25% peak gain (was 20%) before firing; allows 18% pullback (was 15%)

### Exit Strategy

- **Whale exit trigger** (`whale_exit_vault_drop_pct = 22%`) — exits immediately on a single-tick vault drop ≥ 22%; fires faster than the 3-consecutive-decline detector for vertical rugs.
- **Volume exhaustion exit** (`volume_exhaustion_pct = 65%`) — when in profit and the quote vault has shrunk > 65% from entry level, exits before the remaining liquidity evaporates.

### Pool Quality

- `min_pool_size` raised 1 → 2 SOL — sub-2 SOL pools drain within the first 500ms of bot traffic
- `check_name` enabled — zero-cost scam-word filter on token name/symbol
- `max_deployer_rugs_24h` tightened 3 → 2 — blocks repeat ruggers sooner
- `min_pool_score` raised 25 → 35 — combined with 2 SOL floor, focuses on pools with both fresh age and sufficient liquidity

---

## What's New in v1.1.0

### Neural Network — Dueling DQN + N-Step Returns

- **Dueling DQN architecture** — shared trunk splits into a value head V(s) and an advantage head A(s,a). Q(s,a) = V(s) + A(s,a) − mean(A). Reduces Q-overestimation, improves policy stability on rare actions. Old checkpoints load cleanly (standard mode via `#[serde(default)]`).
- **N-step returns (n=5)** — bootstraps rewards across 5 steps: G_t = r_t + γ·r_{t+1} + … + γ⁴·r_{t+4}. Propagates long-horizon credit more accurately than single-step TD.
- **Expanded state space: 18 → 24 features** — 6 new signals: `peak_pnl_pct` (how far off peak), `pool_score_norm` (quality signal), `deployer_rug_rate` (reputation), `volume_velocity` (volume trend), `price_velocity` (momentum), `price_acceleration` (momentum second derivative).
- **Checkpoint versioning** — saves `state_dim` / `action_dim` at write time; resets gracefully on shape mismatch instead of panicking.
- **Action rebalancing** — injects synthetic Hold + SellPartial transitions every 50 train steps to prevent SellAll collapse in the replay buffer.
- **Tournament hyperparameter evolution** — losing tournament variants mutate ±20% lr, ±0.005 epsilon_decay, ±0.005 gamma. Winners are kept intact.
- **NN gating into the buy path** — when `epsilon < 0.3` (agent confident): BuyAgg → 1.5× position, Hold → 0.5× position, SellPartial/SellAll → skip buy entirely.

### Reward Function Redesign

Super-linear profit scaling: `R = pnl × (1 + log₂(1 + pnl/25))` — bigger winners earn disproportionately larger rewards, teaching the agent to let runners run. Fast-exit timing bonuses (+75 for immediate, +30 for ≤3 steps). Rug mercy clause for unavoidable holds (flat penalty reduced when `hold_steps == 0`). Expected value at 30% win rate: +16 (was −192).

### Sniper — Buy Improvements

- **Pool quality sizing** (`pool_quality_sizing = true`) — multiplies position by `pool_score / 100` so high-conviction entries get full size and sketchy pools get scaled down automatically.
- **Absolute SOL floor** (`min_sol_reserve`) — wallet must keep at least this much SOL after any buy. Prevents getting trapped with no gas money.
- **Confirmation window** (`confirmation_window_ms`) — waits N ms after pool detection, then checks if the vault has already been drained >15%. Skips the buy if early bots already pumped it.
- **Session heat cooldown** — tracks loss timestamps in a rolling window; if `session_heat_losses` losses occur within `session_heat_window_secs`, buying pauses for `session_heat_cooldown_mins`. Automatic recovery when the window clears.

### Sniper — Sell Monitor Improvements

- **Volume exhaustion exit** (`volume_exhaustion_pct`) — exits a profitable position when quote vault volume drops below this percentage of the entry-time volume. Catches the "volume dries up before price crashes" pattern.
- **Whale exit detector** (`whale_exit_vault_drop_pct`) — fires an immediate sell if the quote vault drops more than this percentage in a single check. Catches large wallet exits before the cascade.
- **Check interval acceleration** (`check_interval_acceleration = true`) — halves the polling interval (floor 25ms) when 3 consecutive declining price checks are detected. Faster reaction without burning RPC on stable positions.

### Filter Pipeline

- **DeployerWalletAgeFilter** — rejects pools from wallets younger than `deployer_min_age_hours`. Uses `getSignaturesForAddress` on the base mint as a cost-efficient proxy for wallet creation time.
- **Filter TTL cache** — caches pass/fail results per pool pubkey for `filter_cache_ttl_secs` (default 30s). Eliminates redundant RPC calls when duplicate events arrive for the same pool.
- **Cost-ordered pipeline** — filters run cheapest first: in-memory blacklist → freeze → mint renounce → LP burn → pool size → liquidity depth → name → volume → cross-pool → deployer age → holder concentration → liquidity momentum → Jupiter. Expensive filters only see pools that passed cheap guards.
- **RPC error categorization** — `multi_rpc.rs` now classifies errors into `RateLimited` (backoff), `NodeBehind` / `NetworkTimeout` (failover), `AccountNotFound` (ignore), `Other` (log). No more blunt failover on 429s.

### Kelly Sizing

- **Warm-up guard** — returns 0.5× multiplier until at least `kelly_min_trades` trades are recorded. Prevents Kelly from sizing huge on a 1-trade sample.

### Infrastructure

- **Trade log rotation** — archives `scematica-trades.jsonl` to a timestamped backup when it exceeds 10,000 lines. Keeps the NN observer and dashboard fast on long sessions.
- **Arb gas-adjusted minimum profit** — minimum profit threshold is now `max(config_min, tx_fee × 3)`. Never executes an arb whose profit doesn't cover fees by 3×.
- **Arb stale quote detection** — `ArbPath` records `fetched_at_ms`; execution is skipped if more than 800ms (≈2 Solana slots) have elapsed since reserve fetch. Avoids negative-profit reverts on stale data.

### Dashboard

- **NN Q-value bar chart** — the Deep Q* panel now renders a per-action Q-value bar underneath the stats table. The highest-Q action is highlighted green so you can see at a glance what the agent thinks about the current market.
- **Alert history panel** — rolling last 5 confirmed BUY/SELL events displayed in the Overview tab. No more digging through logs to see what just happened.

---

## What's New in v1.0.0

- **WSOL ATA lifecycle hardened** — idempotent create, transfer, SyncNative before every buy. Sell-side close_account fire-and-forget reclaims ~0.002 SOL rent per position.
- **Multi-phase sell monitor** — fast phase (30 checks × 75ms) for dump detection, normal phase (configurable interval, floor 250ms). Both balance reads happen in parallel via `tokio::join!`.
- **Flash-crash detector** — single-check drop ≥ `flash_crash_pct` from entry triggers emergency exit before the 3-decline counter even accumulates.
- **Tiered partial-TP ladder** — up to N levels, each selling `sell_pct` of remaining balance at `trigger_pct` gain. Stop moves to breakeven after tier 1 fires.
- **Profit-lock** — after `profit_lock_checks` consecutive checks above entry, SL floor raises to near-breakeven (entry × 0.98) permanently.
- **Velocity-decay exit** — compares recent vs. previous half of a rolling velocity window; exits when upward momentum is measurably dying but price hasn't reversed yet.
- **Adaptive pullback exit** — pullback threshold scales with peak gain (`θ_eff = base × √(1 + peak/100)`). Big winners get more room to breathe before exiting.
- **Moon Chase mode** (`[m]` key) — swaps momentum-hold parameters to an aggressive "parabolic outlier" preset (8 escalations, 1.75× factor, 25% pullback, 3%/check threshold).
- **Live position registry** — `scematica-positions.json` flushed every second; dashboard Positions tab shows current value, peak, dynamic TP, escalations, and staleness indicator.
- **Process manager** — dashboard spawns and monitors the sniper as a child process; restarts automatically on crash.
- **Session stats** — best/worst trade, win/loss streak, PnL sparkline.

---

## What's New in v0.8.0

- Loss cooldown removed — streak tracking retained for display only
- Evaluation criteria tightened ~30%: PoolScorer bands narrowed, filter defaults stricter
- `min_pool_score` default 0 → 45 — scorer now actually gates buys

## What's New in v0.7.0

- Profit-first growth doctrine with `profit_first_mode`
- Builder mode ladder: Growth / Builder / SuperBuilder (progressive rate scaling)
- Sharper pool scorer with freshness gradient and size sweet-spot

## What's New in v0.6.0

- Expanded rate-mode ladder: Bearish → Micro → Safe → Balanced → Aggressive → Degen → Bullish
- NN observer actually trains (fixed field name mismatch, added `pnl_pct`/`position_age_secs`)

## What's New in v0.5.0

36 features including Kelly sizing, Pool Scorer, Pump.fun monitor, Multi-RPC failover, regime-aware NN branching, adversarial scenario injection, multi-agent tournament, backtesting engine.
