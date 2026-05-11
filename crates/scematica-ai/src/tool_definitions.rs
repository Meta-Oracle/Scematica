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
    ]
}
