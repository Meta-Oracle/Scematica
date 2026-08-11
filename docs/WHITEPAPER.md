# SCEMATICA: Autonomous AI Trading Infrastructure for Solana

### Technical Whitepaper — workspace v1.25.0 · verified against source 2026-08-11

**Contract Address:** `HcsHqEJ9suf4oHJ8mb52M7AVKjhYhnTaeHgTmde7pump`  
**Token:** $SCEMA (Token-2022, Solana Mainnet)  
**Gate:** 250,000 SCEMA required to operate

---

## Abstract

Scematica is a full-stack autonomous trading infrastructure for the Solana blockchain, combining high-frequency token sniping, cross-DEX arbitrage, reinforcement learning via a Dueling Deep Q* agent, multi-agent AI strategy analysis, and a real-time terminal dashboard — all implemented in pure Rust. The system targets Raydium AMM V4 new-pool events and executes decisions in sub-second latency from pool detection to signed transaction. Access is gated behind a 250,000 $SCEMA token balance, aligning operator incentives with the protocol's token economics.

A second theme runs through the later sections and is, by this version, as load-bearing as the trading logic: **the system is built to know when it does not know.** RPC-bound filters fail open by necessity, which means a degraded node can silently turn the filter pipeline into a pass-through that still reports success. The coherence breaker (§12.7) and the Ψ cognitive gate (§22) detect that condition and halt on it; the counterfactual replay endpoint (§23) refuses to price outcomes it has no evidence for; the web layer (§24) labels every simulated value rather than letting it render as a live result; and the BOT Chain port (§25) was measured and then deliberately stopped short of a sniper because the chain produced two pool creations in eight days. A number nobody can check is treated throughout as worse than no number.

This document provides a comprehensive technical description of the architecture, algorithms, risk management framework, and the mathematical foundations underlying Scematica's profit-maximization strategies.

---

## Table of Contents

1. [Introduction and Motivation](#1-introduction-and-motivation)
2. [System Architecture Overview](#2-system-architecture-overview)
3. [Token Gate and Access Control](#3-token-gate-and-access-control)
4. [Sniper Pipeline](#4-sniper-pipeline)
5. [Multi-Source Pool Listening](#5-multi-source-pool-listening)
6. [Filter Pipeline](#6-filter-pipeline)
7. [Transaction Execution Engine](#7-transaction-execution-engine)
8. [Exit Strategy Architecture](#8-exit-strategy-architecture)
9. [Adaptive Pullback Mathematics](#9-adaptive-pullback-mathematics)
10. [Momentum Escalation System](#10-momentum-escalation-system)
11. [Builder Mode Compounding Algorithms](#11-builder-mode-compounding-algorithms)
12. [Risk Management Framework](#12-risk-management-framework)
13. [Cross-DEX Arbitrage Engine](#13-cross-dex-arbitrage-engine)
14. [Deep Q* Reinforcement Learning Agent](#14-deep-q-reinforcement-learning-agent)
15. [Multi-Agent AI Strategy System](#15-multi-agent-ai-strategy-system)
16. [x402 Payment Protocol](#16-x402-payment-protocol)
17. [File-Based IPC Architecture](#17-file-based-ipc-architecture)
18. [TUI Dashboard](#18-tui-dashboard)
19. [Performance Characteristics](#19-performance-characteristics)
20. [Security Model](#20-security-model)
21. [Token Economics](#21-token-economics)
22. [Cognitive Architecture and the Ψ Gate](#22-cognitive-architecture-and-the-ψ-gate)
23. [Counterfactual Replay and Calibration](#23-counterfactual-replay-and-calibration)
24. [Web Interfaces and the Product Surface](#24-web-interfaces-and-the-product-surface)
25. [Cross-Chain Expansion: BOT Chain and the Neural Mesh](#25-cross-chain-expansion-bot-chain-and-the-neural-mesh)
26. [Roadmap](#26-roadmap)

---

## 1. Introduction and Motivation

### 1.1 The New-Pool Trading Opportunity

Solana's throughput (~65,000 TPS theoretical, ~3,000–4,000 TPS sustained) enables a category of trading activity impossible on slower chains: new liquidity pool sniping. When a token creator initializes a Raydium AMM V4 pool, the pool creation transaction is observable on-chain within milliseconds via WebSocket subscription to the Solana program log stream. The window between pool creation and broad market discovery is typically 1–30 seconds, during which early buyers acquire tokens at the initial price before organic demand pushes prices higher.

The statistical reality of this market is well-characterized by a power-law distribution: most new tokens rug-pull within minutes (price → 0), a small percentage experience modest gains (50–200%), and a tiny fraction become multi-bag plays (500–10,000%+). The edge in this market is not in picking winners ex ante — that is extremely difficult — but in:

1. **Entering fast enough** to get the lowest slippage on the initial price
2. **Filtering effectively** to eliminate known rug patterns before entry
3. **Holding winners long enough** to capture the full price appreciation
4. **Exiting losers quickly** to minimize drawdown from the inevitable rugs

Scematica is purpose-built to optimize all four of these dimensions simultaneously.

### 1.2 Why Rust

Rust was chosen as the implementation language for performance-critical reasons:

- **Zero-cost abstractions**: The release profile uses LTO (fat), single codegen unit, and `panic = "abort"`, producing binary-optimized executable with no garbage collection pauses
- **Async runtime**: `tokio` provides deterministic, low-latency async I/O for WebSocket listeners and concurrent buy/sell monitoring without OS thread overhead
- **Memory safety without GC**: No garbage collection pauses that could delay a buy transaction during a collection cycle
- **Type safety**: The compiler enforces data race freedom at compile time, critical for the concurrent position registry accessed by multiple async tasks

### 1.3 Why Pure-Rust ML

The DQ* reinforcement learning agent (`scematica-nn`) is implemented from scratch in safe Rust with no external ML framework dependencies. This was a deliberate choice:

- **Build determinism**: PyTorch, TensorFlow, and ONNX each pull large native dependency trees that create version conflicts with Solana SDK's transitive dependencies
- **Latency predictability**: Python-FFI calls introduce unpredictable latency from interpreter overhead; pure Rust ML runs inference in microseconds
- **Binary portability**: The agent is embedded in the sniper binary, not a separate Python process, eliminating IPC latency from the decision path

---

## 2. System Architecture Overview

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                           Scematica Runtime                                 │
│                                                                             │
│  ┌─────────────────┐    file IPC     ┌──────────────────────────────────┐  │
│  │   scematica-    │◄───────────────►│       scematica-sniper           │  │
│  │   dashboard     │                 │                                  │  │
│  │                 │  .json/.jsonl   │  ┌──────────┐ ┌──────────────┐  │  │
│  │  6-tab TUI      │                 │  │ Listener │ │ Filter Chain │  │  │
│  │  ratatui        │                 │  │ Raydium  │ │ 15+ filters  │  │  │
│  │                 │                 │  │ PumpFun  │ │              │  │  │
│  │  AI Chat        │                 │  │ Whale    │ │ Pool Scorer  │  │  │
│  │  Config         │                 │  └────┬─────┘ └──────┬───────┘  │  │
│  │  Logs           │                 │       │               │          │  │
│  │  Trades         │                 │  ┌────▼───────────────▼──────┐  │  │
│  │  Radar          │                 │  │       Sniper Core         │  │  │
│  │  Overview       │                 │  │  Buy → Monitor → Sell     │  │  │
│  └─────────────────┘                 │  │  Momentum Escalation      │  │  │
│                                      │  │  Adaptive Pullback        │  │  │
│  ┌─────────────────┐                 │  │  Velocity Decay           │  │  │
│  │  scematica-nn   │                 │  │  Tiered Partial TP        │  │  │
│  │                 │◄───────────────►│  └─────────────┬─────────────┘  │  │
│  │  Dueling DQN    │  trades.jsonl   │                 │               │  │
│  │  Double DQN     │                 │  ┌──────────────▼──────────┐   │  │
│  │  N-step PER     │                 │  │   scematica-executor    │   │  │
│  │  Tournament     │                 │  │  Raydium swap builder   │   │  │
│  └─────────────────┘                 │  │  WSOL ATA lifecycle     │   │  │
│                                      │  │  Dynamic fee escalation │   │  │
│  ┌─────────────────┐                 │  └─────────────────────────┘   │  │
│  │  scematica-arb  │                 └──────────────────────────────────┘  │
│  │                 │                                                        │
│  │  Graph search   │  ┌──────────────────────────────────────────────────┐ │
│  │  Raydium/Orca/  │  │             scematica-protocol                   │ │
│  │  Meteora        │  │  x402 HTTP/402 payment server — monetizes APIs   │ │
│  └─────────────────┘  └──────────────────────────────────────────────────┘ │
└─────────────────────────────────────────────────────────────────────────────┘
```

### 2.1 Workspace Crates

| Crate | Role |
|-------|------|
| `scematica-core` | Shared types, config, RPC, wallet, metrics, token utilities |
| `scematica-sniper` | Pool listener, filter pipeline, sniper logic, backtester |
| `scematica-arb` | Cross-DEX arbitrage graph search (Raydium/Orca/Meteora) |
| `scematica-executor` | Multi-DEX swap instruction builders, Jupiter integration |
| `scematica-ai` | LLM agents (Anthropic/Groq/OpenRouter/Cerebras): Chat, Strategy, Risk, Debate, Report |
| `scematica-nn` | Pure-Rust Dueling Deep Q* agent — no external ML dependencies |
| `scematica-sentience` | Singularity Cognitive Architecture as a computable library: Ψ/Ω master equations, ethics gating, knowledge graph, meta-cognition, LLM overlay. Library only — no binary |
| `scematica-dashboard` | Ratatui TUI — 6 tabs, real-time metrics, config, AI chat |
| `scematica-api` | HTTP API backing the `web/` Next.js dashboard; also hosts the Ψ gate, counterfactual replay and calibration endpoints |
| `scematica-protocol` | Rust-native x402 HTTP/402 payment protocol server (facilitator) |
| `scemadex-sdk` | Published agentic-liquidity SDK: intents, Conviction-Routing bonds, inference/experience mesh (no `solana-sdk` by default) |
| `scemadex-settle` | Open devnet reference settler — moves devnet USDC on bond slash |
| `scemadex-integrations` | Bot-side ScemaDEX wiring: x402 bond engine, Jupiter route policy, file signal source (`publish = false`) |
| `scemadex-relay` | Peer-mesh + signal-oracle HTTP server (`scemadex-relay` bin) |
| `scemadex-mcp` | MCP server bridging LLM agents to the ScemaDEX rail over the relay |
| `sdk-dashboard` | ScemaDEX SDK TUI over the bond pipeline (`sdk-dashboard` bin) |
| `scematica-suite` | Umbrella meta-crate: re-exports every component + the `scematica` launcher |

**Trees outside the cargo workspace.** Three components version and build independently:

| Tree | Why it is separate |
|------|--------------------|
| `alchem-link/` | Python, stdlib-only. An Alchemy × Chainlink oracle-safety toolkit — a second product, not a bot module |
| `scema-botchain/` | EVM (chain 677) port. Its own workspace **by necessity**: an EVM stack needs `reqwest 0.12`/`rustls 0.23`, exactly what cannot coexist with `solana-sdk`'s `curve25519-dalek 3` (Appendix B). Two lockfiles make the conflict moot; one resurrects it |
| `scema-bot-mesh/` | Verifiable neural inference for BOT Chain. Separate for the same lockfile reason, plus deliberate dependency minimalism — its inference path is a spec others reimplement |

---

## 3. Token Gate and Access Control

Access to both the sniper and dashboard is gated behind a 250,000 $SCEMA balance check executed at startup. The gate uses up to 5 RPC retries to handle transient node failures and checks balance via Token-2022 helpers (not legacy SPL Token, as SCEMA uses the newer token standard).

```rust
// Pseudocode — actual implementation in scematica-core/token.rs
async fn check_scema_gate(rpc: &RpcClient, wallet: &Pubkey) -> Result<()> {
    for attempt in 0..5 {
        let balance = get_token_2022_balance(rpc, wallet, SCEMATICA_MINT).await?;
        if balance >= MIN_SCEMA_REQUIRED {
            return Ok(());
        }
        sleep(Duration::from_secs(2u64.pow(attempt))).await; // exponential backoff
    }
    Err(anyhow!("Insufficient SCEMA balance: {} < {}", balance, MIN_SCEMA_REQUIRED))
}
```

The gate can be bypassed with `SCEMATICA_SKIP_GATE=1` environment variable, intended only for RPC outages. Bypassing in normal operation violates the token terms.

---

## 4. Sniper Pipeline

### 4.1 High-Level Flow

```
WebSocket event
     │
     ▼
ListenerEvent::NewPool { base_mint, pool_id, initial_liquidity, ... }
     │
     ▼
[Dedup check] — recently_bought within 5 min? → SKIP
     │
     ▼
[Filter pipeline] — 15+ parallel filter checks (3s timeout each)
     │
     ▼                         ─── fail path → FilterRejection logged
[Pool scorer] — 0–100 quality score
     │
     ▼                         ─── score < threshold → SKIP
[Buy gate] — sell mode active? drawdown guard tripped? Kelly halts?
     │
     ▼
[Executor] — build Raydium swap tx, sign, submit with priority fees
     │
     ▼
[Confirmation] — wait for confirmed status, extract received amount
     │
     ▼
[Sell monitor spawned] — Phase 1: 20 × 100ms rapid checks
                         Phase 2: price_check_interval_ms (default 500ms)
```

### 4.2 The Buy Lock

A `DashMap`-backed concurrent position registry replaces the original single-slot lock that previously serialized all buys. The lock architecture:

1. **Buy-phase lock** (`Mutex<()>`): 2-second exclusive window while a buy transaction is in flight. Prevents two buys from racing on the same WSOL ATA simultaneously.
2. **Sell semaphore** (`Semaphore(5)`): Allows up to 5 concurrent sell transactions. With many positions hitting stop-loss simultaneously, exits run 5-at-a-time.
3. **Position registry** (`DashMap<Pubkey, LivePositionSnapshot>`): All sell monitors operate independently on their own entry, with no shared locking between positions.

This architecture enables unlimited concurrent positions — the bot buys into every qualifying pool without a positional cap.

---

## 5. Multi-Source Pool Listening

Three independent listener sources merge into a single `ListenerEvent::NewPool` stream:

### 5.1 Raydium AMM V4 Listener (`listener.rs`)

Subscribes to Solana program log stream via WebSocket, filtering for `InitializeInstruction2` log events from the Raydium AMM V4 program (`675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8`). Each matched log is decoded to extract:
- Pool ID (AMM account)
- Base mint (token being traded)
- Quote mint (WSOL or USDC)
- Open time (Unix timestamp when buys are enabled)
- Initial liquidity

### 5.2 PumpFun Listener (`pumpfun.rs`)

Monitors PumpFun's bonding curve program for graduation events — tokens that have completed their bonding curve and are migrating to Raydium AMM. These represent validated tokens with proven market interest and a pre-formed holder base.

### 5.3 Whale Copy Trading (`whale_copy.rs`)

Monitors a configurable set of high-performing wallet addresses for buy transactions. When a watched wallet initiates a buy, the sniper evaluates the same token through its filter pipeline and potentially mirrors the trade. Copy trading provides an orthogonal signal source to pool-creation events.

### 5.4 Stream Merging

All three sources produce `ListenerEvent::NewPool` events that are sent through a shared `mpsc::channel`. The downstream filter pipeline is source-agnostic — it processes events identically regardless of origin. This clean separation means new listener sources can be added without touching the filter or execution logic.

---

## 6. Filter Pipeline

### 6.1 Filter Trait

```rust
#[async_trait]
pub trait PoolFilter: Send + Sync {
    async fn check(&self, pool: &CachedPool, rpc: &RpcClient) -> FilterResult;
    fn name(&self) -> &'static str;
}
```

Each filter is independent, stateless (per-call), and timeout-wrapped at 3 seconds (`RPC_CALL_TIMEOUT_SECS`). Timeouts prefer fail-open — a slow node that fails a filter check skips the filter for that pool rather than blocking the entire pipeline. This ensures one degraded RPC node cannot stall all pool processing.

### 6.2 Active Filters

| Filter | What it checks | Fail mode |
|--------|----------------|-----------|
| `LiquidityFilter` | Minimum SOL in quote vault at creation | Hard reject |
| `LpBurnFilter` | LP token burn ratio ≥ threshold | Configurable |
| `MintRenounceFilter` | Mint authority removed from token | Configurable |
| `PoolAgeFilter` | Pool must be younger than `max_pool_age_secs` | Hard reject |
| `VolumeFilter` | Minimum transaction volume in first N seconds | Hard reject |
| `BuySellRatioFilter` | Buy transaction ratio above threshold | Soft reject |
| `HoneypotFilter` | Simulates sell transaction to check if sell is blocked | Hard reject |
| `DeployerReputation` | EMA deployer rug rate from reputation ledger | Hard reject |
| `PoolScorer` | Composite quality score 0–100 | Score < min → reject |
| `TokenMetadataFilter` | Checks for scam keywords in token name/symbol | Hard reject |
| `SocialFilter` | Validates social links (Twitter, Telegram) exist | Configurable |
| `TopHolderFilter` | Rejects tokens with >X% held by any single wallet | Hard reject |
| `FreezeAuthorityFilter` | Rejects tokens with active freeze authority | Hard reject |
| `BlacklistFilter` | Checks deployer against known-bad address list | Hard reject |

### 6.3 Filter Stats IPC

Every filter maintains a rejection counter published atomically to `scematica-filter-stats.json` after each pool evaluation. The dashboard displays per-filter rejection rates in real time, letting operators identify which filters are triggering most often and tune thresholds accordingly.

### 6.4 Deployer Reputation Ledger

The deployer reputation system maintains an Exponential Moving Average of rug-pull history per deployer wallet:

```
rug_ema_new = α × is_rug + (1 - α) × rug_ema_old
```

Where α = 0.3 (each new trade has 30% weight on the EMA). A deployer with `rug_ema > max_deployer_rugs_24h` is rejected at the filter stage. The EMA property means:
- A single rug doesn't permanently blacklist a deployer
- But a deployer who rugs consistently converges toward 1.0 and stays blocked
- A deployer who starts legitimate after a rug history can recover over time

---

## 7. Transaction Execution Engine

### 7.1 Raydium AMM V4 Swap Building

The executor (`scematica-executor`) constructs Raydium AMM V4 swap instructions natively, without relying on Jupiter routing for standard buys. This eliminates one round-trip latency (~50–200ms) compared to fetching a quote from Jupiter's API. The swap instruction set:

1. Create/verify WSOL ATA if not exists
2. Sync native SOL → WSOL (wrapping)
3. Build AMM swap instruction (input_amount, min_output from slippage calc)
4. Close WSOL ATA after swap (recover rent, unwrap remaining WSOL)

### 7.2 WSOL ATA Lifecycle

The Wrapped SOL (WSOL) account lifecycle is the most failure-prone aspect of Raydium swaps. The executor's WSOL lifecycle:

```
SOL in wallet
    │
    ▼ create_associated_token_account (idempotent)
WSOL ATA (empty)
    │
    ▼ sync_native (transfer SOL to ATA)
WSOL ATA (funded: buy_amount SOL)
    │
    ▼ amm_swap (exchange WSOL for token)
Token in wallet + WSOL ATA (any unswapped WSOL residual)
    │
    ▼ close_account (recover rent + residual SOL)
SOL in wallet (reclaimed)
```

The ATA close is always included in the buy transaction — even if the swap fails, the close instruction returns the ATA rent (~0.002 SOL) to the wallet.

### 7.3 Dynamic Priority Fee Escalation

Priority fees are computed per-transaction using a combination of:
- Network congestion estimate from recent block fee percentiles
- A per-pool escalation counter that increases fees on successive retry attempts
- Hard cap to prevent runaway fee spending

The fee schedule follows an exponential backoff: each retry multiplies the priority fee by 1.5×, from a base of 1,000 microlamports up to a configured maximum (default: 100,000 microlamports per compute unit).

### 7.4 Slippage Calculation

```
min_out = expected_out × (1 - slippage_pct / 100)
```

Slippage is set separately for buy (`buy_slippage_pct`) and sell (`sell_slippage_pct`). Dump mode overrides `min_out = 0` — accepting any amount received — to guarantee exit from a fully illiquid pool at the cost of extreme slippage.

---

## 8. Exit Strategy Architecture

The sell monitor is the most algorithmically complex component. It runs as a dedicated async task per position, checking price every `price_check_interval_ms` (default 500ms).

### 8.1 Two-Phase Monitoring

**Phase 1 — Rapid Dump Detection (first 20 ticks × 100ms = 2 seconds)**

Designed to catch rug-pulls and honeypots within 2 seconds of buy confirmation. Any 3 consecutive price declines in this window trigger immediate exit at stop-loss. This catches the most common rug pattern: token price immediately collapses to zero as the deployer sells all LP.

**Phase 2 — Continuous Monitoring (price_check_interval_ms interval)**

After Phase 1, the monitor switches to the full exit logic stack.

### 8.2 Exit Logic Evaluation Order

```
1. STOP LOSS          — below entry × (1 - stop_loss_pct/100)?    → exit
2. PROFIT FLOOR       — ever hit profit_first_floor_pct AND        → exit
                         now below that floor?
3. TIERED PARTIAL TP  — at/above tier threshold AND tier unfired?  → sell partial
4. ADAPTIVE PULLBACK  — below (peak - θ_eff)?                      → exit
5. VELOCITY DECAY     — above velocity_decay_min_pnl AND           → exit
                         price decelerating sharply?
6. MOMENTUM ESCALATION— new peak confirmed? escalate TP target     → update
7. FULL TAKE PROFIT   — above dynamic_tp_pct?                      → exit
```

The ordering is critical: stop-loss and floor protection are unconditional hard stops checked before any TP logic.

### 8.3 Live Parameter Hot-Reload

All exit thresholds (`take_profit_pct`, `stop_loss_pct`, `amount_multiplier`) are read from `live_params: Arc<RwLock<LiveParams>>` on every monitoring tick, not captured at entry time. This means:
- Strategy agent adjustments take effect immediately for all open positions
- Builder mode compounding updates every 5 seconds propagate to running sell monitors
- Rate mode changes via dashboard apply without restarting the bot

---

## 9. Adaptive Pullback Mathematics

### 9.1 Formula Derivation

A fixed pullback threshold has a fundamental problem: at low PnL levels, even a small pullback hits the exit threshold before meaningful profit is locked in; at high PnL levels, a fixed threshold wastes a significant fraction of peak gains.

The adaptive formula introduces a square-root scaling with peak:

```
θ_eff = base × √(1 + peak_pnl / 100)
```

**Why square root?** The square root function is:
- Monotonically increasing (larger peaks allow larger pullbacks)
- Concave (grows sublinearly — doesn't blow up at high PnL)
- Anchored at `base` when `peak = 0` (degrades gracefully to fixed threshold)

The exit condition:

```
exit when: current_pnl ≤ peak_pnl - θ_eff
```

### 9.2 Comparison: v1.4.0 (base=8) vs v1.3.0 (base=18)

| Peak PnL | v1.3.0 θ | v1.3.0 exit | v1.4.0 θ | v1.4.0 exit |
|---------|---------|-------------|---------|-------------|
| 0%  | 18.0% | −18.0% | 8.0% | −8.0% |
| 25% | 20.1% | +4.9%  | 8.9% | +16.1% |
| 60% | 22.7% | +37.3% | 10.1% | +49.9% |
| 100%| 25.5% | +74.5% | 11.3% | +88.7% |
| 200%| 31.2% | +168.8%| 13.9% | +186.1%|
| 500%| 44.2% | +455.8%| 19.6% | +480.4%|

The v1.4.0 formula locks in at **higher absolute PnL at every peak level** despite the lower base, because the adaptive component compensates. This is the key insight: tightening the base while keeping the adaptive formula produces better exits.

---

## 10. Momentum Escalation System

### 10.1 Rationale

When a token keeps printing new all-time highs, a static TP target becomes an artificial ceiling. The momentum escalation system raises the TP dynamically on each confirmed new peak, allowing the bot to hold genuine runners without hitting a predefined ceiling.

### 10.2 Escalation Equation

Each time a new peak is confirmed (current PnL > previous peak + `momentum_escalation_threshold_pct`), the take-profit target escalates:

```
TP_n = TP_0 × factor^n
```

Where:
- `TP_0` = initial take-profit from `live_params` (175% in v1.4.0)
- `factor` = `momentum_escalation_factor` (1.8 in v1.4.0)
- `n` = escalation round (0 to `momentum_max_escalations`)

### 10.3 7-Round Ladder (v1.4.0)

| Round | Formula | TP Target |
|-------|---------|-----------|
| 0 (base) | 175% | 175% |
| 1 | 175% × 1.8¹ | 315% |
| 2 | 175% × 1.8² | 567% |
| 3 | 175% × 1.8³ | 1,021% |
| 4 | 175% × 1.8⁴ | 1,837% |
| 5 | 175% × 1.8⁵ | 3,307% |
| 6 | 175% × 1.8⁶ | 5,952% |
| 7 (max) | 175% × 1.8⁷ | 10,714% |

A token would need to sustain continuous upward movement through 7 confirmed escalation triggers (each requiring at least 3% new gain) before the TP ladder reaches its theoretical maximum.

### 10.4 Minimum Peak Guard

The `momentum_min_peak_pct = 60.0` parameter ensures escalation only fires once a genuine 60%+ gain has been established. This prevents noise-triggered escalation on small volatile moves near entry price.

---

## 11. Builder Mode Compounding Algorithms

Builder modes replace static position sizing with live compounding algorithms that target specific SOL milestones. The algorithms run every 5 seconds in a background watcher task and update `live_params` atomically.

### 11.1 Common Framework

All modes use a `progress` variable representing current wallet advancement toward the target:

```
progress = wallet_sol / target_sol    (clamped to [0, 1])
wallet_sol = (session_start_lamports + daily_pnl_lamports) / 1e9
```

### 11.2 Growth Mode (target: 0.2 SOL)

**Objective:** Mild geometric compounding for micro-wallets building initial capital.

```
multiplier = clamp(1.0 + 1.0 × p^0.8, 1.0, 2.0)
TP         = base_tp
SL         = base_sl
```

The concave exponent (0.8) concentrates size growth early — when each SOL of progress represents a large fraction of the target — and moderates as the target is approached.

### 11.3 Builder Mode (target: 1.0 SOL)

**Objective:** Geometric compounding with adaptive TP/SL to accelerate accumulation while protecting near-target gains.

```
multiplier = clamp(1.5 + 2.0 × p^0.65, 1.5, 3.5)
TP         = base_tp × max(1.0, 1.5 - 0.5 × p)
SL         = base_sl × (1.2 - 0.2 × p)
```

**Key behavior:**
- At p=0: 1.50× size, 1.5× TP, 1.2× SL (larger bets with wider targets to compound faster)
- At p=1: 3.50× size, 1.0× TP, 1.0× SL (big bets but conservative exits to lock in gains)

The TP narrows as the target approaches because the cost of a losing trade near the target is high (losing a large fraction of accumulated gains). The SL also tightens slightly — accept smaller wins reliably rather than risking big swings near the goal line.

### 11.4 SuperBuilder Mode (target: 3.0 SOL)

**Objective:** Parabolic compounding with moon-chase phase for explosive growth from micro-wallets.

```
multiplier = clamp(2.0 + 6.0 × p^0.35, 2.0, 8.0)
TP         = base_tp × max(1.0, 2.0 - p)
SL         = base_sl × 1.4        (fixed wider stop)
moon_chase = (p < 0.25)           (auto moon-chase in first quarter)
```

**Key behaviors:**
- **Very sub-linear exponent (0.35)**: The multiplier grows extremely fast at very low progress values. At p=0.01 (1% of 3 SOL = 0.03 SOL wallet), the multiplier is already ~2.6×. This is the "lottery ticket" phase — small position, huge relative size, high variance accepted.
- **Moon chase auto-activation**: When `progress < 0.25`, the momentum-hold escalator runs in aggressive mode with the stop-loss temporarily removed for confirmed mooning tokens. This accepts maximum drawdown risk in exchange for maximum upside capture during the early phase.
- **Fixed wider SL (1.4×)**: SuperBuilder takes 2–8× position size, requiring a proportionally wider stop to avoid stop-hunting on normal volatility.

### 11.5 Progress vs Multiplier Comparison

| Progress | Growth `1+p^0.8` | Builder `1.5+2p^0.65` | SuperBuilder `2+6p^0.35` |
|----------|--------|---------|--------------|
| 0% | 1.00× | 1.50× | 2.00× |
| 10% | 1.16× | 1.95× | 4.68× |
| 25% | 1.33× | 2.31× | 5.69× |
| 50% | 1.57× | 2.77× | 6.71× |
| 75% | 1.79× | 3.16× | 7.43× |
| 100% | 2.00× | 3.50× | 8.00× |

---

## 12. Risk Management Framework

Scematica implements seven independent risk breakers. Each can halt buying independently; all must be cleared for the buy gate to open. This defense-in-depth approach prevents any single point of failure from causing runaway losses.

Six of the seven fire on **money** — drawdown, a loss window, a win rate, a score, a
reputation, a duplicate. All of them are therefore reactive: by the time they trip, the
capital is already gone. The seventh (§12.8) fires on the *epistemic* precondition instead,
and is described last because it is best understood against the others.

### 12.1 ATH Drawdown Guard

```
drawdown_pct = (session_ath - current_balance) / session_ath × 100
if drawdown_pct >= ath_drawdown_pct: HALT BUYS
```

The session ATH is monotonically increasing — it only moves up when the wallet reaches a new high. The guard resets automatically when the wallet recovers to a new ATH. Unlike timer-based pauses, this guard responds proportionally to actual capital preservation.

### 12.2 Grief Breaker

5-minute sliding window of cumulative losses. If `window_loss >= grief_max_loss_sol`, buying is suspended for `grief_cooldown_secs`. The grief breaker catches "tilt trading" scenarios — rapidly re-entering after a string of losses amplifies drawdown exponentially.

### 12.3 Kelly Criterion Sizing

Fractional Kelly (25%) position sizing from rolling win-rate and win/loss ratio:

```
f* = W - (1 - W) / R
fraction = f* × kelly_fraction_multiplier  (default 0.25)
size_sol = wallet_sol × fraction
size_sol = size_sol.clamp(min_bet_sol, max_bet_sol)
```

The quarter-Kelly multiplier accounts for model uncertainty — real-world win rates are noisier than the historical sample suggests. Kelly halts buying if the computed fraction goes negative (win rate too low to bet at all), acting as a data-driven buy pause that adapts to recent performance.

### 12.4 Pool Scorer

A composite predictive score (0–100) computed from pool age, quote vault depth, liquidity concentration, and social signal presence. Pools below `min_pool_score` (default: 40) are rejected before any transaction is attempted. The scorer is calibrated against `pool-cache.json` historical outcomes.

### 12.5 Deployer Reputation

EMA-blended deployer rug rate described in Section 6.4. Rejects deployers whose `rug_ema > max_deployer_rugs_24h`. Cross-references `scematica-deployer-reputation.json` which is updated atomically after each trade closes.

### 12.6 Mint Dedup Guard

```rust
recently_bought: Arc<DashMap<Pubkey, std::time::Instant>>
// entry expires after 300 seconds
```

Prevents duplicate buys on the same mint within 5 minutes. This catches the common pattern of Helius WebSocket reconnect events re-broadcasting recent pool creation messages, which could cause the bot to open multiple positions on the same token.

### 12.7 Coherence Breaker — Halting on Ignorance Rather Than Loss

Every RPC-bound filter in the pipeline is capped at `RPC_CALL_TIMEOUT_SECS` (3s) and **fails
open**. When a node is slow or erroring, `check_mint_renounced`, `check_freezable` and
`check_burned` return `pass()` because they *could not look* — not because they looked and
approved. For a single pool that is the correct trade-off: dropping every candidate on a
node hiccup forfeits the edge entirely.

It is the wrong state in which to keep trading. Past some fraction of unresolved checks the
pipeline is no longer a filter at all — it is a pass-through wearing a filter's name, and
the safety checks the operator believes are running are silently not running. Critically,
the pipeline still reports every one of those pools as "passed", so nothing in the existing
telemetry distinguishes a verified pass from an unverified one.

`coherence.rs` measures the distinction directly:

```
resolution_rate = resolved / (resolved + failed_open)     over a 120s sliding window
Ψ               = master_equation(Perception(resolution_rate, feed_age), …)
if Gate(Ψ) == HOLD: HALT BUYS
```

Ψ comes from `scematica-sentience` — the same master equation behind `GET /api/sentience`
(§22), so there is one definition of coherence across the system rather than two.

Four design constraints, each with a concrete failure behind it:

- **Instrumented in the two shared RPC retry helpers in `filters.rs`**, not at each
  fail-open site. A newly added filter is therefore counted by construction, rather than
  depending on its author remembering to register.
- **`MIN_SAMPLES = 20` before any verdict.** A cold start has resolved 0 of 0 checks, which
  is not evidence of a problem. A breaker that trips on an empty sample fires hardest
  exactly when it knows least.
- **`FEED_STALL_SECS = 180`.** A listener producing no pools is stalled whatever the RPC
  health looks like.
- **Buys only — never wired into the sell path.** A degraded feed is a reason to stop
  opening new risk and never a reason to stop closing existing risk. A breaker that trapped
  positions during an RPC brownout would be strictly worse than no breaker.

Configured by `coherence_breaker` in `config.toml`, default **on** via a `default_true()`
serde helper. A bare `#[serde(default)]` yields `false` for a missing bool, which would have
silently disabled a safety feature for every `config.toml` written before the field existed.

### 12.8 Emergency Controls

| Control | Trigger | Effect |
|---------|---------|--------|
| Sell Mode | Dashboard `[e]` or drawdown guard | Pauses all buys, sells all open positions |
| Dump Mode | Dashboard `[d]` | Force-sells all positions with `min_out=0` |
| High-Speed | Dashboard `[h]` | Bypasses filters/AI/scorer for aggressive entry |
| Rate Mode | Dashboard `[1-8]` | Adjusts size multiplier and TP/SL presets |

---

## 13. Cross-DEX Arbitrage Engine

### 13.1 Architecture

The arbitrage engine (`scematica-arb`) runs as a separate binary (`arb`) and can operate alongside the sniper. It searches for triangular and pairwise arbitrage opportunities across Raydium AMM V4, Orca Whirlpool, and Meteora DLMM simultaneously.

### 13.2 Graph Search

The DEX liquidity graph is represented as a directed graph where:
- **Nodes** = token mints
- **Edges** = liquidity pools (each edge has a weight = effective exchange rate after fees)

Profitable cycles are found using a modified Bellman-Ford algorithm adapted for multiplicative path weights. A cycle `A → B → C → A` is profitable when the product of exchange rates exceeds 1.0 plus the total transaction cost:

```
profit = rate(A→B) × rate(B→C) × rate(C→A) - 1.0 - tx_fees
if profit > min_arb_profit_pct: execute
```

### 13.3 Execution

Arbitrage transactions are bundled into a single atomic transaction where possible (all swaps in one tx). For cross-protocol arbs requiring multiple programs, they are submitted as a tightly sequenced pair with deadline enforcement.

---

## 14. Deep Q* Reinforcement Learning Agent

### 14.1 Overview

The DQ* agent (`scematica-nn`) is a pure-Rust Dueling Double DQN trained on live execution data from `scematica-trades.jsonl`. It **actively gates entries** in the sniper once `train_steps ≥ 10,000` — sizing up on `BuyAggressive`, shading down on a mild bearish lean, and vetoing on a strong one (§14.7 / `DQ_STAR_AGENT.md` §16). The same agent also supplies conviction to the ScemaDEX bond layer (§16.4).

### 14.2 Network Architecture

```
Input: state vector (24 features, normalized to [0,1])
  │
  ▼
Linear(24 → 128) + ReLU    [He init]
  │
  ▼
Linear(128 → 64) + ReLU    [He init]
  │
  ├──────────────────────────┐
  ▼                          ▼
Value head:             Advantage head:
Linear(64 → 1)          Linear(64 → 5)
  V(s)                  A(s, a)
  │                          │
  └──────────── Q = V + A - mean(A) ───────────►  Q-values [5]
```

**Dueling architecture rationale:** Decomposes Q(s,a) into state value V(s) and action advantage A(s,a). This allows the agent to learn that some states are inherently good or bad regardless of action — critical for trading where a deteriorating market state decreases value for all actions.

### 14.3 State Space (24 Features)

| Feature | Description |
|---------|-------------|
| `pool_age_secs` | Pool age in seconds (÷3600, capped at 1.0) |
| `initial_liquidity_sol` | Liquidity at creation (÷100) |
| `price_change_pct` | Price change from creation (normalized) |
| `volume_5min_sol` | 5-min volume in SOL |
| `buy_sell_ratio` | Buy-to-sell transaction ratio |
| `lp_burned` | LP token burn flag (0/1) |
| `mint_renounced` | Mint authority renounced (0/1) |
| `current_pnl_pct` | Unrealized PnL of current position |
| `position_age_secs` | Time position has been open |
| `daily_pnl_sol` | Session cumulative PnL |
| `consecutive_wins` | Current win streak |
| `consecutive_losses` | Current loss streak |
| `sol_balance_sol` | Current wallet balance |
| `regime` | Market regime (-1/0/1 = bear/neutral/bull) |
| `volatility` | Recent price std-dev / mean |
| `spread_pct` | Bid-ask spread fraction |
| `time_of_day_norm` | UTC hour (÷24) |
| `open_positions` | Count of open positions |
| `peak_pnl_pct` ¹ | Highest PnL seen since entry |
| `pool_score_norm` ¹ | Normalized pool quality score |
| `deployer_rug_rate` ¹ | EMA deployer rug rate |
| `volume_velocity` ¹ | Rate of change of 5-min volume |
| `price_velocity` ¹ | First derivative of price |
| `price_acceleration` ¹ | Second derivative of price |

¹ Added in v1.1.0 state space expansion (18 → 24 features)

### 14.4 Action Space

| Action | Meaning |
|--------|---------|
| `Hold (0)` | Maintain current position or do nothing |
| `BuyStandard (1)` | Enter at configured size |
| `BuyAggressive (2)` | Enter at 2× configured size |
| `SellPartial (3)` | Sell 25–50% of position |
| `SellAll (4)` | Close position entirely |

### 14.5 Reward Function

The reward (`DQNAgent::shape_reward`, full treatment in `DQ_STAR_AGENT.md` §5) is
**superlinear in profit and zoned in loss**; `hold_steps` is position age in
**minutes** and the observer loop divides the result by 100:

```
pnl ≥ 0  :  R = pnl × (1 + log₂(1 + pnl/25))  +  timing_bonus
-5 ≤ pnl < 0   :  pnl × 1.0      -30 ≤ pnl < -5 :  pnl × 1.8
-60 ≤ pnl < -30:  pnl × 2.5      pnl < -60      :  pnl × 1.5 − 15 (fast) | × 2.5 − 70 (held)
```

The **timing bonus** is discrete: **+75** (< 1 min) / +30 (≤ 3 min) / +10 (≤ 10 min) /
`−min((hold−10)×2, 40)` beyond — rewarding fast capital recycling.

**Profit reward at key PnL levels (pre-bonus):**

| PnL | Multiplier `1+log₂(1+pnl/25)` | Base reward |
|-----|-----------|-----------------|
| 25% | 2.00× | 50 |
| 50% | 2.58× | 129 |
| 100%| 3.32× | 332 |
| 200%| 4.17× | 834 |
| 500%| 5.39× | 2,696 |

### 14.6 Training Algorithm

The agent combines four advanced techniques:

**Double DQN:** Decouples action selection from evaluation to reduce overestimation bias:
```
a* = argmax_a Q_online(s', a)            // online net selects
target = r + γ × Q_target(s', a*)        // target net evaluates
```

**N-Step Returns (n=5):** Accumulates rewards over 5 steps before bootstrapping, propagating terminal rewards faster through long positions:
```
G₅ = r₀ + γr₁ + γ²r₂ + γ³r₃ + γ⁴r₄ + γ⁵ × Q_target(s₅, a*)
```

**Prioritized Experience Replay:** Samples transitions proportionally to TD error using a sum-tree data structure for O(log n) operations:
```
priority = (|TD_error| + ε)^α    (α=0.6, ε=1e-6)
w_i = (1 / (N × P(i)))^β / max_j w_j    (β: 0.4 → 1.0)
```

**Regime Branching:** At ε < 0.3, separate (online, target) net pairs are activated per market regime (bull/bear/sideways/panic), allowing dedicated policies for each market condition.

### 14.7 Tournament Evolution

Three `DQNAgent` variants run in parallel on the same stream. Every 1000 steps
(`eval_freq`) the highest-`total_reward` variant becomes primary; "balanced"
starts as primary. Promotion only switches which variant acts — it does **not**
re-initialise weights. (A separate `evolve_tournament_variants` routine optionally
hill-climbs the per-variant hyperparameters: keep the winner's triple, mutate the
others by lr ×U(0.8,1.2), ε_decay ±0.0005, γ ±0.005.)

| Variant | Learning Rate | ε Decay | γ |
|---------|--------------|---------|---|
| conservative | 0.0005 | 0.9999 | 0.95 |
| balanced (default primary) | 0.0010 | 0.9995 | 0.99 |
| aggressive | 0.0020 | 0.9990 | 0.95 |

### 14.8 Adversarial Injection

When `auto_inject_adversarial` is enabled, synthetic rug-pull, pump-and-dump, and
honeypot scenarios are injected every **100** training steps (2 per call). The
pump-and-dump case is a *positive* `SellAll` example (peak exit, reward ≈ +4.03);
the rug-pull (held-through `Hold`, ≈ −2.95) and honeypot (`SellAll`, ≈ −2.45) are
negative. This keeps adversarial examples resident in the replay buffer during
calm periods, accelerating safety learning. The always-on counterpart is action
rebalancing (one `Hold` / one `SellPartial` every 50 steps).

---

## 15. Multi-Agent AI Strategy System

### 15.1 Agent Architecture

The AI layer (`scematica-ai`) runs four specialized LLM agents backed by Groq (Llama 3.1 70B) and xAI (Grok) APIs with automatic provider fallback:

| Agent | Role | Output |
|-------|------|--------|
| **Chat** | Interactive assistant in dashboard Chat tab | Free-form responses |
| **Strategy** | Analyzes trade history, adjusts TP/SL/multiplier | `scematica-strategy.json` |
| **Risk** | Monitors drawdown, regime, position concentration | Halt recommendations |
| **Debate** | Adversarial red-team of Strategy agent's proposals | Dissenting analysis |
| **Report** | Generates session PnL summaries and trade analytics | Formatted reports |

### 15.2 Strategy Agent Loop

The strategy agent runs on a configurable interval (default: 300s). Each cycle:

1. Reads the last N trades from `scematica-trades.jsonl`
2. Computes win rate, average win/loss, streak, and regime
3. Calls the LLM with structured trade data and current parameters
4. Parses the response for `take_profit_pct`, `stop_loss_pct`, `amount_multiplier`, `regime`
5. Writes adjustments to `scematica-strategy.json` (atomically)
6. The sniper's strategy file watcher picks up the new params and updates `live_params`

### 15.3 Provider Failover

A result cache with 5-minute TTL is checked before each API call. On cache miss, the primary provider is called; if it fails (rate limit, timeout, or error), the fallback provider is tried. This ensures the strategy agent remains functional during API outages.

### 15.4 Live Data Tool Dispatcher

The Chat agent has access to a tool dispatcher that can query live bot state:

```
Available tools: get_metrics, get_trades, get_positions, get_regime,
                 get_filter_stats, get_nn_stats, get_pool_radar
```

The dispatcher reads the latest file-IPC state atomically and returns structured JSON to the LLM, enabling the chat agent to answer questions like "what's my current win rate?" or "which filter is rejecting the most pools?" with real-time data.

---

## 16. x402 Payment Protocol

### 16.1 Overview

`scematica-protocol` implements the HTTP 402 Payment Required flow — a machine-native micropayment protocol for API monetization. It runs as an HTTP server that requires payment verification before serving premium data endpoints.

### 16.2 Flow

```
Client                    scematica-protocol
  │                              │
  │ GET /api/v1/signal           │
  │─────────────────────────────►│
  │                              │
  │◄─────────────────────────────│
  │ 402 Payment Required         │
  │ X-Payment-Address: <wallet>  │
  │ X-Payment-Amount: <lamports> │
  │                              │
  │ [Client sends on-chain tx]   │
  │                              │
  │ GET /api/v1/signal           │
  │ X-Payment-Tx: <signature>    │
  │─────────────────────────────►│
  │                              │ [Verify tx on-chain]
  │◄─────────────────────────────│
  │ 200 OK + signal data         │
```

### 16.3 Use Cases

The protocol enables monetization of Scematica's computed signals:
- Pool quality scores (output from pool scorer)
- Deployer reputation data
- DQ* agent buy/sell recommendations
- Aggregated pool radar data

Third-party bots and tools can pay micropayments in SOL to access these signals without needing to run the full Scematica stack.

### 16.4 ScemaDEX — The Agentic Liquidity Layer

Building on the x402 rail, the `scemadex-*` crates generalise the bot from a
self-contained trader into an **agentic liquidity layer** in which the *routing
intelligence itself* is a metered, accountable, tradeable product. The published
[`scemadex-sdk`](https://crates.io/crates/scemadex-sdk) defines a lean trait
surface (`RoutePolicy`, `BondEngine`, `VenueExecutor`, `SignalSource`,
`PeerMarket`) with no `solana-sdk` dependency by default, plus reference
implementations; see [`scemadex.md`](scemadex.md) for the full treatment.

Four composing primitives:

- **A · Metered inference routing.** Every quote is produced by a learning policy
  and is individually billable per call over x402 — the *quality of the decision*
  is the SKU.
- **B · Intent solving.** Callers express *what* they want
  (`Objective::{Price, Speed, Stealth}`), not a path; the policy decides
  venue / split / timing (including MEV-resistant `Stealth` execution).
- **C · Signal & reputation oracle.** Reputation, pool scores, and advice are
  monetised read endpoints — the same signals §16.3 sells, exposed through the
  SDK and served by `scemadex-relay`.
- **D · Conviction Routing** *(the defining primitive)*. The policy escrows a
  **slashable performance bond** against its own promise, sized by its conviction.
  Meet the guarantee → reclaim the bond and collect the fee; miss it → the bond
  settles to the caller. `EscrowBondEngine` ships the open settlement state
  machine; `scemadex-settle` performs the real **devnet** USDC transfer on slash;
  the production mainnet x402 facilitator (`scemadex-integrations::X402BondEngine`)
  is the closed companion.

Composed across nodes, these yield a **`PeerMarket` mesh** where agents sell
bonded inferences and trade learned RL experience with one another — an economy of
machine intelligence settled in stablecoins. The same Deep Q\* agent that gates
the bot's entries (§14) supplies the conviction that prices these bonds, via
`scemadex-integrations::JupiterRoutePolicy::with_agent`. Non-Rust agents join over
HTTP: the [Dexter x402 SDK](https://github.com/Dexter-DAO/dexter-x402-sdk)
(`@dexterai/x402`) is the TypeScript client counterpart to `scematica-protocol`'s
Rust facilitator, so a JS/TS agent can pay a Rust ScemaDEX relay for a bonded
inference with no glue code.

---

## 17. File-Based IPC Architecture

### 17.1 Design Philosophy

The sniper and dashboard run as separate OS processes communicating exclusively through JSON files in the working directory. There is no socket, pipe, or shared memory — only the filesystem.

This design is intentional:
- **Process isolation**: A dashboard crash cannot corrupt sniper state
- **Debuggability**: All state is human-readable and can be inspected with any editor
- **Simplicity**: No IPC framework to version, no serialization protocol to negotiate
- **Resilience**: The sniper continues trading even when the dashboard is closed

### 17.2 Atomic Write Convention

All writers use the `write tmp → rename` pattern for atomic visibility:

```rust
let tmp = format!("{}.tmp", path);
std::fs::write(&tmp, content)?;
std::fs::rename(&tmp, path)?;
```

The `rename` syscall is atomic on all POSIX filesystems and Windows NTFS — the reader either sees the old file or the new file, never a partial write.

### 17.3 IPC File Registry

| File | Writer | Reader | Update Frequency |
|------|--------|--------|-----------------|
| `scematica-sniper.log` | Sniper (tracing) | Dashboard (tail) | Every log line |
| `scematica-metrics.json` | Sniper | Dashboard | Every 5s |
| `scematica-trades.jsonl` | Sniper | Dashboard, DQ* agent | Every trade event |
| `scematica-filter-stats.json` | Sniper | Dashboard | Every pool evaluation |
| `scematica-nn-stats.json` | DQ* agent | Dashboard | Every 5s |
| `scematica-nn-agent.json` | DQ* agent | DQ* agent | Every 10 min |
| `scematica-strategy.json` | AI strategy agent | Sniper | Every strategy cycle |
| `scematica-rate-mode.json` | Dashboard | Sniper | On mode change |
| `scematica-builder-mode.json` | Dashboard | Sniper | On mode change |
| `scematica-sell-mode.json` | Dashboard/drawdown | Sniper | On trigger |
| `scematica-dump-mode.json` | Dashboard | Sniper | On trigger |
| `scematica-positions.json` | Sniper | Dashboard | Every 1s |
| `pool-cache.json` | Sniper, pool-seeder | Sniper | On pool discovery |
| `scematica-deployer-reputation.json` | Reputation ledger | Filters | After each trade |
| `scematica-pool-decisions.jsonl` | Sniper | Replay endpoint (§23) | Every pool evaluation |
| `scematica-sniper.lock` | Sniper | Sniper | Single-instance PID guard |

---

## 18. TUI Dashboard

### 18.1 Architecture

The dashboard is a `ratatui` + `crossterm` TUI application running in the alternate terminal screen. It maintains a 250ms tick rate for UI refresh. All rendering is stateless — the UI reads from `Arc<AppState>` atomically on each frame, with no mutable state in the render path.

### 18.2 Tabs

| Tab | Key Bindings | Primary Content |
|-----|-------------|-----------------|
| **Overview** | — | Metrics table, PnL sparkline, live positions, pool radar preview, session stats |
| **Trades** | `[x]` export, `[R]` reset | Trade history table with PnL, status, signature |
| **Logs** | `[e]` sell, `[b]` buy, `[h]` highspeed, `[d]` dump, `[/]` filter | Live log stream with filter bar |
| **Config** | `[1-8]` rate, `[g/j/k/o]` builder, `[↑↓]` scroll | All live parameters, builder mode selector, filter stats |
| **Chat** | `[Enter]` send, `[y/n]` confirm | AI assistant with tool access to live bot data |
| **Radar** | — | Scatter plot of evaluated pools (age vs. score) with table |

### 18.3 Live Position Display

Each open position is rendered as a table row with:
- Mint address (abbreviated)
- Entry SOL amount
- Current value (live)
- PnL % (color-coded: green/yellow/red)
- SL% floor
- 8-character progress bar `░░░███░░` (SL at left, TP at right, fill = current price)
- Decline streak indicator (`▼` / `▼▼`)

### 18.4 Demo Mode

`cargo run --release --bin dashboard -- --demo` runs a simulation with synthetic data — no keypair or RPC connection required. Useful for evaluating the dashboard without a funded wallet.

---

## 19. Performance Characteristics

### 19.1 Pool Detection Latency

Pool detection latency (WebSocket event receipt to buy transaction submission):

| Phase | Typical Latency |
|-------|----------------|
| WebSocket propagation | 10–50ms |
| Filter pipeline (parallel) | 100–500ms (3s timeout per filter) |
| Transaction building | <5ms |
| RPC submission | 10–100ms |
| **Total (typical)** | **~200–700ms** |

High-speed mode bypasses filters, reducing total latency to ~50–150ms at the cost of higher rug exposure.

### 19.2 Sell Monitor Precision

Phase 1 checks every 100ms for the first 2 seconds. Phase 2 checks every 500ms (configurable). At 500ms intervals, the maximum loss from a sudden price collapse after Phase 1 ends is bounded by the stop-loss percentage (default 18%) plus slippage.

### 19.3 Binary Size and Memory

Release profile settings: `lto = "fat"`, `codegen-units = 1`, `panic = "abort"`, `overflow-checks = true`. Typical binary sizes:
- `sniper.exe`: ~12–18 MB
- `dashboard.exe`: ~10–15 MB

Memory footprint at steady state with 10 open positions: ~50–80 MB RSS. The replay buffer (`PrioritizedReplayBuffer`, 10,000 transitions × 24 floats × 8 bytes × 2 states ≈ ~40 MB) is the largest single allocation.

---

## 20. Security Model

### 20.1 Keypair Security

The wallet keypair is loaded at startup and held in memory as an `Arc<Keypair>`. It is never written to disk after initial load, never logged, and never transmitted over any network connection. All transaction signing happens in-process using the loaded keypair.

WSL UNC paths (`\\wsl$\Ubuntu\...`) are supported for keypairs stored in WSL filesystems, allowing the keypair to remain outside the Windows filesystem.

### 20.2 No Remote Code Execution

The bot accepts no incoming network connections (excluding the x402 protocol server, which is separately deployed). All outbound connections are:
- Solana RPC (HTTPS/WSS) — authenticated by RPC provider API key
- CoinGecko price API — read-only, unauthenticated
- Anthropic / Groq / OpenRouter / Cerebras AI APIs — authenticated by API key, server-side only
- Telegram/Discord webhooks — write-only, authenticated by bot token

### 20.3 Config File Trust

`config.toml` is the sole trust boundary for external input. All other inputs (RPC responses, trade data, AI responses) are validated before use. AI agent outputs that would adjust TP/SL are bounded by hard limits in the strategy parser — the LLM cannot instruct the bot to set `stop_loss_pct = 99%` regardless of what the API returns.

### 20.4 Process Isolation

The sniper and dashboard are separate OS processes. A dashboard crash, UI hang, or memory corruption cannot affect sniper execution. The sniper writes a PID lockfile (`scematica-sniper.lock`) and refuses to start if another instance is already running — preventing two snipers from racing on the same wallet and WSOL ATA.

---

## 21. Token Economics

### 21.1 $SCEMA Token

$SCEMA is a Token-2022 token on Solana mainnet with contract address `HcsHqEJ9suf4oHJ8mb52M7AVKjhYhnTaeHgTmde7pump`.

**Access Gate:**
- 250,000 $SCEMA required to operate the sniper or dashboard
- Checked at startup with 5 retries (handles transient RPC failures)
- Bypass only available via `SCEMATICA_SKIP_GATE=1` (intended for RPC outages only)

### 21.2 Utility

$SCEMA serves as a permissioning layer rather than a fee token — holding is required for access, but no $SCEMA is consumed per trade. This design:
- Aligns token holder incentives with protocol usage (demand for SCEMA = demand for the bot)
- Avoids creating a per-trade cost that compounds against small wallets
- Enables the x402 protocol to independently monetize signal access using SOL micropayments

### 21.3 Token-2022 Standard

SCEMA uses the Token-2022 program rather than the legacy SPL Token program. All gate checks and balance queries use Token-2022 helpers. Legacy SPL Token queries will return zero balance — operators and integrators must use Token-2022 ATA addresses when querying SCEMA balances.

---

## 22. Cognitive Architecture and the Ψ Gate

### 22.1 What the Equation Is For

`crates/scematica-sentience` implements the Singularity Cognitive Architecture as 29
computable modules — perception, data integrity, rationality, logic, ethics, knowledge
graph, memory, learning, prediction, agency, meta-cognition, self-model, identity, valence,
attention, curiosity, error correction, contradiction and truth confidence — converging on:

```
S_t     = R_t × L_t × M_t × D_t                    sentience index
Ψ_t     = S_t × I_t × K_t × MC_t × A_g,t × F_t     integrated cognition
Ω_{t+1} = F(Ω_t, Perception, Memory, Reasoning, Ethics, Action, Feedback)
```

Five of the architecture's seventeen axioms are enforced as runtime checks rather than
documented as principles.

The name invites the assumption that this is decorative. It is not, and the reason is
narrow and concrete: **Ψ measures staleness and contradiction, not mood.** Every read
endpoint in `scematica-api` serves its state file identically whether that file was written
four seconds ago or four hours ago, and `/api/health` reports only that a process *was*
here. Without a gate, a live-looking briefing can describe a session that ended overnight.

### 22.2 The Gate

`GET /api/sentience` returns a verdict over three bands:

| Verdict | Meaning | Behaviour |
|---------|---------|-----------|
| **GO** | Data is fresh and internally consistent | Answer normally |
| **CAUTION** | Measured degradation | Answer, but the model is told its footing is weak |
| **HOLD** | State cannot be trusted to describe now | **HTTP 409; the model is not called** |

HOLD does not warn the model and proceed. It refuses, because a warned model still writes a
confident paragraph of stale numbers — the warning changes the prose and not the numbers.

Absent is distinct from HOLD. A deployment with no bot, or an older API without the
endpoint, returns `null` — "no opinion" — and the caller proceeds fully functional. A
missing gate must never be read as a failing one.

### 22.3 Two Failure Modes That Shaped the Implementation

Both were hit during development, and both would have rendered the gate useless in opposite
directions:

**Perception's data ratio is a product.** A single unmeasured channel scored 0 pins Ψ at 0
and jams the gate permanently shut. Unmeasured dimensions therefore evaluate to **1.0** —
"not a limiting factor" — so that only *measured* degradation moves the verdict. The
alternative is a healthy bot sitting in permanent CAUTION, which trains operators to ignore
the badge and is indistinguishable from having no gate.

**The handler must overwrite only measured fields, via `state_mut`.** Calling `set_state`
there also replaces the timestep and the sentience index, which silently cancels every
`/api/sentience/observe` on the very next gate read.

### 22.4 Why Ψ Cannot Be Talked Upward

Ψ is a pure function of measured data integrity by design. Coherent, confident answers do
not raise it. This is deliberate: a gate that a fluent model could argue with would fail in
precisely the case it exists to catch, since a model reasoning over stale numbers produces
*more* internally consistent output, not less. The only input that moves Ψ is measurement.

The same equation drives the coherence breaker (§12.7) — one definition across the system.

---

## 23. Counterfactual Replay and Calibration

### 23.1 The Asymmetry of Evidence

`POST /api/replay` answers "what if the thresholds had been different?" against what the
pipeline **actually measured**. Every evaluated pool is written to
`scematica-pool-decisions.jsonl` with the values that decided it — `pool_score`,
`pool_size_sol`, `pool_age_secs`, `buy_pressure_ratio` — so re-applying a threshold requires
no RPC and no simulation.

The result is deliberately asymmetric, and the asymmetry is the design:

| Direction | What it admits | What can be reported |
|-----------|----------------|----------------------|
| **Tightening** | Excludes pools that *were* taken | **Exact** PnL delta — real trades, real realised SOL |
| **Loosening** | Admits pools that were rejected | Count and measured distribution only — **no return figure** |

Nobody bought the rejected pools, so nothing recorded what they would have done. That is
not a gap to fill with an estimate; it is the shape of the evidence. Inventing an expected
value for the loosening case is the single most tempting move available here and would make
every answer built on it worthless — the same failure the simulation banner in `web/` exists
to prevent.

### 23.2 Why Not the Backtester

Replay is deliberately **not** built on `scematica_sniper::Backtester`. That path replays
`BacktestPool` records through `static_filter_check`, which returns `false` outright
whenever `min_pool_size > 0` or any RPC-bound filter is enabled, and never inspects
`pool_score` at all. Under any realistic configuration it answers "nothing would pass" — a
confident number that means nothing. The decision log has no such problem, because the
measurement already happened.

### 23.3 Calibration

`GET /api/calibration` exploits a property this domain has and most assistant deployments do
not: **ground truth arrives automatically, minutes later.** The assistant says a pool looks
strong; `scematica-trades.jsonl` records what it did. "Of the 40 pools I called strong, 12
rugged" is a measurable fact rather than a stylistic impression.

Two limits are load-bearing:

- **Claims are scoped to the sentence naming the mint**, never to the whole message. A
  paragraph mentioning four mints does not hold four opinions; attributing the message's
  overall sentiment to each would manufacture claims that were never made and then score
  against them.
- **Only claims with an outcome are scored.** Bullish calls resolve against realised PnL.
  Bearish calls usually cannot — nobody buys what the assistant warns against, so nothing
  records whether the warning was right. Unresolved claims are **counted, not scored**.
  Closing that gap with an estimate is how a calibration number becomes flattery.

---

## 24. Web Interfaces and the Product Surface

`web/` is a standalone Next.js application hosting three distinct products that share a
codebase and nothing else — each has its own palette and its own data rules.

### 24.1 The Sniper Dashboard

`app/api/[...slug]/route.ts` proxies a reachable `scematica-api` when `RUST_API_URL`
resolves, and otherwise falls back to a self-contained simulation in `web/lib/sim/` —
including a real Dueling Double-DQN (`lib/sim/dqstar.ts`) mirroring `scematica-nn`.

The labelling rules are absolute. Simulated responses carry `simulated: true` and an
`X-Scematica-Source: simulation` header, surface a permanent SIMULATION banner, and control
POSTs return **503 rather than faking success**. Simulated PnL must never render as live
results.

Three data-sourcing rules that are easy to break:

1. **One timer per endpoint.** Panels subscribe through `lib/store.ts` / `lib/queries.ts`.
   Polling is refcounted so a hidden panel stops fetching; a panel adding its own
   `setInterval` silently undoes that.
2. **Discovery prefers a live bot, falls back to the real public feed, and never invents
   data.** There is no third branch.
3. **The TypeScript pool scorer is a port, not a second brain.** Rust stays authoritative;
   every filter declares `parity: 'port' | 'approx'`, and `npm run check:parity` pins the
   Rust unit-test cases.

### 24.2 alchem-link

An Alchemy × Chainlink oracle developer toolkit, and a second product rather than a sniper
panel. It reads live Chainlink aggregators with **no simulation branch at all** — these
routes read a chain or report the error, because a fabricated price would defeat the entire
point of a staleness verdict.

Heartbeats are **measured per feed per chain** (Polygon ~60s, Base/OP 1200s, mainnet 3600s),
never a shared default, with a 15% staleness tolerance on top: real publish ceilings run a
percent or two over the configured interval, and a feed that flickers STALE every cycle
trains people to ignore the flag. `lib/alchem/` is a port of the authoritative Python
package; `/api/alchem/verify` catches the two drifting by asking the chain rather than
either table.

### 24.3 Scylar Terminal

An avatar chat terminal over live bot state, gated by Ψ (§22). Its constraints follow the
same logic as the rest of the system:

- **Provider keys are server-side, always**, and the chat route **strips client-supplied
  `system` turns**. Without that, a public endpoint with a key behind it is someone else's
  free LLM proxy.
- **The model picks a tool name, never a URL.** `lib/scylar/tools.ts` hard-codes a path per
  tool — all GETs, no control routes — so no model output can reach an endpoint that is not
  on the list. Row counts are clamped and repeated identical calls within a turn are served
  from cache.
- **Live bot state is opt-in and labelled**, tagged `SIMULATED` when it is. The per-turn
  badge is the guarantee; the prompt instruction is only a mitigation, and it was ignored
  entirely until phrased as a required output token rather than a description.

`npm run check:scylar` pins the pure logic: expressions, speech, markdown, commands,
session, tools and gate.

---

## 25. Cross-Chain Expansion: BOT Chain and the Neural Mesh

### 25.1 Why a Separate Workspace Is Mandatory

`scema-botchain/` ports the architecture to BOT Chain (EVM, chain **677**) and lives in the
root workspace's `exclude` list — not for tidiness, but because every current EVM stack
requires `reqwest 0.12` / `rustls 0.23`, which is exactly the combination Appendix B
documents as irreconcilable with `solana-sdk`'s `curve25519-dalek 3`. One workspace means
one lockfile means one resolved `zeroize`, and no version satisfies both trees.

The rule that follows: nothing in that tree may depend on a crate pulling `solana-sdk`. The
chain-agnostic crates (`scematica-nn`, `scematica-sentience`, `scemadex-sdk`) are safe;
`scematica-core` and everything rooted on it are not.

### 25.2 The Measurement That Cancelled the Sniper

Before porting a sniper, the question is whether there is anything to snipe. `botchain-probe`
read the chain rather than the documentation (August 2026):

| Window | V3-style factory | CA factory |
|---|---|---|
| 20,000 blocks (~3.7 h) | 0 | 0 |
| 200,000 blocks (~1.5 d) | 0 | 0 |
| 1,000,000 blocks (~7.7 d) | **2** | 0 |

Two pool creations in roughly eight days, against a Solana side whose PF 6.50 edge exists
precisely because Raydium produces continuous new-pool flow. Supporting reads: 0.29% network
utilisation; ~119k tx/day of which a large share is the per-block `BOTValidatorSet.deposit`
(Parlia system activity, not users); 2 swaps in a 50-transaction sample; four tokens with
real holders followed by a tail of 2-to-6-holder test deployments. Consensus is Parlia PoSA
— a BSC fork.

**No sniper is scheduled for BOT Chain.** The Solidity contracts (`BotchainPriceFeed`,
`ScemaArbExecutor`, `ScemaBondEscrow`, `BotchainNNMesh`) are deployed and tested on 677 so
the port is ready if flow arrives, and the probe re-runs the measurement on demand. This is
a research result acted upon, not a shelved feature.

### 25.3 Verifiable Inference — Determinism Before Cryptography

`scema-bot-mesh/` makes an agent's decisions checkable by a party that did not run them,
including by a contract. Weights are far too large for on-chain storage, but a keccak256
hash of them is 32 bytes and so is a hash of an inference: the agent commits 32 bytes, a
challenger holding the weights re-runs the forward pass, disagreement is provable, and the
bond behind the claim is slashable via `ScemaBondEscrow`.

Commit-and-challenge is old. The reason it is rarely applied to neural inference is that the
challenger's re-run must produce **the same bits**, and floating point does not cooperate:
Solidity cannot represent an `f32` at all, transcendentals are libm implementations rather
than IEEE operations, and JavaScript has no `f32`. The foundation is therefore Q16.16
integer arithmetic, with implementation details promoted to specification:

| Decision | Why it is normative |
|---|---|
| Round-half-**away-from-zero** | Symmetric: `(-x)·y == -(x·y)` exactly, so a sign flip cannot change a magnitude |
| Division, **not `>>`** | An arithmetic shift floors toward −∞ and breaks that symmetry — a real bug here, caught by `multiplication_is_symmetric_under_sign` |
| Fixed summation order, widened accumulator | A SIMD reimplementation that reassociates a sum is a consensus break, not a speedup |
| Ties → lowest index | An unspecified tie is a divergent action, and a divergent action is a divergent game state |
| Saturate, never wrap | A wrapped activation silently becomes its own negation |

`FRAC_BITS`, parameter ordering and domain tags are bound into the hash, so a future change
produces a visibly different commitment rather than a silently incompatible one.

---

## 26. Roadmap

The full quarter-by-quarter record — including a verdict of DELIVERED / PARTIAL / NOT
STARTED / DROPPED against every milestone from the original 2026 plan — is in
[ROADMAP.md](ROADMAP.md). Summarised here:

### Delivered since the v1.11.0 whitepaper baseline

- **Epistemic risk management.** The coherence breaker (§12.7) and the Ψ gate (§22) — the
  first breakers in the system that fire before the loss rather than after it.
- **Counterfactual replay and calibration** (§23), both explicit about what they cannot know.
- **Three-product web surface** (§24): sniper dashboard, alchem-link, Scylar Terminal.
- **Cross-chain expansion** (§25) — the v2.0 milestone, reached and then *paused on its own
  measurement*, which is the outcome the original bullet did not contemplate.
- **USD display.** Live CoinGecko SOL/USD across dashboard panels — previously listed as
  planned, shipped in `scematica-dashboard`.
- **Agent marketplace substrate.** ScemaDEX intents, Conviction-Routing bonds and the
  `PeerMarket` inference/experience mesh (§16.4).

### Near-term

- **Multi-wallet support** — pool funds across wallets. Still the largest unbuilt trading
  feature, carried since Q2 2026.
- **Wire the Ψ gate into the live trading path.** It currently gates the API and the
  coherence breaker; the sniper's own LLM calls do not yet depend on it. A known wiring gap,
  stated as one.
- **`scematica-swap` mainnet deploy.** The arb engine runs **program-less** today (atomic
  revert + final-hop `min_out`), so this is a latency optimisation rather than a
  prerequisite.
- **Automated treasury balancing** across open positions.

### Longer-term

- **External security audit**, covering the executor, the wallet path, the x402 facilitator
  and the relay. Sequenced *before* staking, not after.
- **Strategy marketplace** on the ScemaDEX bonded-teaching primitives, with DQ\* checkpoints
  published for x402-gated tournament scoring.
- **On-chain SCEMA staking** for a share of x402 protocol fee revenue.
- **Community governance systems.**

---

## Appendix A: Configuration Reference

All parameters are set in `config.toml` and support live hot-reload via `scematica-strategy.json` (TP/SL/multiplier) or `scematica-rate-mode.json` / `scematica-builder-mode.json` (presets).

| Parameter | Default | Description |
|-----------|---------|-------------|
| `take_profit_pct` | 175.0 | Base TP target (momentum escalation extends this) |
| `stop_loss_pct` | 18.0 | Hard stop-loss floor |
| `trailing_stop_loss_pct` | 12.0 | Trailing SL tightening above profit floor |
| `profit_first_floor_pct` | 25.0 | Floor below which a profitably-entered position is exited |
| `momentum_escalation_factor` | 1.8 | TP multiplier per escalation round |
| `momentum_max_escalations` | 7 | Maximum rounds of escalation |
| `momentum_escalation_threshold_pct` | 3.0 | Min % gain to trigger a new escalation round |
| `momentum_min_peak_pct` | 60.0 | Peak must be ≥ this before adaptive pullback activates |
| `momentum_pullback_exit_pct` | 8.0 | Base pullback width (adaptive formula scales it with peak) |
| `velocity_decay_min_pnl_pct` | 25.0 | Min PnL for velocity-decay exit to fire |
| `velocity_decay_drop_threshold` | 1.5 | Recent velocity must be < previous / 1.5 to trigger |
| `price_check_interval_ms` | 500 | Sell monitor tick interval (Phase 2) |
| `kelly_fraction_multiplier` | 0.25 | Kelly fraction (quarter-Kelly for uncertainty) |
| `ath_drawdown_pct` | 0.0 (disabled) | Session ATH drawdown tolerance before buy pause. The shipped `config.toml` sets **20.0** |
| `min_pool_score` | 35.0 | Minimum predictive pool score (0–100); 0 disables the scorer gate. The shipped `config.toml` sets **65** — 92 passed only pools still visibly pumping, i.e. parabolic tops |
| `coherence_breaker` | `true` | Epistemic breaker (§12.7). Defaults **on** via `default_true()` — a bare `#[serde(default)]` would yield `false` and silently disable it for every pre-existing `config.toml` |

Values in this table are the **`Default` impl in `crates/scematica-core/src/config.rs`**,
which is what a missing field resolves to. Where the shipped `config.toml` deliberately
differs, both are given.

---

## Appendix B: Dependency Pin Rationale

The workspace pins several dependencies at older versions to maintain build determinism with Solana SDK 1.18.26:

| Dependency | Pinned Version | Reason |
|------------|---------------|--------|
| `solana-sdk` | 1.18.26 | 2.x requires sweeping code changes and creates irreconcilable `zeroize` version conflicts with `reqwest ≥ 0.12` |
| `reqwest` | 0.11 | 0.12 pulls `rustls 0.23` → `zeroize ≥ 1.7`, conflicting with `curve25519-dalek 3` (required by `ed25519-dalek` via `solana-sdk`) |
| `tokio-tungstenite` | 0.21 | Compatibility with the `tungstenite` version required by `solana-sdk` transitively |
| `base64` | 0.21 | 0.22 removes the legacy `decode`/`encode` functions used in `jupiter.rs` and the sniper executor |

---

*Scematica is experimental software. Trading cryptocurrencies involves substantial risk of loss. This whitepaper is a technical description only and does not constitute financial advice. Past performance of algorithmic strategies does not guarantee future results.*
