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
git clone https://github.com/Deadsg/scematica.git
cd scematica
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

## Getting Help

- **GitHub Issues:** https://github.com/Deadsg/scematica/issues
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

# Build the project (do this after any code changes)
cargo build --release

# Run in demo mode (no real money, for testing)
cargo run --release --bin dashboard -- --demo

# Run the real dashboard
cargo run --release --bin dashboard

# Kill the sniper if it gets stuck
taskkill /F /IM sniper.exe

# Check if sniper is running
tasklist | Select-String "sniper"
```
