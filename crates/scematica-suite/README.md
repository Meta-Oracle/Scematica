# scematica-suite

The **batteries-included meta-crate** for [Scematica](https://github.com/Meta-Oracle/Scematica) —
a single dependency (and a single launcher) for the whole Solana sniper +
cross-DEX arb + AI + Deep Q\* + x402 + ScemaDEX stack.

## As a library

One dependency re-exports every component crate:

```toml
[dependencies]
scematica-suite = "1"
```

```rust
use scematica_suite::{core, executor, protocol, ai, nn, sniper, dashboard, scemadex, sentience};

let agent = nn::DQNAgent::default();
let dex = scemadex::reference_client();
```

| Re-export | Crate | What |
|-----------|-------|------|
| `core` | `scematica-core` | config, RPC, wallet, metrics, token utils |
| `executor` | `scematica-executor` | multi-DEX + Jupiter swap builders |
| `protocol` | `scematica-protocol` | x402 payment facilitator |
| `ai` | `scematica-ai` | LLM agent layer |
| `nn` | `scematica-nn` | pure-Rust Double/Dueling Deep Q\* |
| `sniper` | `scematica-sniper` | new-pool sniper engine |
| `dashboard` | `scematica-dashboard` | ratatui monitoring TUI |
| `scemadex` | `scemadex-sdk` | agentic-liquidity SDK |
| `sentience` | `scematica-sentience` | Ψ/Ω cognitive architecture + LLM gating overlay |

Every re-export except `sentience` also ships a binary; `sentience` is a library
only, so it has no launcher subcommand.

## As a launcher

Installing the suite gives a `scematica` command that dispatches to the
component binaries:

```bash
cargo install scematica-suite
scematica help
scematica dashboard --demo     # runs the `dashboard` binary
scematica ddqn                 # runs `scema-ddqn`
scematica scemadex             # runs `scemadex`
```

The launcher resolves each component binary next to its own executable (so a
`cargo install` of the component crates lands them in the same `~/.cargo/bin`)
or anywhere on `PATH`. To install the launcher **and** every runnable in one go:

```bash
cargo install scematica-suite scematica-dashboard scematica-sniper \
              scematica-protocol scematica-nn scemadex-sdk
```

Then `scematica dashboard`, `scematica sniper`, `scematica protocol`,
`scematica backtest`, `scematica ddqn`, and `scematica scemadex` all work. If a
component isn't installed, the launcher prints the exact `cargo install` command
to add it.

## License

MIT
