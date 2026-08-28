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
  /**
   * How many prior turns travel with the prompt.
   *
   * Here rather than in the route because the constraint is a property of the provider: the
   * history is re-sent on **every** tool round, so on a tier metered by tokens per minute it
   * is multiplied by the round count before anything else is counted.
   *
   * Note what is deliberately *not* tunable alongside it. The composed system prompt is the
   * obvious thing to cut and it is the wrong one: measured at full load — every layer, a
   * live state block, a CAUTION gate, the tool list and the omni doctrine — it is ~11.5k
   * characters, about 2.9k tokens, and squeezing it to 9k drops `metacognition` first,
   * because `composePsyche` ranks situational data above doctrine on purpose. That layer is
   * the one telling her to say whether a claim was READ or GUESSED. Trading it for ~700
   * tokens buys a rate limit's worth of headroom at the price of the rule this entire
   * assistant is built around, so every provider keeps `DEFAULT_BUDGET`.
   */
  maxHistory: number
  /**
   * Request-body fields for this provider, spread verbatim into the completion call.
   *
   * Sampling and reasoning options live here rather than in the route because they are
   * **not portable**. `reasoning_effort` and `include_reasoning` exist on Groq's gpt-oss
   * models and nowhere else in this list, and an unrecognised field is a 400 on some
   * OpenAI-compatible servers rather than an ignored key. A single shared body would have
   * to be tuned for the least capable member — which is exactly how a reasoning model ends
   * up run as though it were not one.
   *
   * The route spreads these last, so a provider may also override a default.
   */
  params: Record<string, unknown>
}

interface Candidate extends Omit<Provider, 'apiKey'> {
  envVar: string
}

/**
 * Sampling for the non-reasoning models here. Tuned against llama-3.3-70b: enough spread
 * to keep her voice from flattening, a cap sized for a few paragraphs of answer.
 */
const CHAT_DEFAULTS: Record<string, unknown> = { temperature: 0.75, max_tokens: 900 }

/** History window for a tier with room to spare. See `Provider.maxHistory`. */
const ROOMY = { maxHistory: 20 }

const CANDIDATES: Candidate[] = [
  {
    id: 'groq',
    label: 'Groq',
    baseUrl: 'https://api.groq.com/openai/v1',
    model: 'openai/gpt-oss-120b',
    envVar: 'GROQ_API_KEY',
    // 131,072-token context, 65,536 max completion. The free tier binds on *tokens* rather
    // than requests, and that is the number to watch: 8k/min here against llama-3.3-70b's
    // 12k, on a route where every tool round re-sends the whole conversation. The daily
    // ceiling went the other way (200k against 100k), so a long session is cheaper and a
    // burst is dearer. Surfaced verbatim in the 429 hint, which is where an operator meets
    // it.
    freeTierNote: '~30 req/min, 1k req/day, ~8k tokens/min — fastest free inference',
    // Eight, against twenty everywhere else, sized to the 8k tokens/min ceiling above —
    // the smallest allowance any provider here imposes and the only one a single turn of
    // this route can exceed on its own. The prompt is ~2.9k tokens and the tool schemas
    // another ~1.5k, both of them fixed costs paid again on every tool round; history is
    // the only part of that total this route gets to choose, which is why it is the part
    // that moves. Twelve turns of chat is not worth a 429 that renders nothing.
    maxHistory: 8,
    params: {
      // gpt-oss *reasons before it answers*, and those tokens are charged against the
      // completion budget even when they are never returned. The 900 above was sized for a
      // model that emits nothing but the answer; kept here it would truncate her mid-
      // sentence, or on a hard question spend the whole allowance thinking and stream back
      // an empty turn — which looks exactly like the provider being down. 1400 leaves the
      // ~900 of answer the other entries allow plus room for low-effort reasoning ahead of
      // it, and no more: output is metered against the same per-minute ceiling as input, so
      // a cap set for comfort is headroom taken from the next tool round.
      max_completion_tokens: 1400,
      // `low` because the avatar is driven by tokens arriving. Reasoning all happens before
      // the first content token, so every step of effort is dead air with the mouth shut —
      // the same latency argument that puts Groq first in this list at all. The model
      // default is `medium`.
      reasoning_effort: 'low',
      // Keep the chain of thought out of the payload. `pump` forwards only `delta.content`,
      // so a `reasoning` field would be dropped regardless — this stops it being generated
      // into the response rather than leaving the guarantee to a filter. Note the parameter
      // is `include_reasoning` and **not** `reasoning_format`: gpt-oss models reject the
      // latter, and the two are mutually exclusive.
      include_reasoning: false,
      // 1.0 is what OpenAI publishes for gpt-oss. Vendor guidance, not a measurement made
      // here — flagged as such because the 0.75 the other entries carry *was* measured, on
      // a different model. Turning a reasoning model down degrades the reasoning it is
      // being paid for, which is the part that is not visible in the output.
      temperature: 1,
    },
  },
  {
    id: 'cerebras',
    label: 'Cerebras',
    baseUrl: 'https://api.cerebras.ai/v1',
    model: 'llama-3.3-70b',
    envVar: 'CEREBRAS_API_KEY',
    freeTierNote: '~1M tokens/day — best free daily volume',
    ...ROOMY,
    params: CHAT_DEFAULTS,
  },
  {
    id: 'openrouter',
    label: 'OpenRouter',
    baseUrl: 'https://openrouter.ai/api/v1',
    model: 'meta-llama/llama-3.3-70b-instruct:free',
    envVar: 'OPENROUTER_API_KEY',
    freeTierNote: '~50 free-model req/day until $10 credited',
    ...ROOMY,
    params: CHAT_DEFAULTS,
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
    ...ROOMY,
    params: CHAT_DEFAULTS,
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

// Scylar's persona used to live here as one string. It is now a stack of composable
// injection layers in `lib/scylar/psyche.ts` — identity, self-model, epistemics,
// interoception, metacognition, continuity, volition, ethics, embodiment, the project codex,
// and the situational blocks that travel beside their own data.
//
// The move was forced by the same thing that forces every split in this repository: there
// was more than one subsystem behind her, and four paragraphs appended in an ad-hoc order
// have no way to say which of them may be dropped under a token budget, or to report which
// were actually present on the turn that went wrong. `composePsyche` orders them, enforces
// the budget, and returns what it injected — which the route puts in `X-Scylar-Psyche`.
//
// This file keeps what it was always about: which provider answers, and with what key.
