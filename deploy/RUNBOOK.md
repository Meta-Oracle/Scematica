# Go-live runbook — deploy the relay + list the MCP server

Copy-paste commands to take the rail from code to a public URL that agents can
discover. Two deploy paths (Fly.io and a plain VPS), then the MCP-registry and
Smithery listings.

Prerequisites: a Solana wallet address to be paid (only if you enable x402), and
either a Fly.io account or an Ubuntu VPS with a domain.

---

## 0. Build the seed data (both paths need this)

The relay serves three signal files. With no live bot next to it, produce/copy
them into one directory first:

```bash
# From your machine, in the repo root, with your bot artifacts present:
cargo run -p scemadex-integrations --bin signal-seeder -- --dir .
#   -> scematica-pool-scores.json   (402 real scores from your decision log)

mkdir -p signals
cp scematica-pool-scores.json signals/
cp scematica-deployer-reputation.json signals/    # reputation feed
cp scematica-nn-advice.json signals/              # Deep Q* advice feed
```

`signals/` is now a self-contained feed. (`reputation`/`advice` degrade to a
neutral baseline if a file is missing, so partial feeds still work.)

---

## Path A — Fly.io (managed, public HTTPS URL)

```bash
# 1. Install flyctl + log in
#    macOS/Linux:  curl -L https://fly.io/install.sh | sh
#    Windows:      pwsh -c "iwr https://fly.io/install.ps1 -useb | iex"
fly auth login

# 2. Pick a globally-unique app name and set it in deploy/fly.toml (`app = "..."`).
export APP=scemadex-relay-$(whoami)          # or any unique name
fly apps create "$APP"

# 3. Create the persistent volume the relay mounts at /signals
fly volume create relay_data --size 1 --region iad --app "$APP" --yes

# 4. First deploy (builds deploy/relay.Dockerfile — ~20-40 min the first time)
fly deploy --app "$APP" --config deploy/fly.toml --dockerfile deploy/relay.Dockerfile

# 5. Upload the seed feed into the running machine's volume (read live, no restart)
fly ssh console --app "$APP" -C "mkdir -p /signals/mesh"
for f in signals/*.json; do
  fly ssh sftp shell --app "$APP" <<EOF
put $f /signals/$(basename "$f")
EOF
done

# 6. Verify
curl "https://$APP.fly.dev/health"                       # -> ok
curl "https://$APP.fly.dev/signal/pool_score/<A_MINT_IN_YOUR_FEED>"
```

Your public relay URL is `https://$APP.fly.dev`. Keep it **open** (no payment
flags) to maximize trial; add x402 later (below).

---

## Path B — Ubuntu VPS (Docker + Caddy for auto-TLS)

```bash
# On the VPS (point a DNS A record, e.g. relay.example.com, at its IP first):
sudo apt-get update && sudo apt-get install -y docker.io git
sudo systemctl enable --now docker

git clone https://github.com/Meta-Oracle/Scematica.git && cd Scematica

# Build the relay image (first build is slow — fat LTO)
sudo docker build -f deploy/relay.Dockerfile -t scemadex-relay .

# Put the seed feed on the host (scp your local ./signals up to /opt/scematica/signals)
sudo mkdir -p /opt/scematica/signals && sudo cp -r signals/* /opt/scematica/signals/

# Run the relay on localhost:8080, restarting on boot/crash
sudo docker run -d --name scemadex-relay --restart unless-stopped \
  -p 127.0.0.1:8080:8080 \
  -v /opt/scematica/signals:/signals \
  scemadex-relay --signal-dir /signals --persist-dir /signals/mesh

# TLS + public hostname with Caddy (auto Let's Encrypt):
sudo apt-get install -y caddy
echo 'relay.example.com {
    reverse_proxy 127.0.0.1:8080
}' | sudo tee /etc/caddy/Caddyfile
sudo systemctl restart caddy

# Verify
curl https://relay.example.com/health          # -> ok
```

Your public relay URL is `https://relay.example.com`.

---

## Enabling x402 metering (optional, either path)

Launch open first. To charge per signal call, restart the relay with a fee-payer
keypair (it covers the network fee so callers need only USDC):

```bash
# Upload payer.json into the signals volume/dir (NEVER bake it into the image),
# then run with the payment flags:
scemadex-relay --signal-dir /signals \
  --pay-to <YOUR_WALLET> \
  --keypair /signals/payer.json \
  --rpc-url https://mainnet.helius-rpc.com/?api-key=... \
  --price-usdc 0.001
```

On Fly: `fly ssh sftp` the key up, then set the machine command (or a `[processes]`
override in fly.toml). On the VPS: add these flags to the `docker run` line.

---

## Publish the MCP server to the official registry

The registry hosts metadata only — the crate must be on crates.io first.

```bash
# 1. Publish the crate (from repo root). The README carries the mcp-name marker
#    that the registry checks against server.json's `name`.
cargo publish -p scemadex-mcp        # skip if 0.1.1 is already live

# 2. Install the registry CLI
#    macOS:  brew install mcp-publisher
#    else:   download from github.com/modelcontextprotocol/registry releases

# 3. Authenticate as the GitHub namespace owner (must be able to act as the
#    `meta-oracle` org to claim io.github.meta-oracle/*)
mcp-publisher login github

# 4. Validate then publish the manifest
cd crates/scemadex-mcp
mcp-publisher publish        # reads ./server.json
```

Notes:
- The `name` (`io.github.meta-oracle/scemadex-mcp`) namespace is proven by the
  GitHub login; the `mcp-name:` line in `README.md` proves the crate is yours.
- There was a known `mcp-publisher` v1.0.0 bug that mangled `server.json` on
  submit — use a current CLI version and, if publish fails validation, re-run
  with the latest release.

## List on Smithery (secondary)

`smithery.yaml` (repo root) describes the local stdio server. Submit the repo at
smithery.ai → "Add Server". Because `scemadex-mcp` is a Rust binary (not an
npm/pip package Smithery can build in its sandbox), Smithery serves it as a
local/self-hosted entry: users `cargo install scemadex-mcp`, then Smithery hands
them the `--relay-url` config. The canonical, build-agnostic listing remains the
MCP-registry `server.json`.

---

## After listing — point agents at your relay

Client config (Claude Desktop / Claude Code / Cursor), using the public URL:

```json
{
  "mcpServers": {
    "scemadex": {
      "command": "scemadex-mcp",
      "args": ["--relay-url", "https://<your-relay-url>"]
    }
  }
}
```

Then in an agent: "What's the ScemaDEX pool score for `<mint>`?" → it calls
`scemadex_pool_score` → your relay answers from the seeded feed. That round-trip
working against a public URL is the moment the rail is live.
