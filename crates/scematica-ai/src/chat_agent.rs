use anyhow::{Result, anyhow};
use crate::client::AiClient;
use crate::conversation::ConversationHistory;
use crate::chat_types::{AgentOutput, PendingToolCall, ChatResponse, classify_risk};
use crate::tool_definitions::all_tools;
use crate::types::{AiRequest, ChatMessage};
use serde_json::Value;

pub struct ChatAgent {
    pub client: AiClient,
    pub tools: Vec<Value>,
    pub history: ConversationHistory,
}

impl ChatAgent {
    pub fn new(client: AiClient, history: ConversationHistory) -> Self {
        Self {
            client,
            tools: all_tools(),
            history,
        }
    }

    pub async fn process(&mut self, user_text: &str) -> Result<AgentOutput> {
        self.history.push_user(user_text.to_string());
        
        let mut request = AiRequest::new("llama-3.3-70b-versatile", self.history.as_messages());
        request.tools = Some(self.tools.clone());
        request.tool_choice = Some("auto".to_string());

        let response = self.client.chat(request).await?;
        
        let choice = response.choices.first().ok_or_else(|| anyhow!("No choices in response"))?;
        
        if let Some(tool_calls) = &choice.message.tool_calls {
            // Basic handling of first tool call
            if let Some(tool_call) = tool_calls.first() {
                let call = self.parse_tool_call(tool_call)?;
                let risk = classify_risk(&call);
                
                let pending = PendingToolCall {
                    call_id: tool_call.id.clone(),
                    call,
                    summary: "Proposed tool call".to_string(),
                    risk,
                };
                
                return Ok(AgentOutput::NeedsConfirmation(pending));
            }
        }
        
        let chat_response = ChatResponse {
            message: choice.message.content.clone(),
            trade_entry: None,
            tokens_used: response.usage.map(|u| u.total_tokens).unwrap_or(0),
        };
        
        self.history.push_assistant(chat_response.message.clone());
        Ok(AgentOutput::Reply(chat_response))
    }

    fn parse_tool_call(&self, _tool_call: &crate::types::ToolCallResponse) -> Result<crate::chat_types::ToolCall> {
        // Implementation for parsing JSON arguments to ToolCall enum
        Ok(crate::chat_types::ToolCall::GetBalance) // Simplified placeholder
    }
}
