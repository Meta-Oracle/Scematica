import { NextResponse, type NextRequest } from 'next/server'

import { buildBriefing, contextSystemMessage } from '@/lib/scylar/context'
import { cautionInstruction, holdInstruction, readGate, recordAnswer } from '@/lib/scylar/gate'
import {
  configuredProviders,
  providerEnvVars,
  resolveProvider,
} from '@/lib/scylar/provider'
import {
  DEFAULT_BUDGET,
  composePsyche,
  psycheHeader,
  type PsycheContext,
  type Senses,
} from '@/lib/scylar/psyche'
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

/** Upstream timeout. Free tiers occasionally hang rather than refusing outright. */
const UPSTREAM_TIMEOUT_MS = 45_000

/**
 * How many times she may call tools before having to answer.
 *
 * Three is enough for "look at decisions → notice a mint → pull its trades → answer" and
 * short enough that a model stuck in a loop costs three requests, not a rate limit. The
 * cap is announced to her when hit, so the last answer is written knowingly rather than
 * cut off.
 *
 * What a round actually costs is the whole conversation again — system prompt, history,
 * every tool result so far — so the price of this number is quadratic in tokens, not
 * linear in requests. That is now the binding constraint rather than a footnote: Groq's
 * free tier for gpt-oss-120b allows ~8k tokens/min, so a full three-round turn on a loaded
 * prompt can reach the per-minute ceiling on its own. It is left at three because the
 * alternative is amputating the "notice a mint, then go and read it" path that the tools
 * exist for; a 429 is at least legible, and `freeTierNote` is on the error.
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
  // The window is the provider's, not a constant here: history is re-sent on every tool
  // round, so on a token-metered free tier it is the one part of the request size this
  // route gets to choose. See `Provider.maxHistory`.
  const history = (raw as ChatMessage[])
    .filter((m) => m.role !== 'system')
    .slice(-provider.maxHistory)

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

  // Availability is per-subsystem, because the three are genuinely independent. The omni
  // loop reasons about a source tree and does not care whether the sniper is running; a Ψ
  // HOLD is a statement about stale *bot* state, which says nothing about a repository; and
  // the codex is static data that is correct whether anything is running at all. Gating
  // everything on the bot would switch off reasoning about the codebase at exactly the
  // moment an operator is most likely to be asking what to do about it.
  const availability: ToolAvailability = {
    bot: useTools,
    omni: omniConfig() !== null,
    seal: sealingAllowed(),
    codex: true,
  }
  const anyTools = availableTools(availability)
  const botNames = anyTools.filter((t) => t.group === 'bot').map((t) => t.name)
  const omniNames = anyTools.filter((t) => t.group === 'omni').map((t) => t.name)

  // What she can actually sense this turn, as five distinct bot states rather than a
  // boolean. "I can't see the bot" and "you turned the toggle off" are different sentences
  // and only one of them is a fault report.
  const senses: Senses = {
    bot: !wantContext
      ? 'off'
      : held
        ? 'held'
        : briefing.source === 'unavailable'
          ? 'unavailable'
          : briefing.source,
    omni: availability.omni,
    sealing: availability.seal,
    codex: availability.codex,
    tools: anyTools.map((t) => t.name),
  }

  const psycheCtx: PsycheContext = {
    // Prior turns in this session. `history` ends with the message being answered, so a
    // first message gives 0 — which is what the `first-contact` layer keys off.
    turn: Math.max(0, history.length - 1),
    utterance: history[history.length - 1]?.content ?? '',
    senses,
    gate: gate ? { verdict: gate.verdict, psi: gate.psi, bottleneck: gate.bottleneck } : null,
    provider: { id: provider.id, model: provider.model },
    blocks: {
      botState: contextBlock,
      // The overlay's own annotation, verbatim from the Rust readout — the same string
      // `Overlay::effective_system` would have prepended if it owned the transport.
      gateNote: held ? null : (gate?.note ?? null),
      gateInstruction:
        held && gate
          ? holdInstruction(gate)
          : gate?.verdict === 'caution'
            ? cautionInstruction(gate)
            : null,
      toolInstruction: botNames.length
        ? 'You can call read-only tools for anything the state block does not cover: ' +
          `${botNames.join(', ')}. Call one when the question needs per-pool, per-trade ` +
          'or per-transaction detail. Do not call one for general questions about ' +
          'trading or code. You cannot change the bot — no tool here writes to it.'
        : null,
      omniInstruction: omniNames.length ? OMNI_INSTRUCTION(omniNames, availability.seal) : null,
    },
    budget: DEFAULT_BUDGET,
  }

  // One system message, not five.
  //
  // Llama 3.x's chat template is trained on a single system turn; consecutive ones are
  // out of distribution, and everything here lands in front of the history window.
  // Measured against llama-3.3-70b (the provider default before gpt-oss-120b) with a real
  // state block and eight turns of history:
  // four separate system messages recovered the session PnL 5/5 and the agent's
  // train-step count 4/5, concatenated into one both were 5/5. A small margin, but free,
  // and the failure it removes is the model silently ignoring the block rather than
  // saying it cannot see one.
  //
  // `composePsyche` is what does the concatenating now, in a fixed order and under a
  // budget, and it reports back which layers made it in. See lib/scylar/psyche.ts.
  const psyche = composePsyche(psycheCtx)

  const messages: Record<string, unknown>[] = [
    { role: 'system', content: psyche.text },
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
        messages: msgs,
        ...(anyTools.length
          ? { tools: toolDefinitions(availability), tool_choice: 'auto' }
          : {}),
        // Sampling and reasoning options are the provider's, not this route's — they are
        // not portable across the four candidates and the active one is a reasoning model
        // where the others are not. Spread last so a provider can override anything above
        // it. See `Provider.params` in lib/scylar/provider.ts.
        ...provider.params,
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
    const retryAfter = upstream.headers.get('retry-after')
    return NextResponse.json(
      {
        ok: false,
        error: `${provider.label} returned ${upstream.status}.`,
        detail: detail.slice(0, 600),
        provider: provider.id,
        model: provider.model,
        hint:
          upstream.status === 429
            ? `Free-tier limit reached (${provider.freeTierNote}).` +
              (retryAfter ? ` Retry in ${retryAfter}s.` : '') +
              ' The upstream message above names which limit — a per-minute *token* ' +
              'ceiling is hit by one large request and will repeat on every turn, where a ' +
              'per-day one clears on its own. Wait, or configure another provider.'
            : undefined,
      },
      {
        status: upstream.status === 429 ? 429 : 502,
        // Passed through rather than invented. A number the provider gave is worth
        // showing; one this route made up would be a guess about somebody else's meter.
        ...(retryAfter ? { headers: { 'Retry-After': retryAfter } } : {}),
      },
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
   *
   * Kept for gpt-oss-120b without re-measuring it. Whether that model has the same habit
   * is unknown here, and this is the cheap side of the bet: if it never repeats a call the
   * set is dead weight, and if it does, the failure is a rate-limited turn that renders
   * nothing. Do not delete it on the grounds that the new model "probably" behaves — that
   * is a guess about a model, which is the one kind of claim this whole file is built to
   * refuse.
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

          // Coverage from an omni result, forwarded to the ring.
          //
          // Lifted out of the tool payload rather than recomputed: it is the loop's own
          // count of how many terms it managed to measure, and the ring is forbidden from
          // showing a score without it. Anything unparseable is skipped in silence — a
          // missing coverage renders as `∅`, which is the true statement, where a guessed
          // one would be the exact failure the meter exists to prevent.
          for (const o of outcomes) {
            if (!o.ok || !o.name.startsWith('omni_')) continue
            try {
              const cov = (JSON.parse(o.content) as { coverage?: unknown }).coverage as
                | { measured?: unknown; total?: unknown }
                | undefined
              if (typeof cov?.measured === 'number' && typeof cov?.total === 'number') {
                send({ scylar: { coverage: { measured: cov.measured, total: cov.total } } })
              }
            } catch {
              /* a truncated or non-JSON result carries no coverage; the meter stays empty */
            }
          }

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
      // Which injection layers were actually composed into the system prompt, in order.
      // The point of a layered prompt is that a turn which went wrong can be traced back
      // to what she was given — a claim made in a comment is not checkable, and this is.
      // See lib/scylar/psyche.ts.
      'X-Scylar-Psyche': psycheHeader(psyche),
      // Present only when the budget dropped something, which should be never on a normal
      // turn. If this starts appearing, a layer has grown — that is the signal to look.
      ...(psyche.dropped.length ? { 'X-Scylar-Psyche-Dropped': psyche.dropped.join(',') } : {}),
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
 *
 * **Only `delta.content` is forwarded, and that is load-bearing now that the default
 * provider is a reasoning model.** gpt-oss puts its chain of thought in a sibling
 * `delta.reasoning` field, which this ignores by construction — the provider also asks for
 * it not to be generated (`include_reasoning: false`), so the guarantee is made twice.
 * Forwarding it would put her working-out on screen and drive the mouth sprite with it,
 * and worse, it would reach `recordAnswer` and be scored as something she claimed. If
 * reasoning ever needs showing, it belongs in its own `{"scylar":{...}}` frame with its own
 * label, never merged into the spoken text.
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
      codex: TOOLS.filter((t) => t.group === 'codex').map((t) => t.name),
      seal: SEAL_TOOL.name,
    },
    omni: {
      configured: omniConfig() !== null,
      sealing: sealingAllowed(),
      envVars: ['SCEMA_OMNID_URL', 'SCEMA_OMNID_TOKEN', 'SCYLAR_ALLOW_DECIDE'],
    },
  })
}
