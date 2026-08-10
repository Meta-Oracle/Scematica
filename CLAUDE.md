# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Overview

Scematica is a Rust Solana sniper + cross-DEX arbitrage bot with a ratatui TUI dashboard, a Deep Q* reinforcement-learning agent, and a Rust-native x402 payment protocol server. Targets Raydium AMM V4 new-pool events on Solana mainnet. Gated behind a 250k $SCEMA token balance (mint `HcsHqEJ9suf4oHJ8mb52M7AVKjhYhnTaeHgTmde7pump`).

## Build, Run, Test

```powershell
# Build all binaries (release, ~5-10 min cold)
cargo build --release

# Published crates (crates.io) — install + run without a checkout. The umbrella
# `scematica-suite` installs a `scematica` launcher that dispatches to the
# component binaries (dashboard/sniper/backtest/protocol/ddqn/scemadex).
cargo install scematica-suite              # → `scematica` launcher (+ umbrella lib)
scematica dashboard --demo                 # run any component via the launcher
cargo install scematica-nn && scema-ddqn   # DQ* live training viewer
cargo install scemadex-sdk && scemadex     # ScemaDEX live viewer

# Run primary binaries
cargo run --release --bin dashboard          # TUI entry point
cargo run --release --bin dashboard -- --demo  # No keypair/RPC required
cargo run --release --bin sniper             # Sniper standalone
cargo run --release -p pool-seeder           # Seed pools/ (arb graph) — REQUIRED before arb
cargo run --release --bin arb                # Arb standalone (program-less by default: no on-chain deploy; atomic min-out profit-or-revert. Set SWAP_PROGRAM_ID to use a deployed program, or ARB_PROGRAM_LESS=1/0 to force)
cargo run --release --bin scematica-protocol -- --pay-to <wallet> --price-lamports 10000

# ScemaDEX agentic-liquidity layer (separate from the bot)
cargo run --release --bin sdk-dashboard           # SDK TUI over the bond pipeline (SIM)
cargo run --release --bin sdk-dashboard -- --live # real Jupiter quotes through the bonds
cargo run --release --bin scemadex-relay          # peer-mesh + signal-oracle HTTP server

# Backtester (replays JSONL pool history through filter pipeline + TP/SL sim)
cargo run --release --bin backtest -- --pools historical-pools.jsonl --tp 100 --sl 15

# Tests (no integration test dirs; tests are inline `#[test]` / `#[tokio::test]` in src files)
cargo test --workspace
cargo test -p scematica-sniper kelly         # Filter to a single module
cargo test -p scematica-nn agent::tests::    # Run one module's tests in a crate

# Web dashboard (web/ — standalone Next.js, see below)
cd web ; npm run dev                         # dev server on :3000
cd web ; npx tsc --noEmit                    # typecheck
cd web ; npm run check:parity                # TS pool scorer vs pool_scorer.rs fixtures

# alchem-link (Python, outside the cargo workspace)
cd alchem-link ; $env:PYTHONPATH="src" ; python -m unittest discover -s tests
cd alchem-link ; $env:PYTHONPATH="src" ; python -m alchem_link.cli doctor
cd alchem-link ; $env:PYTHONPATH="src" ; python -m alchem_link.cli verify -n base   # registry vs chain
cd alchem-link ; $env:PYTHONPATH="src" ; python -m alchem_link.cli audit -n arbitrum # consumer-safety lint
cd alchem-link ; $env:PYTHONPATH="src" ; python -m alchem_link.cli simulate           # replay guards vs failure modes
cd alchem-link ; $env:PYTHONPATH="src" ; python -m alchem_link.dashboard              # full-screen console (no deps)
cd alchem-link ; pyinstaller alchem-link.spec ; pyinstaller alchem-link-ui.spec       # standalone binaries

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
  scematica-nn/         Pure-Rust Deep Q* (Double DQN) agent — no external ML deps.
                        Opt-in: QR-DQN distributional returns (distributional.rs),
                        Dreamer-style latent world model (world_model.rs), and an
                        adversarial pool simulator gym (adversarial_sim.rs).
  scematica-dashboard/  Ratatui TUI (6 tabs). Bin: `dashboard`
  scematica-api/        HTTP API backing the web/ Next.js dashboard. Bin: `api`
  scematica-protocol/   x402 HTTP 402 payment server (facilitator). Bin: `scematica-protocol`
  scemadex-sdk/         Published agentic-liquidity SDK: intents, Conviction-Routing
                        bonds, inference/experience mesh. No solana-sdk by default.
  scemadex-settle/      Open devnet reference settler — moves devnet USDC on bond slash
  scemadex-integrations/  Bot-side ScemaDEX wiring: x402 bond engine, Jupiter route
                          policy, file signal source. publish = false
  scemadex-relay/       Peer-mesh + signal-oracle HTTP server. Bin: `scemadex-relay`
  scemadex-mcp/         MCP (Model Context Protocol) server bridging LLM agents to
                        the ScemaDEX rail over the relay. Bin: `scemadex-mcp`
  sdk-dashboard/        ScemaDEX SDK TUI over the bond pipeline. Bin: `sdk-dashboard`
  scematica-sentience/  Singularity Cognitive Architecture as a computable library:
                        Ψ/Ω master equations, ethics gating, knowledge graph,
                        meta-cognition, and an LLM `overlay` that gates a model's
                        output on integrated cognition (GO/CAUTION/HOLD). Library
                        only — no binary, so the `scematica` launcher has no
                        `sentience` subcommand. Re-exported by `scematica-suite`
                        as `sentience`; nothing in the *runtime* path (sniper,
                        dashboard, ai) depends on it yet, so gating live LLM calls
                        on Ψ remains a separate wiring step.
  scematica-suite/      Umbrella meta-crate: re-exports all components + `scematica`
                        launcher dispatching to the component binaries. Bin: `scematica`
  agent-playground/     ScemaDEX agent playground / experimentation
                        (published as `scema-agent-playground`). Bin: `playground`
alchem-link/            Python (not in the cargo workspace). Alchemy x Chainlink
                        developer toolkit: oracle consumer-safety auditing, guard
                        simulation, measured feed cadence, cross-chain divergence,
                        Multicall3-batched reads, event-log history, TWAP/volatility
                        analytics, EIP-1559 gas priced in USD, CCIP lane verification,
                        and consumer codegen. 66 verified feeds across 11 networks.
                        **Stdlib-only, no optional extras** — including a bundled
                        Keccak-256 (`keccak.py`), because hashlib ships SHA3-256 and the
                        padding differs, so function selectors are computed rather than
                        stored; and including `term/`, the in-package terminal system
                        (see below). Bins: `alchem-link`, `alchem-link-ui`.
                        Tests: `python -m unittest discover -s tests` (590, all offline)
tools/
  key-converter/        Keypair format conversion
  pool-seeder/          Seeds the arb pool graph (pools/) from the Raydium/Orca/Meteora APIs. REQUIRED before running `arb` (empty pools/ = empty graph = no trades). Raydium: list endpoint for ids/mints + key/ids endpoint for vaults.
programs/
  scematica-swap/       Anchor on-chain program (NOT in cargo workspace).
                        Devnet deploy: programs/scematica-swap/DEPLOY_DEVNET.md
```

The ScemaDEX SDK family (`scemadex-*`, `sdk-dashboard`) is the agentic-liquidity
layer; see `docs/scemadex.md`. The web dashboard lives in `web/` (Next.js).

**`web/` is standalone.** `app/api/[...slug]/route.ts` proxies a reachable
`scematica-api` when `RUST_API_URL` resolves, and otherwise falls back to a
self-contained simulation in `web/lib/sim/` — including a real Dueling Double-DQN
(`lib/sim/dqstar.ts`) mirroring `scematica-nn`. Simulated responses are tagged
`simulated: true` + `X-Scematica-Source: simulation` and surface a permanent
SIMULATION banner; control POSTs 503 instead of faking success. Never let
simulated PnL render as live results. See `docs/mobile-app.md`.

**Web data sourcing.** Three rules that are easy to break:

1. **One timer per endpoint.** Panels subscribe through `lib/store.ts` / `lib/queries.ts`;
   they must not call `setInterval` themselves. Polling is refcounted, so a hidden panel
   stops fetching — adding a private timer silently undoes that.
2. **Discovery prefers a live bot, falls back to the real feed, never invents data.**
   `lib/useDiscovery.ts` picks the source; there is no third "make something up" branch.
   Panels sourced from the public feed show a `FEED` badge.
3. **The TS pool scorer is a port, not a second brain.** `lib/feed/scorer.ts` copies the
   ladders from `pool_scorer.rs`; Rust stays authoritative. Every filter declares
   `parity: 'port' | 'approx'` — never promote an `approx` without the Rust input
   actually existing. `npm run check:parity` pins the Rust unit-test cases; run it after
   touching either side.

**`/alchem-link` is a second product on the same site**, not a sniper panel: the web
build of the `alchem-link` toolkit (`components/alchem/`, `lib/alchem/`,
`app/api/alchem/`). It reads live Chainlink aggregators and has its own black-and-blue
palette against the sniper's black-and-red — `alchem-*` tokens in `tailwind.config.ts`
and `.alchem-root` in `globals.css` mirror `alchem-link/src/alchem_link/theme.py`, so a
palette change moves both. Three constraints:

- `lib/alchem/` is a **port of the Python package**; Python stays authoritative. When
  `feeds.py` or `networks.py` changes, change the TS table too — `/api/alchem/verify`
  is what catches the two drifting, because it asks the chain rather than either table.
  Heartbeats are **measured per feed per chain** (Polygon ~60s, Base/OP 1200s, mainnet
  3600s), never a shared 3600 default, and `heartbeatMeasured: false` marks a
  conservative bound rather than a measurement. Staleness applies
  `STALENESS_TOLERANCE` (15%) on top, because real publish ceilings run a percent or two
  over the configured interval and a feed that flickers STALE every cycle trains people
  to ignore the flag.
- `lib/alchem/endpoint.ts` is **server-only** (it reads `ALCHEMY_API_KEY` and throws if
  imported in a browser). Client components import `networks.ts`, which is a pure table.
- **No simulation branch.** Unlike the sniper endpoints, these routes read a chain or
  report the error. A fabricated price would defeat the entire point of a staleness
  verdict, and unreadable feeds render as failure rows rather than being dropped.

**`/scylar-terminal` is the third product on the same site** — an avatar chat terminal
(`components/scylar/`, `lib/scylar/`, `app/api/scylar/`) with its own violet palette
(`scylar-*` tokens + `.scylar-root`). It runs on whichever free LLM tier has a key, Groq
first for latency. Four constraints:

- **Provider keys are server-side, always.** `lib/scylar/provider.ts` and
  `portrait.ts` throw if imported in a browser (runtime guard, matching
  `lib/alchem/endpoint.ts` — the `server-only` package is not a dependency). The chat
  route also **strips client-supplied `system` turns**: without that, a public endpoint
  with a key behind it is someone else's free LLM proxy.
- **No fabrication, same as everywhere else.** No provider → 503. No image backend →
  501. The portrait route never hands back a sprite dressed as a generation.
- **The avatar is three flat sprites and a state machine.** `lib/scylar/expressions.ts`
  is pure — `spriteFor` picks the frame, `presenceFor` the pose. Two animation speeds
  that must not be merged: the flap is fast and cyclic, presence is slow and one-shot,
  and `FLAP_CROSSFADE_MS` must stay under `FLAP_PERIOD_MS` or both sprites sit
  permanently half-lit. Breathing is CSS on the outer element because a CSS animation
  and an inline transform on the same property fight, and the animation wins.
- **Live bot state is opt-in and labelled.** `lib/scylar/context.ts` calls the site's own
  `/api/*` (so live-vs-simulated is decided once, in `[...slug]/route.ts`) and tags the
  block `SIMULATED` when it is. The per-turn badge in the UI is the real guarantee — the
  prompt instruction is a mitigation, and it was ignored entirely until it was phrased as
  a required output token rather than a description.
- **Tools: the model picks a name, never a URL.** `lib/scylar/tools.ts` hard-codes a path
  per tool, so no model output reaches an endpoint that is not on the list — the same
  reasoning as `lib/alchem/endpoint.ts` refusing a caller-supplied RPC URL. All GETs, no
  control routes. Row counts are clamped (models ask for 500) and repeated identical calls
  within a turn are answered from cache, because llama-3.3-70b re-calls rather than
  answers when it finds a result thin, and each round is a whole request against a 30/min
  tier.
- **Voice drives the mouth, not the other way round.** `FLAP_PERIOD_MS` is the fallback;
  when `useSpeech` is active, `SpeechSynthesis` word boundaries produce one open-close per
  word (`voicing` phase). Chrome silently stops after ~15s of a single utterance, so
  `splitForSpeech` is a correctness requirement; `onend` is unreliable, so the watchdog
  polling `speechSynthesis.speaking` is what stops a missed event from locking the UI.
  `pickVoice` ranks **gender before quality** — `SpeechSynthesisVoice` has no gender
  field, so it matches a name table on whole tokens, and ranking quality first picks
  "Andrew Online (Natural)" over "Zira" on stock Windows + Edge. The chosen voice name is
  surfaced in the UI because the installed list varies by OS, browser and connectivity;
  it is the only way a wrong pick is diagnosable off the operator's machine.
- **The Ψ gate measures staleness, not mood.** `GET /api/sentience` (Rust, backed by
  `scematica-sentience`) answers "can anything reading this API describe the bot right
  now?" — every read endpoint serves a state file identically whether it was written 4
  seconds or 4 hours ago, and `/api/health` only reports that a process *was* here. HOLD
  returns 409 and does not call the model; a warned model still writes a confident
  paragraph of stale numbers. Two traps, both hit during implementation: `Perception`'s
  data ratio is a **product**, so an unmeasured channel scored 0 pins Ψ at 0 and jams the
  gate shut forever — unmeasured dimensions are 1.0 ("not a limiting factor"), and only
  measured degradation moves the verdict, or a healthy bot sits in permanent CAUTION and
  the badge becomes noise. The handler overwrites only the measured fields via
  `state_mut`; calling `set_state` there replaces the timestep and sentience index too,
  which silently cancels every `/api/sentience/observe` on the next gate read. Ψ stays a
  pure function of measured data integrity by design — a run of coherent answers must
  not be able to talk the gate into trusting stale numbers.

`npm run check:scylar` pins the pure logic (expressions, speech, markdown, commands,
session, tools, gate). Run it after touching any of those modules.

Rebuilding `api.exe` on Windows fails with `Access is denied (os error 5)` while the API
is running — stop it first. Cargo reports this as a build error, not a lock error.

## Architecture: alchem-link terminal system (`term/`)

As of v0.23.0 the terminal UI is in-package — Textual is gone, and there is no `[tui]`
extra. `alchem_link/term/` is a complete stdlib TUI toolkit: `ansi.py` (escape sequences
+ truecolor→256→16→none negotiation + Windows VT), `screen.py` (double-buffered cell grid,
diff blit), `input.py` (raw mode + a pure escape-sequence parser), `widgets.py`,
`app.py` (event loop + worker pool), `boot.py`. The layering is strict and `term/` imports
nothing from `alchem_link` except `theme`. Four rules:

- **`theme.py` is inert and authoritative.** Hex values plus semantic `Style` roles, no
  escape sequences. `ansi` encodes a role for the negotiated depth; `render.py` uses the
  same roles for line output; `web/lib/alchem/` mirrors the values. Render code names a
  *role*, never a colour — `tests/test_theme.py` fails the build on a hardcoded hex in
  any render module, and on status colours that collapse into each other at 256 or 16.
- **Panels render to `List[Line]`, not to the screen.** A line is a list of
  `(text, Style)` segments; `Dashboard` paints a window onto the list. Scrolling and
  clipping are one slice, and every renderer stays a pure function testable without a
  terminal. Don't paint to a `Screen` from a panel renderer.
- **Colour is decoration, never the message.** `NO_COLOR`, a pipe, `--no-color` and a
  16-colour terminal must all produce the same text; `tests/test_render.py` asserts the
  coloured and plain forms are character-identical.
- **`boot.initialize()` repaints the terminal's own defaults** (OSC 11/10/12), not just
  the panes, and is idempotent because the CLI, the shell and the frozen binary each call
  it. It must always be paired with a `restore()` — leaving someone's terminal themed
  after exit is breaking it, not theming it.

Layout arithmetic uses `ansi.display_width`, never `len` — escapes and wide glyphs both
make `len` wrong, and the failure is a column that drifts one cell per row.

## Architecture: alchem-link coding agent (`workspace.py` / `approvals.py` / `agent_tools.py`)

The shell's chat agent can read, write and edit files, scaffold projects, export results,
and run commands. 28 tools. Two independent gates stand in front of every one of them,
and they answer different questions — keep them separate:

- **`Workspace` answers *where*.** Every filesystem tool resolves paths through it and
  nothing else; a tool calling `open()` directly bypasses the whole model. Paths are
  fully resolved (symlinks followed, `..` collapsed) and *then* compared against the
  root — a string check for `..` misses a symlink pointing at `/`.
- **`TrustPolicy` + `Approver` answer *whether*.** Risk is declared per tool
  (`read`/`network`/`write`/`execute`), so a new tool cannot arrive unclassified.
  Preflight order is: hard refusals → explicit rules → session grants → configuration.
  A refusal must never be reversible by a grant given for something else.

Four invariants worth not breaking:

- **Secrets are refused before the prompt, and the refusal is not overridable.** Tool
  results go to a third-party LLM, so reading `.env` is a disclosure of `ALCHEMY_API_KEY`,
  not a read. `PROTECTED_PATTERNS` covers env files, PEM/SSH keys, `.npmrc`, cloud
  credentials and **Solana keypairs** (this repo has them). Protected paths are also
  omitted from `list_dir`, `walk` and `search` — absent, not merely unreadable.
- **No terminal means deny.** `default_approver()` returns `DenyApprover` when stdin is
  not a tty. Piped `chat` and CI must not treat silence as consent; `--yes` is the
  explicit opt-out.
- **Execution is off until `--allow-exec`, and runs without a shell.** `split_command`
  produces an argv (see its docstring for why neither `shlex` mode alone is right on
  Windows). No pipes, no `;`, no second parsing layer between the approval prompt and
  what runs.
- **A refusal tells the model *why*, accurately.** `_refusal()` in `agent.py`
  distinguishes a policy refusal from a declined prompt. Saying "the user declined" when
  no prompt was shown makes the assistant report a decision nobody made.

Codegen routes through `generate_consumer`/`generate_project`, never the model: the
generator bakes in the per-chain **measured** heartbeat and the sequencer gate, and a
model writing that contract from memory hardcodes 3600. Same failure class as a
hallucinated price.

Grants are session-scoped and never persisted — a permission surviving the process turns
one keystroke into standing authorisation. `tests/test_agent_workspace.py` is the
security suite; most of its cases assert something does *not* happen.

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

- **Net:** Dueling Double-DQN `STATE_DIM(24) → 128 → 64 → {V(s), A(s,a)}`, He init, ReLU; `Q(s,a) = V(s) + A(s,a) − mean(A)`. (Standard MLP path retained for old checkpoints.)
- **State** (24 features, normalised to [0,1]): defined in `state.rs` — pool age, liquidity, price change, volume, buy/sell ratio, LP burned, mint renounced, PnL %, position age, daily PnL, streaks, balance, regime, volatility, spread, time-of-day, open positions, **peak PnL, pool score, deployer rug rate, volume velocity, price velocity, price acceleration**
- **Actions** (`action.rs`): `Hold`, `BuyStandard`, `BuyAggressive`, `SellPartial`, `SellAll`
- **Training**: Double DQN (online selects, target evaluates), **prioritized replay** (sum-tree, α=0.6, β 0.4→1.0), **n-step returns** (n=5); epsilon-greedy (1.0 → 0.05, decay 0.9995), target net hard-copy every 200 steps, replay buffer 10k, batch 64. Full reference: `docs/DQ_STAR_AGENT.md`
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
- Release profile is heavy: `lto = "fat"`, `codegen-units = 1`, `panic = "abort"`, `overflow-checks = true`. First release build is slow; `target/` reaches 5-10 GB fresh but **accretes without bound** across incremental debug+release builds — it was measured at 43 GB / 79k files before a clean. Run `cargo clean` when free space gets tight: a full disk surfaces as Windows `error 112` inside unrelated crates ("failed to write file... incremental"), which reads like a compile failure and is not one.
- The repo lives under **OneDrive**. `.gitignore` covers `target/`, `*.log` and `*.apk`, but OneDrive does not read `.gitignore` — exclude `target/` in OneDrive settings or it will try to sync tens of GB of build artifacts.

## Token Gate

Sniper and dashboard both enforce a 250k SCEMA balance check at startup with up to 5 retries. Set `SCEMATICA_SKIP_GATE=1` only during RPC outages — it bypasses the check entirely. SCEMA is a Token-2022 mint; gate code must use Token-2022 helpers, not legacy SPL Token.
