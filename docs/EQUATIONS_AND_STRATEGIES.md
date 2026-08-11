# Scematica — Equations, Strategies, and Algorithm Reference

> Strategy generation 1.4.0 · Crate: workspace 1.25.0 · Verified against source 2026-08-11  
> Internal technical reference for all trading equations, parameter evolution, and strategy rationale.
>
> Parameter values in this document are the **`config.rs` defaults** (verified in
> `crates/scematica-core/src/config.rs`). Live trading reads `config.toml`, which
> overrides many of them — see the note at the top of §10.

---

## Table of Contents

1. [Core Trading Philosophy](#1-core-trading-philosophy)
2. [Position Sizing](#2-position-sizing)
3. [Exit Strategy Architecture](#3-exit-strategy-architecture)
4. [Adaptive Pullback Exit](#4-adaptive-pullback-exit)
5. [Momentum Escalation](#5-momentum-escalation)
6. [Velocity Decay Exit](#6-velocity-decay-exit)
7. [Tiered Partial Take-Profit](#7-tiered-partial-take-profit)
8. [Builder Mode Compounding Algorithms](#8-builder-mode-compounding-algorithms)
9. [Risk Subsystems](#9-risk-subsystems)
10. [Parameter Evolution History](#10-parameter-evolution-history)
11. [Data-Driven Refinements](#11-data-driven-refinements)
12. [DQ* Agent Feedback Loop](#12-dq-agent-feedback-loop)

---

## 1. Core Trading Philosophy

Scematica targets Raydium AMM V4 new-pool events — tokens minted within seconds to minutes of listing. The core insight is:

> **Most tokens dump fast; a small subset run 3–10×. The edge is in holding the runners long enough while protecting the downside.**

Every equation in this system exists to solve one of two problems:
- **Hold winners longer** — resist premature exits on tokens that are still running
- **Exit losers faster** — detect dumps, rugs, and reversal signals early

These two goals are in permanent tension, and the parameter history below documents every adjustment made to balance them.

---

## 2. Position Sizing

### 2.1 Base Position Size

```
position_sol = config.buy_amount_sol × live_params.amount_multiplier
```

The `amount_multiplier` defaults to 1.0 and is modified by builder modes (Section 8). The raw `buy_amount_sol` is set in `config.toml` and represents the baseline stake per trade.

### 2.2 Kelly Criterion Sizing

Kelly provides a mathematically optimal fraction of bankroll to risk given a known win-rate and average win/loss ratio:

```
f* = W - (1 - W) / R
```

Where:
- `W` = rolling win-rate over last N trades (default lookback: 50)
- `R` = average win / average loss ratio
- `f*` = fraction of bankroll to bet

(The code writes the equivalent asymmetric form `f* = (p·b − q) / b`, with
`b = avg_win / avg_loss`, `p` = win rate, `q = 1 − p`.)

**Scematica fractional Kelly.** `KellySizer::compute_multiplier` does **not**
compute an absolute fraction of the wallet — it turns the quarter-Kelly fraction
into a **position-size multiplier** applied to the base quote amount:

```
adjusted   = f* × kelly_fraction          (default fraction: 0.25 — quarter-Kelly)
multiplier = (1.0 + adjusted).clamp(0.25, 3.0)
size       = base_quote_amount × multiplier
```

A zero edge → **1.0×** (base size); a positive edge scales up toward **3.0×**; a
negative edge scales down toward **0.25×**. A **warm-up guard** returns 0.5× until
`kelly_min_trades` samples exist (default 10), and 1.0× when there are no losses
or no wins to form the win/loss ratio. The multiplier feeds `amount_multiplier`
when Kelly mode is active, de-risking losing streaks and pressing winning ones.

### 2.3 Builder Mode Sizing (Section 8 for full derivation)

When a builder mode is active, the position multiplier is computed from wallet progress toward the target and overrides Kelly:

```
progress = (wallet_sol / target_sol).clamp(0.0, 1.0)
```

Progress is the wallet's **fraction of the target** — there is no `start_sol`
baseline subtraction in the code.

---

## 3. Exit Strategy Architecture

The exit layer runs in a two-phase sell monitor:

**Phase 1 — Rapid dump detection** (first 20 checks × 100ms = 2s)
- Watches for 3 consecutive price declines immediately post-buy
- Exits at stop-loss if 3-in-a-row decline detected
- Catches the worst rug-pulls and honeypots within 2 seconds

**Phase 2 — Continuous monitoring** (`price_check_interval_ms` default 500ms)
The following checks run in order each tick; first match wins:

1. **Stop-loss** — below `stop_loss_pct` triggers immediate full exit
2. **Profit floor protection** — if ever hit `profit_first_floor_pct`, enforce a floor stop
3. **Tiered partial TP** — if at/above a tier threshold, sell the tier's partial amount
4. **Adaptive pullback** — if `momentum_pullback_exit_pct` triggered from peak, full exit
5. **Velocity decay** — if deceleration exceeds threshold above `velocity_decay_min_pnl_pct`, exit
6. **Momentum escalation** — escalate the TP target on each new peak confirmation
7. **Full take-profit** — if above `take_profit_pct`, full exit

The ordering matters: stop-loss and floor protection are checked before any TP logic, ensuring losses are bounded absolutely.

---

## 4. Adaptive Pullback Exit

### 4.1 Problem Statement

A fixed pullback percentage (e.g., always exit if down 18% from peak) fails in both directions:
- **Too loose at low PnL**: a 18% pullback from a +20% peak still exits at only +2% profit — barely covering fees
- **Too tight at high PnL**: a 18% pullback from a +300% peak exits at +246%, leaving 54% on the table

### 4.2 The Adaptive Formula

```
θ_eff = base × √(1 + peak_pnl / 100)
```

Where:
- `base` = `momentum_pullback_exit_pct` (config default: 8.0)
- `peak_pnl` = highest PnL seen since position entry (percent)
- `θ_eff` = effective pullback threshold before exit

Exit condition: `current_pnl ≤ peak_pnl - θ_eff`

### 4.3 Worked Examples

| Peak PnL | θ_eff (base=8.0) | Exit triggers at |
|----------|------------------|-----------------|
| +50%     | 8.0 × √1.5 = 9.8% | +40.2% |
| +100%    | 8.0 × √2.0 = 11.3% | +88.7% |
| +200%    | 8.0 × √3.0 = 13.9% | +186.1% |
| +300%    | 8.0 × √4.0 = 16.0% | +284.0% |
| +500%    | 8.0 × √6.0 = 19.6% | +480.4% |
| +1000%   | 8.0 × √11.0 = 26.5% | +973.5% |

### 4.4 Why √ and Not Linear?

A linear formula `θ = base + k × peak` would widen the window faster than justified — at +1000% a linear formula with k=0.05 gives θ=58%, meaning you'd surrender 580 points of profit. The square root grows sublinearly: protective but not excessively generous. The formula is also anchored at `base` when peak=0, so it degrades gracefully to a simple fixed pullback when no profit has been established.

### 4.5 Parameter Evolution

| Version | base | Rationale |
|---------|------|-----------|
| v0.5.0  | 18.0 | Original fixed threshold; simple but too loose at low PnL |
| v1.3.0  | 12.0 | Tightened after observing exits at near-flat PnL on small runs |
| v1.4.0  | 8.0  | Current; adaptive formula compensates — effective window is actually WIDER at high PnL than v0.5.0 |

---

## 5. Momentum Escalation

### 5.1 Concept

When a token keeps printing new all-time highs, a static TP target becomes a ceiling. Momentum escalation dynamically raises the TP target as long as momentum persists, letting winners run without cap.

### 5.2 Escalation Equation

Each time a new peak is confirmed (current PnL > previous peak + threshold), the take-profit target escalates:

```
TP_n = TP_0 × factor^n
```

Where:
- `TP_0` = initial take-profit target (`take_profit_pct` from live_params)
- `factor` = `momentum_escalation_factor` (default: 1.8)
- `n` = escalation round (1 to max_escalations)

Trigger condition: `current_pnl - last_peak ≥ momentum_escalation_threshold_pct`

Cap: escalation stops at `momentum_max_escalations` rounds.

### 5.3 Escalation Table (TP₀ = 175%)

| Round n | TP target |
|---------|-----------|
| 0 (base)| 175% |
| 1       | 315% |
| 2       | 567% |
| 3       | 1,021% |
| 4       | 1,837% |
| 5       | 3,307% |
| 6       | 5,952% |
| 7 (max) | 10,714% |

The minimum PnL to even consider escalation is `momentum_min_peak_pct = 60.0%` — escalation never fires on micro-pumps.

### 5.4 Parameter Evolution

| Version | factor | max_n | threshold | min_peak | Rationale |
|---------|--------|-------|-----------|----------|-----------|
| v0.5.0  | 1.6    | 5     | 5.0%      | 25%      | Conservative initial values |
| v1.4.0  | 1.8    | 7     | 3.0%      | 60%      | More aggressive escalation on confirmed runners; higher min_peak prevents false triggers on noise |

---

## 6. Velocity Decay Exit

### 6.1 Concept

A token can hold its PnL level while momentum is dying — the price stopped rising but hasn't fallen yet. Velocity decay detects this inflection point using the second derivative of price.

### 6.2 Price Velocity Computation

The sell monitor keeps a rolling window of price observations. At each tick:

```
velocity_window = last N price samples
half = len / 2

recent_velocity   = (prices[half:].last - prices[half:].first) / elapsed_recent
previous_velocity = (prices[:half].last - prices[:half].first) / elapsed_previous
```

### 6.3 Decay Detection Condition

```
if current_pnl ≥ velocity_decay_min_pnl_pct AND
   previous_velocity > velocity_decay_drop_threshold × recent_velocity:
       EXIT
```

In plain English: "If we're profitable AND the price was rising significantly faster in the first half of the window than the second half, momentum is fading — exit."

The `velocity_decay_drop_threshold` of 1.5 means recent velocity must be less than 2/3 of previous velocity to trigger — a 33% deceleration requirement prevents noise triggers.

### 6.4 Integration with DQ* State

The DQ* agent's `price_velocity` and `price_acceleration` state features (introduced in v1.1.0) directly encode this computation, allowing the agent to learn when velocity decay correlates with subsequent price crashes and adjust its `SellAll` / `SellPartial` preferences accordingly.

### 6.5 Parameter Evolution

| Version | min_pnl | drop_threshold | Rationale |
|---------|---------|----------------|-----------|
| v0.5.0  | 7.0%    | 1.2            | Triggered too often on normal volatility above small profit |
| v1.4.0  | 25.0%   | 1.5            | Higher min_pnl gates: only exit early if actually profitable; stricter drop ratio reduces false triggers |

---

## 7. Tiered Partial Take-Profit

### 7.1 Why Partial Sells

A full exit at the first TP target captures gains but terminates the position. Partial sells let the bot:
1. Lock in guaranteed profit on part of the position
2. Retain exposure to further upside
3. Psychologically and mathematically de-risk the remaining position

### 7.2 Tier Structure

Tiers are evaluated in ascending order. Each tier `(threshold_pct, sell_fraction_pct)` fires once:

```
for (threshold, fraction) in tiered_partial_tp_levels:
    if current_pnl >= threshold AND tier_not_yet_fired:
        sell fraction% of remaining position
        mark tier as fired
```

### 7.3 Current Tiers (v1.4.0)

| Tier | PnL Threshold | Sell Fraction | Notes |
|------|---------------|---------------|-------|
| 1    | 100%          | 15%           | First double — take a sliver to cover fees |
| 2    | 300%          | 20%           | 4× run — meaningful profit lock-in |
| 3    | 600%          | 25%           | 7× run — significant size reduction |

After tier 3, the remaining 40–60% of the position stays open under momentum escalation until full TP, adaptive pullback, or velocity decay triggers.

### 7.4 Parameter Evolution

| Version | Tiers | Rationale |
|---------|-------|-----------|
| v0.5.0  | (45%, 20%), (100%, 25%), (200%, 25%) | First tier too early — fired on noise pumps |
| v1.4.0  | (100%, 15%), (300%, 20%), (600%, 25%) | Pushed first tier to 100% (genuine double) to avoid premature size reduction |

### 7.5 Profit Floor Protection

After the first tier fires (or if `profit_first_floor_pct` is reached), a floor stop is armed:

```
if current_pnl < profit_first_floor_pct:
    EXIT  // floor stop
```

Default `profit_first_floor_pct = 25.0%` — once we've hit 25% PnL the position will never close below 25% profit (assuming slippage is within range).

| Version | floor | Rationale |
|---------|-------|-----------|
| v0.5.0  | 50.0% | Too high — floor never armed on modest runs |
| v1.4.0  | 25.0% | Lower floor arms sooner, protecting against quick reversals on small winners |

---

## 8. Builder Mode Compounding Algorithms

Builder modes replace the static `amount_multiplier` with a dynamic function of wallet progress toward a target. They run in the sniper main thread, updating `live_params` every 5 seconds.

### 8.1 Progress Variable

```
progress = (wallet_sol / target_sol).clamp(0.0, 1.0)
```

`wallet_sol` is `approx_wallet_sol()` (session-start balance + daily PnL).
Progress is the wallet's fraction of the target — **no `start_sol` subtraction**
(verified `main.rs:766`).

### 8.2 Growth Mode (target: 0.2 SOL)

**Goal:** Mild geometric compounding, conservative sizing.

```
multiplier = (1.0 + 1.0 × progress^0.8).clamp(1.0, 2.0)
TP         = base_tp           // no change
SL         = base_sl           // no change
```

The exponent 0.8 is slightly sub-linear (concave) — multiplier grows quickly early (when each SOL of progress is a large fraction of the target) and plateaus as the target is approached. Range: 1.0–2.0×.

### 8.3 Builder Mode (target: 1.0 SOL)

**Goal:** Aggressive geometric compounding, tightening TP/SL as target approaches.

```
multiplier = (1.5 + 2.0 × progress^0.65).clamp(1.5, 3.5)
TP         = base_tp × (1.5 - 0.5 × progress).max(1.0)
SL         = base_sl × (1.2 - 0.2 × progress)
```

**Multiplier:** Exponent 0.65 (more sub-linear than Growth) — size grows fast early when the account is small relative to the target, and asymptotically approaches 3.5× as the target is reached.

**TP evolution:** Starts at 1.5× base (e.g., 262% if base=175%) and linearly decays to 1.0× base (175%) at 100% progress. Rationale: take smaller risks per-trade as the target is close to lock in gains.

**SL evolution:** Starts at 1.2× base and narrows to 1.0× base at progress=1.0. Tighter stop as the target approaches reduces the risk of giving back accumulated gains.

### 8.4 SuperBuilder Mode (target: 3.0 SOL)

**Goal:** Parabolic compounding with moon-chase phase for explosive runs.

```
multiplier = (2.0 + 6.0 × progress^0.35).clamp(2.0, 8.0)
TP         = base_tp × (2.0 - progress).max(1.0)
SL         = base_sl × 1.4        // fixed wider stop
moon_chase = (progress < 0.25)    // enabled in first quarter
```

**Multiplier:** Exponent 0.35 is strongly sub-linear — almost all of the multiplier growth happens very early (when account is tiny vs target). Near target the multiplier flattens at 8×.

**TP evolution:** At progress=0, TP = 2.0× base (350% if base=175%). At progress=1.0, TP = 1.0× base. The wider early TP allows massive runners to fully develop.

**Moon-chase:** When enabled (progress < 25%), the stop-loss is temporarily removed for confirmed mooning tokens — position is held until adaptive pullback or manual exit. This is the "lottery ticket" phase when the account is small enough that extreme risk is justified by the extreme reward.

**Fixed wider SL (1.4×):** SuperBuilder takes bigger size so a wider stop is needed to avoid stop-hunting on normal volatility.

### 8.5 Builder Mode Progress vs Multiplier Reference

| Progress | Growth `1+p^0.8` | Builder `1.5+2p^0.65` | SuperBuilder `2+6p^0.35` |
|----------|--------|---------|--------------|
| 0%       | 1.00×  | 1.50×   | 2.00× |
| 10%      | 1.16×  | 1.95×   | 4.68× |
| 25%      | 1.33×  | 2.31×   | 5.69× |
| 50%      | 1.57×  | 2.77×   | 6.71× |
| 75%      | 1.79×  | 3.16×   | 7.43× |
| 100%     | 2.00×  | 3.50×   | 8.00× |

### 8.6 Design Rationale: Why Sub-Linear Exponents?

A linear `progress × max_mult` would grow the multiplier slowly early (when the account is small and compounding has the most effect) and quickly late (when close to the target and variance should be reduced). Sub-linear exponents (`p^0.35`, `p^0.65`, `p^0.8`) flip this: size is large early and moderates as the target is approached. This matches the mathematical intuition that compounding is most powerful when the account is small relative to the target — a 2× return on 0.1 SOL adds 0.1 SOL; a 2× return on 2.9 SOL adds 2.9 SOL and risks overshooting or losing near-target gains.

---

## 9. Risk Subsystems

### 9.1 ATH Drawdown Guard

Pauses buys when the wallet has declined more than `ath_drawdown_pct` below the session all-time-high:

```
current_balance_sol  = session_start_sol + cumulative_pnl_sol
session_ath_sol      = max(session_ath_sol, current_balance_sol)
drawdown_pct         = (session_ath_sol - current_balance_sol) / session_ath_sol × 100

if drawdown_pct >= ath_drawdown_pct:
    HALT_BUYS
```

The guard resets when the wallet recovers past the ATH (session_ath moves up), not on a fixed timer. This ensures capital is protected during losing streaks without arbitrary time-based recovery.

### 9.2 Grief Breaker

5-minute sliding window of cumulative losses. If cumulative loss exceeds `grief_max_loss_sol` within any 5-minute window, buys halt for `grief_cooldown_secs`:

```
window_loss = sum(losses in last 300 seconds)
if window_loss >= grief_max_loss_sol:
    halt for grief_cooldown_secs
```

Designed to prevent "tilt trading" — rapidly re-entering after a string of losses amplifies drawdowns.

### 9.3 Pool Scorer

A predictive quality score (0–100) computed from pool age and quote vault size:

```
score = f(pool_age_secs, quote_vault_sol, liquidity_sol, ...)
```

Pools below `min_pool_score` are rejected before any buy is attempted. The scorer is a linear combination of features trained on historical pool outcomes tracked in `pool-cache.json`.

### 9.4 Deployer Reputation

Exponential Moving Average of deployer rug rate from `scematica-deployer-reputation.json`:

```
rug_ema = α × is_rug + (1 - α) × previous_ema
```

Deployers with `rug_ema > max_deployer_rugs_24h` are rejected. The EMA means a single rug doesn't permanently blacklist a deployer, but chronic ruggers are correctly penalized.

### 9.5 Mint Dedup Guard

Prevents buying the same mint within 5 minutes of a confirmed purchase:

```
recently_bought: DashMap<Pubkey, Instant>

if recently_bought.get(mint).elapsed() < 300s:
    SKIP
```

Cleans expired entries on each check (retain: elapsed < 300s). Prevents the bot from loading up multiple positions on the same token when duplicated pool events are received (common with Raydium WebSocket reconnects).

---

## 10. Parameter Evolution History

> **Defaults vs. live config.** The values below (and throughout this document)
> are the **`config.rs` defaults**. The shipped `config.toml` overrides several of
> them with more aggressive tuning — verified 2026-06-05 it runs a **12-round**
> escalation ladder (vs. the default 7), a **15.0** pullback base (vs. 8.0), and
> **disables tiered partial TP** (`partial_tp_pct = 0`) in favour of a single full
> exit gated at `take_profit_pct`. The per-rate-mode presets (Bearish → Bullish)
> additionally set their own escalation/TP values. The *formulas* in §4–§8 are
> unchanged; only these scalar parameters differ between defaults and live config.

### 10.1 Timeline

#### v0.5.0 — Baseline (36 features shipped)
- `take_profit_pct`: 80.0%
- `stop_loss_pct`: 15.0%
- `momentum_escalation_factor`: 1.6
- `momentum_max_escalations`: 5
- `momentum_escalation_threshold_pct`: 5.0%
- `momentum_min_peak_pct`: 25.0%
- `momentum_pullback_exit_pct`: 18.0 (fixed, no adaptive formula)
- `velocity_decay_min_pnl_pct`: 7.0%
- `velocity_decay_drop_threshold`: 1.2
- `tiered_partial_tp_levels`: [(45%, 20%), (100%, 25%), (200%, 25%)]
- `profit_first_floor_pct`: 50.0%

**Problems observed:** Bot exited too early on multi-bag runs. The 80% TP was hit frequently and then the token continued to 300–500%. Tiered TP at 45% created excessive churn on noise pumps. Velocity decay at 7% min PnL fired constantly.

#### v1.3.0 — Incremental improvements
- `take_profit_pct`: 130.0%
- `momentum_pullback_exit_pct`: 12.0
- Adaptive pullback formula introduced (but base was 12.0, not 8.0)

**Problems observed:** Still too conservative. Many tokens that ran to 200–400% were exited at 130%.

#### v1.4.0 — Current (aggressive hold, precision exits)
- `take_profit_pct`: 175.0% — allows 2.75× runners before full exit
- `stop_loss_pct`: 18.0% — slightly wider to prevent stop-hunt exits
- `momentum_escalation_factor`: 1.8 (was 1.6) — faster TP escalation on confirmed runners
- `momentum_max_escalations`: 7 (was 5) — allows 10,714% theoretical max TP
- `momentum_escalation_threshold_pct`: 3.0% (was 5.0%) — triggers escalation on smaller moves
- `momentum_min_peak_pct`: 60.0% (was 25.0%) — guards against noise triggers
- `momentum_pullback_exit_pct`: 8.0 (was 18.0 fixed) — tighter base, adaptive compensates at high PnL
- `velocity_decay_min_pnl_pct`: 25.0% (was 7.0%) — prevents early exit on low-profit positions
- `velocity_decay_drop_threshold`: 1.5 (was 1.2) — requires larger deceleration to trigger
- `tiered_partial_tp_levels`: [(100%, 15%), (300%, 20%), (600%, 25%)] — pushed first tier to genuine double
- `profit_first_floor_pct`: 25.0% (was 50.0%) — lower floor arms sooner

---

## 11. Data-Driven Refinements

### 11.1 Observations That Drove v1.4.0

The following patterns from `scematica-trades.jsonl` and DQ* agent feedback shaped the v1.4.0 parameters:

**Pattern 1: Runner termination at TP**  
Many trades showed the token continuing upward after the 80% TP was hit. Post-hoc analysis of exit points versus peak-within-next-60s showed median missed gain of ~180%. This directly motivated raising TP to 175% and adding momentum escalation with factor=1.8.

**Pattern 2: False velocity decay signals**  
With `velocity_decay_min_pnl_pct = 7.0%`, velocity decay was triggering on positions at only 7–12% PnL where the "decay" was simply normal post-buy consolidation before the second leg up. Raising the floor to 25% eliminated ~70% of these false triggers.

**Pattern 3: Tiered TP at 45% causing missed runs**  
The 45% first tier fired on nearly every trade (it's an easy target for a new pool with momentum). Selling 20% at 45% then watching the token run to 400% left significant value on the table. Pushing the first tier to 100% (a genuine double) made partial sells meaningful.

**Pattern 4: Duplicate buys on reconnect**  
Helius WebSocket reconnect events rebroadcast recent pool creation messages. Without a dedup guard, the bot was occasionally buying the same token 2–3 times on reconnect, inflating position size beyond config limits. The 5-minute mint dedup guard eliminated this.

**Pattern 5: Window minimize on notifications**  
PowerShell processes spawned with default creation flags inherit the terminal's console, causing the parent PowerShell window (the TUI terminal) to lose foreground focus. `CREATE_NO_WINDOW` (0x08000000) prevents any console handle creation for spawned PowerShell subprocesses.

### 11.2 Fee Coverage Analysis

Raydium V4 trades incur:
- AMM swap fee: 0.25%
- Solana network fee: ~5,000 lamports (~$0.001 at $200/SOL)
- Priority fee: variable, typically 1,000–100,000 lamports

For a 0.05 SOL position:
- Round-trip swap fees: ~0.00025 SOL × 2 = 0.0005 SOL
- Priority fees: ~0.0001 SOL
- **Total round-trip cost: ~0.0006 SOL = 1.2% of position**

The minimum profitable exit at 0.05 SOL position size is therefore ~1.5% after slippage. The `profit_first_floor_pct = 25.0%` is deliberately set far above this to ensure floor exits are always meaningfully profitable.

---

## 12. DQ* Agent Feedback Loop

The DQ* reinforcement learning agent (see `DQ_STAR_AGENT.md` for full technical documentation) creates a closed feedback loop with the trading parameters:

### 12.1 How Agent Improvements Feed Back Into Strategy

1. **Agent trains** on `scematica-trades.jsonl` — real execution data
2. **Agent learns** which state configurations led to high vs low reward
3. **Human reviews** agent's Q-value heatmaps and action distribution
4. **Parameters tuned** based on what the agent learned to prefer

Concretely: the v1.4.0 velocity decay threshold change was motivated by observing the agent frequently selecting `Hold` in states where velocity decay had already triggered in the rule-based system. The agent's implicit valuation indicated the rule-based system was too eager to exit.

### 12.2 Reward Function and Strategy Alignment

The agent's reward function (full piecewise definition in `DQ_STAR_AGENT.md` §5)
is superlinear in profit and zoned in loss. The profit term is:

```
R_profit = pnl × (1 + log₂(1 + pnl/25))  +  timing_bonus
```

This superlinear shape trains the agent to value large gains disproportionately over small ones — aligned with the intuition that a 500% gain is far more than 10× as valuable as a 50% gain (rarer and harder to capture). Losses, by contrast, are **multiplied** by escalating factors (×1.0 noise / ×1.8 / ×2.5 by severity, with extra flat penalties in rug territory) so the agent learns to cut early.

The **timing bonus** is *discrete*, keyed on hold time in **minutes**: **+75** for a sub-1-minute exit, +30 (≤ 3 min), +10 (≤ 10 min), then a capital-lock penalty beyond 10 minutes (down to −40). It rewards fast capital recycling — consistent with most new-pool price action happening in the first minutes.

### 12.3 Builder Mode and Agent Compatibility

Builder modes modify `live_params.amount_multiplier`, `take_profit_pct`, and `stop_loss_pct` at runtime. The DQ* agent's `TradeState` includes `sol_balance_sol` and `daily_pnl_sol`, allowing it to infer the current builder mode progress and adapt its action selection accordingly without explicit mode injection.

When the agent reaches `ready_to_advise` (**`train_steps ≥ 10,000`** — a step-count gate, not an ε threshold), its `BuyStandard` vs `BuyAggressive` distinction maps onto the builder framework: `BuyAggressive` → SuperBuilder-style large-position entry (and **1.5× sizing** in the live buy gate), while `BuyStandard` / `Hold` → Growth/Builder-mode conservative entry (a strong bearish lean vetoes the buy; see `DQ_STAR_AGENT.md` §16).

---

*This document should be updated whenever strategy parameters are modified in `config.rs`, new exit mechanisms are added to `sniper.rs`, or significant trading data reveals new patterns. Version this alongside the Cargo.toml workspace version.*
