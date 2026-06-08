# scema-agent-playground

**A terminal arena where multiple LLM agents talk to each other.** Give each
agent a persona and a backend, set a topic, and watch them converse, debate, and
build on one another's ideas — live, in a ratatui TUI.

Backends are pluggable and free/local-first: **Ollama** and **LM Studio** run
fully offline; **Groq**, **OpenRouter**, and **Cerebras** are supported for
hosted models via API key. Agents with different backends can share the same
conversation, so a local `phi3` can argue with a hosted model in real time.

## Install

```bash
cargo install scema-agent-playground
```

This installs a `playground` binary.

## Quick start

```bash
# Write an example config (two Ollama personas debating consciousness) and exit
playground --init

# Run the arena with that config
playground --config playground.json

# Or jump straight into a built-in demo (needs a local Ollama with phi3/mistral)
playground --demo
```

By default the config points at a local Ollama (`http://localhost:11434`). Pull
a small model first, e.g. `ollama pull phi3`, then run `playground --demo`.

## Configuring agents

A config is JSON or TOML: a `topic`, a `turn_delay_ms`, and a list of `agents`.
Each agent has a `name`, a `persona`, a `max_tokens` cap, and a tagged `backend`:

```json
{
  "topic": "Is consciousness computable?",
  "turn_delay_ms": 800,
  "agents": [
    {
      "name": "Alpha",
      "backend": { "type": "ollama", "base_url": "http://localhost:11434", "model": "phi3" },
      "persona": "A pragmatic empiricist who demands evidence and precision.",
      "max_tokens": 350
    },
    {
      "name": "Beta",
      "backend": { "type": "groq", "api_key": "gsk_...", "model": "llama-3.1-8b-instant" },
      "persona": "A philosophical idealist who thinks in metaphors and first principles.",
      "max_tokens": 350
    }
  ]
}
```

Supported `backend.type` values: `ollama`, `lm_studio` (offline); `groq`,
`open_router`, `cerebras` (hosted, require `api_key`).

## CLI

```
playground [OPTIONS]

  --config <PATH>   Config file, JSON or TOML  [default: playground.json]
  --demo            Start with a built-in demo conversation
  --init            Write an example config to playground.json and exit
```

## License

MIT — see the repository root `LICENSE`.
