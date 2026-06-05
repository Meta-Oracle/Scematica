# Fibonacci Protocol — Operations & Tuning Guide

> Companion to [FIBONACCI_PROTOCOL_WHITEPAPER.md](FIBONACCI_PROTOCOL_WHITEPAPER.md)
> (the canonical spec). This guide is the operator's view: how the system is
> wired, the config knobs and their **code defaults**, what to monitor, and how to
> tune. Parameters here are verified against `FibonacciRecoveryConfig`,
> `fibonacci_momentum.rs`, and `fibonacci_pool_scorer.rs` (2026-06-05).
>
> This file consolidates five earlier companion docs (SUMMARY, QUICK_REF,
> INTEGRATION, INTEGRATION_GUIDE, RECOVERY_GUIDE), which had drifted and
> contradicted each other on parameter values.

---

## 1. How it's wired

Three modules in `crates/scematica-sniper/src/`:

| Module | Role |
|--------|------|
| `fibonacci_momentum.rs` | `FibonacciMomentum` — per-position velocity tracking across Fibonacci time windows; emits `FibonacciSignal` (Hold / RunnerDetected / EscalateToNextFib / TakeProfitAtFib / ExitGoldenRetrace / ExitVelocityCollapse). Also `score_pool_fibonacci` (detection-time score). |
| `fibonacci_pool_scorer.rs` | `FibonacciPoolScorer` — blends the Fibonacci score with the Bayesian pool scorer; `fibonacci_position_multiplier(score)`. |
| `fibonacci_recovery_system.rs` | `FibonacciRecoverySystem` — coordinator: `evaluate_entry()` (gate + sizing + expected TP) and `evaluate_exit()` (dead-pool detection + Fibonacci TP/retrace). Config in `FibonacciRecoveryConfig`. |

Entry flow: `evaluate_entry()` → `score_pool_fibonacci()` → gate on `min_entry_score`
→ `fibonacci_position_multiplier(score)` for sizing → `expected_tp_pct` by score band.

Exit flow: `evaluate_exit()` clones the momentum tracker, calls `update()`, and
returns a dead-pool / TP / retracement decision.

---

## 2. Config knobs (`FibonacciRecoveryConfig` defaults)

| Field | Default | Meaning |
|-------|---------|---------|
| `min_entry_score` | **0.50** | Minimum composite Fibonacci score to enter. Exceptional pools below it can still pass via the velocity bypass. |
| `dead_pool_timeout_secs` | **5** | If the position hasn't gained `dead_pool_min_gain_pct` by this age, sell. (Was 3 s — too fast for slow pumpers.) |
| `dead_pool_min_gain_pct` | **2.0** | Minimum gain % to avoid the dead-pool exit. (Was 5% — sat above the AMM spread and cut slow winners.) |
| `tp_levels` | 61.8%/30%, 161.8%/40%, 261.8%/30% | Tiered take-profit (φ−1, φ, φ²) with sell fractions. |
| `fast_exit_mode` | `true` | Exit at the first TP level hit (matches live data). |
| `use_fibonacci_sizing` | `true` | Size entries by `fibonacci_position_multiplier(score)`. |

These are distinct from the main sniper `config.toml` gate (`min_pool_score`,
`no_pump_timeout_secs`, etc.) — don't confuse `dead_pool_timeout_secs` (Fibonacci)
with `no_pump_timeout_secs` (main sell monitor).

---

## 3. Scoring & sizing reference

Composite score `S = 0.35·size + 0.30·age + 0.25·velocity + 0.10·pressure`
(see the whitepaper §2–§3 for the per-signal bands). Sizing by score, from
`fibonacci_position_multiplier`:

| Score | Multiplier |
|-------|-----------|
| ≥ 0.90 | 2.000× |
| ≥ 0.75 | 1.618× |
| ≥ 0.50 | 1.000× |
| ≥ 0.25 | 0.750× |
| < 0.25 | 0.500× |

Consecutive-win escalation (`calculate_position_multiplier`) multiplies the base
by the **raw Fibonacci number** for the streak — 1, 1, 2, 3, 5, 8, 13 — capped at
**21× (F₉)**.

Runner detection during monitoring: `RunnerDetected` ⇔ `age ≤ 13 s` AND
`velocity_ratio ≥ 1.2φ ≈ 1.94` (shortest- vs longest-window SOL/s).

TP escalation: at a Fibonacci target, if `velocity_ratio ≥ 0.85φ` the system
escalates to the next level (61.8 → 161.8 → 261.8 → 423.6 → 685.4 → …) instead of
selling; golden-retrace exit fires once `peak_gain ≥ 50%` and the pullback from
peak reaches ≥ 61.8%.

---

## 4. What to monitor

- **Entry rejection rate** — healthy is most pools rejected; a near-100% pass rate
  means the gate is too low or scoring is broken.
- **Dead-pool exits** — these are *good*: a fast small loss beats a slow large one.
  Watch that losses exit near the `dead_pool_min_gain_pct` floor, not far below.
- **Win rate / expectancy** — track from `scematica-trades.jsonl`; the Fibonacci
  gate's job is loss reduction (rejecting low-velocity/stale pools), so expect the
  biggest improvement in *average loss size*, not just win rate.

---

## 5. Tuning

- **Too few entries** → lower `min_entry_score` (e.g. 0.50 → 0.45) or widen the
  size bands in `config.toml`.
- **Too many losing entries** → raise `min_entry_score` (e.g. 0.50 → 0.65),
  especially in bear/sideways regimes.
- **Losses too large / slow** → lower `dead_pool_timeout_secs` or raise
  `dead_pool_min_gain_pct` so dead pools are cut sooner.
- **Cutting slow-building winners** → raise `dead_pool_timeout_secs` (the reason it
  moved 3 s → 5 s) or lower `dead_pool_min_gain_pct`.

Regime note: Fibonacci structure is strongest in bull regimes; consider raising
`min_entry_score` in bear/sideways conditions (the `scematica-ai` regime detector
can drive this).

---

## 6. Troubleshooting

| Symptom | Likely cause | Check |
|---------|--------------|-------|
| Entering almost every pool | gate too low / score saturating | `min_entry_score`; inspect `score_pool_fibonacci` inputs (size, age, velocity, pressure) |
| Never enters | gate too high / velocity inputs zero | `min_entry_score`; confirm `age_secs` and `velocity_sol_per_sec` are non-zero in `evaluate_entry` |
| Dead-pool exits never fire | timeout misconfigured | `dead_pool_timeout_secs` (should be 5, not 0) |
| Holds losers too long | dead-pool gain floor too low | raise `dead_pool_min_gain_pct` |

---

*For the mathematical derivation, signal bands, and calibration rationale, see
[FIBONACCI_PROTOCOL_WHITEPAPER.md](FIBONACCI_PROTOCOL_WHITEPAPER.md).*
