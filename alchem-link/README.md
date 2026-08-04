# Alchem-Link v0.3.0

A practical Alchemy x Chainlink developer toolkit. Reads live Chainlink price feeds and
Alchemy-served chain state from the command line, a terminal dashboard, or Python — with
no dependencies beyond the standard library.

```bash
pip install alchem-link
alchem-link price ETH/USD
```

```
ETH / USD  (ethereum)
  price      1,877.94
  status     FRESH
  updated    50m 20s ago (heartbeat 3600s)
  round      129127208515966893520
  aggregator 0x5f4eC3Df9cbd43714FE2740f5E3616155c5b8419
```

No API key needed to start. Set `ALCHEMY_API_KEY` when you want real rate limits.

## Why it exists

Alchemy handles the RPC side — node access, WebSocket subscriptions, transaction
monitoring. Chainlink handles the oracle side — price feeds, VRF, automation, CCIP.
Most developers use them in isolation.

Alchem-Link is the layer between them: live reads that work in one command, plus the
integration reference that explains how the two systems fit together.

## Live commands

| Command | What it does |
|---|---|
| `alchem-link price ETH/USD` | Read one Chainlink feed, with a staleness verdict |
| `alchem-link feeds` | List registered feeds and their aggregator addresses |
| `alchem-link feeds --live` | Read every feed on a network at once |
| `alchem-link block` | Current block height and round-trip latency |
| `alchem-link gas` | Current gas price in gwei |
| `alchem-link networks` | Supported networks, chain ids, feed counts |
| `alchem-link doctor` | End-to-end readiness check |
| `alchem-link verify` | Confirm each registered address still reports its filed pair |

Every command takes `-n/--network`, `--rpc-url`, and `--json`.

## Staleness is the point

A feed that answers is not necessarily a feed that published. `latestRoundData()` returns
happily whether the last update was ten seconds or ten hours ago, and acting on a stale
answer is how oracle integrations lose money.

Every reading carries its age against the expected heartbeat:

```
  ETH/USD           1,877.94  FRESH   50m 51s ago
  BTC/USD          64,316.05  FRESH   51m 15s ago
  LINK/USD            8.2016  FRESH   30m 27s ago
  SOL/USD            74.1103  STALE   3h 16m ago
  USDC/USD        0.99974201  FRESH   13h 32m ago
```

`SOL/USD` is flagged; `USDC/USD` is not, despite being far older, because stablecoin
feeds publish on a much longer cadence. `alchem-link price` exits non-zero on a stale
read, so it drops straight into a health check or a CI gate.

## The registry is verified, not copied

Every address ships only after being called for `description()` and `decimals()`, and is
filed under the pair the contract itself reports.

That check earns its keep. The address widely passed around as Base "BTC/USD" reports
`WBTC / USD` on-chain — a wrapper that can depeg from spot BTC. It is registered here
under its real name, with a note.

Re-run the check any time:

```bash
alchem-link verify -n base
```

## Doctor

Three failures are silent in practice: you are on the keyless fallback and getting rate
limited, you are pointed at the wrong chain, or a feed technically responds but has not
published in hours. `doctor` turns each into a visible line.

```
network   ethereum
endpoint  https://ethereum-rpc.publicnode.com  (public fallback)

  [ok  ] credentials      using the keyless public endpoint
           Set ALCHEMY_API_KEY for higher rate limits and reliability.
  [ok  ] rpc reachable    block 25,684,264 in 295 ms
  [ok  ] chain id         expected 1, got 1
  [ok  ] gas price        0.102 gwei
  [ok  ] feed read        ETH/USD = 1,877.9446 (FRESH, 3022s old)
```

An API key in the endpoint is redacted before anything is printed.

## Networks

| Key | Chain | Chain ID | Feeds |
|---|---|---|---|
| `ethereum` | Ethereum Mainnet | 1 | 6 |
| `sepolia` | Ethereum Sepolia | 11155111 | 3 |
| `base` | Base Mainnet | 8453 | 2 |
| `arbitrum` | Arbitrum One | 42161 | 3 |
| `optimism` | OP Mainnet | 10 | 2 |
| `polygon` | Polygon PoS | 137 | 3 |

Endpoint resolution, most explicit first: `--rpc-url`, then `ALCHEMY_URL`, then
`ALCHEMY_API_KEY` combined with the network's Alchemy subdomain, then a keyless public
endpoint. `doctor` always reports which one is in use.

## Python API

```python
from alchem_link import read_feed, read_all_feeds, diagnose, client_for

reading = read_feed("ETH/USD", network="ethereum")
if reading.stale:
    raise RuntimeError(f"{reading.pair} is {reading.age_secs}s old — refusing to trade")
print(reading.price, reading.description, reading.status)

for r in read_all_feeds(network="arbitrum"):
    print(r.pair, r.price, r.status)

rpc = client_for(network="base")
print(rpc.block_number(), rpc.chain_id())

report = diagnose(network="polygon")
print(report.ok, [c.name for c in report.checks if not c.ok])
```

Decoding is exposed too, if you are reading an aggregator this package does not know
about:

```python
from alchem_link import words, to_int, to_uint, decode_string, scale
```

## No dependencies for the live half

Reading a Chainlink aggregator needs four function selectors and the ability to decode
five static words plus one dynamic string. That is a few dozen lines of standard library,
versus pulling in `web3` and its transitive tree for the same result.

Function selectors are the first four bytes of `keccak256(signature)`. Python's stdlib
ships SHA3-256, which is **not** Keccak-256 — the padding differs — so rather than add a
hashing dependency, the four selectors are stored as the constants they are. Each was
verified against a live mainnet aggregator, and the decoded fixtures are pinned in
`tests/test_abi.py`.

The TUI is the only part that needs a third-party package (`textual`).

One practical note baked into the client: several public RPC providers reject Python's
default `Python-urllib/3.x` User-Agent with a 403, which reads as "the chain is down" if
you have not hit it before. The client always sends a real one.

## Terminal UI

```bash
alchem-link-ui
```

Six panels. **Live Feeds** reads real prices off the UI thread, so an RPC round trip never
freezes the app — `r` refreshes, `n` cycles networks. **Blueprint**, **Alchemy**,
**Chainlink**, **Integration** and **Recipes** are the offline reference. Navigate with
arrow keys or `j`/`k`; `q` quits.

Build a standalone executable, no Python needed on the target machine:

```bash
pip install pyinstaller
pyinstaller alchem-link-ui.spec
# output: dist/alchem-link-ui.exe
```

## Reference commands

The integration reference is unchanged and still offline:

```bash
alchem-link blueprint      # full integration blueprint
alchem-link alchemy        # Alchemy capability summary
alchem-link chainlink      # Chainlink capability summary
alchem-link integration    # cross-system integration map
alchem-link recipes        # all developer recipes
alchem-link recipes <id>   # single recipe by id
```

Recipes: `oracle-backed-automation`, `real-time-data-pipeline`,
`secure-bridge-experiment`, `ccip-cross-chain-transfer`.

## Exit codes

| Code | Meaning |
|---|---|
| 0 | Success |
| 1 | Completed, but the result is not usable — a stale feed, a failed check |
| 2 | Usage error — unknown pair, unknown network, missing argument |
| 3 | Network error — endpoint unreachable |
| 4 | RPC error — the node answered with an error |

## Tests

```bash
python -m unittest discover -s tests
```

76 cases, all offline. The RPC transport is stubbed at the single-attempt boundary so
the real retry policy stays under test.

## Requirements

Python 3.10 or later. The TUI needs a terminal with 256-colour support — Windows
Terminal, iTerm2, or any modern Linux terminal.

## License

MIT
