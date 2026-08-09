'use client'

import { useCallback, useEffect, useRef, useState } from 'react'
import Link from 'next/link'

import { ScylarAvatar } from './ScylarAvatar'
import { type AvatarPhase, readsPositive } from '@/lib/scylar/expressions'

// The Scylar terminal: portrait on the left, conversation on the right.
//
// The avatar is a pure function of conversation state — this component owns the phase
// and never reaches into the avatar to set an expression. That keeps "what she looks
// like" derivable from "what the terminal is doing", so the two can't drift.

interface Turn {
  role: 'user' | 'assistant'
  content: string
  /** Set once the turn completes, so the reaction fires once and not on every re-render. */
  done?: boolean
}

interface ProviderStatus {
  ok: boolean
  active: string | null
  model: string | null
  freeTierNote: string | null
  configured: string[]
  checked: string[]
}

export function ScylarTerminal() {
  const [turns, setTurns] = useState<Turn[]>([])
  const [draft, setDraft] = useState('')
  const [phase, setPhase] = useState<AvatarPhase>({ kind: 'idle' })
  const [error, setError] = useState<string | null>(null)
  const [status, setStatus] = useState<ProviderStatus | null>(null)
  const busy = phase.kind === 'thinking' || phase.kind === 'streaming'

  const logRef = useRef<HTMLDivElement>(null)
  const abortRef = useRef<AbortController | null>(null)

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

  const send = useCallback(async () => {
    const text = draft.trim()
    if (!text || busy) return

    setDraft('')
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
          messages: history.map(({ role, content }) => ({ role, content })),
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

      setPhase({ kind: 'streaming', elapsedMs: 0 })
      setTurns((t) => [...t, { role: 'assistant', content: '' }])

      const reader = res.body.getReader()
      const decoder = new TextDecoder()
      let buffer = ''
      let assembled = ''

      for (;;) {
        const { done, value } = await reader.read()
        if (done) break
        buffer += decoder.decode(value, { stream: true })

        // SSE frames are newline-delimited; a chunk can split one mid-frame, so keep the
        // trailing partial in the buffer rather than parsing it.
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
                next[next.length - 1] = { role: 'assistant', content: assembled }
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
        next[next.length - 1] = { role: 'assistant', content: assembled, done: true }
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
  }, [draft, busy, turns])

  const stop = useCallback(() => {
    abortRef.current?.abort()
    abortRef.current = null
    setPhase({ kind: 'idle' })
  }, [])

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
            <div className="scylar-panel-header">CONVERSATION</div>

            <div ref={logRef} className="flex-1 space-y-4 overflow-y-auto p-4">
              {turns.length === 0 && !error && (
                <p className="text-sm leading-relaxed text-scylar-muted">
                  {status && !status.ok ? (
                    <>
                      No LLM provider is configured. Set one of{' '}
                      <code className="text-scylar-violet">{status.checked.join(', ')}</code> in the
                      server environment — Groq is recommended for latency.
                    </>
                  ) : (
                    'Ask Scylar something.'
                  )}
                </p>
              )}

              {turns.map((turn, i) => (
                <div key={i} className="space-y-1">
                  <div
                    className={`text-[0.6rem] tracking-widest ${
                      turn.role === 'user' ? 'text-scylar-dim' : 'text-scylar-violet'
                    }`}
                  >
                    {turn.role === 'user' ? 'OPERATOR' : 'SCYLAR'}
                  </div>
                  <div className="whitespace-pre-wrap break-words text-sm leading-relaxed text-scylar-text">
                    {turn.content}
                    {turn.role === 'assistant' && !turn.done && (
                      <span className="scylar-caret ml-0.5 inline-block h-4 w-2 align-text-bottom" />
                    )}
                  </div>
                </div>
              ))}

              {error && (
                <div className="border border-scylar-red/40 bg-scylar-red/5 p-3 text-xs text-scylar-red">
                  {error}
                </div>
              )}
            </div>

            <div className="flex gap-2 border-t border-scylar-border p-3">
              <textarea
                value={draft}
                onChange={(e) => setDraft(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === 'Enter' && !e.shiftKey) {
                    e.preventDefault()
                    void send()
                  }
                }}
                rows={1}
                placeholder={busy ? 'Scylar is responding…' : 'Message Scylar…  (Enter to send)'}
                disabled={busy}
                className="min-h-[2.5rem] flex-1 resize-none bg-scylar-hi px-3 py-2 text-sm
                           text-scylar-text outline-none ring-1 ring-scylar-border
                           placeholder:text-scylar-dim focus:ring-scylar-violet-dim
                           disabled:opacity-50"
              />
              <button
                onClick={busy ? stop : () => void send()}
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
