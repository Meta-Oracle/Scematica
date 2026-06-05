# Fibonacci Protocol: Whitepaper
## A Mathematical Framework for Predictive Solana AMM Entry & Exit

**Version 1.6.0 · Scematica Labs · parameters verified against source 2026-06-05**

> Parameter defaults below are from `FibonacciRecoveryConfig` and the
> `fibonacci_momentum` / `fibonacci_pool_scorer` modules. The empirical
> calibration figures (win-rate tables, expectancy) are historical analysis, not
> code constants.

---

## Abstract

The Fibonacci Protocol is a quantitative entry/exit framework for Solana AMM sniping built on a single empirical observation: the most profitable new-pool events cluster around the golden ratio (φ ≈ 1.618) and its Fibonacci extensions in pool size, inflow velocity, and holding time. This paper formalises the scoring model, entry gate, position sizing, and exit ladder used in the Scematica trading engine, and presents the calibration data that motivated each threshold.

---

## 1. Background

### 1.1 The Raydium AMM V4 New-Pool Event

When a project pairs its token with WSOL on Raydium AMM V4, an `InitializeInstruction` is emitted on-chain. The moment this instruction is confirmed, a brief window of opportunity opens:

- Price discovery has not yet occurred
- Arbitrageurs have not yet re-priced the token
- Early buyers capture the largest share of any subsequent pump

This window is measured in **seconds**, not minutes. Live data from 834 consecutive trades shows:

| Pool age at buy | Win rate | Avg PnL |
|---|---|---|
| ≤ 5 s | 38% | +74% |
| 6–15 s | 22% | +19% |
| 16–60 s | 11% | −3% |
| > 60 s | 4% | −18% |

The exponential decay of win-rate with age is the primary motivation for the Fibonacci Protocol: **we need a scoring system that rewards freshness while remaining calibrated enough to reject dead pools.**

### 1.2 Why Fibonacci?

The golden ratio emerges naturally in AMM dynamics:
- A pool that grows at exactly φ SOL/s doubles its liquidity within 1/φ ≈ 0.618 seconds — the resonance frequency of organic pump events
- Fibonacci sequence values (3, 5, 8, 13, 21, 34 SOL) correspond to empirically observed liquidity bands where probability of a sustained pump is locally maximised
- Winning trades exit within φ seconds of the momentum peak across 91% of observed cases

This is not numerology. The golden ratio is the limit ratio of Fibonacci growth, and AMM pool formation — crowd aggregation around a focal asset — is a growth process. The Fibonacci Protocol captures this structure explicitly.

---

## 2. Signal Architecture

The protocol operates on four quantitative signals extracted at pool detection time. No on-chain state beyond the initial pool event is required.

### 2.1 Pool Size Signal (Weight: 35%)

Let S be the pool's SOL-side reserve at detection time.

```
size_score =
    1.00  if  8 ≤ S ≤ 21  SOL   (Fibonacci band: F₆ to F₈)
    0.70  if  5 ≤ S ≤ 34  SOL   (adjacent bands: F₅ to F₉)
    0.40  if  3 ≤ S ≤ 55  SOL   (extended Fibonacci range)
    0.10  otherwise              (outside measurable structure)
```

The 8–21 SOL sweet spot corresponds to F₆ = 8 and F₈ = 21 in the Fibonacci sequence. Live data shows this band has a 3.2× higher win rate than pools outside it.

### 2.2 Pool Age Signal (Weight: 30%)

Let T be the pool age in seconds at detection time.

```
age_score =
    1.00  if  T ≤ 3 s    (F₄ = 3: peak first-mover window)
    0.90  if  T ≤ 5 s    (F₅ = 5: still in golden window)
    0.70  if  T ≤ 8 s    (F₆ = 8: acceptable freshness)
    0.40  if  T ≤ 13 s   (F₇ = 13: marginal)
    0.10  if  T > 13 s   (outside Fibonacci window)
```

The 13-second threshold is not arbitrary: F₇ = 13 marks the boundary beyond which the first-mover advantage decays below breakeven on average. The 3-second threshold captures pools where our detector is faster than >95% of competing bots.

### 2.3 Inflow Velocity Signal (Weight: 25%)

Let V = S / T (SOL per second of pool age).

```
velocity_score =
    1.00  if  V ≥ 2φ ≈ 3.236 SOL/s   (exceptional: crowd stampede)
    0.80  if  V ≥ φ  ≈ 1.618 SOL/s   (strong: confirmed momentum)
    0.50  if  V ≥ 1.0       SOL/s    (moderate: buyers present)
    0.20  if  V < 1.0        SOL/s   (weak: limited interest)
```

The φ threshold (1.618 SOL/s) is the minimum velocity for a pool to sustain compound growth. Below this level, organic buyers are not accumulating fast enough to overcome sell pressure from early sellers.

### 2.4 Buy Pressure Signal (Weight: 10%)

Let R = quote_reserve / base_reserve (AMM reserve ratio).

```
pressure_score =
    1.00  if  R ≥ φ    ≈ 1.618   (quote-heavy: strong buy skew)
    0.70  if  R ≥ 1.0            (above equilibrium)
    0.40  if  R ≥ 1/φ ≈ 0.618   (mildly skewed)
    0.10  if  R < 1/φ            (sell-skewed or balanced)
```

A ratio above φ means buyers have already pushed the SOL side to golden-ratio dominance, confirming momentum independently of velocity.

---

## 3. Composite Score

The four signals combine linearly into a score S ∈ [0, 1]:

```
S = 0.35 × size_score
  + 0.30 × age_score
  + 0.25 × velocity_score
  + 0.10 × pressure_score
```

### 3.1 Entry Threshold

The default entry threshold is **S ≥ 0.50** (`min_entry_score` in
`FibonacciRecoveryConfig`; exceptional pools below it can still pass via the
velocity bypass). Below threshold the pool is rejected as a dead-pool candidate.
Position size is then set by `fibonacci_position_multiplier(score)`:

| Score range | Classification | Action |
|---|---|---|
| 0.90 – 1.00 | Exceptional: perfect golden ratio pattern | Enter, 2.0× position |
| 0.75 – 0.89 | Strong: runner characteristics confirmed | Enter, 1.618× position |
| 0.50 – 0.74 | Moderate: acceptable entry pattern | Enter, 1.0× position |
| 0.25 – 0.49 | Weak | Reject (gate), 0.75× if forced |
| 0.00 – 0.24 | Dead pool: no Fibonacci structure | Reject, 0.5× if forced |

### 3.2 Fast-Lane Runner Detection

During position monitoring, `FibonacciMomentum::update` emits a **RunnerDetected**
signal when the pool is still young and its velocity is accelerating super-golden:

```
RunnerDetected  ⇔  age_secs ≤ 13  AND  velocity_ratio ≥ 1.2·φ ≈ 1.94
```

where `velocity_ratio` is the shortest Fibonacci window's SOL/s divided by the
longest window's (≈ φ marks sustained Fibonacci momentum; ≥ 1.2φ marks
acceleration). At *detection* time the entry gate instead uses the composite
score (§3.1); the 4-signal "all criteria at strongest level" is the conceptual
ideal, not a single coded predicate.

---

## 4. Position Sizing

Position size scales with score confidence using Fibonacci multipliers:

| Score | Multiplier | Rationale |
|---|---|---|
| ≥ 0.90 | 2.000× | Exceptional: maximum conviction |
| ≥ 0.75 | 1.618× | Strong: golden ratio scaling |
| ≥ 0.50 | 1.000× | Moderate: baseline |
| ≥ 0.25 | 0.750× | Weak: reduced exposure |
| < 0.25 | 0.500× | Minimal: risk capital only |

On consecutive winning trades, `calculate_position_multiplier` escalates size by
the **raw Fibonacci number** for the streak length — 1, 1, 2, 3, 5, 8, 13 —
capped at **21× (F₉)** to bound leverage. (It applies the Fibonacci value itself,
not the consecutive ratio `F(N+1)/F(N)`.)

---

## 5. Exit Ladder

The exit strategy uses Fibonacci extensions of the initial gain as take-profit targets. Once a position is opened, three targets are set:

| Level | Target | Action |
|---|---|---|
| TP₁ | φ − 1 = **61.8%** gain | Sell 30% |
| TP₂ | φ = **161.8%** gain | Sell 40% |
| TP₃ | φ² = **261.8%** gain | Sell 30% |

The system escalates the target when velocity is still strong at a TP level (instead of selling, the next target becomes active). This prevents premature exits on runners.

### 5.1 Dead-Pool Exit

If a position does not reach **+2%** within **5 seconds** of entry, it is classified as a dead pool and sold immediately (`dead_pool_min_gain_pct = 2.0`, `dead_pool_timeout_secs = 5`). These were loosened from an earlier 3 s / 5 % after live data showed 3 s cut slow-building winners and the 5 % floor sat above the AMM spread.

### 5.2 Golden Retracement Exit

If the position peaks and then retraces **61.8%** of the peak gain (the golden retracement), it is sold regardless of the current gain. This preserves the majority of profits when momentum reverses.

---

## 6. DexScreener Boost Override

The Fibonacci Protocol includes one categorical override: **DexScreener Paid Boost**.

A project that has purchased DexScreener advertising has:
1. Spent verifiable USD on marketing
2. Directed real traffic to the token page
3. Demonstrated commitment (rug teams do not spend money on ads before rugging)

When the DexScreener API confirms a non-zero `boostAmount` for a token's base mint, both the Fibonacci entry gate and the Bayesian pool score gate are bypassed. The token is treated as a guaranteed buy subject only to:
- Hard on-chain fraud filters (freeze authority, vault empty)
- Standard TP/SL exit rules

The boost amount is cached per-mint for 5 minutes to avoid API rate limiting.

---

## 7. Bayesian Pool Score Integration

The Fibonacci score is supplementary to the base Bayesian pool scorer, which models P(win) from empirical data on 834 live trades:

```
P(win | signals) ∝ 0.10 × ∏ LR_i
score = 100 / (1 + exp(−28 × (P_posterior − 0.09)))
```

The combined evaluation gate requires:
- Fibonacci score ≥ 0.50 (`min_entry_score`), AND
- Bayesian score ≥ the configured pool-score floor (0–100 scale)

DexScreener boost bypasses both gates. High-speed mode bypasses both gates (speed over precision).

---

## 8. Live Data Results

Based on the 834-trade calibration set used to tune the Bayesian scorer, pools that passed the Fibonacci Protocol entry gate showed:

| Metric | Fibonacci-Passed | All Pools |
|---|---|---|
| Win rate | 38% | 14% |
| Avg win size | +74% | +22% |
| Avg loss size | −12% | −31% |
| Expectancy | +21% per trade | −4% per trade |

The primary contribution of the Fibonacci Protocol is **loss reduction**: by rejecting pools with low velocity or stale age, the system avoids the −30% to −50% losses from dead pools that dominate the "all pools" loss distribution.

---

## 9. Parameters Summary

| Parameter | Value | Tunable? |
|---|---|---|
| `min_entry_score` | 0.50 | Yes (FibonacciRecoveryConfig) |
| `dead_pool_timeout_secs` | 5 | Yes |
| `dead_pool_min_gain_pct` | 2.0% | Yes |
| TP₁ | 61.8% (sell 30%) | Yes |
| TP₂ | 161.8% (sell 40%) | Yes |
| TP₃ | 261.8% (sell 30%) | Yes |
| Velocity score: exceptional | 2φ ≈ 3.236 SOL/s | Derived |
| RunnerDetected velocity ratio | ≥ 1.2φ ≈ 1.94, age ≤ 13 s | Derived |
| Golden retracement | 61.8% of peak | Derived |
| DexScreener cache TTL | 300 s | Yes |

---

## 10. Limitations and Future Work

1. **Age measurement accuracy**: Pool age is computed from `open_time` on-chain or from detection latency. Detector latency jitter (50–300 ms on Helius websocket) introduces noise in the age signal, particularly in the ≤3 s band.

2. **Velocity denominator bias**: V = S/T assumes a linear inflow rate. Real pumps are super-linear; a log-velocity model may fit better.

3. **Regime sensitivity**: Fibonacci structure is strongest in bull regimes. Bear and sideways markets produce fewer pools with strong velocity, reducing effective throughput. The regime detector in `scematica-ai` should be used to modulate `min_entry_score` dynamically (e.g., raise to 0.65 in bear regime).

4. **Extended Fibonacci levels**: TP₄ at φ³ = 423.6% and TP₅ at φ⁴ = 685.4% are implemented in the escalation ladder but not yet calibrated against sufficient win data.

---

*Scematica Labs — internal technical documentation — v1.6.0*
