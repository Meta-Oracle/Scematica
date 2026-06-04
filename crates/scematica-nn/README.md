# scematica-nn

A pure-Rust **Double / Dueling Deep Q\*** reinforcement-learning agent with **no
external ML-framework dependency** — the network, replay buffer, and training
loop are all implemented from scratch in safe Rust.

Originally built to drive trade decisions in the [Scematica](https://crates.io/crates/scemadex-sdk)
Solana trading stack, it is general enough for any discrete-action RL task.

## Features

- **Dueling DQN** head: `Q(s,a) = V(s) + A(s,a) − mean(A)`
- **Double DQN** target: online net selects actions, target net evaluates them
- **Prioritized experience replay** with N-step returns
- Epsilon-greedy exploration with decay; periodic hard target-net copy
- Per-regime network branching and a parallel agent tournament
- JSON checkpointing with dimension-mismatch–safe loading
- Serde-serializable state/stats for external dashboards

## Quick start

```rust
use scematica_nn::{DQNAgent, TradeState};

let mut agent = DQNAgent::default();
let state = TradeState::default();
let (action, q_values) = agent.advise(&state);
println!("chose {} — Q={:?}", action.label(), q_values);
```

## Core types

| Type | Purpose |
|------|---------|
| `DQNAgent` | The agent: `advise`, `observe`, `save`/`load`, `stats` |
| `TradeState` | 24-feature state vector, normalized to `[0,1]` |
| `TradeAction` | `Hold` · `BuyStandard` · `BuyAggressive` · `SellPartial` · `SellAll` |
| `AgentTournament` | Runs conservative/balanced/aggressive variants in parallel |

## License

MIT
