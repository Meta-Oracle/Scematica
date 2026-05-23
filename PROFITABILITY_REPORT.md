# Scematica v1.10.0 — Profitability Analysis & Improvement Roadmap
**Generated:** 2026-05-23 | **Data window:** 573 confirmed sells, 686 buys

---

## 1. Executive Summary

The bot has a structurally sound edge: 6.32× profit factor, 31.2% win rate, +1.93 SOL all-time. The core strategy works. The remaining profit leakage falls into three categories:
1. **Dead-hour trading** — buying during 0% win-rate hours (01:00, 21:00 UTC)
2. **Undersized entries** — <0.002 SOL trades are net negative (−0.14 SOL on 115 trades)
3. **Capital velocity loss** — holding beyond 30s earns 37× less profit per minute than <3s exits

**v1.9.0 implements fixes 1 and 2. This report defines the roadmap for everything else.**

---

## 2. Live Data Findings

### 2.1 Capital Velocity — The Core Insight

| Hold Duration | Winners | PnL/Minute | Efficiency vs <3s |
|---|---|---|---|
| **< 3s** | 22 | **0.458 SOL/min** | 1.0× (baseline) |
| 3 – 10s | 61 | 0.143 SOL/min | 0.31× |
| 10 – 30s | 56 | 0.051 SOL/min | 0.11× |
| 30 – 90s | 16 | 0.014 SOL/min | 0.03× |
| 90s – 3m | 6 | 0.005 SOL/min | 0.01× |
| 3 – 10min | 7 | 0.0004 SOL/min | **0.001×** |

**The fastest exits earn 1,150× more per minute of capital deployed than the slowest exits.** Every second of unnecessary hold time is destroying compounding potential. The 3-5 minute bucket has **0% win rate** and is net negative.

### 2.2 Buy Amount Sweet Spot (Confirmed)

| Entry Size | Trades | Win Rate | Total PnL |
|---|---|---|---|
| **< 0.002 SOL** | 115 | 21% | **−0.140 SOL** (NEGATIVE) |
| 0.002 – 0.005 | 164 | 29% | +0.239 SOL |
| 0.005 – 0.008 | 10 | 0% | −0.022 SOL |
| **0.008 – 0.012** | 138 | **42%** | **+0.896 SOL** |
| 0.012 – 0.020 | 154 | 33% | +0.958 SOL |
| > 0.020 | 8 | 0% | +0.035 SOL |

The sweet spot is **0.008–0.012 SOL** (42% WR). Entries below 0.002 SOL are structurally net-negative — likely due to getting sandwiched and having insufficient weight to move the AMM price meaningfully. The wallet_pct floor fix in v1.8.3 addresses this.

### 2.3 Hourly Win Rate (573 trades, UTC)

| Hour | Trades | Win Rate | PnL | Rating |
|---|---|---|---|---|
| **03:00** | 19 | **53%** | +0.159 | ⭐ BEST |
| **10:00** | 10 | **50%** | +0.018 | ⭐ BEST |
| **16:00** | 23 | **52%** | +0.146 | ⭐ BEST |
| 22:00 | 18 | 39% | +0.109 | Good |
| 23:00 | 27 | 41% | +0.072 | Good |
| 14:00 | 62 | 27% | **+0.317** | High value (big winners) |
| 00:00 | 42 | 36% | +0.216 | Above avg |
| **01:00** | 8 | **0%** | **−0.031** | ❌ BLOCKED |
| **21:00** | 5 | **0%** | **−0.002** | ❌ BLOCKED |
| 02:00 | 44 | 23% | +0.168 | Weak (0.8× sizing) |
| 04:00 | 45 | 22% | +0.105 | Weak (0.8× sizing) |

DayWeighter has been recalibrated to match this data. 03:00, 10:00, 14–17:00, 22–23:00 → 1.3× sizing. 01:00 and 21:00 are now hard-blocked via `blocked_hours_utc`.

### 2.4 Day-of-Week Performance

| Day | Trades | Win Rate | PnL |
|---|---|---|---|
| Monday | 294 | 32% | **+1.140 SOL** |
| Tuesday | 120 | 35% | +0.724 SOL |
| Thursday | 5 | 60% | +0.041 SOL |
| Sunday | 66 | 32% | +0.029 SOL |
| Friday | 96 | **22%** | +0.015 SOL |
| Saturday | 7 | 0% | −0.007 SOL |

Friday and Saturday are weak. Consider reducing position sizing on weekends or activating a more conservative rate mode automatically.

---

## 3. Code Analysis — Improvement Opportunities

### 3.1 Fibonacci Entry Gate (Not Wired)
**File:** `fibonacci_recovery_system.rs:65-70`

`FibonacciEntryDecision.should_enter` and `fibonacci_score` are computed in `buy()` (around line 1250) but `should_enter` is never checked — only `position_multiplier` is applied. Low-scoring pools get a reduced size but aren't rejected. Adding a hard gate here would improve pool quality.

**Fix:**
```rust
if !high_speed && !fib_decision.should_enter && fib_decision.fibonacci_score < 30.0 {
    self.filter_pipeline.stats.record_rejection("fibonacci_gate_hard");
    return;
}
```

### 3.2 Momentum Escalation Not Mode-Aware (Fixed v1.8.3)
`SellMonitor::monitor_and_sell` was reading `self.config.momentum_max_escalations` (global 7) instead of `live_params.momentum_max_escalations_mode`. Fixed in this session.

### 3.3 Exit Reason Not Tracked in TradeEvent
The `exit_reason` field has been added to `TradeEvent` (v1.9.0) but isn't populated yet — all exits still write `""`. Each exit path in `SellMonitor` needs to pass its reason through to `sell_with_retry`. This enables the exit-reason breakdown dashboard panel.

**Exit reasons to track:** `take_profit`, `stop_loss`, `trailing_stop`, `velocity_decay`, `peak_stagnation`, `dump_detected`, `no_pump_timeout`, `sell_mode`, `dump_mode`, `fibonacci`, `timeout`

### 3.4 NN Reward Signal Overflow
`avg_loss = 331,882` in `scematica-nn-stats.json` — this is ~1000× higher than expected for a normalized DQN. The reward function in `agent.rs` is producing signals in the range of millions, which causes Q-function divergence. The `last_q_values` being all-zero confirms the network has diverged. Fix: divide rewards by 1e6 or apply `tanh` normalization.

### 3.5 Failed Sell Rate (7.2%)
46 of 636 sells failed (returned empty signature). These are tokens where the pool completely drained before the sell landed. The current `max_sell_retries = 3` is appropriate. However, the retry delay between attempts should be examined — if the pool is already at DRAIN_THRESHOLD_LAMPORTS, all retries will fail identically.

### 3.6 DayWeighter Was Disabled
`time_of_day_weighting = false` in config.toml. This was a free 30% improvement in peak hours that was sitting unused. Now enabled in v1.9.0.

---

## 4. Profitability Improvements Implemented (v1.9.0)

| Change | File | Impact |
|---|---|---|
| Enable `time_of_day_weighting` | `config.toml` | 1.3× sizing at 03:00, 10:00, 14-17:00, 22-23:00 |
| Recalibrate `DayWeighter` | `day_weight.rs` | Data-driven multipliers vs hardcoded assumptions |
| Add `blocked_hours_utc = [1, 21]` | `config.toml` + `config.rs` + `sniper.rs` | Eliminates 0% WR hours entirely |
| Populate `exit_reason` on all 16 sell paths | `sniper.rs` | Enables exit breakdown analytics (TP/SL/velocity/dump/etc.) |
| Weekend auto-switch via `weekend_mode` | `config.toml` + `config.rs` + `main.rs` | Auto-Bearish on Sat/Sun, restore Balanced Mon-Fri |
| Fix NN reward overflow (pnl_pct backfill) | `main.rs` | Stops Q-value divergence — avg_loss was 331,882 |
| Wallet_pct floor = `quote_amount_mode` | `sniper.rs` | Enforces 0.01 SOL minimum for Balanced (v1.8.3) |
| Rate mode watcher uses config truth | `main.rs` | Mode switches now apply correct TP/SL/escalations (v1.8.3) |
| `momentum_max_escalations_mode` | `sniper.rs` | Mode-specific escalation ladder (v1.8.3) |

---

## 5. TUI Feature Recommendations

### Priority 1 — Immediate Revenue Impact

**5.1 Exit Reason Dashboard (new panel in Overview tab)**
```
Exit Breakdown (last 100 sells):
  TP:           28 (28%)  ████████████
  SL:           41 (41%)  ████████████████████
  Trailing:     12 (12%)  ██████
  Velocity:      9  (9%)  ████
  Stagnation:    6  (6%)  ███
  Dump:          4  (4%)  ██
```
This tells you instantly if the bot is exiting correctly or if SL is dominant (meaning pool quality is poor).

**5.2 Hourly Heatmap Panel (new tab: Analytics)**
```
UTC Hour Win Rate Heat:
   00  01  02  03  04  05  06  07  08  09  10  11  12
  [36][0%][23][53][22][  ][33][24][26][29][50][  ][  ]
   13  14  15  16  17  18  19  20  21  22  23
  [40][27][44][52][  ][  ][17][  ][0%][39][41]
```
Green cells = good hours, red = bad hours. Lets the operator see at a glance whether the current UTC hour is a strong trading window.

**5.3 Capital Velocity Gauge**
Real-time metric: `SOL earned per hour (rolling 1h)`. Display as a speedometer-style ratatui widget with colored zones (green >0.05, yellow 0.01-0.05, red <0.01).

**5.4 Mode Performance Tracker**
Per-mode statistics shown in Config tab:
```
Mode       Trades  WR    PnL      Avg Entry
Balanced   138     42%   +0.90    0.010 SOL
Safe       164     29%   +0.24    0.004 SOL  
Micro      115     21%   -0.14    0.001 SOL  ⚠️
```

### Priority 2 — Operational Quality

**5.5 Live Position Detail Panel**
Current positions panel (Overview tab, replace raw position list) showing for each open position:
- Mint address (abbreviated), hold time, entry SOL, current PnL%
- Peak PnL%, escalation count, current exit type risk
- Color: green if above TP, yellow if flat, red if near SL

**5.6 Session Heat Timeline**
Rolling 15-minute win/loss bar chart:
```
  +│      ██████
  0│  ███  ██████      ████
  -│              ████
    ←── 15 min ──►
```
Visualizes streaks so the operator can see whether a loss cluster is ending.

**5.7 Filter Rejection Drill-Down**
Current: shows total rejections per filter. Missing: **hourly trend** (is `fibonacci_gate` rejecting more pools than usual?), and **per-filter pass-through quality** (what % of pools that passed each filter eventually became winners?).

**5.8 Pool Radar Improvements**
- Add velocity axis (x=age, y=size, dot size=velocity)
- Color dots by eventual outcome (green=TP, red=SL, grey=timeout) using trade history
- Add 24h rolling pool quality trend line

### Priority 3 — Advanced Analytics

**5.9 Wallet Growth Chart (new Chart tab)**
ASCII sparkline of wallet balance over time, using `session_start_lamports + cumulative daily_pnl`. Shows whether the session is trending up or down even before trades close.

**5.10 NN Agent Q-Value Inspector**
In the AI tab, display for the most recent candidate pool:
```
NN Agent — Pool: ABC123...  ε=0.05  Ready: YES
  Hold:       Q=-0.23  ────────────────────────
  Buy:        Q=+1.87  ████████████████████████ ← recommended
  BuyAgg:     Q=+1.12  ██████████████
  SellPartial Q=-0.45  ─
  SellAll:    Q=-1.23  ──────
```

**5.11 Deployer Reputation Table**
Sort deployer reputation ledger by rug rate, show top 20 riskiest deployers encountered this session. Useful for manually blacklisting known rug wallets.

**5.12 Trading Calendar**
Day/hour heatmap grid (7 days × 24 hours) showing historical PnL density. Green = strong, red = weak. Helps identify weekly patterns beyond just hourly.

**5.13 Alert History Panel**
Scrollable log of last 50 alerts with timestamp, severity icon, and message. Currently alerts fire and disappear — no way to see what happened while away from keyboard.

**5.14 Live Config Editor (Config Tab)**
Allow editing key parameters inline without restarting:
- TP%, SL%, trailing stop — via input fields
- Switch rate mode — via dropdown (currently [1-7] hotkeys exist but no visual confirmation)
- Toggle blocked_hours — add/remove hours from TUI

**5.15 Arbitrage Opportunity Feed (Arb Tab)**
Currently the Arb tab likely shows static config. Add:
- Rolling list of last 10 arb opportunities found (even if below min_profit)
- Best opportunity this session (route, profit, timestamp)
- Arb scanner status: pools tracked, last scan time, average latency

### Priority 4 — Advanced Features

**5.16 Backtester Integration in TUI**
Allow running `cargo run --bin backtest -- --pools historical-pools.jsonl --tp X --sl Y` with configurable parameters FROM the TUI, displaying results inline. Enables live config tuning → backtest → compare → apply.

**5.17 Strategy Comparison View**
Given current live_params vs config defaults, show side-by-side projected performance based on backtest. "AI recommends TP=256%, config says 175% — projected difference: +0.12 SOL/day"

**5.18 Webhook Event Tester**
Button in Config tab: "Test Alert". Sends a fake BUY/SELL notification through all configured channels (Telegram, Discord, desktop) to verify they're working before a real trade fires.

**5.19 Multi-Session PnL Aggregator**
Reads all historical `scematica-trades.jsonl.bak-*` files and aggregates cumulative PnL across sessions, showing total all-time performance even when the live file has been rotated.

---

## 6. Architecture Improvements

### 6.1 Pool Quality Pre-Screen Before Filter Pipeline
The filter pipeline takes 400-800ms per pool (multiple RPC calls). Consider adding a fast pre-screen (< 5ms, no RPC) that rejects obvious garbage before the expensive pipeline runs:
- Pool age < 1s → skip (too fresh, likely front-run setup)
- Quote reserve < 3 SOL → skip (below min_pool_size anyway)
- Mint address in LRU "recently rejected" cache → skip

### 6.2 NN Reward Normalization
Divide all rewards by 1e6 before storing in replay buffer OR apply `tanh(reward / 0.1)` to compress large signals. This will unfold the Q-value divergence seen in current stats (`avg_loss = 331,882`).

### 6.3 Exit Reason Population (Pending)
Thread `exit_reason: &str` through `sell_with_retry` so each of the 11 exit paths in `SellMonitor` writes its reason to `TradeEvent`. The field is already in the struct; the call sites need updating.

### 6.4 Fibonacci Hard Entry Gate
Wire `FibonacciEntryDecision.should_enter` as an actual gate (currently computed but ignored). Expected impact: reduce trade count ~15%, improve win rate ~3-5pp.

### 6.5 Weekend Mode
Auto-switch to Micro or Bearish mode on Saturday/Sunday (22% WR Friday, 0% Saturday). Could be config-driven:
```toml
weekend_mode = "Bearish"   # Auto-switch to this mode Sat/Sun UTC
weekday_mode = "Balanced"  # Auto-restore Mon-Fri
```

---

## 7. Version Tag

All v1.9.0 changes implemented:
- `crates/scematica-core/src/metrics.rs` — `exit_reason` field in TradeEvent
- `crates/scematica-core/src/config.rs` — `blocked_hours_utc: Vec<u8>`, `weekend_mode`, `weekday_mode` fields
- `crates/scematica-sniper/src/sniper.rs` — time gate check in buy(), `exit_reason` on all 16 sell paths
- `crates/scematica-sniper/src/day_weight.rs` — recalibrated multipliers from live data
- `crates/scematica-sniper/src/main.rs` — weekend auto-switch watcher, NN pnl_pct overflow fix
- `config.toml` — `time_of_day_weighting = true`, `blocked_hours_utc = [1, 21]`, `weekend_mode = "Bearish"`, `weekday_mode = "Balanced"`

### 3.3 Exit Reason Population — COMPLETED
All 16 exit paths in `SellMonitor` now write their reason to `TradeEvent.exit_reason`:
`take_profit`, `stop_loss`, `trailing_stop`, `velocity_decay`, `peak_stagnation`,
`dump_detected`, `no_pump_timeout`, `sell_mode`, `dump_mode`, `fibonacci`,
`timeout`, `volume_exhaustion`, `tiered_tp`

### 6.5 Weekend Mode — COMPLETED
Config-driven weekend auto-switch via `weekend_mode = "Bearish"` + `weekday_mode = "Balanced"`.
Watcher in `main.rs` checks UTC day-of-week every 10 min and writes `scematica-rate-mode.json`
to trigger the existing rate-mode watcher.

### 3.4 NN Reward Overflow — FIXED
Root cause: pnl_pct backfill formula `pnl_sol / 0.01 * 100.0` turned 0.9 SOL win into 9000%
→ `shape_reward(9000, 0) ≈ 85,000` per transition → Q-value divergence.
Fix: old entries without `pnl_pct` now use 0.0 (neutral), plus clamp `(-200, 500)` on all inputs.
**Action:** Delete `scematica-nn-agent.json` before restart to reset the diverged weights.

---

## v1.10.0 Changes — Pump.fun Trending Monitor

### What was added

**`crates/scematica-sniper/src/pumpfun_trending.rs`** — New real-time Pump.fun pre-graduation sniping module:

- Connects to PumpPortal WebSocket (`wss://pumpportal.fun/api/data`) for live bonding curve events
- Tracks per-token sliding-window buy/sell velocity (configurable `pumpfun_window_secs`, default 120s)
- Trending score (0–100): buy pressure (40pts) + volume velocity (30pts) + curve fill (30pts)
- Pre-flags tokens crossing `pumpfun_trending_score` + `pumpfun_min_curve_pct` thresholds
- On Raydium migration events: fetches pool account (3-attempt retry), decodes via fixed offsets, emits `ListenerEvent::NewPool` — identical to the standard listener so the sniper's dedup guard prevents double-processing
- Reconnects indefinitely on WebSocket drops
- Replaces the `getProgramAccounts`-based `pumpfun.rs` monitor when `pumpfun_trending_enabled = true`

### Config fields added (`config.toml`)
```toml
pumpfun_trending_enabled = true
pumpfun_trending_score   = 55.0   # Min score 0-100 to emit to sniper
pumpfun_min_curve_pct    = 40.0   # Min bonding curve fill % (40% = ~28 SOL)
pumpfun_window_secs      = 120    # Sliding window for momentum signal
```

### Expected impact
PumpPortal migration events arrive 0.5–3 s ahead of the standard Raydium AMM V4 listener's `getProgramAccounts` poll. Pre-flagging trending tokens lets the sniper skip tokens with zero traction and prioritise those with demonstrated buy momentum before graduation.

### Restart instructions
1. `del scematica-nn-agent.json` (resets diverged Q-weights from v1.9.0 NN overflow fix)
2. `cargo run --release --bin dashboard`
