use crate::chat_types::{
    classify_risk, AgentOutput, ChatResponse, PendingToolCall, RiskLevel, ToolCall, ToolResult,
};
use crate::client::AiClient;
use crate::conversation::ConversationHistory;
use crate::tool_definitions::all_tools;
use crate::tool_dispatcher::ToolDispatcher;
use crate::types::{AiRequest, ChatMessage};
use anyhow::{anyhow, Result};
use serde_json::Value;

pub struct ChatAgent {
    pub client: AiClient,
    tools: Vec<Value>,
    pub history: ConversationHistory,
    dispatcher: ToolDispatcher,
    pending: Option<PendingToolCall>,
}

impl ChatAgent {
    pub fn new(client: AiClient, history: ConversationHistory, dispatcher: ToolDispatcher) -> Self {
        Self {
            tools: all_tools(),
            client,
            history,
            dispatcher,
            pending: None,
        }
    }

    /// Process a user message. Safe tool calls are auto-executed; others return NeedsConfirmation.
    pub async fn process(&mut self, user_text: &str) -> Result<AgentOutput> {
        self.history.push_user(user_text.to_string());
        self.run_llm().await
    }

    /// Execute the pending tool call after user confirmation.
    pub async fn confirm_pending(&mut self) -> Result<AgentOutput> {
        let pending = self
            .pending
            .take()
            .ok_or_else(|| anyhow!("No pending tool call"))?;
        self.execute_tool(pending).await
    }

    /// Cancel the pending tool call.
    pub fn reject_pending(&mut self) -> String {
        match self.pending.take() {
            Some(p) => format!("Cancelled: {}.", p.summary),
            None => "No pending action to cancel.".into(),
        }
    }

    pub fn has_pending(&self) -> bool {
        self.pending.is_some()
    }

    async fn run_llm(&mut self) -> Result<AgentOutput> {
        let mut request = AiRequest::new(self.client.model.clone(), self.history.as_messages());
        request.tools = Some(self.tools.clone());
        request.tool_choice = Some("auto".to_string());
        request.max_tokens = 1024;

        let response = self.client.chat(request).await?;
        let choice = response
            .choices
            .first()
            .ok_or_else(|| anyhow!("No choices in response"))?;

        if let Some(tool_calls) = &choice.message.tool_calls {
            if let Some(tc) = tool_calls.first() {
                let call = self.parse_tool_call_json(&tc.function.name, &tc.function.arguments);
                let risk = classify_risk(&call);
                let summary = self.describe_tool_call(&tc.function.name, &tc.function.arguments);

                // Record the assistant message with tool_calls in history
                let assistant_msg = ChatMessage {
                    role: "assistant".into(),
                    content: if choice.message.content.is_empty() {
                        None
                    } else {
                        Some(choice.message.content.clone())
                    },
                    tool_calls: Some(tool_calls.clone()),
                    tool_call_id: None,
                    name: None,
                };
                self.history.push_message(assistant_msg);

                let pending = PendingToolCall {
                    call_id: tc.id.clone(),
                    call,
                    summary,
                    risk: risk.clone(),
                };

                return match risk {
                    RiskLevel::Safe => self.execute_tool(pending).await,
                    _ => {
                        self.pending = Some(pending.clone());
                        Ok(AgentOutput::NeedsConfirmation(pending))
                    }
                };
            }
        }

        let content = choice.message.content.clone();
        let tokens = response.usage.map(|u| u.total_tokens).unwrap_or(0);
        self.history.push_assistant(content.clone());

        Ok(AgentOutput::Reply(ChatResponse {
            message: content,
            trade_entry: None,
            tokens_used: tokens,
        }))
    }

    async fn execute_tool(&mut self, pending: PendingToolCall) -> Result<AgentOutput> {
        let result = self.dispatcher.dispatch(pending.call.clone()).await?;

        let result_text = match &result {
            ToolResult::Success { message, .. } => message.clone(),
            ToolResult::Failure { message, .. } => format!("Error: {}", message),
        };

        self.history
            .push_message(ChatMessage::tool_result(&pending.call_id, &result_text));

        // Follow-up LLM call for natural language summary; don't allow another tool call
        let mut request = AiRequest::new(self.client.model.clone(), self.history.as_messages());
        request.tools = Some(self.tools.clone());
        request.tool_choice = Some("none".to_string());
        request.max_tokens = 512;

        let response = self.client.chat(request).await?;
        let choice = response
            .choices
            .first()
            .ok_or_else(|| anyhow!("No choices in follow-up response"))?;
        let content = choice.message.content.clone();
        let tokens = response.usage.map(|u| u.total_tokens).unwrap_or(0);
        self.history.push_assistant(content.clone());

        Ok(AgentOutput::Reply(ChatResponse {
            message: content,
            trade_entry: None,
            tokens_used: tokens,
        }))
    }

    fn parse_tool_call_json(&self, name: &str, args: &str) -> ToolCall {
        let v: Value = serde_json::from_str(args).unwrap_or(Value::Object(Default::default()));
        match name {
            "swap_token" => ToolCall::SwapToken {
                from: v["from"].as_str().unwrap_or("SOL").to_string(),
                to: v["to"].as_str().unwrap_or("USDC").to_string(),
                amount_sol: v["amount_sol"].as_f64().unwrap_or(0.0),
                slippage_bps: v["slippage_bps"].as_u64().map(|x| x as u32),
            },
            "get_quote" => ToolCall::GetQuote {
                from: v["from"].as_str().unwrap_or("SOL").to_string(),
                to: v["to"].as_str().unwrap_or("USDC").to_string(),
                amount_sol: v["amount_sol"].as_f64().unwrap_or(0.0),
            },
            "set_bot_mode" => ToolCall::SetBotMode {
                mode: v["mode"].as_str().unwrap_or("idle").to_string(),
            },
            "get_trade_history" => ToolCall::GetTradeHistory {
                n: v["n"].as_u64().map(|x| x as u32),
            },
            "x402_search" => ToolCall::X402Search {
                query: v["query"].as_str().unwrap_or("").to_string(),
                verified_only: v["verified_only"].as_bool(),
                min_quality_score: v["min_quality_score"].as_f64(),
                limit: v["limit"].as_u64().map(|x| x as u32),
            },
            "x402_check" => ToolCall::X402Check {
                url: v["url"].as_str().unwrap_or("").to_string(),
                method: v["method"].as_str().map(str::to_string),
            },
            "x402_fetch" | "x402_pay" => ToolCall::X402Fetch {
                url: v["url"].as_str().unwrap_or("").to_string(),
                method: v["method"].as_str().map(str::to_string),
            },
            "x402_wallet" => ToolCall::X402Wallet,
            "scan_arb" => ToolCall::ScanArb,
            "get_bot_status" => ToolCall::GetBotStatus,
            _ => ToolCall::GetBalance,
        }
    }

    fn describe_tool_call(&self, name: &str, args: &str) -> String {
        let v: Value = serde_json::from_str(args).unwrap_or(Value::Object(Default::default()));
        match name {
            "swap_token" => format!(
                "Swap {:.4} {} → {}",
                v["amount_sol"].as_f64().unwrap_or(0.0),
                v["from"].as_str().unwrap_or("?"),
                v["to"].as_str().unwrap_or("?")
            ),
            "set_bot_mode" => format!("Set bot mode to '{}'", v["mode"].as_str().unwrap_or("?")),
            "x402_search" => format!(
                "Search x402 marketplace for '{}'",
                v["query"].as_str().unwrap_or("?")
            ),
            "x402_check" => format!("Check x402 price for {}", v["url"].as_str().unwrap_or("?")),
            "x402_fetch" | "x402_pay" => {
                format!(
                    "Call paid x402 endpoint {}",
                    v["url"].as_str().unwrap_or("?")
                )
            }
            "x402_wallet" => "Check x402 wallet setup".to_string(),
            _ => name.replace('_', " "),
        }
    }
}
