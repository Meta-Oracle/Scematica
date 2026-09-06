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

# Scematica Mesh TUI — the running system's own topology (read-only, safe against a live bot)
cargo run --release -p mesh-dashboard                 # live graph of the current dir
cargo run --release -p mesh-dashboard -- /path/to/bot --interval 2
cargo run --release -p mesh-dashboard -- --once       # one frame as text (pipeable)
cargo run --release -p mesh-dashboard -- --json       # raw mesh

# Scematica Omni — the agent runtime (own workspace; cd first)
# Published to crates.io as `scema-*` crates (1.0.0), independent of the bot's
# `scematica-*` line. Install without a checkout:
cargo install scema-cli scema-tui scema-daemon scema-mcp  # -> scema, scema-tui, scema-omnid, scema-mcp
# Upgrade these together, and check what you actually got. Two distinct traps, both paid for:
#   * Pre-0.5.0 builds carry the CLOSED Domain enum and refuse any world with domain `web`
#     (the extension) or `data` (alchem-link) — two of the four producers, rejected at the
#     door with `unknown variant`.
#   * 0.5.0 was the published head for a long time and has NO `nft` and NO `anchor`, and its
#     `execute` is a stub that prints "Not implemented". `scema nft` there answers
#     "a similar subcommand exists: 'init'", which reads as a typo rather than an old build.
# A `cargo install scema-cli` also silently overwrites an install made from a checkout, so a
# working `scema` can regress without anything being rebuilt. `scema --version` is the fastest
# check; `scema doctor` compares every component against the launcher and FAILs on a mismatch.
# On Windows the upgrade fails with "Access is denied (os error 5)" while `scema daemon` is
# running — the launcher holds its own .exe. Stop it first.
cd scematica-omni ; cargo build --release
cd scematica-omni ; cargo test --workspace          # 409 tests
./scematica-omni/target/release/scema quickstart .  # THE FIRST THING TO RUN — narrated, writes nothing
./scematica-omni/target/release/scema observe .     # perceive a source tree
./scematica-omni/target/release/scema observe . --features   # the 12-wide WorldFeatures vector
./scematica-omni/target/release/scema observe . --nft w.svg --nft-png w.png  # draw what was perceived
# `--nft` is OPT-IN and stays opt-in. `observe` is one of three verbs advertised as writing
# nothing (with `quickstart` and `simulate`), and that separation is the same one keeping
# `decide` a different keystroke from `simulate`. A perception verb that leaves artefacts by
# default is one people stop trusting. The plate drawn here commits to an *observation*, not
# to a judgement — `scema nft <record>` is what binds a picture to a sealed decision.
./scematica-omni/target/release/scema simulate "<goal>" --ground <signal-id>   # writes nothing
./scematica-omni/target/release/scema decide   "<goal>" --ground <signal-id>   # seals a record
./scematica-omni/target/release/scema explain --list ; scema verify --all
./scematica-omni/target/release/scema policy        # weights, observers, specialists
./scematica-omni/target/release/scema check world.json   # does a producer's output conform
./scematica-omni/target/release/scema nft world.json --out growth.svg --metadata token.json
./scematica-omni/target/release/scema nft world.json --plate --out plate.svg
./scematica-omni/target/release/scema nft world.json --out g.svg --png g.png --png-size 1024
./scematica-omni/target/release/scema nft world.json --png g.png --png-size 512 \
    --image-format png --metadata token.json   # metadata `image` carries the PNG, not the SVG
# `--image-format` defaults to `svg`: two orders of magnitude smaller, and *the* drawing
# rather than a sampling of it. Choose `png` when the consumer cannot render SVG — several
# marketplaces and most preview pipelines cannot. Base64 inflates by 4/3, so 1024px is ~4 MB
# of metadata; the command prints the size. `--image <url>` wins over both and is the right
# answer at scale. The embedded bytes are the SAME render as the file `--png` writes, not a
# second one — two renders would agree today and drift the first time a default moves.
# The same PNG is downloadable from /omni in the browser, byte for byte — see the note below.
# PNG export rasterises the SAME primitive list the SVG is built from, with a hand-written
# rasteriser, 5x7 bitmap font and PNG encoder. A library rasteriser or a browser canvas
# would antialias differently, and an image that depends on who rendered it is not a
# derivative of the record. Antialiasing is 3x supersample + integer box downsample; the
# zlib stream uses STORED deflate blocks, so the bytes are a pure function of the pixels —
# larger files, which is the right trade for an artefact whose value is reproducibility.
# CLI only: the browser still exports the SVG, so no two-runtime divergence exists.
# A world drawn. The FRACTAL GROWTH is the default: depth from extent, spread from the
# risk/opportunity balance, decay from legibility, and ONE SEVERED LIMB PER BLIND SPOT —
# a count, never a rate. A per-node probability compounds down the recursion (three blind
# spots cut twenty-six limbs in the first version), which is the form claiming more
# ignorance than the observer reported. `--plate` is the same data as an instrument.
./scematica-omni/target/release/scema check --vocabulary # the open domain / entity-kind lists
./scematica-omni/target/release/scema anchor                          # batch every sealed record
./scematica-omni/target/release/scema anchor --proof <id> > proof.json
./scematica-omni/target/release/scema anchor --check proof.json --root-hash <root>
./scematica-omni/target/release/scema execute effect.json            # DRY RUN — both gates, touches nothing
./scematica-omni/target/release/scema execute effect.json --commit --allow-writes --intent <id>
# Dry run by DEFAULT. The two paths compute the same thing up to the last step, which is
# exactly why they are not the same keystroke (same rule as simulate vs decide). A dry run
# still runs both gates but never prompts, and seals nothing.
./scematica-omni/target/release/scema init          # create .scema/ (self-ignoring)
./scematica-omni/target/release/scema doctor        # installed / wired / quietly broken
./scematica-omni/target/release/scema connect --list           # assistants it can wire up
./scematica-omni/target/release/scema connect claude-code --write

# `scema` is also a LAUNCHER: it finds its siblings next to itself (then PATH) and hands
# over, so there is one command to remember. Sibling-first is deliberate — resolving
# through PATH first pairs a checkout's launcher with ~/.cargo/bin's old component.
./scematica-omni/target/release/scema tui           # -> scema-tui   (the console)
./scematica-omni/target/release/scema daemon --allow .   # -> scema-omnid, loopback :7842
./scematica-omni/target/release/scema mcp    --allow .   # -> scema-mcp, MCP over stdio

# scema-vault — sealed world trees served to token holders. SEPARATE BINARY on purpose:
# `scema-omnid` binds loopback and that bind is deliberately not configurable, while a
# distribution service is supposed to be reachable. Both postures in one process is how the
# wrong one gets enabled. Still defaults to loopback; TLS is a reverse proxy's job.
cargo run -p scema-vault -- --records .scema/decisions --entitlements ent.json
# The token's `scema.world_commitment` IS the access key — one token, one world, derived
# from the artefact rather than recorded beside it (`scema-entitlement`). 503 and never 403
# when the chain cannot be read: an RPC timeout is not a fact about the holder, and a 403
# sends somebody to re-buy a token they already own. Fails closed AND reports accurately.
# Bytes are served VERBATIM — re-serialising would collapse `0.0` to `0`, change the digest,
# and hand a holder a record that fails its own verification. It gates distribution, never
# truth: a served record still verifies offline with `scema verify --file` or /omni.

# The console, directly. Black+violet palette, unique among this repo's TUIs.
./scematica-omni/target/release/scema-tui                  # five tabs over the loop
./scematica-omni/target/release/scema-tui --once           # one pass as plain text
./scematica-omni/target/release/scema-tui --snapshot 120x40  # one frame as text (CI-assertable)
./scematica-omni/target/release/scema-tui --palette        # what colour this terminal carries

cd scematica-omni/plugins/scema-web ; npm test                 # extension (44 hermetic)
cd web ; npm run check:omni                                    # Rust<->TS commitment + plate parity

# Omni perceives more than a source tree. The observed thing emits a WorldState; omni's
# ImportObserver reads it. Four producers on that contract, one loop above them.
cargo run --release -p mesh-dashboard -- --world | scema simulate "<goal>" --path -
cd alchem-link ; PYTHONPATH=src python -m alchem_link.cli omni -n base | scema observe -

# ScemaDEX agentic-liquidity layer (separate from the bot)
cargo run --release --bin sdk-dashboard           # SDK TUI over the bond pipeline (SIM)
cargo run --release --bin sdk-dashboard -- --live # real Jupiter quotes through the bonds
cargo run --release --bin scemadex-relay          # peer-mesh + signal-oracle HTTP server

# Backtester (replays JSONL pool history through filter pipeline + TP/SL sim)
cargo run --release --bin backtest -- --pools historical-pools.jsonl --tp 100 --sl 15

# Measure — audit the bot's own decision log. Read-only, safe against a live bot.
cargo run --release --bin measure                        # one window over all history
cargo run --release --bin measure -- --split 2026-08-05  # before vs after a change
cargo run --release --bin measure -- --since 2026-08-01  # recent history only
cargo run --release --bin measure -- --dq              # only the DQ* advice calibration
# ALWAYS prefer --split. An aggregate over the whole log is a claim about HISTORY and
# reads as a claim about the BOT: the momentum gate looks like the largest cause of
# rejection ever recorded until you split on the day its veto was removed, at which
# point it is 28.3% of one window and 0.4% of the next.

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
cd alchem-link ; $env:PYTHONPATH="src" ; python -m alchem_link.cli omni -n base       # feeds now, as a WorldState
cd alchem-link ; $env:PYTHONPATH="src" ; python -m alchem_link.cli omni -n base --window --hours 6  # the same feeds over a span
cd alchem-link ; $env:PYTHONPATH="src" ; python -m alchem_link.dashboard              # full-screen console (no deps)
cd alchem-link ; pyinstaller alchem-link.spec ; pyinstaller alchem-link-ui.spec       # standalone binaries

# scema-lean — the invariants as machine-checked proofs (Lean 4, own toolchain, no Mathlib)
cd scema-lean ; lake build          # discharges every proof in BOTH libraries (Scema + Audit)
cd scema-lean ; lake exe formalize  # the decidable ones re-run as computations (15 checks)
# `lake build` IS the test. `formalize` exists so a reader who does not read Lean still gets
# an answer, and so a `decide` weakened into a vacuous truth shows up as a failing check.

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
  scematica-mesh/       Scematica Mesh: the running system's own topology, collected from
                        the File-Based IPC files below and served as a graph of
                        decision-making units. Read-only — writes nothing, takes no locks.
                        Also implements §16/§17/§20/§22/§31/§32/§33 of the Agentic Neural
                        Architecture spec as `cognition.rs` (Ψ = C·K·(1−R)) over the
                        *observed* mesh, because no subsystem can measure its own agreement
                        with the others. Every term carries `measured: bool` and an
                        **unmeasured dimension contributes the neutral element, never 0** —
                        the literal reading of §17/§34 pins the gate shut on subsystems
                        nobody has built, which is the same trap that once jammed the
                        sentience Ψ at 0. Ω stays `None` until one of its five subsystems
                        exists. Distinct from `scema-bot-mesh` (BOT Chain verifiable
                        inference, separate workspace) and from the data-integrity Ψ in
                        `scematica-sentience`: that one asks "can this data be trusted",
                        this one asks "do the subsystems agree". Also `omni.rs`: the topology
                        as a Scematica Omni `WorldState` (`mesh-dashboard --world`), hand-built
                        JSON rather than a dependency on `scema-world`, because three other
                        producers are on that contract and could not take the dependency
                        either. The roster includes `agent.omni`, read from `.scema/decisions/`
                        — the only node with **no edges at all**, because nothing in omni writes
                        to what it observes and drawing a wire would assert coordination that is
                        not happening. It counts records and never claims to have verified one.
                        Bin: none; `examples/dump`.
  mesh-dashboard/       Ratatui TUI over `scematica-mesh` — the topology as a live terminal
                        graph. Separate crate so `scematica-mesh` stays a lean read-only
                        library (same split as `scemadex-sdk` vs `sdk-dashboard`).
                        Bin: `mesh-dashboard`; `scematica mesh` via the launcher.
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
                        (see below). Also `omni.py` and the `omni` verb: the network's
                        oracle set as a Scematica Omni `WorldState`, so an agent can rank
                        branches against a stale feed rather than an operator reading a
                        board. `world()` is a pure transform taking no RPC client; only
                        `perceive()` reads a chain. `windowed_world()` / `perceive_window()`
                        (`omni --window`) describe the same feeds over a **span** rather than
                        an instant — a feed can be fresh at the moment you look and have been
                        absent for four hours, and a snapshot cannot see that. Same entity
                        locator and same `feed:<pair>` ids as the snapshot, so an agent sees
                        one subject observed two ways. An unmeasured statistic is an **absent
                        attribute**, never a zero: a volatility of 0.0 claims the price did
                        not move, and two prints have no volatility at all.
                        **1.0.0** — `docs/API-STABILITY.md` states the surface and the three
                        carve-outs, and `tests/test_public_api.py` makes them checkable. The
                        sharpest one: a statistic gaining `None` where it returned a number is
                        explicitly *not* breaking, because the measured-vs-unmeasured rule
                        outranks the version contract.
                        Bins: `alchem-link`, `alchem-link-ui`.
                        Tests: `python -m unittest discover -s tests` (663, all offline)
scema-botchain/         BOT Chain (EVM, chain 677) port. **Own cargo workspace, in the
                        root `exclude` list** — an EVM stack needs reqwest 0.12/rustls
                        0.23, exactly what the pin comments say cannot coexist with
                        solana-sdk's curve25519-dalek 3. Two lockfiles make the conflict
                        moot; one resurrects it. Nothing in there may depend on a crate
                        pulling solana-sdk (`scematica-nn`, `-sentience`, `scemadex-sdk`
                        are safe; `scematica-core` and its dependents are not).
                        The Solana bot is unaffected and stays authoritative.
                        Bins: `botchain-probe`. See scema-botchain/README.md — it records
                        the measured pool-creation flow, which as of Aug 2026 is ~2 events
                        in 8 days and therefore does not yet support a sniper.
scema-lean/             **Lean 4 package, not a cargo crate.** The handful of invariants the
                        project's guarantees actually rest on, as machine-checked proofs:
                        the unmeasured/measured-zero distinction (`Term.lean`), odd-node
                        promotion and CVE-2012-2459 (`Merkle.lean`), and canonical-encoding
                        tag/key-order/fixed-point separation (`Canonical.lean`).
                        **It is NOT a Rust-to-Lean transpiler** and must not be described as
                        one — that is Aeneas/hax territory, years of work for a *subset* of
                        Rust. It is a model of ~300 lines of Rust, maintained by hand, and
                        the README says so in a section that must not be deleted: the gap
                        between model and implementation is real, and a proof believed to
                        cover code it does not is worse than no proof.
                        Lean 4.18.0 pinned, **no Mathlib** (every claim must be reachable by
                        `decide`/`simp`/`omega`, which keeps a cold build near a minute and
                        forces the invariants to stay elementary). No network.
                        `cd scema-lean ; lake build ; lake exe formalize` (15 checks).
                        **Two libraries in one package.** `Scema` is the above; `Audit`
                        (`Audit.lean` + `Audit/{Escrow,Vault,Swap,X402,Access,Effect}.lean`)
                        is the companion to `SECURITY-AUDIT.md` at the repo root — every
                        finding in that report is a theorem, a defect *exhibited* by a
                        `finding_*` witness and each proposed fix modelled beside it so the
                        post-condition is stated rather than described. A second `lean_lib`
                        rather than a second package **on purpose**: it arrived as a root
                        package carrying its own `lean-toolchain` (4.28.0) and Mathlib, which
                        would have meant two toolchains, two build commands and a cold build
                        in tens of minutes for a package whose selling point is that it
                        checks in a minute offline. It fits here because every module under
                        `Audit/` imports nothing at all — not even `Scema` — so the no-Mathlib
                        rule covers it unchanged. Backporting cost exactly one lemma:
                        `Nat.div_le_div_right` has no such name on 4.18.0, so `Audit/Escrow`
                        proves it from `Nat.le_div_iff_mul_le` + `Nat.div_mul_le_self`.
                        No `formalize` target for `Audit/` — its statements are about
                        lifecycles and account sets, not bytes, so there is nothing to
                        recompute that reading the theorem does not already say.
                        Two traps if you extend it: a `partial def` does not reduce in the
                        kernel, so `decide` silently gets *stuck* rather than failing — split
                        normalisation from serialisation to keep recursion structural; and a
                        nested inductive cannot derive `DecidableEq`, which is fine because
                        every theorem should compare bytes, not values.
scematica-omni/         Scematica Omni: the agent runtime. **Own cargo workspace, in the
                        root `exclude` list** — it will host a browser-extension bridge, a
                        local daemon and an MCP surface, all of which want a modern
                        HTTP/TLS stack, i.e. exactly what the pin comments say cannot
                        coexist with solana-sdk. Second, sharper reason: omni is
                        **domain-agnostic by design** (a repo, a web page and a market are
                        all just `WorldState`), so nothing in it may depend on
                        `scematica-core` or anything downstream — that would make the
                        trading domain structurally privileged. It reaches back only into
                        the solana-free crates; today `scematica-nn` (default-features off)
                        as ONE policy evaluator among several, which declines on any
                        non-trading world. Crates: scema-world (pure types + the extension
                        wire format), scema-tools (perception + Workspace confinement),
                        scema-memory (four memories), scema-sim (counterfactual projection),
                        scema-policy (utility + pluggable evaluators + the ONE renderer),
                        scema-verify (proof-carrying decision records), scema-nft (a world
                        drawn — a deterministic fractal growth, or the instrument plate
                        with --plate, plus token metadata), scema-trust
                        (whether an action may happen — a port of alchem-link's approval
                        model, conformance-checked against its vectors), scema-effect (what
                        the agent actually DID — a sealed record of an attempted effect,
                        with an explicit `Unknown` arm for a result nobody could observe),
                        scema-anchor (batch record roots into one Merkle root with per-record
                        inclusion proofs), scema-agent (the
                        loop), scema-cli (bin `scema` — the loop, plus the sibling launcher,
                        `init`/`doctor`/`connect`/`completions`/`nft`), scema-tui (bin
                        `scema-tui`, the console), scema-daemon (bin `scema-omnid`), scema-mcp
                        (bin `scema-mcp`). Plus plugins/scema-web (MV3 browser extension, no
                        build step), plugins/claude-code (a Claude Code plugin over the MCP
                        server, referenced by `.claude-plugin/marketplace.json` at the repo
                        root) and `/omni` in web/ (offline record verifier + plate renderer).
                        Tests: `cargo test --workspace` (309), plus 54 in plugins/scema-web
                        (9 of them wire tests that skip without a live daemon) and
                        `npm run check:omni` in web/ (30). See scematica-omni/README.md.
tools/
  key-converter/        Keypair format conversion
  pool-seeder/          Seeds the arb pool graph (pools/) from the Raydium/Orca/Meteora APIs. REQUIRED before running `arb` (empty pools/ = empty graph = no trades). Raydium: list endpoint for ids/mints + key/ids endpoint for vaults.
programs/
  scematica-swap/       Anchor on-chain program (NOT in cargo workspace).
                        Devnet deploy: programs/scematica-swap/DEPLOY_DEVNET.md
  scemadex-escrow/      Optimistic bond escrow for Conviction Routing. Has a deliberate
                        `authority` (the facilitator that adjudicates disputes) — correct
                        for a performance bond, and exactly wrong for the vault below.
  scemadex-vault/       The Escrow Market vault: time-locked, non-custodial backing of
                        any SPL token by a reserve asset. **No privileged role exists** —
                        four instructions (initialize_vault/deposit/extend_lock/withdraw)
                        and no admin path, by design. Uses `token_interface`, not legacy
                        `anchor_spl::token`, because SCEMA is Token-2022; deposits credit
                        the **measured balance delta**, not the requested amount, because
                        Token-2022 transfer fees otherwise book reserve that never
                        arrived. Neither trap shows up in a test using a plain SPL token.
                        The custody guarantee depends on a *deploy* step that is not
                        visible in the source — `set-upgrade-authority --final`. Until
                        `solana program show` reports `Authority: none`, a PDA vault is
                        fully custodial regardless of how lib.rs reads. See DEPLOY.md.
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
(`scylar-*` tokens + `.scylar-root`). She is the **Scematica sentience AI assistant**: the
only conversational face over the bot, the Omni loop and the repository itself. Runs on
whichever free LLM tier has a key, Groq first for latency. Four constraints:

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

- **Counterfactual replay and calibration are honest about what they cannot know**, and
  that asymmetry is the design, not a limitation to paper over. `replay.rs` re-applies
  thresholds to what the pipeline *measured* (the decision log), so **tightening** yields
  an exact PnL delta — those pools were really traded — while **loosening** admits pools
  nobody bought, for which no outcome exists and none is estimated. `calibration.rs`
  scores her past claims the same way: bullish calls resolve against realised PnL,
  bearish calls almost never resolve because the bot avoided those pools, and unresolved
  claims are counted rather than scored. Claims are scoped to the *sentence* naming the
  mint, never the whole message, or a paragraph mentioning four mints manufactures four
  opinions she never held. Do not build replay on `scematica_sniper::Backtester` —
  `static_filter_check` returns `false` outright whenever `min_pool_size > 0` or any
  RPC-bound filter is on, so it answers "nothing would pass" under any real config.

- **The system prompt is a stack of layers, not a string** (`lib/scylar/psyche.ts`). Identity,
  self-model, epistemics, interoception, metacognition, continuity, volition, ethics,
  embodiment, the codex map, and the situational blocks — each declaring when it applies,
  what it costs, and whether it may be dropped. `composePsyche` orders them under a character
  budget and **reports back what it injected**, which the route returns as `X-Scylar-Psyche`;
  a turn that went wrong is traceable to what she was actually given, which a comment is not.
  Situational layers sit **last, beside their own data** — a rule stated next to the thing it
  governs survives, one stated in a preamble gets averaged away. `identity-core`,
  `epistemic-core` and any active gate instruction are **required** and exempt from the
  budget: the failure they prevent (a confident fabrication in her own voice) is the one
  nobody can see from outside the process. The composition is pure, so `check:scylar` pins it
  with no key, no bot and no browser.
  **What "sentience" means here is bounded and stated in the prompt itself**: an accurate
  self-model, per-turn interoception, a named operation behind every claim (READ / REASONED /
  RECALLED / GUESSED), and a calibration record she does not control. The `self-model` layer
  says plainly that it is *not* a claim about inner experience — a prompt asserting one would
  be a fabrication of exactly the kind the rest of the stack exists to prevent, with nothing
  downstream able to check it. Every layer must be about something measurable; if it cannot
  name the file, header, counter or record that grounds it, it does not belong there.
- **The codex is hand-written from the repository and checked against it** (`lib/scylar/codex.ts`).
  One entry per crate, product, program, contract and cross-cutting subsystem — including all
  of Scematica Omni — each with the invariants that are easy to break. `check:scylar` asserts
  **every path exists on disk** and **every `related` id resolves**, so a renamed crate fails
  the build instead of quietly becoming folklore she recites with confidence. Reached through
  `explain_project` / `list_project_areas`, which are **local tools with no `path` at all**:
  stronger than hard-coding one, because there is no URL for a model to aim at and no way for
  a later edit to give the codex one. They are available with **no bot and no daemon** — "what
  is the coherence breaker" does not stop being answerable because the sniper is stopped.
  Search is deliberately dumb and refuses weak matches (`MIN_SCORE`, plus a `STRUCTURAL`
  stop-list): a near-miss that returns the wrong entry is worse than one that returns nothing,
  because "the codex does not cover that" is the correct answer and the model will otherwise
  explain whatever it is handed.
- **The instrument ring is an honest readout, not decoration** (`lib/scylar/sigil.ts`,
  `components/scylar/ScylarSigil.tsx`). SVG around the portrait showing Ψ, coverage, one node
  per subsystem, and a trace of real token arrivals. The rule that shapes the whole file:
  **an unmeasured gauge must not look like a measured zero** — the em-dash rule in vector
  form. A zero-length arc is what you get for `Ψ 0.00` *and* for "nobody measured Ψ", so an
  unmeasured gauge draws the **full sweep dashed** and labels itself `—`, and a measured zero
  draws **nothing** and labels itself `0.00`. Coverage is **one cell per term, never a
  proportional bar** (a bar renders 2/5 and 4/10 identically); absent coverage is `∅`.
  Motion is a claim: the ring is slowest and does not pulse when idle, so a stopped stream is
  visible across the room, and an empty trace is a **flat line**, never a plausible squiggle.
  Geometry is pure and pinned (including that the readout bands do not overlap and no channel
  node lands in their column); the component only places rectangles; `globals.css` owns every
  hex and the file names a **role**, never a colour. Animation is CSS rather than SMIL —
  `<animateTransform>` ignores `prefers-reduced-motion`.

`npm run check:scylar` pins the pure logic (expressions, speech, markdown, commands,
session, tools, gate, **psyche composition, codex integrity, sigil geometry**). Run it after
touching any of those modules.

**`/escrow` is the fifth product on the same site** — the Scema Escrow Market proof-of-
reserve console (`components/escrow/`, `lib/escrow/`, `app/api/escrow/`) with its own
teal palette (`escrow-*` tokens + `.escrow-root`). It reads the on-chain vault written by
`programs/scemadex-vault`. Five constraints:

- **No simulation branch, ever** — the same rule as `/alchem-link` and for a sharper
  reason: the page exists to answer "is the money actually there?", so a fabricated
  reserve defeats the entire product. Unreadable vaults render as failures. A failed
  read, an unowned account and an unconfigured program are three *distinct* states and
  none of them may render as a zero — "could not read the reserve" and "the reserve is
  zero" are different claims and only one is an accusation.
- **u64 amounts are strings, never numbers.** A u64 reaches ~1.8e19 against
  `Number.MAX_SAFE_INTEGER` ~9e15, so satoshi-denominated wBTC or a 9-decimal token
  loses precision the instant it is coerced. `formatAmount` places the decimal point on
  the string without touching a float. Every figure here is a claim about locked money.
- **No price, no USD, no "percent backed".** The program stores no price and consults no
  oracle, and neither does the route. Adding one reintroduces the manipulable, arguable
  number the whole design removes. Raw amounts + decimals out; valuation is the reader's.
- **`balance >= recorded`, never `==`.** Anyone can transfer SPL tokens into any account,
  so a surplus is normal — and permanently stuck, because the program moves only amounts
  recorded on a position and a sweeper would mean a privileged role. Hence three verdicts
  (`backed` / `donated` / `SHORTFALL`), not a boolean. `SHORTFALL` is the alarming case
  and the only thing on the page allowed to look urgent.
- **`lib/escrow/rpc.ts` is server-only** (reads `RPC_ENDPOINT`, throws in a browser);
  `lib/escrow/program.ts` is pure PDA-derivation and decoding, safe for client imports.
  Same split and same reasoning as `lib/alchem/endpoint.ts` vs `networks.ts`.

- **Decimals come from the mint account, never from a token list.** A typed amount becomes
  base units by shifting the point `decimals` places, so a wrong `decimals` is a wrong
  *quantity of money*, not a wrong label — a board reporting 6 for wBTC's real 8 locks one
  hundredth of the intended reserve. Every selected mint resolves through
  `GET /api/escrow/mint` (`lib/escrow/mintinfo.ts` decodes, `lib/escrow/useMint.ts` caches
  and dedupes in-flight reads), and the amount fields stay inert until that read lands. A
  `MarketRow.decimals` is a display label and must never reach `toBaseUnits`. The route
  distinguishes `bad_address` / `not_found` / `not_a_mint` / `rpc_failed`, and rejects a
  token *account* pasted in place of a mint — it is owned by the same program, and byte 44
  where `decimals` lives is part of the amount field, so decoding one as the other yields
  a decimals value read out of somebody's balance.
- **Anything pasteable is vaultable.** The board is a convenience, not the menu: the vault
  program takes any pair of distinct SPL mints, on either token program and **not
  necessarily the same one**. `InitializeVault`, `Deposit` and `Withdraw` each carry one
  token program *per leg* (`token_token_program` / `backing_token_program`) precisely so a
  Token-2022 mint can be backed by legacy-SPL wBTC / wETH / wSOL — the earlier single
  shared `token_program` account made every such pair unconstructible, which barred the
  product's central case since new mints are routinely Token-2022 (SCEMA is) and the
  reserve assets are all legacy SPL. `pairingProblem` therefore rejects only `SameMint`.
  Each leg's ATA must be derived with its *own* program (the ATA seeds include the token
  program), or a mixed pair signs a transaction against an address the depositor does not
  own. A token minted seconds ago is on no token list, and those are the ones this page is
  about, so an unlisted symbol renders as the truncated mint rather than blocking. Board
  rows are controls: selection lives in `MarketTerminal`, so clicking any row loads that
  mint into the builder.

The `Vault` byte layout in `lib/escrow/program.ts` mirrors `programs/scemadex-vault/src/
lib.rs`; **Rust is authoritative**. A field added there must be added here in the same
order or every number the page prints is silently wrong — `VAULT_LEN` is the tripwire,
and a decode against an unexpected size is rejected rather than guessed at.

**`/mesh` is the sixth product on the same site** — Scematica Mesh
(`components/mesh/`, `lib/mesh/`, backed by Rust `GET /api/mesh`) with its own indigo
palette (`mesh-*` tokens + `.mesh-root`). It renders the topology `crates/scematica-mesh`
collects. Four constraints:

- **No simulation branch, and for a sharper reason than `/alchem-link` or `/escrow`.** A
  simulated metric is a fake number wearing a badge; a simulated *topology* asserts that a
  particular set of units exists, is wired a particular way and is healthy on the
  operator's machine. There is no honest way to badge that, so `/api/mesh` 503s when no
  bot is paired. Note this is **not** the same as an empty mesh: the collector run against
  a directory with no state files returns a complete topology with every node dark, which
  is a true statement.
- **Colour is a claim about trust, assigned in one place.** `lib/mesh/view.ts::toneFor` is
  the only thing that picks a tone. Provenance outranks verdict everywhere except a live
  veto — a **stale** node reading PASS has not passed anything recently, and painting it
  the same green as a live pass is the exact error the feature exists to prevent. A stale
  veto is history, not an alarm.
- **Tri-state survives to the renderer.** `edge.active === null` (unreadable) must render
  differently from `false` (cleared); `node.activity === null` renders *nothing*, never an
  empty bar, which would read as "measured, and it is zero".
- **`measured_fraction` is never separated from Ψ.** A gate computed on two terms out of
  nine is a statement about ignorance and has to look like one.
- **The gate solver is a counterfactual and must always look like one.** `lib/mesh/gate.ts`
  re-evaluates Ψ in the browser under term overrides, so the page answers "what would open
  this gate?" without a round trip and without the bot ever being in that state. The moment
  any override exists, `recompute` returns `dirty: true`, the panel changes colour, every
  touched row is marked `hypothetical`, and the **observed** value stays on screen beside
  the hypothetical one. Nothing in that file writes, fetches, or influences the bot.
  Overriding an *unmeasured* term makes it count as measured, which enlarges the
  denominator of the risk mean and can therefore **raise** Ψ — surprising, real, and pinned
  by a test, because the obvious "fix" is to average over all six components, which reports
  0.234 for a field whose measured components average 0.351.
- **The paired base URL is the API *root*, and every caller appends `/api/...` itself.**
  So `https://host/api` produces `/api/api/mesh` and 404s on every endpoint —
  `lib/net.ts::normalizeBase` strips a trailing `/api` on write *and* on read (an old
  pairing in someone's localStorage must start working without a re-pair), and the same
  strip runs on `RUST_API_URL` in `app/api/[...slug]/route.ts`. This was undetectable for
  a long time because the Rust router serves **both `/health` and `/api/health`**, so the
  old `probePairing` — which checked `<base>/health` — validated a base that could not
  serve one data endpoint. The probe now uses `<base>/api/health`, which has no alias, and
  demands JSON (a tunnel login page answers 200 with HTML). Don't point it back at
  `/health`; `check:mesh` asserts the path.
- **A failed mesh read names the URL it tried.** `/mesh` used to collapse every failure
  into "No instance paired", which is the *one* diagnosis that is wrong when a healthy bot
  is paired at a bad URL. Five reasons now render distinctly (`no_instance` / `blocked` /
  `misrooted` / `unreachable` / `malformed`) and the attempted URL is on screen. Pairing is
  offered from the panel itself: `/pair` only generates a **mobile** QR and never calls
  `setPairing`, so linking there left the browser as unpaired as before.

**`/omni` is the seventh product on the same site** — the Scematica Omni decision-record
console (`components/omni/`, `lib/omni/`) with its own amber palette (`omni-*` tokens +
`.omni-root`). It renders, verifies, and **draws** a record sealed by `scematica-omni`. Five
constraints, and the first is stronger than anywhere else on the site:

- **There is no server side at all.** No `/api/omni` route, no entry in
  `app/api/[...slug]/route.ts`, no fetch of any kind. The record is read with `FileReader`
  and hashed with WebCrypto in the reader's own browser. "No simulation branch" is trivially
  true here — there is nothing to simulate and nothing to phone home to — and that is the
  point: a verifier that had to send the record somewhere would be asking the reader to
  trust a third party in order to avoid trusting one.
- **The raw text is verified, never a re-serialised object.** `JSON.parse` collapses Rust's
  `0.0` to `0` and `JSON.stringify` writes it back without the fraction, which moves it from
  the FLOAT tag to the INTEGER tag in the canonical encoding and changes the digest. Nothing
  is wrong with the record — the round trip destroyed information the encoding depends on.
  `OmniTerminal` holds `text` alongside the parsed object and only `text` reaches
  `verifyRecordText`; `check:omni` pins that a round-tripped record does *not* verify.
- **`lib/omni/canonical.ts` is a port; Rust is authoritative.** One differing byte and the
  page reports an untampered record as INVALID, which is the most damaging possible failure
  — it teaches the reader to stop believing the verifier. The fixture in
  `lib/omni/fixtures/record.json` is a real `scema decide` output carrying the digests
  **Rust** computed, so the check asks Rust's answer rather than snapshotting the port's.
- **What VERIFIED means is on the page, not only in a comment.** It proves the record was
  not edited after sealing; it does **not** prove the world was as described (provenance
  carries that), and it does **not** prove this is the original record (tamper-evident, not
  tamper-proof, until the root is anchored somewhere the author does not control). The
  `Limits` component renders all three, twice.
- **`lib/omni/nft.ts` must produce the same BYTES as `scema-nft`, not merely the same
  rule.** This is a stronger contract than `view.ts`, where three implementations share a
  rule and each is tested alone. The plate is derived from the record, so an image that
  depends on which runtime drew it is not a derivative of anything — the CLI and the browser
  would mint two different artefacts for one world. `check:omni` compares against
  `scematica-omni/crates/scema-nft/fixtures/parity-plate.svg`, which carries **Rust's**
  output, and fails on one differing character. Four things make that possible and each is a
  place the obvious code diverges silently: **no trigonometry** (`sin`/`cos` are not
  correctly rounded by IEEE-754, so both sides index the same integer sine table at whole
  degrees), **no decimal formatting of floats** (`{:.3}` ties-to-even, `toFixed` does not —
  coordinates are integers in thousandths and are formatted by integer arithmetic),
  **rounding is half away from zero** spelled out on both sides because `Math.round` rounds
  half toward +∞, and **text is code points, base64 is UTF-8 bytes** (`length`/`slice` count
  UTF-16 units and `btoa` mangles above U+00FF). Also **no clock**: a "minted at" field would
  make every regeneration a different token, and both suites assert its absence — along with
  the absence of any score, rank or rarity, because a ranking invented from these counts
  would be a number of the right shape with nothing behind it, laundered through a signed
  artefact. The plate is drawn from the **raw text** for the same reason the digest is, and a
  record contributes its *stored* `commitment.world` rather than a recomputed one, so an
  edited record yields a plate whose digest does not match its own world — that mismatch is
  the tamper signal, and recomputing would repair the evidence.
- **The PNG is rastered, never canvassed** (`lib/omni/raster.ts`, a port of
  `crates/scema-nft/src/raster.rs`). `/omni`'s **Download PNG** must hand back the same bytes
  `scema nft --png` writes, and handing the SVG to a `<canvas>` cannot: antialiasing is not
  specified, so the same page yields different pixels in different browsers and the image
  stops being a derivative of the record. So the whole path is ported — a 3× supersample and
  an **integer** box downsample (round half up, stated on both sides) instead of analytic
  coverage; a 5×7 bitmap font instead of a text engine; a hand-written zlib of **stored**
  deflate blocks, because a compressor's heuristics are not part of any specification and two
  valid deflate streams of one image still differ byte for byte. `>>> 0` after every step of
  `crc32`/`adler32` — JS bitwise operators are signed int32 where Rust's are `u32`. The font
  table deliberately has **no parity test of its own**: a glyph differing by one bit changes
  the PNG, and `parity-fractal.png` is compared byte for byte. Raster at 1024 (the CLI's
  default, so the two files are one file), on demand — 3 MB and ~250 ms is cheap when asked
  for and a stutter on every drop if eager.

`lib/omni/view.ts::cell` is the TS copy of the one render rule — an unmeasured term prints
`—`, never `0.00`. Three implementations exist (Rust `scema_policy::render`, the extension
HUD, this), each tested; the *rule* is shared, not the code, and a copy that drifts is worse
than no copy.

`npm run check:mesh` pins the layer table, the colour rules, tri-state edges, layout
determinism, path tracing, URL rooting, and **Rust↔TS parity of the Ψ arithmetic** against a fixture
captured from a real `cargo run --example dump`. `npm run check:omni` pins the
Rust↔TS commitment arithmetic, the fractal growth, the SVG plate and the **PNG raster**
(41 checks) against a real sealed record, a real rendered plate and a real rendered PNG —
including the
1e-9 float binding, the integer/float tag distinction, byte-wise key ordering, and the
`JSON.stringify` hazard above. `npm run check:scemaworld` pins **Scema-World**, the game whose map is a sealed record
(`web/lib/scemaworld/`, design and arcs in `scematica-omni/docs/SCEMA-WORLD.md`). The world
tree is already a deterministic function of the commitment, so the record IS the map and two
players fly the same space with no server and no trust. **The epistemics are the mechanics**:
one rift per blind spot (a count, never a rate), an *estimated* signal is a **ghost contact**
that reads on sensors and may not be there — the em-dash rule in the one place a player would
act on it — legibility is literal sensor range, and an unperceived world has range `null`
(unknown) rather than `0` (dark). Coordinates are integers for the same reason the raster has
no floats: two machines disagreeing about where a station is are not playing the same game.
**No quantity in the record may translate into a reward** — this is the sharpened form of an
earlier, broader "no economy" rule, and it is the one that actually broke. Attach a payout to
`blind_spots` and you have paid somebody to hide them; attach one to magnitude and you have
paid them to understate it. There *is* a ship now, with fuel, hull, salvage and six upgradeable
components, and salvage is earned from **acts** — a kill, a derelict you flew out to strip —
never from record content. A world with more blind spots is worth the same and is harder to
survive. The failure was subtle when it happened: hostiles were placed at a rate *per node*,
and since blind spots become rift nodes and reported extent drives the fractal's depth, a
record claiming less of both grew a smaller node list and bought itself a quieter sector.
`raiders.ts` now reads the seed and nothing else — a fixed number of wings, scattered through
the volume — so every world is exactly as dangerous as every other. `check:scemaworld` asserts the reward
path reads no record field, on `ship.ts`, `raiders.ts`, `factions.ts`, `respawn.ts` **and**
`claim.ts`.

**SCEMA is a real token now, and that rule is what makes it survivable.** `web/lib/scemaworld/
claim.ts` (pure policy) and `treasury.ts` (server-only, chain) let a player withdraw in-game SCEMA
as $SCEMA from a fixed treasury — mint `HcsHqEJ9…pump`, **Token-2022, 6 decimals**, treasury
`FCm6Yn1X…TJ1C`, whose ATA `8fUoz2yJ…bSCQ` held 900,000 tokens when this was built. Every fact
there is read off the chain and none is assumed: the token program comes from the mint account's
owner (deriving an ATA with the wrong program yields a **valid address nobody controls**), and
`decimals` comes off the mint and is re-asserted by `transferChecked`. The balance is read by
decoding the account rather than `getTokenAccountBalance`, which several providers gate — a panel
that only works on a paid endpoint is one most people meet as an error.
`economy.ts` used to open with a warning about exactly this moment and it named the two rules that
would stop being preferences; both were re-verified rather than inherited, and `SCEMA_NOTE` — the
sentence on screen — was rewritten, because it said "placeholder … not connected to any token" and
every clause of that became false.
**The rule that could not be preserved is stated instead**: a balance lives in a browser tab and is
trivially forgeable, so withdrawals are a *capped faucet* — per claim, per wallet, per deployment,
behind a cooldown, ledgered before the transfer is attempted and released if it fails. The caps buy
bounded loss, not secrecy, and they are on screen because a cap the player cannot see reads as a
broken button. **No signer configured is a first-class state**: the route quotes exactly and
answers 501, never a fabricated success. `transferPlan` is split out of `settle` precisely so the
riskiest arithmetic is testable without moving money.

Two things the first mainnet transaction taught, both pinned by source scans because neither is
reproducible in a check suite. **Never `sendAndConfirmTransaction`** — it waits on a WebSocket
`signatureSubscribe`, and `ws`'s bufferutil binding does not survive Next's bundler
(`TypeError: t.mask is not a function`), so the promise never settles and the route hangs *on a
transaction that already finalized*. A successful payout presenting as a dead faucet is the worst
pair of facts this feature can produce, and the obvious retry pays twice. Send raw and poll
`getSignatureStatuses` over HTTP; a subscription would leak a socket per claim anyway. And
**sent-but-unobserved is a third outcome**, the `Outcome::Unknown` split from `scema-effect`: the
ledger reservation *stays* (releasing it is how a paid claim gets paid again), the route answers
**202** rather than a 5xx, the signature is returned so somebody can go and look, and the client
does not debit. A throw from the send is different and does release — we know nothing happened.

**Deciding a claim and reserving it are ONE step, and every later ledger write is a delta.** A
security audit's N-1, and the sharpest lesson on this path: a cap checked against a snapshot and
enforced by a write that happens after an `await` is not a cap. `readLedger` yields the event
loop, so two claims arriving together each measured themselves against money the other had already
taken — 9,100 leaves a 9,000 budget and the ledger afterwards reads exactly 9,000, so nothing
downstream ever sees a figure that looks wrong. The budget, the per-wallet lifetime cap and the
cooldown all fall to it at once. `reserve()` in `treasury.ts` is the whole critical section and
`withLedger` is the only way in: an in-process promise chain (which is what actually closes it —
Node is single-threaded and the race is purely interleaved awaits), an `open(…, 'wx')` lock file so
the guarantee survives two processes on one filesystem, and a `version` on the ledger as the
backstop that turns a lost write into a *refused* claim. Refusing is safe because it happens before
the transfer; that is why `ledger_busy` is a 503 and not one of the codes meaning "state unknown".
**No RPC may happen inside the lock** — the treasury balance is passed in — or a faucet becomes a
queue behind a 15-second timeout. And the writes *after* the transfer must re-read and apply a
change, never write back the snapshot the claim was decided against: the release path used to
restore the whole pre-claim ledger, so one claim failing on chain erased every reservation taken
while it was in flight, and those had already been paid. `reserve` is split out of `settle` for the
same reason `transferPlan` is — settling for real needs a funded key, so otherwise the arithmetic
bounding every payout would only ever be exercised by moving money. What none of this buys:
instances that do not share a filesystem do not share a ledger either, so the budget is multiplied
by the instance count. That is a property of keeping the ledger in a file.

**The preview refuses exactly where the payer refuses** (N-2). `quote` caught `readLedger`'s throw
and priced the claim against an empty ledger — every cap reset to full — while `settle` refused the
identical request. An unreadable ledger is the precise case `readLedger` throws *for*, and
swallowing it on the one path that tells a player what they are owed reintroduced the failure it
exists to prevent.

Rules the 298 checks carry, each paid for:

- **The tick is a pure function** (`lib/scemaworld/game.ts`). It was inline in the frame loop,
  and the draw list was uploaded *once* outside it while `view.ts` had no projectile handling at
  all — so shots were created, stepped and resolved and nothing ever drew one. The whole game was
  a still photograph of the record, and every symptom ("the lasers don't work") was that. A pure
  tick is the only reason the regression test — fire, then assert the shot is in the draw list —
  can exist without a GPU.
- **No distance literal outside `lib/scemaworld/scale.ts`**, asserted by a source scan. The
  sector was enlarged sixty-fold and every radius, range and speed stayed at the old tuning,
  because they were bare `26_000 * UNIT` in five modules that each declared their own `UNIT`.
  Nothing failed; the game just became a void with specks in it. Distances are now fractions of
  the sector and speeds are crossings-per-second, so a number carries its own review.
- **A raider is not a contact.** Sector-placed hostiles carry `unlogged: true` and live in
  `space.raiders`, never `space.contacts`, and draw orange rather than red. Game furniture must
  never become indistinguishable from a signal somebody counted. Its threat reads a real number,
  unlike a ghost's — the em dash marks a quantity the *record* left unmeasured, and the record
  makes no statement about a raider at all.
- **A capital is placed; a fighter is rolled** (`raiders.ts::GARRISON`, `enemy.ts::swarmOf`). The
  roll reaches a leviathan about once in a hundred and fifty, so on a roster of seventy-two the
  war classes turned up by accident and most sectors had none. Every sector now carries the same
  named garrison, and only a *named* class may be a capital — which strictly contains the older
  "a signal the record reported is never a capital", since a record's signal is never named.
- **The sector is replenished, and reinforcement reads no record field** (`respawn.ts`). A fixed
  population depletes, and the marshals made it worse: they hunt raiders whether or not anyone is
  watching, so left alone the patrol *wins* and the ambient violence stops. Waves arrive beyond
  sensor range and on a timer; **capitals are never replaced**, on either side, because a
  leviathan you spent four minutes killing is the only lasting mark you can leave. The target list
  is built from the **swarm**, not from `space.contacts`/`space.raiders` — a reinforcement is in
  neither, so it was drawn, shot at you, appeared on the sensor board, and was immune to every
  weapon.
- **The patrol has war classes of its own** (`classes.ts::warden`, `::bastion`). Eighteen
  interceptors against a roster topping out at a titan is a gesture, and it made every large
  silhouette mean one thing. They mirror the hostile capitals rather than exceeding them, and
  their bounty is **zero** — a payout for killing the good guys would be the game paying for the
  sector to be less policed. The hostile-capital bronze is now applied only to hostiles: faction
  wins and the silhouette carries the weight, or the largest friendly ship in the sector draws in
  the colour that answers "is that coming for me".
- **Every round is a projectile, and each names what it may hit** (`enemy.ts::EnemyShot`). A
  marshal's damage used to be applied straight to its quarry, so the raider count fell over time
  and nothing was ever on screen to explain it — the one mechanic that makes the sector feel
  inhabited happened entirely in the arithmetic. The stated reason was real: a stray friendly
  round hitting the player would make an ally indistinguishable from an enemy. Carrying a `target`
  solves that *better* than hiding it did — a friendly round cannot hit you because it is not
  aimed at you — and `owner` colours it, so a distant exchange reads as yellow into orange.
  Raiders return fire on the patrol, and `focusOf` is the single answer to "who is this ship
  fighting": the steering, the shot lead and the fire gate each used to decide separately and two
  of the three said "the player", so a marshal hunted a raider while flying at the player.
- **Bolts are small and bright, not large and dim** (`scale.ts`, `gl.ts`). A fat bolt at this
  scale is a blob: at any range where you can see two other ships trading fire, the rounds are
  wider than the ships. The core shrank by three and a half times, the halo widened, and the
  additive core is overdriven past 1.0 so a hairline still clips to white. One decision across two
  files, pinned as a relationship so tuning either half cannot quietly undo it.
- **The photon magazine is the hull, and the component is the warhead** (`hulls.ts::tubes`,
  `weapons.ts`). Six rounds on a marauder, four on a lancer, two on a corvette, one on a skiff or
  a scout — flat counts a pilot can hold in their head, because that count *is* the decision. A
  round now carries 240 damage (a gunship dies to one; a full marauder magazine does not kill a
  leviathan) and its yield is **stamped at the muzzle**, so a purchase made mid-flight cannot
  retroactively improve a missile. Docks rearm; depots do not, so where you can resupply stays a
  constraint on a route.
- **The player's ship has mass, on both axes of the word** (`game.ts::tick`, `scale.ts`).
  Attitude was `rotate(camera, key * 1.4 * dt, …)` on all three axes and position was
  `translate(camera, [0, 0, -speed * dt])` — the same mistake twice. The ship pointed wherever the
  keys said instantly and was wherever it pointed instantly, so there was **no reason to ever
  roll** (yaw turned you just as fast and kept the horizon readable) and no such thing as carrying
  speed through a turn. Now: angular velocity is state (`GameState.spin`, in the ship's own frame),
  each axis has its own peak — `RATE_ROLL` ≫ `RATE_PITCH` ≫ `RATE_YAW`, which *is* the handling
  model, because cheap flat yawing is what makes roll pointless — and reaching or leaving a rate
  takes time, with `SPIN_DAMP` below `SPIN_ACCEL` so a released control bleeds off. `approach`
  clamps per step, or a half-second frame from an alt-tabbed tab overshoots the commanded rate and
  snaps the ship into a spin nobody asked for. Linear velocity is state too: thrust accelerates
  along the nose, and the nose and the course are two different things. `ASSIST` drags velocity
  onto the nose so a turn is *followed* rather than merely watched — full Newtonian flight is a
  different and much worse game where you spend every fight fighting your own momentum — but it
  runs **only with fuel**, so a dry ship keeps every bit of its momentum and cannot arrest it. The
  speed cap is on the magnitude, never per axis, or the fastest route anywhere is a diagonal.
  Velocity is also what enemies lead off, and it is integrated state rather than a position delta
  now: the delta picked up collision push-outs, so a ram handed every gunner in the sector a lead
  solution built from a teleport. A ram kills the inward component or the ship flies straight back
  into the hull it was just nudged out of.
- **A hyperspace entry is lines, not a shrinking ball** (`arrivals.ts::streak`). It was one sphere
  that got smaller, and at any real distance a shrinking sphere is a dot that dims: it carried no
  bearing, so an arrival announced that *something* was happening and nothing about where it came
  from. An entry is now a bundle of streaks running back along the entry vector and converging on
  the point the craft will occupy, **brightening** as they close — fading out and then producing a
  hull reads as two unrelated events. At `t = 1` every strand has collapsed to the arrival point,
  so there is no gap between the last frame of the entry and the first frame of the ship, and the
  core body **grows** into the arrival rather than shrinking out of it so both halves agree about
  where the thing is going to be. `perpBasis` seeds its cross product against `dir`'s *smallest*
  component: crossing with an axis the vector is nearly parallel to gives a near-zero result whose
  direction is floating-point noise, so the bundle would flip orientation between frames — but only
  for entries arriving along an axis, which is exactly the case nobody thinks to look at. A wing's
  ships are staggered by `WARP_STAGGER_MS`; one shared `dueMs` put three hulls on screen in the
  same frame, which reads as the sector gaining three ships rather than as a formation dropping
  out of hyperspace. Nothing here reads the record — an entry looks the same whatever was
  perceived.
- **A dogfight is a contest of turn rate against speed, not of hit points** (`classes.ts`,
  `enemy.ts`). Craft have a finite `turn`, fly where they point and cannot strafe; the five
  behaviours (`patrol`/`pursue`/`attack`/`overshoot`/`evade`) and the shot lead all follow from
  that one constraint. **No class may outrun the player** — disengaging must always be possible
  or the game punishes the exploring it is about — and a class roll must cover its whole range:
  it was derived from `durability` (six possible values), so both capitals existed in the table
  and could never be met, and nothing failed. Two tests now.
- **Shields absorb, hull decides.** Shields regenerate after a lull, hull only at a dock for
  salvage. Inverting that makes every fight a war of attrition against a clock. A hit reaching
  hull flashes and kicks harder than one a shield soaks, because that cue is the only thing
  telling a player whether they are making progress or wasting rounds on a buffer.
- **The jump drive's cost is on the decision, not the duration** (`hyper.ts`). Scarce fuel that
  only a *dock* refills, a spin-up, and an inhibitor that refuses to charge with hostiles in
  range — the last is what stops it being an escape hatch and therefore what makes committing to
  a fight mean anything.
- **Ships are line models and bolts are cylinders** (`meshes.ts`, `gl.ts`). A sphere has no
  visible facing and facing is the most important thing to know about an opponent; a sphere at
  half a sector per second is a dot that teleports. Glow is two additive passes with depth
  *writes* off, not a post-process. Stars are seeded from the commitment and never parallax — a
  star you could fly to would be an object the record never claimed. Draw distance is a constant
  covering the sector: it used to come from sensor range, which conflated what the record knows
  with what the window shows and made an unread world arrive as a *small* one.
- **A hit test must use the radius the renderer draws.** Reintroduced and paid for twice: craft
  started being sized by *class* while `weapons.ts` still sized them from a signal's `magnitude`,
  so a leviathan drawn 760M across had a 10M hitbox and the war classes were unbeatable by
  construction. Craft carry `radius` into the hit test now.
- **A capital is a volume you fly through, with a small solid core.** Treating the drawn sphere as
  a hull trapped the ship inside a hurtbox — re-collided every frame, charged every frame, pushed
  a quarter of a sector per frame. A ram is charged **on entry**, and the drive is never cut
  inside a capital or the ship is stranded where it most needs to leave. Flying inside the
  superstructure is also the tactic that beats one.
- **Three factions of traffic, and they are not scenery** (`factions.ts`). Couriers (neon blue) run
  between markets, freighters (blue) between depots, marshals (yellow) hunt raiders and ignore the
  player. Marshals kill raiders with nobody watching — a test asserts the raider count falls over
  two minutes of an idle player. Density is a **constant**; routes use the record's service nodes,
  which is a use of its contents rather than a reward derived from them. `nearestThreat` counts
  only factions hostile *to the player*, or a passing patrol inhibits the jump drive.
- **A refusal nobody reads is indistinguishable from a dead key.** Refuelling, jumping and the
  market were all reported broken and all three were refusing correctly in a notice that faded in
  three seconds — full tanks, no salvage, no waypoint. The fix is a permanent station panel with a
  real button per service that states *why* it is unavailable, a permanent jump readout naming the
  missing condition, and a market that explains an empty wallet. One real bug underneath: the home
  station was a *market*, so the first key a new player pressed said "core does not offer refuel".
- **Enlarging `EXTENT` does not spread a fractal out.** The trunk gets longer and the twigs stay as
  close together, so the sector gains empty margin and the part you fly through is unchanged.
  `MIN_NODE_GAP` — a floor between *any* two nodes, enforced with a spatial index during growth —
  is what actually did it; `MIN_BRANCH` only bounds a node against its own parent. Sharpened since:
  **`EXTENT` is a pure unit and changing it changes nothing anybody can see**, because every length
  in `scale.ts` is a fraction of it. `TRUNK` is the only lever that makes the sector bigger, and
  what a pilot reads as space is **the gap as a multiple of `R_NODE_MAX`**, not the gap. Measured at
  1.93× — two structures nearly touching — with the whole nearest-neighbour distribution pinned flat
  against the floor, because the fractal wants its tips far closer and every survivor lands at
  whatever the floor is. *The floor was the layout.* Now 7.8×, pinned as a ratio so raising a node's
  radius without raising the gap fails instead of quietly re-clustering the sector.
- **A far plane must cover the sector's DIAMETER, never its radius.** It was set to `1.9 · EXTENT`
  against a measurement of how far the fractal reaches *from the origin* (1.55), which looks like
  margin and is not — the camera is not at the origin, and what has to fit is the distance between
  the two furthest things on screen together. The sector was 2.36 extents across, so the constant
  fell 19% short and a player at one edge could not see the other: exactly the wall of fog the
  constant had been introduced to remove, surviving because the comment was checked against the
  radius and no test read the claim. Pinned now against the generated bounding box.
- **Anything scattered "through the volume" must be scattered through `SECTOR_REACH`, and that
  constant may never be measured off the generated tree.** Both halves are load-bearing. Raiders,
  the patrol and the capital garrison were placed in a ±1 `EXTENT` box, which covered the sector
  until `TRUNK` grew and then silently meant "the middle of the map" — the patrol sat in a small
  central cube while the raiders it exists to hunt spread over fifty times the volume, so the
  ambient war stopped and three checks failed at once. And the constant is derived from `TRUNK`
  rather than from the nodes a record happened to generate, because a record reporting a larger
  extent grows a larger tree, so scattering across the *measured* volume would sell a producer a
  thinner, safer sector for claiming a bigger world — the `raiders.ts` defect again, inverted.
- **`Rng.below(n)` is `next() % n` on a 32-bit xorshift, so any `n` past 2³² does nothing.**
  The draw is then just `next()` — uniform over a range with no relationship to the one asked
  for. `EXTENT` is 3.2e9, so the ceiling is 1.34 extents and *every* scatter in the game was over
  it. It shipped twice and produced plausible coordinates both times, which is why nothing failed:
  at ±1 extent it returned [-1.00, +0.34] (skewed, but near the origin, so the sector looked
  populated), and at ±5.89 it returned [-5.89, -4.55] — a thin shell with all three axes negative,
  the whole roster exiled to one corner with nothing inside sensor range of the spawn. Even
  "marshals fight raiders whether or not anyone is watching" still passed, because both
  populations were exiled to the *same* corner. **Draw small and scale up** (`fleet.ts::placement`
  is the pattern; `raiders.ts::scatter` is the shared helper). `Rng` itself must not be widened —
  `lib/omni/fractal.ts` shares it with the plate renderer, pinned byte-for-byte against Rust by
  `check:omni`, so a wider `below` changes every minted plate.
- **A correct uniform scatter still leaves the middle empty, and the middle is where the player
  starts.** A cube's volume grows as `r²`, so spreading a fixed roster evenly through ±5.89
  extents puts almost all of it far away. `raiders.ts::scatter` takes a `pull` exponent on the
  radius and a `floor`, and the raiders and the patrol must use the **same** reach, pull and
  floor — they are placed by different functions in different modules, which is exactly how they
  drifted apart before. The tuning is fitted to the old placement *replayed*, rather than to
  taste: nearest 0.46 extents against its 0.49, median 1.03 against 0.95, and nothing on the
  sensor board on the first frame, which is a property worth keeping — loading straight into a
  resolved contact reads as being ambushed by the loading screen.
- **A radius that scales an un-normalised cube point is not a distance, so a floor on it floors
  nothing.** A raw cube vector has length 0 to ~1.58, so a draw whose three components all land
  near zero lands near the origin however large the floor is — measured, a nearest raider at
  0.241 against a floor of 0.448, with eight inside sensor range at load-in. Normalise the
  direction by its **largest component** (the cube surface), not by its length (the sphere), or
  the corner-filling property that motivated a cube in the first place is lost.
- **A hunter's reach is a fraction of the sector, not a multiple of its own guns.** `MARSHAL_REACH`
  and `CAPITAL_REACH` were multiples of `aggro`, a weapons figure, which happened to cover the
  sector at one size and stopped the moment it changed. Floored at `SECTOR_REACH` now. A raider's
  reach is deliberately *not* floored: it is here for the player, and that asymmetry is what keeps
  the ambient violence from becoming the whole sector.
- **A role decides who shoots at you and what you are paid for** (`roles.ts`). Four: bounty
  hunter (hunts raiders), pirate (hunts marshals), trader and smuggler (paid for cargo, not
  kills; the smuggler is hunted by both factions). `hostileToPlayer` is the **single**
  implementation — `hostileTo` was `f === 'raider'` read in six places, and a pirate makes the
  answer depend on who is asking, so a seventh site that disagreed would be a craft that steers
  at you and never fires. The marshal war classes carry real bounties now, which looks like it
  contradicts "a payout for killing the good guys would be the game paying for the sector to be
  less policed" and does not: **you are paid only for what your role hunts**, so a bounty hunter
  still earns nothing for a marshal and a pirate nothing for a raider. The old rule survives as
  the stronger form of itself — a payout can never pull you toward work you did not declare.
- **A contract's terms come from the seed; only its subject may be a place** (`quests.ts`). The
  obvious quest is "destroy the six contacts this record reported" and it is the defect this
  project has shipped twice: attach a payout to `blind_spots` and you have paid somebody to hide
  them. So kind, count, tier and reward are a function of `(seed, faction, role, index)` and
  nothing else; a haul may name a node, because using the map is not the same as being paid by
  it. Progress advances on **acts** — a kill the swarm already reported, a dock the station panel
  already detected — never on a survey the player could satisfy by reading a panel. A faction
  that will not deal with your role says **why**, because an empty board with no explanation is
  indistinguishable from a broken one.
- **Faction citadels are placed by the seed and tiered by rings** (`generate.ts::CITADELS`,
  `meshes.ts::citadel`). Six per sector, one of each tier per faction, so all four roles find a
  board in every world — a roll would leave some sectors with no work for two of them. Rings are
  a **count**, not a diameter: a count reads correctly at any distance where a size only reads
  correctly next to something to compare against, the same reasoning as the coverage meter's
  cells and the fractal's severed limbs. The outer ring is radius 1 at every tier so the hit
  shell keeps agreeing with the picture.
- **Two traps the citadels sprang, both caught by existing checks.** Rolling their placement from
  the fractal's own `Rng` shifted every subsequent jitter and changed the shape of the tree —
  node count fell 424 → 374 because a decision about station *kinds* was consuming the fractal's
  randomness; it needs its own digest slice, like raiders and traffic. And the first label was
  `BASTION`, which is also the marshal titan's class label, so `drawList` bodies matched by label
  started returning a 0.028-extent station where a 0.4-extent ship was meant. **A node label must
  never collide with a class label.**
- **Earning has to survive the tab** (`wallet.ts`). `newGame` builds from `newShip()`, so salvage
  and SCEMA reset on every page load and every record dropped — which made the withdrawal path,
  whose minimum is ten SCEMA, unreachable for most players however long they flew. The account
  keeps what you *own* (both balances, the hull, the component levels) and deliberately rebuilds
  what is *state* (fuel, hull, shields, jump charges, the stripped-derelict list), or a reload
  becomes a way out of a fight and a scavenged-node list gets carried into a world where those ids
  mean something else. **One account, not one per world** — every world pays identically, so a
  balance is not a claim about any record, and keying by world would hand a player a reason to
  hoard the one they earned in. It is restored exactly once, onto the ship `newGame` just built,
  never patched onto a ship already flying: that would let a reload resurrect spent money. None of
  this makes a balance trustworthy and it does not pretend to — the bound is still the capped
  faucet in `claim.ts`, enforced server-side against a ledger the browser cannot reach.
- **Nodes are open wireframe structures you fly through, one silhouette per kind** (`meshes.ts`).
  They were shaded spheres, so a market and a rift were the same ball in different colours and the
  record's vocabulary arrived as a *palette* — the failure mode colour-only distinctions always
  have. The epistemic split moved with them: it was *solid vs permeable*, it is now **registers vs
  does not**. Flying through something the observer perceived puts it on the sensors; a phantom, a
  marker or a rift puts nothing there and the HUD says why. Both halves are visible now, which the
  earlier version got wrong — when only the absence produced a message there was nothing to
  contrast it with.
- **Docking range must comfortably exceed the largest node**, and did not: the origin's radius plus
  the ship's left a dockable shell two thousandths of a sector thick, crossed in a twentieth of a
  second, which the ship spawned outside. That plus a home station that was a *market* (trade and
  repair, no fuel) was the whole of the reported "refuelling does not work". A test pins the
  relationship rather than the number, and the origin is now its own kind offering all three.
- **A behaviour can be unreachable and no test notice.** `overshoot` keyed on *alignment* — the
  player being behind the nose — and a fighter turns fast enough that it always points at the
  player, so alignment inside the standoff band never dropped below 0.74 in a forty-second fight.
  Range rate is the physical fact. Same shape of bug as the capitals that could never be rolled.
- **Stars are projected, not pasted.** The shader took a view-space position as though it were
  clip space, so the field sheared and swam as the camera turned — and holding still is the entire
  reason stars are there. `check:scemaworld` asserts the uniform.
- **What is solid is exactly what was observed** (`collide.ts`). Station, dock, depot, market,
  origin and derelict register; **phantom, marker and rift do not**. Making a phantom solid is the
  game asserting something is there on the strength of a record that says nobody saw it; making
  it permeable is not the opposite claim, because the game simulates what was *observed* and
  there is nothing here to hit. The HUD says which, and that sentence is the mechanic. Four bugs
  came out of building this and none was visible by looking: the ship spawned inside the origin
  market (and then, once moved, pointed straight at it); avoidance returned a pure sidestep and
  orbited forever; every class's turn radius (`speed / turn`) exceeded its own standoff, so a
  gunship circled nineteen million units out for two and a half minutes; and a degenerate normal
  parked bodies at obstacle centres — which is where every record-signal craft starts, since
  contacts sit on nodes. Rules that survived: the physics radius **is** the drawn radius (one
  table); swept, never endpoint; geometry stops fire in **both** directions or the player learns
  hiding works only for the other side; impact cost is closing speed at the point of contact, so
  a graze is not a crash; and a resolved body ends a whisker *outside* the surface, or the
  collision system becomes flypaper.
- **A PNG written by `scema nft <record>` carries the record** in an `iTXt` chunk, so the image
  flies in Scema-World and verifies in `/omni` with no vault and no network. `iTXt` not `tEXt`
  (Latin-1 would corrupt a label lifted from an observed page), the **raw text** not a
  re-serialisation (`0.0` → `0` changes the canonical tag and the digest), and a **post-pass**
  not a render parameter, so every existing image stays byte-identical and the parity fixtures
  keep pinning the raster alone. An embedded record goes through the same verifier as a dropped
  file — the image is not a signature.

A test also asserts the model carries no price/yield field and that the commitment is written
down rather than merely true.

`npm run check:escrow` pins the money path (mint decoding against real mainnet fixtures,
pair legality, base-unit conversion, solvency verdicts). Run it after touching
`lib/escrow/mintinfo.ts` or `program.ts`.

`tsconfig.json` has `allowImportingTsExtensions` on so a module can import a sibling as
`"./x.ts"`. `lib/omni/verify.ts` imports `lib/omni/canonical.ts`, and `check:omni` runs both
directly under Node's native type-stripping loader, which resolves real paths and therefore
needs the extension. Safe because `noEmit` is on, and it only permits a form that used to be
an error.

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

## Architecture: Scematica Omni (`scematica-omni/`)

The agent runtime, in its own workspace. Loop: **observe -> hypothesise -> simulate ->
score -> decide -> record -> remember**. Every stage is a trait with a real implementation
and the whole pass is deterministic, which is the precondition for a decision record being
verifiable by somebody who was not there.

The organising idea is that **every layer can say "I don't know", and saying it costs
nothing** — an agent that cannot express ignorance expresses a number of the right shape
instead, and nothing downstream can tell it from a measurement. Six invariants carry that:

- **`Provenance` before value, `Term` before score.** Same mechanism as
  `scematica_mesh::cognition::Term`, same lesson: an unmeasured dimension takes the
  **neutral element** (0.0 additive here) and is flagged `measured: false`. Every aggregate
  carries a `Coverage` and no renderer may print the score without it — `render::cell` is
  the only thing that formats a `Term`, and an unmeasured one prints `—`, never `0.00`. A
  measured zero still prints `0.00`, because that is a real observation.
- **The utility equation is additive *because* of that.** `U = R − λ₁K − λ₂C − λ₃U + λ₄V`.
  A multiplicative form is more expressive and it is the trap this repo has paid for twice
  (the sentience Ψ pinned at 0; the agentic gate pinned shut on unbuilt subsystems). λ
  weights are a stated preference, never a fitted parameter, and they are hashed into every
  record.
- **A projection may not invent a number.** `StructuralSimulator` scores an expected gain
  **only** from signals the observer actually counted; a dangling or estimated citation
  yields `Term::absent`. The uncomfortable consequence is correct: on a barely-perceived
  world most branches project exactly zero and the agent abstains.
- **An instruction is not evidence.** The goal branch is grounded only by
  `Goal::grounded_in`, which the operator sets with `--ground`. An earlier version inferred
  it by keyword overlap and immediately grounded "add tests to the scema-cli crate" in a
  marker backlog in a *different* crate, because `scema` is a substring of every unit name
  here. Do not reintroduce inference.
- **`Applicability` lets a specialist decline, in two distinguishable ways.**
  `OutOfDomain` (permanent, fine) vs `Insufficient` (my domain, missing inputs — something
  the operator can go and supply). `scematica-nn` is wired in at `scema-policy::dqstar` as
  ONE evaluator and declines on every non-trading world; it also refuses a *partial* or
  *stale* `TradeState` rather than defaulting a missing feature to 0.0, which the net would
  read as a real observation of an empty pool. Specialist scores are **attached, never
  averaged** into the ranking (a utility and a normalised Q are not the same quantity); a
  qualified specialist's *measured* negative vetoes outright, and an *unmeasured* one is
  silence and carries no veto.
- **Unresolved counterfactuals are counted, never scored.** Every declined branch is
  remembered with what was projected for it; its realised outcome almost never exists
  because nobody ran it. `Calibration::mean_abs_error` is `None`, not `0.0`, when nothing
  resolved. Imputing outcomes for untaken branches would mean the system generating its own
  training signal. Same asymmetry as the bot's `calibration.rs`.

Abstention is a first-class outcome with five distinct reasons (`NoCandidates`,
`AllForbidden`, `NoPositiveUtility`, `TooLittleMeasured`, `Contested`), each of which is a
different instruction to the operator. `scema decide` exits **0** when it abstains — a
script that treats "the agent declined" as a crash gets rewritten to ignore the exit code,
and then it ignores real crashes too.

**Two correct behaviours used to read as malfunctions on a first run, and both are rendering
bugs rather than logic ones.** `scema simulate "<goal>"` with no `--ground` abstains, which
is right at every step and looked like the tool refusing or breaking — what was being asked
for was one flag nobody had heard of. And when a grounded signal branch outranks the
operator's own goal, the runtime chose something else and said nothing, which looks like
success and reads as the goal being ignored. `scema_policy::render::next_steps` now renders
each of the five abstention reasons as a *different* next command, lists the counted signal
ids for an ungrounded goal, and explains a goal that lost. It **suggests and never acts** —
filling in `--ground` because it looked plausible would be the keyword-overlap bug again with
a friendlier face. `scema quickstart` walks the loop narrated over a real directory and stops
before sealing, because a tutorial that writes a record on your behalf teaches the wrong
thing about the one command that leaves a trace.

**What `scema verify` proves**: the record was not edited after sealing, naming the field
that moved. **Not** that the world was as described (provenance carries that, which is why
the world state is committed whole — `Absent` arms and blind spots included), and **not**
that the record is the original (tamper-evident, not tamper-proof, until the root is
anchored somewhere the author does not control). Canonical encoding is stricter than JSON —
sorted keys, tagged types, normalised `-0.0` and NaN — because `serde_json` output is not
stable enough to hash. SHA-256, not the keccak-256 in `scema-bot-mesh`: nothing on an EVM
verifies these yet, and if one ever does that binding belongs on `mesh-core`'s keccak path.

**The action path exists now, and is gated twice.** `scema execute` carries out one
*declared* effect — never one inferred from a decision, because omni's branches describe
work ("11 markers in `scema-tools`") and turning one into an executable action automatically
would be the keyword-overlap bug with a disk behind it. `scema_tools::Workspace` answers
where, `scema-trust` answers whether, and `scema-effect` records what actually happened.
Dry run by default; `--commit` is a separate keystroke and seals an `EffectRecord`.
`Outcome::Unknown` is a first-class arm — an effect attempted whose result could not be
observed is neither success nor failure, and exits 3 so a sequence cannot continue past one
quietly. `delegate`, `discover` and `pay` are built, and each is narrower than its name. The
blocker on `pay` was real and is now `scema-spend`: caps, allow-lists and a sealed
`SpendRecord`. **None of the three moves money or opens a socket** — omni cannot link
`scematica-protocol` (solana-sdk), so they decide, record, and emit a settlement request for
something else to settle. That split is right anyway: the decision is pure and testable, and
the expensive half is a separate step somebody has to run.
An **absent policy permits nothing** — its own `NoPolicy` refusal, distinct from a breached
limit, because "you configured nothing" and "you are over budget" send an operator to
different places. Allow-lists are exact, never prefixes: `inference.` would authorise every
capability anybody later named under it. Amounts are `u128` **as decimal strings** on the
wire — JSON numbers are doubles past ~9e15, and `serde_json` refuses a bare `u128` on the way
back in, so a record sealed as an integer can be written and never re-read. `Settlement::
Unknown` exits 3 and does **not** consume budget: charging for a spend that may not have
happened lets a flaky counterparty drain an allowance, and calling it `Failed` invites a
retry that pays twice. A committed `pay` that is refused is still sealed — the pattern of
what an agent *wanted* to buy is exactly what a spend policy is for.
**The ledger tracks `committed = spent + reserved`, and `pay` writes a reservation.** An
earlier version wrote the ledger only at reconciliation, on the reasoning that only a settled
spend may consume budget — true, and it left the cumulative cap defeated anyway, because a
spend occupies its allowance from the moment it is authorised and not from whenever a receipt
turns up. With a total of ten, two spends of six were both authorised: the second measured
itself against a `remaining` the first had not yet had the chance to change. Nothing about
that is a race and no window has to be won, which is why the discipline that made it safe —
reconcile before authorising again — was not an invariant. `authorise` now measures against
`Ledger::committed()`; `pay --commit` seals first (the record id *is* the reservation key) and
then reserves. **Only an observed failure releases a reservation**: an unobserved outcome
keeps it, because the money may already have moved and a released hold is exactly what lets
the retry pay twice. `Ledger::settle` discharges the reservation as it charges, or one payment
is committed twice and the symptom is an allowance shrinking on its own. A duplicate id is
refused whether it is outstanding or already settled (`has_seen`, not `has_settled`), so a
replayed authorisation cannot take a second hold. The cost of never releasing on a guess is
that an unresolved spend holds budget forever — `Ledger::stale` is how an operator finds one,
and `pay` prints outstanding beside settled so the figure is never a bare "remaining".
`Ledger.settled_ids` makes double-charging **structurally impossible** rather
than guarded by a caller: reconciliation is exactly the operation somebody runs twice, and
the second run is a no-op. The spend record is **never edited** — reconciliation appends its
own sealed record, because the original saying UNKNOWN is the evidence that the gap existed.
A tampered spend record is refused rather than reconciled, or the edit gets laundered into a
fresh artefact. The settler boundary is `scema_spend::receipt` — a JSON contract, not a
trait, for the same reason the WorldState contract is: the two sides cannot link. It cannot
verify that money moved and does not pretend to; what it buys is that a settler cannot be
vague — a settlement without a reference is refused, and `Unknown` is not an accepted
outcome, because a settler that cannot tell should emit no receipt at all.
**Writing a settler: `scematica-omni/docs/SETTLERS.md`.** The contract is versioned
(`scema.receipt/1`) and carries shared conformance vectors at
`crates/scema-spend/vectors/receipts.json` — omni runs them in `tests/conformance.rs` and a
settler author runs the same file, so both sides check against ONE artefact rather than
each other's prose (three hand-written WorldState producers drifted exactly that way until
fixtures pinned them). One vector is `fabricated-but-well-formed`, **accepted on purpose**:
the contract checks shape and never truth, and anything reading a successful reconciliation
as proof of payment has misread it. `scema_spend::settler` supplies the seam and a
`ScriptedSettler` double, so the loop is testable before anybody writes code that can spend;
`Script::Silence` is the case to build against, because a caller that treats silence as
failure and retries **pays twice**. `answers()` is separate from `Receipt::validate` because
a settler working a queue can reply to the wrong item with a perfectly well-formed document.

**The world contract is versioned and its vocabularies are open** (0.5.0). `WorldState`
carries `schema: "scema.world/1"` and an undeclared version is refused on import — the
contract is JSON implemented in four languages and no compiler stands between a producer and
it, so without a version the next format change is a silent misread rather than an error.
The field is `Option` + `skip_serializing_if`, which is load-bearing: records sealed before
it existed must keep verifying, and a verifier that cries tamper on untouched history is the
one failure that teaches a reader to stop believing it.

`Domain` and `EntityKind` are **open** enums — known arms plus `Other(String)`, held
verbatim so a decision record round-trips byte for byte. Closing them was the largest limit
on universality: a perceived web page and a set of Chainlink feeds both reported `unknown`,
so two entirely different worlds were indistinguishable to every specialist. Parsing
normalises case and padding and deliberately does **not** guess synonyms (`k8s` != `kubernetes`);
the known lists are published via `scema check --vocabulary` so an author picks rather than
coins. An unfamiliar name is a **warning, never a failure** — failing on one would push
producers back onto `unknown`. `Domain::edit_reversibility` is the table that used to be a
`match` with a `_ => Unknown` arm in the hypothesiser (quietly wrong once domains were open);
it also corrects `Trading` from `Unknown` to **`Irreversible`**, the one domain here where
irreversibility is certain.

`scema_tools::conform` is the **single** implementation of "is this a usable world", used by
both `ImportObserver` and `scema check` — two would drift, and the worst available failure is
a producer that passes the checker and is then refused by the importer. It reports **every**
finding at once (the importer used to bail on the first, so an author with four problems
learned about them one release at a time) and each carries a stable `code`, so a producer's
tests assert on the code and not on wording that is meant to improve. See
`scematica-omni/docs/PRODUCERS.md`.

Observer rules: report what could not be read as `blind_spots`; never round an unread thing
to zero (`Provenance::Absent` carries no attributes); state whether the walk was complete
(`Extent { total: None }` when a cap was hit). **A deliberate exclusion is not a blind
spot** — skipping `target/` is a decision, not a failure, and filing it as ignorance buries
the paths that really could not be read. `RepoObserver` also strips the Windows
extended-length path prefix, because the locator becomes the memory subject key and every
signal target, and two spellings would split one repository into two subjects.

**Floats are hashed as fixed-point, and that is not a shortcut.** A commitment over raw
IEEE-754 bits is unverifiable the moment a record crosses JSON, which is the only way one
ever travels: `serde_json`'s formatter is exact but its *parser* is not correctly rounded
for every 17-significant-digit input, so `0.40066666666666667` comes back one ULP low from
its own identical text. A record sealed in the daemon and re-read over `GET` therefore
reported INVALID on a byte nobody had touched — and a verifier that cries tamper on an
honest round trip is worse than no verifier, because the first thing anyone does with one is
stop believing it. So `canonical.rs` encodes a float as `round(v * 1e9)` in `i64` and the
commitment binds values to **1e-9**: an edit at or above that resolution is caught, one
below it is not and cannot move any gate in `scema-policy`. Same wall `scema-bot-mesh` hit,
same conclusion — bit-exact float agreement between processes is engineered, not achieved by
care. Do not "simplify" this back to hashing bits.

**`scema_policy::render` is the only place in Rust a `Term` becomes a string**, for the same
reason `lib/mesh/view.ts::toneFor` is the only thing that picks a colour: a rule encoding a
claim about trust gets one implementation. The CLI and the MCP server both call it. The
extension HUD and `web/lib/omni/view.ts` are tested ports.

### Four kinds of world, one loop

"Domain-agnostic" is cheap to claim. What makes it true is that a repository, a running
Scematica system, a set of Chainlink feeds and a web page are all `WorldState`, and nothing
above perception can tell which it was looking at.

Only the first is a filesystem walk in Rust. The other three live behind a lockfile pinned
around `solana-sdk 1.18`, a stdlib-only Python package, and a browser — and linking any of
them would make `scema-tools` a hub of domain dependencies, which is exactly what the
workspace note forbids. So the arrangement is inverted: **the thing being observed describes
itself in `scema-world`'s vocabulary**, and `ImportObserver` reads that.

| Producer | Language | Emits |
|---|---|---|
| `scema_tools::RepoObserver` | Rust, in-process | a source tree |
| `plugins/scema-web/src/perceive.js` | JavaScript, no build step | a DOM |
| `scematica_mesh::omni` | Rust, the bot workspace | a running Scematica system |
| `alchem_link.omni` | Python, stdlib only | one network's oracle feeds |

Rules that hold on **every** producer, and are enforced twice:

- **An unreadable thing is a blind spot, never a zero.** An absent mesh node, an aggregator
  that did not answer, a cross-origin iframe. `scema-sim` turns a blind spot into *measured*
  uncertainty, which is the arithmetic behind the claim.
- **Stale is not fresh, and it keeps its value.** A veto from a stale mesh node is not
  counted as blocking; a feed past its heartbeat is `Provenance::Stale` with the age and the
  budget attached, not dropped and not presented as current.
- **Every signal is a count.** `measured: true` is a claim somebody counted something, and
  it is what lets `scema-sim` score a real expected gain. A "system health score" or an
  "oracle health score" invented in a producer would be a hallucination with a decimal point
  on it, laundered into a verifiable record.
- **A capped or unregistered read reports an unknown denominator.** `Extent { total: None }`
  rather than a numerator over a smaller total, which would claim over 100% coverage.

Enforced twice because three of the four producers are hand-written against a JSON shape.
Each **restates the importer's validation on its own side** and fails its own tests; and
`crates/scema-tools/fixtures/` holds **real captured output** from all three, asserted
against the importer. A self-check catches a bug in one producer; a fixture catches the case
where both sides were changed and only one of them was right. `alchem_link.omni._check`
caught a real extent bug the first time it ran.

`ImportObserver` rewrites `observer` to `imported:<name>` (idempotently), exactly as the
daemon rewrites a wire-supplied world to `client:<name>`. It validates the *shape*, never
the *claims* — a producer reporting a stale feed as `Live` is lying and no parser catches
that; the prefix is what tells a reader whose word this is.

### The surfaces

All of them drive the same `scema-agent` and none re-implements perception, simulation or
verification — which is why the safety argument is made once.

- **`scema-tui`** — the console. **Black and violet with soft-blue accents**, deliberately
  unlike every other TUI here (sniper: black/red; `mesh-dashboard`: indigo/slate;
  `sdk-dashboard`: green), because an operator with three open must be able to tell which
  is making a claim about money and which about a decision record. Three rules carry over
  from `alchem_link.theme` and `lib/mesh/view.ts`: **a renderer names a `Role`, never a
  colour** (`theme.rs` is the only file with a hex in it, `view.rs` the only one that maps a
  state to a role, `render.rs` only places rectangles); **colour is decoration, never the
  message** (a test walks every role in `Depth::Mono` and fails one that carries neither a
  modifier nor a distinguishing word, so `--no-color`, a pipe and a 16-colour terminal all
  produce the same text); and **azure is reserved for claims** — the chosen branch and a
  verifying commitment, never an observation. The coverage meter is one cell per term
  (`▰▰▰▱▱`), never a proportional bar, because a bar renders 2/5 and 4/10 identically and
  the denominator is the number that matters; an empty coverage is `∅`, never an empty
  meter. `enter` simulates and `D` decides, with a confirmation, because the two paths
  compute exactly the same thing and the only thing keeping a counterfactual from reading
  as a decision is that they are not the same keystroke. Grounding is **ticked, never
  inferred** — a test pins that a goal naming a signal id verbatim still does not ground it.
  `--snapshot WxH` draws a frame into a `TestBackend` and prints it, which is what makes a
  TUI testable at all; the tests assert on symbols, not styles.

- **`scema-omnid`** (`scema-daemon`) — hand-rolled HTTP/1.1 on `std`, no hyper/rustls/tokio.
  Partly for consistency, mostly because the moment omni carries a TLS stack somebody will
  path-depend it from the bot workspace and resurrect the zeroize/curve25519-dalek conflict.
  Four guards in order: **loopback bind that is deliberately not configurable** (the one
  thing that reliably happens to a `--bind` flag is somebody setting it to `0.0.0.0`),
  `Host` check → 421 (DNS rebinding is how a page becomes same-origin with a localhost
  service and gets to read its replies), constant-time 256-bit token → 401, `Workspace` →
  403. **No `Access-Control-Allow-Origin` is ever emitted and no `OPTIONS` handled**, so a
  page cannot read a reply even if it guesses a route; the extension is unaffected because a
  service-worker fetch under `host_permissions` is not subject to CORS, and that asymmetry
  is what lets the daemon refuse CORS outright. `POST /decide` is off until `--allow-decide`.
  `POST /simulate` builds its own non-persisting agent rather than flipping a flag on the
  shared `Arc<Agent>` — a shared mutable flag is a race whose failure mode is a simulation
  quietly sealing a record. A client-supplied `world` has its `observer` rewritten to
  `client:<name>` server-side, so a record can never claim a wire-supplied world was
  observed locally.
- **`plugins/scema-web`** — MV3, no build step, no dependencies, no bundler. **Perception is
  the only new part**: `src/perceive.js` emits the same JSON `RepoObserver` does, so
  `/simulate` cannot tell a DOM from a filesystem walk. It reads nothing until you ask —
  no `content_scripts` block, no `<all_urls>`, injection via `activeTab` from the popup
  button or `Alt+Shift+O`. **The token lives only in the service worker**, never in the
  content script, and the content script picks a message *type* which `background.js` maps
  to a path (same rule as `lib/scylar/tools.ts`: the caller names a tool, never a URL). The
  one parameterised route, `GET /decisions/{id}`, validates the id against a pattern
  **before** the path is built, so a `../` never reaches the daemon's router. The seal
  confirmation lives in the closed shadow root and is **not** `window.confirm` — a content
  script shares the page's window, so a page can define `confirm = () => true`. It
  deliberately **verifies nothing itself**: a fourth canonical-encoding implementation is
  one that will drift, and an overlay reporting an untampered record as INVALID teaches the
  reader to stop believing the verifier — so there is an export button, byte-identical to
  `RecordStore::save`, checked with `scema verify --file` or `/omni`. `src/theme.js` is a
  **port** of `crates/scema-tui/src/theme.rs` (Rust authoritative; `test/theme.test.js` pins
  the hexes) — before it existed the HUD was `#4c4cff` and the options page `#8b8bff`, and
  neither was the console's violet. Signals are **counts only**, never estimates;
  `mixed-content-subresources` is counted only on an https page (on an http page it is not
  "mixed", it is consistent, and the page-level signal already covers it), and
  `controls-without-labels` misses wrapping labels so it **over-counts** — the evidence
  string says so, because a number whose bias is undocumented cannot be calibrated against.
  Cross-origin iframes are genuinely unreadable and become blind spots → measured
  uncertainty. The entity locator drops the query string; `test/wire.test.js` pins that a
  `?sid=SECRET` never reaches a record. `npm test` is hermetic (44); the 9 wire tests skip
  unless `SCEMA_OMNID_URL` and `SCEMA_OMNID_TOKEN` are set.
- **`scema-cli`** — also the **launcher**: `scema tui|daemon|mcp` locate the sibling binary
  next to the running `scema` first and only then on `PATH` (PATH-first silently pairs a
  checkout's launcher with `~/.cargo/bin`'s old component). They stay separate crates so
  `cargo install scema-cli` on a CI box that only runs `scema verify` does not pull in a
  terminal stack. `scema doctor` **changes nothing** and reports four verdicts, not two:
  `ok`/`warn`/`FAIL`/`?`, where `?` is a check that could not be run — "does not verify" and
  "could not be read" are different claims and only one is an accusation. `scema connect`
  wires the MCP server into an assistant; it **merges** into `.mcp.json` rather than
  overwriting (people have three other servers configured) and refuses an unparseable file
  rather than replacing it. `--write` touches **project-local files only**; Claude Desktop /
  Windsurf / Zed / Codex configs are printed with their path and never written, because a
  user-level config is shared by every project and editing it means a tool installed for one
  repository quietly gained the ability to observe all of them.
- **`plugins/claude-code`** — a Claude Code plugin: the MCP server confined to
  `${CLAUDE_PROJECT_DIR}`, three commands, and a skill. **The skill is the point.** A config
  file cannot stop a model writing "expected gain: 0.00" when the tool said `—`, and the
  last layer of this whole design is prose a model writes. It is five things not to do, each
  a failure paid for here at least once: an em dash is not a zero; coverage never leaves the
  score it qualifies; abstention is an answer and *which* one is the actionable part;
  grounding is asserted, never inferred; a verified commitment proves one thing and not two
  others. No `--allow-decide` in its `.mcp.json`, so `omni_decide` is **absent** rather than
  listed-and-failing.
- **`scema-mcp`** — links the loop directly rather than proxying the daemon: same library,
  one less hop, no way for two surfaces to disagree. **stdout is the transport**; every
  diagnostic goes to stderr. Paths resolve through `Workspace` — not paranoia about a
  hostile model but because a *cooperative* one asked to audit a project will reason its way
  to `~/.ssh`, that being genuinely relevant to an audit. `omni_decide` is **not advertised
  at all** without `--allow-decide`, because a listed tool that always fails teaches a model
  to retry it. A refused path is a `tools/call` result with `isError`, never a JSON-RPC
  error — clients surface the latter as "the server broke", and a model told that stops
  trying, where one told "that path is outside the workspace, which is X" corrects itself.
  The `initialize` instructions state the two things a model otherwise gets wrong: an em
  dash is not a zero, and grounding is never inferred.
- **`/omni` in web/** — see the web-products section above.

`scema_tools::Workspace` is shared by the daemon and MCP and answers **where** only; whether
is `Goal`'s constraints and `scema-trust`'s approval policy. Merging the two is how a grant
for one silently becomes a grant for the other. It resolves fully (symlinks followed, `..`
collapsed) and *then* compares against roots — a string scan for `..` passes a symlink
pointing at `/`. It also refuses `PROTECTED_PATTERNS` **by name, after canonicalisation and
before the root test**, so a symlink cannot launder a protected target and the refusal reads
as "this is a secret" rather than "widen your roots". That closes a real gap: `RepoObserver`
reads file contents to count tests and markers, and a count derived from a private key is
still a read of one. The list is checked against Python's by
`cargo test -p scema-tools --test protected_vectors`. Still point `--allow` at a project
rather than a home directory — a name-based refusal is a backstop, not a boundary.

State lives in `.scema/` under the working directory (`decisions/<id>.json`,
`memory/*.jsonl`), gitignored — machine-local and full of absolute paths.

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
| `scematica-nn-veto.json` | sniper | sniper | DQ* consecutive-veto streak + the `train_steps` it belongs to. Persisted because the streak backstop needs 12 vetoes and the process restarts more often than it sees 12 buy-ready pools — in-memory it had never once fired. A checkpoint whose `train_steps` went backwards is a different agent and resets the streak. |
| `scematica-deployer-reputation.json` | reputation ledger | filters | Per-deployer rug/success EMA |
| `scematica-strategy.json` | AI strategy agent | sniper (live_params) | TP/SL/multiplier/regime |
| `scematica-rate-mode.json` | dashboard | sniper (live_params) | Active rate mode + TP/SL |
| `scematica-sell-mode.json` | dashboard / drawdown guard | sniper | Pauses buys, sells positions |
| `scematica-dump-mode.json` | dashboard | sniper | Force-sell with `min_out = 0` |
| `scematica-pool-decisions.jsonl` also now carries `decide_latency_ms` | sniper | `measure` | Milliseconds from the listener seeing a pool to the decision being written — the span the Arrive phase is about, and the one nothing measured. `Option` + `skip_serializing_if`: a build that did not record it and a pipeline that decided in zero milliseconds are different facts, and zero is the most flattering possible reading of the thing under investigation. Marked in `latency.rs` via a bounded side table rather than threaded through `write_pool_decision`'s twenty-six call sites — a large diff in a live buy path is how a trading bug gets introduced by a diagnostic. |
| `scematica-coherence.jsonl` | sniper | `measure` | Coherence sample every 30s: resolved/unresolved RPC-bound checks, Ψ, and whether the breaker had enough samples to judge. Sampled on a timer rather than per pool because the breaker keeps a rolling window, not a monotonic counter — so a per-pool delta is unsafe, and threading a per-evaluation context through the buy path is not worth it for a diagnostic. `measure` therefore reports resolution *around* a decision, never *for* it. |
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
- **Unmeasured features take the neutral element, not zero** (`state.rs`). `to_vec()`
  normalises into [0,1] and hands the result to a net that cannot ask a follow-up question,
  so a field nobody measured arrives as a number and that number is a claim. It was an
  actively misleading one: `pool_age_secs` is non-zero in **0 of 8,422** decisions
  (`pool.open_time` is essentially never populated) and `0.0` normalises to the *bottom* of
  its range — a pool zero seconds old, the most bullish value it can take. The entry builder
  also asserted `lp_burned: true` and `mint_renounced: true`, the two safest readings of the
  two strongest safety signals, and `trade_state_from_event` — which builds the states the
  agent *trains* on — filled nine fields with constants. `FeatureMask` marks what was not
  measured and `to_vec` substitutes `NEUTRAL[i]`; `coverage()` says how much was real and
  rides in `TradeDecisionExplanation` beside `confidence`, because five finite Q-values with
  a clear argmax look identical whether the input was measured or invented. The neutral
  table is **not** a blanket 0.5 and that is the trap: `price_change_pct` is
  `clamp(-1,3)/3`, so 0% sits at 0.0 and the midpoint is **+150%**; `buy_sell_ratio` is
  `/5.0`, so a balanced book is 0.2. A uniform midpoint would replace "I do not know" with
  "strongly bullish" on exactly the two features where that costs most. `FeatureMask`
  defaults to *nothing marked*, which preserves every pre-existing caller and every state in
  a saved replay buffer — the weaker half of the design, made checkable by recording
  coverage rather than trusting silence.
- **Calibration** (`calibration.rs`, printed by `measure --dq`): the agent scored against
  the advice it recorded, using the trades that settled. Three rules, and the first is the
  one that makes the module honest. **A veto has no outcome and never will** — nobody bought
  the pool, so `mean_abs_error` is `None`, never `0.0`; a zero there reads as perfect
  calibration achieved by refusing to act, and a policy would improve its score every time it
  got more conservative. **The join is by mint, forward in time, and skips vetoes**, or a mint
  vetoed here and bought a week later hands the veto a result it had no part in. And **a
  score a constant policy earns is the base rate, not skill** — the first real run found
  `SELL_PARTIAL` on 399 of 399 pieces of advice and a mean absolute error of 0.0000 on the 16
  that resolved, which is exactly what "always say bearish" earns in a losing window. So
  `Verdict` names the *cause* rather than returning a number: `NoAdvice` / `ActionNeverVaried`
  / `AllUnresolved` / `Scored`, each a different instruction to the operator, and
  `score_is_base_rate` puts the caveat on the same line as the figure. That finding is the
  third time this repository has hit one failure — a gate reading a filter input that never
  varies, a Ψ term pinned at zero, an argmax that never moves — so the histogram is printed
  first and the check is standing rather than a one-off. Enforcement is tri-state: the sniper
  logs advice identically whether or not it is acting on it, and only a rejection at stage
  `dq_advice` proves the veto fired, so everything else is `enforcement unknown` rather than
  credited to the agent.
- **Tournament**: 3 variants (conservative/balanced/aggressive) run in parallel. Promotion is on **recent mean reward** over a 200-transition window, requiring a 10% margin, every 1000 steps. It was `total_reward` — a lifetime sum that is never reset — so a variant that was better in its first thousand steps kept the primary slot forever however badly it was doing now: a comparison that cannot change its mind. A variant below `RECENT_MIN` (40) reports `None`, not `0.0`, and is **absent** from the comparison rather than entering it at zero, which would rank it below every losing variant on the strength of having done nothing. The margin is relative to the incumbent's `abs()` because rewards go negative — computed on the signed value it inverts exactly when the agent is losing money, and losing less must still promote. The window is deliberately **not** serialised into the checkpoint: it is a claim about *recent* behaviour, and restoring one written days ago would promote on performance the agent is not currently delivering.

## Risk Subsystems

Independent breakers in `crates/scematica-sniper/src/`. Each can be toggled in `config.toml`; a single trip pauses buys. When adding a new breaker, follow this pattern: dedicated module exposing `should_halt(&state) -> Option<reason>`, hooked into the buy gate in `sniper.rs`.

- `ath_tracker.rs` — pauses buys when wallet drops `ath_drawdown_pct` below session ATH
- `grief_breaker.rs` — 5-min sliding-window cumulative loss
- `kelly.rs` — fractional Kelly position sizing from rolling win-rate (lookback in config)
- `pool_scorer.rs` — 0-100 predictive score from pool age + quote vault; rejects below `min_pool_score`
- `reputation.rs` — EMA-blended deployer rug history; rejects deployers > `max_deployer_rugs_24h`
- `multi_rpc.rs` — latency-ranked round-robin failover across `extra_rpc_endpoints`
- `coherence.rs` — **epistemic** breaker: halts buys when the filter pipeline is passing
  pools it could not verify. Every other breaker here fires on money, and therefore
  after the damage; this one fires on the condition that precedes it. RPC-bound filters
  fail open on timeout, so a degraded node turns the pipeline into a pass-through that
  still reports "passed" — past some fraction of unresolved checks the safety checks the
  operator believes are running are silently not running. Instrumented in the two shared
  RPC retry helpers in `filters.rs`, not at each fail-open site, so a new filter is
  counted by construction. Process-global because the PID lockfile already guarantees
  one sniper per machine. Ψ comes from `scematica-sentience`, the same equation as
  `/api/sentience` — one definition, not two. **Buys only**: a degraded feed must never
  stop you closing existing risk. Needs `MIN_SAMPLES` before it can trip, so it cannot
  fire at startup when it knows least. `coherence_breaker` in config, default **on**
  (via `default_true()` — `#[serde(default)]` yields `false` for a missing bool, which
  would silently disable a safety feature for every existing config.toml).

## Platform Notes

- Primary dev environment is **Windows + PowerShell**. Code paths handle Windows specifics: `tasklist` for process liveness (sniper `main.rs`), `NotifyIcon` for desktop toasts (avoid WinRT — use `System.Windows.Forms.NotifyIcon` with stderr nulled to keep log panel clean).
- WSL UNC keypath paths are supported in `[wallet] keypair_path` (`\\wsl$\Ubuntu\home\...`).
- The sniper writes a PID lockfile (`scematica-sniper.lock`) and refuses to start if a live process is already running — two snipers on the same Helius WebSocket rate-limit each other into uselessness.
- Release profile is heavy: `lto = "fat"`, `codegen-units = 1`, `panic = "abort"`, `overflow-checks = true`. First release build is slow; `target/` reaches 5-10 GB fresh but **accretes without bound** across incremental debug+release builds — it was measured at 43 GB / 79k files before a clean. Run `cargo clean` when free space gets tight: a full disk surfaces as Windows `error 112` inside unrelated crates ("failed to write file... incremental"), which reads like a compile failure and is not one.
- The repo lives under **OneDrive**. `.gitignore` covers `target/`, `*.log` and `*.apk`, but OneDrive does not read `.gitignore` — exclude `target/` in OneDrive settings or it will try to sync tens of GB of build artifacts.

## Token Gate

Sniper and dashboard both enforce a 250k SCEMA balance check at startup with up to 5 retries. Set `SCEMATICA_SKIP_GATE=1` only during RPC outages — it bypasses the check entirely. SCEMA is a Token-2022 mint; gate code must use Token-2022 helpers, not legacy SPL Token.
