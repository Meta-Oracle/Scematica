import { NextResponse, type NextRequest } from 'next/server'

import { buildBriefing, contextSystemMessage } from '@/lib/scylar/context'
import { cautionInstruction, holdInstruction, readGate, recordAnswer } from '@/lib/scylar/gate'
import {
  SCYLAR_SYSTEM_PROMPT,
  configuredProviders,
  providerEnvVars,
  resolveProvider,
} from '@/lib/scylar/provider'
import { SEAL_TOOL, TOOLS, availableTools, runTool, toolDefinitions } from '@/lib/scylar/tools'
import type { ToolAvailability, ToolOutcome } from '@/lib/scylar/tools'
import { OMNI_INSTRUCTION, omniConfig, sealingAllowed } from '@/lib/scylar/omni'

// Scylar's chat endpoint — streams tokens from whichever free provider has a key, and
// runs her read-only tool calls against the operator's bot in between.
//
//   POST /api/scylar/chat   { messages: [{ role, content }, ...], context?: boolean }
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
// The stream is re-encoded rather than proxied byte-for-byte. It used to be a straight
// passthrough, which was right when nothing here needed to understand the payload; now
// the tool loop has to read `tool_calls` out of it anyway, so parsing is happening
// regardless and a second, unparsed path would only be a way for the two to disagree.
// Emitted frames stay OpenAI-shaped so the client parser is unchanged, plus one custom
// `{"scylar":{...}}` frame carrying tool activity — frames with no `choices` are ignored
// by anything that only reads deltas.
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

/**
 * How many times she may call tools before having to answer.
 *
 * Three is enough for "look at decisions → notice a mint → pull its trades → answer" and
 * short enough that a model stuck in a loop costs three requests, not a rate limit. The
 * cap is announced to her when hit, so the last answer is written knowingly rather than
 * cut off.
 */
const MAX_TOOL_ROUNDS = 3

interface ChatMessage {
  role: 'user' | 'assistant' | 'system'
  content: string
}

interface ToolCallAccum {
  id: string
  name: string
  args: string
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

  // Optional live-state block. Opt-in per request rather than always-on: it costs a
  // round trip and a few hundred tokens on every turn, and most questions ("explain
  // fractional Kelly") do not need it. See lib/scylar/context.ts.
  const wantContext = (body as { context?: unknown })?.context === true
  const briefing = wantContext
    ? await buildBriefing(request.nextUrl.origin)
    : { source: 'unavailable' as const, text: null }

  // The cognitive gate, from scematica-sentience via the Rust API. Only consulted when
  // she is about to speak from bot state — a question about Kelly sizing does not become
  // untrustworthy because the sniper's metrics went stale.
  const gate = wantContext ? await readGate(request.nextUrl.origin) : null
  const held = gate?.verdict === 'hold'

  // A HOLD removes the data, not the turn. See `holdInstruction` for why the 409 that
  // used to live here was the wrong shape of enforcement.
  const contextBlock = held ? null : contextSystemMessage(briefing)

  // Tools are offered only alongside context. Without a reachable bot every call would
  // come back "could not reach the API", which teaches the model to stop trying and
  // wastes a round doing it. On HOLD they are off for a stronger reason: they read the
  // very files the gate has just declared untrustworthy, so leaving them on would hand
  // back through the side door exactly what withholding the block took away.
  const useTools = wantContext && briefing.source !== 'unavailable' && !held

  // One system message, not five.
  //
  // Llama 3.x's chat template is trained on a single system turn; consecutive ones are
  // out of distribution, and everything here lands in front of up to 20 turns of history.
  // Measured against llama-3.3-70b with a real state block and eight turns of history:
  // four separate system messages recovered the session PnL 5/5 and the agent's
  // train-step count 4/5, concatenated into one both were 5/5. A small margin, but free,
  // and the failure it removes is the model silently ignoring the block rather than
  // saying it cannot see one.
  const systemParts: string[] = [SCYLAR_SYSTEM_PROMPT]
  if (contextBlock) systemParts.push(contextBlock)
  if (held && gate) {
    systemParts.push(holdInstruction(gate))
  } else {
    // The overlay's own annotation, verbatim from the Rust readout — the same string
    // `Overlay::effective_system` would have prepended if it owned the transport.
    if (gate?.note) systemParts.push(gate.note)
    if (gate?.verdict === 'caution') systemParts.push(cautionInstruction(gate))
  }
  // Availability is per-subsystem, because the two are genuinely independent. The omni
  // loop reasons about a source tree and does not care whether the sniper is running — and
  // a Ψ HOLD is a statement about stale *bot* state, which says nothing about a repository.
  // Gating everything on the bot would switch off reasoning about the codebase at exactly
  // the moment an operator is most likely to be asking what to do about it.
  const availability: ToolAvailability = {
    bot: useTools,
    omni: omniConfig() !== null,
    seal: sealingAllowed(),
  }
  const anyTools = availableTools(availability)

  if (anyTools.length > 0) {
    const botNames = anyTools.filter((t) => t.group === 'bot').map((t) => t.name)
    const omniNames = anyTools.filter((t) => t.group === 'omni').map((t) => t.name)

    if (botNames.length) {
      systemParts.push(
        'You can call read-only tools for anything the state block does not cover: ' +
          `${botNames.join(', ')}. Call one when the question needs per-pool, per-trade ` +
          'or per-transaction detail. Do not call one for general questions about ' +
          'trading or code. You cannot change the bot — no tool here writes to it.',
      )
    }

    if (omniNames.length) {
      systemParts.push(OMNI_INSTRUCTION(omniNames, availability.seal))
    }
  }

  const messages: Record<string, unknown>[] = [
    { role: 'system', content: systemParts.join('\n\n') },
    ...history,
  ]

  const controller = new AbortController()
  const timeout = setTimeout(() => controller.abort(), UPSTREAM_TIMEOUT_MS)

  const call = (msgs: Record<string, unknown>[]) =>
    fetch(`${provider.baseUrl}/chat/completions`, {
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
        messages: msgs,
        ...(anyTools.length
          ? { tools: toolDefinitions(availability), tool_choice: 'auto' }
          : {}),
      }),
      signal: controller.signal,
    })

  // The first call happens before the Response is constructed, so a dead provider still
  // produces a real HTTP status instead of a 200 whose body says "sorry".
  let upstream: Response
  try {
    upstream = await call(messages)
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

  const encoder = new TextEncoder()
  const origin = request.nextUrl.origin

  /**
   * Tool calls already served this turn, keyed by name + arguments.
   *
   * Observed against llama-3.3-70b: given a result it finds thin, it will call the same
   * tool with the same arguments again rather than answer. Re-fetching feeds the loop —
   * the second result is identical, so the third round looks just as unsatisfying — and
   * each round is a whole request against a 30/min free tier, which is how a single
   * question ends in a 429 with nothing rendered. Answering the repeat with a pointer
   * back to the data costs nothing and breaks the cycle.
   */
  const served = new Set<string>()

  const stream = new ReadableStream<Uint8Array>({
    async start(ctl) {
      const send = (obj: unknown) =>
        ctl.enqueue(encoder.encode(`data: ${JSON.stringify(obj)}\n\n`))
      const text = (s: string) => send({ choices: [{ delta: { content: s } }] })

      let current: Response = upstream
      let spoken = ''

      // Announced up front so a CAUTION is visible on the turn it applies to, not
      // inferred afterwards from a hedged tone.
      if (gate) send({ scylar: { gate: gate.verdict, psi: gate.psi, bottleneck: gate.bottleneck } })

      try {
        for (let round = 0; ; round++) {
          const { content, toolCalls } = await pump(current, text)
          spoken += content

          if (toolCalls.length === 0) break

          if (round >= MAX_TOOL_ROUNDS) {
            // Tell her the budget is gone rather than silently dropping the calls; the
            // next answer is then written in full knowledge of what it is missing.
            messages.push({
              role: 'system',
              content:
                'Tool budget exhausted. Answer now with what you already have, and say ' +
                'which part you could not check.',
            })
            const final = await call(messages)
            if (!final.ok || !final.body) {
              send({ scylar: { error: `Provider returned ${final.status} on the final turn.` } })
              break
            }
            spoken += (await pump(final, text)).content
            break
          }

          messages.push({
            role: 'assistant',
            content: content || null,
            tool_calls: toolCalls.map((t) => ({
              id: t.id,
              type: 'function',
              function: { name: t.name, arguments: t.args },
            })),
          })

          const outcomes: ToolOutcome[] = await Promise.all(
            toolCalls.map((t): Promise<ToolOutcome> => {
              const sig = `${t.name}:${t.args}`
              if (served.has(sig)) {
                return Promise.resolve({
                  name: t.name,
                  ok: true,
                  content:
                    'You already called this with these arguments in this turn. The ' +
                    'result is above and has not changed. Answer with it now.',
                })
              }
              served.add(sig)
              return runTool(origin, t.name, t.args, availability)
            }),
          )
          for (let i = 0; i < outcomes.length; i++) {
            messages.push({
              role: 'tool',
              tool_call_id: toolCalls[i].id,
              name: outcomes[i].name,
              content: outcomes[i].content,
            })
          }

          // Announced to the client so the turn can show what she actually looked at.
          // An answer sourced from `get_pool_decisions` and one sourced from nothing
          // read identically, and only one of them is checkable.
          send({ scylar: { tools: outcomes.map((o) => ({ name: o.name, ok: o.ok })) } })

          // A seal the model asked for. Nothing has been written — `runTool` intercepts
          // that tool and records the request instead. This puts the proposal in front of
          // the operator, whose confirmation is the only thing that reaches the write.
          for (const o of outcomes) {
            if (o.proposal) send({ scylar: { sealProposal: o.proposal } })
          }

          const next = await call(messages)
          if (!next.ok || !next.body) {
            send({ scylar: { error: `Provider returned ${next.status} after a tool call.` } })
            break
          }
          current = next
        }
      } catch {
        // Client navigated away or upstream cut out mid-stream. The partial response is
        // already rendered; nothing useful to add to it.
      } finally {
        clearTimeout(timeout)
        // Close the stream first, then advance the loop. `record` is the half that makes
        // Ψ move at all, but it is a measurement of a turn that is already over — making
        // the operator wait on it would trade a live feature for a bookkeeping one.
        ctl.enqueue(encoder.encode('data: [DONE]\n\n'))
        ctl.close()
        if (gate && spoken.trim()) void recordAnswer(origin, spoken)
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
      // What she was actually given. The UI badges this per turn rather than trusting
      // the toggle: asking for context and silently getting none is the failure worth
      // showing, because her answer will look identical either way. `held` is its own
      // value rather than reusing the briefing's source — the briefing may have read
      // perfectly well and been withheld, and badging that turn LIVE would be a lie of
      // exactly the kind this header exists to prevent.
      'X-Scylar-Context': !wantContext ? 'off' : held ? 'held' : briefing.source,
      'X-Scylar-Tools': anyTools.length ? 'on' : 'off',
      // Surfaced per-turn for the same reason the context badge is: an operator has to
      // be able to tell whether an answer had the loop behind it or was reasoning from
      // the persona alone, and a header is checkable where a paragraph is not.
      'X-Scylar-Omni': availability.omni ? (availability.seal ? 'seal' : 'read') : 'off',
      'X-Scylar-Gate': gate?.verdict ?? 'off',
    },
  })
}

/**
 * Read one upstream SSE response, forwarding content as it arrives and accumulating any
 * tool calls.
 *
 * Content is emitted immediately rather than buffered to the end of the round — that is
 * the whole reason the loop streams every round instead of running the tool phase
 * non-streamed and synthesising the answer afterwards. Buffering would be simpler, and
 * would also mean the avatar's mouth opens once, at the end.
 *
 * Tool-call arguments arrive in fragments across many frames and are keyed by `index`,
 * not by `id` — the id appears once, on the first fragment. Accumulating by index is the
 * part that is easy to get wrong and produces truncated JSON when it is.
 */
async function pump(
  res: Response,
  emit: (s: string) => void,
): Promise<{ content: string; toolCalls: ToolCallAccum[] }> {
  const reader = res.body!.getReader()
  const decoder = new TextDecoder()

  let buffer = ''
  let content = ''
  const byIndex = new Map<number, ToolCallAccum>()

  try {
    for (;;) {
      const { done, value } = await reader.read()
      if (done) break
      buffer += decoder.decode(value, { stream: true })

      const lines = buffer.split('\n')
      buffer = lines.pop() ?? ''

      for (const line of lines) {
        const trimmed = line.trim()
        if (!trimmed.startsWith('data:')) continue
        const payload = trimmed.slice(5).trim()
        if (!payload || payload === '[DONE]') continue

        let delta: Record<string, unknown> | undefined
        try {
          delta = JSON.parse(payload)?.choices?.[0]?.delta
        } catch {
          // A malformed frame is not worth aborting a good response over; the next
          // frame almost always parses.
          continue
        }
        if (!delta) continue

        const chunk = delta.content
        if (typeof chunk === 'string' && chunk) {
          content += chunk
          emit(chunk)
        }

        const calls = delta.tool_calls
        if (Array.isArray(calls)) {
          for (const c of calls) {
            const idx = typeof c?.index === 'number' ? c.index : 0
            const acc = byIndex.get(idx) ?? { id: '', name: '', args: '' }
            if (c?.id) acc.id = String(c.id)
            if (c?.function?.name) acc.name += String(c.function.name)
            if (c?.function?.arguments) acc.args += String(c.function.arguments)
            byIndex.set(idx, acc)
          }
        }
      }
    }
  } finally {
    reader.releaseLock()
  }

  // A call with no id is unusable — the tool result must reference one — so give it a
  // deterministic stand-in rather than dropping the call and leaving the model waiting.
  const toolCalls = [...byIndex.entries()]
    .sort((a, b) => a[0] - b[0])
    .map(([idx, t]) => ({ ...t, id: t.id || `call_${idx}` }))
    .filter((t) => t.name)

  return { content, toolCalls }
}

/** Which providers are configured — lets the UI show a status without a chat turn. */
export async function GET() {
  const provider = resolveProvider()
  return NextResponse.json({
    ok: Boolean(provider),
    active: provider?.id ?? null,
    model: provider?.model ?? null,
    freeTierNote: provider?.freeTierNote ?? null,
    configured: configuredProviders(),
    checked: providerEnvVars(),
    // Reported by group, and with the omni state spelled out. "Which tools exist" was a
    // useful diagnostic when they all reached the same subsystem; now that they do not, a
    // flat list cannot distinguish "the daemon is not configured" from "the daemon is fine
    // and sealing is off", which are different things to go and fix.
    tools: {
      bot: TOOLS.filter((t) => t.group === 'bot').map((t) => t.name),
      omni: TOOLS.filter((t) => t.group === 'omni').map((t) => t.name),
      seal: SEAL_TOOL.name,
    },
    omni: {
      configured: omniConfig() !== null,
      sealing: sealingAllowed(),
      envVars: ['SCEMA_OMNID_URL', 'SCEMA_OMNID_TOKEN', 'SCYLAR_ALLOW_DECIDE'],
    },
  })
}
