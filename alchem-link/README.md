# Alchem-Link v0.23.24

An Alchemy × Chainlink developer toolkit that reads chains instead of documentation.
Live oracle reads, a consumer-safety audit, measured feed behaviour, and a simulator that
replays your guards against the failure modes that have already cost people money — from
the command line, a full-screen dashboard, or Python, with **no dependencies at all**.

Not "no dependencies except the UI". The terminal system is in the package: screen
diffing, colour-depth negotiation, input parsing, widgets and the event loop all live in
`alchem_link.term`. `pip install alchem-link` pulls nothing, and that now includes the
dashboard.

```bash
pip install alchem-link
alchem-link price ETH/USD
```

```
ETH / USD  (ethereum)
  price      1,929.60
  status     FRESH
  updated    18m 7s ago (heartbeat 1h)
  round      129127208515966893596  (phase 7)
  aggregator 0x5f4eC3Df9cbd43714FE2740f5E3616155c5b8419
```

No API key needed to start. Set `ALCHEMY_API_KEY` when you want real rate limits.

## The problem this exists for

`latestRoundData()` succeeding tells you almost nothing. It succeeds when the feed has
not published in a day, when the answer is pinned against a circuit breaker, when the
round is a carried-over duplicate, and — on an L2 — when the sequencer has been down for
an hour and the price is a fossil. **Every one of those returns a well-formed number and
raises nothing.** Each has cost real protocols real money.

`alchem-link audit` runs the checks a careful consumer contract would:

```bash
alchem-link audit ETH/USD -n base
```

```
ETH / USD  (base)  1,930.24   [ok]
  0x71041dddad3595F9CEd3DcCFBe3D1F4b0a16Bb70
  [info] BOUNDS_WIDE: circuit breakers are far from the current price
         the price would have to fall 1.93e+11x to reach minAnswer
  [info] SEQUENCER_OK: L2 sequencer is up and past its grace period
         up for 3619518s, past the 3600s grace period
```

| Check | Catches |
|---|---|
| `STALE` | Fresh-looking answer that has not published within its heartbeat |
| `NON_POSITIVE` | Zero or negative answers, which are never real quotes |
| `INCOMPLETE_ROUND` | `updatedAt == 0` — a round started but never finalised |
| `CARRIED_ROUND` | `answeredInRound < roundId` — no fresh answer this round |
| `BOUNDED_ANSWER` | `minAnswer`/`maxAnswer` near the price — the **LUNA** failure mode |
| `SEQUENCER_DOWN` / `SEQUENCER_GRACE` | An L2 whose sequencer is down, or back up but not long enough |
| `DESCRIPTION_MISMATCH` | An address filed under the wrong pair |
| `DECIMALS_MISMATCH` / `NON_STANDARD_DECIMALS` | Consumers hardcoding `1e8` |

`BOUNDED_ANSWER` is the one worth explaining. An aggregator with circuit breakers
*cannot* report outside them. When LUNA fell through its floor, the feed kept returning
the floor — fresh, well-formed, and orders of magnitude wrong. Seeing it requires
resolving the proxy and reading the bounds off the implementation, which is what
`alchem-link inspect` does.

## Heartbeats are measured, not copied

This registry used to declare `3600` for every feed, inherited from Ethereum mainnet.
That is wrong almost everywhere, and wrong in the dangerous direction:

| Network | Real ETH/USD heartbeat | A 3600s check would… |
|---|---|---|
| Polygon | **60s** | miss a dead feed for an hour |
| Optimism / Base | **1200s** | miss it for 40 minutes |
| Arbitrum (USDC) | **300s** | miss it for 55 minutes |
| Ethereum | 3600s | be correct |

Every heartbeat in the registry now comes from walking that feed's own round history.
The trick is that Chainlink publishes on *either* a heartbeat or a deviation threshold,
and the two leave different fingerprints — intervals pile up against a ceiling (the
heartbeat), and anything arriving well inside it was triggered by price movement.

```bash
alchem-link cadence ETH/USD -n optimism
```

```
ETH/USD  (optimism)   [matches]
  declared   20m
  observed   20m   (ceiling 1212s, median 442s over 39 intervals)
  triggers   8 by heartbeat, 31 by deviation
  deviation  threshold is at most 0.1509% (largest move seen 0.407%)
```

It also knows when it *cannot* tell you. On a fast L2 the price may never sit still long
enough for the clock to trigger a publish, and the window's longest gap is then a lower
bound, not a measurement:

```
ETH/USD  (arbitrum)   [not observed]
  every round in this 2941s window was deviation-triggered, so the heartbeat was never
  exercised — it is at least 451s, and the declared 3600s is not contradicted.
```

Feeds whose heartbeat is a bound rather than a measurement are marked `*` in
`alchem-link feeds`.

## Cross-chain divergence

ETH/USD is not one number. It is a separate oracle deployment per chain, and they do not
agree:

```bash
alchem-link divergence ETH/USD
```

```
ETH/USD   [agree]   consensus 1,923.88
  ethereum            1,929.60     +29.7 bps         48m 19s old
  linea               1,926.54     +13.8 bps         1h 49m old
  scroll              1,925.97     +10.9 bps         1h 22m old
  bnb                 1,921.96     -10.0 bps         43s old
  polygon             1,923.07      -4.2 bps         8s old
  ...
  10 chains agree — worst leg 29.7 bps from consensus (threshold 50)
```

Stale legs are excluded from consensus — a feed past its heartbeat is not evidence about
the current price — and outliers are attributed, so a lagging chain is not confused with
a broken one. **Testnets are excluded entirely**: Sepolia's feeds carry unrelated data,
and averaging them in produces a consensus that describes nothing.

## Generate a consumer that passes the audit

```bash
alchem-link generate ETH/USD -n base
```

Emits dependency-free Solidity with the address, the *measured* heartbeat, and — because
the target is a rollup — the sequencer gate and grace period:

```solidity
uint256 public constant MAX_AGE = 1380;   // measured 1200s heartbeat + 15% slack

if (sequencerStatus != 0) revert SequencerDown();
if (sequencerStartedAt == 0) revert SequencerDown();
if (block.timestamp - sequencerStartedAt <= GRACE_PERIOD) revert GracePeriodNotOver(...);

if (updatedAt == 0) revert IncompleteRound();
if (block.timestamp - updatedAt > MAX_AGE) revert StalePrice(updatedAt, MAX_AGE);
if (answer <= 0) revert InvalidPrice(answer);
if (answeredInRound < roundId) revert StaleRoundAnswer(answeredInRound, roundId);
```

Generate the same feed on Ethereum and the sequencer lines are absent — they would be
noise on an L1. `--lang typescript` and `--lang python` emit equivalents.

## Would your guards have caught it?

`audit` tells you about a feed as it is right now. `simulate` asks the question that
actually decides whether a protocol survives: **given the checks your contract performs,
what would have happened?**

```bash
alchem-link simulate
```

```
scenario          result  accepted  what it takes to catch
healthy           clean   8/8       a good guard accepts every round here
bounded_crash     MISSED  10/10     needs a consumer-side price bound or a move limit — staleness will not catch it
frozen_feed       caught  5/10      caught by a staleness window sized to the real heartbeat
sequencer_outage  MISSED  10/10     needs the sequencer uptime gate; the price feed answers throughout
carried_rounds    MISSED  10/10     needs answeredInRound >= roundId
flash_spike       MISSED  7/7       needs a move limit; the spike is fresh, positive and complete
incomplete_round  caught  5/6       needs the updatedAt != 0 check
clock_skew        caught  5/6       needs signed age arithmetic; unsigned subtraction underflows here

4/8 scenarios handled (50%)
gaps: bounded_crash, sequencer_outage, carried_rounds, flash_spike
try --strict, or turn on the specific guard each gap names
```

That is the *default* guard — a staleness window and a positivity check, which is what
most integrations have. `--strict` turns everything on and handles all eight. `--naive`
is `latestRoundData()` and nothing else, and handles two.

`bounded_crash` is the one worth staring at. It is the LUNA shape: the price falls
through the aggregator's `minAnswer` and the feed pins to the floor. Every observation
after that is **fresh, positive, complete, and orders of magnitude wrong**. Staleness
cannot see it. Only a consumer-side bound or a move limit can.

There is a healthy control in the set on purpose — without it, a guard that rejects
everything would score perfectly.

Then check the other direction, against real history:

```bash
alchem-link backtest ETH/USD -n base --strict
```

Rejections there are false positives: rounds the feed legitimately produced that your
guard would have thrown away. A guard that scores 8/8 on the scenarios and rejects a
third of real history is not a guard anyone can ship.

## Statistics that account for how oracles publish

```bash
alchem-link stats ETH/USD -n base
```

```
ETH/USD  (base)  30 rounds over 8h 20m
  last          1,930.24
  change          +0.412%
  range         1,918.00 – 1,944.51   (1.38%)
  twap          1,926.88
  vs twap            +17 bps
  volatility        31.4% annualised
  max drawdown     0.884%
  largest move      94.2 bps
  interval      median 20m
```

Two of those numbers are computed differently from how a naive implementation would.

**TWAP is time-weighted, not sample-weighted.** An oracle publishes on a heartbeat *or*
on a deviation threshold, so it prints most often precisely when the price is moving —
which means the mean of the answers systematically over-weights volatile periods. A price
that sat at 1,900 for fifty minutes and then walked to 1,950 over six one-minute rounds
has a sample mean of **1,921** and a time-weighted mean of **1,902**: the flat stretch was
90% of the window but only two of the seven prints. The second number is what a TWAP
oracle would have reported, and spot is 253 bps above it.

**Volatility is scaled by measured spacing.** Annualising needs the sampling interval;
assuming one is how the same asset reports wildly different volatility on Polygon (60s
publishes) and Ethereum (3600s). The interval comes from the timestamps.

## Commands

**Live** — `price` · `feeds` · `audit` · `inspect` · `history` · `updates` · `stats` ·
`cadence` · `divergence` · `sequencer` · `watch` · `gas` · `holdings` · `transfers` ·
`block` · `doctor` · `verify` · `ccip`

**Offline** — `search` · `networks` · `coverage` · `simulate` · `backtest` · `theme`

**Interactive** — `ui` · `shell` · `chat` · `providers`

`shell` and `chat` take `--workspace DIR`, `--yes`, `--allow-exec` and `--read-only`.

**Reference and codegen** — `generate` · `alchemy` · `chainlink` · `integration` ·
`blueprint` · `recipes`

Every command takes `-n/--network`, `--rpc-url`, `--no-color` and `--format`.

### `--format` — one flag, every command

Every result object exposes `as_dict()`, so anything that returns a list can be emitted
as `json`, `ndjson`, `csv`, `markdown` or a **Prometheus** scrape body:

```bash
alchem-link feeds --live -n polygon --format prometheus
```

```
# HELP alchem_link_feed_stale 1 when the answer is older than its heartbeat plus tolerance
# TYPE alchem_link_feed_stale gauge
alchem_link_feed_stale{network="polygon",pair="ETH/USD"} 0
alchem_link_feed_age_seconds{network="polygon",pair="ETH/USD"} 41
```

Scrape that on a timer and `alchem_link_feed_stale == 1` is a complete alert rule — the
tool stops being something you run when you are worried and starts telling you when to
be.

## Batched reads

Reading a network's feeds means three `eth_call`s each. Done naively that is 48 round
trips on Ethereum — about 20 seconds on a public endpoint. The toolkit collapses them
through **Multicall3**, falls back to **JSON-RPC batching** where Multicall3 is not
deployed, and reports which tier it used:

```
ethereum    16/16 OK   607ms  http=2 logical=50
```

The tier matters beyond speed. Multicall3 executes every sub-call in one EVM invocation,
so all 48 reads come from the *same block* — without which comparing two feeds means
comparing two different moments.

## Verified registry — 66 feeds, 11 networks

Every address ships only after being called for `description()` and `decimals()`, and is
filed under the pair the contract itself reports. That check keeps earning its place:

- The address widely shared as Base **"BTC/USD"** reports `WBTC / USD` — a wrapper that
  can depeg. Registered under its real name, with a note.
- The Gnosis address commonly labelled **"xDAI/USD"** reports `DAI / USD`.
- Two candidate CCIP routers had **no code at all** and were dropped.

```bash
alchem-link verify -n base
```

| Network | Chain ID | Feeds | |
|---|---|---|---|
| `ethereum` | 1 | 16 | CCIP |
| `sepolia` | 11155111 | 3 | testnet, CCIP |
| `base` | 8453 | 6 | L2, uptime feed, CCIP |
| `arbitrum` | 42161 | 8 | L2, uptime feed, CCIP |
| `optimism` | 10 | 6 | L2, uptime feed, CCIP |
| `polygon` | 137 | 7 | CCIP |
| `avalanche` | 43114 | 5 | CCIP |
| `bnb` | 56 | 5 | CCIP |
| `gnosis` | 100 | 4 | |
| `scroll` | 534352 | 3 | L2 |
| `linea` | 59144 | 3 | L2 |

Endpoint resolution, most explicit first: `--rpc-url`, then `ALCHEMY_URL`, then
`ALCHEMY_API_KEY` combined with the network's Alchemy subdomain, then a keyless public
endpoint. `doctor` always reports which one is in use, with any key redacted.

## Gas, priced in dollars

`eth_gasPrice` returns one number and hides the structure. `eth_feeHistory` returns the
base fee, the priority-fee percentiles, and — because EIP-1559 fixes it from the parent
block — the next block's base fee as *fact*, not forecast. And since the package already
reads Chainlink, the estimate converts through the native token's feed on the same chain:

```bash
alchem-link gas
```

```
ethereum  (20 blocks sampled)
  base fee   0.2057 gwei  →  next block 0.2215 gwei [rising]
  congestion 48% of target
  ETH/USD    1,929.60

  tier         tip (gwei)   max (gwei)    transfer        swap
  slow             0.0009       0.4438     $0.0090     $0.0772
  standard         0.1500       0.5930     $0.0151     $0.1290
  fast             1.8970       2.3400     $0.0858     $0.7358
```

That composition is the whole Alchemy-plus-Chainlink premise in one command: chain state
from the node, valuation from the oracle.

## CCIP

CCIP does not address chains by chain ID. It uses 64-bit **chain selectors** — Ethereum
is chain ID 1 and selector 5009297550715157269 — and passing one where the other belongs
compiles, deploys, and reverts. Every router here answers `typeAndVersion()` as
`Router 1.2.0`, and every lane is confirmed against the router's own `isChainSupported`:

```bash
alchem-link ccip -n base
```

```
router 0x881e3A65B4d4a04dD529061dd0071cf975F58bCD  (base)
  arbitrum     selector 4949039107694359620    open
  ethereum     selector 5009297550715157269    open
  sepolia      selector 16015286601757825753   closed
```

## Python API

One object holds a network, a connection and a per-feed cache, so a session is one client
rather than one per call:

```python
from alchem_link import connect

link = connect("base")
link.price("ETH/USD")            # one round trip
link.price("ETH/USD")            # cached — no round trip
link.audit()                     # reuses the same client and Multicall3 probe
link.stats("ETH/USD")            # TWAP, volatility, drawdown over recent history
link.everywhere("ETH/USD")       # every chain that carries it, concurrently
link.rpc_stats()                 # what the whole session actually cost
```

Cache TTLs come from each feed's **measured** heartbeat, not a constant — a Polygon feed
on a 60-second cadence caches for 20 seconds, an hourly mainnet feed for two minutes.
`price(..., strict=True)` raises `StaleFeed` instead of returning a reading that says so,
for the contract-facing paths where forgetting to check `.stale` is the bug.

Every exception descends from `AlchemLinkError`, and the ones that replaced a builtin
still inherit it — `UnknownNetwork` is a `KeyError`, `AbiError` is a `ValueError` — so
nothing that caught them before stops working.

The functional API is unchanged:

```python
from alchem_link import read_feed, audit_feed, profile_feed, compare_pair

reading = read_feed("ETH/USD", network="polygon")
if reading.stale:
    raise RuntimeError(f"{reading.pair} is {reading.age_secs}s old — refusing to trade")

audit = audit_feed("ETH/USD", network="base")
if not audit.safe_to_consume:
    for finding in audit.sorted_findings:
        print(finding.severity, finding.code, finding.detail)

profile = profile_feed("ETH/USD", network="arbitrum", rounds=120)
print(profile.observed_heartbeat, profile.heartbeat_verdict)

report = compare_pair("ETH/USD")
print(report.spread_bps, [leg.network for leg in report.outliers])
```

Batched reads of arbitrary contracts, with the ABI codec exposed:

```python
from alchem_link import Call, batch_call, client_for, encode_call, selector

print(selector("latestRoundData()"))          # 0xfeaf968c — computed, not stored
rpc = client_for("ethereum")
report = batch_call(rpc, [
    Call(token, "balanceOf(address)", (wallet,), ["uint256"], "balance"),
    Call(token, "symbol()", (), ["string"], "symbol"),
])
print(report.tier, report.block_atomic, report.by_label("balance").one())
```

History from event logs rather than by walking rounds — one `eth_getLogs` instead of a
hundred `eth_call`s — plus the statistics over it:

```python
from alchem_link import Series, answer_updates, summarise

updates = answer_updates(address, hours=24, network="base")
stats = summarise(Series.from_updates(updates, "ETH/USD", "base"))
print(stats.twap, stats.volatility_annual, stats.max_drawdown_pct)
```

Fan a read across every chain that carries a pair, concurrently, with failures reported
as rows rather than raised:

```python
from alchem_link import read_pair_everywhere

sweep = read_pair_everywhere("ETH/USD")
print(sweep.values(), sweep.failed, f"{sweep.speedup:.1f}x")
```

Replay your consumer's guards, offline:

```python
from alchem_link import Guard, audit_guard

result = audit_guard(Guard(max_age_secs=3600, require_positive=True))
print(result.score, result.failed)     # 0.5 ['bounded_crash', 'sequencer_outage', ...]
```

## Zero dependencies, including the hash

`hashlib` ships SHA3-256, which is **not** Keccak-256 — NIST changed the
domain-separation byte between the Keccak submission and the final standard, and
Ethereum froze on the original. That single byte is why so much Ethereum tooling reaches
for a native extension.

This package implements Keccak-256 in about a hundred lines of integer arithmetic, so
function selectors are **computed rather than trusted**:

```python
>>> selector("getRoundData(uint80)")
'0x9a6fc8f5'
```

That is what makes the ABI codec possible — including the `(address,bool,bytes)[]`
tuple-array encoding that `Multicall3.aggregate3` requires — and it is pinned in tests
against the standard vectors *and* the four selectors this package previously shipped as
hand-verified constants.

As of 0.23.0 there is no optional extra either. The dashboard used to need `textual`,
which made the user interface the one place the zero-dependency claim was quietly
abandoned. It is not any more — see below.

One practical note baked into the client: several public RPC providers reject Python's
default `Python-urllib/3.x` User-Agent with a 403, which reads as "the chain is down" if
you have not hit it before. The client always sends a real one.

## The agent can write code, not just talk about it

```bash
alchem-link shell
```

```
alchem:base › write me a consumer for ETH/USD on base, with tests

  · audit_feed(pair='ETH/USD', network='base')

   WRITE  generate_project  oracle/
  scaffold a foundry project for ETH/USD into oracle/ (consumer, mocks, tests, deploy)
  [y] once  [a] always here  [n] no  [d] never here  [v] view
  approve? y

  ✎ generate_project(pair='ETH/USD', out='oracle')

  Scaffolded a Foundry project in oracle/. The consumer checks staleness against
  base's measured 1200s heartbeat, rejects carried rounds, and gates on the L2
  sequencer with its grace period. Tests cover each failure mode.

  changed: oracle/src/EthUsdConsumer.sol, oracle/test/EthUsdConsumer.t.sol, …
```

Twenty-eight tools: read, write and edit files, make and search directories, scaffold
projects, export results, and run commands. `:tools` lists them grouped by what they do
to your machine.

### Codegen goes through the generator, not the model

When you ask for a Chainlink consumer the agent calls `generate_consumer` rather than
writing Solidity from memory, and the system prompt says so in as many words. The reason
is the same one the rest of this toolkit exists for: a model writing that contract from
training data produces something that compiles, looks correct, hardcodes `3600`, and
omits the sequencer check. The generator bakes in that feed's **measured** heartbeat for
that chain and every check `audit` looks for.

A plausible-looking contract is the same class of failure as a plausible-looking price.

### Three gates, not a checkbox

Everything the agent does to your filesystem passes through:

**A workspace root.** Paths are fully resolved — symlinks followed, `..` collapsed — and
*then* compared against the root. Rejecting strings that contain `..` is theatre; a
symlink inside the workspace pointing at `/` is not caught by it, and neither is a
Windows junction.

**A secrets refusal that approval cannot override.** This is the non-obvious one. Tool
results are sent to a third-party model, so reading `.env` is not a read, it is a
disclosure of your `ALCHEMY_API_KEY` to whoever runs the inference endpoint. `.env`, PEM
files, SSH keys, `.npmrc`, cloud credentials and **Solana keypairs** are refused before
the approval prompt, for reads as much as writes, and they do not appear in directory
listings or search results either. You cannot meaningfully consent to a disclosure you
have not been shown.

**A prompt you can actually answer.** It leads with the verb and the path, not the tool
name, and `v` shows the diff. Approving a write you have not seen is a keystroke, not
consent — and the edits where that matters are the ones that look routine.

```
   WRITE  write_file  src/Consumer.sol
  overwrite src/Consumer.sol — 84 lines, 3,201 bytes
  [y] once  [a] always here  [n] no  [d] never here  [v] view
```

`a` grants for that directory for the rest of the session, not for the whole workspace
and not for the next one. Grants are **never written to disk**: a permission that
survives the process turns one distracted keystroke into a standing authorisation.

### Execution is off until you turn it on

`run_command` is refused outright until `--allow-exec`, and then still prompts per
command. It runs **without a shell** — the command is split into an argument vector, so
pipes, redirection and `;` are not interpreted. That costs some convenience and removes a
class of injection: the argument list in the approval prompt is exactly what runs, with
no second layer of parsing in between.

### Defaults, and why

| Situation | Reads | Writes | Commands |
|---|---|---|---|
| `alchem-link shell` | yes | prompt | refused |
| `alchem-link shell --yes` | yes | yes | refused |
| `alchem-link shell --allow-exec` | yes | prompt | prompt |
| `alchem-link shell --read-only` | yes | refused | refused |
| `alchem-link chat "…"` piped or in CI | yes | **refused** | refused |

The last row is the important one. When there is no terminal, a prompt cannot be
answered, and treating silence as consent is how an agent quietly rewrites a repository
in a CI job. `--yes` is the explicit opt-out, and it has to be typed.

A refusal also tells the model *why*, accurately. "Running commands is not enabled in
this session" and "the user was asked and declined" are different facts, and reporting
the second when no prompt was ever shown leaves you arguing with an assistant about a
decision neither of you made.

### Steering it from the shell

```
:workspace [dir]   show or move the directory the agent may write in
:trust             what it may do without asking
:trust write       stop prompting for writes, this session
:trust exec        enable commands, this session
:trust readonly    refuse writes and commands
:trust revoke      drop every grant given so far
:changes           every file written this session
:diff <path>       what is in a file now
```

## The terminal system

```bash
pip install alchem-link
alchem-link ui
```

```
  Alchem-Link v0.23.24                             base  ·  66 feeds  ·  truecolor
 ALCHEM-LINK      ┏━ Live Feeds ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┓
                  ┃ETH/USD            1,930.24  FRESH      3m 21s ago · hb 20m  ┃
 Live Feeds       ┃WBTC/USD          67,940.11  FRESH      1m 04s ago · hb 20m  ┃
 Safety Audit     ┃  Wrapped BTC, not spot BTC — can depeg.                     ┃
 Analytics        ┃USDC/USD               1.00  FRESH     41m 12s ago · hb 1d   ┃
 Cross-chain      ┗━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┛
 ...
 Live Feeds · base          ↑↓/jk scroll · tab pane · n network · r refresh · q quit
```

Fifteen panels, including the guard simulator and registry coverage. Every panel that
talks to a chain does so on a worker thread, so an RPC round trip never freezes the app,
and results cache per panel until `r`. `n`/`N` cycle networks, `tab` moves focus between
the rail and the panel, `1`-`9` jump, `q` quits.

### It is black and blue before the first frame

Drawing black rectangles gets you a black *pane*. The columns past the last painted cell,
the scrollback above the prompt, and anything a subprocess writes all stay whatever colour
the terminal was. So `alchem_link.term.boot` repaints the terminal's **own defaults** via
OSC 11/10/12 — background, foreground and cursor — and hands them back on exit. That runs
from `alchem-link`, from `alchem-link shell`, and from the compiled binary, which is why
plain `alchem-link price ETH/USD` output sits on the same surface the dashboard does.

Colour is negotiated per stream and degrades rather than breaking: truecolor → xterm-256
→ the basic sixteen → none. `NO_COLOR`, a pipe, or `--no-color` each reduce output to
plain text **with the layout intact** — the tests assert the two are character-identical,
because this output goes into CI logs and issue reports at least as often as onto a
screen. On Windows, VT processing is enabled through the Win32 API first; without that
call the whole UI renders as visible `←[38;2;…m` garbage.

### What is actually in there

| Module | Job |
|---|---|
| `term/ansi.py` | escape sequences, colour-depth negotiation, display-width measurement |
| `term/screen.py` | double-buffered cell grid emitting only the runs that changed |
| `term/input.py` | raw mode, and a pure parser from escape sequences to named keys |
| `term/widgets.py` | panels, tables, sparklines, gauges, tabs, scroll state |
| `term/app.py` | event loop, worker pool, resize handling |
| `term/boot.py` | terminal initialisation — and putting it back |

An idle frame costs **zero bytes**: the screen diffs the back buffer against what is
actually on the terminal and emits one cursor move per changed run. That is the difference
between a dashboard that is usable over SSH and one that is not.

The layering is strict — `ansi` knows about bytes, `screen` about cells, `widgets` about
rectangles, `app` about events — and nothing in the subpackage imports anything from
`alchem_link` except the inert palette, so it can be read and reused on its own.

### Testable without a terminal

Panels render to a list of `(text, style)` lines; the app paints a window onto that list.
Scrolling and clipping are one slice on one list, and every renderer is a pure function —
so `tests/test_dashboard.py` exercises all fifteen panels in their loading, empty and
error states without a terminal in sight. That matters more here than usual: a dashboard
that crashes takes the whole screen with it.

### Standalone binaries

No Python needed on the target machine, and nothing to collect — the package has no
dependencies to bundle:

```bash
pip install pyinstaller
pyinstaller alchem-link.spec        # the CLI
pyinstaller alchem-link-ui.spec     # the dashboard
```

A binary launched by double-click lands in a fresh console with no `TERM` at all, which is
where colour detection has the fewest hints and where theming matters most. `boot` detects
the frozen case and themes it anyway.

### The palette

`alchem_link/theme.py` is the single source: an inert table of hex values and semantic
`Style` roles, with no escape sequences in it. `ansi` encodes a role for whatever depth is
available; `render` uses the same roles for line output; the web build under
`web/lib/alchem/` mirrors the same values.

`tests/test_theme.py` fails the build if any render module hardcodes a colour, if a role
paints on an undefined surface, or if the three status colours collapse into each other
once quantised to 256 or 16 colours — which is how a "harmless" palette tweak stops a
STALE badge from being distinguishable on a bare console.

```bash
alchem-link theme     # the palette, and what this terminal negotiated
```

## Web build

The same reader runs at `/alchem-link` in the Scematica dashboard (`web/`), porting
`abi.py`, `networks.py` and `feeds.py` to TypeScript under `web/lib/alchem/` and serving
routes that read aggregators **server-side**, so `ALCHEMY_API_KEY` never reaches the
browser and CORS-less public RPC hosts still work.

This package stays authoritative. When the registry here changes, change
`web/lib/alchem/feeds.ts` too — `/api/alchem/verify` is what catches the two drifting,
because it asks the chain rather than either table.

## Exit codes

| Code | Meaning |
|---|---|
| 0 | Success |
| 1 | Completed, but the result is not usable — a stale feed, a failed check, a divergence |
| 2 | Usage error — unknown pair, unknown network, missing argument, missing API key |
| 3 | Network error — endpoint unreachable |
| 4 | RPC error — the node answered with an error |

`price` exits non-zero on a stale read and `audit` on any finding at medium or worse, so
both drop straight into a CI gate.

## Tests

```bash
python -m unittest discover -s tests
```

561 cases, all offline — no test in this suite reads a chain.

That is a design constraint rather than a convenience. `analytics` and `simulate` compute
numbers people size positions and write guards against, and a number that can only be
checked against a live chain cannot be checked at all. The RPC transport is stubbed at the
single-attempt boundary so the real retry policy stays under test; every dashboard panel is
rendered in its loading, error and empty states; and the terminal engine is exercised
against an in-memory screen, including the cases that are invisible until they are
catastrophic — a diff that reports "nothing changed" when something did, a wide character
that shifts every cell after it by one column, and an escape sequence emitted at a terminal
that would render it as literal digits.

## Requirements

Python 3.10 or later. Nothing else.

The dashboard wants a terminal, but does not require much of one: it negotiates down to
256 colours, to the basic sixteen, and to no colour at all, and every command works
unstyled through a pipe.

## License

MIT
