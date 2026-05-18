# Scematica DQ* Agent — Technical Reference

> Version 1.1.0 | Last updated: 2026-05-18  
> Full technical documentation of the Deep Q* reinforcement learning agent in `crates/scematica-nn`.

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
16. [Observer Mode and Buy Gating](#16-observer-mode-and-buy-gating)
17. [Persistence — Checkpoint Format](#17-persistence--checkpoint-format)
18. [Version History](#18-version-history)

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

The reward is computed at position close time and injected as the terminal reward for the episode:

```
R = pnl × (1 + log₂(1 + pnl/25))    if pnl > 0
R = pnl                               if pnl ≤ 0
```

Where `pnl` is the realized profit/loss in percent.

### 5.1 Superlinear Positive Reward

The `(1 + log₂(1 + pnl/25))` multiplier makes rewards superlinear in positive PnL:

| PnL | Multiplier | Effective reward |
|-----|-----------|-----------------|
| 0%  | 1.00×     | 0 |
| 25% | 2.00×     | 50 |
| 50% | 2.58×     | 129 |
| 100%| 3.00×     | 300 |
| 200%| 3.58×     | 716 |
| 500%| 4.32×     | 2,158 |

This teaches the agent that holding a 500% winner is worth far more than closing 10 separate 50% winners — it should be reluctant to exit runners early.

### 5.2 Timing Bonus

An additional reward is given for fast profitable exits:

```
timing_bonus = +0.5 × (1 - position_age_secs / 3600)   if pnl > 0 AND position_age_secs < 3600
```

Range: 0 to +0.5 (maximum for instant exit, zero for 1-hour holds). This discourages the agent from holding losers indefinitely hoping for recovery.

### 5.3 Loss Penalty

Losses are penalized linearly (no multiplier). This asymmetry is intentional — large gains are rewarded disproportionately but large losses are not penalized disproportionately, avoiding excessive loss aversion that would make the agent too conservative.

### 5.4 Adversarial Scenario Rewards (Section 14)

Synthetic rug-pull and pump-and-dump scenarios inject extreme negative rewards (-100) into the replay buffer. This teaches the agent to associate states like `lp_burned=false, mint_renounced=false, volume_velocity < 0` with catastrophic outcomes even before the bot has seen enough real rugs.

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
| 1386  | 0.500 (ready_to_advise threshold) |
| 2000  | 0.368 |
| 2996  | 0.300 (regime branching threshold) |
| 5000  | 0.082 |
| 5990  | 0.050 (ε_min reached) |

### 10.3 Readiness Thresholds

- **`ready_to_advise`**: ε < 0.5 AND replay_size ≥ batch_size (64). The agent is sufficiently trained to be referenced by the dashboard and future buy-gate integration.
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

Each regime net pair is a full `(QNetwork, QNetwork)` dueling network. They are initialised as copies of the global online net when regime branching activates, so they start from a good initialization rather than random.

---

## 13. Tournament Evolution

Three variant agents run in parallel, each with slightly mutated hyperparameters. Every 1000 steps the winner (highest `total_reward`) is promoted as the primary agent.

### 13.1 Variant Configuration

| Variant | lr | ε_decay | γ | Style |
|---------|----|---------|----|-------|
| Conservative | 0.0008 | 0.9990 | 0.98 | Slower learning, higher discount |
| Balanced | 0.001 | 0.9995 | 0.99 | Baseline |
| Aggressive | 0.0012 | 0.9998 | 0.995 | Faster learning, less epsilon decay |

### 13.2 Mutation on Promotion

After a winner is promoted, all three variants are re-initialised from the winner's weights with ±20% perturbation to hyperparameters:

```
new_lr = winner_lr × Uniform(0.8, 1.2)
new_ε_decay = winner_ε_decay ± 0.005
new_γ = winner_γ ± 0.005
```

This is a simplified evolutionary strategy (ES) — gradient-free optimization of hyperparameters using population-based selection.

### 13.3 Checkpoint

The tournament state is saved to `scematica-nn-tournament.json` every 1000 steps, including per-variant rewards and the current primary variant index.

---

## 14. Adversarial Injection

To train the agent on scenarios it may not see frequently (rugs, honeypots, pump-and-dumps), synthetic transitions are injected into the replay buffer every 50 training steps.

### 14.1 Rug-Pull Scenario

```
State: { lp_burned: false, mint_renounced: false, buy_sell_ratio: 3.0,
         volume_velocity: -0.8, price_change_pct: 0.5 }
Action: BuyStandard
Reward: -100  (catastrophic loss)
Done: true
```

Teaches the agent: high buy pressure on a non-burned LP that then has collapsing volume → rug. `BuyStandard` in this state should have a very negative Q-value.

### 14.2 Pump-and-Dump Scenario

```
State: { price_change_pct: 2.0, price_velocity: 0.9, price_acceleration: -0.7,
         lp_burned: false, pool_age_secs: 300 }
Action: BuyAggressive  (bought top of pump)
Reward: -80
Done: true
```

Teaches the agent: strong price move with accelerating upward velocity that then decelerates → entering near the top of a pump. `BuyAggressive` at high `price_change_pct` with negative `price_acceleration` should be penalized.

### 14.3 Honeypot Scenario

```
State: { lp_burned: true, mint_renounced: false, buy_sell_ratio: 5.0,
         current_pnl_pct: 0.8, position_age_secs: 600 }
Action: SellPartial  (could not sell — honeypot)
Reward: -100
Done: true
```

Teaches the agent: even with LP burned, a honeypot contract can block sells. High buy/sell ratio (because nobody can sell) with no mint authority renounced is a red flag — `Hold` is wrong here, `SellAll` at first opportunity is correct.

### 14.4 Why Adversarial Injection?

Rug-pulls are rare (1–5% of pools in normal market conditions). At 1 trade per minute and 5% rug rate, the agent might see only ~70 rugs in a day. Without injection, the agent trains for thousands of steps before learning to avoid them. Injection ensures the replay buffer always contains diverse adversarial examples, dramatically accelerating safety learning.

---

## 15. Action Rebalancing

The action distribution in the replay buffer is heavily skewed toward `Hold` — most timesteps during a trade do nothing. Without correction, the agent would overfit to `Hold`.

Every 50 training steps, synthetic `Hold` and `SellPartial` transitions are rebalanced:

```
for each rebalancing step:
    inject 5 synthetic Hold transitions (small negative reward: -0.01)
    inject 3 synthetic SellPartial transitions at current pnl levels
```

The `Hold` penalty of -0.01 per step teaches the agent that doing nothing is mildly costly — it should eventually act, not hold indefinitely.

The `SellPartial` injections at a range of PnL levels help the agent learn the partial exit policy from varied starting points, not just from real trades (which tend to cluster around the actual TP/SL thresholds).

---

## 16. Observer Mode and Buy Gating

### 16.1 Current Status

The agent currently operates in **observer mode**: it trains on real trade data from `scematica-trades.jsonl` but does not yet gate buy decisions in the sniper.

The agent publishes its state to `scematica-nn-stats.json` every 5 seconds:

```json
{
  "epsilon": 0.342,
  "steps": 8432,
  "replay_size": 10000,
  "total_reward": 1847.3,
  "ready_to_advise": true,
  "recommended_action": "BUY",
  "regime": "bull"
}
```

### 16.2 Ready-to-Advise Condition

```
ready_to_advise = ε < 0.5 AND replay_size ≥ 64
```

When `ready_to_advise = true`, the agent is considered sufficiently trained to provide meaningful recommendations. The dashboard displays this status in the NN Stats panel.

### 16.3 Planned Buy Gate Integration

When the agent is promoted to active mode (future version), the sniper's buy gate will consult the agent:

```
if ready_to_advise AND agent.recommend(state) == BuyStandard:
    proceed with standard buy
elif ready_to_advise AND agent.recommend(state) == BuyAggressive:
    proceed with 2× size buy
elif ready_to_advise AND agent.recommend(state) == Hold:
    skip this pool
```

The gate only activates when `ε < 0.3` (regime branching threshold) to ensure the agent has meaningfully converged before it can block real trades.

---

## 17. Persistence — Checkpoint Format

### 17.1 Agent Checkpoint (`scematica-nn-agent.json`)

Saved every 10 minutes and on clean shutdown. Contains:

```json
{
  "online": { /* QNetwork: layers, value_head, advantage_head */ },
  "target": { /* QNetwork */ },
  "epsilon": 0.342,
  "steps": 8432,
  "total_reward": 1847.3,
  "regime_nets": { /* optional, per regime */ }
}
```

### 17.2 Stats File (`scematica-nn-stats.json`)

Written every 5 seconds (atomic rename):

```json
{
  "epsilon": 0.342,
  "steps": 8432,
  "replay_size": 10000,
  "total_reward": 1847.3,
  "ready_to_advise": true
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

The `QNetwork` struct uses `#[serde(default)]` on `value_head` and `advantage_head`. This means:
- Old checkpoints (standard DQN without dueling heads) load cleanly — heads default to `None`
- New checkpoints (dueling) load correctly with the head weights
- Mixed loads work: an old checkpoint can be loaded, the dueling fields will be randomly initialised on the next `new_dueling()` call

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
- **Regime branching**: Separate nets per regime, activates at ε < 0.3
- **Tournament evolution**: 3-variant hyperparameter search
- **Adversarial injection**: Rug-pull, pump-and-dump, honeypot scenarios
- **Action rebalancing**: Synthetic Hold/SellPartial every 50 steps
- **Checkpoint compatibility**: Old checkpoints load without dueling heads (graceful degradation)

---

*This document should be updated whenever the network architecture, state space, action space, reward function, or training algorithm is modified. Version alongside `crates/scematica-nn/Cargo.toml`.*
