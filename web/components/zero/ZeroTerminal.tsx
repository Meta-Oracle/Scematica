'use client'

// Scematica Zero — the console.
//
// The layout answers one question first, because it is the only question whose wrong
// answer costs money silently: **are my exits being evaluated?** Everything else is
// context for it. See `lib/zero/readout.ts`.
//
// This component places rectangles and names roles. It picks no colours — `globals.css`
// owns every hex — and it computes nothing: every figure comes from the pure core, so
// there is no second implementation of a rule here to drift from the first.

import { useCallback, useEffect, useMemo, useRef, useState } from 'react'

import { DEFAULT_CONFIG, cell, type Position } from '@/lib/zero/types'
import { evaluate as evaluateCoherence, liveness } from '@/lib/zero/gate'
import { defaultCaps, readout as sessionReadout } from '@/lib/zero/session'
import { evaluateGate, GATE_NOTE } from '@/lib/zero/gatekeep'
import { buildReadout, coverageMeter, type Role } from '@/lib/zero/readout'
import { initialState, type ZeroState } from '@/lib/zero/engine'
import { loadRpc, saveRpc, ZeroRpc, type RpcConfig } from '@/lib/zero/host/rpc'
import { SESSION_WARNING } from '@/lib/zero/session'
import { ZeroRuntime } from '@/lib/zero/host/runtime'
import { resolveVaults, submitSwap, bytesToBase58 } from '@/lib/zero/host/actions'
import { createSession, destroySession, loadSession, sessionSigner, type Signer } from '@/lib/zero/host/signer'
import { clearFunder } from '@/lib/zero/host/treasury'
import { SessionPanel } from './SessionPanel'
import type { Keypair } from '@solana/web3.js'

const ROLE_CLASS: Record<Role, string> = {
  ok: 'text-zero-ok',
  warn: 'text-zero-warn',
  alarm: 'text-zero-alarm',
  idle: 'text-zero-dim',
  unmeasured: 'text-zero-dim',
  claim: 'text-zero-accent',
}

export function ZeroTerminal() {
  const [state, setState] = useState<ZeroState>(() =>
    initialState(DEFAULT_CONFIG, defaultCaps(Date.now() / 1000)),
  )
  const [rpc, setRpc] = useState<RpcConfig | null>(null)
  const [endpoint, setEndpoint] = useState('')
  const [now, setNow] = useState(() => Date.now() / 1000)
  const runtime = useRef<ZeroRuntime | null>(null)
  const signer = useRef<Signer | null>(null)
  // Held in state as well as in the ref so the funding panel re-renders when the key
  // appears. The ref is what the runtime signs with; this is what the UI displays.
  const [sessionKey, setSessionKey] = useState<Keypair | null>(null)
  const [caps] = useState(() => defaultCaps(Date.now() / 1000))

  useEffect(() => {
    const existing = loadSession()
    if (existing && existing.stored.expiresAtUnix > Date.now() / 1000) {
      setSessionKey(existing.keypair)
      signer.current = sessionSigner(existing.keypair)
    }
  }, [])

  useEffect(() => {
    setRpc(loadRpc())
    // One timer, and it renders rather than decides — see runtime.ts. Its whole job is
    // to make a stale feed VISIBLE, which is the opposite of a timer deciding money.
    const id = setInterval(() => setNow(Date.now() / 1000), 2000)
    return () => clearInterval(id)
  }, [])

  // The runtime owns the socket, the lease and persistence. It is created once the
  // endpoint is known, and torn down on unmount so a navigation cannot leave a second
  // writer holding the lock.
  useEffect(() => {
    if (!rpc) return
    const client = new ZeroRpc(rpc)
    const rt = new ZeroRuntime(DEFAULT_CONFIG, caps, {
      onState: setState,
      resolveVaults: mint => resolveVaults(client, mint, bytesToBase58),
      submitSwap: (effect, s) => submitSwap(client, effect, s),
      signerFor: kind => (kind === 'session' ? signer.current : null),
    })
    runtime.current = rt
    rt.start(rpc)
    return () => {
      rt.stop()
      runtime.current = null
    }
  }, [rpc, caps])

  // Creating the key and arming the loop are one button, but funding it is deliberately
  // a separate, wallet-approved step: creating a key risks nothing, and the moment a
  // human decides how much the bot may lose should not be buried inside "start".
  const arm = useCallback(() => {
    const nowUnix = Date.now() / 1000
    const existing = loadSession()
    const s =
      existing && existing.stored.expiresAtUnix > nowUnix
        ? existing
        : createSession(caps.expiresAtUnix, nowUnix)
    setSessionKey(s.keypair)
    signer.current = sessionSigner(s.keypair)
    runtime.current?.dispatch({ kind: 'arm', atUnix: nowUnix })
  }, [caps])

  // The kill switch destroys the key material rather than clearing a flag, so "disarmed"
  // means the same thing to a reader of localStorage as it does to this UI.
  const kill = useCallback(() => {
    destroySession()
    clearFunder()
    setSessionKey(null)
    signer.current = null
    runtime.current?.dispatch({ kind: 'kill', atUnix: Date.now() / 1000 })
  }, [])

  const coherence = useMemo(
    () => evaluateCoherence(state.coherence, state.config.minCoherenceSamples, state.config.minPsi),
    [state.coherence, state.config],
  )
  const live = useMemo(
    () => liveness(state.socketOpen, state.lastArrivalUnix, now),
    [state.socketOpen, state.lastArrivalUnix, now],
  )
  const session = useMemo(
    () => sessionReadout(state.ledger, state.caps, state.armed, null, now),
    [state.ledger, state.caps, state.armed, now],
  )
  const gate = useMemo(() => evaluateGate(null), [])

  const open = (Object.values(state.positions) as Position[]).filter(
    p => p.state === 'open' || p.state === 'opening',
  )
  const view = useMemo(
    () => buildReadout(coherence, live, session, state.lease, gate, open.length, state.config.minPsi),
    [coherence, live, session, state.lease, gate, open.length, state.config.minPsi],
  )

  const connect = useCallback(() => {
    const trimmed = endpoint.trim()
    if (!trimmed) return
    const config = { http: trimmed }
    saveRpc(config)
    setRpc(config)
  }, [endpoint])

  return (
    <main className="zero-root min-h-screen bg-zero-black text-zero-text px-4 py-6 md:px-8">
      <header className="max-w-5xl mx-auto mb-6">
        <h1 className="text-lg tracking-[0.2em] text-zero-accent">SCEMATICA ZERO</h1>
        <p className="text-xs text-zero-dim mt-1 max-w-2xl">
          The loop in your browser. No local API, no custody, no install. It is{' '}
          <strong className="text-zero-text">not a sniper</strong> — a page loses the first-block
          race by construction, so Zero trades on selectivity and on being able to prove what it
          did, including what it declined.
        </p>
      </header>

      <div className="max-w-5xl mx-auto space-y-4">
        {/* The headline. First, always. */}
        <section className={`border border-zero-border bg-zero-surface px-4 py-3 ${ROLE_CLASS[view.headline.role]}`}>
          <div className="text-[10px] uppercase tracking-wider text-zero-dim">status</div>
          <div className="text-sm mt-1">{view.headline.text}</div>
        </section>

        {!rpc && <RpcSetup endpoint={endpoint} onEndpoint={setEndpoint} onConnect={connect} />}

        <section className="grid grid-cols-2 md:grid-cols-4 gap-2">
          {view.gauges.map(g => (
            <div key={g.label} className="border border-zero-border bg-zero-surface px-3 py-2">
              <div className="text-[10px] uppercase tracking-wider text-zero-dim">{g.label}</div>
              <div className={`text-lg ${ROLE_CLASS[g.role]}`}>{g.text}</div>
              {/* An unmeasured gauge draws a DASHED full sweep and a measured zero draws
                  nothing. A zero-length bar for both is the em-dash rule broken in pixels. */}
              <div className="h-1 mt-1 bg-zero-hi">
                {g.fill === null ? (
                  <div className="h-full w-full border-t border-dashed border-zero-dim" />
                ) : (
                  <div className="h-full bg-zero-accent" style={{ width: `${g.fill * 100}%` }} />
                )}
              </div>
              <div className="text-[10px] text-zero-dim mt-1 leading-tight">{g.note}</div>
            </div>
          ))}
        </section>

        <section className="border border-zero-border bg-zero-surface">
          <div className="px-4 py-2 border-b border-zero-border text-xs text-zero-accent uppercase tracking-wider">
            autonomy
          </div>
          <div className="px-4 py-3 space-y-2 text-xs">
            <div className={ROLE_CLASS[view.session.role]}>{view.session.text}</div>
            {/* The cap is on screen whenever the key is armed. A cap the operator cannot
                see reads as a broken button. */}
            <div className="text-zero-warn">{SESSION_WARNING}</div>
            <div className={ROLE_CLASS[view.lease.role]}>{view.lease.text}</div>
            <div className={ROLE_CLASS[view.gate.role]}>{view.gate.text}</div>
            <div className="text-zero-dim">{GATE_NOTE}</div>
            <div className="flex gap-2 pt-1">
              <button
                onClick={arm}
                disabled={!rpc || state.armed || state.killed}
                className="px-3 py-1.5 border border-zero-border text-zero-accent hover:bg-zero-hi disabled:opacity-30"
              >
                arm
              </button>
              {/* Halts entries and destroys the key. It deliberately does NOT liquidate:
                  a kill switch that dumps at market is a different and far more dangerous
                  control, and conflating the two means nobody can stop new entries
                  without also being forced to sell. */}
              <button
                onClick={kill}
                className="px-3 py-1.5 border border-zero-alarm text-zero-alarm hover:bg-zero-hi"
              >
                kill — halts entries, keeps positions
              </button>
            </div>
          </div>
        </section>

        {view.notes.length > 0 && (
          <section className="border border-zero-alarm bg-zero-surface px-4 py-3 space-y-2">
            {view.notes.map((n, i) => (
              <div key={i} className="text-xs text-zero-warn">{n}</div>
            ))}
          </section>
        )}

        <SessionPanel
          rpc={rpc}
          session={sessionKey}
          caps={caps}
          openPositions={open.length}
          onFunded={() => setNow(Date.now() / 1000)}
        />

        <Positions state={state} />
        <Decisions state={state} />
      </div>
    </main>
  )
}

function RpcSetup({
  endpoint,
  onEndpoint,
  onConnect,
}: {
  endpoint: string
  onEndpoint: (v: string) => void
  onConnect: () => void
}) {
  return (
    <section className="border border-zero-warn bg-zero-surface px-4 py-3">
      <div className="text-xs text-zero-warn uppercase tracking-wider">bring your own endpoint</div>
      <p className="text-xs text-zero-dim mt-2 max-w-2xl">
        Zero needs a Solana RPC with a WebSocket. It is stored in this browser and never sent to
        our server — there is no keyed endpoint we could ship, because anything a page can read is
        served to every visitor. Your rate limit is your own.
      </p>
      <div className="flex gap-2 mt-3">
        <input
          value={endpoint}
          onChange={e => onEndpoint(e.target.value)}
          placeholder="https://mainnet.helius-rpc.com/?api-key=..."
          className="flex-1 bg-zero-black border border-zero-border px-3 py-2 text-xs outline-none focus:border-zero-accent"
        />
        <button
          onClick={onConnect}
          className="px-4 py-2 text-xs border border-zero-border text-zero-accent hover:bg-zero-hi"
        >
          connect
        </button>
      </div>
    </section>
  )
}

function Positions({ state }: { state: ZeroState }) {
  const rows: Position[] = Object.values(state.positions)
  return (
    <section className="border border-zero-border bg-zero-surface">
      <div className="px-4 py-2 border-b border-zero-border text-xs text-zero-accent uppercase tracking-wider">
        positions
      </div>
      {rows.length === 0 ? (
        <p className="px-4 py-4 text-xs text-zero-dim">None open.</p>
      ) : (
        <div className="divide-y divide-zero-border">
          {rows.map(p => (
            <div key={p.mint} className="px-4 py-2 text-xs flex justify-between gap-3">
              <span className="text-zero-text">{p.symbol}</span>
              <span className="text-zero-dim">
                entry {cell(p.entryPriceSol, 8)} · last {cell(p.lastPriceSol, 8)} · peak{' '}
                {cell(p.peakPriceSol, 8)}
              </span>
              <span className={p.state === 'unknown' ? 'text-zero-alarm' : 'text-zero-dim'}>
                {p.state}
              </span>
            </div>
          ))}
        </div>
      )}
    </section>
  )
}

function Decisions({ state }: { state: ZeroState }) {
  const rows = [...state.records].reverse().slice(0, 20)
  return (
    <section className="border border-zero-border bg-zero-surface">
      <div className="px-4 py-2 border-b border-zero-border text-xs text-zero-accent uppercase tracking-wider">
        decisions — including the declines
      </div>
      <p className="px-4 py-2 text-[10px] text-zero-dim border-b border-zero-border">
        A branch nobody took has no outcome and never will. The declines are counted, never
        scored — a policy scored only on the trades it took improves by taking fewer.
      </p>
      {rows.length === 0 ? (
        <p className="px-4 py-4 text-xs text-zero-dim">Nothing decided yet.</p>
      ) : (
        <div className="divide-y divide-zero-border">
          {rows.map((r, i) => (
            <div key={`${r.mint}-${r.atUnix}-${i}`} className="px-4 py-2 text-xs">
              <div className="flex justify-between gap-3">
                <span className={r.act ? 'text-zero-ok' : 'text-zero-dim'}>
                  {r.act ? 'ACTED' : `declined · ${r.decline}`}
                </span>
                <span className="text-zero-dim">
                  score {cell(r.score, 0)} · Ψ {cell(r.psi)} · {coverageMeter(r.coverage)}
                </span>
              </div>
              <div className="text-zero-dim mt-0.5 leading-tight">{r.reason}</div>
            </div>
          ))}
        </div>
      )}
    </section>
  )
}
