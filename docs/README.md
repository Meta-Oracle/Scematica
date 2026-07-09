# Scematica Documentation

Project root: [`../README.md`](../README.md) · Project instructions for Claude:
[`../CLAUDE.md`](../CLAUDE.md).

This folder holds the long-form docs. Some technical references carry their own
`Version | Last updated` stamp and describe the system as of that date — treat
those as point-in-time references, not always current to the latest release.

## Start here
- **[`../Ideal-Scema-Trading.txt`](../Ideal-Scema-Trading.txt)** — the data-derived
  operating playbook: how much SOL you need (0.7–1.0 to start), what entry size /
  hours / days actually make money, and how to run the bot at full potential. Every
  number from the real trade log.
- **[`../Scematica-Thesis.md`](../Scematica-Thesis.md)** — the full article-style thesis:
  Scematica from conception to now, every feature and why it exists, plus what the data says.
- **[`../The-Fibonacci-Protocol.md`](../The-Fibonacci-Protocol.md)** — article on how the
  Fibonacci scoring protocol works and how it sharpens the AI's detection of good pools
  (the golden-ratio signals, the composite score, and how it feeds the DQ* agent as both a
  gate and a feature). The formal spec is in [FIBONACCI_PROTOCOL_WHITEPAPER.md](FIBONACCI_PROTOCOL_WHITEPAPER.md).

## Getting started
- **Install from crates.io (fastest)** — `cargo install scematica-suite` then
  `scematica help`. The `scematica` launcher runs every app (`dashboard`, `sniper`,
  `arb`, `protocol`, `ddqn`, `scemadex`) with no clone or build. Full command
  table + per-crate versions in the [root README](../README.md#install-from-cratesio).
- [BEGINNER_GUIDE.md](BEGINNER_GUIDE.md) — zero-to-running on Windows: the
  crates.io easy path, then the one-click `init.bat` → `build.bat` →
  `start-*.bat` flow.
- [QUICKSTART.md](QUICKSTART.md) — crates.io install, script reference, dashboard
  navigation, rate modes, and troubleshooting.

## Mobile app (Android companion remote)
- [mobile-beginners-guide.md](mobile-beginners-guide.md) — plain-English: install the
  `.apk`, pair to your self-hosted bot, use the Beginner/Pro modes and sliders.
- [mobile-app.md](mobile-app.md) — developer build/ship runbook (Capacitor, signing,
  dApp Store, wallet deep-link).

## ScemaDEX SDK (agentic liquidity layer)
- [scemadex.md](scemadex.md) — the published `scemadex-sdk`: intent solving,
  Conviction-Routing bonds, the inference/experience mesh, x402 over the Dexter
  SDK, plus investor and developer adoption guides.
- [scemadex-bond-settlement-decision.md](scemadex-bond-settlement-decision.md) —
  ADR on the open (devnet) vs. proprietary (mainnet) settlement boundary.
- [settlement-v2-design.md](settlement-v2-design.md) — the optimistic dispute-window
  settlement state machine + insurance primitive + zkML proof backends (transparent
  spot-check and a real arkworks Groth16/BN254 SNARK). Status: implemented.
- On-chain program devnet deploy:
  [`../programs/scematica-swap/DEPLOY_DEVNET.md`](../programs/scematica-swap/DEPLOY_DEVNET.md).

## Trading strategy, math & the RL agent
- [EQUATIONS_AND_STRATEGIES.md](EQUATIONS_AND_STRATEGIES.md) — every trading
  equation, parameter evolution, and the rationale behind each.
- [DQ_STAR_AGENT.md](DQ_STAR_AGENT.md) — technical reference for the Deep Q*
  agent in `crates/scematica-nn`.
- [RATE_MODES_GUIDE.md](RATE_MODES_GUIDE.md) ·
  [RATE_MODES_QUICK_REF.md](RATE_MODES_QUICK_REF.md) — the rate/builder modes and
  their TP/SL/sizing.

## Arbitrage (cross-DEX, program-less)
- `scematica-arb` runs **program-less by default** — no on-chain deploy. Seed the pool
  graph with `cargo run --release -p pool-seeder`, then `cargo run --release --bin arb`.
  Solana's atomic revert + the final-hop min-out enforce profit-or-revert for free.

## Fibonacci protocol (entry/exit framework)
- [FIBONACCI_PROTOCOL_WHITEPAPER.md](FIBONACCI_PROTOCOL_WHITEPAPER.md) — the
  canonical mathematical spec (verified against source).
- [FIBONACCI_OPS_GUIDE.md](FIBONACCI_OPS_GUIDE.md) — operator's guide: how it's
  wired, config knobs and defaults, monitoring, tuning, and troubleshooting.
  (Consolidates the five earlier companion docs.)

## Architecture & vision
- [WHITEPAPER.md](WHITEPAPER.md) — system architecture and design.
- [ROADMAP.md](ROADMAP.md) — Q1 2026 → Q1 2027 plan.

## Reports
- [PROFITABILITY_REPORT.md](PROFITABILITY_REPORT.md) — live-data profitability analysis,
  refreshed to the full 685-trade dataset (+2.26 SOL net, profit factor 6.50). Reproduce
  with `python tools/deep_analysis.py`.
