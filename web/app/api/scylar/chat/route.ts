import { NextResponse, type NextRequest } from 'next/server'

import {
  SCYLAR_SYSTEM_PROMPT,
  configuredProviders,
  providerEnvVars,
  resolveProvider,
} from '@/lib/scylar/provider'

// Scylar's chat endpoint — streams tokens from whichever free provider has a key.
//
//   POST /api/scylar/chat   { messages: [{ role, content }, ...] }
//   -> text/event-stream of OpenAI-style chunks
//
// Server-side because the API key must never reach the browser bundle. The upstream
// providers also send no CORS headers, so a direct call from the page would fail even
// if the key were public.
//
// **No simulation branch.** With no provider configured this returns 503 rather than a
// canned reply. A fabricated answer from a chat avatar is indistinguishable from a real
// one, and the operator would have no way to tell the model never ran — the same reason
// the sniper's control POSTs 503 instead of faking success.
//
// Note for the mobile build: this route does not exist in the static export
// (`MOBILE_EXPORT=1`), so the Capacitor app must either call this origin over the
// network or carry its own key. See docs — that decision is still open.

export const dynamic = 'force-dynamic'
export const runtime = 'nodejs'

/** Cap on conversation history sent upstream, newest-first. */
const MAX_HISTORY = 20

/** Upstream timeout. Free tiers occasionally hang rather than refusing outright. */
const UPSTREAM_TIMEOUT_MS = 45_000

interface ChatMessage {
  role: 'user' | 'assistant' | 'system'
  content: string
}

function isChatMessage(v: unknown): v is ChatMessage {
  if (!v || typeof v !== 'object') return false
  const m = v as Record<string, unknown>
  return (
    (m.role === 'user' || m.role === 'assistant' || m.role === 'system') &&
    typeof m.content === 'string'
  )
}

export async function POST(request: NextRequest) {
  const provider = resolveProvider()
  if (!provider) {
    return NextResponse.json(
      {
        ok: false,
        error: 'No LLM provider configured.',
        detail:
          `Set one of ${providerEnvVars().join(', ')} in the server environment. ` +
          'Groq is recommended — it has the fastest free tier, which is what keeps ' +
          'the avatar animation responsive.',
      },
      { status: 503 },
    )
  }

  let body: unknown
  try {
    body = await request.json()
  } catch {
    return NextResponse.json({ ok: false, error: 'Malformed JSON body.' }, { status: 400 })
  }

  const raw = (body as { messages?: unknown })?.messages
  if (!Array.isArray(raw) || !raw.every(isChatMessage)) {
    return NextResponse.json(
      { ok: false, error: 'Body must be { messages: [{ role, content }, ...] }.' },
      { status: 400 },
    )
  }

  // Drop any client-supplied system turns: the persona is set here, and letting the
  // browser prepend its own system prompt is how a public endpoint becomes someone
  // else's free LLM proxy.
  const history = (raw as ChatMessage[]).filter((m) => m.role !== 'system').slice(-MAX_HISTORY)

  if (history.length === 0) {
    return NextResponse.json({ ok: false, error: 'No messages to answer.' }, { status: 400 })
  }

  const controller = new AbortController()
  const timeout = setTimeout(() => controller.abort(), UPSTREAM_TIMEOUT_MS)

  let upstream: Response
  try {
    upstream = await fetch(`${provider.baseUrl}/chat/completions`, {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
        Authorization: `Bearer ${provider.apiKey}`,
      },
      body: JSON.stringify({
        model: provider.model,
        stream: true,
        temperature: 0.75,
        max_tokens: 900,
        messages: [{ role: 'system', content: SCYLAR_SYSTEM_PROMPT }, ...history],
      }),
      signal: controller.signal,
    })
  } catch (err) {
    clearTimeout(timeout)
    const aborted = err instanceof Error && err.name === 'AbortError'
    return NextResponse.json(
      {
        ok: false,
        error: aborted
          ? `${provider.label} did not respond within ${UPSTREAM_TIMEOUT_MS / 1000}s.`
          : `Could not reach ${provider.label}.`,
        detail: err instanceof Error ? err.message : String(err),
        provider: provider.id,
      },
      { status: 502 },
    )
  }

  if (!upstream.ok || !upstream.body) {
    clearTimeout(timeout)
    // Surface the upstream text: on a free tier this is usually the rate-limit reason,
    // which is the single most useful thing to show rather than a generic failure.
    const detail = await upstream.text().catch(() => '')
    return NextResponse.json(
      {
        ok: false,
        error: `${provider.label} returned ${upstream.status}.`,
        detail: detail.slice(0, 600),
        provider: provider.id,
        hint:
          upstream.status === 429
            ? `Free-tier limit reached (${provider.freeTierNote}). Wait, or configure another provider.`
            : undefined,
      },
      { status: upstream.status === 429 ? 429 : 502 },
    )
  }

  // Pass the SSE stream straight through. The client already has to parse OpenAI-style
  // chunks, so re-encoding them here would add a format to maintain for no gain.
  const stream = new ReadableStream<Uint8Array>({
    async start(ctl) {
      const reader = upstream.body!.getReader()
      try {
        for (;;) {
          const { done, value } = await reader.read()
          if (done) break
          ctl.enqueue(value)
        }
      } catch {
        // Client navigated away or upstream cut out mid-stream. The partial response is
        // already rendered; nothing useful to add to it.
      } finally {
        clearTimeout(timeout)
        ctl.close()
        reader.releaseLock()
      }
    },
    cancel() {
      clearTimeout(timeout)
      controller.abort()
    },
  })

  return new Response(stream, {
    headers: {
      'Content-Type': 'text/event-stream; charset=utf-8',
      'Cache-Control': 'no-cache, no-transform',
      Connection: 'keep-alive',
      'X-Scylar-Provider': provider.id,
      'X-Scylar-Model': provider.model,
    },
  })
}

/** Which providers are configured — lets the UI show a status line without a chat turn. */
export async function GET() {
  const provider = resolveProvider()
  return NextResponse.json({
    ok: Boolean(provider),
    active: provider?.id ?? null,
    model: provider?.model ?? null,
    freeTierNote: provider?.freeTierNote ?? null,
    configured: configuredProviders(),
    checked: providerEnvVars(),
  })
}
