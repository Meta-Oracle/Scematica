'use client'

import { useCallback, useEffect, useRef, useState } from 'react'
import Link from 'next/link'

import { ScylarAvatar } from './ScylarAvatar'
import { ScylarMessage } from './ScylarMessage'
import { helpText, parseCommand } from '@/lib/scylar/commands'
import { type AvatarPhase, readsPositive } from '@/lib/scylar/expressions'
import {
  type Turn,
  clearTranscript,
  loadContextPref,
  loadTranscript,
  saveContextPref,
  saveTranscript,
} from '@/lib/scylar/session'

// The Scylar terminal: portrait on the left, conversation on the right.
//
// The avatar is a pure function of conversation state — this component owns the phase
// and never reaches into the avatar to set an expression. That keeps "what she looks
// like" derivable from "what the terminal is doing", so the two can't drift.

interface ProviderStatus {
  ok: boolean
  active: string | null
  model: string | null
  freeTierNote: string | null
  configured: string[]
  checked: string[]
}

/** History sent upstream. Matches MAX_HISTORY in the route; the route still enforces it. */
const SEND_HISTORY = 20

export function ScylarTerminal() {
  const [turns, setTurns] = useState<Turn[]>([])
  const [draft, setDraft] = useState('')
  const [phase, setPhase] = useState<AvatarPhase>({ kind: 'idle' })
  const [error, setError] = useState<string | null>(null)
  const [status, setStatus] = useState<ProviderStatus | null>(null)
  const [useContext, setUseContext] = useState(true)
  const [hydrated, setHydrated] = useState(false)
  const busy = phase.kind === 'thinking' || phase.kind === 'streaming'

  const logRef = useRef<HTMLDivElement>(null)
  const abortRef = useRef<AbortController | null>(null)
  const inputRef = useRef<HTMLTextAreaElement>(null)

  // Restore after mount, never during render: the server has no localStorage, and
  // reading it in the initial state would make the first client render disagree with the
  // markup Next.js sent.
  useEffect(() => {
    setTurns(loadTranscript())
    setUseContext(loadContextPref(true))
    setHydrated(true)
  }, [])

  useEffect(() => {
    if (hydrated) saveTranscript(turns)
  }, [turns, hydrated])

  useEffect(() => {
    if (hydrated) saveContextPref(useContext)
  }, [useContext, hydrated])

  // Probe the provider once so a missing key surfaces immediately, rather than after
  // someone types their first message and waits on a 503.
  useEffect(() => {
    let alive = true
    fetch('/api/scylar/chat')
      .then((r) => r.json())
      .then((s: ProviderStatus) => alive && setStatus(s))
      .catch(() => alive && setStatus(null))
    return () => {
      alive = false
    }
  }, [])

  useEffect(() => {
    logRef.current?.scrollTo({ top: logRef.current.scrollHeight, behavior: 'smooth' })
  }, [turns])

  useEffect(() => () => abortRef.current?.abort(), [])

  /** Append a message from the terminal itself. Not sent upstream, not persisted as history. */
  const say = useCallback((content: string) => {
    setTurns((t) => [...t, { role: 'assistant', content, done: true, context: 'local' }])
  }, [])

  const ask = useCallback(
    async (text: string, opts: { context: boolean }) => {
      setError(null)
      const history: Turn[] = [...turns, { role: 'user', content: text, done: true }]
      setTurns(history)
      setPhase({ kind: 'thinking' })

      const ctl = new AbortController()
      abortRef.current = ctl

      try {
        const res = await fetch('/api/scylar/chat', {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({
            // Local notices are terminal output, not conversation — sending them back
            // would have the model treat its own help text as something it said.
            messages: history
              .filter((t) => t.context !== 'local')
              .slice(-SEND_HISTORY)
              .map(({ role, content }) => ({ role, content })),
            context: opts.context,
          }),
          signal: ctl.signal,
        })

        if (!res.ok || !res.body) {
          const detail = await res.json().catch(() => null)
          // Show the upstream reason verbatim. On a free tier this is nearly always a
          // rate-limit message, and paraphrasing it costs the operator the one fact that
          // tells them whether to wait a minute or add a key.
          setError(
            [detail?.error, detail?.hint || detail?.detail].filter(Boolean).join(' — ') ||
              `Request failed (${res.status}).`,
          )
          setPhase({ kind: 'idle' })
          return
        }

        // What she was actually given, which is not necessarily what was asked for.
        const contextUsed = res.headers.get('X-Scylar-Context') || 'off'

        setPhase({ kind: 'streaming', elapsedMs: 0 })
        setTurns((t) => [...t, { role: 'assistant', content: '', context: contextUsed }])

        const reader = res.body.getReader()
        const decoder = new TextDecoder()
        let buffer = ''
        let assembled = ''

        for (;;) {
          const { done, value } = await reader.read()
          if (done) break
          buffer += decoder.decode(value, { stream: true })

          // SSE frames are newline-delimited; a chunk can split one mid-frame, so keep
          // the trailing partial in the buffer rather than parsing it.
          const lines = buffer.split('\n')
          buffer = lines.pop() ?? ''

          for (const line of lines) {
            const trimmed = line.trim()
            if (!trimmed.startsWith('data:')) continue
            const payload = trimmed.slice(5).trim()
            if (!payload || payload === '[DONE]') continue

            try {
              const delta = JSON.parse(payload)?.choices?.[0]?.delta?.content
              if (typeof delta === 'string' && delta) {
                assembled += delta
                setTurns((t) => {
                  const next = [...t]
                  next[next.length - 1] = {
                    role: 'assistant',
                    content: assembled,
                    context: contextUsed,
                  }
                  return next
                })
              }
            } catch {
              // A malformed frame is not worth aborting a good response over; the next
              // frame almost always parses.
            }
          }
        }

        setTurns((t) => {
          const next = [...t]
          next[next.length - 1] = {
            role: 'assistant',
            content: assembled,
            done: true,
            context: contextUsed,
          }
          return next
        })

        if (assembled.trim()) {
          setPhase({ kind: 'settled', positive: readsPositive(assembled), sinceMs: 0 })
        } else {
          // Upstream closed without emitting anything — a silent empty turn looks like a
          // UI bug, so say what happened.
          setError('The model returned an empty response.')
          setPhase({ kind: 'idle' })
        }
      } catch (err) {
        if (err instanceof Error && err.name === 'AbortError') return
        setError(err instanceof Error ? err.message : String(err))
        setPhase({ kind: 'idle' })
      } finally {
        abortRef.current = null
      }
    },
    [turns],
  )

  const submit = useCallback(
    (raw: string) => {
      const text = raw.trim()
      if (!text || busy) return

      const cmd = parseCommand(text)
      switch (cmd.kind) {
        case 'help':
          setDraft('')
          setTurns((t) => [
            ...t,
            { role: 'user', content: text, done: true },
            { role: 'assistant', content: helpText(), done: true, context: 'local' },
          ])
          return

        case 'clear':
          setDraft('')
          setError(null)
          clearTranscript()
          setTurns([])
          setPhase({ kind: 'idle' })
          return

        case 'context': {
          setDraft('')
          const next = cmd.enabled === 'toggle' ? !useContext : cmd.enabled
          setUseContext(next)
          say(next ? 'Live context **on**.' : 'Live context **off**. I am flying blind.')
          return
        }

        case 'retry': {
          const last = [...turns].reverse().find((t) => t.role === 'user')
          setDraft('')
          if (!last) {
            say('Nothing to retry.')
            return
          }
          // Drop everything after the message being retried so the transcript reads as
          // one attempt rather than an accumulating pile of near-identical answers.
          const idx = turns.lastIndexOf(last)
          setTurns(turns.slice(0, idx))
          setTimeout(() => void ask(last.content, { context: useContext }), 0)
          return
        }

        case 'ask':
          setDraft('')
          // Forced on: these questions are meaningless without the state block, and
          // answering them from a general model would produce a confident fabrication.
          void ask(cmd.prompt, { context: true })
          return

        default:
          setDraft('')
          void ask(text, { context: useContext })
      }
    },
    [busy, turns, useContext, ask, say],
  )

  const stop = useCallback(() => {
    abortRef.current?.abort()
    abortRef.current = null
    setPhase({ kind: 'idle' })
  }, [])

  // Escape stops a stream from anywhere on the page — the STOP button is across the
  // panel from where your hands already are.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape' && busy) stop()
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [busy, stop])

  const contextBadge = badgeFor(turns)

  return (
    <div className="scylar-root flex min-h-screen flex-col">
      <header className="border-b border-scylar-border px-4 py-3">
        <div className="mx-auto flex max-w-[1400px] items-center gap-3">
          <div className="flex flex-col leading-tight">
            <span className="glow-violet text-sm font-bold tracking-[0.3em]">SCYLAR</span>
            <span className="text-xs tracking-widest text-scylar-dim">
              RESIDENT INTELLIGENCE
            </span>
          </div>

          <div className="ml-auto flex items-center gap-3 text-xs">
            {status && (
              <span
                className="hidden items-center gap-1.5 tracking-widest text-scylar-muted sm:flex"
                title={status.freeTierNote ?? undefined}
              >
                <span
                  className={`h-1.5 w-1.5 rounded-full ${
                    status.ok ? 'animate-pulse bg-scylar-violet' : 'bg-scylar-red'
                  }`}
                />
                {status.ok ? status.active?.toUpperCase() : 'NO PROVIDER'}
              </span>
            )}
            <Link
              href="/"
              className="border border-scylar-border px-2 py-0.5 tracking-widest text-scylar-violet
                         transition-all hover:border-scylar-violet hover:text-scylar-violet-hi"
            >
              ◈ SCEMATICA
            </Link>
          </div>
        </div>
      </header>

      <main className="mx-auto flex w-full max-w-[1400px] flex-1 flex-col gap-4 px-3 py-4 lg:flex-row">
        <section className="flex flex-col items-center gap-3 lg:sticky lg:top-4 lg:self-start">
          <div className="scylar-panel overflow-hidden">
            <ScylarAvatar phase={phase} size={420} />
          </div>
          <p className="text-center text-[0.65rem] tracking-widest text-scylar-dim">
            {phase.kind === 'thinking' && 'THINKING'}
            {phase.kind === 'streaming' && 'SPEAKING'}
            {phase.kind === 'settled' && 'IDLE'}
            {phase.kind === 'idle' && (turns.length ? 'IDLE' : 'AWAITING INPUT')}
          </p>
        </section>

        <section className="flex min-w-0 flex-1 flex-col">
          <div className="scylar-panel flex min-h-[420px] flex-1 flex-col">
            <div className="scylar-panel-header">
              CONVERSATION
              <span className="ml-auto flex items-center gap-3 normal-case tracking-normal">
                {contextBadge && (
                  <span
                    className={`text-[0.6rem] tracking-widest ${
                      contextBadge.tone === 'warn' ? 'text-scylar-red' : 'text-scylar-dim'
                    }`}
                    title={contextBadge.title}
                  >
                    {contextBadge.label}
                  </span>
                )}
                <button
                  onClick={() => setUseContext((v) => !v)}
                  title="Attach a live read of the bot to each message"
                  className={`text-[0.6rem] tracking-widest transition-colors ${
                    useContext
                      ? 'text-scylar-violet hover:text-scylar-violet-hi'
                      : 'text-scylar-dim hover:text-scylar-muted'
                  }`}
                >
                  CONTEXT {useContext ? 'ON' : 'OFF'}
                </button>
                <button
                  onClick={() => submit('/clear')}
                  disabled={busy || turns.length === 0}
                  className="text-[0.6rem] tracking-widest text-scylar-dim transition-colors
                             hover:text-scylar-violet-hi disabled:opacity-30"
                >
                  NEW
                </button>
              </span>
            </div>

            <div
              ref={logRef}
              className="flex-1 space-y-4 overflow-y-auto p-4"
              // Streaming text is announced as it settles rather than per token; `polite`
              // so it never interrupts what a screen reader is already saying.
              aria-live="polite"
              aria-busy={busy}
            >
              {turns.length === 0 && !error && (
                <div className="space-y-2 text-sm leading-relaxed text-scylar-muted">
                  {status && !status.ok ? (
                    <p>
                      No LLM provider is configured. Set one of{' '}
                      <code className="scylar-inline-code">{status.checked.join(', ')}</code> in the
                      server environment — Groq is recommended for latency.
                    </p>
                  ) : (
                    <>
                      <p>Ask Scylar something, or try a command.</p>
                      <div className="flex flex-wrap gap-2 pt-1">
                        {['/status', '/positions', '/filters', '/nn', '/help'].map((c) => (
                          <button
                            key={c}
                            onClick={() => submit(c)}
                            className="border border-scylar-border px-2 py-0.5 text-xs tracking-widest
                                       text-scylar-violet transition-all hover:border-scylar-violet
                                       hover:text-scylar-violet-hi"
                          >
                            {c}
                          </button>
                        ))}
                      </div>
                    </>
                  )}
                </div>
              )}

              {turns.map((turn, i) => (
                <div key={i} className="space-y-1">
                  <div
                    className={`flex items-center gap-2 text-[0.6rem] tracking-widest ${
                      turn.role === 'user' ? 'text-scylar-dim' : 'text-scylar-violet'
                    }`}
                  >
                    {turn.role === 'user' ? 'OPERATOR' : 'SCYLAR'}
                    {turn.role === 'assistant' && turn.context === 'simulation' && (
                      <span className="text-scylar-red" title="Answered from simulated bot state">
                        SIMULATED STATE
                      </span>
                    )}
                    {turn.role === 'assistant' && turn.context === 'live' && (
                      <span className="text-scylar-green" title="Answered from a live bot read">
                        LIVE STATE
                      </span>
                    )}
                    {turn.role === 'assistant' && turn.context === 'unavailable' && (
                      <span className="text-scylar-red" title="Context was requested but no bot answered">
                        NO STATE
                      </span>
                    )}
                  </div>

                  {turn.role === 'assistant' ? (
                    <div className="min-w-0">
                      <ScylarMessage content={turn.content} streaming={!turn.done} />
                      {!turn.done && (
                        <span className="scylar-caret ml-0.5 inline-block h-4 w-2 align-text-bottom" />
                      )}
                    </div>
                  ) : (
                    <div className="whitespace-pre-wrap break-words text-sm leading-relaxed text-scylar-text">
                      {turn.content}
                    </div>
                  )}
                </div>
              ))}

              {error && (
                <div className="space-y-2 border border-scylar-red/40 bg-scylar-red/5 p-3 text-xs text-scylar-red">
                  <p>{error}</p>
                  <button
                    onClick={() => submit('/retry')}
                    disabled={busy}
                    className="border border-scylar-red/50 px-2 py-0.5 tracking-widest
                               transition-colors hover:text-scylar-red disabled:opacity-30"
                  >
                    RETRY
                  </button>
                </div>
              )}
            </div>

            <div className="flex gap-2 border-t border-scylar-border p-3">
              <textarea
                ref={inputRef}
                value={draft}
                onChange={(e) => setDraft(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === 'Enter' && !e.shiftKey) {
                    e.preventDefault()
                    submit(draft)
                  }
                }}
                rows={1}
                placeholder={
                  busy ? 'Scylar is responding…  (Esc to stop)' : 'Message Scylar…  (/help for commands)'
                }
                disabled={busy}
                className="min-h-[2.5rem] flex-1 resize-none bg-scylar-hi px-3 py-2 text-sm
                           text-scylar-text outline-none ring-1 ring-scylar-border
                           placeholder:text-scylar-dim focus:ring-scylar-violet-dim
                           disabled:opacity-50"
              />
              <button
                onClick={busy ? stop : () => submit(draft)}
                disabled={!busy && !draft.trim()}
                className="border border-scylar-border px-4 text-xs tracking-widest
                           text-scylar-violet transition-all hover:border-scylar-violet
                           hover:text-scylar-violet-hi disabled:opacity-30
                           disabled:hover:border-scylar-border"
              >
                {busy ? 'STOP' : 'SEND'}
              </button>
            </div>
          </div>
        </section>
      </main>
    </div>
  )
}

/**
 * Header note about the most recent answer's grounding.
 *
 * Only warns. A `live` badge already sits on the turn itself; repeating it in the header
 * would make the two states look equally noteworthy, when only one of them means "these
 * numbers are not real".
 */
function badgeFor(turns: Turn[]): { label: string; tone: 'warn' | 'muted'; title: string } | null {
  const last = [...turns].reverse().find((t) => t.role === 'assistant' && t.context)
  if (!last) return null

  if (last.context === 'simulation') {
    return {
      label: 'SIMULATED',
      tone: 'warn',
      title: 'No bot is paired — figures come from the built-in simulation engine.',
    }
  }
  if (last.context === 'unavailable') {
    return {
      label: 'NO BOT',
      tone: 'warn',
      title: 'Context was requested but nothing answered on /api.',
    }
  }
  return null
}
