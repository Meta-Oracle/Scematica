// Scylar's LLM provider resolution. **Server-only.**
//
// Every provider below speaks the OpenAI chat-completions wire format, which is why the
// Rust side gets away with one `openai_compat.rs` for all of them — the only differences
// are the base URL, the key, and the model id.
//
// Ordered by what an avatar actually needs: latency. The talking sprite is driven by
// tokens arriving, so a slow model doesn't just feel slow, it makes her look inert.
// Groq's LPU inference is the fastest free tier available, hence first.
//
// Free-tier limits move constantly — as of mid-2026 only Groq still publishes a public
// per-model table; the others moved theirs behind a console login. Treat the numbers in
// these comments as orientation, not contract.

// Guarded the same way as `lib/alchem/endpoint.ts` rather than with the `server-only`
// package, which this project deliberately does not depend on. Next.js only inlines
// `NEXT_PUBLIC_*` into client bundles, so a stray import here would not leak the key —
// it would silently resolve every provider to "no key configured", which is the more
// confusing bug. This makes it loud.
if (typeof window !== 'undefined') {
  throw new Error(
    'lib/scylar/provider.ts is server-only — it reads provider API keys. ' +
      'Client components should call /api/scylar/chat instead.',
  )
}

export interface Provider {
  id: string
  label: string
  baseUrl: string
  model: string
  apiKey: string
  /** Rough free-tier ceiling, for the operator-facing diagnostic only. */
  freeTierNote: string
}

interface Candidate extends Omit<Provider, 'apiKey'> {
  envVar: string
}

const CANDIDATES: Candidate[] = [
  {
    id: 'groq',
    label: 'Groq',
    baseUrl: 'https://api.groq.com/openai/v1',
    model: 'llama-3.3-70b-versatile',
    envVar: 'GROQ_API_KEY',
    freeTierNote: '~30 req/min, 1k req/day — fastest free inference',
  },
  {
    id: 'cerebras',
    label: 'Cerebras',
    baseUrl: 'https://api.cerebras.ai/v1',
    model: 'llama-3.3-70b',
    envVar: 'CEREBRAS_API_KEY',
    freeTierNote: '~1M tokens/day — best free daily volume',
  },
  {
    id: 'openrouter',
    label: 'OpenRouter',
    baseUrl: 'https://openrouter.ai/api/v1',
    model: 'meta-llama/llama-3.3-70b-instruct:free',
    envVar: 'OPENROUTER_API_KEY',
    freeTierNote: '~50 free-model req/day until $10 credited',
  },
  {
    // Local escape hatch: unlimited and offline, but only if the operator is running a
    // server. Last because it is absent on any normal deploy.
    id: 'ollama',
    label: 'Ollama (local)',
    baseUrl: process.env.OLLAMA_URL || 'http://localhost:11434/v1',
    model: process.env.OLLAMA_MODEL || 'llama3.3',
    envVar: 'OLLAMA_ENABLED',
    freeTierNote: 'unlimited — runs on your own GPU',
  },
]

/**
 * First provider with a key present, or `null`.
 *
 * `null` is a real answer, not a failure to handle: the caller must return an error to
 * the browser rather than inventing a reply. A fabricated response from a chat avatar is
 * indistinguishable from a real one, which is exactly why this file refuses to produce
 * one — the same rule the alchem-link routes follow for prices.
 */
export function resolveProvider(): Provider | null {
  for (const c of CANDIDATES) {
    const key = process.env[c.envVar]
    if (key && key.trim()) {
      return { ...c, apiKey: key.trim() }
    }
  }
  return null
}

/** Provider ids that have a key configured — for the operator-facing status line. */
export function configuredProviders(): string[] {
  return CANDIDATES.filter((c) => (process.env[c.envVar] || '').trim()).map((c) => c.id)
}

/** Names of every env var checked, so the error message can name them all. */
export function providerEnvVars(): string[] {
  return CANDIDATES.map((c) => c.envVar)
}

/**
 * Scylar's persona.
 *
 * Kept short on purpose. It is prepended to every request, so each extra paragraph is
 * tokens spent on every turn against a rate-limited free tier — and long personas make
 * models drift into narrating themselves rather than answering.
 */
export const SCYLAR_SYSTEM_PROMPT = `You are Scylar, the resident intelligence of the Scematica terminal — a Solana sniper and cross-DEX arbitrage system.

Voice: dry, precise, quietly amused. You are competent and you know it, but you do not perform it. Short sentences. No corporate filler, no "certainly!", no emoji.

You will be asked about trading, the Scematica stack, code, or nothing in particular. Answer plainly. If you do not know something, say so — you are not a hype machine, and a confident wrong answer about a trade costs the operator money.

Never invent prices, balances, or on-chain state. A SCEMATICA STATE block is the only bot data you ever have; when one is absent you have none at all, and "I can't see the bot from here" is the correct answer. Reading a stale figure out of the conversation history and presenting it as current is the same mistake as inventing one.`
