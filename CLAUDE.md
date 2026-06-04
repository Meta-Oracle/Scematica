# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Overview

Scematica is a Rust Solana sniper + cross-DEX arbitrage bot with a ratatui TUI dashboard, a Deep Q* reinforcement-learning agent, and a Rust-native x402 payment protocol server. Targets Raydium AMM V4 new-pool events on Solana mainnet. Gated behind a 250k $SCEMA token balance (mint `AbKiP2Jc6nM7937jTDfqoJC1bsg5FQ24Buk2iqRFpump`).

## Build, Run, Test

```powershell
# Build all binaries (release, ~5-10 min cold)
cargo build --release

# Run primary binaries
cargo run --release --bin dashboard          # TUI entry point
cargo run --release --bin dashboard -- --demo  # No keypair/RPC required
cargo run --release --bin sniper             # Sniper standalone
cargo run --release --bin arb                # Arb standalone
cargo run --release --bin scematica-protocol -- --pay-to <wallet> --price-lamports 10000

# Backtester (replays JSONL pool history through filter pipeline + TP/SL sim)
cargo run --release --bin backtest -- --pools historical-pools.jsonl --tp 100 --sl 15

# Tests (no integration test dirs; tests are inline `#[test]` / `#[tokio::test]` in src files)
cargo test --workspace
cargo test -p scematica-sniper kelly         # Filter to a single module
cargo test -p scematica-nn agent::tests::    # Run one module's tests in a crate

# Lint / format
cargo clippy --workspace --all-targets
cargo fmt --all

# On-chain Anchor program (built separately — excluded from cargo workspace)
cd programs/scematica-swap && anchor build
```

The dashboard launches the sniper as a child process and prefers `target/release/sniper.exe` over `target/debug/sniper.exe` — rebuild release before running the dashboard if you change sniper code.

## Critical Dependency Constraints

The workspace pins old versions for hard transitive-dep reasons documented in `Cargo.toml`. **Do not bump these without reading the comments first:**

- `solana-sdk` / `solana-client` / `solana-program` pinned to **1.18.26**. 2.x requires sweeping code changes and produces unresolvable `zeroize` version conflicts with `reqwest ≥ 0.12` / `rustls ≥ 0.22`.
- `reqwest` pinned to **0.11**. 0.12 pulls `rustls 0.23` which requires `zeroize ≥ 1.7`, conflicting with `curve25519-dalek 3` (transitively required by `solana-sdk` via `ed25519-dalek`, capped at `< 1.4`).
- `tokio-tungstenite` pinned to **0.21** for compatibility with the `tungstenite` version solana-sdk transitively requires.
- `base64` pinned to **0.21**. 0.22 removes the legacy `decode`/`encode` functions used in `jupiter.rs` and the sniper executor.

If a new dep pulls in a conflicting `zeroize` or `rustls`, the build error will be in a transitive crate — start at the top of `Cargo.toml` and re-read the pin comments before trying upgrades.

## Workspace Layout

```
crates/
  scematica-core/       Shared: config, RPC, wallet, metrics, types, token utils
  scematica-sniper/     Pool listener + filter pipeline + sniper logic + alerts + backtester
                        Bins: `sniper` (main.rs), `backtest` (bin/backtest.rs)
  scematica-arb/        Cross-DEX arb graph search (Raydium/Orca/Meteora). Bin: `arb`
  scematica-executor/   Multi-DEX swap instruction builders + Jupiter integration
  scematica-ai/         LLM agents (Groq/xAI): Chat, Strategy, Risk, Debate, Report
  scematica-nn/         Pure-Rust Deep Q* (Double DQN) agent — no external ML deps
  scematica-dashboard/  Ratatui TUI (6 tabs). Bin: `dashboard`
  scematica-protocol/   x402 HTTP 402 payment server. Bin: `scematica-protocol`
tools/
  key-converter/        Keypair format conversion
  pool-seeder/          Pre-seeds pool-cache.json from on-chain state
programs/
  scematica-swap/       Anchor on-chain program (NOT in cargo workspace)
```

## Architecture: File-Based IPC

The sniper and dashboard are separate processes that communicate exclusively through JSON files in the working directory. There is no socket/IPC channel — touching one of these files is how the dashboard issues commands, and tailing them is how the dashboard observes state. **When adding cross-process behavior, follow this pattern; don't introduce new IPC mechanisms.**

| File | Writer | Reader | Purpose |
|---|---|---|---|
| `scematica-sniper.log` | sniper (tracing) | dashboard (tail) | Log stream |
| `scematica-sniper.lock` | sniper | sniper | Single-instance guard (PID) |
| `scematica-trades.jsonl` | sniper | dashboard, NN agent | Append-only trade events |
| `scematica-metrics.json` | sniper | dashboard | Snapshot every 5s |
| `scematica-filter-stats.json` | sniper | dashboard | Per-filter rejection counts |
| `scematica-nn-stats.json` | NN agent | dashboard | ε, steps, replay size, total reward |
| `scematica-nn-agent.json` | NN agent | NN agent | Model checkpoint (every 10 min) |
| `scematica-nn-tournament.json` | NN tournament | NN tournament | Per-variant rewards + primary |
| `scematica-deployer-reputation.json` | reputation ledger | filters | Per-deployer rug/success EMA |
| `scematica-strategy.json` | AI strategy agent | sniper (live_params) | TP/SL/multiplier/regime |
| `scematica-rate-mode.json` | dashboard | sniper (live_params) | Active rate mode + TP/SL |
| `scematica-sell-mode.json` | dashboard / drawdown guard | sniper | Pauses buys, sells positions |
| `scematica-dump-mode.json` | dashboard | sniper | Force-sell with `min_out = 0` |
| `pool-cache.json` | sniper, pool-seeder | sniper | Pool → mint lookup for sells |

**Writer convention:** always write to `<file>.tmp` then `rename` for atomic visibility (see `FilterStats::write_to_file` in `crates/scematica-sniper/src/filters.rs`).

## Architecture: Sniper Pipeline

1. **Listener layer** (`listener.rs`, `pumpfun.rs`, `whale_copy.rs`) — all sources merge into one `ListenerEvent::NewPool` stream; downstream code is source-agnostic.
2. **Filter pipeline** (`filters.rs`) — each filter implements `PoolFilter` trait. Per-RPC-call timeout is **3 s** (`RPC_CALL_TIMEOUT_SECS`); pipeline has a hard cap. Failure modes prefer "fail open" so one slow node doesn't stall the queue. New filters must register a name with `FilterStats` for dashboard visibility.
3. **Executor** (`executor.rs`) — Raydium swap building, WSOL ATA lifecycle, dynamic fee escalation.
4. **Sniper main loop** (`sniper.rs`) — orchestrates buy → sell-monitor → exit.
5. **Sell monitor** is two-phase: first 20 checks at 100ms (catches rapid post-buy dumps via 3-consecutive-decline detector), then `price_check_interval_ms`. Re-reads `live_params` each iteration for hot-reload of TP/SL.

Configuration uses `#[serde(default)]` on `SniperConfig` and `FilterConfig` — new config fields must have `Default` impls so existing `config.toml` files keep loading.

## Architecture: NN Agent (scematica-nn)

Pure-Rust Double DQN, no ML framework dependency. Lives inside the sniper process.

- **Net:** MLP `STATE_DIM(18) → 128 → 64 → ACTION_DIM(5)`, He init, ReLU, linear output
- **State** (18 features, normalised to [0,1]): defined in `state.rs` — pool age, liquidity, price change, volume, buy/sell ratio, LP burned, mint renounced, PnL %, position age, daily PnL, streaks, balance, regime, volatility, spread, time-of-day, open positions
- **Actions** (`action.rs`): `Hold`, `Buy`, `BuyAgg`, `SellPartial`, `SellAll`
- **Training**: epsilon-greedy (1.0 → 0.05, decay 0.9995), target net hard-copy every 200 steps, replay buffer 10k, batch 64
- **Active buy-gating** (`sniper.rs`, `advise()` block): once `ready_to_advise()` is true the agent sizes entries (`BuyAggressive`→1.5x, `Hold`→0.5x) and can veto a buy on `SellPartial`/`SellAll`. `ready_to_advise = train_steps >= 10_000 && last_q_values has signal`. The veto only *fully suppresses* a buy when the bearish Q exceeds the best buy Q by ≥15% (`NN_VETO_REL_MARGIN`); a weaker lean downgrades to 0.5x sizing so a partially-converged net can't silently kill the PF≈6.5 edge.
- **Regime branching**: separate `(online, target)` net pairs per regime engaged when `epsilon < 0.3` and a known regime (`bull`/`bear`/`sideways`/`panic`) is set.
- **Tournament**: 3 variants (conservative/balanced/aggressive) run in parallel; highest `total_reward` is promoted every 1000 steps.

## Risk Subsystems

Independent breakers in `crates/scematica-sniper/src/`. Each can be toggled in `config.toml`; a single trip pauses buys. When adding a new breaker, follow this pattern: dedicated module exposing `should_halt(&state) -> Option<reason>`, hooked into the buy gate in `sniper.rs`.

- `ath_tracker.rs` — pauses buys when wallet drops `ath_drawdown_pct` below session ATH
- `grief_breaker.rs` — 5-min sliding-window cumulative loss
- `kelly.rs` — fractional Kelly position sizing from rolling win-rate (lookback in config)
- `pool_scorer.rs` — 0-100 predictive score from pool age + quote vault; rejects below `min_pool_score`
- `reputation.rs` — EMA-blended deployer rug history; rejects deployers > `max_deployer_rugs_24h`
- `multi_rpc.rs` — latency-ranked round-robin failover across `extra_rpc_endpoints`

## Platform Notes

- Primary dev environment is **Windows + PowerShell**. Code paths handle Windows specifics: `tasklist` for process liveness (sniper `main.rs`), `NotifyIcon` for desktop toasts (avoid WinRT — use `System.Windows.Forms.NotifyIcon` with stderr nulled to keep log panel clean).
- WSL UNC keypath paths are supported in `[wallet] keypair_path` (`\\wsl$\Ubuntu\home\...`).
- The sniper writes a PID lockfile (`scematica-sniper.lock`) and refuses to start if a live process is already running — two snipers on the same Helius WebSocket rate-limit each other into uselessness.
- Release profile is heavy: `lto = "fat"`, `codegen-units = 1`, `panic = "abort"`, `overflow-checks = true`. First release build is slow; `target/` reaches 5-10 GB — run `cargo clean` periodically.

## Token Gate

Sniper and dashboard both enforce a 250k SCEMA balance check at startup with up to 5 retries. Set `SCEMATICA_SKIP_GATE=1` only during RPC outages — it bypasses the check entirely. SCEMA is a Token-2022 mint; gate code must use Token-2022 helpers, not legacy SPL Token.
