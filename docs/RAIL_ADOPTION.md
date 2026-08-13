# The ScemaDEX Rail — adoption playbook

This is the plan for turning the ScemaDEX rail from working code into an adopted
network. The thesis: **the sniper is single-player and capital-at-risk; the rail
is multiplayer and zero-capital-to-try.** Adoption effort belongs on the rail.

Two audiences, one rail:

- **Experts (agent / LLM devs):** "Give your agent live Solana pool intelligence
  as a paid tool call." Discovery through MCP registries; payment through x402
  micro-amounts. No token purchase required to try.
- **Newcomers (traders / $SCEMA holders):** "The bot's brain, now readable by any
  agent." A live public dashboard + a one-command demo. Holding $SCEMA unlocks the
  higher tiers and the bot itself.

## Go-live checklist

1. **Seed the feed** — `signal-seeder` turns historical decisions into real
   day-one pool scores (see `deploy/README.md`, Step 0). A relay that answers
   `pool_score` with real numbers on launch is worth calling; an empty one isn't.
2. **Deploy one public relay** — Docker / compose / Fly, per `deploy/README.md`.
   Get a stable URL (e.g. `https://relay.scematica.org` or `*.fly.dev`).
3. **Decide open vs. metered** — launch `/signal/*` **open** to maximize trial,
   then turn on x402 once there's demand. Free-to-try is the on-ramp; the token
   gate is not the first wall.
4. **List the MCP server** (below) so agents discover it.
5. **Announce to both audiences** (copy below).
6. **Instrument** — log call volume per endpoint; that's your adoption metric,
   not GitHub stars.

## MCP registry listing (ready to submit)

Submit `scemadex-mcp` to MCP registries (the modelcontextprotocol.io registry,
Smithery, PulseMCP, `awesome-mcp-servers`). Suggested listing fields:

- **Name:** ScemaDEX — Solana pool intelligence
- **Command:** `scemadex-mcp` (install: `cargo install scemadex-mcp`)
- **Category / tags:** `solana`, `defi`, `trading`, `x402`, `market-data`, `finance`
- **Short description:**
  > Give your agent live Solana new-pool intelligence — deployer reputation,
  > 0–100 pool-quality scores, and Deep Q* trade advice — as MCP tool calls,
  > settled per-call over x402 (agent pays USDC, no SOL needed).
- **Tools:** `scemadex_reputation`, `scemadex_pool_score`, `scemadex_advice`,
  `scemadex_inference_quote`, `scemadex_experience_buy` (see
  `crates/scemadex-mcp/README.md`).
- **Client config entry** (point at your public relay):

  ```json
  {
    "mcpServers": {
      "scemadex": {
        "command": "scemadex-mcp",
        "args": ["--relay-url", "https://<your-public-relay>"]
      }
    }
  }
  ```

Distinctive hook for the listing: this is one of the few MCP servers that is
**natively metered** — a paid tool returns x402 payment requirements the agent
can settle autonomously (`--features pay`), rather than requiring a pre-provisioned
API key. Lead with that; it's the novel capability.

## Announcement copy

### Expert / agent-dev version (X, HN, MCP Discord)

> Your agent can now buy Solana pool intelligence per-call.
>
> `cargo install scemadex-mcp` adds 5 tools to Claude / Cursor / any MCP client:
> deployer reputation, a 0–100 pool-quality score, and Deep Q* trade advice on any
> mint — served from a live sniper's own signal pipeline.
>
> Paid tools return x402 requirements the agent settles itself in USDC (no SOL,
> no API key dance). Lean Rust core; `--features pay` for auto-settlement.
>
> Point it at the public relay: <URL>. Tools + contract: <repo link>

### Newcomer / trader version (Telegram, pump.fun, $SCEMA channels)

> The bot's brain is now readable by any AI agent.
>
> Scematica's sniper scores every new Solana pool — liquidity, deployer rug
> history, Deep Q* conviction. That intelligence is now a live rail: ask an AI
> agent "is this mint worth it?" and it queries Scematica's scores directly.
>
> Try it free: `cargo install scematica-suite && scematica dashboard --demo`.
> Run the real bot with 250k $SCEMA. Same signals, now agent-accessible.

## The discovery funnel

```
   MCP registries / agent devs ─┐
   X / HN (expert post) ────────┤
                                ├─▶  public relay  ─▶  paid calls (x402)  ─▶  $SCEMA demand
   Telegram / pump.fun ─────────┤        │                                     (higher tiers,
   (newcomer post) ─────────────┘        └─▶ live dashboard (front door)         run the bot)
```

The relay is the hub every channel points at. Until it's live at a public URL,
none of these channels have anywhere to send people — deploying it is the
gating step for the entire funnel.

## What's built vs. what's left

**Built & verified:** the SDK (8 primitives), the relay (8-endpoint contract),
the x402 facilitator (adversarially tested), the MCP bridge (5 tools), and the
`signal-seeder` that gives a fresh relay real data.

**Left (not code — operations):** deploy a public relay instance, point a domain
at it, submit the registry listings, publish the announcements. These need
accounts and a wallet, not commits.
