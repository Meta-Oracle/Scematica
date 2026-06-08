# scematica-core

Core types, traits, and shared utilities for the [Scematica](https://github.com/Meta-Oracle/Scematica)
Solana trading stack.

Provides the building blocks the rest of the stack shares:

- **Config** — `BotConfig` and friends, loaded from env / TOML.
- **RPC** — `RpcConnection`, a thin async wrapper over `solana-client`.
- **Wallet** — keypair loading from file / WSL / base58 sources.
- **Metrics** — file-based metric snapshots used for cross-process IPC.
- **Token utils** — base-unit ↔ UI conversions, known-token tables, Token-2022 helpers.

This crate has no internal Scematica dependencies and is consumed by
`scematica-executor`, `scematica-ai`, `scematica-protocol`, `scematica-sniper`,
and `scematica-dashboard`.

## License

MIT
