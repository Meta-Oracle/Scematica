# Scematica DQ* Agent — Technical Reference

> Crate: `scematica-nn` 1.25.0 · Agent architecture generation: **v1.1.0** (dueling / PER / n-step) · Last verified against source: 2026-08-11  
> Full technical documentation of the Deep Q* reinforcement learning agent in `crates/scematica-nn`.
>
> Note on versions: the crate ships at the workspace version (1.25.0); the
> "v1.x" labels in this document refer to the **agent architecture generation**
> (see §18), a separate lineage from the package version.

---

## Table of Contents

1. [Overview and Design Goals](#1-overview-and-design-goals)
2. [Architecture](#2-architecture)
3. [State Space (24 features)](#3-state-space-24-features)
4. [Action Space (5 actions)](#4-action-space-5-actions)
5. [Reward Function](#5-reward-function)
6. [Network Architecture — Dueling DQN](#6-network-architecture--dueling-dqn)
7. [Training Algorithm — Double DQN](#7-training-algorithm--double-dqn)
8. [N-Step Returns](#8-n-step-returns)
9. [Prioritized Experience Replay](#9-prioritized-experience-replay)
10. [Exploration — ε-Greedy with Decay](#10-exploration--ε-greedy-with-decay)
11. [Target Network](#11-target-network)
12. [Regime Branching](#12-regime-branching)
13. [Tournament Evolution](#13-tournament-evolution)
14. [Adversarial Injection](#14-adversarial-injection)
15. [Action Rebalancing](#15-action-rebalancing)
16. [Buy Gating & ScemaDEX Integration](#16-buy-gating--scemadex-integration)
17. [Persistence — Checkpoint Format](#17-persistence--checkpoint-format)
18. [Version History](#18-version-history)
19. [Distributional RL — QR-DQN (opt-in)](#19-distributional-rl--qr-dqn-opt-in)
20. [World Model — Dreamer-style Planning (opt-in)](#20-world-model--dreamer-style-planning-opt-in)

---

## 1. Overview and Design Goals

The DQ* agent (`crates/scematica-nn`) is a pure-Rust reinforcement learning system that learns optimal trading decisions from execution data. It implements several advances over basic DQN:

| Component | Algorithm |
|-----------|-----------|
| Value estimation | Dueling DQN (V + A decomposition) |
| Bootstrap target | Double DQN (online net selects, target net evaluates) |
| Return computation | N-step returns (n=5) |
| Experience sampling | Prioritized Experience Replay (sum-tree, α=0.6) |
| Exploration | ε-greedy with exponential decay (1.0 → 0.05) |
| Regime adaptation | Per-regime separate (online, target) net pairs |
| Hyperparameter search | 3-variant tournament evolution |
| Adversarial robustness | Synthetic rug-pull / pump-and-dump / honeypot injection |
| Return modelling *(v1.2, opt-in)* | QR-DQN distributional returns (51 quantiles/action) — see §19 |
| Planning *(v1.2, opt-in)* | Dreamer-style latent world model + Dyna imagination — see §20 |

**No external ML dependencies.** The entire forward pass, backpropagation, and optimizer are implemented from scratch in safe Rust. This eliminates version compatibility issues and allows deterministic builds across Rust toolchain versions.

---

## 2. Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                     scematica-nn agent                          │
│                                                                 │
│  scematica-trades.jsonl ──► TradeState (24 features)           │
│                                    │                            │
│                              ε-greedy select                    │
│                                    │                            │
│             ┌──────────────────────▼──────────────────────┐    │
│             │              QNetwork (Dueling)              │    │
│             │  [24] → [128] → [64] → { V(s) │ A(s,a) }   │    │
│             │          Q(s,a) = V + A - mean(A)            │    │
│             └──────────────────────┬──────────────────────┘    │
│                                    │                            │
│                          TradeAction (5 choices)                │
│                                    │                            │
│             ┌──────────────────────▼──────────────────────┐    │
│             │     Prioritized Replay Buffer (10k)          │    │
│             │     SumTree: O(log n) priority sampling       │    │
│             └──────────────────────┬──────────────────────┘    │
│                                    │                            │
│                          N-step return (n=5)                    │
│                                    │                            │
│                          Double DQN target:                     │
│                          a* = argmax_a Q_online(s',a)           │
│                          target = r + γ × Q_target(s',a*)       │
│                                    │                            │
│                          backward_step + SGD                    │
│                          lr=0.001, clip=1.0                     │
└─────────────────────────────────────────────────────────────────┘
```

---

## 3. State Space (24 features)

All features are normalised to approximately [0, 1] before being fed to the network.

| Index | Feature | Raw Units | Normalisation |
|-------|---------|-----------|---------------|
| 0  | `pool_age_secs`        | seconds       | ÷ 3600, min 1.0 |
| 1  | `initial_liquidity_sol`| SOL           | ÷ 100, min 1.0 |
| 2  | `price_change_pct`     | fractional    | clamp(-1,3)/3 |
| 3  | `volume_5min_sol`      | SOL           | ÷ 50, min 1.0 |
| 4  | `buy_sell_ratio`       | ratio         | ÷ 5, min 1.0 |
| 5  | `lp_burned`            | bool          | 0.0 or 1.0 |
| 6  | `mint_renounced`       | bool          | 0.0 or 1.0 |
| 7  | `current_pnl_pct`      | fractional    | clamp(-1,2)/2 + 0.5 |
| 8  | `position_age_secs`    | seconds       | ÷ 3600, min 1.0 |
| 9  | `daily_pnl_sol`        | SOL           | clamp(-2,2)/2 + 0.5 |
| 10 | `consecutive_wins`     | count         | ÷ 10, min 1.0 |
| 11 | `consecutive_losses`   | count         | ÷ 10, min 1.0 |
| 12 | `sol_balance_sol`      | SOL           | ÷ 10, min 1.0 |
| 13 | `regime`               | {-1, 0, 1}    | (+1)/2 |
| 14 | `volatility`           | unitless      | clamp(0,1) |
| 15 | `spread_pct`           | fractional    | ÷ 0.1, min 1.0 |
| 16 | `time_of_day_norm`     | UTC hour norm | clamp(0,1) |
| 17 | `open_positions`       | count         | ÷ 5, min 1.0 |
| 18 | `peak_pnl_pct` ¹       | fractional    | clamp(0,5)/5 |
| 19 | `pool_score_norm` ¹    | 0–1           | clamp(0,1) |
| 20 | `deployer_rug_rate` ¹  | EMA 0–1       | clamp(0,1) |
| 21 | `volume_velocity` ¹    | delta SOL/obs | clamp(-1,1)×0.5+0.5 |
| 22 | `price_velocity` ¹     | d(price)/obs  | clamp(-1,1)×0.5+0.5 |
| 23 | `price_acceleration` ¹ | d²(price)/obs²| clamp(-1,1)×0.5+0.5 |

¹ Added in v1.1.0 (STATE_DIM expanded 18 → 24)

### 3.1 Feature Design Rationale

**Features 0–7 (pool fundamentals):** These encode the raw market context the agent cannot control — how old is this pool, how much liquidity, how fast has price moved, what's the buying pressure. They're the primary buy-decision inputs.

**Features 8–13 (position context):** These encode the current state of the position — how long held, daily P&L, streak, balance. They inform exit decisions and risk sizing.

**Features 14–17 (market microstructure):** Volatility, spread, time-of-day, and crowding (open positions) inform whether the current environment is favorable.

**Features 18–23 (v1.1.0 additions):**
- `peak_pnl_pct`: Allows the agent to reason about exit efficiency — "how far off the peak are we?" This mirrors the adaptive pullback formula's peak reference.
- `pool_score_norm`: Gives the agent the same quality signal the rule-based filter sees.
- `deployer_rug_rate`: Allows the agent to become more cautious on repeat-rug deployers.
- `volume_velocity`, `price_velocity`, `price_acceleration`: The kinematic trio — first and second derivatives of market momentum. The price_acceleration feature is the neural equivalent of the velocity decay exit rule.

---

## 4. Action Space (5 actions)

```
ACTION_DIM = 5
```

| Index | Action | Meaning |
|-------|--------|---------|
| 0 | `Hold` | Do nothing — maintain current position |
| 1 | `BuyStandard` | Enter position at configured size |
| 2 | `BuyAggressive` | Enter position at 2× configured size |
| 3 | `SellPartial` | Sell 25–50% of position |
| 4 | `SellAll` | Close position entirely |

The action space was chosen to match the actual decision states the sniper can be in:
- No position → can only Hold or Buy
- In position → can Hold, SellPartial, or SellAll
- Buy actions are masked out when already in a position (the agent cannot stack positions)

The `BuyAggressive` action corresponds conceptually to SuperBuilder-mode entry — a larger bet size for high-conviction setups.

---

## 5. Reward Function

The reward is computed at position close time by `DQNAgent::shape_reward(pnl_pct, hold_steps)`,
where `pnl_pct` is the realized P/L **in percent** and `hold_steps` is position
age **in minutes** (the call site passes `age_secs / 60`). The observer loop
divides the result by 100 before pushing to the replay buffer, so the stored
reward scale is roughly `[-3, +4]` for typical trades.

The function is piecewise — superlinear in profit, zoned in loss:

```
pnl ≥ 0  :  R = pnl × (1 + log₂(1 + pnl/25))  +  timing_bonus
-5 ≤ pnl < 0   :  R = pnl × 1.0     (noise — don't overfit)
-30 ≤ pnl < -5 :  R = pnl × 1.8     (avoidable dip-holding)
-60 ≤ pnl < -30:  R = pnl × 2.5     (failure to cut losses)
pnl < -60      :  R = pnl × 1.5 - 15   if hold_steps == 0  (mercy: unavoidable rug)
                  R = pnl × 2.5 - 70   otherwise           (held through a dump)
```

### 5.1 Superlinear Positive Reward

The `(1 + log₂(1 + pnl/25))` multiplier makes the profit reward superlinear:

| PnL | Multiplier `1+log₂(1+pnl/25)` | Base reward (pre-bonus) |
|-----|-----------|-----------------|
| 0%  | 1.00×     | 0 |
| 25% | 2.00×     | 50 |
| 50% | 2.58×     | 129 |
| 100%| 3.32×     | 332 |
| 200%| 4.17×     | 834 |
| 500%| 5.39×     | 2,696 |

This teaches the agent that holding a 500% winner is worth far more than closing 10 separate 50% winners — it should be reluctant to exit runners early.

### 5.2 Timing Bonus (profit only)

For profitable exits a **discrete** timing bonus is added to the base reward,
keyed on `hold_steps` (minutes):

| Hold time | Bonus |
|-----------|-------|
| `hold_steps == 0` (< 1 min) | **+75** (maximum-efficiency fast snipe) |
| ≤ 3 min | +30 (quick clean exit) |
| ≤ 10 min | +10 (acceptable hold) |
| > 10 min | `−min((hold−10) × 2, 40)` (capital-lock cost, capped at −40) |

This rewards fast capital recycling and penalises sitting on a position past ten
minutes.

### 5.3 Loss Zones

Losses are **multiplied** by an escalating factor by severity (see the piecewise
definition above): tiny losses (≥ −5%) are treated as noise (×1.0) so they don't
drown the profit signal; moderate-to-heavy losses are penalised progressively
(×1.8, ×2.5) to push the agent to cut early. Below −60% (rug territory) a fast
exit (`hold_steps == 0`) receives mercy (×1.5 − 15) because the loss was
unavoidable, while holding through the dump incurs full punishment (×2.5 − 70).

### 5.4 Adversarial Scenario Rewards (Section 14)

Synthetic scenarios are injected at rewards **already on the divided-by-100
scale** so they don't contradict real-trade magnitudes: rug-pull held-through ≈
**−2.95**, fast pump-and-dump peak exit ≈ **+4.03**, honeypot capital-lock ≈
**−2.45**. See §14 for the exact states and actions.

---

## 6. Network Architecture — Dueling DQN

### 6.1 Dueling Architecture

Standard DQN outputs Q(s,a) directly. Dueling DQN decomposes the Q-value into:

```
Q(s,a) = V(s) + A(s,a) - mean_a(A(s,a))
```

Where:
- **V(s)**: State value — "how good is this state regardless of action?"
- **A(s,a)**: Advantage — "how much better is action a than the average action?"
- **mean subtraction**: Makes V and A uniquely identifiable (without it, V could absorb all the value and A would be meaningless)

### 6.2 Layer Structure

```
Input: [24] (state vector)
  │
  ▼
Linear(24 → 128) + ReLU   [He init: w ~ Uniform(-√(2/24), +√(2/24))]
  │
  ▼
Linear(128 → 64) + ReLU   [He init]
  │
  ├───────────────┬───────────────┐
  ▼               ▼               │
Value head:    Advantage head:    │
Linear(64→1)   Linear(64→5)      │
  │               │               │
  ▼               ▼               │
 V(s)          A(s, 0..4)         │
  │               │               │
  └───────────────┘               │
          │                       │
  Q(s,a) = V + A - mean(A)        │
          │                       │
          ▼                       │
   Q-values [5]                   │
```

### 6.3 He Initialisation

Weights are initialised using He (Kaiming) uniform initialisation:

```
bound = √(2 / fan_in)
w ~ Uniform(-bound, +bound)
```

He init is appropriate for ReLU activations. It maintains variance through the network, preventing vanishing or exploding gradients during the initial training phase when the randomly initialised weights are far from optimal.

### 6.4 Gradient Clipping

All gradient updates are clamped to `[-grad_clip, +grad_clip]` (default: 1.0) before the SGD update. This prevents a single high-error sample from causing a catastrophic weight update, which is critical in early training when the replay buffer contains many high-priority samples (they all start at max priority).

---

## 7. Training Algorithm — Double DQN

### 7.1 The Double DQN Problem

Standard DQN computes the bootstrap target as:

```
target = r + γ × max_a Q_target(s', a)
```

The `max_a` operation over the target network tends to overestimate Q-values because the same network is used to both select the best action and evaluate it. Double DQN separates these:

```
a*     = argmax_a Q_online(s', a)     // online net selects
target = r + γ × Q_target(s', a*)    // target net evaluates
```

This breaks the correlation between action selection and evaluation, significantly reducing overestimation bias.

### 7.2 Training Loop (per step)

1. Select action via ε-greedy from online net
2. Execute/observe transition (s, a, r, s', done)
3. Accumulate in n-step buffer (Section 8)
4. When n-step buffer has 5 transitions, push to PER buffer
5. Sample batch of 64 from PER buffer
6. For each sample:
   - Compute a* = argmax Q_online(s')
   - Compute target = r_nstep + γ^n × Q_target(s', a*) × (1 - done)
   - Compute TD error = |Q_online(s, a) - target|
7. Backward pass with IS weights from PER
8. Update priorities in PER with TD errors
9. Every 200 steps: copy online → target (hard update)
10. Decay ε

### 7.3 Hyperparameters

| Parameter | Value | Rationale |
|-----------|-------|-----------|
| Learning rate | 0.001 | Standard for Adam-equivalent SGD |
| Batch size | 64 | Balance memory/compute |
| γ (discount) | 0.99 | Long-horizon — trades span minutes |
| ε start | 1.0 | Full exploration at start |
| ε end | 0.05 | 5% residual exploration |
| ε decay | 0.9995 per step | ~1400 steps to reach 0.5 |
| Target update | every 200 steps | Hard copy |
| Replay capacity | 10,000 | ~1 trading day at 1 trade/minute |
| Grad clip | 1.0 | Prevents catastrophic updates |

---

## 8. N-Step Returns

N-step returns reduce the variance of the bootstrap target by accumulating actual rewards over N steps before bootstrapping from the value function:

```
G_n = r_t + γ × r_{t+1} + γ² × r_{t+2} + ... + γ^{n-1} × r_{t+n-1} + γ^n × V(s_{t+n})
```

With n=5:

```
G_5 = r_0 + γ r_1 + γ² r_2 + γ³ r_3 + γ⁴ r_4 + γ⁵ × Q_target(s_5, a*)
```

**Why n=5?** Trading episodes are long (minutes to hours) and rewards are sparse (only at close). N-step returns propagate the terminal reward backward through the intermediate Hold steps faster than 1-step TD learning, dramatically accelerating convergence.

**Implementation:** The agent maintains a deque of the last 5 transitions. When the deque is full, it computes the n-step return by accumulating discounted rewards from oldest to newest, then pushes a synthetic transition `(s_0, a_0, G_5, s_5, done_5)` to the replay buffer.

---

## 9. Prioritized Experience Replay

### 9.1 Motivation

Uniform sampling from the replay buffer means high-error transitions (where the agent has the most to learn) are sampled at the same rate as low-error transitions (where the agent already has a good estimate). PER samples proportionally to TD error.

### 9.2 Sum-Tree Data Structure

The replay buffer is backed by a power-of-two sum-tree for O(log n) updates and queries:

```
          [total]
         /       \
      [L]         [R]
     /    \      /    \
  [LL]  [LR]  [RL]  [RR]
  ...
  leaf priorities: p_1, p_2, ..., p_n
```

Each leaf stores `priority = (|TD error| + ε)^α`. The root stores the sum of all priorities. To sample, draw a uniform random value in [0, total] and descend the tree until a leaf is reached.

### 9.3 Priority Update Formula

```
priority = (|TD_error| + ε)^α
```

Where:
- `ε = 1e-6`: Small constant ensuring no transition has zero probability
- `α = 0.6`: Exponent controlling the degree of prioritisation (0 = uniform, 1 = pure greedy)

### 9.4 Importance Sampling Correction

Non-uniform sampling introduces bias: frequently sampled transitions dominate gradient updates. IS weights correct for this:

```
w_i = (1 / (N × P(i)))^β / max_j w_j
```

Where:
- `N`: buffer size
- `P(i)`: sampling probability for transition i
- `β`: annealing exponent, starting at 0.4 and increasing to 1.0 (increment 0.001 per sample call)

Gradients are scaled by `w_i` before the weight update. As `β → 1.0`, the correction becomes exact and the algorithm converges to unbiased estimation.

### 9.5 New Transitions Get Maximum Priority

New transitions are inserted with `max_priority` (the largest priority seen so far), ensuring every transition is sampled at least once before it could be displaced. This prevents newly added transitions from being "wasted" because they have no TD error yet.

---

## 10. Exploration — ε-Greedy with Decay

### 10.1 Policy

At each decision step:

```
with probability ε:   select random action (exploration)
with probability 1-ε: select argmax_a Q(s, a) (exploitation)
```

### 10.2 Decay Schedule

```
ε_{t+1} = max(ε_min, ε_t × ε_decay)
```

With `ε_decay = 0.9995`:

| Steps | ε |
|-------|---|
| 0     | 1.000 |
| 200   | 0.905 |
| 500   | 0.779 |
| 1000  | 0.607 |
| 1386  | 0.500 |
| 2000  | 0.368 |
| 2996  | 0.300 (regime branching threshold) |
| 5000  | 0.082 |
| 5990  | 0.050 (ε_min reached) |

### 10.3 Readiness Thresholds

- **`ready_to_advise`**: `train_steps ≥ 10,000` AND `last_q_values` contains at least one finite, non-zero value. This is **step-count based, not ε-based** — at 10k train steps the agent has seen ~2,000 trades, enough for a stable policy before it gates real entries. (Earlier thresholds left pessimistic early weights vetoing every buy.)
- **Regime branching activation**: ε < 0.3. At this point the agent has converged enough that separate regime-specific nets produce meaningfully different policies.

---

## 11. Target Network

The target network is a lagged copy of the online network, updated every 200 steps via hard copy:

```rust
target_net.copy_from(&online_net);
```

### 11.1 Why a Target Network?

Without a frozen target, the Q-value target is computed from the same network being updated. This creates a moving target that causes training to oscillate or diverge. The target network provides a stable reference for `Q_target(s', a*)` while the online network is being updated.

### 11.2 Hard vs Soft Update

Scematica uses hard copy (not Polyak averaging `θ_target ← τθ_online + (1-τ)θ_target`). With the relatively small replay buffer (10k) and batch size (64), hard copy at 200-step intervals provides adequate stability. Polyak averaging would be appropriate if the buffer were much larger and the target needed to track online changes more smoothly.

---

## 12. Regime Branching

The agent maintains separate (online, target) net pairs per market regime. Regime is detected externally and provided as the `regime` state feature (-1/0/1 = bear/neutral/bull) plus the string label for branching.

### 12.1 Activation

Regime branching is enabled when `ε < 0.3` AND the current regime is one of `{bull, bear, sideways, panic}`. Before this threshold, a single net pair handles all regimes.

### 12.2 Rationale

The optimal trading policy differs by market regime:
- **Bull**: Hold winners longer, enter more aggressively
- **Bear**: Tighter exits, smaller sizes
- **Sideways**: Focus on range-trading, avoid new entries
- **Panic**: Very wide stops or no entry, prioritize capital preservation

A single Q-network must implicitly learn all four policies and switch between them based on the `regime` input feature. Separate networks allow dedicated parameterisation per regime without the feature needing to "route" policy implicitly.

### 12.3 Architecture

Each regime net pair is a **standard (non-dueling)** `(QNetwork, QNetwork)` MLP of shape `[24 → 128 → 64 → 5]` — note this differs from the global network, which is dueling. The pair is created **lazily** the first time a regime is engaged (in `notify_regime_shift_labeled`, or in `train_step` when the active regime has no pair yet), He-initialised from scratch — **not** copied from the global net — with the target seeded from its own freshly-initialised online net. Each `train_step` also trains the active regime's online net on a smaller re-sampled batch (≤ 32 transitions), and its target is hard-synced on the same 200-step cadence as the global target.

When a regime shift is signalled, `notify_regime_shift_labeled` also **spikes ε** by +0.25 (capped at 0.40) so the agent re-explores under the new regime rather than applying a stale policy.

---

## 13. Tournament Evolution

The agent supports two related but **distinct** mechanisms: a parallel
**tournament** (`AgentTournament`) that promotes the best of three live variants,
and an optional **hyperparameter hill-climb** (`DQNAgent::evolve_tournament_variants`).

### 13.1 The Tournament (`AgentTournament`)

Three `DQNAgent` variants train in parallel on the same transition stream
(`observe_all` / `train_all`). Every `eval_freq = 1000` steps the variant with the
highest `total_reward` is promoted to primary; "balanced" starts as primary.

| Variant | ε_decay | lr | γ | Style |
|---------|---------|----|----|-------|
| conservative | 0.9999 | 0.0005 | 0.95 | Slowest ε-decay, smallest steps |
| balanced (default primary) | 0.9995 | 0.0010 | 0.99 | Baseline, longest horizon |
| aggressive | 0.9990 | 0.0020 | 0.95 | Fastest ε-decay and learning |

Promotion only changes which variant's action is used — it does **not** reset or
mutate any weights.

### 13.2 Hyperparameter Hill-Climb (`evolve_tournament_variants`)

A separate, optional routine evolves the per-variant hyperparameter triples
`(ε_decay, lr, γ)` held on the agent: the winner's triple is kept and the others
are replaced with mutations of it —

```
new_lr      = winner_lr × Uniform(0.8, 1.2)              clamp [1e-4, 5e-3]
new_ε_decay = winner_ε_decay + Uniform(-0.0005, 0.0005)  clamp [0.998, 0.9999]
new_γ       = winner_γ + Uniform(-0.005, 0.005)          clamp [0.95, 0.999]
```

This mutates **hyperparameters only** (never weights) — a gradient-free,
population-based search across the hyperparameter landscape.

### 13.3 Checkpoint

`AgentTournament` serialises to `scematica-nn-tournament.json` (primary index,
per-variant `total_reward`, per-variant ε). On load it rebuilds three fresh
agents and restores only `primary_idx` / `eval_freq`, since the variant
hyperparameters are fixed at construction.

---

## 14. Adversarial Injection

When `auto_inject_adversarial` is enabled, `train_step` injects synthetic
transitions into the replay buffer every **100 training steps** (2 scenarios per
call, cycling through the three below). Rewards are pre-scaled to the
divided-by-100 reward scale so they match real-trade magnitudes. All three are
**terminal** (`done = true`).

### 14.1 Rug-Pull — held through

```
State: briefly pumped pool, lp_burned=false, mint_renounced=false,
       deployer_rug_rate=0.80  →  next: price −99%, pnl −95%
Action: Hold          Reward: ≈ −2.95
```

Teaches: holding through a collapsing, non-burned pool is heavily punished. The
penalising signal attaches to **Hold** in this state, not to a buy.

### 14.2 Pump-and-Dump — peak exit (a *positive* example)

```
State: fast rise, high peak_pnl_pct, volume_velocity & price_velocity > 0,
       low deployer_rug_rate  →  next: price crashes, but position already
       closed at peak (current_pnl=0)
Action: SellAll       Reward: ≈ +4.03
```

Teaches: selling everything into a vertical pump *before* the dump is the
highest-reward exit. This scenario **rewards** `SellAll`, the opposite of the
penalty the prior revision of this doc described.

### 14.3 Honeypot — capital locked

```
State: absurd buy_sell_ratio (no sells clear), mint_renounced=false,
       position stuck 10–60 min, volume_velocity < 0, deployer_rug_rate=0.90
       →  next: pnl −100%
Action: SellAll       Reward: ≈ −2.45
```

Teaches: the salvage `SellAll` still costs, so the agent should learn to avoid
entering honeypot-shaped states in the first place.

### 14.4 Why Adversarial Injection?

Rug-pulls and honeypots are rare (1–5% of pools). Without injection the agent
trains for thousands of steps before encountering enough of them to learn
avoidance. Periodic injection keeps diverse adversarial examples resident in the
PER buffer, accelerating safety learning. Injection is gated behind the
`auto_inject_adversarial` flag; the always-on counterpart is action rebalancing
(§15).

---

## 15. Action Rebalancing

Every **real** trade ends in a sell, so the replay buffer skews heavily toward
`SellAll` — without correction the agent collapses to a single-action policy. To
keep `Hold` and `SellPartial` represented, `train_step` injects balanced
synthetic transitions every **50 training steps** (always on — distinct from the
`auto_inject_adversarial`-gated injection in §14). Each call adds exactly two
transitions:

- **One `Hold`** on a stable, LP-burned + mint-renounced, gently-rising pool —
  reward **+0.15** (a small *positive*: correctly holding a healthy pumping pool).
- **One `SellPartial`** at a moderate profit — reward
  `shape_reward(pnl × 100, 0) / 100`, the same shaped reward a real partial exit
  at that PnL would earn.

This ensures the patient-hold and partial-exit policies are learned from varied
starting points, not just from the `SellAll`-dominated real trade stream.

---

## 16. Buy Gating & ScemaDEX Integration

The agent is **no longer observer-only**. The `scematica-nn` crate exposes
`advise(state) → (action, q_values)` (greedy, never explores) and
`ready_to_advise()`; the sniper (`crates/scematica-sniper/src/sniper.rs`) consults
them in its live buy gate.

### 16.1 How the sniper uses the agent

Once `ready_to_advise()` is true (§10.3), each candidate pool is scored by
`advise()` and the result **sizes or vetoes** the entry:

- `BuyAggressive` → size **1.5×** the configured entry.
- Mild bearish lean → size **0.5×** (cautious, not blocked).
- A **strong** bearish lean → **veto** the buy entirely.

### 16.2 The veto margin (`NN_VETO_REL_MARGIN = 0.15`)

A buy is *fully* suppressed only when the best sell-side Q exceeds the best
buy-side Q by ≥ 15%:

```rust
let strong_veto = sell_q > 0.0
    && (buy_q <= 0.0 || sell_q >= buy_q * (1.0 + 0.15));
```

A weaker bearish lean does **not** kill the entry — it downgrades sizing to 0.5×.
This deliberately prevents a partially-converged net from silently suppressing
the proven rule-based edge (PF ≈ 6.5): the agent can *shade* entry size long
before it is trusted to block trades outright.

### 16.3 Stats published to the dashboard

`stats()` returns an `AgentStats` snapshot, written to `scematica-nn-stats.json`
(atomic temp-then-rename) for the dashboard NN panel:

```json
{
  "step_count": 84320,
  "train_steps": 12050,
  "epsilon": 0.087,
  "replay_size": 10000,
  "total_reward": 1847.3,
  "avg_loss": 0.0042,
  "target_updates": 60,
  "ready_to_advise": true,
  "last_action": "BUY_AGG",
  "last_q_values": [0.12, 0.41, 0.55, -0.03, 0.08]
}
```

### 16.4 The agent as a ScemaDEX route policy

Beyond the sniper, the same `DQNAgent` is consumed by the ScemaDEX SDK layer.
`scemadex-integrations::jupiter::JupiterRoutePolicy::with_agent(DQNAgent)` uses
`advise()` to set the **conviction** that sizes a Conviction-Routing bond
(discounted by the Jupiter quote's price impact), and `observe_outcome()` closes
the reinforcement loop from the realised fill vs. the bonded promise. The agent
that gates the bot's entries can thus also price the SDK's bonded inferences —
see [`scemadex.md`](scemadex.md).

Runnable: `cargo run -p scemadex-integrations --example agent_conviction` loads
the bot's checkpoint (`SCEMATICA_NN_CHECKPOINT`, default `scematica-nn-agent.json`),
prices a bond against a **real Jupiter quote** with the agent's conviction, and
closes the loop. The live `sdk-dashboard --live` loads the same checkpoint via
`with_agent_from_checkpoint`, so the running TUI prices bonds with the trained
weights when a checkpoint is present.

---

## 17. Persistence — Checkpoint Format

### 17.1 Agent Checkpoint (`scematica-nn-agent.json`)

Saved every 10 minutes and on clean shutdown (atomic temp-then-rename). The
`Checkpoint` struct contains:

```json
{
  "online_net":  { /* QNetwork: layers, value_head, advantage_head */ },
  "target_net":  { /* QNetwork */ },
  "epsilon": 0.087,
  "step_count": 84320,
  "train_steps": 12050,
  "total_reward": 1847.3,
  "target_updates": 60,
  "regime_nets": { /* per-regime (online, target) pairs */ },
  "active_regime": "bull",
  "state_dim": 24,
  "action_dim": 5
}
```

`state_dim` / `action_dim` are recorded so that on load, a checkpoint whose
dimensions no longer match the current `STATE_DIM`/`ACTION_DIM` is **silently
discarded** (a fresh agent is returned) rather than crashing on mismatched weight
shapes — see §17.4.

### 17.2 Stats File (`scematica-nn-stats.json`)

The `AgentStats` snapshot (see §16.3 for the full field set):

```json
{
  "step_count": 84320,
  "train_steps": 12050,
  "epsilon": 0.087,
  "replay_size": 10000,
  "total_reward": 1847.3,
  "avg_loss": 0.0042,
  "target_updates": 60,
  "ready_to_advise": true,
  "last_action": "BUY_AGG",
  "last_q_values": [0.12, 0.41, 0.55, -0.03, 0.08]
}
```

### 17.3 Tournament File (`scematica-nn-tournament.json`)

Written every 1000 steps:

```json
{
  "primary_variant": 1,
  "variants": [
    { "name": "conservative", "total_reward": 1200.0 },
    { "name": "balanced", "total_reward": 1847.3 },
    { "name": "aggressive", "total_reward": 1540.1 }
  ]
}
```

### 17.4 Checkpoint Compatibility

Two independent mechanisms keep old checkpoints from crashing the loader:

- **Serde defaults on the dueling heads.** `QNetwork.value_head` /
  `advantage_head` are `#[serde(default)]`, so a pre-dueling checkpoint (heads
  absent) deserialises with both `None` and runs the standard (non-dueling)
  forward path.
- **Dimension guard.** `Checkpoint` records `state_dim` / `action_dim`. On load,
  if either differs from the current `STATE_DIM` (24) / `ACTION_DIM` (5) — e.g. a
  checkpoint from the 18-feature v1.0.0 era — the checkpoint is discarded and a
  fresh agent is returned, rather than panicking on mismatched weight matrices.
  (A missing `state_dim` / `action_dim` field defaults to the legacy `18` / `5`.)

---

## 18. Version History

### v1.0.0 — Initial DQN
- Standard MLP: `[18] → 128 → 64 → [5]`
- Simple replay buffer (uniform sampling)
- 1-step TD learning
- No regime branching, no tournament

### v1.1.0 — Major Upgrade (current)
- **State expansion**: 18 → 24 features (peak_pnl, pool_score, deployer_rug_rate, volume_velocity, price_velocity, price_acceleration)
- **Dueling DQN**: Separate V(s) and A(s,a) heads
- **Double DQN**: Decoupled action selection / evaluation
- **N-step returns**: n=5, accumulated in a transition deque
- **Prioritized Experience Replay**: Sum-tree, α=0.6, β: 0.4→1.0
- **Regime branching**: Separate (standard) nets per regime, activates at ε < 0.3
- **Tournament evolution**: 3-variant hyperparameter search
- **Adversarial injection**: Rug-pull, pump-and-dump, honeypot scenarios
- **Action rebalancing**: Synthetic Hold/SellPartial every 50 steps
- **Checkpoint compatibility**: Old checkpoints load without dueling heads (graceful degradation)

### Doc revision — 2026-06-05 (verified against source)
This document was re-checked line-by-line against `crates/scematica-nn` and
corrected where it had drifted from the code: the **reward function** (discrete
timing bonus, multiplied loss zones, rug mercy — §5), the **adversarial
scenarios** (correct actions/rewards; pump-and-dump is a *positive* SellAll
example — §14), **action rebalancing** (one Hold +0.15 / one SellPartial; the real
skew is toward SellAll — §15), the **`ready_to_advise`** condition (`train_steps ≥
10,000`, not ε-based — §10.3/§16), **regime nets** (standard, not dueling — §12.3),
the **tournament configs** (§13), and the now-**live buy gate** with its veto
margin and ScemaDEX route-policy reuse (§16). No code was changed; only the
documentation was brought into agreement with it.

---

*This document should be updated whenever the network architecture, state space, action space, reward function, or training algorithm is modified. Version alongside `crates/scematica-nn/Cargo.toml`.*

---

## 19. Distributional RL — QR-DQN (opt-in)

Source: `crates/scematica-nn/src/distributional.rs`.

The classic path predicts a single scalar `Q(s,a)` — the *expected* return. A
distributional agent instead predicts the **entire return distribution**
`Z(s,a)`, represented as `N_QUANTILES = 51` learned quantiles per action. This
is the quantile-regression parameterisation (QR-DQN; Dabney et al. 2017), chosen
over categorical C51 because it needs no fixed value support and no categorical
projection — which keeps the from-scratch f64 backprop tractable.

**Architecture** — the dueling decomposition, generalised per quantile:

```
trunk (ReLU, [STATE_DIM, 128, 64]) ─┬─► value head V:  → N_QUANTILES
                                    └─► advantage head A: → ACTION_DIM · N_QUANTILES
Z(s,a)_i = V_i + A(a)_i − mean_b A(b)_i          (i indexes the quantile)
Q(s,a)   = (1/N) Σ_i Z(s,a)_i                    (mean-of-quantiles = drop-in Q)
```

Because `q_values()` returns the mean of quantiles, **every existing consumer**
(`advise`, `greedy_action`, the sniper's Q-value buy gate, the decision
explainer) works unchanged.

**Loss** — quantile Huber loss over all (predicted-quantile *i*, target-sample
*j*) pairs, with quantile midpoints `τ_i = (i + 0.5)/N`:

```
u        = T_j − θ_i
huber(u) = ½u²            if |u| ≤ κ   else   κ(|u| − ½κ)        (κ = 1)
ρ_ij     = |τ_i − 1{u<0}| · huber(u) / κ
```

**Bellman target** (Double DQN, per quantile): the online net selects the greedy
next action `a*` by mean-of-quantiles, the target net supplies its distribution,
and `T_j = r + γ · Z_target(s', a*)_j` (or `r` on terminal). The mean of this
target equals the scalar Double-DQN target, so PER priorities
(`|mean(target) − Q(s,a)|`) stay comparable across modes.

**Why it matters** — modelling the full distribution is more sample-efficient
and *keeps the fat left tail* that rugs create, rather than averaging it away.
This is the substrate for future risk-sensitive action selection (e.g. CVaR).

**Risk-sensitive selection (CVaR).** Because the full distribution is available,
actions can be chosen by **CVaR** — the mean of the worst `alpha`-fraction of
outcomes — instead of the mean (`QuantileNetwork::cvar_values`, `DQNAgent::
set_risk_alpha`, env `SCEMATICA_NN_CVAR_ALPHA`). `alpha = 1.0` is risk-neutral;
smaller `alpha` structurally avoids the fat left tail that rugs create. In the
adversarial-sim A/B (`examples/ab_benchmark.rs`), CVaR at 0.25 is a strong
downside guard — it beats both the scalar and the mean-based distributional agent
on overall reward by refusing to buy rug/honeypot pools — while a mean-based
policy captures more upside but bleeds it back on the tail archetypes. Treat CVaR
as a capital-preservation dial, and **measure on your data before flipping it
live**: at modest training budgets the distributional machinery is not yet a
proven PnL improvement, only a proven loss-avoider.

**Enabling** — off by default. A fresh agent starts distributional when
`SCEMATICA_NN_DISTRIBUTIONAL=1`. Distributional mode cannot be retrofitted onto
an existing scalar checkpoint (weight shapes differ); scalar checkpoints continue
to load and run scalar. Distributional checkpoints round-trip via `#[serde(default)]`
`dist_online`/`dist_target` fields and are shape-guarded on load.

Regime branching (§12) is a scalar-mode feature and is not run in distributional
mode.

---

## 20. World Model — Dreamer-style Planning (opt-in)

Source: `crates/scematica-nn/src/world_model.rs`.

A latent world model learns the *dynamics of the market itself* so the agent can
train on **imagined** trajectories in addition to real ones. Live trades are
scarce and expensive; a learned model lets the agent dream many plausible
roll-outs per real step (Dyna-style planning) — the lever that compounds the
edge without risking more live capital.

**Components** (compact modular design à la Ha & Schmidhuber / Dreamer, adapted
to pure-Rust f64):

```
encoder  : state(24)          → latent z(16)          (compress observation)
decoder  : latent z(16)       → reconstructed state   (ground the latent)
dynamics : [z(16), onehot(a)] → next latent ẑ'        (imagine forward)
reward   : [z(16), onehot(a)] → r̂                     (imagine payoff)
```

**Training** is modular with stop-gradients: encoder+decoder train jointly as an
autoencoder (`‖decode(encode(s)) − s‖²`, gradient flows dec→enc); dynamics and
reward train on *detached* latents (`‖dynamics(z,a) − z'‖²`, `‖reward(z,a) − r‖²`).
Every backward pass is a simple chain — no cross-module autodiff graph.

**Imagination** (`imagine`) rolls dynamics + reward forward from a real start
state, decoding each latent back to a 24-dim state vector. Produced transitions
live in the *same* space as the replay buffer, so `imagine_into_replay(rollouts,
horizon)` folds them straight in as synthetic experience (Dyna). Actions are
chosen greedily off the imagined state with a 20% exploration chance.
`prediction_error(s,a,s')` gives a one-step latent error usable as an
intrinsic-curiosity signal or a gate on trusting rollouts.

**Live loop** — when attached, the sniper spawns a task (`main.rs`) that every
15 s runs a few `train_world_model_step()` calls and one
`imagine_into_replay(8, 4)` (up to 32 dreamed transitions per tick).

**Enabling** — off by default; set `SCEMATICA_NN_WORLD_MODEL=1`. Unlike the
distributional policy, the world model is orthogonal to the policy weights and
can be attached to **any** agent, including a loaded scalar checkpoint. It
round-trips via a `#[serde(default)]` `world_model` field, shape-guarded on load.

**Env flags summary**

| Variable | Effect |
|---|---|
| `SCEMATICA_NN_DISTRIBUTIONAL=1` | Fresh agents use QR-DQN distributional returns |
| `SCEMATICA_NN_WORLD_MODEL=1` | Attach the latent world model + Dyna imagination |
| `SCEMATICA_NN_CVAR_ALPHA=0.25` | Risk-sensitive CVaR action selection at this α (distributional only) |
| `SCEMATICA_NN_PRETRAIN_EPISODES=500` | Pre-train on the adversarial simulator at boot before going live |

### Closing the flywheel — scar-driven pre-training

`SCEMATICA_NN_PRETRAIN_EPISODES=N` makes the sniper run `N` adversarial-simulator
episodes at boot (`DQNAgent::pretrain_on_simulator`), hardening a fresh agent
against rug/honeypot/pump-dump scenarios *before* it risks live capital. The
simulator's `ScarProfile` is loaded from `scematica-scar-profile.json` when
present — that file is how live **Scar-Market** statistics feed the agent: the
bot writes it via `scemadex_integrations::scar_profile::write_scar_profile`,
which derives the failure *rate* (from the bond honored/slashed ledger) and
*severity* (from mean slashed collateral) from real verified failures. Absent the
file, a sensible default distribution is used. This is the loop: **the market's
un-fakeable failures shape what the next agent learns to avoid.**

> Note: `doubt_spread`/market-conviction as *live per-pool* NN state features is
> deliberately deferred until there's a live per-pool doubt source to populate
> them — adding always-zero features would only reset checkpoints for no signal.
