// The briefing: what Scylar is allowed to know about the running bot.
//
// Without this she is a general chat model wearing a Scematica badge — she cannot answer
// "why did we skip that pool", "what's my drawdown", or "is the sniper even up", which
// are the only questions worth asking a terminal that sits next to the dashboard. With
// it she reads the same snapshot the panels do.
//
// Three rules hold this together, and the third is the one that matters:
//
//   1. **One hop, same resolution.** This calls the site's own `/api/*` routes rather
//      than re-deriving live-vs-simulated. `app/api/[...slug]/route.ts` already decides
//      whether an operator's bot is reachable; a second implementation here would drift
//      from it and start disagreeing with the panels on the same page.
//   2. **Read-only, and small.** Five endpoints, no control routes, capped output. Every
//      line is tokens spent on a rate-limited free tier, on every turn.
//   3. **Simulated data is labelled, loudly, in the prompt itself.** The dashboard shows
//      a permanent SIMULATION banner for this exact reason. A model given simulated PnL
//      with no marker will discuss it as real money, and unlike a banner there is nothing
//      on screen to contradict it. The marker is repeated on the section header *and* per
//      figure, because models skim.
//
// Not guarded as server-only: it reads no keys, only the same public endpoints the
// browser already polls. It is server-side because that is where the origin is known,
// not because the data is privileged.

import type {
  FilterStats,
  HealthStatus,
  LivePosition,
  Metrics,
  NNStats,
} from '@/lib/types'

/** How long the whole briefing may take. It sits in front of a user's message. */
const FETCH_TIMEOUT_MS = 2_500

/** Positions listed individually before collapsing to a summary line. */
const MAX_POSITIONS = 6

/** Rejection reasons listed. The tail is a long flat distribution and adds nothing. */
const MAX_REJECTIONS = 5

export type BriefingSource = 'live' | 'simulation' | 'unavailable'

export interface Briefing {
  source: BriefingSource
  /** Rendered block, or null when nothing could be read. */
  text: string | null
  /** Why it is unavailable, when it is. Surfaced to the operator, not to the model. */
  reason?: string
}

interface Fetched<T> {
  data: T | null
  source: BriefingSource | null
}

async function grab<T>(origin: string, path: string): Promise<Fetched<T>> {
  try {
    const res = await fetch(`${origin}/api/${path}`, {
      headers: { Accept: 'application/json' },
      cache: 'no-store',
      signal: AbortSignal.timeout(FETCH_TIMEOUT_MS),
    })
    if (!res.ok) return { data: null, source: null }

    // Set by the proxy on every response. This is the authority on whether the numbers
    // below are real — not the shape of the payload, which is identical either way.
    const header = res.headers.get('X-Scematica-Source')
    const source: BriefingSource | null =
      header === 'live' ? 'live' : header === 'simulation' ? 'simulation' : null

    return { data: (await res.json()) as T, source }
  } catch {
    return { data: null, source: null }
  }
}

const sol = (lamports: number) => (lamports / 1e9).toFixed(4)
const pct = (n: number) => `${n >= 0 ? '+' : ''}${n.toFixed(1)}%`

function uptime(secs: number): string {
  if (secs < 60) return `${Math.round(secs)}s`
  if (secs < 3600) return `${Math.round(secs / 60)}m`
  const h = Math.floor(secs / 3600)
  return `${h}h ${Math.round((secs % 3600) / 60)}m`
}

/**
 * Read the current bot snapshot and render it as a prompt block.
 *
 * `origin` is this deployment's own origin — the caller has it from the incoming
 * request. `SCYLAR_SELF_ORIGIN` overrides it for deployments that terminate TLS
 * somewhere the request URL does not reflect.
 */
export async function buildBriefing(origin: string): Promise<Briefing> {
  const base = (process.env.SCYLAR_SELF_ORIGIN || origin).replace(/\/$/, '')

  const [health, metrics, positions, nn, filters] = await Promise.all([
    grab<HealthStatus>(base, 'health'),
    grab<Metrics>(base, 'metrics'),
    grab<LivePosition[]>(base, 'positions'),
    grab<NNStats>(base, 'nn'),
    grab<FilterStats>(base, 'filters'),
  ])

  const sources = [health, metrics, positions, nn, filters]
    .map((r) => r.source)
    .filter((s): s is BriefingSource => s !== null)

  if (sources.length === 0) {
    return {
      source: 'unavailable',
      text: null,
      reason: `No endpoint under ${base}/api answered within ${FETCH_TIMEOUT_MS}ms.`,
    }
  }

  // Any simulated endpoint taints the whole briefing. Mixing the two and labelling
  // per-section would put the burden of tracking which number is real on the model,
  // and it will get that wrong before it gets anything else wrong.
  const source: BriefingSource = sources.includes('simulation') ? 'simulation' : 'live'
  const tag = source === 'simulation' ? ' [SIMULATED]' : ''

  const lines: string[] = []

  if (health.data) {
    lines.push(
      `Sniper: ${health.data.sniper_running ? 'RUNNING' : 'STOPPED'}` +
        (health.data.api ? ` (api: ${health.data.api})` : ''),
    )
  }

  if (metrics.data) {
    const m = metrics.data
    const winRate =
      m.trades_attempted > 0
        ? ` (${((m.trades_confirmed / m.trades_attempted) * 100).toFixed(0)}% confirmed)`
        : ''
    lines.push(
      `Session${tag}: PnL ${sol(m.total_pnl_lamports)} SOL · ` +
        `${m.trades_confirmed}/${m.trades_attempted} trades${winRate} · ` +
        `${m.trades_failed} failed · ${m.pools_tracked} pools tracked · up ${uptime(m.uptime_secs)}`,
    )
    if (m.arb_opportunities_found > 0) {
      lines.push(`Arb: ${m.arb_executed}/${m.arb_opportunities_found} opportunities executed`)
    }
  }

  if (Array.isArray(positions.data)) {
    const open = positions.data
    if (open.length === 0) {
      lines.push('Open positions: none')
    } else {
      lines.push(`Open positions${tag}: ${open.length}`)
      for (const p of open.slice(0, MAX_POSITIONS)) {
        const move = p.entry_lamports > 0
          ? ((p.current_value_lamports - p.entry_lamports) / p.entry_lamports) * 100
          : 0
        const age = Math.max(0, Math.floor(Date.now() / 1000) - p.entry_unix_secs)
        lines.push(
          `  - ${p.mint.slice(0, 6)}…${p.mint.slice(-4)} ${pct(move)} · ` +
            `${sol(p.current_value_lamports)} SOL · held ${uptime(age)} · ` +
            `TP ${p.dynamic_tp_pct.toFixed(0)}% SL ${p.current_sl_pct.toFixed(0)}%` +
            (p.escalations > 0 ? ` · ${p.escalations} escalations` : ''),
        )
      }
      if (open.length > MAX_POSITIONS) {
        lines.push(`  - …and ${open.length - MAX_POSITIONS} more`)
      }
    }
  }

  if (nn.data) {
    const n = nn.data
    lines.push(
      `Deep Q*: ${n.train_steps} train steps · ε ${n.epsilon.toFixed(3)} · ` +
        `replay ${n.replay_size} · avg loss ${n.avg_loss.toFixed(4)} · ` +
        `${n.ready_to_advise ? 'gating buys' : 'not yet advising'}`,
    )
  }

  if (filters.data) {
    const f = filters.data
    const passRate = f.pools_seen > 0 ? ((f.pools_passed / f.pools_seen) * 100).toFixed(1) : '0.0'
    lines.push(`Filters: ${f.pools_passed}/${f.pools_seen} pools passed (${passRate}%)`)

    const top = Object.entries(f.rejections || {})
      .filter(([, n]) => n > 0)
      .sort((a, b) => b[1] - a[1])
      .slice(0, MAX_REJECTIONS)
    if (top.length > 0) {
      lines.push(`Top rejections: ${top.map(([name, n]) => `${name} ${n}`).join(', ')}`)
    }
  }

  return { source, text: lines.join('\n') }
}

/**
 * Turn a briefing into the system message that carries it.
 *
 * The instructions travel *with* the data rather than living in the persona, so a turn
 * sent without context cannot inherit a prompt telling her she has live state. That
 * asymmetry is the point: the failure mode being designed against is her answering
 * confidently from a briefing that was never attached.
 */
export function contextSystemMessage(briefing: Briefing): string | null {
  if (!briefing.text) return null

  // Phrased as a hard output requirement rather than a description of the situation.
  // Tested against Groq's llama-3.3-70b: "say so whenever you cite them" was ignored
  // outright — it read as context, not as an instruction — while naming the required
  // word is complied with. The per-turn SIMULATED badge in the UI remains the actual
  // guarantee; this only reduces how often the text and the badge disagree.
  const preamble =
    briefing.source === 'simulation'
      ? [
          'These figures come from the SIMULATION ENGINE, not a live bot. No real money',
          'and no real positions are involved. REQUIRED: any reply that cites a number',
          'from this block must contain the word "simulated". On screen these read',
          'exactly like live figures, so omitting it misleads the operator about their',
          'own money.',
        ].join(' ')
      : [
          "The block below is a live read of the operator's running Scematica instance,",
          'taken just now. It is your only source of truth about the bot.',
        ].join(' ')

  return [
    `SCEMATICA STATE (${briefing.source.toUpperCase()})`,
    '',
    preamble,
    '',
    'Cite figures from this block exactly. Do not extrapolate a trend from a single',
    'snapshot, do not estimate anything absent from it, and if asked about something it',
    'does not cover, say it is not in the snapshot rather than guessing.',
    '',
    '---',
    briefing.text,
    '---',
  ].join('\n')
}
