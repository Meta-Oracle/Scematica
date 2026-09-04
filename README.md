# Scematica v1.28.0

**CA: HcsHqEJ9suf4oHJ8mb52M7AVKjhYhnTaeHgTmde7pump**

Autonomous AI trading infrastructure for Solana. Token sniping, cross-DEX arbitrage, Dueling Deep Q* reinforcement learning, and a Rust-native x402 monetization protocol — unified under a real-time TUI dashboard.

> **New to coding?** See [BEGINNER_GUIDE.md](docs/BEGINNER_GUIDE.md) for a complete step-by-step setup walkthrough — no experience needed.
>
> 📚 **All documentation lives in [`docs/`](docs/README.md)** — getting started, the ScemaDEX SDK, trading strategy, the Fibonacci protocol, and the whitepaper.

## Quick Start (Windows, one-click)

After installing Rust + Git, double-click these from the project folder in order:

```text
init.bat                  # one-time: toolchain check + fetch all deps + scaffold .env
build.bat                 # compile every binary (5-10 min first run)
start-dashboard-demo.bat  # bot dashboard, demo mode (no tokens/RPC)
start-sdk-dashboard.bat   # ScemaDEX SDK dashboard, SIM mode (offline)
```

Full bot mode (`start-dashboard.bat`) needs 250k SCEMA + an RPC endpoint in
`.env`. See [QUICKSTART.md](docs/QUICKSTART.md) for the full script reference and
[scemadex.md](docs/scemadex.md) for the ScemaDEX SDK — the agentic liquidity layer
(intent solving, Conviction-Routing bonds, and an inference/experience mesh),
with x402 payments over the [Dexter x402 SDK](https://github.com/Dexter-DAO/dexter-x402-sdk).

---

## Install from crates.io

Every component is published to [crates.io](https://crates.io) and installs a
ready-to-run command — no clone or local build required.

### One line: the whole stack

```bash
cargo install scematica-suite      # installs the `scematica` launcher
scematica help                     # list every subcommand
scematica dashboard --demo         # try the dashboard with zero setup
```

`scematica-suite` is the umbrella crate. As a **library** it re-exports every
component — `scematica_suite::{core, executor, protocol, ai, nn, sniper, dashboard, scemadex}` —
so one dependency pulls the whole stack. As a **launcher** its `scematica` binary
dispatches to the component binaries (found next to itself or on `PATH`). Install
the launcher plus every runnable in one shot:

```bash
cargo install scematica-suite scematica-dashboard scematica-sniper \
              scematica-protocol scematica-nn scemadex-sdk
```

Then all of these work (via the launcher or directly):

| Command | Crate | What it runs |
|---|---|---|
| `scematica dashboard` · `dashboard` | `scematica-dashboard` | Bot monitoring TUI (`--demo` = no setup) |
| `scematica sniper` · `sniper` | `scematica-sniper` | New-pool sniper engine |
| `scematica backtest` · `backtest` | `scematica-sniper` | Backtester over pool history |
| `scematica protocol` · `protocol` | `scematica-protocol` | x402 payment facilitator server |
| `scematica ddqn` · `scema-ddqn` | `scematica-nn` | Deep Q\* agent live **training viewer** |
| `scematica scemadex` · `scemadex` | `scemadex-sdk` | ScemaDEX agentic-liquidity **live viewer** |
| `playground` | `scema-agent-playground` | Multi-LLM agent arena |

### Easiest ways to run (no keypair, RPC, or tokens)

```bash
cargo install scematica-dashboard     && dashboard --demo   # bot dashboard with demo data
cargo install scematica-nn            && scema-ddqn         # watch the DQ* agent learn live
cargo install scemadex-sdk            && scemadex           # ScemaDEX bond pipeline (offline)
cargo install scema-agent-playground  && playground --demo  # LLM agents debate (needs local Ollama)
```

Live trading — `sniper`, or `dashboard` without `--demo` — needs a 250k SCEMA
balance, an RPC endpoint, and a funded keypair (see [Configuration](#configuration)).
The installed `dashboard` automatically finds an installed `sniper` on `PATH`.

### Versioning

The bot stack moves as one version, set once in `[workspace.package]` — every
`scematica-*` crate inherits it, so a crate cannot quietly drift ahead of the
rest again. The ScemaDEX SDK family and the playground version independently and
are still pre-1.0.

> **Why 1.27.0, and what happened to 1.26.0.** 1.26.0 was published to crates.io on
> 2026-08-21 and never described anywhere — no changelog entry, no README, and every doc
> in the tree still said 1.25.0. It is the same drift v1.25.0 was cut to end, caught one
> release later and from the other direction: last time the manifests ran ahead of the
> docs, this time a *published artifact* did. 1.26.0 stays where it is, because a
> published version is a fact and cannot be folded away; v1.27.0 is the release that
> describes it, together with everything that has landed since — Scematica Mesh, the
> Escrow Market, the Omni runtime reaching 0.5.0, and the Scylar terminal's move to a
> reasoning model. The rule that survives both incidents: the version is not the number in
> `Cargo.toml`, it is the number in `Cargo.toml` **and** every place that repeats it.

| Crate | Version | Installs | Kind |
|---|---|---|---|
| `scematica-suite` | 1.28.0 | `scematica` | launcher + umbrella lib |
| `scematica-dashboard` | 1.28.0 | `dashboard` | bin + lib |
| `scematica-sniper` | 1.28.0 | `sniper`, `backtest` | bin + lib |
| `scematica-arb` | 1.28.0 | `arb` | bin + lib (cross-DEX arbitrage; program-less) |
| `scematica-protocol` | 1.28.0 | `protocol` | bin + lib |
| `scematica-ai` | 1.28.0 | — | library |
| `scematica-executor` | 1.28.0 | — | library |
| `scematica-core` | 1.28.0 | — | library |
| `scematica-nn` | 1.28.0 | `scema-ddqn` | bin + lib |
| `scematica-sentience` | 1.28.0 | — | library (Ψ/Ω cognitive gate; no binary) |
| `scemadex-sdk` | 0.3.1 | `scemadex` | bin + lib (incl. zkML + real SNARK backend) |
| `scemadex-mcp` | 0.1.3 | `scemadex-mcp` | MCP server (LLM agents buy intelligence over x402) |
| `scemadex-settle` | 0.1.3 | — | devnet reference settler |
| `scema-agent-playground` | 0.1.1 | `playground` | bin |

Scematica Omni versions on its own track — a separate workspace on a modern HTTP stack, kept
out of the bot's lockfile on purpose (see `CLAUDE.md`). It is at **1.0**, and on a
verification runtime that number is a promise rather than a maturity badge: *a record sealed
today still verifies tomorrow.* What is frozen, what stays open, and what is deliberately not
covered are in [`scematica-omni/docs/COMPATIBILITY.md`](scematica-omni/docs/COMPATIBILITY.md)
— enforced by `scematica-omni/corpus/`, real records sealed by builds that no longer exist
and re-verified on every commit.

| Crate | Version | Installs | Kind |
|---|---|---|---|
| `scema-cli` | 1.0.0 | `scema` | bin (the loop, the launcher, `nft`, `execute`, `anchor`) |
| `scema-tui` | 1.0.0 | `scema-tui` | bin (the console) |
| `scema-daemon` | 1.0.0 | `scema-omnid` | bin (loopback HTTP) |
| `scema-mcp` | 1.0.0 | `scema-mcp` | bin (MCP over stdio) |
| `scema-world` `-tools` `-memory` `-sim` `-policy` `-verify` `-nft` `-trust` `-effect` `-anchor` `-agent` | 1.0.0 | — | libraries |

> **Install the 1.0.0 line, and upgrade every component together.** `Domain` and
> `EntityKind` became open enums in 0.5.0. The browser extension emits `domain: "web"` and
> `alchem-link` emits `domain: "data"`, so a `scema` or `scema-omnid` built before that
> rejects both at the door — *unknown variant `web`, expected one of `software`,
> `infrastructure`, `trading`, `unknown`* — which is two of the four producers. They are
> separate crates but one runtime; a new `scema` beside an old `scema-omnid` fails the same
> way. `scema --version` and `scema doctor` are the fastest checks.

`alchem-link` (Python, PyPI) versions independently again at **1.0.0**. What 1.0 promises, and the three things it deliberately does not, are in `alchem-link/docs/API-STABILITY.md`.

The **web dashboard** (`web/`) and the **Android companion app** share one version,
sourced from `web/package.json` (**1.28.0**). The mobile build reads it at build time to
set the app's `versionName`/`versionCode` and names the artifact **`scematica-v<version>.apk`**
— bump `web/package.json` to version the app. See [docs/mobile-app.md](docs/mobile-app.md).

Pin a version with `cargo install <crate> --version <x.y.z>`, or depend on a
library with `<crate> = "<x.y>"`. Library embedders who want a lean build
(no TUI deps) use `default-features = false` on `scematica-nn` / `scemadex-sdk`.

---

## Changelog

Full version history (v0.5.0 → v1.28.0) now lives in [CHANGELOG.md](CHANGELOG.md).

## The ScemaDEX rail — agent-accessible intelligence

Beyond the bot, Scematica exposes its signal pipeline as a payable, agent-facing
**rail**: any MCP client (Claude Desktop/Code, Cursor, the Agent SDK) can query
deployer reputation, 0–100 pool-quality scores, and Deep Q* advice for any Solana
mint — settled per-call over x402 (the agent pays USDC; no SOL, no API key).

- **Deploy a public relay:** [deploy/README.md](deploy/README.md)
- **Adoption playbook + MCP registry listing:** [docs/RAIL_ADOPTION.md](docs/RAIL_ADOPTION.md)
- **MCP server (5 tools):** [crates/scemadex-mcp/README.md](crates/scemadex-mcp/README.md)
- **Seed a fresh relay with real data:** `cargo run -p scemadex-integrations --bin signal-seeder`

---

## Architecture

Scematica is a Rust workspace. The published crates and the binaries they install:

| Crate | Binary | Purpose |
|---|---|---|
| `scematica-suite` | `scematica` | Umbrella lib (re-exports all) + launcher dispatching to every command |
| `scematica-core` | — | Shared config, RPC, wallet, metrics, types |
| `scematica-sniper` | `sniper`, `backtest` | Raydium pool sniping with filter pipeline and sell mechanics |
| `scematica-arb` | `arb` | Cross-DEX arbitrage graph search (Raydium / Orca / Meteora) |
| `scematica-executor` | — | Multi-DEX swap execution layer, Jupiter integration |
| `scematica-ai` | — | LLM agents: Risk, Arb, Debate, Strategy, Report, Chat |
| `scematica-nn` | `scema-ddqn` | Dueling Deep Q* RL agent + live training viewer |
| `scematica-dashboard` | `dashboard` | Ratatui TUI: monitor, control, AI chat |
| `scematica-protocol` | `protocol` | Rust-native x402 HTTP payment protocol for Solana |
| `scemadex-sdk` | `scemadex` | Agentic-liquidity SDK + live viewer (intents, bonds, mesh) |
| `scema-agent-playground` | `playground` | Multi-LLM agent-to-agent arena |

Tools:
- `tools/key-converter` — Convert keypair formats
- `tools/pool-seeder` — Pre-seed pool cache from on-chain data

The `programs/scematica-swap` Anchor program must be built and deployed separately with `anchor build`.

### File-based IPC

The sniper and dashboard are separate processes that communicate exclusively through JSON files in the working directory. All writes are atomic (write to `.tmp`, then rename). Never add sockets or channels — use this pattern.

| File | Writer | Purpose |
|---|---|---|
| `scematica-sniper.log` | sniper | Log stream tailed by dashboard |
| `scematica-trades.jsonl` | sniper | Append-only trade events (rotated at 10k lines) |
| `scematica-metrics.json` | sniper | Metrics snapshot every 5s |
| `scematica-filter-stats.json` | sniper | Per-filter rejection counts |
| `scematica-nn-stats.json` | NN agent | ε, steps, replay size, reward, Q-values (every 5s) |
| `scematica-nn-agent.json` | NN agent | Model checkpoint (every 10 min) |
| `scematica-nn-tournament.json` | NN tournament | Per-variant rewards + primary |
| `scematica-deployer-reputation.json` | reputation ledger | Per-deployer rug/success EMA |
| `scematica-strategy.json` | AI strategy agent | TP/SL/multiplier/regime |
| `scematica-rate-mode.json` | dashboard | Active rate mode + TP/SL |
| `scematica-sell-mode.json` | dashboard / drawdown guard | Pauses buys, sells positions |
| `scematica-dump-mode.json` | dashboard | Force-sell with `min_out = 0` |
| `pool-cache.json` | sniper / pool-seeder | Pool → mint lookup for sells |

---

## Prerequisites

- [Rust](https://rustup.rs/) (stable, 1.75+)
- [Solana CLI](https://docs.solana.com/cli/install-solana-cli-tools) (for keypair generation)
- A Solana wallet keypair (`~/.config/solana/id.json` or any path)
- At least **250,000 SCEMA** tokens in your wallet (token-gated — CA above)
- A private RPC endpoint (Helius, QuickNode, or Triton recommended)
- Optional: Groq or xAI API key for the AI chat and strategy agents

---

## Installation

```bash
git clone https://github.com/Meta-Oracle/Scematica.git
cd Scematica

# Build all binaries (release mode, ~5-10 min first run)
cargo build --release

# Binaries will be at:
#   target/release/sniper.exe      (Windows)
#   target/release/dashboard.exe
#   target/release/arb.exe
#   target/release/scematica-protocol.exe
```

> **Disk space:** Release builds use 5–10 GB. Run `cargo clean` periodically to reclaim space.

---

## Configuration

### Environment file (`.env`)

Create `.env` in the repo root:

```env
RPC_ENDPOINT=https://mainnet.helius-rpc.com/?api-key=YOUR_KEY
RPC_WS_ENDPOINT=wss://mainnet.helius-rpc.com/?api-key=YOUR_KEY

# AI (optional)
GROQ_API_KEY=gsk_...
# or
XAI_API_KEY=xai-...

# Emergency gate bypass (use only during RPC outages)
# SCEMATICA_SKIP_GATE=1
```

### `config.toml` reference

```toml
[rpc]
endpoint    = "https://mainnet.helius-rpc.com/?api-key=YOUR_KEY"
ws_endpoint = "wss://mainnet.helius-rpc.com/?api-key=YOUR_KEY"
commitment  = "confirmed"

[wallet]
keypair_path = "C:\\Users\\you\\.config\\solana\\id.json"

[sniper]
enabled            = true
quote_mint         = "WSOL"
quote_amount       = 0.01       # SOL per snipe (scaled by rate mode)
buy_slippage_pct   = 1.5
sell_slippage_pct  = 2.5
take_profit_pct    = 175.0      # v1.4.0: base TP; momentum escalation takes over above this
stop_loss_pct      = 18.0
trailing_stop_loss_pct   = 12.0
partial_tp_pct           = 50.0
partial_tp_trigger       = 100.0  # legacy; unused when tiered_partial_tp = true
price_check_interval_ms  = 250
price_check_duration_ms  = 900000   # 15 min total window
max_sell_retries         = 3        # v1.5.0: was 5; 3×4 rounds = 12 total attempts
max_buy_retries          = 3
auto_sell                = true
one_token_at_a_time      = true
max_buys                 = 0        # 0 = unlimited
max_concurrent_positions = 0       # 0 = unlimited; set e.g. 5 to cap open positions
daily_loss_limit_sol     = 0.05
blacklist_path           = "blacklist.txt"

# ── v1.1.0 new fields ────────────────────────────────────────
min_sol_reserve          = 0.02     # keep at least this SOL in wallet after buy
confirmation_window_ms   = 0        # 0=disabled; check vault drain before buying
pool_quality_sizing      = false    # scale position by pool_score/100
kelly_min_trades         = 10       # trades required before Kelly activates
check_interval_acceleration = true  # halve poll interval on 3 consecutive declines

# Session heat cooldown
session_heat_losses      = 0        # 0=disabled; losses in window to trigger pause
session_heat_window_secs = 3600     # rolling window for loss counting
session_heat_cooldown_mins = 15     # pause duration when heat trips

# Sell monitor exit signals
volume_exhaustion_pct    = 65.0     # exit when volume drops below 65% of entry (in profit only)
whale_exit_vault_drop_pct = 22.0    # exit on single-check vault drop ≥ 22%

# ── Kelly position sizing ─────────────────────────────────────
kelly_sizing    = false
kelly_fraction  = 0.25
kelly_lookback  = 20

# ── Pool predictive scoring ───────────────────────────────────
min_pool_score  = 35.0

# ── Profit-first growth ───────────────────────────────────────
profit_first_mode        = true
profit_first_floor_pct   = 25.0   # v1.4.0: tightened from 50%; exit rugs faster
wallet_target_sol        = 0.15

# ── Momentum hold ─────────────────────────────────────────────
momentum_hold                     = true
momentum_max_escalations          = 7      # v1.4.0: was 5
momentum_escalation_factor        = 1.8    # v1.4.0: was 1.6
momentum_pullback_exit_pct        = 8.0    # v1.4.0: tighter base, adaptive formula scales up
momentum_min_peak_pct             = 60.0   # v1.4.0: was 25%; require real peak before pullback fires
momentum_escalation_threshold_pct = 3.0    # v1.4.0: was 5%; lower bar for escalation trigger

# ── Risk breakers ─────────────────────────────────────────────
ath_drawdown_pct       = 0.0
grief_loss_window_secs = 300
grief_loss_limit_sol   = 0.0

# ── Multi-RPC failover ────────────────────────────────────────
extra_rpc_endpoints = []

[sniper.filters]
check_mint_renounced         = true
check_freezable              = true
check_burned                 = true
min_pool_size                = 5.0
max_price_impact_pct         = 3.5
check_holder_concentration   = true
max_top10_holder_pct         = 67.0
check_liquidity_momentum     = true
check_cross_pool_correlation = true
max_deployer_rugs_24h        = 2
filter_cache_ttl_secs        = 30   # cache filter results per pool (seconds)

# ── Deployer wallet age filter (v1.1.0) ──────────────────────
check_deployer_wallet_age    = false
deployer_min_age_hours       = 48

[arb]
enabled             = true
start_mint          = "WSOL"
start_amount        = 0.005
min_profit_lamports = 10000
max_hops            = 3

[execution]
compute_unit_limit = 400000
compute_unit_price = 200000
skip_preflight     = true

[alerts]
telegram_bot_token    = ""
telegram_chat_id      = ""
discord_webhook_url   = ""
desktop_notifications = true
```

---

## Running

### Dashboard (recommended)

```bash
# Full mode (requires config.toml + wallet + SCEMA tokens)
cargo run --release --bin dashboard

# Demo mode (no keypair or RPC needed — simulated data)
cargo run --release --bin dashboard -- --demo
```

### Standalone bots

```bash
cargo run --release --bin sniper
cargo run --release --bin arb
```

### Scematica Protocol (x402 API server)

```bash
cargo run --release --bin scematica-protocol -- \
  --pay-to YOUR_WALLET_ADDRESS \
  --price-lamports 10000 \
  --bind 0.0.0.0:4020
```

---

## Dashboard Navigation

Navigate tabs with `Tab` / `Shift+Tab` or `→` / `←`.

### Tab 0 — Overview

Live stats: SOL balance, SCEMA balance, open positions, session PnL, trade counts, NN agent status (ε, steps, Q-value chart), last 5 alerts.

### Tab 1 — Trades

Scrollable trade history.

| Key | Action |
|-----|--------|
| `x` | Export trades to CSV |

### Tab 2 — Logs

Live log stream from sniper.

| Key | Action |
|-----|--------|
| `e` | Toggle **Sell Mode** — pause buys, sell all positions |
| `d` | Toggle **Dump Mode** — force-sell everything at zero slippage |
| `/` | Log filter (type to search, `Esc` to exit) |

### Tab 3 — Control

| Key | Action |
|-----|--------|
| `s` | Start **Sniper** |
| `a` | Start **Arb** |
| `b` | Start **Both** |
| `x` | **Stop** all bots |
| `1` | Rate: **Bearish** — 0.3×, TP 30%, SL 8% |
| `2` | Rate: **Micro** — 0.1×, TP 40%, SL 10% (tiny wallets) |
| `3` | Rate: **Safe** — 0.5×, TP 50%, SL 10% |
| `4` | Rate: **Balanced** — 1.0×, TP 100%, SL 15% (default) |
| `5` | Rate: **Aggressive** — 2.0×, TP 200%, SL 25% |
| `6` | Rate: **Degen** — 4.0×, TP 300%, SL 40% |
| `7` | Rate: **Bullish** — 6.0×, TP 500%, SL 50% |
| `g` | Builder: **Growth** — 0.2 SOL · mild geometric scaling 1.0–2.0× |
| `j` | Builder: **Builder** — 1.0 SOL · geometric compounding 1.5–3.5×, TP scales with distance |
| `k` | Builder: **SuperBuilder** — 3.0 SOL · parabolic compounding 2.0–8.0×, auto moon-chase early |
| `o` | Builder: **Off** — config.toml values |
| `m` | Toggle **Moon Chase** — aggressive momentum params |

### Tab 4 — Chat

AI assistant (requires `GROQ_API_KEY` or `XAI_API_KEY`).

### Tab 5 — Radar

Pool age-vs-size scatter heatmap.

### Global keys

| Key | Action |
|-----|--------|
| `q` / `Esc` | Quit |
| `Ctrl+C` | Force quit |

---

## Neural Network Agent (scematica-nn)

The Dueling Deep Q* agent (v1.1.0) runs inside the sniper process.

**Architecture:** Shared trunk (STATE_DIM=24 → 128 → 64) → separate V(s) head (scalar) + A(s,a) head (5 actions). Q(s,a) = V(s) + A(s,a) − mean(A).

**State (24 features):** pool age, liquidity, price change, volume, buy/sell ratio, LP burned, mint renounced, PnL %, position age, daily PnL, win/loss streaks, SOL balance, regime, volatility, spread, time-of-day, open positions, **peak PnL, pool score, deployer rug rate, volume velocity, price velocity, price acceleration**.

**Actions:** `Hold`, `Buy`, `BuyAgg`, `SellPartial`, `SellAll`

**Training:** Double DQN (online selects, target evaluates) + Dueling decomposition. N-step returns (n=5). Prioritized experience replay (10k buffer, batch=64). ε-greedy: 1.0 → 0.05, decay 0.9995.

**Buy gating:** When ε < 0.3, reads `scematica-nn-stats.json` to gate buys: BuyAgg=1.5×, Hold=0.5×, SellAll=skip.

**Tournament:** 3 variants (conservative / balanced / aggressive) run in parallel. Highest total_reward promotes every 1,000 steps. Losers mutate ±20% lr, ±0.005 ε-decay/γ.

---

## Risk Subsystems

| Module | Trips when | Effect |
|---|---|---|
| Daily loss limit | session SOL loss > `daily_loss_limit_sol` | Pause buys |
| Max drawdown | wallet < (1 − `max_drawdown_pct`) × start | Sell Mode |
| ATH drawdown | wallet < (1 − `ath_drawdown_pct`) × ATH | Pause buys |
| Grief breaker | window loss > `grief_loss_limit_sol` | Pause buys |
| Session heat | N losses in window | Pause for cooldown |
| Portfolio heat | `max_concurrent_positions` reached | Skip new buys |
| SOL floor | balance < `min_sol_reserve` + buy amount | Skip buy |

---

## Sell Mechanics

Each position gets its own sell monitor task. Exit triggers (first to fire wins):

1. **Take profit** — `current_value ≥ entry × (1 + dynamic_tp_pct/100)`
2. **Stop loss** — `current_value ≤ entry × (1 − stop_loss_pct/100)`
3. **Trailing stop** — `current_value ≤ peak × (1 − trailing_stop_loss_pct/100)`
4. **Profit lock** — after N consecutive above-entry checks, SL floor → entry × 0.98
5. **Tiered partial TP** — sells % of remaining position at each trigger level
6. **Flash crash** — single-check drop ≥ `flash_crash_pct` from entry
7. **Dump detection** — 3 consecutive declining checks (post fast phase)
8. **Velocity-decay exit** — momentum second derivative negative in profit
9. **Adaptive pullback** — `peak − current ≥ θ_eff` (θ scales with peak height)
10. **Volume exhaustion** — volume < `volume_exhaustion_pct` × entry volume (while in profit)
11. **Whale exit** — vault drops > `whale_exit_vault_drop_pct` in a single check
12. **Sell/Dump Mode** — operator-triggered immediate exit
13. **Position time cap** — `max_position_hold_mins` hard limit
14. **Window expiry** — `price_check_duration_ms` elapsed

Sell retries escalate slippage across 4 rounds (3s total): normal → 2× → min_out=0 → min_out=0 final. Pool-drained check before each round writes an immediate loss event and skips remaining attempts.

---

## Backtesting

```bash
cargo run --release --bin backtest -- --pools historical-pools.jsonl --tp 100 --sl 15
```

Reports: pools evaluated / passed filters / win rate / avg win % / avg loss % / expected value.

---

## Alerts

Every confirmed buy/sell fans out in parallel to:
- **Telegram** — bot token + chat ID
- **Discord** — webhook URL
- **Windows desktop** — balloon notification (Works on Win 10/11)

---

## Scematica Protocol (x402)

A Rust-native HTTP 402 payment server. Clients pay micro-SOL per API call via the `X-Payment` header.

**Paid endpoints:** `/signals/pools`, `/signals/trades`, `/stats/nn`, `/stats/metrics`

**Free endpoints:** `/health`, `/supported`

---

## Security

- Private keys never leave your machine
- SCEMA gate checks retry up to 5 times (`SCEMATICA_SKIP_GATE=1` for RPC outages)
- Arb uses `scematica-swap` on-chain program: profit-or-revert
- Protocol server validates every payment before settling
- Arb skips stale quotes (>800ms) and requires profit ≥ tx_fee × 3

---

## License

MIT — see [LICENSE](LICENSE).
