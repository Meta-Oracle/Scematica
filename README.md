# Scematica v1.11.0

**CA: AbKiP2Jc6nM7937jTDfqoJC1bsg5FQ24Buk2iqRFpump**

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

The crates version independently; the bot stack shares major **1.x**, while the
SDK and playground are pre-1.0.

| Crate | Version | Installs | Kind |
|---|---|---|---|
| `scematica-suite` | 1.11.2 | `scematica` | launcher + umbrella lib |
| `scematica-dashboard` | 1.11.2 | `dashboard` | bin + lib |
| `scematica-sniper` | 1.11.2 | `sniper`, `backtest` | bin + lib |
| `scematica-protocol` | 1.11.2 | `protocol` | bin + lib |
| `scematica-ai` | 1.11.2 | — | library |
| `scematica-executor` | 1.11.2 | — | library |
| `scematica-core` | 1.11.2 | — | library |
| `scematica-nn` | 1.13.0 | `scema-ddqn` | bin + lib |
| `scemadex-sdk` | 0.2.1 | `scemadex` | bin + lib (incl. zkML-verified bonds) |
| `scemadex-mcp` | 0.1.0 | `scemadex-mcp` | MCP server (LLM agents buy intelligence over x402) |
| `scemadex-settle` | 0.1.0 | — | devnet reference settler |
| `scema-agent-playground` | 0.1.0 | `playground` | bin |

Pin a version with `cargo install <crate> --version <x.y.z>`, or depend on a
library with `<crate> = "<x.y>"`. Library embedders who want a lean build
(no TUI deps) use `default-features = false` on `scematica-nn` / `scemadex-sdk`.

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
git clone https://github.com/Deadsg/scematica.git
cd scematica

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
