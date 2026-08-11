# scema-botchain/contracts

Foundry project for Scematica's on-chain pieces on **BOT Chain mainnet (chain 677)**.

Separate from `programs/` — those are Anchor programs for Solana and are built with
`anchor build`. Nothing is shared between the two beyond intent.

## Setup

Foundry is not currently installed on this machine. From Git Bash:

```bash
curl -L https://foundry.paradigm.xyz | bash
foundryup
```

Then:

```bash
cd scema-botchain/contracts
cp .env.example .env          # fill it in; .env is gitignored
forge install foundry-rs/forge-std --no-git
forge build
forge test -vvv
```

## Signing: use a keystore, not a dotfile

```bash
cast wallet import botchain-deployer --interactive
```

A `PRIVATE_KEY=` line in `.env` is one `cat` away from a screen share and lands in shell
history the moment it is exported. The keystore is encrypted at rest and prompts at use.
Deployments here move real BOT.

## Chain facts (verified, not copied from docs)

| | |
|---|---|
| Chain ID | 677 |
| RPC | `https://rpc.botchain.ai` (~615 ms) |
| Explorer | `https://scan.botchain.ai` (Blockscout) |
| Native | BOT, 18 decimals |
| Block time | 0.67 s measured |
| Gas limit | 35,000,000 |
| `baseFeePerGas` | **0** — BSC-style; use `--legacy`, and price with `eth_gasPrice` (~20 gwei) |
| EVM forks | Shanghai **and** Cancun active — `withdrawalsRoot` and `blobGasUsed` both present |

That last row is why `foundry.toml` pins `evm_version = "shanghai"` rather than guessing.
On BSC-derived chains that predate Shanghai, solc 0.8.20+ emits `PUSH0`, the deploy
transaction succeeds, and then every call reverts — a failure that looks like a bug in
your contract rather than an EVM mismatch. Shanghai is confirmed supported here; Cancun
is too, but nothing needs `TSTORE`/`MCOPY` and the narrower target keeps the bytecode
portable to other BSC forks.

## Deploy

**Testnet first, always.** Chain 968, funded from the BOT Chain faucet. Note 968 is not a
unique chain id (ChainList registers it as Datagram, and it is BSC's Rialto), so confirm
you are on the right network before trusting anything:

```bash
cast chain-id --rpc-url $BOTCHAIN_TESTNET_RPC_URL     # expect 968
forge script script/Deploy.s.sol --rpc-url botchain_testnet --account botchain-deployer --broadcast --legacy
```

Mainnet, only after the testnet deploy has been exercised:

```bash
# 1. Dry run — simulates without broadcasting. Read the gas estimate.
forge script script/Deploy.s.sol --rpc-url botchain --account botchain-deployer --legacy

# 2. Broadcast. Irreversible.
forge script script/Deploy.s.sol --rpc-url botchain --account botchain-deployer --broadcast --legacy

# 3. Verify source on Blockscout
forge verify-contract <ADDRESS> src/<Contract>.sol:<Contract> \
  --chain 677 --verifier blockscout \
  --verifier-url https://scan.botchain.ai/api
```

`--legacy` is not optional here: `baseFeePerGas` is 0, so EIP-1559 pricing produces a
transaction the chain prices at zero priority and validators have no reason to include.

## Rules

- **Testnet before mainnet.** Every time, including for a "trivial" change.
- **Dry run before broadcast.** Read the gas estimate and the target address out loud.
- **Verify after deploying.** An unverified contract asking people for funds is
  indistinguishable from a scam, because they cannot read what they are approving.
- **A deploy is not undoable.** There is no redeploy over the same address, no refund,
  and a wrong constructor argument is permanent. Check the treasury address twice.
