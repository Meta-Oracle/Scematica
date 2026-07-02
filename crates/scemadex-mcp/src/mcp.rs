//! Minimal Model Context Protocol (MCP) server over stdio.
//!
//! Implements just enough of the MCP JSON-RPC 2.0 surface (`initialize`,
//! `tools/list`, `tools/call`, `ping`, and the `notifications/initialized`
//! notification) for any MCP-capable LLM agent (Claude Desktop, Claude Code, the
//! Agent SDK, etc.) to discover and call ScemaDEX tools. Transport is
//! newline-delimited JSON on stdin/stdout, per the MCP stdio convention — so
//! **stdout is sacred**: all logging goes to stderr.
//!
//! Each tool proxies to a running `scemadex-relay`. A `402 Payment Required`
//! from the relay's x402 gate is surfaced to the agent as a normal (non-error)
//! result carrying the payment requirements, so "buy this intelligence" is a
//! first-class, self-describing action.

use serde_json::{json, Value};

use crate::relay::RelayClient;

/// MCP protocol revision this server implements.
pub const PROTOCOL_VERSION: &str = "2024-11-05";

pub struct McpServer {
    relay: RelayClient,
    server_version: String,
}

impl McpServer {
    pub fn new(relay: RelayClient) -> Self {
        Self {
            relay,
            server_version: env!("CARGO_PKG_VERSION").to_string(),
        }
    }

    /// The advertised tool catalogue (name, description, JSON-Schema input).
    fn tool_definitions() -> Value {
        json!([
            {
                "name": "scemadex_reputation",
                "description": "Get the deployer reputation signal for a Solana mint from the ScemaDEX oracle (0..1, higher = fewer historical rugs). Reads the bot's live deployer-reputation ledger.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "mint": { "type": "string", "description": "Solana mint (base58) or deployer address." }
                    },
                    "required": ["mint"]
                }
            },
            {
                "name": "scemadex_pool_score",
                "description": "Get the 0..100 predictive pool-quality score for a Solana mint from the ScemaDEX oracle.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "mint": { "type": "string", "description": "Solana mint (base58)." }
                    },
                    "required": ["mint"]
                }
            },
            {
                "name": "scemadex_advice",
                "description": "Get the current Deep Q* agent trading advice/decision signal for a Solana mint from the ScemaDEX oracle.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "mint": { "type": "string", "description": "Solana mint (base58)." }
                    },
                    "required": ["mint"]
                }
            },
            {
                "name": "scemadex_inference_quote",
                "description": "Request the cheapest open bonded-inference offer for a given intent digest from the ScemaDEX peer mesh. Returns the InferenceOffer (a slashable Conviction-Routing bond backing the quote) or a not-found result.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "intent_digest": { "type": "string", "description": "FNV-1a digest of the routing Intent (see scemadex-sdk Intent::digest)." }
                    },
                    "required": ["intent_digest"]
                }
            },
            {
                "name": "scemadex_experience_buy",
                "description": "Buy the cheapest available ExperienceBatch (a bundle of reinforcement-learning transitions) from the ScemaDEX experience mesh at or below your max price.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "max_price_usdc": { "type": "number", "description": "Maximum price to pay, in USDC (e.g. 0.25)." }
                    },
                    "required": ["max_price_usdc"]
                }
            }
        ])
    }

    /// Handle one inbound line. Returns `Some(response_json)` for requests, or
    /// `None` for notifications (which get no reply).
    pub async fn handle_line(&self, line: &str) -> Option<String> {
        let line = line.trim();
        if line.is_empty() {
            return None;
        }
        let msg: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(e) => {
                // Parse error — id unknown, per JSON-RPC use null.
                return Some(error_response(Value::Null, -32700, &format!("parse error: {e}")));
            }
        };

        let id = msg.get("id").cloned();
        let method = msg.get("method").and_then(|m| m.as_str()).unwrap_or("");
        let params = msg.get("params").cloned().unwrap_or(Value::Null);

        // Notifications (no id) get no response.
        let is_notification = id.is_none();

        match method {
            "initialize" => Some(result_response(
                id.unwrap_or(Value::Null),
                json!({
                    "protocolVersion": PROTOCOL_VERSION,
                    "capabilities": { "tools": {} },
                    "serverInfo": { "name": "scemadex-mcp", "version": self.server_version }
                }),
            )),
            "notifications/initialized" | "initialized" => None,
            "ping" => Some(result_response(id.unwrap_or(Value::Null), json!({}))),
            "tools/list" => Some(result_response(
                id.unwrap_or(Value::Null),
                json!({ "tools": Self::tool_definitions() }),
            )),
            "tools/call" => {
                let id = id.unwrap_or(Value::Null);
                let name = params.get("name").and_then(|n| n.as_str()).unwrap_or("");
                let args = params.get("arguments").cloned().unwrap_or(json!({}));
                let (text, is_error) = self.call_tool(name, &args).await;
                Some(result_response(
                    id,
                    json!({
                        "content": [ { "type": "text", "text": text } ],
                        "isError": is_error
                    }),
                ))
            }
            _ if is_notification => None,
            _ => Some(error_response(
                id.unwrap_or(Value::Null),
                -32601,
                &format!("method not found: {method}"),
            )),
        }
    }

    /// Dispatch a tool call to the relay. Returns `(text_content, is_error)`.
    async fn call_tool(&self, name: &str, args: &Value) -> (String, bool) {
        let str_arg = |k: &str| args.get(k).and_then(|v| v.as_str()).map(str::to_string);

        match name {
            "scemadex_reputation" | "scemadex_pool_score" | "scemadex_advice" => {
                let kind = match name {
                    "scemadex_reputation" => "reputation",
                    "scemadex_pool_score" => "pool_score",
                    _ => "advice",
                };
                let Some(mint) = str_arg("mint") else {
                    return ("missing required argument: mint".into(), true);
                };
                match self.relay.get_signal(kind, &mint).await {
                    Ok(r) => Self::format_response(&format!("{kind} for {mint}"), r),
                    Err(e) => (format!("relay request failed: {e}"), true),
                }
            }
            "scemadex_inference_quote" => {
                let Some(digest) = str_arg("intent_digest") else {
                    return ("missing required argument: intent_digest".into(), true);
                };
                match self.relay.inference_quote(&digest).await {
                    Ok(r) => Self::format_response("inference quote", r),
                    Err(e) => (format!("relay request failed: {e}"), true),
                }
            }
            "scemadex_experience_buy" => {
                let Some(usdc) = args.get("max_price_usdc").and_then(|v| v.as_f64()) else {
                    return ("missing/invalid required argument: max_price_usdc".into(), true);
                };
                if !usdc.is_finite() || usdc < 0.0 {
                    return ("max_price_usdc must be a non-negative number".into(), true);
                }
                let micro = (usdc * 1_000_000.0).round() as u64;
                match self.relay.experience_buy(micro).await {
                    Ok(r) => Self::format_response("experience batch", r),
                    Err(e) => (format!("relay request failed: {e}"), true),
                }
            }
            other => (format!("unknown tool: {other}"), true),
        }
    }

    /// Turn a relay HTTP outcome into MCP tool text. `402` is informational, not
    /// an error: the payment requirements are handed back so the agent can pay.
    fn format_response(label: &str, r: crate::relay::RelayResponse) -> (String, bool) {
        match r.status {
            200 => (r.body, false),
            402 => (
                format!(
                    "Payment required (x402) for {label}. Pay per the following requirements, \
                     then retry with the X-Payment header:\n{}",
                    r.body
                ),
                false,
            ),
            404 => (format!("No {label} available (relay returned 404)."), false),
            s => (format!("relay error {s} for {label}: {}", r.body), true),
        }
    }
}

// ── JSON-RPC helpers ─────────────────────────────────────────────────────────

fn result_response(id: Value, result: Value) -> String {
    json!({ "jsonrpc": "2.0", "id": id, "result": result }).to_string()
}

fn error_response(id: Value, code: i64, message: &str) -> String {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } }).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn server() -> McpServer {
        McpServer::new(RelayClient::new("http://localhost:8080"))
    }

    #[tokio::test]
    async fn initialize_advertises_tools_capability() {
        let resp = server()
            .handle_line(r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#)
            .await
            .expect("initialize replies");
        let v: Value = serde_json::from_str(&resp).unwrap();
        assert_eq!(v["id"], 1);
        assert_eq!(v["result"]["protocolVersion"], PROTOCOL_VERSION);
        assert!(v["result"]["capabilities"]["tools"].is_object());
        assert_eq!(v["result"]["serverInfo"]["name"], "scemadex-mcp");
    }

    #[tokio::test]
    async fn tools_list_returns_all_five_tools() {
        let resp = server()
            .handle_line(r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#)
            .await
            .unwrap();
        let v: Value = serde_json::from_str(&resp).unwrap();
        let tools = v["result"]["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 5);
        // Every tool has a name + object input schema.
        for t in tools {
            assert!(t["name"].is_string());
            assert_eq!(t["inputSchema"]["type"], "object");
        }
    }

    #[tokio::test]
    async fn initialized_notification_gets_no_reply() {
        assert!(server()
            .handle_line(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#)
            .await
            .is_none());
    }

    #[tokio::test]
    async fn unknown_method_is_json_rpc_error() {
        let resp = server()
            .handle_line(r#"{"jsonrpc":"2.0","id":9,"method":"does/not/exist"}"#)
            .await
            .unwrap();
        let v: Value = serde_json::from_str(&resp).unwrap();
        assert_eq!(v["error"]["code"], -32601);
    }

    #[tokio::test]
    async fn tool_call_missing_arg_is_reported_as_error_content() {
        let resp = server()
            .handle_line(
                r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"scemadex_reputation","arguments":{}}}"#,
            )
            .await
            .unwrap();
        let v: Value = serde_json::from_str(&resp).unwrap();
        assert_eq!(v["result"]["isError"], true);
        assert!(v["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("mint"));
    }

    #[tokio::test]
    async fn parse_error_returns_minus_32700() {
        let resp = server().handle_line("{not json").await.unwrap();
        let v: Value = serde_json::from_str(&resp).unwrap();
        assert_eq!(v["error"]["code"], -32700);
    }
}
