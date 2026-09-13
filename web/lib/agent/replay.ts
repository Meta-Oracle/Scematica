// Replay an Omni-Agent proposal log, in the reader's own browser.
//
// This is a PORT of the state machine in `omni-agent/src/lib/queue.ts`, and the
// TypeScript there is authoritative. Same relationship as `lib/omni/canonical.ts` to
// `scema-verify`, and the same hazard: a port that drifts reports a history that did not
// happen, which is worse than no port at all because it looks like evidence.
//
// ── What this file may and may not do ────────────────────────────────────────
//
// The queue is append-only JSONL, and state is *derived* by replaying it rather than
// stored. So this module replays and nothing else. In particular:
//
//   - **It never invents a score.** A proposal written before the cortex was reachable
//     carries no scores at all. `scored: false` is a different fact from `taste: 0`, and
//     the renderer must be able to tell them apart — see `agent-unscored` in the palette.
//   - **It never computes a rate the log cannot support.** An approval rate over three
//     decisions is not a measurement of taste, so the counts are what leave this file and
//     the reader does the dividing, or does not.
//   - **An unrecognised event is counted, not dropped.** A log written by a newer agent
//     than this port will contain event types it has never heard of, and silently
//     discarding them would render a confident, incomplete history. They surface as
//     `unknownEvents` and the page says so.
//
// ── The part worth reading twice ─────────────────────────────────────────────
//
// `labelsFor` is why the page exists. The interesting thing about this log is not that a
// draft was posted — it is that every decision became a labelled training example, and an
// *edit* became two of them with opposite signs. That asymmetry is the whole learning
// signal, it is invisible in the raw JSONL, and it is the one thing here a reader could
// not work out by eye.

export type ProposalStatus =
  | 'pending'
  | 'approved'
  | 'rejected'
  | 'posted'
  | 'failed'
  | 'expired'

export interface ProposalScores {
  salience: number
  taste: number
  resonance: number
  novelty: number
  priority: number
}

export interface Engagement {
  likes: number
  reposts: number
  replies: number
  measuredAt: string
}

export interface Proposal {
  id: string
  createdAt: string
  text: string
  rationale: string
  sources: string[]
  topic: string
  /** Absent when the cortex never scored this draft. Not zero. */
  scores?: ProposalScores
  status: ProposalStatus
  editedText?: string
  decidedAt?: string
  decidedBy?: string
  postedId?: string
  postedAt?: string
  error?: string
  engagement?: Engagement
}

/** One labelled example a decision handed the cortex. */
export interface Label {
  /** The text that carried the label. An edit produces two, with different text. */
  text: string
  taste: 0 | 1
  /** Only an approval asserts salience; a rejection says nothing about it. */
  salience?: 1
  source: 'telegram' | 'telegram-edit-original' | 'telegram-edit-final'
}

export interface Replay {
  proposals: Proposal[]
  counts: Record<ProposalStatus, number>
  /** Lines that parsed but named an event type this port does not know. */
  unknownEvents: number
  /** Lines that did not parse as JSON at all. */
  malformedLines: number
  /** Total events replayed, including the unknown ones. */
  events: number
}

const EMPTY_COUNTS = (): Record<ProposalStatus, number> => ({
  pending: 0,
  approved: 0,
  rejected: 0,
  posted: 0,
  failed: 0,
  expired: 0,
})

/**
 * The text that actually went out: the operator's edit if there was one.
 *
 * Mirrors `finalText` in the agent. Kept as a function rather than inlined because the
 * distinction between "what the agent wrote" and "what was published" is exactly what a
 * reader of this page is trying to see.
 */
export function finalText(proposal: Proposal): string {
  return proposal.editedText ?? proposal.text
}

/** Was this draft ever scored by the cortex? */
export function isScored(proposal: Proposal): proposal is Proposal & { scores: ProposalScores } {
  return proposal.scores !== undefined
}

/**
 * The labels a decision produced.
 *
 * An edit is a richer signal than a bare approval: the original was not good enough
 * (negative), the rewrite was (positive). Teaching both is what lets the net learn the
 * *difference* rather than just the direction, and it is the reason an operator who edits
 * is worth more to this system than one who only presses the green button.
 *
 * A pending, expired or failed proposal produced no labels — nobody decided anything, and
 * an undecided draft is not a rejected one.
 */
export function labelsFor(proposal: Proposal): Label[] {
  if (proposal.status === 'rejected') {
    return [{ text: finalText(proposal), taste: 0, source: 'telegram' }]
  }
  if (proposal.status !== 'approved' && proposal.status !== 'posted') return []

  const edited = proposal.editedText
  if (edited !== undefined && edited !== proposal.text) {
    return [
      { text: proposal.text, taste: 0, source: 'telegram-edit-original' },
      { text: edited, taste: 1, salience: 1, source: 'telegram-edit-final' },
    ]
  }
  return [{ text: finalText(proposal), taste: 1, salience: 1, source: 'telegram' }]
}

function asScores(value: unknown): ProposalScores | undefined {
  if (typeof value !== 'object' || value === null) return undefined
  const s = value as Record<string, unknown>
  const keys = ['salience', 'taste', 'resonance', 'novelty', 'priority'] as const
  // Every field or none. A partial score object is a bug somewhere upstream, and filling
  // the gaps with zeros would turn it into five confident numbers.
  if (!keys.every((k) => typeof s[k] === 'number' && Number.isFinite(s[k] as number))) {
    return undefined
  }
  return {
    salience: s.salience as number,
    taste: s.taste as number,
    resonance: s.resonance as number,
    novelty: s.novelty as number,
    priority: s.priority as number,
  }
}

/**
 * Replay a proposals.jsonl into the state it describes.
 *
 * Never throws. A log is an artefact somebody drags onto a page, so half of them will be
 * the wrong file, truncated, or written by a version this port has not seen — and each of
 * those is a thing to report rather than an exception to raise.
 */
export function replayQueue(text: string): Replay {
  const proposals = new Map<string, Proposal>()
  let unknownEvents = 0
  let malformedLines = 0
  let events = 0

  for (const line of text.split('\n')) {
    if (!line.trim()) continue
    let event: Record<string, unknown>
    try {
      const parsed: unknown = JSON.parse(line)
      if (typeof parsed !== 'object' || parsed === null) throw new Error('not an object')
      event = parsed as Record<string, unknown>
    } catch {
      malformedLines += 1
      continue
    }
    events += 1

    const type = event.type
    if (type === 'proposed') {
      const raw = event.proposal as Record<string, unknown> | undefined
      if (!raw || typeof raw.id !== 'string') {
        unknownEvents += 1
        continue
      }
      proposals.set(raw.id, {
        id: raw.id,
        createdAt: typeof raw.createdAt === 'string' ? raw.createdAt : '',
        text: typeof raw.text === 'string' ? raw.text : '',
        rationale: typeof raw.rationale === 'string' ? raw.rationale : '',
        sources: Array.isArray(raw.sources) ? (raw.sources as string[]) : [],
        topic: typeof raw.topic === 'string' ? raw.topic : '',
        scores: asScores(raw.scores),
        status: 'pending',
      })
      continue
    }

    const id = typeof event.id === 'string' ? event.id : null
    const proposal = id ? proposals.get(id) : undefined
    // An event about a proposal whose `proposed` line is not in this file is not
    // malformed — it is a truncated log, which is an ordinary thing to be handed.
    if (!proposal) {
      if (type !== 'decided' && type !== 'posted' && type !== 'failed' &&
          type !== 'expired' && type !== 'engagement') {
        unknownEvents += 1
      }
      continue
    }

    switch (type) {
      case 'decided': {
        const status = event.status
        if (status !== 'approved' && status !== 'rejected') {
          unknownEvents += 1
          break
        }
        proposal.status = status
        proposal.decidedAt = typeof event.at === 'string' ? event.at : undefined
        proposal.decidedBy = typeof event.by === 'string' ? event.by : undefined
        if (typeof event.editedText === 'string') proposal.editedText = event.editedText
        break
      }
      case 'posted':
        proposal.status = 'posted'
        proposal.postedId = typeof event.postedId === 'string' ? event.postedId : undefined
        proposal.postedAt = typeof event.at === 'string' ? event.at : undefined
        break
      case 'failed':
        proposal.status = 'failed'
        proposal.error = typeof event.error === 'string' ? event.error : undefined
        break
      case 'expired':
        proposal.status = 'expired'
        break
      case 'engagement':
        if (
          typeof event.likes === 'number' &&
          typeof event.reposts === 'number' &&
          typeof event.replies === 'number'
        ) {
          proposal.engagement = {
            likes: event.likes,
            reposts: event.reposts,
            replies: event.replies,
            measuredAt: typeof event.at === 'string' ? event.at : '',
          }
        } else {
          unknownEvents += 1
        }
        break
      default:
        unknownEvents += 1
    }
  }

  const list = [...proposals.values()].sort((a, b) => b.createdAt.localeCompare(a.createdAt))
  const counts = EMPTY_COUNTS()
  for (const proposal of list) counts[proposal.status] += 1

  return { proposals: list, counts, unknownEvents, malformedLines, events }
}

/**
 * How many labels this log handed the cortex, and in what proportion.
 *
 * Counts only. There is deliberately no "approval rate" here: over the handful of
 * decisions a real log contains, a percentage is a number of the right shape with nothing
 * behind it, and this project has paid for that mistake often enough to name it.
 */
export function labelTotals(proposals: Proposal[]): {
  positive: number
  negative: number
  fromEdits: number
} {
  let positive = 0
  let negative = 0
  let fromEdits = 0
  for (const proposal of proposals) {
    const labels = labelsFor(proposal)
    if (labels.length === 2) fromEdits += 1
    for (const label of labels) {
      if (label.taste === 1) positive += 1
      else negative += 1
    }
  }
  return { positive, negative, fromEdits }
}

/** A score as text, or an em dash when the cortex never scored this draft. */
export function scoreCell(proposal: Proposal, key: keyof ProposalScores): string {
  // The one rule this file shares with `lib/omni/view.ts::cell` and
  // `scema_policy::render`: a quantity nobody measured prints as an em dash, and a
  // measured zero prints as `0.00`, because that is a real observation.
  if (!isScored(proposal)) return '—'
  return proposal.scores[key].toFixed(2)
}
