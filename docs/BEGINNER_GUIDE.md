# Scematica Beginner's Guide
### Complete Setup Walkthrough — No Coding Experience Needed

This guide walks you through everything from a fresh Windows computer to running the Scematica trading bot. Each step has exact instructions. If something goes wrong, the Troubleshooting section at the bottom has answers.

---

## Before You Start — What You Need

Before spending any time on setup, make sure you have:

- A Windows 10 or Windows 11 computer
- At least 20 GB of free disk space (the build process is large)
- A stable internet connection
- A Solana wallet with **at least 0.1 SOL** (for transaction fees)
- **250,000 SCEMA tokens** (required to run the bot — see Step 5)
- A Helius RPC API key (free tier works for testing — see Step 6)

---

## Easiest Path — Install from crates.io (No Download, No Build)

If you just want to **see the apps run**, you don't need to download the code or
build anything. Once Rust is installed (Step 1 below), open a terminal and run
one line. Every command here works with **no wallet, no RPC, and no tokens**:

```bash
# The bot dashboard, with demo data
cargo install scematica-dashboard
dashboard --demo

# Watch the Deep Q* AI agent learn, live
cargo install scematica-nn
scema-ddqn

# The ScemaDEX agentic-liquidity viewer (offline)
cargo install scemadex-sdk
scemadex
```

The first `cargo install` of each takes a few minutes (it compiles once, then
the command stays installed). Press **`q`** to quit any viewer.

**Want everything behind one command?** Install the suite launcher:

```bash
cargo install scematica-suite
scematica help                 # lists every command
scematica dashboard --demo     # run the dashboard
scematica ddqn                 # run the DQ* training viewer
scematica scemadex             # run the ScemaDEX viewer
```

To run the **live trading bot** (not demo), you still need 250k SCEMA, an RPC
key, and a funded wallet — keep reading for that full setup. For just trying
things out, the commands above are all you need.

---

## Fast Path — One-Click Scripts (Recommended)

Scematica ships Windows batch scripts that automate everything after the
toolchain is installed. **You only need to do the manual install for Rust, Git,
and Build Tools (Steps 1–3); the scripts handle the rest.**

Once Rust + Git are installed and you've downloaded the code (Step 4),
double-click these in order from the project folder:

| Order | Double-click | What it does |
|-------|--------------|--------------|
| 1️⃣ | **`init.bat`** | One-time setup: checks your toolchain, adds the `rustfmt`/`clippy` tools, downloads every dependency, and creates a `.env` template for you to fill in. |
| 2️⃣ | **`build.bat`** | Compiles all the programs (5–10 minutes the first time). |
| 3️⃣ | **`verify-setup.bat`** | Confirms everything is ready before you go live. |
| 4️⃣ | **`start-dashboard-demo.bat`** | Launches the bot dashboard in **demo mode** — no tokens or RPC needed. Try this first! |

Then, when you're ready for the real thing:

| Double-click | What it does |
|--------------|--------------|
| **`start-dashboard.bat`** | Full bot mode (needs 250,000 SCEMA + an RPC endpoint in `.env`). |
| **`start-sdk-dashboard.bat`** | The ScemaDEX SDK dashboard (SIM mode by default — fully offline). |
| **`start-relay.bat`** | Runs the ScemaDEX peer-mesh + signal relay. |

> **Tip:** After running `init.bat`, open the `.env` file it created and paste in
> your RPC endpoint and wallet path before using full mode. Demo and SIM modes
> need nothing.

If a script reports something missing, the detailed manual steps below explain
exactly how to install it. **New users: do Steps 1–4, then use the scripts
above.**

---

## Step 1 — Install Rust

Rust is the programming language Scematica is written in. You need it to build the project.

1. Open your web browser and go to: **https://rustup.rs**
2. Click the **"64-bit"** download button. A file called `rustup-init.exe` will download.
3. Double-click `rustup-init.exe` to run it.
4. A black terminal window opens. When it asks what to do, type `1` and press **Enter**.
5. Wait for the installation to finish. It will say **"Rust is installed now. Great!"** when done.
6. **Close the terminal window and open a new one.** This is important — Rust won't work in the old window.

**How to open a terminal:** Press the Windows key, type `PowerShell`, right-click **Windows PowerShell**, and choose **Run as administrator**.

**Verify it worked:** Type this and press Enter:
```
rustc --version
```
You should see something like `rustc 1.78.0`. If you see that, Rust is installed correctly.

---

## Step 2 — Install Git

Git is a tool for downloading code from the internet.

1. Go to: **https://git-scm.com/download/win**
2. The download should start automatically. If not, click the link for the **64-bit** version.
3. Run the downloaded file (called something like `Git-2.xx.x-64-bit.exe`).
4. Click **Next** through all the screens — the default options are fine. Click **Install** at the end.
5. Click **Finish** when done.

**Verify it worked:** Open a new PowerShell window and type:
```
git --version
```
You should see something like `git version 2.44.0`. 

---

## Step 3 — Install Visual Studio Build Tools

Rust needs Windows build tools to compile code. This is a one-time setup.

1. Go to: **https://visualstudio.microsoft.com/visual-cpp-build-tools/**
2. Click **Download Build Tools**.
3. Run the downloaded file (`vs_BuildTools.exe`).
4. When the installer opens, check the box for **"Desktop development with C++"**.
5. Click **Install** and wait. This takes 5-15 minutes.
6. Restart your computer when the installation finishes.

---

## Step 4 — Download the Scematica Code

1. Open PowerShell as administrator (see Step 1 for how).
2. Decide where you want to save the project. The default Documents folder works well.
3. Type these commands one at a time, pressing Enter after each:

```powershell
cd "$env:USERPROFILE\Documents"
git clone https://github.com/Meta-Oracle/Scematica.git
cd Scematica
```

You should now see the project files. Type `ls` and press Enter — you should see folders like `crates`, `tools`, and files like `Cargo.toml`.

---

## Step 5 — Get a Solana Wallet and SCEMA Tokens

The bot needs a Solana wallet to trade with. It also requires 250,000 SCEMA tokens to start.

### Setting up a Solana wallet

If you already have a Phantom or Solflare wallet, you can export your private key from there. If you don't have one:

1. Go to **https://phantom.app** and install the Phantom browser extension.
2. Click **Create New Wallet** and follow the prompts.
3. **Write down your recovery phrase on paper and keep it safe.** This is your backup.

### Exporting your private key from Phantom

The bot needs your private key as a file. Here's how to export it:

1. Open Phantom and click the **gear icon** (Settings) at the bottom.
2. Click **Security & Privacy**, then **Export Private Key**.
3. Enter your password.
4. You'll see a long string of letters and numbers. Copy it.

Now save it as a file:

1. In PowerShell, type (replace `YOUR_PRIVATE_KEY_HERE` with the key you copied):
```powershell
echo "YOUR_PRIVATE_KEY_HERE" > "$env:USERPROFILE\Documents\my-wallet.txt"
```

> **Security warning:** Never share this file or the key inside it with anyone. Anyone who has it can take all your funds.

### Getting SCEMA tokens

You need 250,000 SCEMA tokens to use the bot. SCEMA's mint address is:
```
AbKiP2Jc6nM7937jTDfqoJC1bsg5FQ24Buk2iqRFpump
```

You can buy SCEMA on pump.fun or Raydium by searching for this mint address. Make sure you're buying the right token — always verify the mint address.

---

## Step 6 — Get a Helius RPC Endpoint

The bot connects to the Solana blockchain through a service called an RPC endpoint. Helius provides fast, reliable access.

1. Go to **https://helius.dev** and create a free account.
2. After logging in, click **Create New Project** (or similar — the UI may vary).
3. Choose **Mainnet**.
4. Copy your **API Key** — it looks like `abc12345-def6-7890-...`
5. Your RPC URL will be: `https://mainnet.helius-rpc.com/?api-key=YOUR_API_KEY`
6. Your WebSocket URL will be: `wss://mainnet.helius-rpc.com/?api-key=YOUR_API_KEY`

Write these down — you'll need them in the next step.

---

## Step 7 — Create Your Configuration File

The bot reads its settings from a file called `config.toml`. You need to create this.

1. In PowerShell, navigate to the scematica folder:
```powershell
cd "$env:USERPROFILE\Documents\scematica"
```

2. Copy the example config:
```powershell
copy config.example.toml config.toml
```

3. Open `config.toml` in Notepad:
```powershell
notepad config.toml
```

4. Find and edit these key sections (press Ctrl+F to search):

**Your wallet:**
```toml
[wallet]
keypair_path = "C:\\Users\\YourName\\Documents\\my-wallet.txt"
```
Replace `YourName` with your actual Windows username.

**Your RPC endpoints:**
```toml
[rpc]
http_url = "https://mainnet.helius-rpc.com/?api-key=YOUR_API_KEY"
ws_url = "wss://mainnet.helius-rpc.com/?api-key=YOUR_API_KEY"
```
Replace `YOUR_API_KEY` with your Helius API key from Step 6.

**Buy amount (start small!):**
```toml
[sniper]
quote_amount_sol = 0.01
```
This controls how much SOL the bot spends per trade. **Start with 0.01 SOL while learning.**

5. Press **Ctrl+S** to save and close Notepad.

---

## Step 8 — Create the Environment File

Some settings go in a separate `.env` file. This is a security practice — it keeps your API keys out of the main config.

1. In PowerShell, type:
```powershell
notepad .env
```

2. Notepad will ask if you want to create a new file — click **Yes**.

3. Add these lines (replacing with your actual values):
```
HELIUS_API_KEY=your_helius_api_key_here
```

4. Save and close.

---

## Step 9 — Build the Project

This compiles the Scematica code into programs your computer can run. **This takes 10-30 minutes the first time.** Your computer will work hard — this is normal.

> **Shortcut:** instead of the commands below, you can just double-click
> **`init.bat`** (one-time dependency setup) and then **`build.bat`**. They run
> exactly these steps for you. The manual version is below if you prefer it.

1. In PowerShell, make sure you're in the scematica folder:
```powershell
cd "$env:USERPROFILE\Documents\scematica"
```

2. Run the build:
```powershell
cargo build --release
```

3. Watch the output scroll by. If it ends with something like:
```
Finished release [optimized] target(s) in 14m 32s
```
...the build succeeded. If you see any red `error:` lines, see the Troubleshooting section.

---

## Step 10 — Test with Demo Mode

Before using real money, test the dashboard in demo mode. Demo mode runs without a real wallet or RPC connection.

```powershell
cargo run --release --bin dashboard -- --demo
```

A colorful terminal interface (called the TUI) should appear. You should see tabs at the top: Overview, Sniper, Positions, NN Agent, Arb, Logs.

Press **Tab** to switch between tabs. Press **Q** to quit.

If the dashboard appears and you can navigate it, your setup is working.

---

## Step 11 — Run the Real Dashboard

When you're ready to use the bot with real funds:

```powershell
cargo run --release --bin dashboard
```

The dashboard will:
1. Check your SCEMA token balance (must be ≥ 250,000)
2. Connect to Solana via your RPC endpoint
3. Start the sniper process in the background
4. Show you the live TUI

### Dashboard Controls

| Key | Action |
|-----|--------|
| `Tab` / `Shift+Tab` | Switch between tabs |
| `B` | Toggle buy mode on/off |
| `S` | Sell all open positions immediately |
| `D` | Dump mode (force-sell, no slippage protection) |
| `+` / `-` | Increase / decrease buy amount |
| `Q` | Quit |

---

## Step 12 — Understanding the Tabs

**Overview tab** — Live status: wallet balance, active positions, recent alerts, session profit/loss.

**Sniper tab** — Shows pools being detected and filter decisions. Each pool goes through a filter pipeline before the bot decides to buy.

**Positions tab** — Currently open trades with entry price, current value, and profit/loss.

**NN Agent tab** — The neural network that learns from trades. Shows its current confidence (epsilon), training progress, and Q-values per action.

**Arb tab** — Cross-DEX arbitrage opportunities found between Raydium and Orca.

**Logs tab** — Raw log output from the sniper process.

---

## Understanding the Risk

**This bot trades real money automatically. You can lose all funds you give it.**

Important things to understand:
- Start with the smallest possible buy amount (`quote_amount_sol = 0.01` or less)
- The bot uses stop-loss settings — make sure `stop_loss_pct` is set in your config (e.g., `15` for 15%)
- New tokens on Solana are extremely high risk — many are scams ("rug pulls")
- The neural network agent improves over time but starts with no knowledge
- Never put money into this bot that you cannot afford to lose entirely

---

## Troubleshooting

### "error: linker `link.exe` not found"
You need to install Visual Studio Build Tools (Step 3). Make sure you checked "Desktop development with C++" during installation.

### "error: could not compile" or red error lines during build
1. Make sure you have at least 20 GB of free disk space.
2. Try running `cargo clean` and then `cargo build --release` again.
3. Make sure your internet connection is stable — the build downloads dependencies.

### Dashboard shows "Token gate check failed"
Your wallet doesn't have 250,000 SCEMA tokens. Check your balance on Phantom. If you're testing and want to skip the check temporarily:
```powershell
$env:SCEMATICA_SKIP_GATE = "1"
cargo run --release --bin dashboard
```

### Dashboard shows "RPC connection failed"
1. Check your `config.toml` — make sure the `http_url` and `ws_url` are correct.
2. Make sure your Helius API key is valid. Log in to helius.dev and check.
3. Try the demo mode (`--demo`) to confirm the dashboard itself works.

### "Access is denied" when building
Another program is using a file the build needs. Check if a previous sniper is still running:
```powershell
tasklist | Select-String "sniper"
```
If you see `sniper.exe`, kill it:
```powershell
taskkill /F /IM sniper.exe
```
Then try building again.

### The bot bought something and now I can't sell
Open the dashboard, go to Positions tab, and press `S` to sell all. If that doesn't work, press `D` for dump mode (this sells immediately with no slippage protection, which may result in a worse price but guarantees the sell goes through).

### I want to stop the bot completely
1. Press `Q` in the dashboard.
2. The sniper process should stop automatically. If it doesn't:
```powershell
taskkill /F /IM sniper.exe
```

---

---

## Web Dashboard

The web dashboard is a browser-based interface for the sniper. It shows live metrics, pool radar, trade history, PnL chart, log stream, and control buttons — all in a modern dark UI you can open in any browser. It works alongside the sniper and connects your Phantom wallet to gate access and collect per-trade fees.

> **TUI vs Web:** The ratatui TUI dashboard (Steps 10–12) and the web dashboard are two separate ways to monitor and control the same sniper process. You can use one or both — they read the same files.

---

### Web Step 1 — Install Node.js

The web dashboard is a Next.js app, which requires Node.js.

1. Go to **https://nodejs.org** and download the **LTS** version (the left button — "Recommended For Most Users").
2. Run the downloaded installer (`node-vXX.X.X-x64.msi`).
3. Click **Next** through all screens — defaults are fine. Click **Install**, then **Finish**.
4. Close any open PowerShell windows and open a new one.

**Verify it worked:**
```powershell
node --version
npm --version
```
You should see version numbers for both (e.g. `v20.11.0` and `10.4.0`).

---

### Web Step 2 — Install Web Dependencies

The web app is inside the `web` folder in the project.

```powershell
cd "$env:USERPROFILE\Documents\scematica\web"
npm install
```

This downloads all the packages the web app needs. It takes 1–3 minutes and only needs to be done once (or again after updates).

---

### Web Step 3 — Configure the RPC Endpoint (Optional)

By default the web app uses a Helius RPC endpoint already embedded in the code. If you want it to use your own Helius key (recommended for production), create a local environment file:

1. In PowerShell, while in the `web` folder:
```powershell
notepad .env.local
```
2. Notepad will ask to create a new file — click **Yes**.
3. Add this line (replace with your Helius API key from Step 6 of the main guide):
```
NEXT_PUBLIC_RPC_ENDPOINT=https://mainnet.helius-rpc.com/?api-key=YOUR_API_KEY
```
4. Save and close.

> **Why this matters:** The web app connects your Phantom wallet to Solana mainnet to check your SCEMA balance and send per-trade fees. Using your own RPC key prevents rate-limit issues.

---

### Web Step 4 — Build and Run the Rust API Server

The web dashboard talks to a small Rust HTTP server that reads the sniper's data files and accepts control commands. You need to run this alongside the sniper.

**Build it once** (if you haven't already built the full project):
```powershell
cd "$env:USERPROFILE\Documents\scematica"
cargo build --release --bin api
```

**Run the API server** (keep this window open):
```powershell
cd "$env:USERPROFILE\Documents\scematica"
.\target\release\api.exe
```

You should see:
```
INFO scematica_api: Scematica API listening on http://0.0.0.0:3001
```

> The API server runs on **port 3001**. Leave this terminal window open — closing it stops the API.

---

### Web Step 5 — Start the Sniper

The web dashboard shows live data from the sniper. Open a second terminal window and start the sniper:

```powershell
cd "$env:USERPROFILE\Documents\scematica"
.\target\release\sniper.exe
```

Or if you prefer to use the TUI dashboard at the same time, start that instead — it also manages the sniper:
```powershell
.\target\release\dashboard.exe
```

> The web dashboard and TUI dashboard can run simultaneously. Both read the same sniper data files.

---

### Web Step 6 — Start the Web Dashboard

Open a third terminal window and run:

```powershell
cd "$env:USERPROFILE\Documents\scematica\web"
npm run dev
```

You should see:
```
  ▲ Next.js 14.x.x
  - Local:        http://localhost:3000
  - Ready in 2.3s
```

Now open **http://localhost:3000** in your browser (Chrome or Firefox recommended).

**For a production-grade server** (faster, no dev overhead):
```powershell
npm run build
npm start
```

---

### Web Step 7 — Connect Your Phantom Wallet

When you open the web dashboard you'll see the Scematica interface. In the top-right corner:

1. Click the **Select Wallet** button.
2. Choose **Phantom** from the popup.
3. Approve the connection in Phantom.

The header will now show your wallet address, your SOL balance, and your SCEMA balance.

> **Phantom required:** Install Phantom from **https://phantom.app** if you don't have it. Backpack and Solflare also work — they auto-register via the Wallet Standard protocol.

---

### Web Step 8 — The SCEMA Token Gate

The **Controls** section of the web dashboard (Sell Mode, Dump Mode, High Speed, and rate buttons) requires you to hold **250,000 SCEMA** in your connected wallet.

If your wallet has enough SCEMA, the controls unlock immediately and the header shows **GATED ✓** in green.

If you don't have enough, the controls section shows:
- How many SCEMA you currently hold
- How many more you need
- A **Buy $SCEMA on Jupiter** link

The gate re-checks your balance every 15 seconds automatically.

**To get SCEMA:**
- Mint address: `AbKiP2Jc6nM7937jTDfqoJC1bsg5FQ24Buk2iqRFpump`
- Buy on Jupiter (https://jup.ag) or pump.fun — search the mint address above.
- Always verify the mint address before buying.

---

### Web Step 9 — Using the Controls

Once gated, the **Controls** panel gives you these buttons (keyboard shortcuts work anywhere on the page):

| Button | Key | What it does |
|--------|-----|--------------|
| SELL | `S` | Pauses new buys, sells all open positions through normal TP/SL |
| DUMP | `D` | Force-sells everything immediately with no slippage protection |
| FAST | `H` | High-speed mode — bypasses slower filters for faster entries |
| CONSERVATIVE | `1` | 0.5× position size multiplier |
| NORMAL | `2` | 1× multiplier (default) |
| AGGRESSIVE | `3` | 2× multiplier |
| RUNNER | `4` | 3× multiplier |
| Cycle mode | `R` | Cycles through rate modes in order |

> **Keyboard shortcuts** work as long as you are not typing in a text field. Press the key anywhere on the page.

---

### Web Step 10 — Understanding the Panels

**Live Metrics** — Five cards at the top showing session PnL, trade counts, win/loss ratio, pools tracked, and uptime. Updates every 3 seconds.

**Deep Q* Agent** — Shows the neural network's training progress: steps trained, epsilon (how exploratory vs. exploitative), replay buffer size, and total reward signal.

**Pool Radar** — Live feed of every pool detected on Raydium. Shows score, liquidity, and age. Green rows passed the filter pipeline; dim rows were rejected.

**Filter Stats** — Counts how many pools each filter has rejected. Useful for tuning — if one filter is blocking nearly everything, it may be too strict.

**Trade History** — Every buy and sell the sniper has made this session, newest first. Shows time, token, type, SOL amount, and PnL percentage.

**Open Positions** — Tokens the sniper currently holds. Shows entry amount and how long the position has been open.

**PnL Curve** — Cumulative profit/loss over the session as a chart. Green line = net positive, red = net negative.

**Log Stream** — Live log output from the sniper process, same as the TUI Logs tab.

---

### Web Step 11 — The Fee System

Every time the sniper confirms a sell trade, the web dashboard tracks a small fee owed:

- **1% of the SOL received** from that sell
- **1% of the same value in $SCEMA** (priced live via Jupiter)

Fees from multiple trades are batched together and sent in a single transaction when the total reaches **0.0005 SOL** — or you can click the fee indicator in the header to pay immediately at any time.

Both the SOL fee and the SCEMA fee go out in a single Solana transaction signed by your Phantom wallet. Phantom will prompt you to approve it.

> The fee only applies to **confirmed sell trades** — not buys, not failed trades, and not arb trades.

---

### Web Troubleshooting

**"API server unreachable" / panels show no data**
The Rust API server isn't running. Open a new terminal and start it:
```powershell
cd "$env:USERPROFILE\Documents\scematica"
.\target\release\api.exe
```

**Health badge shows red / sniper offline**
The sniper isn't running, or it crashed. Start it:
```powershell
.\target\release\sniper.exe
```

**Controls show "Insufficient $SCEMA"**
Your connected Phantom wallet doesn't have 250,000 SCEMA. Buy SCEMA on Jupiter using the mint address `AbKiP2Jc6nM7937jTDfqoJC1bsg5FQ24Buk2iqRFpump` and then reconnect.

**Panels show data but controls do nothing**
Make sure the API server (`api.exe`) is running — it's what receives control POST requests from the web UI.

**"npm: command not found" in PowerShell**
Close all PowerShell windows and reopen one. Node.js updates the system PATH on install, but existing windows don't see it. If still not found, restart your computer.

**Web dashboard is blank or shows a React error**
Run `npm run build` in the `web` folder and look at the error output — it will tell you what's wrong. Common cause: a missing package. Fix with `npm install`.

**Phantom wallet shows wrong SCEMA balance**
The web app queries on-chain state directly — it bypasses Phantom's cached balance. If Phantom shows a different number, the web app is correct.

---

### Web Quick Reference

```powershell
# Open the web folder
cd "$env:USERPROFILE\Documents\scematica\web"

# Install dependencies (once, or after updates)
npm install

# Start the web dashboard (development)
npm run dev

# Start the web dashboard (production — faster)
npm run build
npm start

# Start the Rust API server (separate terminal, from project root)
cd "$env:USERPROFILE\Documents\scematica"
.\target\release\api.exe
```

Three processes to keep running simultaneously:
1. **Sniper** — `.\target\release\sniper.exe` (or `dashboard.exe` for the TUI)
2. **API server** — `.\target\release\api.exe`
3. **Web app** — `npm run dev` (or `npm start` after a build)

Then open **http://localhost:3000** in your browser.

---

## Getting Help

- **GitHub Issues:** https://github.com/Meta-Oracle/Scematica/issues
- **README.md:** Full technical documentation in the project root

For questions, open a GitHub issue with:
1. What you were trying to do
2. What you expected to happen
3. What actually happened (copy the error message)

---

## Quick Reference: Most Common Commands

```powershell
# Navigate to the project folder
cd "$env:USERPROFILE\Documents\scematica"

# Build everything (do this after any code changes)
cargo build --release

# Run in demo mode (no real money, for testing)
cargo run --release --bin dashboard -- --demo

# Run the TUI dashboard
cargo run --release --bin dashboard

# Run just the sniper (no TUI)
.\target\release\sniper.exe

# Run the Rust API server (required for web dashboard)
.\target\release\api.exe

# Kill the sniper if it gets stuck
taskkill /F /IM sniper.exe

# Check if sniper is running
tasklist | Select-String "sniper"

# ── Web dashboard ─────────────────────────────────────────────
cd "$env:USERPROFILE\Documents\scematica\web"

# Install web dependencies (once)
npm install

# Start web dashboard — dev mode
npm run dev

# Start web dashboard — production mode
npm run build && npm start
# Then open http://localhost:3000 in your browser
```
