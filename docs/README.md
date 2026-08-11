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

## Alchem-Link (Python developer toolkit)
- [alchem-link/README.md](../alchem-link/README.md) — install, CLI, and TUI dashboard overview.
- [alchem-link/docs/GUIDE.md](../alchem-link/docs/GUIDE.md) — full developer guide: all modules, recipes, integration patterns, and the Python API.
- [alchem-link/docs/TUI.md](../alchem-link/docs/TUI.md) — TUI dashboard reference: navigation, panels, keybindings, and building the standalone exe.
- [alchem-link/docs/RECIPES.md](../alchem-link/docs/RECIPES.md) — all four developer recipes with full step-by-step breakdowns and extension guidance.

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

## Cognition, epistemics & the newer subsystems
- [`../crates/scematica-sentience/README.md`](../crates/scematica-sentience/README.md) —
  the Singularity Cognitive Architecture as 29 computable modules: Ψ/Ω master equations,
  ethics gating, knowledge graph, meta-cognition, and the LLM overlay. Library only.
- **The Ψ gate** — `GET /api/sentience`. What it measures is *staleness and contradiction*,
  not mood: every read endpoint serves its state file identically whether it was written
  4 seconds or 4 hours ago, so a live-looking briefing can describe a session that ended
  overnight. HOLD returns 409 and the model is not called. [WHITEPAPER.md §22](WHITEPAPER.md).
- **The coherence breaker** — `crates/scematica-sniper/src/coherence.rs`. The one breaker
  that fires *before* the loss: RPC-bound filters fail open, so a degraded node silently
  turns the pipeline into a pass-through that still reports "passed".
  [WHITEPAPER.md §12.7](WHITEPAPER.md).
- **Replay & calibration** — `POST /api/replay`, `GET /api/calibration`. Tightening a
  threshold gives an exact PnL delta; loosening admits pools with no outcome and
  deliberately gets no number. [WHITEPAPER.md §23](WHITEPAPER.md).

## Cross-chain (BOT Chain)
- [`../scema-botchain/README.md`](../scema-botchain/README.md) — the EVM (chain 677) port,
  its own cargo workspace by dependency necessity, and the measurement that **cancelled the
  sniper**: 2 pool creations in ~8 days. Contracts are deployed and tested; the Solana bot
  stays authoritative.
- [`../scema-bot-mesh/README.md`](../scema-bot-mesh/README.md) — verifiable neural
  inference: Q16.16 fixed-point so a challenger's re-run produces the same bits, committed
  on chain in 32 bytes and slashable via `ScemaBondEscrow`.

## Architecture & vision
- [WHITEPAPER.md](WHITEPAPER.md) — system architecture and design. **Stamped
  `workspace v1.25.0 · verified against source 2026-08-11`.**
- [ROADMAP.md](ROADMAP.md) — Q1 2026 → Q2 2027. Both a plan and a **record**: every
  milestone from the original 2026 plan carries a DELIVERED / PARTIAL / NOT STARTED /
  DROPPED verdict against the actual codebase.

## Release articles
Plain-text, publication-ready write-ups of what shipped in each release. Newest first.
- [article-v1.13.0-web-standalone.txt](article-v1.13.0-web-standalone.txt) — the web
  dashboard stops simulating discovery: live mint feed, the pool scorer ported to
  TypeScript with an enforced parity contract, one shared poll per endpoint,
  wallet-signed swaps, and the pairing-probe fix.
- [article-alchem-link-v0.3.0.txt](article-alchem-link-v0.3.0.txt) — alchem-link goes
  from prose scaffold to a toolkit that reads live Chainlink feeds, with staleness
  detection and a verified feed registry.
- [article-v1.12.0.txt](article-v1.12.0.txt) · [article-v1.12.0-fibonacci.txt](article-v1.12.0-fibonacci.txt)
  — the v1.12.0 release and the Fibonacci recovery system.
- [article-alchem-link-v0.2.0.txt](article-alchem-link-v0.2.0.txt) — the alchem-link TUI.

## Web products (`web/`)
One standalone Next.js app hosting three products that share a codebase and nothing else —
each has its own palette and its own data rules. Architecture notes live in
[`../CLAUDE.md`](../CLAUDE.md) and [WHITEPAPER.md §24](WHITEPAPER.md).
- **`/` — the sniper dashboard.** Proxies a live `scematica-api` when `RUST_API_URL`
  resolves, otherwise falls back to a self-contained simulation that is *always* labelled:
  `simulated: true`, an `X-Scematica-Source: simulation` header, a permanent banner, and
  control POSTs that return 503 rather than faking success.
- **`/alchem-link`** — the web build of the Python oracle toolkit. **No simulation branch
  at all**: these routes read a chain or report the error, because a fabricated price would
  defeat the point of a staleness verdict.
- **`/scylar-terminal`** — an avatar chat terminal over live bot state, gated by Ψ. Provider
  keys are server-side and client-supplied `system` turns are stripped; the model picks a
  tool *name*, never a URL.

Checks: `npm run check:parity` (TS pool scorer vs the Rust fixtures) and
`npm run check:scylar` (expressions, speech, markdown, commands, session, tools, gate).

## Reports
- [PROFITABILITY_REPORT.md](PROFITABILITY_REPORT.md) — live-data profitability analysis,
  refreshed to the full 685-trade dataset (+2.26 SOL net, profit factor 6.50). Reproduce
  with `python tools/deep_analysis.py`.
