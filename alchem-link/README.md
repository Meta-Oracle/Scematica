# Alchem-Link v0.4.0

An Alchemy × Chainlink developer toolkit that reads chains instead of documentation.
Live oracle reads, a consumer-safety audit, and measured feed behaviour — from the
command line, a terminal dashboard, or Python, with **no dependencies beyond the
standard library**.

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

## Commands

**Live** — `price` · `feeds` · `audit` · `inspect` · `history` · `cadence` ·
`divergence` · `sequencer` · `watch` · `gas` · `holdings` · `transfers` · `block` ·
`networks` · `doctor` · `verify` · `ccip`

**Reference and codegen** — `generate` · `alchemy` · `chainlink` · `integration` ·
`blueprint` · `recipes`

Every command takes `-n/--network`, `--rpc-url` and `--json`.

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

The TUI is the only part needing a third-party package (`textual`), and it is an optional extra — `pip install alchem-link` pulls nothing at all.

One practical note baked into the client: several public RPC providers reject Python's
default `Python-urllib/3.x` User-Agent with a 403, which reads as "the chain is down" if
you have not hit it before. The client always sends a real one.

## Terminal UI

```bash
pip install 'alchem-link[tui]'
alchem-link-ui
```

Eleven panels — Live Feeds, Safety Audit, Cross-chain, L2 Sequencer, Gas, CCIP Lanes,
plus the reference set. Every panel that talks to a chain does so on a worker thread, so
an RPC round trip never freezes the app, and results cache per panel until `r`. `n`
cycles networks; `j`/`k` or arrows navigate; `q` quits.

Build a standalone executable, no Python needed on the target machine:

```bash
pip install pyinstaller
pyinstaller alchem-link-ui.spec
```

The palette lives in `alchem_link/theme.py`, the single source for both the Textual
stylesheet and the inline Rich markup; `tests/test_theme.py` fails if a panel starts
hardcoding its own colours.

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

214 cases, all offline. The RPC transport is stubbed at the single-attempt boundary so
the real retry policy stays under test, and every TUI panel is rendered in its loading,
error and empty states.

## Requirements

Python 3.10 or later. The TUI needs a terminal with 256-colour support.

## License

MIT
