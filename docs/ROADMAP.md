# Scematica Roadmap

**Workspace v1.25.0 · verified against source 2026-08-11**

---

## How to read this document

The first version of this roadmap was a plan. This one is a plan **and a record**, because
three of its five original quarters are now in the past and a roadmap that never says what
happened is marketing rather than engineering.

Every past-quarter item carries a verdict against the actual codebase:

| Verdict | Meaning |
|---|---|
| **DELIVERED** | Shipped and reachable in the tree today, with the file or crate named |
| **PARTIAL** | Real work exists, but the milestone as written is not met — the gap is stated |
| **NOT STARTED** | No implementation. Either re-scheduled below or dropped, and which is said |
| **DROPPED** | Deliberately abandoned. The reason is given, usually because measurement killed it |

Nothing here is marked DELIVERED on the strength of a design document. Where the honest
answer is "we measured it and stopped", that is recorded too — the BOT Chain sniper is the
clearest case, and it is a result, not a failure.

---

# Vision

Scematica is built as autonomous AI trading infrastructure: open-source intelligence,
predictive market systems, and agentic integrations that other software can actually use.

The mission is not another hype-based trading bot. It is a fully reactive, prediction-driven
autonomous trading architecture that adapts to real-time market conditions — and, just as
importantly, one that **knows when it does not know**. The epistemic breakers and the Ψ gate
added in v1.25.0 are the clearest expression of that: a system trading on stale or
unverified data is more dangerous than one that stops.

---

# Q1 2026 — Foundation & Core Infrastructure
### January 2026 → March 2026 · **COMPLETE**

## Autonomous Trading Core

- **Foundational reactive trading engine** — DELIVERED. `crates/scematica-sniper/src/sniper.rs`,
  the buy → sell-monitor → exit loop.
- **Modular signal ingestion architecture** — DELIVERED. `listener.rs`, `pumpfun.rs` and
  `whale_copy.rs` merge into one `ListenerEvent::NewPool` stream; everything downstream is
  source-agnostic.
- **Multi-source market data pipelines** — DELIVERED. Raydium AMM V4, Pump.fun and whale-copy
  sources, with `multi_rpc.rs` providing latency-ranked failover across endpoints.
- **Predictive strategy execution framework** — DELIVERED. The `PoolFilter` pipeline in
  `filters.rs` plus `pool_scorer.rs`.
- **Low-latency execution optimisation** — DELIVERED. `executor.rs`: Raydium swap building,
  WSOL ATA lifecycle, dynamic priority-fee escalation.

## AI Infrastructure

- **Claude-enabled reasoning layer** — DELIVERED. `crates/scematica-ai` speaks the Anthropic
  Messages API directly (`client.rs`); `ANTHROPIC_API_KEY` enables Claude pool evaluation,
  with Groq, OpenRouter and Cerebras as fallback providers.
- **Initial reinforcement-learning experimentation** — DELIVERED, and well past
  experimentation: `crates/scematica-nn` is a pure-Rust Dueling Double-DQN with prioritized
  replay and n-step returns. See [DQ_STAR_AGENT.md](DQ_STAR_AGENT.md).
- **Memory and context-aware trading systems** — DELIVERED. Persistent mint cooldown,
  `reputation.rs` deployer EMA ledger, and the replay buffer as trading memory.
- **Dynamic market pattern recognition** — DELIVERED. `pool_scorer.rs` and the Fibonacci
  scoring protocol ([FIBONACCI_PROTOCOL_WHITEPAPER.md](FIBONACCI_PROTOCOL_WHITEPAPER.md)).
- **Autonomous risk-evaluation framework** — DELIVERED. `ath_tracker.rs`, `grief_breaker.rs`,
  `kelly.rs`, `reputation.rs` — independent breakers, any one of which pauses buys.

## Agentic Integrations

- **x402 integration framework** — DELIVERED. `crates/scematica-protocol`, a Rust-native
  HTTP 402 facilitator.
- **Dexter interoperability layer** — DELIVERED. x402 payments interoperate with the Dexter
  x402 SDK; see [scemadex.md](scemadex.md).
- **Eliza agent communication layer** — NOT STARTED, and **DROPPED** as a named integration.
  The generic path won: `crates/scemadex-mcp` bridges *any* MCP-speaking agent to the rail,
  which covers the Eliza use case without a framework-specific adapter to maintain.
- **Modular API abstraction system** — DELIVERED. `crates/scematica-api` plus the
  relay's HTTP surface.
- **Agent orchestration testing environment** — DELIVERED. `agent-playground/`
  (published as `scema-agent-playground`).

## Infrastructure

- **Scalable cloud-native architecture** — PARTIAL. `deploy/` has `docker-compose.yml`,
  `relay.Dockerfile`, `fly.toml` and a RUNBOOK — but these cover the **relay** only. The
  sniper and dashboard remain operator-run local processes by design (they hold a keypair),
  so "cloud-native" applies to the rail, not the bot.
- **Secure wallet management framework** — DELIVERED. `scematica-core` wallet handling, WSL
  UNC keypath support, and the PID lockfile single-instance guard.
- **Internal analytics dashboard** — DELIVERED. The six-tab ratatui TUI,
  `crates/scematica-dashboard`.
- **CI/CD pipelines** — DELIVERED. `.github/workflows/ci.yml`, with `scemadex-sdk` gated on
  fmt + clippy `-D warnings` + tests and doctests.
- **Containerized deployment systems** — PARTIAL. Relay containerised; no bot image, for the
  keypair reason above.

## Ecosystem & Community

- **Official website launch** — DELIVERED. `web/` (Next.js), now hosting three products.
- **Branding and design rollout** — DELIVERED. Three distinct palettes: sniper black-and-red,
  alchem-link black-and-blue, Scylar violet.
- **Technical whitepaper release** — DELIVERED. [WHITEPAPER.md](WHITEPAPER.md).
- **GitHub open-source repository initialization** — DELIVERED.
- **Developer documentation foundation** — DELIVERED. [docs/README.md](README.md) indexes the set.

---

# Q2 2026 — Intelligence Layer Expansion
### April 2026 → June 2026 · **COMPLETE**

## Predictive Systems

- **Real-time sentiment analysis engine** — PARTIAL. Social-link enrichment and the Pump.fun
  trending monitor feed pool selection, but there is no standalone sentiment engine and none
  is planned — the measured edge came from pool mechanics, not sentiment.
- **Cross-market behavioural correlation** — DELIVERED. `cross_pool_correlation` in the filter
  pipeline.
- **Predictive volatility modelling** — DELIVERED. Volatility is a DQ\* state feature, and
  velocity / acceleration terms drive the exit ladder.
- **Adaptive trade weighting** — DELIVERED. `kelly.rs` fractional Kelly sizing from rolling
  win rate.
- **Autonomous portfolio balancing** — PARTIAL. Multi-position management and per-position
  exits exist; there is no cross-position rebalancer.

## AI & Agentic Development

- **Multi-agent coordination framework** — DELIVERED. `crates/scematica-ai`: Chat, Strategy,
  Risk, Debate and Report agents.
- **Autonomous trade debate systems** — DELIVERED. The Debate agent.
- **Dynamic strategy mutation engine** — DELIVERED. The DQ\* tournament: three variants
  (conservative / balanced / aggressive) run in parallel, highest `total_reward` promoted
  every 1000 steps.
- **Self-adjusting risk management** — DELIVERED. Live-param hot-reload via
  `scematica-strategy.json`, re-read each sell-monitor iteration.
- **Reinforcement-learning optimisation layer** — DELIVERED. Double DQN with prioritized
  replay, plus opt-in QR-DQN distributional returns and a Dreamer-style latent world model.

## Trading Features

- **Multi-wallet trading support** — NOT STARTED. No implementation in the tree.
  **Re-scheduled to Q4 2026** below.
- **DEX aggregation architecture** — DELIVERED. `crates/scematica-executor` with Jupiter
  integration; `crates/scematica-arb` searches Raydium / Orca / Meteora.
- **Cross-chain trade execution research** — DELIVERED **as research, with a negative
  result** — which is what "research" is supposed to be able to return. See
  `scema-botchain/`; the measurement is in Q3 below.
- **Advanced liquidity routing** — DELIVERED. Arb graph search plus ScemaDEX
  Conviction-Routing bonds.
- **Position management automation** — DELIVERED. Two-phase sell monitor, momentum
  escalation ladder, adaptive pullback.

## Security & Reliability

- **Trading safeguard protocols** — DELIVERED. The full breaker set.
- **Sandboxed execution testing** — DELIVERED. The backtester, the adversarial pool simulator
  gym (`adversarial_sim.rs`), and SIM mode in the SDK dashboard.
- **Failover infrastructure** — DELIVERED. `multi_rpc.rs`; AI provider failover chain.
- **Monitoring and telemetry** — DELIVERED. The file-based IPC surface plus
  `scematica-tx-telemetry.jsonl`.
- **Security audit preparation** — PARTIAL. Internal review and a security-focused test suite
  (`test_agent_workspace.py`); **no external audit has been commissioned**. Q1 2027 below.

## Ecosystem Goals

- **Closed alpha onboarding** — DELIVERED. Token-gated at 250k SCEMA.
- **Developer contributor program** — NOT STARTED. Re-scheduled to Q1 2027.
- **Community governance discussions** — NOT STARTED. Q2 2027.
- **Strategic integration partnerships** — PARTIAL. Dexter x402 SDK interop; MCP Registry and
  Smithery listings for `scemadex-mcp`.
- **Technical research publications** — DELIVERED. The thesis article series,
  [EQUATIONS.md](../EQUATIONS.md), [EQUATIONS-ANALYSIS.md](../EQUATIONS-ANALYSIS.md), and the
  Fibonacci Protocol whitepaper.

---

# Q3 2026 — Alpha Ecosystem Launch
### July 2026 → September 2026 · **IN PROGRESS** (v1.25.0 shipped 2026-08-11)

## Live Trading Systems

- **Autonomous live trading deployment** — DELIVERED. Running against Solana mainnet;
  measured profit factor **6.50** / **+1.95 SOL** on the validated-edge session.
- **Real-time adaptive market response** — DELIVERED. Rate modes, regime branching in the
  DQ\* agent, weekend auto-switch.
- **AI-driven trade execution refinement** — DELIVERED. The DQ\* buy gate sizes entries
  (`BuyAggressive` → 1.5×, `Hold` → 0.5×) and can veto once `train_steps ≥ 10_000`.
- **High-frequency signal interpretation** — DELIVERED. 100ms Phase-1 sell monitoring with a
  three-consecutive-decline detector.
- **Performance optimisation framework** — DELIVERED. Fat LTO, single codegen unit, and the
  measured latency path in [WHITEPAPER.md](WHITEPAPER.md) §19.

## Intelligence Expansion

- **Neural predictive forecasting** — DELIVERED. QR-DQN distributional returns and the latent
  world model.
- **Dynamic macro-event reaction** — PARTIAL. Regime branching reacts to market state; there
  is no external macro-event feed.
- **AI-generated strategy evolution** — DELIVERED. Tournament promotion plus the Strategy
  agent writing `scematica-strategy.json`.
- **Advanced anomaly detection** — DELIVERED, and beyond the original scope. Two systems the
  2026 plan did not anticipate:
  - **`coherence.rs`** — an *epistemic* breaker. Every other breaker fires on money and
    therefore after the damage; this one halts buys when the pipeline is passing pools it
    could not verify, because RPC-bound filters fail open and a degraded node silently turns
    the pipeline into a pass-through.
  - **Policy-collapse detection** — the intelligence ratio `I = Var[Q*]/E[Q*]²` distinguishes
    conviction from collapse, which a margin test cannot.
- **Multi-layer market reasoning** — DELIVERED. `crates/scematica-sentience`, the Singularity
  Cognitive Architecture as 29 computable modules with Ψ/Ω master equations and ethics gating.

## Integrations

- **Expanded DEX integrations** — DELIVERED. Raydium, Orca, Meteora, Jupiter.
- **Solana ecosystem interoperability** — DELIVERED. Token-2022, Pump.fun, x402.
- **Agent-to-agent communication standards** — DELIVERED. `scemadex-relay` peer mesh plus
  `scemadex-mcp` (MCP).
- **External strategy module support** — PARTIAL. The `PoolFilter` trait and the SDK's
  trait-based core admit external modules; there is no dynamic plugin loader.
- **Open plugin architecture research** — PARTIAL. Superseded in practice by MCP, which
  solved the problem from the other direction.

## Platform Infrastructure

- **User dashboard alpha release** — DELIVERED. `web/` Next.js dashboard, standalone: it
  proxies a reachable `scematica-api` and otherwise falls back to a self-contained
  simulation that is *always* labelled — simulated responses carry `simulated: true`, an
  `X-Scematica-Source: simulation` header and a permanent banner, and control POSTs return
  503 rather than faking success.
- **Live analytics visualization suite** — DELIVERED. Web panels plus the TUI.
- **Strategy monitoring systems** — DELIVERED. Filter stats, decision log, NN stats.
- **Automated reporting tools** — DELIVERED. The Report agent; CSV / NDJSON / Markdown /
  Prometheus exporters in alchem-link.
- **Scalable backend upgrades** — DELIVERED for the rail. `scemadex-relay` with a deploy path.

## Delivered in Q3 but never on the roadmap

The quarter's largest items were not in the original plan at all, and the record is more
useful than the plan here:

- **Scylar Terminal** (`/scylar-terminal`) — an avatar chat terminal over live bot state,
  gated by Ψ. HOLD returns 409 and the model is not called, because a warned model still
  writes a confident paragraph of stale numbers.
- **Counterfactual replay + calibration** — `POST /api/replay` re-applies thresholds to what
  the pipeline actually measured; `GET /api/calibration` scores past claims against realised
  PnL. Both are explicit about what they cannot know: tightening yields an exact delta,
  loosening admits pools with no outcome and gets no number.
- **alchem-link 0.23.x** — a second product: a stdlib-only Alchemy × Chainlink toolkit with
  its own in-package terminal system and a coding agent behind two independent gates.
  590 tests, all offline.
- **BOT Chain port** (`scema-botchain`) and the **neural mesh** (`scema-bot-mesh`).

## The BOT Chain result — measured, then paused

The Q2 "cross-chain trade execution research" milestone returned a **negative result**, and
acting on it is the point:

| Window | V3-style factory | CA factory |
|---|---|---|
| 20,000 blocks (~3.7 h) | 0 | 0 |
| 200,000 blocks (~1.5 d) | 0 | 0 |
| 1,000,000 blocks (~7.7 d) | **2** | 0 |

Two pool creations in roughly eight days, 0.29% network utilisation, and 2 swaps in a
50-transaction sample. **A sniper is not scheduled for BOT Chain** — there is nothing to
snipe. The contracts (`BotchainPriceFeed`, `ScemaArbExecutor`, `ScemaBondEscrow`,
`BotchainNNMesh`) are deployed and tested on 677 so that the port is ready if flow arrives,
and `botchain-probe` re-runs the measurement on demand. The Solana bot remains authoritative.

---

# Q4 2026 — Autonomous Expansion Phase
### October 2026 → December 2026 · **PLANNED**

Re-scoped against what actually exists. Items carried forward from earlier quarters are
marked.

## Trading Infrastructure

- **Multi-wallet support** *(carried from Q2)* — pool funds across wallets to exceed
  per-wallet position limits. Still the largest unbuilt trading feature.
- **`scematica-swap` mainnet deploy** — the Anchor program has a devnet path
  (`programs/scematica-swap/DEPLOY_DEVNET.md`) and the arb engine currently runs
  **program-less** (atomic revert + final-hop `min_out`). Mainnet deploy is an optimisation,
  not a prerequisite — the honest framing that earlier roadmaps got wrong.
- **Automated treasury balancing** — cross-position rebalancing, closing the Q2 PARTIAL.

## AI Advancement

- **Wire the Ψ gate into the live trading path.** `scematica-sentience` currently gates the
  API and the coherence breaker; nothing in the sniper's LLM calls depends on it yet. This is
  a known wiring gap, stated as one rather than implied to be done.
- **Long-context market memory** — extend the replay buffer with a durable episodic store.
- **Self-learning execution optimisation** — let the agent tune fee escalation, not just entry
  size.

## Developer Ecosystem

- **Strategy marketplace architecture** *(carried from Q4 original)* — the ScemaDEX
  `PeerMarket` and bonded-teaching primitives are the substrate; the marketplace itself is
  unbuilt.
- **Publish DQ\* checkpoints for community evaluation** via x402-gated tournament scoring.

## Reliability

- **Bot containerisation** — resolve the keypair-custody question that blocked it, or
  document why it stays local permanently.
- **Extended stress testing** under RPC brownout, now that `coherence.rs` gives a measurable
  signal to test against.

---

# Q1 2027 — Public Platform Evolution
### January 2027 → March 2027 · **PLANNED**

- **External security audit** *(carried from Q2 2026)* — commissioned, not merely prepared
  for. Scope: the executor, the wallet path, the x402 facilitator and the relay.
- **Public platform release candidate** — the web dashboard as a first-class product rather
  than an operator tool.
- **Developer contributor program** *(carried from Q2 2026)*.
- **Full strategy deployment suite** — user-defined filter chains without a recompile.
- **Distributed agent coordination** — multi-node relay mesh with the attestation path from
  `scema-bot-mesh` applied to live inference.
- **Advanced real-time simulation environments** — grow the adversarial gym into a
  full shadow-trading environment.

---

# Q2 2027 — Ecosystem & Governance
### April 2027 → June 2027 · **PLANNED**

- **Community governance systems** *(carried from Q4 2026 original)*.
- **On-chain SCEMA staking** — stake for a share of x402 protocol fee revenue. Requires the
  audit above to land first; sequencing this before an audit was a mistake in the original
  plan.
- **Ecosystem incentive structures** and grant initiatives.
- **Protocol-level integrations** beyond MCP and x402.
- **Long-term sustainability framework.**

---

# Long-Term Direction

Scematica is being built toward:

- A fully autonomous AI trading infrastructure layer
- An open-source ecosystem for intelligent market agents
- A scalable execution framework for predictive finance
- A decentralised coordination layer for autonomous financial intelligence
- A next-generation interface between AI systems and digital markets

With one commitment that the last three quarters made concrete, and that outranks the rest:
**the system reports what it measured, and says so when it did not measure anything.** The
fail-open breaker, the Ψ gate, the replay endpoint's refusal to price un-taken pools, the
simulation banner, and the BOT Chain sniper that was cancelled by its own data are all the
same decision. A number nobody can check is worse than no number.

---

*Scematica is experimental software. Trading cryptocurrencies involves substantial risk of
loss. Roadmap items beyond the current quarter are intentions, not commitments, and past
performance of algorithmic strategies does not guarantee future results.*
