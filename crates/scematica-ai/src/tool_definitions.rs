use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub parameters: Value,
}

pub fn all_tools() -> Vec<Value> {
    vec![
        json!({
            "type": "function",
            "function": {
                "name": "swap_token",
                "description": "Swap a token on the DEX.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "from": { "type": "string", "description": "Token to swap from" },
                        "to": { "type": "string", "description": "Token to swap to" },
                        "amount_sol": { "type": "number", "description": "Amount in SOL" },
                        "slippage_bps": { "type": "integer", "description": "Slippage in basis points (1-2000)" }
                    },
                    "required": ["from", "to", "amount_sol"]
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": "get_quote",
                "description": "Get a swap quote.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "from": { "type": "string" },
                        "to": { "type": "string" },
                        "amount_sol": { "type": "number" }
                    },
                    "required": ["from", "to", "amount_sol"]
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": "get_balance",
                "description": "Get wallet balance.",
                "parameters": { "type": "object", "properties": {}, "required": [] }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": "set_bot_mode",
                "description": "Set the trading bot mode (e.g., 'aggressive', 'safe').",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "mode": { "type": "string" }
                    },
                    "required": ["mode"]
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": "scan_arb",
                "description": "Scan for arbitrage opportunities.",
                "parameters": { "type": "object", "properties": {}, "required": [] }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": "get_trade_history",
                "description": "Get trade history.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "n": { "type": "integer", "description": "Number of trades" }
                    },
                    "required": []
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": "get_bot_status",
                "description": "Get bot status and metrics.",
                "parameters": { "type": "object", "properties": {}, "required": [] }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": "x402_search",
                "description": "Search a configured x402 API marketplace for paid or free APIs. Prefer verified endpoints with qualityScore >= 75.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "query": { "type": "string", "description": "Broad API search query, e.g. weather, token risk, prices" },
                        "verified_only": { "type": "boolean", "description": "Only return verified marketplace entries" },
                        "min_quality_score": { "type": "number", "description": "Minimum qualityScore threshold; default 75" },
                        "limit": { "type": "integer", "description": "Maximum number of results; default 8" }
                    },
                    "required": ["query"]
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": "x402_check",
                "description": "Preview x402 pricing for a URL without paying. Always run this before x402_fetch.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "url": { "type": "string", "description": "Endpoint URL to check" },
                        "method": { "type": "string", "description": "HTTP method; defaults to GET" }
                    },
                    "required": ["url"]
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": "x402_fetch",
                "description": "Call an x402 endpoint after price preview and user confirmation. Pays only within SCEMATICA_X402_MAX_USDC.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "url": { "type": "string", "description": "Endpoint URL to call" },
                        "method": { "type": "string", "description": "HTTP method; defaults to GET" }
                    },
                    "required": ["url"]
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": "x402_pay",
                "description": "Alias for x402_fetch. Use after x402_check and explicit user confirmation.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "url": { "type": "string", "description": "Endpoint URL to call and pay" },
                        "method": { "type": "string", "description": "HTTP method; defaults to GET" }
                    },
                    "required": ["url"]
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": "x402_wallet",
                "description": "Show x402 wallet configuration, active networks, marketplace setup, and per-call spending limit.",
                "parameters": { "type": "object", "properties": {}, "required": [] }
            }
        }),
    ]
}
