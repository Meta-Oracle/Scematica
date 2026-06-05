# Scematica Documentation

Project root: [`../README.md`](../README.md) · Project instructions for Claude:
[`../CLAUDE.md`](../CLAUDE.md).

This folder holds the long-form docs. Some technical references carry their own
`Version | Last updated` stamp and describe the system as of that date — treat
those as point-in-time references, not always current to the latest release.

## Getting started
- [BEGINNER_GUIDE.md](BEGINNER_GUIDE.md) — zero-to-running on Windows, with the
  one-click `init.bat` → `build.bat` → `start-*.bat` flow.
- [QUICKSTART.md](QUICKSTART.md) — script reference, dashboard navigation, rate
  modes, and troubleshooting.

## ScemaDEX SDK (agentic liquidity layer)
- [scemadex.md](scemadex.md) — the published `scemadex-sdk`: intent solving,
  Conviction-Routing bonds, the inference/experience mesh, x402 over the Dexter
  SDK, plus investor and developer adoption guides.
- [scemadex-bond-settlement-decision.md](scemadex-bond-settlement-decision.md) —
  ADR on the open (devnet) vs. proprietary (mainnet) settlement boundary.
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

## Fibonacci protocol (entry/exit framework)
- [FIBONACCI_PROTOCOL_WHITEPAPER.md](FIBONACCI_PROTOCOL_WHITEPAPER.md) — the
  canonical mathematical spec (verified against source).
- [FIBONACCI_OPS_GUIDE.md](FIBONACCI_OPS_GUIDE.md) — operator's guide: how it's
  wired, config knobs and defaults, monitoring, tuning, and troubleshooting.
  (Consolidates the five earlier companion docs.)

## Architecture & vision
- [WHITEPAPER.md](WHITEPAPER.md) — system architecture and design.
- [ROADMAP.md](ROADMAP.md) — Q1 2026 → Q1 2027 plan.

## Reports (point-in-time)
- [PROFITABILITY_REPORT.md](PROFITABILITY_REPORT.md) — a dated live-data
  profitability analysis (snapshot, not maintained).
