# Tasks: Chat Trading Interface

## Overview

Implementation tasks for the chat trading interface feature. Work proceeds in dependency order: types and API extensions first, then the core agent logic, then the dispatcher, then the TUI panel, and finally integration wiring.

---

## Tasks

- [ ] 1. Extend `AiRequest`/`AiResponse` types for tool-calling
  - Add optional `tools: Option<Vec<serde_json::Value>>` and `tool_choice: Option<String>` fields to `AiRequest` in `scematica-ai/src/types.rs`
  - Add `tool_calls: Option<Vec<ToolCallResponse>>` to `AiChoice`
  - Add new types `ToolCallResponse` and `ToolCallFunction` to `types.rs`
  - Ensure all new fields use `#[serde(skip_serializing_if = "Option::is_none")]` for backward compatibility
  - Verify existing `AiRequest` serialisation tests still pass
  - **Files**: `crates/scematica-ai/src/types.rs`

- [ ] 2. Implement `ToolDefinition` and `all_tools()` (scematica-ai)
  - Create `crates/scematica-ai/src/tool_definitions.rs`
  - Define `ToolDefinition` struct with `name`, `description`, `parameters` (JSON Schema `serde_json::Value`)
  - Implement `all_tools() -> Vec<ToolDefinition>` returning definitions for all seven tools: `swap_token`, `get_quote`, `get_balance`, `set_bot_mode`, `scan_arb`, `get_trade_history`, `get_bot_status`
  - Each tool's `parameters` must be a valid JSON Schema object with `type`, `properties`, `required` fields
  - Add `pub mod tool_definitions;` to `scematica-ai/src/lib.rs`
  - **Files**: `crates/scematica-ai/src/tool_definitions.rs`, `crates/scematica-ai/src/lib.rs`

- [ ] 3. Implement `ToolCall` enum and `PendingToolCall` types (scematica-ai)
  - Create `crates/scematica-ai/src/chat_types.rs`
  - Define `ToolCall` enum with variants: `SwapToken`, `GetQuote`, `GetBalance`, `SetBotMode`, `ScanArb`, `GetTradeHistory`, `GetBotStatus`
  - Define `RiskLevel` enum: `Safe`, `Moderate`, `High`
  - Define `PendingToolCall` struct: `call_id: String`, `call: ToolCall`, `summary: String`, `risk: RiskLevel`
  - Define `ToolResult` enum: `Success { message, signature, data }`, `Failure { message, recoverable }`
  - Define `ChatResponse` struct: `message: String`, `trade_entry: Option<TradeEntry>`, `tokens_used: u32`
  - Define `AgentOutput` enum: `NeedsConfirmation(PendingToolCall)`, `Reply(ChatResponse)`
  - Implement `classify_risk(call: &ToolCall) -> RiskLevel`
  - Add `pub mod chat_types;` to `scematica-ai/src/lib.rs`
  - **Files**: `crates/scematica-ai/src/chat_types.rs`, `crates/scematica-ai/src/lib.rs`

- [ ] 4. Implement `ConversationHistory` (scematica-ai)
  - Create `crates/scematica-ai/src/conversation.rs`
  - Implement `ConversationHistory` struct with `system_prompt: ChatMessage`, `turns: VecDeque<ChatMessage>`, `max_turns: usize`
  - Implement `push_user`, `push_assistant`, `push_tool_call`, `push_tool_result` methods
  - Implement `as_messages() -> Vec<ChatMessage>` — system prompt always first, then turns in order
  - Enforce `max_turns` cap: when `turns.len() > max_turns`, drop oldest entries (maintain pairs where possible)
  - Implement `reset()` to clear turns while keeping system prompt
  - Add `pub mod conversation;` to `scematica-ai/src/lib.rs`
  - **Files**: `crates/scematica-ai/src/conversation.rs`, `crates/scematica-ai/src/lib.rs`

- [ ] 4.1 Write property test for `ConversationHistory` window invariant
  - Using `proptest`, generate arbitrary sequences of push operations
  - Assert `history.as_messages().len() <= max_turns + 1` after every sequence
  - Assert system prompt is always `as_messages()[0]`
  - **Files**: `crates/scematica-ai/src/conversation.rs` (in `#[cfg(test)]` module)
  - **PBT**: proptest

- [ ] 5. Implement `resolve_symbol` (scematica-ai)
  - Create `crates/scematica-ai/src/symbol_resolver.rs`
  - Implement `resolve_symbol(symbol: &str) -> Result<Pubkey>` with the lookup order: known tokens → base58 pubkey parse + on-chain mint validation → Jupiter token list HTTP lookup
  - Cache the Jupiter token list in a `OnceLock<HashMap<String, Pubkey>>` (lazy-loaded on first unknown symbol)
  - Symbol matching is case-insensitive
  - For base58 pubkeys: validate the address is an actual SPL token mint by checking the account owner is `spl_token::id()`
  - Return descriptive `Err` for unknown symbols
  - Add `pub mod symbol_resolver;` to `scematica-ai/src/lib.rs`
  - **Files**: `crates/scematica-ai/src/symbol_resolver.rs`, `crates/scematica-ai/src/lib.rs`

- [ ] 5.1 Write property test for `resolve_symbol` idempotency
  - Using `proptest`, generate arbitrary strings
  - For strings that resolve successfully, assert `resolve_symbol(resolve_symbol(s).unwrap().to_string()) == resolve_symbol(s)`
  - For known token symbols (SOL, USDC, BONK, etc.), assert they resolve to the correct `known_tokens` constants
  - **Files**: `crates/scematica-ai/src/symbol_resolver.rs` (in `#[cfg(test)]` module)
  - **PBT**: proptest

- [ ] 6. Implement `ToolDispatcher` (scematica-ai)
  - Create `crates/scematica-ai/src/tool_dispatcher.rs`
  - Implement `ToolDispatcher` struct holding `Arc<RpcClient>`, `Arc<Wallet>`, `Arc<BotConfig>`, `Arc<AppState>` (for mode changes and trade history)
  - Implement `dispatch(call: ToolCall) -> Result<ToolResult>` routing each variant:
    - `SwapToken`: resolve symbols → `JupiterBuilder::get_quote` → `get_swap_transaction` → sign with wallet → RPC send → return signature
    - `GetQuote`: resolve symbols → `JupiterBuilder::get_quote` → return quote data as JSON
    - `GetBalance`: RPC `get_sol_balance` → return formatted balance
    - `SetBotMode`: validate mode string → update `AppState::active_mode`
    - `ScanArb`: placeholder returning "Arb scan triggered" (full integration in a later task)
    - `GetTradeHistory`: read `AppState::trades` → return last N entries as JSON
    - `GetBotStatus`: read `AppState::metrics` snapshot → return as JSON
  - Validate `amount_sol > 0.0` and `amount_sol <= wallet_balance` before any swap
  - Clamp `slippage_bps` to `[1, 2000]`
  - Add `pub mod tool_dispatcher;` to `scematica-ai/src/lib.rs`
  - **Files**: `crates/scematica-ai/src/tool_dispatcher.rs`, `crates/scematica-ai/src/lib.rs`
  - **Depends on**: Tasks 3, 5

- [ ] 6.1 Write property test for `ToolDispatcher` amount validation
  - Using `proptest`, generate arbitrary `f64` values for `amount_sol` and a fixed wallet balance
  - Assert that for all `amount_sol <= 0.0` or `amount_sol > wallet_balance`, `dispatch` returns `Ok(ToolResult::Failure { recoverable: false, .. })`
  - Assert that for all `slippage_bps` inputs, the value passed to Jupiter is always in `[1, 2000]`
  - Use a mock `JupiterBuilder` that records the slippage value it receives
  - **Files**: `crates/scematica-ai/src/tool_dispatcher.rs` (in `#[cfg(test)]` module)
  - **PBT**: proptest

- [ ] 7. Implement `ChatAgent` (scematica-ai)
  - Create `crates/scematica-ai/src/chat_agent.rs`
  - Implement `ChatAgent` struct with `client: AiClient`, `tools: Vec<serde_json::Value>`, `history: ConversationHistory`
  - Implement `new(client: AiClient) -> Self` — initialises tools from `all_tools()`, sets system prompt
  - Implement `process(&mut self, user_text: &str) -> Result<AgentOutput>`:
    - Append user message to history
    - Build `AiRequest` with tools and `tool_choice: "auto"`
    - Call `client.chat(request).await`
    - On error: roll back history (remove the user message just added), return `Err`
    - On `finish_reason == "tool_calls"`: parse tool call, classify risk; if `Safe` dispatch immediately and return `Reply`; otherwise return `NeedsConfirmation`
    - On `finish_reason == "stop"`: append assistant message to history, return `Reply`
  - Implement `execute_confirmed(&mut self, pending: PendingToolCall, dispatcher: &ToolDispatcher) -> Result<ChatResponse>`:
    - Call `dispatcher.dispatch(pending.call)`
    - Append tool result to history
    - Send follow-up request to Groq for natural language summary
    - Build `ChatResponse` with trade entry if applicable
  - Implement `reset_history(&mut self)`
  - Add `pub mod chat_agent;` and re-export `ChatAgent` from `scematica-ai/src/lib.rs`
  - **Files**: `crates/scematica-ai/src/chat_agent.rs`, `crates/scematica-ai/src/lib.rs`
  - **Depends on**: Tasks 1, 2, 3, 4, 6

- [ ] 7.1 Write property test for confirmation gate invariant
  - Using `proptest`, generate arbitrary `ToolCall` values
  - For all calls where `classify_risk(call) != RiskLevel::Safe`, assert that a mocked `ChatAgent::process` (with a mock client returning that tool call) returns `AgentOutput::NeedsConfirmation` and that `dispatcher.dispatch` was NOT called
  - Use a mock `ToolDispatcher` that records whether `dispatch` was called
  - **Files**: `crates/scematica-ai/src/chat_agent.rs` (in `#[cfg(test)]` module)
  - **PBT**: proptest

- [ ] 8. Implement `ChatPanel` TUI widget (scematica-dashboard)
  - Create `crates/scematica-dashboard/src/chat.rs`
  - Implement `ChatPanel` struct with `messages: VecDeque<ChatLine>`, `input_buffer: String`, `state: ChatPanelState`, `scroll_offset: usize`
  - Implement `ChatLine`, `ChatLineRole`, `ChatPanelState`, `ChatPanelAction` types
  - Implement `render(&self, f: &mut Frame, area: Rect)`:
    - Split area into message history pane (top) and input box (bottom, 3 lines)
    - Render messages with colour coding: cyan (User), white (Assistant), yellow (System), red (Error)
    - Show "⏳ thinking…" spinner line when `state == WaitingForResponse` or other loading states
    - Show highlighted confirmation banner when `state == WaitingForConfirmation`
    - Word-wrap long messages; never panic on empty history or oversized messages
  - Implement `handle_key(&mut self, key: KeyEvent) -> Option<ChatPanelAction>`:
    - `Enter` with non-empty buffer → `Submit(buffer)`, clear buffer
    - `Enter` with empty buffer → `None`
    - `'y'` when `WaitingForConfirmation` → `Confirm`
    - `'n'` when `WaitingForConfirmation` → `Reject`
    - Any key when `WaitingForResponse` → `None` (blocked)
    - Character keys when `Idle` → append to buffer, return `None`
    - `Backspace` → remove last char from buffer
    - `PageUp`/`PageDown` → `ScrollUp`/`ScrollDown`
  - Add `pub mod chat;` to `scematica-dashboard/src/lib.rs`
  - **Files**: `crates/scematica-dashboard/src/chat.rs`, `crates/scematica-dashboard/src/lib.rs`

- [ ] 8.1 Write property test for `ChatPanel::handle_key` input buffer
  - Using `proptest`, generate arbitrary sequences of printable character key events
  - Assert that `input_buffer` equals the concatenation of those characters (no dropped or duplicated chars)
  - Assert that `Backspace` removes exactly one character from the end
  - **Files**: `crates/scematica-dashboard/src/chat.rs` (in `#[cfg(test)]` module)
  - **PBT**: proptest

- [ ] 9. Add Chat tab to the dashboard (scematica-dashboard)
  - Update `AppState` in `app.rs` to add `chat_panel: RwLock<ChatPanel>` field
  - Update `AppState::next_tab` and `prev_tab` to cycle through 5 tabs (0–4) instead of 4
  - Update `ui.rs` `render_tabs` to include "Chat" as the fifth tab title
  - Update `ui.rs` `render` match arm to call `render_chat(f, chunks[2], state)` for tab index 4
  - Implement `render_chat` function that delegates to `state.chat_panel.read().render(f, area)`
  - Update `events.rs` `handle_key` to forward character/enter/backspace keys to `ChatPanel::handle_key` when tab 4 is active, and emit a new `DashboardAction::Chat(ChatPanelAction)` variant
  - Update `main.rs` to handle `DashboardAction::Chat` events
  - **Files**: `crates/scematica-dashboard/src/app.rs`, `crates/scematica-dashboard/src/ui.rs`, `crates/scematica-dashboard/src/events.rs`, `crates/scematica-dashboard/src/main.rs`
  - **Depends on**: Task 8

- [ ] 10. Wire `ChatAgent` into the dashboard event loop (scematica-dashboard)
  - Add `scematica-ai` and `scematica-executor` as dependencies in `scematica-dashboard/Cargo.toml`
  - In `main.rs`, construct `AiClient::from_env()`, `ToolDispatcher::new(rpc, wallet, config, state)`, and `ChatAgent::new(client)`
  - Wrap `ChatAgent` in `Arc<Mutex<ChatAgent>>` for sharing with the async task
  - On `DashboardAction::Chat(ChatPanelAction::Submit(text))`:
    - Set `chat_panel.state = WaitingForResponse`
    - Spawn a `tokio::task` that calls `chat_agent.lock().await.process(&text).await`
    - Send result back to the main loop via an `mpsc` channel
  - On receiving `AgentOutput::NeedsConfirmation(pending)`:
    - Push the summary as an Assistant message to `chat_panel`
    - Set `chat_panel.state = WaitingForConfirmation(pending)`
  - On `DashboardAction::Chat(ChatPanelAction::Confirm)`:
    - Spawn task calling `chat_agent.execute_confirmed(pending, &dispatcher).await`
    - On `ChatResponse`: push message to panel, push `trade_entry` to `AppState::trades` if present, set state to `Idle`
  - On `DashboardAction::Chat(ChatPanelAction::Reject)`:
    - Push "Trade cancelled." as System message
    - Set state to `Idle`
  - On AI error: push error message to panel, set state to `Idle`
  - **Files**: `crates/scematica-dashboard/src/main.rs`, `crates/scematica-dashboard/Cargo.toml`
  - **Depends on**: Tasks 7, 9

- [ ] 11. Integration test: end-to-end swap flow
  - Write an integration test in `scematica-ai` that:
    1. Creates a `ChatAgent` with a mock `AiClient` returning a `swap_token` tool call
    2. Calls `process("buy 0.1 SOL of BONK")` and asserts `AgentOutput::NeedsConfirmation`
    3. Creates a mock `ToolDispatcher` returning `ToolResult::Success { signature: Some("abc123"), .. }`
    4. Calls `execute_confirmed(pending, &mock_dispatcher)` and asserts `ChatResponse.trade_entry.is_some()`
    5. Asserts `ChatResponse.message` contains the signature
  - **Files**: `crates/scematica-ai/tests/chat_integration.rs`
  - **Depends on**: Tasks 7, 6

- [ ] 12. Integration test: error recovery
  - Write an integration test that:
    1. Creates a `ChatAgent` with a mock `AiClient` that returns an error on the first call
    2. Calls `process("buy 0.1 SOL of BONK")` and asserts `Err(_)` is returned
    3. Asserts `agent.history.turns.len()` equals its pre-call value (history rolled back)
    4. Calls `process("what is my balance?")` with a mock that now succeeds — asserts `Ok(AgentOutput::Reply(_))`
  - **Files**: `crates/scematica-ai/tests/chat_integration.rs`
  - **Depends on**: Task 7

- [ ] 13. Verify build and run `cargo check`
  - Run `cargo check --workspace` and fix any compilation errors
  - Run `cargo test --workspace` and ensure all new tests pass
  - Verify the dashboard binary compiles: `cargo build -p scematica-dashboard`
  - **Files**: workspace root
  - **Depends on**: Tasks 1–12
