use crate::chat_types::ToolCall;
use crate::types::ChatMessage;
use std::collections::VecDeque;

pub struct ConversationHistory {
    pub system_prompt: ChatMessage,
    pub turns: VecDeque<ChatMessage>,
    pub max_turns: usize,
}

impl ConversationHistory {
    pub fn new(system_prompt: ChatMessage, max_turns: usize) -> Self {
        Self {
            system_prompt,
            turns: VecDeque::new(),
            max_turns,
        }
    }

    pub fn push_user(&mut self, content: String) {
        self.turns.push_back(ChatMessage::user(content));
        self.enforce_cap();
    }

    pub fn push_assistant(&mut self, content: String) {
        self.turns.push_back(ChatMessage::assistant(content));
        self.enforce_cap();
    }

    pub fn push_message(&mut self, msg: ChatMessage) {
        self.turns.push_back(msg);
        self.enforce_cap();
    }

    pub fn push_tool_call(&mut self, _call: ToolCall) {
        self.enforce_cap();
    }

    pub fn push_tool_result(&mut self, _result: String) {
        self.enforce_cap();
    }

    fn enforce_cap(&mut self) {
        while self.turns.len() > self.max_turns {
            self.turns.pop_front();
        }
    }

    pub fn as_messages(&self) -> Vec<ChatMessage> {
        let mut msgs = vec![self.system_prompt.clone()];
        msgs.extend(self.turns.iter().cloned());
        msgs
    }

    pub fn reset(&mut self) {
        self.turns.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cap() {
        let mut history = ConversationHistory::new(ChatMessage::system("system"), 2);
        history.push_user("1".into());
        history.push_user("2".into());
        history.push_user("3".into());
        assert_eq!(history.turns.len(), 2);
    }
}
