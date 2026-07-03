# scemadex-mcp

<!-- MCP Registry ownership marker — must match `name` in server.json. -->
mcp-name: io.github.meta-oracle/scemadex-mcp

A [Model Context Protocol](https://modelcontextprotocol.io) (MCP) server that
exposes the **ScemaDEX** agentic-liquidity rail to any MCP-capable LLM agent
(Claude Desktop, Claude Code, the Agent SDK, and others).

It bridges MCP (stdio JSON-RPC) to a running
[`scemadex-relay`](https://crates.io/crates/scemadex-relay) over HTTP:

```
LLM agent ⇄ (MCP/stdio) ⇄ scemadex-mcp ⇄ (HTTP) ⇄ scemadex-relay ⇄ bot artifacts
```

Because the relay's signal endpoints can be **x402-gated**, this turns "buy
trading intelligence" into a discoverable, priced tool call: a `402` from the
relay is handed back to the agent as the x402 payment requirements, ready to pay.

## Tools

| Tool | What it returns |
|---|---|
| `scemadex_reputation` | Deployer reputation for a mint (0..1) |
| `scemadex_pool_score` | 0..100 predictive pool-quality score |
| `scemadex_advice` | Current Deep Q* agent advice signal |
| `scemadex_inference_quote` | Cheapest open bonded-inference offer for an intent digest |
| `scemadex_experience_buy` | Cheapest ExperienceBatch at/below your max USDC price |

## Install & run

```bash
cargo install scemadex-mcp
scemadex-mcp --relay-url http://localhost:8080   # or set SCEMADEX_RELAY_URL
```

### Register with an MCP client

Example client config entry:

```json
{
  "mcpServers": {
    "scemadex": {
      "command": "scemadex-mcp",
      "args": ["--relay-url", "http://localhost:8080"]
    }
  }
}
```

Transport is newline-delimited JSON-RPC 2.0 on stdio (MCP `2024-11-05`). All
logging goes to stderr; stdout carries only protocol messages.

## Auto-pay (optional `pay` feature)

By default a paid (`402`) signal is surfaced to the agent as x402 payment
requirements — the agent decides whether to pay. Build with the `pay` feature to
have the server **settle the payment itself** (sign the SPL transfer, retry with
the `X-Payment` header) so purchases are transparent:

```bash
cargo install scemadex-mcp --features pay
scemadex-mcp --relay-url http://localhost:8080 --keypair ./payer.json
```

The default build stays lean (no `solana-sdk`); `pay` pulls it in only when you
ask for it — "lean core, injected power".

Part of the [Scematica](https://github.com/meta-oracle/scematica) suite. MIT.
