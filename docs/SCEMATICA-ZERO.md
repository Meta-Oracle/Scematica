# Scematica Zero

**Zero install, zero local API, zero custody.** The Scematica loop — perceive, score,
decide, gate, execute, seal — running entirely inside a browser tab against real Solana
mainnet, with no Rust process anywhere and no key on our servers.

Status: **phases 0-2 built.** The pure core (`web/lib/zero/`), the browser host
(`web/lib/zero/host/`), the `/zero` page, the fill read and the funding flow all exist,
with 205 checks in `npm run check:zero`. What remains is an armed run against mainnet
and phase 3, the extension.

This document was written before any of it, so the load-bearing decisions were argued
once rather than made accidentally by an implementation. Where building changed an
answer, the change is recorded here rather than left in a commit message.

---

## 1. The one thing Zero is not

**Zero is not the sniper, and must never be presented as one.**

This is not modesty, it is arithmetic. The sniper's edge on a new Raydium pool is
sub-second, and it earns that with a colocated keyed WebSocket, a locally-held keypair
that signs with no prompt, and a hand-built Raydium instruction. Zero has a browser event
loop, an RPC endpoint somewhere on the internet, and Jupiter's router in the path. It will
lose the first-block race every single time.

Shipping it as "the sniper, in your browser" would be simulated performance wearing a live
badge — the precise failure `X-Scematica-Source: simulation` and the permanent SIMULATION
banner exist to prevent. The repo already wrote this down once, in `web/lib/swap.ts`:

> ⚠️ THIS IS NOT SNIPING. A wallet prompt puts a human in the loop, which costs seconds;
> the bot's edge on a new pool is sub-second.

Zero's edge is **selectivity and provable discipline**, not speed. It can decline. It can
size. It can exit on rules. And — uniquely in this codebase — it can *prove what it did*,
including the trades it refused to take.

### What Zero is

**The sixth `scema.world/1` producer, and the first one that can act.**

Five producers emit a `WorldState` today (`RepoObserver`, the browser extension,
`scematica_mesh::omni`, `alchem_link.omni`, `lib/scemaworld/github.ts`). All five observe.
Zero observes a Solana market the same way, runs the same loop over it, and then **spends
money on the result and seals a record of having done so**. `lib/omni/canonical.ts`
already produces byte-identical commitments to Rust in the browser, and `/omni` already
verifies a record with no server in the path at all. Zero is the first product here where
the decision record and the money are the same event.

That is the product. Not "a bot you don't have to install" — *a bot whose every decision
is checkable by someone who was not there.*

---

## 2. The four settled decisions

| # | Decision | Chosen |
|---|---|---|
| 1 | Signing | Attended (wallet prompt) by default; **capped session key** as an explicit opt-in |
| 2 | Data plane | **BYO RPC key**, stored in the browser only, never sent to our server |
| 3 | Surface | **Page first** (`/zero`), extension second, one shared pure core |
| 4 | v1 scope | **Full loop** — post-launch entries, event-driven exits |

Each is argued below, with its cost stated. A decision whose cost is not written down gets
reversed by the first person who meets that cost and thinks it is a bug.

### 2.1 Signing — two modes, and the second one is armed deliberately

**Attended** is the default and needs no new trust: Jupiter builds the route, the user's
own wallet signs, no key ever exists outside the wallet. `lib/swap.ts` already does this.
Its cost is that Zero cannot act while you are not looking, which means it is a decision
engine rather than a bot.

**The capped session key** is what makes autonomy possible. An Ed25519 keypair generated
in the browser, funded by the user with a bounded amount, signing without a prompt. It is a
hot key in a browser tab and there is no way to pretend otherwise — so the design does not
pretend, it **bounds**:

- a **hard cap** on what may sit in it, checked before every top-up;
- an **expiry**, after which it signs nothing and must be re-armed;
- a **spend ledger** — per-trade, per-hour, per-session — enforced before signing;
- a **one-click sweep** returning everything to the funding wallet;
- and all of it **on screen**, permanently.

The precedent is exact. `web/lib/scemaworld/claim.ts` faced the same problem — a balance
that lives in a browser tab is trivially forgeable — and answered it the same way:

> the caps buy bounded loss, not secrecy, and they are on screen because a cap the player
> cannot see reads as a broken button.

**Cost, stated plainly:** a session key is malware-reachable and XSS-reachable in a way a
hardware wallet is not. The mitigation is the cap, not the storage. The UI must say *"treat
this balance as the maximum you can lose"* in those words, and the funding step must
default to a small number rather than to the wallet's balance.

### 2.2 Data plane — the user brings the key

`NEXT_PUBLIC_ESCROW_PROGRAM_ID` is public by nature; `NEXT_PUBLIC_RPC_ENDPOINT` is
**deliberately unset in this repo**, and `web/lib/escrow/rpc.ts` throws if imported into a
browser bundle, because anything with that prefix is served to every visitor. So there is
no keyed endpoint Zero may ship with.

The user pastes their own Helius / Triton / QuickNode key. It is stored in browser storage
and **never** transmitted to our server, never written into a decision record, and never
included in an error string. Their rate limit is their own; there is no shared key to
exhaust or to leak.

**Cost:** a setup step stands between a visitor and anything real, and Zero is unusable
without it. That is accepted. The alternative — proxying through our own Vercel function —
would reintroduce a server we pay for and rate-limit across all users, and would reduce the
headline claim from "needs no API" to "needs no *local* API", which is a much weaker
sentence.

The public cluster endpoint stays available for read-only browsing, and **arming execution
requires a key**. A degraded read-only mode must be labelled as such and must never be the
mode a first-time visitor judges the product by.

### 2.3 Surface — page first, but the core is shell-agnostic from day one

`/zero` ships first: linkable, demoable, no store review.

The extension is phase 2 and its job is precisely to remove the limitation §3 describes —
an MV3 **offscreen document** can hold a persistent WebSocket that a page cannot. That is a
hosting difference, not a logic difference, so the core must be pure from the first commit
or the extension becomes a rewrite.

`web/lib/zero/` therefore contains **no DOM, no React, no `chrome.*`, and no `window`**. It
is a state machine driven by injected effects, exactly as `scema-tui`, `scema-omnid` and
`scema-mcp` all drive one `scema-agent` and none of them re-implements the loop. Two
implementations of an entry rule would drift, and the drifted one would spend money.

### 2.4 Scope — the race Zero skips

Zero enters **after** launch, on pools its scorer rates, and exits on rules evaluated from
account-change events. It does not attempt the first block, because it would lose and the
loss would be invisible in a backtest.

---

## 3. The liveness model — the trap that decides the architecture

**A background tab cannot be trusted to run a timer.** Chrome throttles `setInterval` in
hidden tabs to roughly once per minute, and tightens further after five minutes
("intensive throttling"). A sell monitor built on `setInterval` does not fail loudly when
the user switches tabs — it *keeps holding a position and stops checking it*.

This is the single most dangerous property of a browser bot, and it produces the worst
failure this product can have: a stop-loss that did not fire because nobody was looking at
the tab.

**Two facts make a correct design possible:**

1. **WebSocket message handlers are not timer-throttled.** An `accountSubscribe`
   notification delivered to a hidden tab still runs its handler. Arrival-driven code
   keeps working where polling code does not.
2. **The Page Visibility API tells us when we are in that state**, so the condition is
   detectable and therefore reportable.

Hence:

> **Z-1. No timer may decide money.** Every evaluation that can open, size, or close a
> position is driven by the arrival of a chain event. Timers may refresh cosmetics and
> nothing else.

Concretely: subscribe to the pool's base and quote vaults; recompute price, PnL, peak and
the exit ladder **on notification**. Take-profit, stop-loss, the trailing/pullback exit and
the dead-pool timeout all become predicates over arriving state rather than a polling loop.
This is better engineering regardless of the browser, which is a good sign — the constraint
pushed toward the right design rather than toward a workaround.

**Where a timer is genuinely unavoidable** — a time-based exit such as "no pump within 30
seconds" — the rule must be evaluated on the *next arrival* with the elapsed time read from
the clock, never assumed to have fired on schedule. A position that has gone quiet has not
satisfied its exit; it has an *unevaluated* one.

> **Z-2. A socket that has gone quiet is DEGRADED, and Zero says so.** An exit rule that
> could not be evaluated is not an exit rule that said "hold".

This is `coherence.rs` transplanted, and Zero needs it *more* than the Rust bot does: in a
browser, RPC-bound reads fail under a rate limit far more often than on a server. Past a
threshold of unresolved checks, Zero halts **entries** and says why — never exits, because
a degraded feed must never stop you closing existing risk.

**The audio mitigation, and its cost.** A tab playing audio is exempted from intensive
throttling. Playing a silent loop is a real and widely used technique, and it is worth
offering — but it puts a speaker icon on the tab, may be blocked before a user gesture, and
is a browser behaviour rather than a guarantee. It is a *mitigation offered with its cost
stated*, never the thing correctness rests on. Correctness rests on Z-1.

---

## 4. Invariants

These are the rules that make this Scematica rather than a generic browser bot. Each names
the failure it prevents, because a rule without its reason gets optimised away.

- **Z-1. No timer may decide money.** §3. Background tabs throttle to ~1/min, silently,
  while holding a position.
- **Z-2. Quiet is degraded, and degraded is visible.** §3. Halts entries, never exits.
- **Z-3. The session key's cap is on screen, always.** Bounded loss, not secrecy. A cap the
  user cannot see reads as a broken button (`claim.ts`).
- **Z-4. The RPC key never leaves the browser.** Not to our server, not into a record, not
  into a log line, not into an error message. Same rule as `lib/escrow/rpc.ts` and
  `lib/alchem/endpoint.ts`, one layer further out.
- **Z-5. Zero has no simulation branch.** It carries its own source tag and never borrows
  `lib/sim/`. A figure it could not read renders as "could not read", never as a zero —
  "no liquidity" and "could not measure liquidity" are different claims and only one is a
  reason not to buy.
- **Z-6. Every decision seals a record, including the declines.** A decline has no outcome
  and never will, so its error is `None` and never `0.0` — imputing one lets a policy
  improve its score by refusing to act (`calibration.rs`, `scema-policy`).
- **Z-7. Unmeasured features take the neutral element, not zero.** The `FeatureMask`
  discipline from `state.rs` carries over and matters *more* here, because a browser can
  measure fewer of the 24 features than the bot can. `coverage()` rides beside every
  Q-value: five finite Q-values with a clear argmax look identical whether the input was
  measured or invented.
- **Z-8. Sent-but-unobserved is a third outcome.** Never `sendAndConfirmTransaction` — poll
  `getSignatureStatuses`. A fill Zero cannot observe is `Unknown`: the position is recorded
  as *possibly open*, the UI says so, and nothing retries automatically. The treasury path
  already paid for this one; a successful payout presenting as a dead faucet is the worst
  pair of facts available, and the obvious retry pays twice.
- **Z-9. The scorer stays a port.** `pool_scorer.rs` remains authoritative;
  `npm run check:parity` still pins it. Zero raises the stakes — the port now spends money
  rather than previewing — so promoting an `approx` filter to `port` requires the Rust
  input to actually exist.
- **Z-10. The kill switch is local and unconditional.** One control that halts entries and
  disarms the session key, implemented as a browser-local fact requiring no network call to
  take effect. A stop that needs the network is not a stop.
- **Z-11. Zero never claims a P&L it did not settle.** Realised P&L comes from observed
  swap output, not from a quote (`exit_strategy_v180`). An open position's mark is labelled
  as a mark.

---

## 5. Architecture

```
web/lib/zero/                 pure core — no DOM, no React, no chrome.*, no window
  types.ts        Position, ZeroEvent, Verdict, Term; ZeroConfig aliases SniperConfig
  session.ts      keypair, caps, expiry, spend ledger, sweep
  observe.ts      fill confirmation by getSignatureStatuses; Unknown is an arm
  seal.ts         DecisionRecord via lib/omni/canonical.ts
  policy.ts       the pinned DQ* checkpoint. Never trains.
  lease.ts        Web Locks single-writer election
  gatekeep.ts     the $SCEMA default (not a boundary)
  readout.ts      what Zero shows about itself
  funding.ts      moving SOL into and out of the session key
  engine.ts       the loop: pure reducer over ZeroEvent, effects injected

web/lib/zero/sniper/          THE BOT'S OWN LOGIC — ports, pinned against Rust
  config.ts       config.toml's [sniper] section + the 7 rate modes
  psi.ts          the scematica-sentience master equation slice coherence.rs uses
  coherence.ts    the epistemic breaker (coherence.rs)
  kelly.ts        KellySizer (kelly.rs)
  exit-ladder.ts  the sell monitor's ladder (the loop in sniper.rs)
  fixtures/sniper-parity.json   emitted by the sniper; see §5.1

web/lib/zero/host/            browser effects: WebSocket, RPC, wallet, signing

web/app/zero/                 the page shell (phase 1)
plugins/scema-zero/           the extension shell (phase 2, offscreen document)
```

### 5.1 There is no Zero strategy, and that is the point

Zero decides what the sniper decides. Not "something similar" — the same thresholds, the
same ladders, the same arithmetic. Anything else makes it a second bot wearing the first
one's name, and the first thing such a bot produces is a decision record nobody can check
against the system it claims to mirror.

That was not true for the first pass, and none of it failed:

| | Zero was | the sniper is |
|---|---|---|
| take profit | 100% | **175%** |
| stop loss | 15% | **10%** (25% under profit-first mode) |
| pullback give-back | 25 pts | **15 pts** |
| momentum peak floor | 140% | **200%** |
| concurrent positions | 3 | **1** |
| no-pump rule | ±5% after 30s | **peak under 8% after 15s** |
| Kelly | half, min 8 trades, a fraction of base | **quarter, min 10, a multiplier in [0.25, 3.0]** |
| Ψ | the resolution rate | **the master equation, max ≈ 0.2055** |
| pool score | a local depth heuristic | **the Bayesian LR ladder** |
| entry strategies | three invented ones | **there are none** — filters plus the score |
| exit rules | four | **eleven**, with a ratcheting stop and an escalating target |

Every row is a different bot. The Ψ row is the sharpest: Zero's Ψ was a *different
quantity* on a different scale, so a threshold copied across meant nothing, and the two
gates opened in different places for reasons no reader could see.

So the numbers now come from the bot, mechanically:

```
config.toml  ──▶  cargo test -p scematica-sniper zero_parity
                        │  emits
                        ▼
              lib/zero/fixtures/sniper-parity.json
                        │  checked against
                        ▼
              lib/zero/sniper/*.ts   ──▶  npm run check:zero
```

`crates/scematica-sniper/src/zero_parity.rs` builds the fixture by *calling* the sniper's
own code — `KellySizer::compute_multiplier`, `coherence::assess`, `PoolScorer::score`, the
`SniperConfig` loaded from `config.toml` — rather than restating it. Editing a threshold in
`config.toml` fails `cargo test` until the fixture is regenerated
(`ZERO_PARITY_WRITE=1 cargo test -p scematica-sniper zero_parity`), and then fails
`check:zero` until the port follows. **There is no third place to put a number.**

The comparison is exact: 72 Ψ cases, 12 Kelly cases and 32 pool-score cases all reproduce
Rust bit for bit. Two things make that possible and both are places the obvious code
diverges silently — the multiplication order in `psi.ts` is Rust's left-to-right order
(float multiplication is not associative), and `Bounded::new`'s clamp is applied at every
intermediate rather than once at the end (which agrees on healthy inputs and differs
exactly where a term overflows, i.e. in the degraded case the gate exists for).

**What is *not* pinned, stated rather than glossed:** the exit ladder's control flow. The
Rust original is four hundred lines inline in an async loop in `sniper.rs`, and extracting
it into a callable function would be a large diff in a live buy-and-sell path — which is
how a trading bug gets introduced by a diagnostic. So `exit-ladder.ts` is a hand port, and
what the fixture buys is that every threshold it branches on is checked: a rule can be
wrong about *order*, never about *where it fires*. `check:zero` asserts the order
separately, which is a weaker guarantee than a fixture and is labelled as one. Same posture
as `scema-lean`'s README on the gap between a model and the code it models.

**`engine.ts` is a reducer.** `(state, ZeroEvent) -> (state, Effect[])`. It performs no I/O,
so the entire decision path is testable with no network, no wallet and no browser — which is
the only way `check:zero` can pin behaviour that would otherwise need real money to
exercise. Everything reused is reused rather than re-derived: `lib/feed/scorer.ts`,
`lib/sim/dqstar.ts` (the `DuelingNet` and `DQStarAgent` are already here), `lib/swap.ts`,
`lib/omni/canonical.ts`.

Note the one thing deliberately *not* reused: `lib/sim/engine.ts`. Zero must have no path
by which a simulated figure reaches a decision (Z-5).

---

## 6. Phases

**Phase 0 — the core, no money. DONE.** `lib/zero/` reducer, types, gate, decide, seal.
Z-1 is pinned *behaviourally* as well as by source scan: 2000 ticks fed to the reducer in
every reachable state — including one holding a position past its stop, its target, its
pullback and its timeout — produce no swap, and the same position exits on the first real
chain arrival.

**Phase 1 — `/zero`. DONE.** BYO endpoint, `accountSubscribe` on both vaults, the Raydium
AMM V4 layout read rather than assumed, Jupiter routing, `getSignatureStatuses` polling
with `Unknown` as a real outcome, the Web Locks election, and the readout. Two things
building it changed:

- **The layout orientation is read, not assumed.** Raydium does not guarantee which leg
  is SOL, and assuming inverts the price on half of all pools — which is not obviously
  wrong to look at, it just makes every exit rule fire backwards.
- **The pure decoding moved out of the action layer** (`host/parse.ts`), because
  `lib/swap.ts` uses a TypeScript parameter property that Node's strip-only loader
  refuses, so anything importing it is untestable. The layout offsets are exactly the
  code that must be tested — a wrong offset yields a valid-looking pubkey and does not
  throw — so they belong on the pure side.

**Phase 2 — the session key. BUILT.** Caps, ledger, expiry, arming, kill switch, funding
and sweep. Two rules the funding flow rests on:

- **The cap is on the resulting balance, not on the transfer**, so repeated top-ups cannot
  walk past it one increment at a time — the same shape as the spend ledger's `committed`.
- **A sweep's destination is not an input.** It is read from the wallet that funded the
  key. A sweep that took an address would be a one-click drain of the hot key to anywhere,
  reachable by anything that can run in the page — the same threat model the key lives
  under, so it would hand over the whole balance rather than the bounded slice the caps
  exist to expose. A sweep is also refused while positions are open: taking the SOL out
  leaves them owned by a key with nothing to pay a sell fee with.

The preview refuses exactly where the payer refuses. `fundSession` will not fund against
a balance it could not read, so the panel does not default that to zero either — the
escrow path already paid for the other version, where `quote` swallowed a failed ledger
read and priced a claim against an empty ledger while `settle` refused the same request.

What is still **not** done is an armed end-to-end run against mainnet. Nothing here
should be trusted with money until that pass happens.

The kill switch deliberately does **not** liquidate. A control that dumps at market is a
different and far more dangerous thing, and conflating the two means nobody can stop new
entries without also being forced to sell. It halts entries and destroys the key
material — so "disarmed" means the same thing to a reader of `localStorage` as it does to
the UI.

**Phase 3 — the extension.** Same core, offscreen document, persistent socket, cross-site
perception on pump.fun / DexScreener / Raydium. Lifts the §3 limitation rather than working
around it.

**Phase 4 — the record surface.** Zero's sealed records become a public, verifiable track
record at `/omni` — the calibration story, with declines counted rather than scored.

---

## 7. Open — the features still to form

Deliberately unanswered, for the next pass:

All six are answered and built. What they settled:

1. **Strategy set** — *withdrawn*. The first pass invented three (`scored-entry`,
   `pullback`, `continuation`), each with its own thresholds, on the reasoning that Zero's
   latency tolerance called for entries the sniper does not have. It was the wrong call and
   §5.1 is why: the sniper's entry is the filter pipeline plus the pool score, and adding a
   parallel one made Zero a different bot at exactly the point where a decision record is
   supposed to be checkable. The entry gate is now the score floor, the depth band and the
   breakers, from `config.toml`.
2. **Exit ladder** — the sniper's, ported whole (`sniper/exit-ladder.ts`): the exit gate,
   the profit floor, the ratcheting trailing stop, take-profit escalation, the adaptive
   pullback, velocity decay, tiered partials, flash crash, the vault rules, the no-pump
   timeout, the dump detector and the hold cap. Eleven rules where the first pass had four,
   and the four it had used different numbers. Order is load-bearing in three places, each
   asserted in `check:zero`: the peak updates before the trailing stop reads it; the exit
   gate is computed once from the raw PnL at the top; and take-profit/stop-loss is
   evaluated **last**, against the stop as every rule above has left it — putting the
   protective rule first reads as obviously correct and gives back exactly the gain the
   ratchet exists to keep.
3. **Does Zero train?** No. Pinned checkpoint, transitions logged, training nowhere
   (`policy.ts`). A per-tab policy is one nobody can reproduce, and a record citing
   weights that exist nowhere destroys the only thing Zero is for. `NEUTRAL` and the
   feature order are read out of `state.rs` at check time.
4. **Multi-tab** — Web Locks single-writer election (`lease.ts`). A `localStorage` flag is
   not a lock and a crashed leader wedges it forever; the browser releases a Web Lock when
   the tab dies. A **missing** API is not treated as leadership, since that would let every
   tab trade against one budget.
5. **What Zero shows about itself** — `readout.ts`. An unmeasured gauge draws a dashed full
   sweep and an em dash; a measured zero draws nothing and prints `0.00`. The headline
   answers the only question whose wrong answer costs money silently.
6. **Token gate** — reading and attended swaps ungated, arming gated (`gatekeep.ts`), and
   the file says plainly that a client-side check with no server behind it is a **default,
   not a boundary**. An unreadable balance is `unknown`, never `insufficient`.

**The fill read** (`host/fills.ts`) closed the gap that made Zero safe-but-useless armed.
It reads `pre/postTokenBalances` from the transaction rather than taking a balance delta,
because a delta taken around one signature absorbs anything else that landed meanwhile and
Zero may hold several positions at once. Three outcomes stay distinguishable and each
costs money if collapsed into another:

| Reading | Meaning | What Zero does |
|---|---|---|
| `null` | landed, output unreadable | position is `unknown`; no entry price; **no exit rule can fire** |
| `0` | landed, produced nothing | a measured, real, terrible fill |
| `n > 0` | a fill | an entry price, and every exit percentage after it |

A first buy has no PRE entry because the account did not exist — that is a genuine zero,
not an unknown, and getting it backwards makes every first purchase unpriceable. An amount
past `MAX_SAFE_INTEGER` is refused rather than rounded, because a wrong `tokensOut` is a
wrong entry price. A sell is measured in lamports **net of the fee**: the fee is money that
left on that trade, and adding it back reports proceeds nobody received.

What is genuinely still open:

- **An armed end-to-end run against mainnet.** Everything below the money is tested; the
  money is not.
- **The SCEMA balance read** behind the token gate — `evaluateGate` is wired to `null`,
  which correctly reports `unknown` and fails closed on arming.
- **The extension** (phase 3), the only thing that lifts §3's limitation rather than
  reporting it.
