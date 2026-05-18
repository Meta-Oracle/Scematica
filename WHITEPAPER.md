# SCEMATICA: Autonomous AI Trading Infrastructure for Solana

### Technical Whitepaper — v1.4.0

**Contract Address:** `AbKiP2Jc6nM7937jTDfqoJC1bsg5FQ24Buk2iqRFpump`  
**Token:** $SCEMA (Token-2022, Solana Mainnet)  
**Gate:** 250,000 SCEMA required to operate

---

## Abstract

Scematica is a full-stack autonomous trading infrastructure for the Solana blockchain, combining high-frequency token sniping, cross-DEX arbitrage, reinforcement learning via a Dueling Deep Q* agent, multi-agent AI strategy analysis, and a real-time terminal dashboard — all implemented in pure Rust. The system targets Raydium AMM V4 new-pool events and executes decisions in sub-second latency from pool detection to signed transaction. Access is gated behind a 250,000 $SCEMA token balance, aligning operator incentives with the protocol's token economics. This document provides a comprehensive technical description of the architecture, algorithms, risk management framework, and the mathematical foundations underlying Scematica's profit-maximization strategies.

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
22. [Roadmap](#22-roadmap)

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
| `scematica-ai` | LLM agents (Groq/xAI): Chat, Strategy, Risk, Debate, Report |
| `scematica-nn` | Pure-Rust Dueling Deep Q* agent — no external ML dependencies |
| `scematica-dashboard` | Ratatui TUI — 6 tabs, real-time metrics, config, AI chat |
| `scematica-protocol` | Rust-native x402 HTTP/402 payment protocol server |

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

| Progress | Growth | Builder | SuperBuilder |
|----------|--------|---------|--------------|
| 0% | 1.00× | 1.50× | 2.00× |
| 10% | 1.21× | 1.88× | 3.04× |
| 25% | 1.44× | 2.13× | 3.89× |
| 50% | 1.66× | 2.52× | 5.04× |
| 75% | 1.84× | 2.88× | 6.07× |
| 100% | 2.00× | 3.50× | 8.00× |

---

## 12. Risk Management Framework

Scematica implements six independent risk breakers. Each can halt buying independently; all must be cleared for the buy gate to open. This defense-in-depth approach prevents any single point of failure from causing runaway losses.

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

### 12.7 Emergency Controls

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

The DQ* agent (`scematica-nn`) is a pure-Rust Dueling Double DQN trained on live execution data from `scematica-trades.jsonl`. It currently operates in observer mode — training on real trades and publishing recommendations — with planned integration as a buy gate when `ε < 0.3`.

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

The reward is superlinear in positive PnL to teach the agent to value large wins disproportionately:

```
R = pnl × (1 + log₂(1 + pnl/25))    if pnl > 0
R = pnl                               if pnl ≤ 0
```

A timing bonus rewards fast profitable exits:

```
timing_bonus = +0.5 × (1 - position_age_secs / 3600)   if pnl > 0 AND age < 3600s
```

**Reward at key PnL levels:**

| PnL | Multiplier | Effective reward |
|-----|-----------|-----------------|
| 25% | 2.00× | 50 |
| 50% | 2.58× | 129 |
| 100%| 3.00× | 300 |
| 200%| 3.58× | 716 |
| 500%| 4.32× | 2,158 |

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

Three hyperparameter variants run in parallel. Every 1000 steps, the highest-reward variant becomes the primary agent and all variants are re-initialized from the winner with ±20% hyperparameter perturbation:

| Variant | Learning Rate | ε Decay | γ |
|---------|--------------|---------|---|
| Conservative | 0.0008 | 0.9990 | 0.98 |
| Balanced | 0.0010 | 0.9995 | 0.99 |
| Aggressive | 0.0012 | 0.9998 | 0.995 |

### 14.8 Adversarial Injection

Synthetic rug-pull, pump-and-dump, and honeypot scenarios are injected every 50 training steps. This ensures the replay buffer always contains adversarial examples even during calm market periods, accelerating the agent's safety learning by an estimated 10–20× vs. waiting for real rugs.

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
- Groq/xAI AI APIs — authenticated by API key
- Telegram/Discord webhooks — write-only, authenticated by bot token

### 20.3 Config File Trust

`config.toml` is the sole trust boundary for external input. All other inputs (RPC responses, trade data, AI responses) are validated before use. AI agent outputs that would adjust TP/SL are bounded by hard limits in the strategy parser — the LLM cannot instruct the bot to set `stop_loss_pct = 99%` regardless of what the API returns.

### 20.4 Process Isolation

The sniper and dashboard are separate OS processes. A dashboard crash, UI hang, or memory corruption cannot affect sniper execution. The sniper writes a PID lockfile (`scematica-sniper.lock`) and refuses to start if another instance is already running — preventing two snipers from racing on the same wallet and WSOL ATA.

---

## 21. Token Economics

### 21.1 $SCEMA Token

$SCEMA is a Token-2022 token on Solana mainnet with contract address `AbKiP2Jc6nM7937jTDfqoJC1bsg5FQ24Buk2iqRFpump`.

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

## 22. Roadmap

### v1.5.0 — Planned

- **DQ* buy gate activation**: Promote the DQ* agent from observer mode to active buy gating when `ε < 0.3`. The agent's `BuyStandard` / `BuyAggressive` / `Hold` recommendations will gate the sniper's buy decision, with override available via dashboard.
- **On-chain Anchor program**: Deploy `scematica-swap` for atomic multi-hop swaps, eliminating the two-transaction WSOL wrapping overhead and reducing buy latency by ~50ms.
- **USD display**: Live CoinGecko SOL/USD price feed integrated into all dashboard panels — header, metrics, Sell Mode and Dump Mode banners.
- **Multi-wallet support**: Pool funds across multiple wallets to exceed per-wallet position limits without manual management.

### v2.0.0 — Long-Term Vision

- **Full DQ* gating**: Agent controls all entry decisions; rule-based filters serve as pre-filtering only
- **Cross-chain expansion**: Extend the architecture to EVM chains (Base, Arbitrum) using the same filter/executor/RL framework
- **Agent marketplace**: Publish DQ* agent checkpoints for community evaluation and tournament scoring via the x402 protocol
- **On-chain SCEMA staking**: Stake SCEMA to receive a share of protocol fee revenue from x402 API payments

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
| `ath_drawdown_pct` | 15.0 | Session ATH drawdown tolerance before buy pause |

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
