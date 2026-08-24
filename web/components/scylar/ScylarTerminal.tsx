'use client'

import { useCallback, useEffect, useRef, useState } from 'react'
import Link from 'next/link'

import { usePortraits } from '@/lib/scylar/usePortraits'
import { ScylarAvatar } from './ScylarAvatar'
import { ScylarMessage } from './ScylarMessage'
import { useSpeech } from './useSpeech'
import { helpText, parseCommand } from '@/lib/scylar/commands'
import { type AvatarPhase, readsPositive } from '@/lib/scylar/expressions'
import {
  type ToolUse,
  type Turn,
  clearTranscript,
  loadContextPref,
  loadTranscript,
  loadVoicePref,
  saveContextPref,
  saveTranscript,
  saveVoicePref,
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
  // Generated portraits, if a ComfyUI backend is configured and reachable. Returns `{}`
  // otherwise and the avatar keeps its sprites — nothing here is on the chat path.
  const portraits = usePortraits()
  const [useVoice, setUseVoice] = useState(false)
  const [hydrated, setHydrated] = useState(false)
  // Index of the turn whose seal is in flight, so only one can run and the right button
  // shows the pending state. `null` rather than a boolean: two proposals can be on screen.
  const [sealing, setSealing] = useState<number | null>(null)
  const [sealError, setSealError] = useState<string | null>(null)
  const speech = useSpeech(useVoice)

  // Speaking counts as busy. Letting the next message go out while she is mid-sentence
  // leaves two answers overlapping in the same voice, which is unintelligible.
  const busy =
    phase.kind === 'thinking' || phase.kind === 'streaming' || phase.kind === 'voicing'

  const logRef = useRef<HTMLDivElement>(null)
  const abortRef = useRef<AbortController | null>(null)
  const inputRef = useRef<HTMLTextAreaElement>(null)

  // Restore after mount, never during render: the server has no localStorage, and
  // reading it in the initial state would make the first client render disagree with the
  // markup Next.js sent.
  useEffect(() => {
    setTurns(loadTranscript())
    setUseContext(loadContextPref(true))
    setUseVoice(loadVoicePref())
    setHydrated(true)
  }, [])

  useEffect(() => {
    if (hydrated) saveTranscript(turns)
  }, [turns, hydrated])

  useEffect(() => {
    if (hydrated) saveContextPref(useContext)
  }, [useContext, hydrated])

  useEffect(() => {
    if (hydrated) saveVoicePref(useVoice)
  }, [useVoice, hydrated])

  // Speech owns the phase while it is talking. Each word boundary pushes a new phase
  // object, which is what restarts the avatar's per-word mouth cycle; `word.seq` is in
  // the dependency list precisely so an identical duration twice running still counts
  // as a new word.
  useEffect(() => {
    if (speech.speaking && speech.word) {
      setPhase({ kind: 'voicing', sinceWordMs: 0, wordMs: speech.word.wordMs })
    }
  }, [speech.speaking, speech.word])

  // When she stops speaking, hand the phase back — reacting if the answer warranted it.
  const lastSpoken = useRef<string | null>(null)
  useEffect(() => {
    if (speech.speaking || phase.kind !== 'voicing') return
    const text = lastSpoken.current ?? ''
    setPhase({ kind: 'settled', positive: readsPositive(text), sinceMs: 0 })
  }, [speech.speaking, phase.kind])

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
        let tools: ToolUse[] = []
        let gate: string | undefined
        // A seal she asked for. Nothing is written by the time this arrives — the server
        // intercepts that tool and records the request — so this only ever produces a
        // control for the operator to act on.
        let sealProposal: { goal: string; ground: string[] } | undefined
        // Set by a mid-stream `scylar.error` frame. Tracked separately from the `error`
        // state because the generic "empty response" fallback below must not overwrite
        // it — a rate-limit message is the one fact worth showing, and losing it behind
        // "the model returned nothing" is how a diagnosable failure becomes a mystery.
        let streamError: string | null = null

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
              const frame = JSON.parse(payload)

              // Out-of-band frame: tool activity, or a mid-stream provider failure. It
              // carries no `choices`, so a parser that only reads deltas skips it.
              const notice = frame?.scylar
              if (notice) {
                if (Array.isArray(notice.tools)) tools = [...tools, ...notice.tools]
                if (typeof notice.gate === 'string') gate = notice.gate
                if (notice.sealProposal && typeof notice.sealProposal.goal === 'string') {
                  sealProposal = {
                    goal: notice.sealProposal.goal,
                    ground: Array.isArray(notice.sealProposal.ground)
                      ? notice.sealProposal.ground.filter((g: unknown) => typeof g === 'string')
                      : [],
                  }
                }
                if (typeof notice.error === 'string') {
                  streamError = notice.error
                  setError(notice.error)
                }
              }

              const delta = frame?.choices?.[0]?.delta?.content
              if (typeof delta === 'string' && delta) {
                assembled += delta
              }

              if (notice || (typeof delta === 'string' && delta)) {
                setTurns((t) => {
                  const next = [...t]
                  next[next.length - 1] = {
                    role: 'assistant',
                    content: assembled,
                    context: contextUsed,
                    tools,
                    gate,
                    sealProposal,
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
          // Nothing said and nothing read: drop the placeholder rather than leave an
          // empty SCYLAR bubble, which reads as a rendering bug. If tools did run, the
          // bubble stays — the badges are a record of what she got through before the
          // failure, and that is worth more than tidiness.
          if (!assembled.trim() && tools.length === 0) return next.slice(0, -1)
          next[next.length - 1] = {
            role: 'assistant',
            content: assembled,
            done: true,
            context: contextUsed,
            tools,
            gate,
            sealProposal,
          }
          return next
        })

        if (assembled.trim()) {
          if (useVoice && speech.supported) {
            lastSpoken.current = assembled
            speech.speak(assembled)
            // Provisional: the first word boundary replaces this. Set here rather than
            // waiting so the mouth does not sit shut through the engine's start-up
            // latency, which on a cold voice list is a noticeable half-second.
            setPhase({ kind: 'voicing', sinceWordMs: 0, wordMs: 280 })
          } else {
            setPhase({ kind: 'settled', positive: readsPositive(assembled), sinceMs: 0 })
          }
        } else {
          // Upstream closed without emitting anything — a silent empty turn looks like a
          // UI bug, so say what happened. Unless the stream already said something more
          // specific, in which case that stands.
          if (!streamError) setError('The model returned an empty response.')
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
    [turns, useVoice, speech],
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
    speech.cancel()
    setPhase({ kind: 'idle' })
  }, [speech])

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

  /**
   * Write the record she proposed.
   *
   * This is the only place in the client that causes a write, and it runs from a click.
   * The model can put a proposal on screen; it cannot press this. The route independently
   * requires `confirm: true` and re-checks that sealing is enabled, so a forged call from
   * elsewhere on the origin still meets a server-side gate — but the gate that matters to
   * the operator is this one, because it is the one they can see.
   */
  const confirmSeal = useCallback(
    async (index: number) => {
      const turn = turns[index]
      if (!turn?.sealProposal || turn.sealed) return
      setSealing(index)
      setSealError(null)
      try {
        const res = await fetch('/api/scylar/omni/seal', {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({
            goal: turn.sealProposal.goal,
            ground: turn.sealProposal.ground,
            confirm: true,
          }),
        })
        const data = await res.json().catch(() => null)
        if (!res.ok) {
          // The route distinguishes its refusals — sealing disabled, daemon unreachable,
          // the daemon itself refusing. Passing the detail through means the operator can
          // act on it instead of retrying a button that will never work.
          setSealError(data?.detail ?? `The seal was refused (${res.status}).`)
          return
        }
        setTurns((t) => {
          const next = [...t]
          const target = next[index]
          if (target) {
            next[index] = { ...target, sealed: { id: data.id, root: data.root } }
          }
          return next
        })
      } catch {
        setSealError('Could not reach the seal endpoint.')
      } finally {
        setSealing(null)
      }
    },
    [turns],
  )

  /** Drop a proposal without writing. Declining is a normal outcome, not an error. */
  const dismissSeal = useCallback((index: number) => {
    setSealError(null)
    setTurns((t) => {
      const next = [...t]
      const target = next[index]
      if (target) {
        const { sealProposal: _dropped, ...rest } = target
        next[index] = rest
      }
      return next
    })
  }, [])

  return (
    // A device, not a document: the viewport is the chassis and only the transcript
    // scrolls inside it. Previously the page itself scrolled, which made the portrait's
    // `lg:sticky` a no-op (there was nothing to stick against) and, below `lg`, scrolled
    // her off the top entirely — the projection wandering out of its own projector.
    // `100dvh` rather than `100vh` because mobile browsers shrink the visual viewport
    // when the URL bar retracts, and `vh` does not follow it.
    <div className="scylar-root flex h-[100dvh] flex-col overflow-hidden">
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

      {/* `min-h-0` on every link of the flex chain. Without it a flex child refuses to
          shrink below its content and the inner `overflow-y-auto` never engages — the
          transcript grows instead of scrolling and pushes the composer off the chassis. */}
      <main className="mx-auto flex w-full min-h-0 max-w-[1400px] flex-1 flex-col gap-4 px-3 py-4 lg:flex-row">
        <section className="scylar-stage flex w-full max-w-[420px] shrink-0 flex-col items-center gap-3 self-center lg:self-start">
          <div className="scylar-panel scylar-projector overflow-hidden">
            <ScylarAvatar phase={phase} size={420} portraits={portraits} />
          </div>
          <p className="scylar-readout text-center text-[0.65rem] tracking-widest text-scylar-dim">
            {phase.kind === 'thinking' && 'THINKING'}
            {phase.kind === 'streaming' && 'WRITING'}
            {phase.kind === 'voicing' && 'SPEAKING'}
            {phase.kind === 'settled' && 'IDLE'}
            {phase.kind === 'idle' && (turns.length ? 'IDLE' : 'AWAITING INPUT')}
          </p>
        </section>

        <section className="flex min-h-0 min-w-0 flex-1 flex-col">
          <div className="scylar-panel flex min-h-0 flex-1 flex-col">
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
                {/* Rendered only where it can work. A toggle that silently does nothing
                    is worse than an absent one — you cannot tell it from a broken voice. */}
                {speech.supported && (
                  <button
                    onClick={() => setUseVoice((v) => !v)}
                    // The voice name is in the tooltip because the installed list varies
                    // by OS, browser and connectivity — when the pick is wrong, this is
                    // the only way to know which voice to blame.
                    title={
                      speech.voiceName
                        ? `Speak answers aloud — voice: ${speech.voiceName}${
                            speech.voiceIsFemale ? '' : ' (could not confirm a female voice)'
                          }`
                        : 'Speak answers aloud, with the mouth driven by real speech timing'
                    }
                    className={`text-[0.6rem] tracking-widest transition-colors ${
                      useVoice
                        ? 'text-scylar-violet hover:text-scylar-violet-hi'
                        : 'text-scylar-dim hover:text-scylar-muted'
                    }`}
                  >
                    VOICE {useVoice ? 'ON' : 'OFF'}
                  </button>
                )}
                {/* Only when it failed. A correct pick needs no announcement. */}
                {speech.supported && useVoice && speech.voiceName && !speech.voiceIsFemale && (
                  <span
                    className="text-[0.6rem] tracking-widest text-scylar-red"
                    title={`Picked "${speech.voiceName}" — no female English voice was recognised on this machine.`}
                  >
                    VOICE?
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
              className="min-h-0 flex-1 space-y-4 overflow-y-auto p-4"
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
                    {/* Distinct from NO STATE: the bot answered, and the gate withheld
                        what it said. Collapsing the two would hide the difference
                        between "nothing is there" and "what is there is not true". */}
                    {turn.role === 'assistant' && turn.context === 'held' && (
                      <span
                        className="text-scylar-red"
                        title="Ψ HOLD — bot state is stale or self-contradictory, so it was withheld from her"
                      >
                        STATE HELD
                      </span>
                    )}
                    {/* Only CAUTION is shown. GO is the ordinary case, and badging it
                        would make the two look equally worth reading. */}
                    {turn.role === 'assistant' && turn.gate === 'caution' && (
                      <span
                        className="text-scylar-red"
                        title="Ψ below the GO threshold — bot state is partial or going stale"
                      >
                        Ψ CAUTION
                      </span>
                    )}
                  </div>

                  {/* What she actually read. An answer sourced from the decisions log
                      and one sourced from nothing look identical in prose; only one of
                      them can be checked against the bot. */}
                  {turn.role === 'assistant' && turn.tools && turn.tools.length > 0 && (
                    <div className="flex flex-wrap gap-1.5 pb-0.5">
                      {turn.tools.map((t, j) => (
                        <span
                          key={j}
                          title={t.ok ? 'Read successfully' : 'The call failed; she was told so'}
                          className={`border px-1.5 py-px text-[0.55rem] tracking-widest ${
                            t.ok
                              ? 'border-scylar-border text-scylar-muted'
                              : 'border-scylar-red/40 text-scylar-red'
                          }`}
                        >
                          {t.name.replace(/^get_/, '').replace(/_/g, ' ').toUpperCase()}
                        </span>
                      ))}
                    </div>
                  )}

                  {/* A decision record she has asked to write.
                      Nothing has been written at this point: the server intercepts the
                      sealing tool and records the request. This control is the only path
                      to the write, which is the whole reason it exists — a confirmation
                      the model can satisfy on its own behalf is not a confirmation. Same
                      split as the console, where `enter` simulates and `D` decides. */}
                  {turn.role === 'assistant' && turn.done && turn.sealProposal && (
                    <div className="space-y-2 border border-scylar-accent/40 bg-scylar-accent/5 p-3 text-xs">
                      {turn.sealed ? (
                        <>
                          <p className="text-scylar-muted">
                            Sealed as{' '}
                            <span className="text-scylar-accent">{turn.sealed.id}</span>.
                          </p>
                          <p className="text-scylar-muted">
                            Check it yourself:{' '}
                            <code className="text-scylar-text">
                              scema verify {turn.sealed.id}
                            </code>{' '}
                            — or drop the record into /omni, which verifies in your browser
                            and sends it nowhere.
                          </p>
                        </>
                      ) : (
                        <>
                          <p className="tracking-widest text-scylar-accent">
                            SEAL A DECISION RECORD?
                          </p>
                          <p className="text-scylar-muted">
                            She is proposing to record this permanently. Nothing has been
                            written yet.
                          </p>
                          <p className="break-words text-scylar-text">
                            {turn.sealProposal.goal}
                          </p>
                          {turn.sealProposal.ground.length > 0 && (
                            <p className="text-scylar-muted">
                              grounded in {turn.sealProposal.ground.join(', ')}
                            </p>
                          )}
                          <div className="flex gap-2 pt-1">
                            <button
                              onClick={() => confirmSeal(i)}
                              disabled={sealing !== null}
                              className="border border-scylar-accent/50 px-2 py-0.5 tracking-widest
                                         transition-colors hover:text-scylar-accent
                                         disabled:opacity-30"
                            >
                              {sealing === i ? 'SEALING…' : 'SEAL'}
                            </button>
                            <button
                              onClick={() => dismissSeal(i)}
                              disabled={sealing !== null}
                              className="border border-scylar-border px-2 py-0.5 tracking-widest
                                         text-scylar-muted transition-colors
                                         hover:text-scylar-text disabled:opacity-30"
                            >
                              DISMISS
                            </button>
                          </div>
                          {sealError && <p className="text-scylar-red">{sealError}</p>}
                        </>
                      )}
                    </div>
                  )}

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
  if (last.context === 'held') {
    return {
      label: 'STATE HELD',
      tone: 'warn',
      title:
        'The cognitive gate returned HOLD, so the state block was withheld from her — ' +
        'the bot state on disk is stale or self-contradictory. She answered without it.',
    }
  }
  return null
}
