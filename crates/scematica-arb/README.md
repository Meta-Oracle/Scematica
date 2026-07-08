# scematica-arb

Cross-DEX arbitrage engine for [Scematica](https://github.com/Meta-Oracle/Scematica) —
graph search across **Raydium, Orca, Meteora, and Jupiter** for profitable cycles, with
**atomic profit-or-revert** execution.

## What it does

- Builds a live pool graph and searches for arbitrage cycles (`graph.rs`, `searcher.rs`,
  `opportunity.rs`).
- Sizes and executes the best path atomically (`executor.rs`), optionally gated/scored by
  the `scematica-ai` layer.
- Multi-DEX swap instructions come from `scematica-executor`.

## Program-less by default (no on-chain deploy)

Arbitrage runs with **no custom Solana program**. Solana transactions are atomic, so the
profit guarantee comes for free: the final hop's `min_out` is set to
`input + profit_floor`, and any shortfall fails that swap and reverts the whole
transaction — your capital is untouched.

```bash
cargo run --release --bin arb          # program-less (no SWAP_PROGRAM_ID) — zero deploy
SWAP_PROGRAM_ID=<id> cargo run ...      # use a deployed scematica-swap program instead
ARB_PROGRAM_LESS=1|0 cargo run ...      # force either path
```

Program-less mode is for **self-funded** arbitrage (you hold the starting capital); a
flash-loan (borrow-in-transaction) strategy needs a flash-loan protocol's program.

## Install

```bash
cargo install scematica-arb
arb --help
```

Part of the Scematica suite. See the [workspace README](https://github.com/Meta-Oracle/Scematica)
for the full bot (sniper, dashboard, AI, Deep Q*, x402).

## License

MIT
