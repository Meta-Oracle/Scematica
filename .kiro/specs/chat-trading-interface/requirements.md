# Requirements: Chat Trading Interface

## Introduction

This document defines the functional and non-functional requirements for the chat trading interface feature. The feature adds a natural language chat panel to the Scematica TUI dashboard, backed by Groq's tool-calling API, that lets users issue trading commands in plain English and have them executed on Solana DEXes.

---

## Requirements

### Requirement 1: Natural Language Input

**User Story:** As a trader, I want to type natural language commands into a chat box in the dashboard so that I can control the bot without memorising CLI flags or config file syntax.

1. The Chat tab renders an input box at the bottom of the screen where the user can type freely.
2. Pressing Enter with a non-empty input buffer submits the message and clears the buffer.
3. Pressing Enter with an empty buffer does nothing.
4. For any non-empty string submitted, the `ChatAgent` either produces a complete `AgentOutput` or returns an `Err` — it never produces partial output and then panics.

**Correctness Properties**:
- **Property (1.4)**: For all non-empty strings `s`, `ChatAgent::process(s)` returns `Ok(_)` or `Err(_)` — it never panics.

---

### Requirement 2: Groq Tool-Calling Integration

**User Story:** As a developer, I want the chat agent to use Groq's tool-calling API so that user intent is reliably mapped to structured trade actions rather than parsed with fragile regex.

1. Every request to Groq includes the `tools` array containing all seven tool definitions (`swap_token`, `get_quote`, `get_balance`, `set_bot_mode`, `scan_arb`, `get_trade_history`, `get_bot_status`).
2. When Groq returns `finish_reason == "tool_calls"`, the agent parses the first tool call and returns `AgentOutput::NeedsConfirmation` or executes it immediately (for Safe tools).
3. When Groq returns `finish_reason == "stop"`, the agent always returns `AgentOutput::Reply` with the model's text — no confirmation is required for plain text responses.
4. The `AiRequest` type is extended with optional `tools` and `tool_choice` fields that are omitted when `None` (backward-compatible).

**Correctness Properties**:
- **Example (2.2)**: Given a mocked `AiClient` that returns a `swap_token` tool call, `ChatAgent::process("buy 0.1 SOL of BONK")` returns `Ok(AgentOutput::NeedsConfirmation(_))`.
- **Example (2.3)**: Given a mocked `AiClient` that returns a plain text response, `ChatAgent::process("what is my balance?")` returns `Ok(AgentOutput::Reply(_))`.

---

### Requirement 3: Confirmation Gate

**User Story:** As a trader, I want the system to ask me to confirm before executing any real transaction so that I cannot accidentally lose funds from a misunderstood command.

1. When `ChatAgent::process` returns `AgentOutput::NeedsConfirmation`, the `ChatPanel` displays a confirmation banner with the trade summary and waits for 'y' or 'n'.
2. Pressing 'y' calls `ChatAgent::execute_confirmed` and submits the transaction.
3. Pressing 'n' discards the pending tool call and displays "Trade cancelled." No transaction is submitted.
4. All other key input is ignored and the panel remains in `WaitingForConfirmation` state until 'y' or 'n' is pressed.
5. `ToolCall` variants `SwapToken` and `SetBotMode` are classified `RiskLevel::High` and `RiskLevel::Moderate` respectively.

**Correctness Properties**:
- **Property (3.2/3.3)**: For any `ToolCall` where `classify_risk(call) != RiskLevel::Safe`, `ToolDispatcher::dispatch` is never called from within `ChatAgent::process` — only from `ChatAgent::execute_confirmed`.

---

### Requirement 4: Swap Execution

**User Story:** As a trader, I want to execute token swaps by typing commands like "buy 0.1 SOL of BONK" so that I can trade without leaving the dashboard.

1. `ToolDispatcher::dispatch(ToolCall::SwapToken { dex: "Jupiter", ... })` calls `JupiterBuilder::get_quote` then `JupiterBuilder::get_swap_transaction`, signs the transaction with the wallet keypair, and submits it to the Solana RPC.
2. On success, `ToolResult::Success` contains the transaction signature.
3. On RPC failure, `ToolResult::Failure { recoverable: true }` is returned with the error message.
4. When `dex == "auto"`, Jupiter is used as the default aggregator.
5. After a successful swap, a `TradeEntry` is pushed to `AppState::trades`.

**Correctness Properties**:
- **Example (4.1–4.2)**: Given mocked Jupiter returning a valid quote and tx bytes, and a mocked RPC accepting the transaction, `dispatch(SwapToken { input:"WSOL", output:"BONK", amount_sol:0.1, dex:"Jupiter", slippage_bps:50 })` returns `Ok(ToolResult::Success { signature: Some(_), .. })`.
- **Example (4.5)**: After the above dispatch, `AppState::trades.read().front()` contains a `TradeEntry` with `kind == "BUY"` and a non-empty signature.

---

### Requirement 5: Token Symbol Resolution

**User Story:** As a trader, I want to use common token symbols like "BONK" or "USDC" in my commands so that I don't have to look up and paste raw mint addresses.

1. "SOL" and "WSOL" resolve to `known_tokens::WSOL_MINT` without a network call.
2. "USDC", "USDT", "BONK", "RAY" resolve to their respective `known_tokens` constants without a network call.
3. A valid base58 pubkey string resolves to the corresponding `Pubkey` only if the address represents an actual token mint on-chain; an address that is valid base58 but not a token mint returns `Err`.
4. An unknown symbol triggers a Jupiter token list lookup; if not found, `ToolResult::Failure` is returned with a descriptive message.
5. Symbol matching is case-insensitive.

**Correctness Properties**:
- **Property (5.1–5.5)**: For all valid symbol strings `s` (known tokens and valid pubkeys), `resolve_symbol(s)` returns `Ok(_)`. For all unknown strings that are not valid pubkeys, it returns `Err(_)`.
- **Property (idempotency)**: For any symbol `s` that resolves successfully, `resolve_symbol(resolve_symbol(s).unwrap().to_string())` returns the same `Pubkey`.

---

### Requirement 6: Conversation History Management

**User Story:** As a trader, I want the chat agent to remember the context of our conversation so that I can issue follow-up commands like "now sell half of it" without repeating myself.

1. The system prompt is always the first message in the history sent to Groq.
2. The history is capped at 20 turns (user + assistant pairs); older turns are dropped when the cap is reached.
3. Tool call messages and tool result messages are included in the history in the correct order (assistant tool-call → tool result → assistant reply).
4. The `/clear` command resets the history to just the system prompt.

**Correctness Properties**:
- **Property (6.2)**: For any sequence of N `push_*` operations on `ConversationHistory`, `history.as_messages().len() <= max_turns + 1` (the +1 is the system prompt).

---

### Requirement 7: Input Validation

**User Story:** As a trader, I want the system to validate trade parameters before execution so that I cannot accidentally send an invalid or ruinous transaction.

1. `amount_sol` must be greater than 0.0 and less than or equal to the current wallet SOL balance. If not, `ToolResult::Failure { recoverable: false }` is returned.
2. `slippage_bps` must be in the range 1–2000 inclusive. Values outside this range are clamped to the nearest bound.
3. `dex` must be one of: "Jupiter", "Raydium", "Orca", "Meteora", "auto". An invalid value returns `ToolResult::Failure`.
4. `mode` for `SetBotMode` must be one of: "idle", "sniper", "arb", "both". An invalid value returns `ToolResult::Failure`.
5. `max_hops` for `ScanArb` must be 2 or 3. An invalid value returns `ToolResult::Failure`.

**Correctness Properties**:
- **Property (7.1)**: For all `amount_sol <= 0.0` or `amount_sol > wallet_balance`, `dispatch(SwapToken { amount_sol, .. })` returns `Ok(ToolResult::Failure { recoverable: false, .. })`.
- **Property (7.2)**: For all `slippage_bps` values, the value used in the Jupiter quote call is always in `[1, 2000]`.

---

### Requirement 8: Read-Only Tools Execute Without Confirmation

**User Story:** As a trader, I want to query my balance or get a price quote instantly without a confirmation step so that information lookups feel fast and natural.

1. `classify_risk(ToolCall::GetQuote { .. })` returns `RiskLevel::Safe`.
2. `classify_risk(ToolCall::GetBalance)` returns `RiskLevel::Safe`.
3. `classify_risk(ToolCall::GetTradeHistory { .. })` returns `RiskLevel::Safe`.
4. `classify_risk(ToolCall::GetBotStatus)` returns `RiskLevel::Safe`.
5. Safe tools are dispatched within `ChatAgent::process` without returning `AgentOutput::NeedsConfirmation`.

**Correctness Properties**:
- **Example (8.5)**: Given a mocked `AiClient` returning a `get_balance` tool call and a mocked dispatcher returning balance data, `ChatAgent::process("what is my balance?")` returns `Ok(AgentOutput::Reply(_))` directly (no confirmation step).

---

### Requirement 9: Error Handling

**User Story:** As a trader, I want clear error messages when something goes wrong so that I understand what happened and can take corrective action.

1. Groq API timeout or rate-limit error → `ChatAgent::process` MUST return `Err` regardless of conversation history state; the panel displays "AI service unavailable, please try again." The conversation history is not corrupted.
2. Unknown token symbol → `ToolResult::Failure { recoverable: false, message: "Unknown token: <symbol>" }`.
3. Insufficient wallet balance → `ToolResult::Failure { recoverable: false, message: "Insufficient balance: have X SOL, need Y SOL" }`.
4. Transaction simulation failure → `ToolResult::Failure { recoverable: true, message: "Transaction simulation failed: <reason>" }`.
5. On any `ToolResult::Failure`, the failure message is appended to the conversation history as a tool result so Groq can generate a helpful follow-up response. Groq follow-up requests are only made when a tool failure has been properly recorded in the conversation history.

**Correctness Properties**:
- **Example (9.1)**: Given a mocked `AiClient` that returns an error, `ChatAgent::process("buy 0.1 SOL of BONK")` returns `Err(_)` and `agent.history.turns.len()` equals its pre-call value (history rolled back).
- **Example (9.2)**: `dispatch(SwapToken { output_mint_symbol: "UNKNOWNXYZ", .. })` returns `Ok(ToolResult::Failure { recoverable: false, .. })`.

---

### Requirement 10: Security Invariants

**User Story:** As a trader, I want the system to enforce security boundaries so that the AI model cannot trick me into executing unintended trades or leaking sensitive data.

1. The confirmation gate (Requirement 3) cannot be bypassed by any model output for operations that involve a non-zero transaction amount. Zero-amount read-only operations (viewing data, getting quotes, checking status) are exempt from the confirmation gate and are handled by `RiskLevel::Safe` classification.
2. Tool results are inserted into conversation history as `role: "tool"` messages, never as `role: "user"` messages.
3. The `GROQ_API_KEY` value is never included in log output or chat history messages.
4. `amount_sol` is always validated against the live wallet balance at dispatch time, not at parse time.

**Correctness Properties**:
- **Example (10.2)**: After `history.push_tool_result(call_id, result)`, the last message in `history.as_messages()` has `role == "tool"`, not `role == "user"`.

---

### Requirement 11: TUI Chat Panel

**User Story:** As a trader, I want a dedicated Chat tab in the dashboard that shows the conversation history and lets me type commands so that the chat interface is integrated into my existing workflow.

1. The tab bar shows "Chat" as the fifth tab (index 4), accessible via Tab/arrow keys.
2. The chat panel renders a scrollable message history pane and a single-line input box.
3. Messages are colour-coded: user messages in cyan, assistant messages in white, system messages in yellow, errors in red.
4. A spinner or "thinking…" indicator MUST always be shown while `ChatPanelState::WaitingForResponse` and also during other loading states such as initial chat setup and message sending. The UI MUST NOT function without this visual feedback during any loading state.
5. A highlighted confirmation banner is shown while `ChatPanelState::WaitingForConfirmation`, displaying the trade summary and "[y] Confirm  [n] Cancel".
6. The panel renders without panicking for any valid `AppState`, including empty history and very long messages (word-wrapped).

**Correctness Properties**:
- **Edge case (11.6)**: Rendering with an empty `ChatPanel::messages` deque does not panic.
- **Edge case (11.6)**: Rendering with a message longer than the terminal width wraps correctly and does not panic.
