# scematica-sniper

The new-pool **sniper engine** for the
[Scematica](https://github.com/Meta-Oracle/Scematica) Solana trading stack.

Listens for Raydium AMM V4 new-pool events (plus pump.fun and whale-copy
sources), runs each candidate through a fail-open **filter pipeline**, sizes and
executes entries, then monitors positions with a two-phase TP/SL sell loop. Ships
independent risk breakers (ATH drawdown, grief, fractional Kelly, pool scorer,
deployer reputation, multi-RPC failover) and a backtester.

```bash
cargo run --bin sniper                                   # live (needs keypair + RPC)
cargo run --bin backtest -- --pools pools.jsonl --tp 100 --sl 15
```

The sniper communicates with `scematica-dashboard` purely through JSON files in
the working directory (no socket IPC).

> Trading software — use at your own risk. Nothing here is financial advice.

## License

MIT
