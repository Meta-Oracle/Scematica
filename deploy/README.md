# Deploying a public ScemaDEX relay

This directory turns the ScemaDEX rail from a **localhost-only** demo into a
**hostable public network**. Once a relay is live at a public URL, any MCP-capable
LLM agent (Claude Desktop, Claude Code, Cursor, the Agent SDK) can discover and
consume your bot's trading intelligence — reputation, pool scores, Deep Q* advice —
as priced tool calls, paid over x402.

```
                        ┌──────────────────────────────────────────┐
   your sniper bot ───▶ │  scematica-*.json  (reputation/score/…)   │
   (writes artifacts)   └──────────────────────────────────────────┘
                                        │  --signal-dir
                                        ▼
   agents ⇄ (MCP) ⇄ scemadex-mcp ⇄ (HTTP) ⇄  PUBLIC scemadex-relay  ⇄ x402 settle
                                        ▲
                        peer agents post/consume bonded inference + experience (mesh)
```

## Why this is the adoption surface

The sniper is single-player and capital-at-risk. The relay is **multiplayer and
zero-capital-to-try**: agents pay micro-amounts to read intelligence, and the
mesh lets agents trade bonded inference and learned experience with each other.
More participants → more signal → more value. The relay is the one component that
has network effects, and until it is deployed publicly nothing about the rail is
actually reachable.

## Step 0 — Seed day-one data (no live bot required)

A relay reads the sniper's artifacts from `--signal-dir`. If you don't have a bot
running next to it, the `/signal/pool_score/:mint` endpoint would return a neutral
baseline for everything — a dead feed. The **signal-seeder** distills your
historical decision log into a real per-mint score map so the relay ships with
data on day one:

```bash
# From a directory that holds scematica-pool-decisions.jsonl (+ optional
# scematica-pool-radar.json), produce scematica-pool-scores.json:
cargo run -p scemadex-integrations --bin signal-seeder -- --dir /path/to/artifacts
# -> "seeded 402 pool score(s) -> .../scematica-pool-scores.json"
```

Point the relay's `--signal-dir` at that directory. Re-run the seeder (e.g. via
cron) to refresh the feed as new history accumulates. With a live sniper writing
artifacts continuously you can skip this — but seeding is what makes a brand-new
relay worth calling before it has traffic.

## Option A — Docker (self-host anywhere)

Build from the **repo root** (the relay depends on several workspace crates, so
the build context must be the whole workspace):

```bash
docker build -f deploy/relay.Dockerfile -t scemadex-relay .
```

Run it **open** (free signals + mesh), reading a live bot's artifacts from `$PWD`:

```bash
docker run -p 8080:8080 -v "$PWD:/signals" scemadex-relay \
  --signal-dir /signals --persist-dir /signals/mesh
```

Verify:

```bash
curl http://localhost:8080/health          # -> ok
curl http://localhost:8080/signal/pool_score/<MINT>   # a pool score, if the bot has one
```

## Option B — docker compose

```bash
docker compose -f deploy/docker-compose.yml up --build
# point SIGNAL_DIR at where your bot writes artifacts:
SIGNAL_DIR=/path/to/bot docker compose -f deploy/docker-compose.yml up --build
```

## Option C — Fly.io (public URL in minutes)

```bash
# edit deploy/fly.toml: set a globally-unique `app` name first
fly launch --no-deploy --copy-config --dockerfile deploy/relay.Dockerfile
fly volume create relay_data --size 1
fly deploy
# -> https://<your-app>.fly.dev
```

Any host that runs a container works the same way (Railway, Render, a VPS). The
only requirements: expose port 8080 and give it a directory of bot artifacts.

## Turning on x402 (pay-per-call monetization)

By default `/signal/*` is served **open**. To meter it, supply a fee-payer
keypair, a wallet to be paid, and an RPC endpoint. The fee-payer covers the
Solana transaction fee so **callers need only the USDC being charged — no SOL**:

```bash
docker run -p 8080:8080 -v "$PWD:/signals" -v "$PWD/payer.json:/payer.json:ro" \
  scemadex-relay --signal-dir /signals \
    --pay-to <YOUR_WALLET> \
    --keypair /payer.json \
    --rpc-url https://mainnet.helius-rpc.com/?api-key=... \
    --price-usdc 0.001
```

Now a call to a `/signal/*` endpoint returns `402 Payment Required` with the
x402 requirements; a client pays and retries. The MCP server surfaces that `402`
to the agent as payment requirements (or, built with `--features pay`, settles it
automatically). Never bake the keypair into the image — mount it read-only or use
a platform secret.

## Pointing agents at your public relay

Once deployed, register the MCP bridge against the **public URL** instead of
localhost. Example Claude Desktop / Claude Code config entry:

```json
{
  "mcpServers": {
    "scemadex": {
      "command": "scemadex-mcp",
      "args": ["--relay-url", "https://<your-app>.fly.dev"]
    }
  }
}
```

Install the bridge with `cargo install scemadex-mcp` (add `--features pay` for
auto-settlement). See `crates/scemadex-mcp/README.md` for the full tool list.

## Endpoint contract

| Method | Path | Purpose | Gated |
|---|---|---|---|
| GET  | `/health` | liveness → `ok` | no |
| POST | `/inference/offer` | publish a bonded inference offer | no |
| POST | `/inference/quote` | claim cheapest offer for an intent digest | no |
| POST | `/experience/offer` | publish an experience batch | no |
| POST | `/experience/buy` | claim cheapest batch ≤ max price | no |
| GET  | `/signal/reputation/:mint` | deployer reputation | x402 (opt) |
| GET  | `/signal/pool_score/:mint` | 0–100 pool-quality score | x402 (opt) |
| GET  | `/signal/advice/:mint` | Deep Q* advice signal | x402 (opt) |

## Operational notes

- **First build is slow** (~20–40 min): the release profile uses fat LTO +
  `codegen-units = 1`. Rebuilds are layer-cached.
- **Persistence:** `--persist-dir` mirrors the mesh order-books to disk so a
  restart doesn't drop offers/experience. On Fly this is the mounted volume.
- **Signals come from artifacts, not necessarily a live bot:** `/signal/*` reads
  the JSON the sniper writes. Run the relay alongside a live sniper for a
  continuously-fresh feed, **or** use `signal-seeder` (Step 0) to build the
  pool-score map from historical data. `reputation` reads
  `scematica-deployer-reputation.json`; `pool_score` reads the seeded
  `scematica-pool-scores.json`; `advice` reads `scematica-nn-advice.json`. Absent
  a file, that signal degrades to a neutral baseline rather than erroring.
- **Keys:** the fee-payer keypair is the only secret. Keep it out of the image
  (`.dockerignore` already excludes `payer.json` and `config.toml`).
