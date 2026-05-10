# 🌌 Scematica

**Next-Generation Autonomous AI Trading Infrastructure for Solana.**

Scematica is a modular, multi-strategy trading bot system that unifies token sniping, multi-DEX arbitrage, and agentic market analysis into a single extensible platform. Powered by an advanced AI layer, it doesn't just execute trades—it evaluates them through a multi-agent consensus framework.

---

## 🚀 Vision

The future of trading is adaptive, autonomous, and predictive. Scematica is engineered to bridge the gap between static algorithmic trading and agentic intelligence, enabling high-performance execution on Solana with institutional-grade risk management.

## 🏗️ Architecture

Scematica is built as a highly modular Rust workspace:

- **`scematica-core`**: The backbone. Shared types, configuration management, RPC wrappers, and wallet utilities.
- **`scematica-ai`**: The brain. A sophisticated agentic framework featuring:
    - **Risk Agent**: Evaluates new tokens for rug/honeypot risk.
    - **Arb Agent**: Scores arbitrage paths for executability and profit.
    - **Debate Agent**: A multi-agent "Bull vs. Bear" debate system for high-stakes decisions.
    - **Strategy Agent**: Dynamically adjusts TP/SL based on market regimes.
    - **Report Agent**: Generates natural language performance reports.
- **`scematica-sniper`**: High-speed token sniper with advanced filters and AI integration.
- **`scematica-arb`**: Cross-DEX arbitrage engine using a custom graph-based pathfinder.
- **`scematica-executor`**: Multi-DEX execution layer supporting Raydium, Orca, Meteora, and Jupiter.
- **`scematica-dashboard`**: A real-time TUI for monitoring bot performance and market health.
- **`programs/scematica-swap`**: An on-chain Anchor program implementing the **profit-or-revert** pattern for safe atomic swaps.

## ✨ Key Features

- **AI-Powered Decision Making**: Every trade can be vetted by LLM-based agents (Llama 3.3 via Groq/OpenRouter).
- **Atomic Arbitrage**: Profit-or-revert guarantees—if a trade isn't profitable, the transaction fails before funds are moved.
- **Multi-DEX Support**: Native integration with Raydium (V4/Amm), Orca (Whirlpool), and Meteora (DLMM).
- **Advanced Sniper Filters**: Mint renounced check, freeze authority detection, LP burn verification, and metadata analysis.
- **Dynamic Risk Management**: AI-driven TP/SL adjustments and position sizing.
- **Jito Integration**: Support for Jito bundles to avoid front-running and land transactions reliably.

---

## 🛠️ Getting Started

### Prerequisites

- [Rust](https://rustup.rs/) (latest stable)
- [Solana CLI](https://docs.solana.com/cli/install-solana-cli-tools)
- [Anchor Framework](https://www.anchor-lang.com/docs/installation) (if modifying on-chain programs)
- A Groq API Key (Free tier available at [console.groq.com](https://console.groq.com))

### Installation

1. **Clone the repository:**
   ```bash
   git clone https://github.com/your-repo/scematica.git
   cd scematica
   ```

2. **Setup environment variables:**
   ```bash
   cp .env.example .env
   # Edit .env with your RPC URL and API keys
   ```

3. **Install dependencies and build:**
   ```bash
   cargo build --release
   ```

### Configuration

Scematica uses a dual-layer configuration system:
- **`.env`**: For sensitive keys (RPC URLs, Wallet paths, AI API keys).
- **`config.toml`**: For strategy parameters (TP/SL, amounts, filters).

---

## 🏃 Running the Bot

### 1. Token Sniper
Detect and buy new tokens based on strict filters and AI risk scoring.
```bash
cargo run --release --bin sniper -- --config config.toml
```

### 2. Arbitrage Engine
Search and execute cross-DEX arbitrage opportunities.
```bash
cargo run --release --bin arb -- --config config.toml
```

### 3. Dashboard
Monitor all bot activities in a real-time TUI.
```bash
cargo run --release --bin dashboard
```

---

## ⛓️ On-Chain Program

Scematica includes an Anchor program (`scematica-swap`) that ensures atomic profitability for arbitrage.

### Deployment to Mainnet

1. **Build the program:**
   ```bash
   anchor build
   ```

2. **Get your Program ID:**
   ```bash
   solana address -k target/deploy/scematica_swap-keypair.json
   ```

3. **Update Program IDs:**
   - Update `declare_id!` in `programs/scematica-swap/src/lib.rs`.
   - Update `[programs.mainnet]` in `Anchor.toml`.
   - Update `SWAP_PROGRAM_ID` in your `.env`.

4. **Deploy:**
   ```bash
   anchor deploy --provider.cluster mainnet
   ```

---

## 💰 Funding & Testing

- **Profit-or-Revert**: The on-chain `scematica-swap` program ensures that you can never lose capital due to slippage or front-running on an arb (only the transaction fee).
- **Local Keys**: Your private keys never leave your machine.
- **Fail-Safe AI**: If the AI layer is unavailable, the bot falls back to conservative, rule-based filters.

## 🗺️ Roadmap

- **Q2 2026**: Multi-agent "Debate" system for trade validation. (✅ Completed)
- **Q3 2026**: Predictive market regime detection and automatic strategy switching.
- **Q4 2026**: Social sentiment integration via X/Telegram scrapers.
- **Q1 2027**: Fully autonomous "Agentic Mode" where the bot manages its own portfolio.

## 📄 License

MIT License. See [LICENSE](LICENSE) for more information.
