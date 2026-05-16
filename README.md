# Scematica v0.5.0

**CA: AbKiP2Jc6nM7937jTDfqoJC1bsg5FQ24Buk2iqRFpump**

Autonomous AI trading infrastructure for Solana. Token sniping, cross-DEX arbitrage, deep Q-learning reinforcement, and a Rust-native x402 monetization protocol — unified under a real-time TUI dashboard.

---

## Architecture

Scematica is a Rust workspace with 8 active crates:

| Crate | Binary | Purpose |
|---|---|---|
| `scematica-core` | — | Shared config, RPC, wallet, metrics, types |
| `scematica-sniper` | `sniper` | Raydium pool sniping with filter pipeline and sell mechanics |
| `scematica-arb` | `arb` | Cross-DEX arbitrage graph search (Raydium / Orca / Meteora) |
| `scematica-executor` | — | Multi-DEX swap execution layer, Jupiter integration |
| `scematica-ai` | — | LLM agents: Risk, Arb, Debate, Strategy, Report, Chat |
| `scematica-nn` | — | Deep Q* reinforcement learning agent |
| `scematica-dashboard` | `dashboard` | Ratatui TUI: monitor, control, AI chat |
| `scematica-protocol` | `scematica-protocol` | Rust-native x402 HTTP payment protocol for Solana |

Tools (non-bot utilities):
- `tools/key-converter` — Convert keypair formats
- `tools/pool-seeder` — Pre-seed pool cache from on-chain data

The `programs/scematica-swap` Anchor program must be built and deployed separately with `anchor build`.

---

## Prerequisites

- [Rust](https://rustup.rs/) (stable, 1.75+)
- [Solana CLI](https://docs.solana.com/cli/install-solana-cli-tools) (for keypair generation)
- A Solana wallet keypair (`~/.config/solana/id.json` or any path)
- At least **250,000 SCEMA** tokens in your wallet (token-gated access — CA above)
- A private RPC endpoint (Helius, QuickNode, or Triton recommended)
- Optional: Groq or xAI API key for the AI chat and strategy agents

---

## Installation

```bash
git clone https://github.com/Deadsg/scematica.git
cd scematica

# Build all binaries (release mode, ~5-10 min first run)
cargo build --release

# Binaries will be at:
#   target/release/sniper.exe
#   target/release/arb.exe
#   target/release/dashboard.exe
#   target/release/scematica-protocol.exe
```

> **Disk space note:** Release builds generate ~5-10 GB of artifacts. Run `cargo clean` periodically to reclaim space.

---

## Configuration

### Environment file (`.env`)

Create a `.env` in the repo root with sensitive keys:

```env
# RPC (can also be set in config.toml)
RPC_ENDPOINT=https://mainnet.helius-rpc.com/?api-key=YOUR_KEY
RPC_WS_ENDPOINT=wss://mainnet.helius-rpc.com/?api-key=YOUR_KEY

# AI (optional — enables Strategy agent and Chat tab)
GROQ_API_KEY=gsk_...
# or
XAI_API_KEY=xai-...

# Scematica gate bypass (emergency only)
# SCEMATICA_SKIP_GATE=1
```

### `config.toml` reference

Full annotated example:

```toml
[rpc]
endpoint    = "https://mainnet.helius-rpc.com/?api-key=YOUR_KEY"
ws_endpoint = "wss://mainnet.helius-rpc.com/?api-key=YOUR_KEY"
commitment  = "confirmed"   # confirmed | finalized | processed

[wallet]
# Supports: local file path, WSL UNC path, or base58 private key string
keypair_path = "C:\\Users\\you\\.config\\solana\\id.json"
# keypair_path = "\\\\wsl$\\Ubuntu\\home\\user\\.config\\solana\\id.json"

# ─── Sniper ───────────────────────────────────────────────────
[sniper]
enabled            = true
quote_mint         = "WSOL"     # WSOL or USDC
quote_amount       = 0.01       # SOL per snipe (scaled by rate mode)
buy_slippage_pct   = 1.0
sell_slippage_pct  = 20.0       # wider = faster exit on thin pools
take_profit_pct    = 100.0      # sell all when up 100%
stop_loss_pct      = 15.0       # sell all when down 15%
trailing_stop_loss_pct = 8.0    # trail 8% below peak (0 = use fixed SL)
partial_tp_pct     = 50.0       # sell 50% of position at partial TP
partial_tp_trigger = 60.0       # trigger partial TP when up 60%
price_check_interval_ms  = 1000 # sell monitor poll rate
price_check_duration_ms  = 180000  # 3 min total monitor window
max_sell_retries   = 5
max_buy_retries    = 3
auto_sell          = true
one_token_at_a_time = true      # sequential snipes (safer)
max_buys           = 10         # auto sell-mode after N buys; 0 = unlimited
max_concurrent_positions = 5    # max open positions
cooldown_after_losses = 3       # pause buys after N consecutive losses
cooldown_minutes   = 20
daily_loss_limit_sol  = 0.05    # halt if daily loss exceeds X SOL
max_drawdown_pct   = 30.0       # halt if wallet down X% from session start
blacklist_path     = "blacklist.txt"
copy_wallets       = []         # wallet addresses to copy-trade

[sniper.filters]
check_interval_ms    = 1000
check_duration_ms    = 12000    # max time to wait for filter pass
consecutive_matches  = 1
check_mint_renounced = true
check_freezable      = true
check_burned         = true
check_mutable        = true
check_socials        = false
check_name           = true     # reject scam/rug keywords in token name
min_pool_size        = 5.0      # minimum pool SOL reserve
max_pool_size        = 0.0      # 0 = no limit
check_liquidity_depth = true
max_price_impact_pct  = 5.0     # reject if our buy moves price >5%
check_volume         = false    # require recent txn activity
min_volume_txns      = 3

# ─── Arbitrage ────────────────────────────────────────────────
[arb]
enabled             = true
start_mint          = "WSOL"
start_amount        = 0.005
min_profit_lamports = 10000
max_hops            = 3
dexes               = ["Raydium", "Orca", "Meteora"]
pool_dir            = "pools"
amount_levels       = 4

# ─── Execution ────────────────────────────────────────────────
[execution]
executor           = "default"  # default | jito
custom_fee_sol     = 0.001
compute_unit_limit = 400000
compute_unit_price = 200000
skip_preflight     = true
jito_url           = "https://mainnet.block-engine.jito.wtf"

# ─── Alerts ───────────────────────────────────────────────────
[alerts]
telegram_bot_token    = ""      # leave empty to disable
telegram_chat_id      = ""
discord_webhook_url   = ""
desktop_notifications = true    # Windows toast on buy/sell
```

---

## Running

### Dashboard (recommended entry point)

```bash
# Full mode (requires config.toml + wallet)
cargo run --release --bin dashboard

# Demo mode (no keypair or RPC needed — simulated data)
cargo run --release --bin dashboard -- --demo
```

### Bots standalone

```bash
cargo run --release --bin sniper
cargo run --release --bin arb
```

### Scematica Protocol (x402 API server)

```bash
cargo run --release --bin scematica-protocol -- \
  --pay-to YOUR_WALLET_ADDRESS \
  --price-lamports 10000 \
  --bind 0.0.0.0:4020 \
  --keypair ~/.config/solana/id.json
```

---

## Dashboard Navigation

The dashboard has 5 tabs. Navigate with `Tab` / `Shift+Tab` or `→` / `←`.

### Tab 0 — Overview

Live stats panel: SOL balance, SCEMA balance, wallet address, open positions, session PnL, trade counts, and NN agent status (epsilon, total steps, win rate).

No interactive keys on this tab.

### Tab 1 — Trades

Scrollable trade history table (buy/sell events from `scematica-trades.jsonl`).

| Key | Action |
|-----|--------|
| `x` | Export trades to CSV (`scematica-trades-YYYYMMDD.csv`) |

### Tab 2 — Logs

Live log stream (tails `scematica-sniper.log` + dashboard internal events).

| Key | Action |
|-----|--------|
| `e` | Toggle **Sell Mode** — pauses all buys, sells all open positions |
| `d` | Toggle **Dump Mode** — force-sells everything at zero slippage |
| `/` | Activate log filter (type to search, `Backspace` to clear, `Esc` to exit filter) |

### Tab 3 — Control

Bot process control and rate mode selection.

| Key | Action |
|-----|--------|
| `s` | Start **Sniper** only |
| `a` | Start **Arb** only |
| `b` | Start **Both** (sniper + arb) |
| `x` | **Stop** all bots |
| `1` | Rate mode: **Safe** — 0.5x, TP 50%, SL 10% |
| `2` | Rate mode: **Balanced** — 1.0x, TP 100%, SL 15% |
| `3` | Rate mode: **Aggressive** — 2.0x, TP 200%, SL 20% |
| `4` | Rate mode: **Degen** — 3.0x, TP 300%, SL 30% |

### Tab 4 — Chat

AI assistant powered by Groq (Llama) or xAI (Grok). Requires `GROQ_API_KEY` or `XAI_API_KEY`.

| Key | Action |
|-----|--------|
| Type | Compose message |
| `Enter` | Send message |
| `Backspace` | Delete character |
| `y` | Confirm a pending AI action (shown when bot proposes a trade) |
| `n` | Reject a pending AI action |

### Global keys

| Key | Action |
|-----|--------|
| `q` / `Esc` | Quit dashboard |
| `Tab` / `→` | Next tab |
| `Shift+Tab` / `←` | Previous tab |
| `Ctrl+C` | Force quit |

---

## Sell Mechanics

The sniper uses a two-phase sell monitor per position:

1. **Fast phase** (first 20 checks × 100ms): catches rapid dumps immediately after buy
2. **Slow phase** (remaining time × `price_check_interval_ms`): standard monitoring

Each iteration:
- Re-reads `live_params` for dynamic TP/SL (updated by rate mode or config hot-reload)
- Checks trailing stop — resets peak price on new highs
- Triggers partial TP at `partial_tp_trigger`% gain (sells `partial_tp_pct`% of position)
- Detects dump: 3 consecutive declining prices in fast phase → immediate exit
- Falls back to AMM constant-product price: `out = (reserve_out × in × 9975) / (reserve_in × 10000 + in × 9975)`

**Emergency controls:**
- **Sell Mode** (`e` key or `scematica-sell-mode.json`): pauses all buys, sell-scans all wallet positions
- **Dump Mode** (`d` key or `scematica-dump-mode.json`): `min_out = 0`, retries every 30s until all positions are gone
- **Max drawdown guard**: auto-activates sell mode if wallet drops `max_drawdown_pct` from session start
- **Daily loss limit**: halts new buys if daily SOL loss exceeds `daily_loss_limit_sol`

---

## Neural Network Agent (scematica-nn)

The Deep Q* agent runs inside the sniper process and learns from every completed trade.

- **State**: PnL%, position age, price momentum, win/loss streaks, liquidity score, open positions
- **Actions**: BuyStandard, BuySized, SellAll, SellPartial, Hold
- **Reward shaping**: +PnL%, −time penalty, win streak bonus, loss streak penalty
- **Training**: experience replay buffer (10,000 samples), target network (sync every 100 steps)
- **Checkpoints**: saved to `scematica-nn-agent.json` every 10 minutes
- **Stats**: written to `scematica-nn-stats.json` every 30s (visible in Overview tab)
- **Regime shift**: when the Strategy Agent detects a regime change, epsilon is spiked so the NN re-explores

---

## Scematica Protocol

A Rust-native implementation of the [x402 HTTP payment standard](https://github.com/x402-foundation/x402) for Solana.

Clients pay per API call with a micro-SOL transfer embedded in the `X-Payment` request header. No subscription, no API key — just pay-per-use on-chain.

### Paid endpoints

| Route | Description |
|---|---|
| `GET /signals/pools` | Live pool signals from the sniper stream |
| `GET /signals/trades` | Recent trade events |
| `GET /stats/nn` | NN agent performance stats |
| `GET /stats/metrics` | Bot metrics snapshot |

### Free endpoints

| Route | Description |
|---|---|
| `GET /health` | Liveness check |
| `GET /supported` | Payment requirements (asset, amount, destination) |

### How it works

1. Client requests a paid route → server returns `402 Payment Required` with `X-Payment-Response` header
2. Client builds a partial SPL `TransferChecked` transaction signed by their key
3. Client base64-encodes the tx and includes it in the `X-Payment` header of the next request
4. Server verifies the partial tx (mint, destination, amount, client sig)
5. Server refreshes blockhash, signs as fee payer, submits — then returns the API response

---

## State Files

The sniper and dashboard communicate via JSON files in the working directory:

| File | Written by | Purpose |
|---|---|---|
| `scematica-sell-mode.json` | Dashboard / drawdown guard | Activates emergency sell mode |
| `scematica-dump-mode.json` | Dashboard | Activates dump mode (zero slippage) |
| `scematica-rate-mode.json` | Dashboard | Active rate mode + TP/SL/multiplier |
| `pool-cache.json` | Sniper | Pool → mint mapping for sell lookups |
| `scematica-trades.jsonl` | Sniper | Trade history (append-only JSONL) |
| `scematica-sniper.log` | Sniper | Log file tailed by dashboard |
| `scematica-nn-agent.json` | NN agent | Model checkpoint |
| `scematica-nn-stats.json` | NN agent | Stats for dashboard display |
| `scematica-filter-stats.json` | Filter pipeline | Per-filter pass/fail counts |

---

## Security

- Private keys never leave your machine
- All SCEMA gate checks retry up to 5 times before failing (set `SCEMATICA_SKIP_GATE=1` to bypass during RPC outages)
- Arbitrage uses the `scematica-swap` on-chain program with profit-or-revert: if the arb is not profitable, the transaction fails before any funds move
- The Protocol server verifies every payment before settling — partial tx is validated for correct mint, destination, and amount

---

## License

MIT — see [LICENSE](LICENSE).
