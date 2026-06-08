# scematica-ai

The LLM agent layer for the [Scematica](https://github.com/Meta-Oracle/Scematica)
Solana trading stack.

Wraps OpenAI-compatible providers (Groq, xAI, OpenRouter, Cerebras) behind a
single `AiClient`, and exposes purpose-built agents:

- **Chat** — an interactive tool-calling assistant (drives the dashboard chat tab).
- **Strategy** — tunes TP/SL/multiplier/regime live.
- **Risk** — scores pools and trades.
- **Debate / Report** — multi-agent deliberation and run summaries.

Includes the conversation history, tool dispatcher, and prompt scaffolding the
agents share.

## License

MIT
