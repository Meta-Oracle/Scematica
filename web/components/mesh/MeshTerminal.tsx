'use client'

import { useState } from 'react'

import { GateSolver } from './GateSolver'
import { MeshGraph } from './MeshGraph'
import { Pairing } from '@/components/Pairing'
import { apiFetch, apiBase, getPairing, isMixedContent } from '@/lib/net'
import { usePoll } from '@/lib/store'
import { TONE_HEX, ageLabel, toneFor, visibilityLabel } from '@/lib/mesh/view'
import { isMesh, type Cognition, type Mesh, type MeshNode, type Term } from '@/lib/mesh/types'

// The page shell: fetch, headline, graph, gate, detail.
//
// Subscribes through the shared store (`usePoll`) rather than owning a `setInterval`, so
// this page costs exactly one timer no matter how many panels read the same key.

const POLL_MS = 4_000

/**
 * Why this page cannot show a mesh right now.
 *
 * Collapsing these costs the operator the fix, and this page used to collapse all of them
 * into `no_instance` — which is how a *correctly running* bot behind a mis-rooted base URL
 * came out reading "No instance paired". Every remedy below is different:
 *
 *   no_instance   nothing is paired at all — the honest empty state
 *   blocked       a pairing exists, but the browser refuses to send the request
 *   misrooted     something answered, at a URL that is not the API root
 *   unreachable   the request was sent and nothing answered
 *   malformed     the API answered 200 with something that is not a mesh
 */
type Unavailable = {
  unavailable: string
  reason: 'no_instance' | 'blocked' | 'misrooted' | 'unreachable' | 'malformed'
  /** The URL actually requested, when one was. Shown verbatim — it is usually the bug. */
  attempted?: string
}

async function fetchMesh(): Promise<Mesh | Unavailable> {
  const base = apiBase()
  const attempted = (base || '') + '/api/mesh'

  // An `http://` pairing read from an `https://` page is refused by the browser as mixed
  // content — the request never leaves, so there is no status code to interpret and the
  // only symptom is a console warning the page cannot see. Detect it up front rather than
  // reporting the resulting `TypeError` as "unreachable", which sends the operator off
  // checking a firewall that is not the problem. This is the default situation on a
  // Vercel deploy paired to a LAN or localhost instance.
  if (base && isMixedContent(base)) {
    return {
      reason: 'blocked',
      attempted,
      unavailable: `This page is served over HTTPS and the paired instance is ${base} — the browser blocks that request as mixed content before it is sent. Serve the instance over HTTPS (cloudflared, ngrok or Tailscale Funnel each give you one), or open this dashboard from http://localhost:3000 on the machine running the bot.`,
    }
  }

  // `apiFetch`, not `fetch`: a paired instance is read directly, exactly like every
  // other panel on the site. Unpaired, this is a same-origin call into the Next proxy.
  let res: Response
  try {
    res = await apiFetch('/api/mesh')
  } catch {
    return {
      reason: 'unreachable',
      attempted,
      unavailable: base
        ? `No response from ${attempted}. Check that \`scematica-api\` is running, that the tunnel in front of it is up, and that it is reachable from this browser — a CORS rejection also lands here.`
        : 'The dashboard could not reach its own API route. If this is a hosted deploy, the server had no upstream to forward to.',
    }
  }

  const json = await res.json().catch(() => null)
  if (res.ok && isMesh(json)) return json

  const body = (json ?? {}) as { error?: string; hint?: string }

  // 404 is the signature of a base URL pointing one level off — the single most common
  // pairing mistake, because the API serves BOTH /health and /api/health, so a base of
  // `https://host/api` passes a naive reachability probe and then 404s on every real
  // endpoint. Say so instead of reporting "nothing is paired".
  if (res.status === 404 || body.error === 'upstream_route_missing') {
    return {
      reason: 'misrooted',
      attempted,
      unavailable:
        body.hint ??
        `Something answered at ${attempted} with 404. The paired URL must be the API root — \`https://host\`, not \`https://host/api\` — because this page appends \`/api/mesh\` itself.`,
    }
  }

  // The designed empty state: the web layer's own 503, which carries a hint.
  if (res.status === 503 || body.error === 'no_instance_paired') {
    return { reason: 'no_instance', attempted, unavailable: body.hint ?? 'No mesh available.' }
  }

  // 200 with a body that is not a mesh. Almost always a tunnel interstitial or an
  // auth wall answering in place of the API. Never silently rendered as "unpaired".
  if (res.ok) {
    return {
      reason: 'malformed',
      attempted,
      unavailable: `${attempted} answered 200, but the body is not a mesh (no \`nodes\`/\`edges\`/\`summary\`). Something is standing in front of the API — a tunnel login page or an auth proxy will do this.`,
    }
  }

  return {
    reason: 'unreachable',
    attempted,
    unavailable:
      body.hint ?? `${attempted} answered ${res.status}${body.error ? ` — ${body.error}` : ''}.`,
  }
}

export function MeshTerminal() {
  const [selected, setSelected] = useState<string | null>(null)
  const snap = usePoll('mesh', fetchMesh, POLL_MS)

  const payload = snap.data
  const mesh = payload && 'nodes' in payload ? payload : null
  const node = mesh?.nodes.find(n => n.id === selected) ?? null

  return (
    <div className="mesh-root min-h-screen">
      <header className="border-b border-mesh-border px-5 py-4">
        <div className="flex items-baseline justify-between gap-4 flex-wrap">
          <div>
            <h1 className="text-mesh-accent text-sm uppercase tracking-[0.2em]">Scematica Mesh</h1>
            <p className="text-[11px] text-mesh-dim mt-1 max-w-2xl">
              Every decision-making unit in the running system, what it last decided, and —
              first — whether it can be seen at all. A dark node is unseen, not idle.
            </p>
          </div>
          {mesh && (
            <div className="text-right">
              <div className="text-mesh-text text-sm">{visibilityLabel(mesh)}</div>
              <div className="text-[10px] text-mesh-dim">{mesh.generated_at.slice(0, 19).replace('T', ' ')}Z</div>
            </div>
          )}
        </div>
      </header>

      {!payload && <div className="px-5 py-10 text-mesh-dim text-xs">reading the system…</div>}

      {payload && 'unavailable' in payload && <Unpaired state={payload} />}

      {mesh && (
        <>
          <Diagnosis mesh={mesh} />
          <div className="px-5 py-4">
            <MeshGraph mesh={mesh} selected={selected} onSelect={setSelected} />
          </div>
          <Legend traced={selected !== null} />
          <GatePanel cognition={mesh.cognition} />
          <GateSolver cognition={mesh.cognition} />
          {node && <NodeDetail node={node} onClose={() => setSelected(null)} />}
        </>
      )}
    </div>
  )
}

/**
 * The empty state. It is the most-seen screen on this page — a public visitor has no bot
 * — so it has to say what is missing and how to supply it, per reason. The refusal to
 * invent a topology stays at the bottom: it is the design, not an error.
 *
 * The pairing form is opened from *here*, not from `/pair`. `/pair` generates a QR for
 * the mobile app and never calls `setPairing`, so following the link this panel used to
 * show left the browser exactly as unpaired as before.
 */
function Unpaired({ state }: { state: Unavailable }) {
  const [pairing, setPairing] = useState(false)

  if (pairing) {
    return (
      <div className="fixed inset-0 z-[100] bg-scema-black overflow-y-auto">
        <button
          onClick={() => setPairing(false)}
          aria-label="Cancel pairing"
          className="absolute top-4 right-4 z-10 text-mesh-dim hover:text-mesh-text text-lg"
        >
          ✕
        </button>
        <Pairing onPaired={() => setPairing(false)} />
      </div>
    )
  }

  const heading = {
    no_instance: 'No instance paired',
    blocked: 'Pairing blocked by the browser',
    misrooted: 'Paired URL is not the API root',
    unreachable: 'Paired instance not answering',
    malformed: 'Something answered, but it is not the API',
  }[state.reason]

  // A misrooted or malformed base is a *wrong* pairing, not a missing one. The remedy is
  // to re-enter the URL, so the button leads with that rather than with setup steps the
  // operator has already done.
  const isWrongPairing = state.reason === 'misrooted' || state.reason === 'malformed'

  return (
    <div className="m-5 border border-mesh-border px-4 py-3">
      <div className={`text-xs uppercase tracking-wider ${isWrongPairing ? 'text-mesh-veto' : 'text-mesh-stale'}`}>
        {heading}
      </div>
      <p className="text-mesh-muted text-xs mt-1.5 max-w-2xl">{state.unavailable}</p>

      {/* The URL that was actually requested. On every failure above except "nothing is
          paired" this line *is* the diagnosis, and it was the one thing the page never
          showed — an operator with a working bot had no way to see that the request had
          gone to `/api/api/mesh`. */}
      {state.attempted && state.reason !== 'no_instance' && (
        <p className="text-[11px] text-mesh-dim mt-2 break-all">
          requested <code className="text-mesh-text">{state.attempted}</code>
        </p>
      )}

      {state.reason === 'no_instance' && (
        <ol className="text-mesh-dim text-[11px] mt-2.5 max-w-2xl space-y-1 list-decimal pl-4">
          <li>
            Run the API next to your bot:{' '}
            <code className="text-mesh-text">cargo run --release --bin api</code> (it serves{' '}
            <code className="text-mesh-text">/api/mesh</code> on port 3001, reading the state
            files in the working directory).
          </li>
          <li>
            Point this dashboard at it — locally, set{' '}
            <code className="text-mesh-text">RUST_API_URL=http://localhost:3001</code> and run{' '}
            <code className="text-mesh-text">npm run dev</code>; from a hosted page (Vercel),
            use <span className="text-mesh-accent">Pair an instance</span> below.
          </li>
          <li>
            Give the API <strong className="text-mesh-text">root</strong> URL —{' '}
            <code className="text-mesh-text">https://host</code>, not{' '}
            <code className="text-mesh-text">https://host/api</code>. This page appends{' '}
            <code className="text-mesh-text">/api/mesh</code> itself.
          </li>
          <li>
            A hosted HTTPS page can only read an instance served over HTTPS — put a tunnel in
            front of it, or open the dashboard on the machine running the bot.
          </li>
        </ol>
      )}

      <button
        onClick={() => setPairing(true)}
        className="mt-3 px-3 py-1.5 border border-mesh-accent/50 text-mesh-accent text-[11px]
                   uppercase tracking-widest hover:bg-mesh-accent/10 transition-colors"
      >
        {isWrongPairing ? 'Fix the pairing' : 'Pair an instance'}
      </button>

      <p className="text-mesh-dim text-[11px] mt-3 max-w-2xl">
        There is deliberately no simulated mesh. A fake metric is a fake number; a fake
        topology would assert that a particular set of units exists and is healthy on your
        machine, which is not something this page is willing to invent.
      </p>
    </div>
  )
}

function Diagnosis({ mesh }: { mesh: Mesh }) {
  const s = mesh.summary
  const alarming = s.blocking > 0
  return (
    <div
      className={`mx-5 mt-4 border px-4 py-3 ${
        alarming ? 'border-mesh-veto/60' : 'border-mesh-border'
      }`}
    >
      <div className={`text-[10px] uppercase tracking-wider ${alarming ? 'text-mesh-veto' : 'text-mesh-dim'}`}>
        {alarming ? 'Blocking' : 'Diagnosis'}
      </div>
      <p className={`text-sm mt-1 ${alarming ? 'text-mesh-veto' : 'text-mesh-text'}`}>{s.diagnosis}</p>
      {s.blocking_stale > 0 && s.blocking === 0 && (
        <p className="text-[11px] text-mesh-stale mt-1.5">
          {s.blocking_stale} veto{s.blocking_stale === 1 ? '' : 'es'} recovered from stale state —
          shown for reference, not counted as current.
        </p>
      )}
    </div>
  )
}

/** Ψ and its terms. The measured fraction sits directly beside the number, on purpose. */
function GatePanel({ cognition: c }: { cognition: Cognition }) {
  const [open, setOpen] = useState(false)
  const verdictTone: Record<Cognition['verdict'], string> = {
    act: 'text-mesh-live',
    damp: 'text-mesh-stale',
    abstain: 'text-mesh-veto',
    unevaluated: 'text-mesh-dim',
  }

  return (
    <section className="mx-5 mb-5 border border-mesh-border">
      <div className="px-4 py-3 border-b border-mesh-border flex items-baseline justify-between gap-4 flex-wrap">
        <div>
          <span className="text-[10px] text-mesh-dim uppercase tracking-wider">
            Agentic coherence gate · §32
          </span>
          <div className="text-mesh-muted text-[11px] mt-0.5">Ψ = C · K · (1 − R)</div>
        </div>
        <div className="text-right">
          <span className={`text-2xl ${verdictTone[c.verdict]}`}>{c.psi.toFixed(3)}</span>
          <span className={`ml-2 text-[11px] uppercase tracking-wider ${verdictTone[c.verdict]}`}>
            {c.verdict}
          </span>
          {/* Never separated from Ψ. A gate computed on a fifth of its inputs is a
              statement about ignorance and has to look like one. */}
          <div className="text-[10px] text-mesh-dim">
            computed on {Math.round(c.measured_fraction * 100)}% of its terms
          </div>
        </div>
      </div>

      <div className="grid gap-px bg-mesh-border sm:grid-cols-4">
        <Factor label="C · confidence" value={c.confidence} section="§17" />
        <Factor label="K · coherence" value={c.coherence.value} section="§31" note={`${c.coherence.subsystems} live subsystems`} />
        <Factor label="R · risk" value={c.risk.value} section="§20" invert />
        <Factor
          label="Ω · cognitive state"
          value={c.omega}
          section="§33"
          note={c.omega === null ? 'no subsystem exists yet' : undefined}
        />
      </div>

      <p className="px-4 py-2.5 text-[11px] text-mesh-muted border-t border-mesh-border">{c.reading}</p>

      <button
        onClick={() => setOpen(v => !v)}
        className="w-full px-4 py-2 text-[10px] text-mesh-dim hover:text-mesh-accent uppercase tracking-wider text-left border-t border-mesh-border"
      >
        {open ? '− hide' : '+ show'} the {c.confidence_terms.length + c.risk.components.length + c.omega_terms.length} terms
      </button>

      {open && (
        <div className="border-t border-mesh-border divide-y divide-mesh-border/40">
          {[...c.confidence_terms, ...c.risk.components, ...c.omega_terms].map(t => (
            <TermRow key={`${t.section}-${t.symbol}`} term={t} />
          ))}
        </div>
      )}
    </section>
  )
}

function Factor({
  label,
  value,
  section,
  note,
  invert,
}: {
  label: string
  value: number | null
  section: string
  note?: string
  invert?: boolean
}) {
  // Risk reads "good" when low, everything else when high.
  const good = value === null ? false : invert ? value < 0.25 : value > 0.75
  return (
    <div className="bg-mesh-surface px-4 py-3">
      <div className="text-[10px] text-mesh-dim uppercase tracking-wider">
        {label} <span className="text-mesh-border-hi">{section}</span>
      </div>
      <div className={`text-lg mt-0.5 ${value === null ? 'text-mesh-dim' : good ? 'text-mesh-live' : 'text-mesh-text'}`}>
        {value === null ? '—' : value.toFixed(3)}
      </div>
      {note && <div className="text-[10px] text-mesh-dim">{note}</div>}
    </div>
  )
}

function TermRow({ term: t }: { term: Term }) {
  return (
    <div className="px-4 py-2 flex items-baseline gap-3 text-[11px]">
      <span
        className={`shrink-0 w-[68px] text-[9px] uppercase tracking-wider ${
          t.measured ? 'text-mesh-live' : 'text-mesh-absent'
        }`}
      >
        {t.measured ? 'measured' : 'unmeasured'}
      </span>
      <span className="shrink-0 w-16 text-mesh-accent">{t.symbol}</span>
      <span className="shrink-0 w-12 text-mesh-text tabular-nums">{t.value.toFixed(3)}</span>
      <span className="shrink-0 w-10 text-mesh-border-hi">{t.section}</span>
      <span className="text-mesh-muted">{t.note}</span>
    </div>
  )
}

function Legend({ traced }: { traced: boolean }) {
  const items: [string, string, string][] = [
    ['live', TONE_HEX.live, 'reporting now — values are actionable'],
    ['stale', TONE_HEX.stale, 'reported once, now quiet — values are history'],
    ['unseen', TONE_HEX.absent, 'no source at all — not idle, unseen'],
    ['veto', TONE_HEX.veto, 'actively blocking, from a live source'],
  ]
  return (
    <div className="px-5 pb-4 flex gap-x-6 gap-y-2 flex-wrap items-center">
      <span className="text-[10px] text-mesh-dim">
        {traced
          ? 'tracing — only units connected to the selection are lit; click it again to clear'
          : 'click any node to trace what reaches it and what it reaches'}
      </span>
      {items.map(([label, hex, blurb]) => (
        <div key={label} className="flex items-center gap-2">
          <span className="inline-block w-2.5 h-2.5" style={{ backgroundColor: hex }} />
          <span className="text-[11px] text-mesh-text">{label}</span>
          <span className="text-[10px] text-mesh-dim">{blurb}</span>
        </div>
      ))}
    </div>
  )
}

function NodeDetail({ node, onClose }: { node: MeshNode; onClose: () => void }) {
  const tone = toneFor(node)
  const age = ageLabel(node.provenance)
  return (
    <section className="mx-5 mb-6 border" style={{ borderColor: TONE_HEX[tone] }}>
      <div className="px-4 py-3 border-b border-mesh-border flex items-start justify-between gap-4">
        <div>
          <h2 className="text-mesh-text text-sm">{node.label}</h2>
          <p className="text-[11px] text-mesh-muted mt-0.5">{node.blurb}</p>
        </div>
        <button onClick={onClose} className="text-[10px] text-mesh-dim hover:text-mesh-text uppercase">
          close
        </button>
      </div>

      <div className="px-4 py-2 text-[11px] flex gap-x-5 gap-y-1 flex-wrap border-b border-mesh-border/50">
        <span style={{ color: TONE_HEX[tone] }}>
          {node.provenance.kind}
          {age ? ` · ${age} old` : ''}
        </span>
        <span className="text-mesh-muted">verdict {node.verdict}</span>
        <span className="text-mesh-dim">{node.id}</span>
      </div>

      {node.reason && (
        <p className="px-4 py-2.5 text-xs" style={{ color: TONE_HEX[tone] }}>
          {node.reason}
        </p>
      )}

      {node.detail.length > 0 ? (
        <ul className="divide-y divide-mesh-border/40">
          {node.detail.map(([k, v]) => (
            <li key={k} className="px-4 py-1.5 flex justify-between gap-4 text-[11px]">
              <span className="text-mesh-dim">{k}</span>
              <span className="text-mesh-text">{v}</span>
            </li>
          ))}
        </ul>
      ) : (
        <p className="px-4 py-3 text-[11px] text-mesh-dim">
          No values — this unit has no source on disk, so there is nothing to report rather
          than nothing happening.
        </p>
      )}
    </section>
  )
}
