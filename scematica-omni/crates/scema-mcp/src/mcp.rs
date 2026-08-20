//! Minimal Model Context Protocol server over stdio.
//!
//! Enough of the MCP JSON-RPC 2.0 surface — `initialize`, `tools/list`, `tools/call`,
//! `ping`, and the `notifications/initialized` notification — for any MCP-capable client to
//! discover and call the omni loop. Same shape as `scemadex-mcp` in the bot workspace, and
//! the same non-negotiable rule:
//!
//! > **stdout is the transport.** Every log line goes to stderr. One stray `println!` in a
//! > library on the call path corrupts the stream and the client reports a protocol error
//! > that looks nothing like its cause.
//!
//! ## This links the loop rather than proxying the daemon
//!
//! `scemadex-mcp` proxies an HTTP relay because the intelligence it exposes lives on
//! somebody else's machine. Here it does not: the agent, the observers and the record store
//! are all in-process. Linking `scema-agent` directly removes a network hop, a token, and
//! the possibility of the two surfaces disagreeing about what the loop does. The daemon and
//! this server are siblings over one library, not layers.
//!
//! ## A tool error is a result, not a protocol error
//!
//! A refused path or a missing argument comes back as `tools/call` result with
//! `isError: true` and text saying what to do instead. A JSON-RPC error would be a
//! transport-level failure, which clients surface as "the server broke" — and a model that
//! is told the server broke stops trying, where a model told "that path is outside the
//! workspace, which is X" corrects itself.

use serde_json::{json, Value};

use crate::tools::Tools;

/// MCP protocol revision this server implements.
pub const PROTOCOL_VERSION: &str = "2024-11-05";

pub struct McpServer {
    tools: Tools,
    version: String,
}

impl McpServer {
    pub fn new(tools: Tools) -> Self {
        McpServer { tools, version: env!("CARGO_PKG_VERSION").to_string() }
    }

    /// Handle one newline-delimited JSON message.
    ///
    /// `None` means "no reply" — correct for a notification, and required: replying to a
    /// notification is a protocol violation that some clients treat as fatal.
    pub fn handle_line(&self, line: &str) -> Option<String> {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            return None;
        }
        let msg: Value = match serde_json::from_str(trimmed) {
            Ok(v) => v,
            // No id available, so this cannot be attributed to a request. -32700 is the
            // parse-error code.
            Err(e) => return Some(error_response(Value::Null, -32700, &format!("parse error: {e}"))),
        };

        let method = msg.get("method").and_then(Value::as_str).unwrap_or("");
        let id = msg.get("id").cloned();

        // A notification has no id, and the correct reply to one is nothing at all —
        // answering a notification is a protocol violation some clients treat as fatal.
        let id = id?;

        match method {
            "initialize" => Some(ok_response(
                id,
                json!({
                    "protocolVersion": PROTOCOL_VERSION,
                    "capabilities": { "tools": {} },
                    "serverInfo": { "name": "scema-mcp", "version": self.version },
                    "instructions":
                        "The Scematica Omni loop: perceive an environment, rank competing branches \
                         against a goal, and seal a verifiable record. Two things to know before \
                         calling anything. First, an unmeasured quantity renders as an em dash and \
                         contributed nothing to the score — it is not a zero, and reasoning about it \
                         as one will be wrong. Second, grounding is never inferred: a goal that does \
                         not cite a counted signal id in `ground` has no measured expected gain and \
                         will score at or below zero, and that is the correct answer rather than a \
                         malfunction. Call omni_observe first to see the signal ids."
                }),
            )),
            "ping" => Some(ok_response(id, json!({}))),
            "tools/list" => Some(ok_response(id, json!({ "tools": self.tools.definitions() }))),
            "tools/call" => {
                let params = msg.get("params").cloned().unwrap_or_else(|| json!({}));
                let name = params.get("name").and_then(Value::as_str).unwrap_or("");
                let args = params.get("arguments").cloned().unwrap_or_else(|| json!({}));
                if name.is_empty() {
                    return Some(error_response(id, -32602, "params.name is required"));
                }
                let result = self.tools.call(name, &args);
                Some(ok_response(
                    id,
                    json!({
                        "content": [{ "type": "text", "text": result.text }],
                        "isError": result.is_error,
                    }),
                ))
            }
            other => Some(error_response(id, -32601, &format!("method not found: {other}"))),
        }
    }
}

fn ok_response(id: Value, result: Value) -> String {
    json!({ "jsonrpc": "2.0", "id": id, "result": result }).to_string()
}

fn error_response(id: Value, code: i32, message: &str) -> String {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } }).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use scema_agent::Agent;
    use scema_tools::Workspace;
    use std::fs;
    use std::path::PathBuf;

    fn scratch() -> PathBuf {
        let p = std::env::temp_dir().join(format!(
            "scema-omni-rpc-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(p.join("src")).unwrap();
        fs::write(p.join("Cargo.toml"), "[package]\nname = \"t\"\n").unwrap();
        fs::write(p.join("src/lib.rs"), "fn a() {}\n").unwrap();
        p
    }

    fn server(root: &PathBuf) -> McpServer {
        McpServer::new(Tools {
            agent: Agent::new(root.join(".scema"), None),
            workspace: Workspace::new([root]).unwrap(),
            root: root.join(".scema"),
            allow_decide: false,
        })
    }

    fn call(s: &McpServer, line: &str) -> Value {
        serde_json::from_str(&s.handle_line(line).expect("expected a reply")).unwrap()
    }

    #[test]
    fn initialize_advertises_the_protocol_and_the_two_rules_a_model_needs() {
        let root = scratch();
        let v = call(&server(&root), r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#);
        assert_eq!(v["result"]["protocolVersion"], json!(PROTOCOL_VERSION));
        let instructions = v["result"]["instructions"].as_str().unwrap();
        // Both are load-bearing: a model that reads an em dash as zero, or expects grounding
        // to be inferred, will misread every result it gets.
        assert!(instructions.contains("em dash"));
        assert!(instructions.contains("never inferred"));
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_notification_gets_no_reply() {
        // Replying to a notification is a protocol violation some clients treat as fatal.
        let root = scratch();
        assert!(server(&root)
            .handle_line(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#)
            .is_none());
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn tools_list_returns_the_catalogue() {
        let root = scratch();
        let v = call(&server(&root), r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#);
        let names: Vec<&str> = v["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap())
            .collect();
        assert!(names.contains(&"omni_observe"));
        assert!(names.contains(&"omni_simulate"));
        assert!(!names.contains(&"omni_decide"), "not advertised when disabled");
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_refused_path_is_a_tool_result_not_a_protocol_error() {
        // The distinction that decides whether a model corrects itself or gives up.
        let root = scratch();
        let outside = std::env::temp_dir().to_string_lossy().to_string();
        let line = json!({
            "jsonrpc": "2.0", "id": 3, "method": "tools/call",
            "params": { "name": "omni_observe", "arguments": { "locator": outside } }
        })
        .to_string();
        let v = call(&server(&root), &line);
        assert!(v.get("error").is_none(), "must not be a JSON-RPC error");
        assert_eq!(v["result"]["isError"], json!(true));
        assert!(v["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("outside this workspace"));
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn malformed_json_is_a_parse_error_with_a_null_id() {
        let root = scratch();
        let v = call(&server(&root), "{ not json");
        assert_eq!(v["error"]["code"], json!(-32700));
        assert_eq!(v["id"], json!(null));
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn an_unknown_method_is_method_not_found() {
        let root = scratch();
        let v = call(&server(&root), r#"{"jsonrpc":"2.0","id":9,"method":"does/not/exist"}"#);
        assert_eq!(v["error"]["code"], json!(-32601));
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_tools_call_without_a_name_is_an_invalid_params_error() {
        let root = scratch();
        let v = call(&server(&root), r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{}}"#);
        assert_eq!(v["error"]["code"], json!(-32602));
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_blank_line_is_ignored() {
        let root = scratch();
        assert!(server(&root).handle_line("   ").is_none());
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn every_reply_is_a_single_line() {
        // Newline-delimited JSON is the transport. A pretty-printed reply would split one
        // message across many frames and desynchronise the stream.
        let root = scratch();
        let s = server(&root);
        for line in [
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize"}"#,
            r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#,
            r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"omni_policy","arguments":{}}}"#,
        ] {
            let reply = s.handle_line(line).unwrap();
            assert!(!reply.contains('\n'), "reply must be one line: {reply}");
        }
        fs::remove_dir_all(&root).ok();
    }
}
