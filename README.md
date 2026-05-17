# Scematica v0.8.0

**CA: AbKiP2Jc6nM7937jTDfqoJC1bsg5FQ24Buk2iqRFpump**

Autonomous AI trading infrastructure for Solana. Token sniping, cross-DEX arbitrage, deep Q-learning reinforcement, and a Rust-native x402 monetization protocol — unified under a real-time TUI dashboard.

---

## What's New in v0.8.0

- **Loss cooldown removed.** The bot no longer pauses buys after consecutive
  losses. Streak tracking still runs for the reputation ledger + dashboard
  display, but `cooldown_after_losses` and `cooldown_minutes` are now no-ops
  (retained in config for back-compat).
- **Evaluation strategies ~30 % stricter** across the board to bias toward
  win-rate over opportunity volume:
  - **PoolScorer bands tightened**: ultra-fresh shrank 10s → 7s, sweet-spot
    size narrowed 5–30 SOL → 6.5–22 SOL, stale-pool penalty grew from -30
    to -38, future-dated open_time penalised -32 (was -25).
  - **Filter defaults**: `consecutive_matches` 1 → 2, `check_duration_ms`
    8000 → 5600, `min_pool_size` 0.3 → 0.4, `max_price_impact_pct` 5 → 3.5,
    `max_top10_holder_pct` 95 → 67, `max_deployer_rugs_24h` 3 → 2.
  - **Liquidity momentum** + **cross-pool rug correlation** now ON by default
    (cheap to evaluate, sharp signal).
  - `min_pool_score` default 0 → **45** — the sharpened scorer now actually
    gates buys; pools with neither a freshness nor sweet-spot bonus are skipped.
  - **dQ\* reward shaping**: fast-win window 2 → 1 step, mid-loss band -10..0
    → -7..0, rug-magnitude penalty pulled forward to ≤-35 (was -50) with
    multiplier 3.0× → 3.2× and flat -55 (was -50).

## What's New in v0.7.0

- **Profit-first growth doctrine.** New `profit_first_mode` (on by default) gates
  the stop-loss while the wallet is below `wallet_target_sol` (default 0.2 SOL).
  Positions only exit at TP, partial TP, or the rug-only `profit_first_floor_pct`
  (default -50%). The bot books wins before tolerating drawdown — the dump
  detector and the window-expiry force-sell both honour the gate. Once the
  wallet crosses target, normal SL behaviour resumes.
- **Builder-mode ladder** — three growth targets, hot-toggle from the dashboard
  Config tab:
  - `[g] Growth` — 0.2 SOL target (matches the default)
  - `[j] Builder` — 1.0 SOL target
  - `[k] SuperBuilder` — 3.0 SOL target **plus progressive rate-mode scaling**
    (position size grows linearly from 1.0× → 2.5× as the wallet climbs from
    0% → 100% of target)
  - `[o] Off` — fall back to the config-file value
- **Sharper Pool Scorer.** Tighter freshness gradient (ultra-fresh ≤10s gets
  +30, anything >10min is -30), tightened size sweet spot (5–30 SOL = +18,
  outside that the rug surface is too high), and a clock-skew guard for
  future-dated `open_time` values.
- **Refined Deep Q* reward shaping.** Fast small wins (≤2 mins) earn a 1.5×
  multiplier, rug-magnitude losses (≤-50 %) get a 3× penalty + flat -50 so
  the agent learns to avoid the *selection*, not just the exit. Capped hold
  penalty so a long winning hold isn't drowned out.
- **Radar fixed.** Switched to log-scaled Y axis (1 → 1000 SOL fits without
  clamping), X axis now uses "seconds since sniper observed" instead of the
  unreliable `pool.open_time`, table sorts by newest first.

## What's New in v0.6.0

- **Expanded rate-mode ladder** — seven presets covering every wallet size and risk
  appetite: `Bearish → Micro → Safe → Balanced → Aggressive → Degen → Bullish`.
  `Micro` (0.1× multiplier, ~0.001 SOL/trade) is sized for wallets with only
  $1–2 of SOL — enough to actually place a buy after fees and ATA rent.
- **Deep Q* agent now actually trains.** The NN observer was reading the wrong
  keys (`action`/`pnl_sol`) from `scematica-trades.jsonl` while the writer was
  serialising `kind`/`pnl` — every SELL was silently skipped, so the dashboard
  panel stayed pinned at ε=1.0, steps=0, replay=0 forever. Fixed the field
  contract, added `pnl_pct` and `position_age_secs` to `TradeEvent` so the
  agent gets real reward signal, dropped the stats flush from 30 s → 5 s, and
  made the file writes atomic (`tmp` + rename).
- **Hot dashboard updates.** NN panel refreshes on every dashboard tick now
  that the sniper flushes stats six times faster.

## What's New in v0.5.0

v0.5.0 lands 36 new features across detection, risk, AI, and execution. Headline additions:

**Trading intelligence**
- **Kelly position sizing** (`kelly.rs`) — fractional Kelly multiplier `f* = (p·b − q) / b` clamped to `[0.25, 3.0]`, computed from rolling trade history.
- **Pool predictive scorer** (`pool_scorer.rs`) — 0–100 score from pool age + quote-vault size; sniper rejects below `min_pool_score`.
- **Pump dump exit** — 3-consecutive-decline detector in the fast-poll phase triggers immediate exit.
- **Cross-pool deployer correlation** — rejects pools from deployers with >N rugs in the persistent ledger (`scematica-deployer-reputation.json`).
- **Time-of-day weighting** (`day_weight.rs`) — 1.3× during 14–17 UTC peak hours, 0.7× overnight (0–5 UTC).
- **Gas war mode** — escalates `compute_unit_price` toward `gas_war_max_cu_price` when pools arrive in rapid succession.
- **Liquidity momentum filter** — rejects pools whose quote vault isn't growing between checks.
- **Pool staleness gate** — skips pools opened >5 min ago.

**Risk & portfolio**
- **Grief-loss circuit breaker** (`grief_breaker.rs`) — 5-min sliding-window cumulative-loss halt.
- **ATH drawdown watermark** (`ath_tracker.rs`) — pauses buys when wallet drops `ath_drawdown_pct` below all-time-high.
- **Profit extraction scheduler** — auto-sweeps a configurable % of profit to a cold wallet once session PnL exceeds threshold.
- **Holder-concentration check** — rejects pools where top-10 holders hold more than `max_top10_holder_pct` of supply.
- **Portfolio heat limit** — `max_concurrent_positions` caps in-flight risk.
- **Daily loss limit + max drawdown** — separate halt conditions on absolute SOL loss and % equity drop.

**Signal sources**
- **Pump.fun graduation monitor** (`pumpfun.rs`) — polls bonding-curve accounts; emits a synthetic `NewPool` when within 10 SOL of the ~85 SOL graduation threshold.
- **Jupiter price discrepancy filter** (`jup_oracle.rs`) — only buys when the pool price is at least `jupiter_min_premium_pct` below Jupiter's reference price.
- **Multi-RPC failover** (`multi_rpc.rs`) — latency-ranked round-robin across `extra_rpc_endpoints`; `update_latencies()` re-elects the fastest endpoint as primary.
- **Whale copy-trading** (`whale_copy.rs`) — `logsSubscribe` on configured wallets; their Raydium activity emits `NewPool` for the sniper pipeline.
- **Adaptive slippage** / **Sandwich shield** config flags for Jito-protected routing.

**AI & learning**
- **Regime-aware NN branching** — separate `(online, target)` Q-network pairs per regime (`bull` / `bear` / `sideways` / `panic`); engaged when `epsilon < 0.3` and a known regime is set.
- **Adversarial scenario injection** — every 100 train steps, synthetic rug/pump/honeypot transitions are pushed into the replay buffer.
- **Explainable decisions** — `select_action_with_reason()` exposes Q-values + a human-readable `top_reason`.
- **Multi-agent tournament** (`tournament.rs`) — 3 hyperparameter variants (conservative / balanced / aggressive) run in parallel; the highest `total_reward` agent is promoted every 1,000 steps. Persisted in `scematica-nn-tournament.json`.

**Infrastructure**
- **Backtesting engine** (`backtester.rs` + `bin/backtest.rs`) — replays JSONL pool history through the filter pipeline; reports win rate, avg win/loss, and expected value.
- **Pool radar tab** (Tab 5) — age-vs-size scatter heatmap.
- **AlertManager** (`alerts.rs`) — Telegram + Discord webhook + Windows desktop toast on every buy/sell.
- **Dynamic fee escalation** in the executor pipeline.
- **Pool cache** persisted to `pool-cache.json` and pre-seeded from on-chain state via `tools/pool-seeder`.
- **Scematica Protocol (x402)** — Rust-native HTTP 402 payment server for monetizing signals (see below).

---

## Architecture

Scematica is a Rust workspace with 8 active crates:

| Crate | Binary | Purpose |
|---|---|---|
| `scematica-core` | — | Shared config, RPC, wallet, metrics, types |
| `scematica-sniper` | `sniper` | Raydium pool sniping with filter pipeline and sell mechanics |
| `scematica-arb` | `arb` | Cross-DEX arbitrage graph search (Raydium / Orca / Meteora) |
| `scematica-executor` | — | Multi-DEX swap execution layer, Jupiter integration |
| `scematica-ai` | — | LLM agents: Risk, Arb, Debate, Strategy, Report, Chat |
| `scematica-nn` | — | Deep Q* reinforcement learning agent |
| `scematica-dashboard` | `dashboard` | Ratatui TUI: monitor, control, AI chat |
| `scematica-protocol` | `scematica-protocol` | Rust-native x402 HTTP payment protocol for Solana |

Tools (non-bot utilities):
- `tools/key-converter` — Convert keypair formats
- `tools/pool-seeder` — Pre-seed pool cache from on-chain data

The `programs/scematica-swap` Anchor program must be built and deployed separately with `anchor build`.

---

## Prerequisites

- [Rust](https://rustup.rs/) (stable, 1.75+)
- [Solana CLI](https://docs.solana.com/cli/install-solana-cli-tools) (for keypair generation)
- A Solana wallet keypair (`~/.config/solana/id.json` or any path)
- At least **250,000 SCEMA** tokens in your wallet (token-gated access — CA above)
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
#   target/release/sniper.exe
#   target/release/arb.exe
#   target/release/dashboard.exe
#   target/release/scematica-protocol.exe
```

> **Disk space note:** Release builds generate ~5-10 GB of artifacts. Run `cargo clean` periodically to reclaim space.

---

## Configuration

### Environment file (`.env`)

Create a `.env` in the repo root with sensitive keys:

```env
# RPC (can also be set in config.toml)
RPC_ENDPOINT=https://mainnet.helius-rpc.com/?api-key=YOUR_KEY
RPC_WS_ENDPOINT=wss://mainnet.helius-rpc.com/?api-key=YOUR_KEY

# AI (optional — enables Strategy agent and Chat tab)
GROQ_API_KEY=gsk_...
# or
XAI_API_KEY=xai-...

# Scematica gate bypass (emergency only)
# SCEMATICA_SKIP_GATE=1
```

### `config.toml` reference

Full annotated example:

```toml
[rpc]
endpoint    = "https://mainnet.helius-rpc.com/?api-key=YOUR_KEY"
ws_endpoint = "wss://mainnet.helius-rpc.com/?api-key=YOUR_KEY"
commitment  = "confirmed"   # confirmed | finalized | processed

[wallet]
# Supports: local file path, WSL UNC path, or base58 private key string
keypair_path = "C:\\Users\\you\\.config\\solana\\id.json"
# keypair_path = "\\\\wsl$\\Ubuntu\\home\\user\\.config\\solana\\id.json"

# ─── Sniper ───────────────────────────────────────────────────
[sniper]
enabled            = true
quote_mint         = "WSOL"     # WSOL or USDC
quote_amount       = 0.01       # SOL per snipe (scaled by rate mode)
buy_slippage_pct   = 1.0
sell_slippage_pct  = 20.0       # wider = faster exit on thin pools
take_profit_pct    = 100.0      # sell all when up 100%
stop_loss_pct      = 15.0       # sell all when down 15%
trailing_stop_loss_pct = 8.0    # trail 8% below peak (0 = use fixed SL)
partial_tp_pct     = 50.0       # sell 50% of position at partial TP
partial_tp_trigger = 60.0       # trigger partial TP when up 60%
price_check_interval_ms  = 1000 # sell monitor poll rate
price_check_duration_ms  = 180000  # 3 min total monitor window
max_sell_retries   = 5
max_buy_retries    = 3
auto_sell          = true
one_token_at_a_time = true      # sequential snipes (safer)
max_buys           = 10         # auto sell-mode after N buys; 0 = unlimited
max_concurrent_positions = 5    # max open positions
cooldown_after_losses = 3       # pause buys after N consecutive losses
cooldown_minutes   = 20
daily_loss_limit_sol  = 0.05    # halt if daily loss exceeds X SOL
max_drawdown_pct   = 30.0       # halt if wallet down X% from session start
blacklist_path     = "blacklist.txt"
copy_wallets       = []         # whale wallet addresses for copy-trade listener

# ── Kelly position sizing ────────────────────────────────────
kelly_sizing       = false      # scale quote_amount by fractional Kelly
kelly_fraction     = 0.25       # 0.25 = quarter-Kelly (conservative)
kelly_lookback     = 20         # recent trades used for win-rate estimate

# ── Pool predictive scoring ──────────────────────────────────
min_pool_score     = 0.0        # 0–100; 0 = disabled. Pools below score are skipped

# ── Gas war mode ─────────────────────────────────────────────
gas_war_mode       = false      # escalate CU price on rapid pool bursts
gas_war_max_cu_price = 2_000_000  # ceiling in micro-lamports

# ── ATH drawdown watermark ───────────────────────────────────
ath_drawdown_pct   = 0.0        # pause buys at X% below session ATH (0 = disabled)

# ── Grief-loss circuit breaker ───────────────────────────────
grief_loss_window_secs = 300    # sliding window (5 min default)
grief_loss_limit_sol   = 0.0    # halt if window loss exceeds X SOL (0 = disabled)

# ── Time-of-day weighting ────────────────────────────────────
time_of_day_weighting  = false  # 1.3x peak / 0.7x overnight UTC

# ── Profit extraction ────────────────────────────────────────
profit_extraction_threshold_sol = 0.0  # trigger sweep when session PnL exceeds X SOL
profit_extraction_pct           = 0.0  # % of profit to send to cold wallet
profit_extraction_wallet        = ""   # destination address

# ── Multi-RPC failover ───────────────────────────────────────
extra_rpc_endpoints = []        # additional RPC URLs for latency-ranked failover

# ── Execution protection ─────────────────────────────────────
adaptive_slippage  = false      # tune slippage by recent sell success rate
sandwich_shield    = false      # switch to Jito bundle routing on sandwich pattern

[sniper.filters]
check_interval_ms    = 1000
check_duration_ms    = 12000    # max time to wait for filter pass
consecutive_matches  = 1
check_mint_renounced = true
check_freezable      = true
check_burned         = true
check_mutable        = true
check_socials        = false
check_name           = true     # reject scam/rug keywords in token name
min_pool_size        = 5.0      # minimum pool SOL reserve
max_pool_size        = 0.0      # 0 = no limit
check_liquidity_depth = true
max_price_impact_pct  = 5.0     # reject if our buy moves price >5%
check_volume         = false    # require recent txn activity
min_volume_txns      = 3

# Holder concentration
check_holder_concentration = true
max_top10_holder_pct       = 70.0  # reject if top-10 wallets own > this %

# Liquidity momentum (quote vault must be growing)
check_liquidity_momentum   = false
liquidity_momentum_pct     = 5.0   # required % growth between checks

# Cross-pool deployer correlation (rug history)
check_cross_pool_correlation = false
max_deployer_rugs_24h        = 3   # reject deployers with > N historical rugs

# Jupiter price discrepancy (buy when AMM is cheaper than Jupiter)
check_jupiter_discrepancy    = false
jupiter_min_premium_pct      = 5.0 # require Jup price ≥ AMM price + X%

# ─── Arbitrage ────────────────────────────────────────────────
[arb]
enabled             = true
start_mint          = "WSOL"
start_amount        = 0.005
min_profit_lamports = 10000
max_hops            = 3
dexes               = ["Raydium", "Orca", "Meteora"]
pool_dir            = "pools"
amount_levels       = 4

# ─── Execution ────────────────────────────────────────────────
[execution]
executor           = "default"  # default | jito
custom_fee_sol     = 0.001
compute_unit_limit = 400000
compute_unit_price = 200000
skip_preflight     = true
jito_url           = "https://mainnet.block-engine.jito.wtf"

# ─── Alerts ───────────────────────────────────────────────────
[alerts]
telegram_bot_token    = ""      # leave empty to disable
telegram_chat_id      = ""
discord_webhook_url   = ""
desktop_notifications = true    # Windows toast on buy/sell
```

---

## Running

### Dashboard (recommended entry point)

```bash
# Full mode (requires config.toml + wallet)
cargo run --release --bin dashboard

# Demo mode (no keypair or RPC needed — simulated data)
cargo run --release --bin dashboard -- --demo
```

### Bots standalone

```bash
cargo run --release --bin sniper
cargo run --release --bin arb
```

### Scematica Protocol (x402 API server)

```bash
cargo run --release --bin scematica-protocol -- \
  --pay-to YOUR_WALLET_ADDRESS \
  --price-lamports 10000 \
  --bind 0.0.0.0:4020 \
  --keypair ~/.config/solana/id.json
```

---

## Dashboard Navigation

The dashboard has 5 tabs. Navigate with `Tab` / `Shift+Tab` or `→` / `←`.

### Tab 0 — Overview

Live stats panel: SOL balance, SCEMA balance, wallet address, open positions, session PnL, trade counts, and NN agent status (epsilon, total steps, win rate).

No interactive keys on this tab.

### Tab 1 — Trades

Scrollable trade history table (buy/sell events from `scematica-trades.jsonl`).

| Key | Action |
|-----|--------|
| `x` | Export trades to CSV (`scematica-trades-YYYYMMDD.csv`) |

### Tab 2 — Logs

Live log stream (tails `scematica-sniper.log` + dashboard internal events).

| Key | Action |
|-----|--------|
| `e` | Toggle **Sell Mode** — pauses all buys, sells all open positions |
| `d` | Toggle **Dump Mode** — force-sells everything at zero slippage |
| `/` | Activate log filter (type to search, `Backspace` to clear, `Esc` to exit filter) |

### Tab 3 — Control

Bot process control and rate mode selection.

| Key | Action |
|-----|--------|
| `s` | Start **Sniper** only |
| `a` | Start **Arb** only |
| `b` | Start **Both** (sniper + arb) |
| `x` | **Stop** all bots |
| `1` | Rate mode: **Bearish** — 0.3x, ~0.003 SOL/trade, TP 30%, SL 8% |
| `2` | Rate mode: **Micro** — 0.1x, ~0.001 SOL/trade, TP 40%, SL 10% (≈$1–2 wallets) |
| `3` | Rate mode: **Safe** — 0.5x, ~0.005 SOL/trade, TP 50%, SL 10% |
| `4` | Rate mode: **Balanced** — 1.0x, ~0.010 SOL/trade, TP 100%, SL 15% (default) |
| `5` | Rate mode: **Aggressive** — 2.0x, ~0.020 SOL/trade, TP 200%, SL 25% |
| `6` | Rate mode: **Degen** — 4.0x, ~0.040 SOL/trade, TP 300%, SL 40% |
| `7` | Rate mode: **Bullish** — 6.0x, ~0.060 SOL/trade, TP 500%, SL 50% |
| `g` | Builder mode: **Growth** — wallet target 0.2 SOL |
| `j` | Builder mode: **Builder** — wallet target 1.0 SOL |
| `k` | Builder mode: **SuperBuilder** — wallet target 3.0 SOL + progressive scaling |
| `o` | Builder mode: **Off** — fall back to `config.toml` `wallet_target_sol` |

The seven rate modes form a ladder from least to most aggressive — each
multiplier scales `quote_amount` (default 0.01 SOL) and rewrites live TP/SL
in the running sniper via `scematica-rate-mode.json`.

The four builder modes are orthogonal: they set the wallet-growth target that
profit-first mode uses to decide when the bot is "in build-up" (SL gated to
the rug-only floor) vs "at target" (normal SL). **SuperBuilder** additionally
applies a 1.0× → 2.5× progressive multiplier to position sizing as the wallet
grows toward 3 SOL, so winning streaks compound automatically. Cleared by
pressing `o` or by deleting `scematica-builder-mode.json`. No restart required.

### Tab 4 — Chat

AI assistant powered by Groq (Llama) or xAI (Grok). Requires `GROQ_API_KEY` or `XAI_API_KEY`.

| Key | Action |
|-----|--------|
| Type | Compose message |
| `Enter` | Send message |
| `Backspace` | Delete character |
| `y` | Confirm a pending AI action (shown when bot proposes a trade) |
| `n` | Reject a pending AI action |

### Tab 5 — Radar

Pool radar: age-vs-size scatter heatmap rendered from the live pool stream. Helps spot bursts of suspiciously thin pools or whale-backed launches at a glance. No interactive keys.

### Global keys

| Key | Action |
|-----|--------|
| `q` / `Esc` | Quit dashboard |
| `Tab` / `→` | Next tab |
| `Shift+Tab` / `←` | Previous tab |
| `Ctrl+C` | Force quit |

---

## Sell Mechanics

The sniper uses a two-phase sell monitor per position:

1. **Fast phase** (first 20 checks × 100ms): catches rapid dumps immediately after buy
2. **Slow phase** (remaining time × `price_check_interval_ms`): standard monitoring

Each iteration:
- Re-reads `live_params` for dynamic TP/SL (updated by rate mode or config hot-reload)
- Checks trailing stop — resets peak price on new highs
- Triggers partial TP at `partial_tp_trigger`% gain (sells `partial_tp_pct`% of position)
- Detects dump: 3 consecutive declining prices in fast phase → immediate exit
- Falls back to AMM constant-product price: `out = (reserve_out × in × 9975) / (reserve_in × 10000 + in × 9975)`

**Emergency controls:**
- **Sell Mode** (`e` key or `scematica-sell-mode.json`): pauses all buys, sell-scans all wallet positions
- **Dump Mode** (`d` key or `scematica-dump-mode.json`): `min_out = 0`, retries every 30s until all positions are gone
- **Max drawdown guard**: auto-activates sell mode if wallet drops `max_drawdown_pct` from session start
- **Daily loss limit**: halts new buys if daily SOL loss exceeds `daily_loss_limit_sol`

---

## Neural Network Agent (scematica-nn)

The Deep Q* agent runs inside the sniper process and learns from every completed trade.

- **MLP**: STATE_DIM(18) → 128 → 64 → ACTION_DIM(5), He init, ReLU hidden, linear output
- **State**: PnL%, position age, price momentum, win/loss streaks, liquidity score, open positions, regime, volatility, spread, time-of-day, SOL balance, …
- **Actions**: `Hold`, `Buy`, `BuyAgg`, `SellPartial`, `SellAll`
- **Double DQN**: online net selects next action; target net evaluates it (reduces Q-overestimation). Target sync every 200 steps.
- **Reward shaping**: +PnL% × win/loss multiplier − hold penalty
- **Replay**: 10,000-transition buffer, uniform random sampling, batch=64
- **Epsilon-greedy**: starts 1.0, decays 0.9995/step, floor 0.05
- **Checkpoints**: saved to `scematica-nn-agent.json` every 10 min
- **Stats**: written to `scematica-nn-stats.json` every 30s (visible in Overview tab)

### Regime-aware branching

When `epsilon < 0.3` and the active regime is recognised, the agent routes Q-value lookups through a regime-specific `(online, target)` Q-network pair (`bull` / `bear` / `sideways` / `panic`). Each regime accumulates its own weights; this lets the agent specialise without forgetting.

### Adversarial scenario injection

If `auto_inject_adversarial` is set, every 100 training steps the agent synthesises a small batch of rug / pump / honeypot transitions and inserts them into the replay buffer. This stress-tests learned policies against tail outcomes that natural play under-samples.

### Explainable trade decisions

`select_action_with_reason()` returns the chosen action plus the full Q-value vector and a human-readable `top_reason` string (e.g. "Q(SellAll)=1.42 > Q(Hold)=0.91 — pnl_pct=42%, position_age=18s"). Surfaced in the dashboard logs and trade history.

### Multi-agent tournament

`AgentTournament` runs 3 hyperparameter variants in parallel against the same transition stream:

| Variant | epsilon_decay | learning_rate | gamma |
|---|---|---|---|
| conservative | 0.9999 | 5e-4 | 0.95 |
| balanced (default) | 0.9995 | 1e-3 | 0.99 |
| aggressive | 0.999 | 2e-3 | 0.95 |

Every 1,000 transitions the highest `total_reward` agent is promoted to primary; state is persisted in `scematica-nn-tournament.json`.

---

## Signal Sources

The sniper accepts pool candidates from multiple producers, all merged into one `ListenerEvent::NewPool` stream:

1. **Raydium AMM V4 listener** — primary `logsSubscribe` on `initialize2` instructions.
2. **Pump.fun monitor** (`pumpfun.rs`) — polls program accounts every 30 s; emits a synthetic pool when a bonding curve is within 10 SOL of the ~85 SOL graduation threshold.
3. **Whale copy-trading** (`whale_copy.rs`) — `logsSubscribe` on each configured `copy_wallets[]`; their Raydium activity yields a synthetic pool for filter validation.
4. **Pool cache pre-seed** — `tools/pool-seeder` scans on-chain state and pre-populates `pool-cache.json` so the sell-side lookup works from cold start.

All candidates flow into the same filter pipeline; downstream code doesn't distinguish by source.

---

## Risk Subsystems

Independent breakers, each can be enabled or disabled in `config.toml`. They evaluate on every buy attempt; a single trip pauses buys until cleared.

| Module | Trips when | Effect |
|---|---|---|
| Daily loss limit | session SOL loss > `daily_loss_limit_sol` | Pause buys |
| Max drawdown | wallet < `(1 − max_drawdown_pct) × session_start_balance` | Activate Sell Mode |
| ATH drawdown (`ath_tracker.rs`) | wallet < `(1 − ath_drawdown_pct) × session_ATH` | Pause buys |
| Grief breaker (`grief_breaker.rs`) | cumulative loss in last `grief_loss_window_secs` > `grief_loss_limit_sol` | Pause buys |
| Cooldown after losses | `cooldown_after_losses` consecutive losses | Pause buys for `cooldown_minutes` |
| Portfolio heat | `max_concurrent_positions` reached | Skip new buys |

### Profit extraction scheduler

When session PnL exceeds `profit_extraction_threshold_sol`, `profit_extraction_pct` of the profit is swept to `profit_extraction_wallet` automatically. Combined with the breakers above, this turns the bot into a self-managing capital allocator: it reduces exposure on losses *and* protects realised gains on wins.

---

## Backtesting

```bash
cargo run --release --bin backtest -- --pools historical-pools.jsonl --tp 100 --sl 15
```

The backtester (`crates/scematica-sniper/src/backtester.rs`) replays a JSONL file of `BacktestPool` records (one per line) through the static portion of the filter pipeline and a simple TP/SL simulator. It reports:

- Pools considered / passed filters / simulated buys
- Win rate, average win %, average loss %
- Expected value: `win_rate × avg_win − (1 − win_rate) × avg_loss`

RPC-bound filters (mint renounce, freeze, LP burn, social links) are skipped — backtesting validates static signal quality and TP/SL choices, not RPC state.

---

## Alerts (AlertManager)

`scematica-sniper/src/alerts.rs` fans every buy/sell event to all enabled channels in parallel:

- **Telegram** — `telegram_bot_token` + `telegram_chat_id`; Markdown-formatted body
- **Discord** — `discord_webhook_url`; embed with title + green accent
- **Windows desktop** — `desktop_notifications = true`; `System.Windows.Forms.NotifyIcon` balloon (no WinRT, works on Win10/11). PowerShell stdout/stderr are nulled so the log panel stays clean.

---

## Scematica Protocol

A Rust-native implementation of the [x402 HTTP payment standard](https://github.com/x402-foundation/x402) for Solana.

Clients pay per API call with a micro-SOL transfer embedded in the `X-Payment` request header. No subscription, no API key — just pay-per-use on-chain.

### Paid endpoints

| Route | Description |
|---|---|
| `GET /signals/pools` | Live pool signals from the sniper stream |
| `GET /signals/trades` | Recent trade events |
| `GET /stats/nn` | NN agent performance stats |
| `GET /stats/metrics` | Bot metrics snapshot |

### Free endpoints

| Route | Description |
|---|---|
| `GET /health` | Liveness check |
| `GET /supported` | Payment requirements (asset, amount, destination) |

### How it works

1. Client requests a paid route → server returns `402 Payment Required` with `X-Payment-Response` header
2. Client builds a partial SPL `TransferChecked` transaction signed by their key
3. Client base64-encodes the tx and includes it in the `X-Payment` header of the next request
4. Server verifies the partial tx (mint, destination, amount, client sig)
5. Server refreshes blockhash, signs as fee payer, submits — then returns the API response

---

## State Files

The sniper and dashboard communicate via JSON files in the working directory:

| File | Written by | Purpose |
|---|---|---|
| `scematica-sell-mode.json` | Dashboard / drawdown guard | Activates emergency sell mode |
| `scematica-dump-mode.json` | Dashboard | Activates dump mode (zero slippage) |
| `scematica-rate-mode.json` | Dashboard | Active rate mode + TP/SL/multiplier |
| `pool-cache.json` | Sniper / pool-seeder | Pool → mint mapping for sell lookups |
| `scematica-trades.jsonl` | Sniper | Trade history (append-only JSONL) |
| `scematica-metrics.json` | Sniper | Metrics snapshot, flushed every 5s |
| `scematica-strategy.json` | AI strategy agent | TP/SL/multiplier/regime snapshot |
| `scematica-sniper.log` | Sniper | Log file tailed by dashboard |
| `scematica-nn-agent.json` | NN agent | Model checkpoint (every 10 min) |
| `scematica-nn-stats.json` | NN agent | ε, steps, replay size, total reward, last action |
| `scematica-nn-tournament.json` | NN tournament | Per-variant rewards + active primary |
| `scematica-deployer-reputation.json` | Reputation ledger | Per-deployer rug/success counts (EMA-blended) |
| `scematica-filter-stats.json` | Filter pipeline | Per-filter pass/fail counts |

---

## Security

- Private keys never leave your machine
- All SCEMA gate checks retry up to 5 times before failing (set `SCEMATICA_SKIP_GATE=1` to bypass during RPC outages)
- Arbitrage uses the `scematica-swap` on-chain program with profit-or-revert: if the arb is not profitable, the transaction fails before any funds move
- The Protocol server verifies every payment before settling — partial tx is validated for correct mint, destination, and amount

---

## License

MIT — see [LICENSE](LICENSE).
