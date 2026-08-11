// The cognitive gate: whether Scylar has the footing to answer about the bot at all.
//
// Ψ is computed in Rust (`GET /api/sentience`, backed by `scematica-sentience`), never
// here. That follows the same rule as the pool scorer — one authoritative implementation,
// ported only where a port is unavoidable — and this one is not even a port: there is a
// single caller and it is already talking to the API.
//
// What the gate actually measures is worth stating plainly, because "cognitive
// architecture" invites the assumption that it is decorative. It is a staleness and
// contradiction check with a real failure case behind it: every read endpoint serves a
// state file identically whether it was written four seconds ago or four hours ago, and
// `/api/health` reports the lock file, which says a process *was* here. So a live-looking
// briefing can describe a session that ended overnight. HOLD is that case.
//
//   GO       answer normally
//   CAUTION  answer, but the model is told its footing is weak
//   HOLD     do not answer from bot state — say why instead
//
// Absent is not the same as HOLD. A deploy with no bot, or an older API without the
// endpoint, must stay fully usable; `null` means "no opinion", and the caller proceeds.

export type GateVerdict = 'go' | 'caution' | 'hold'

export interface GateReadout {
  verdict: GateVerdict
  psi: number
  sentience: number
  /** Which term is holding Ψ down — the one thing to fix first. */
  bottleneck: string
  /** The overlay's own system-prompt annotation. */
  note: string
  /** The measurements behind the verdict, for the operator rather than the model. */
  inputs: Record<string, unknown>
}

/** Short enough that a wedged API cannot stall a chat turn. */
const GATE_TIMEOUT_MS = 2_000

function isVerdict(v: unknown): v is GateVerdict {
  return v === 'go' || v === 'caution' || v === 'hold'
}

/**
 * Read the gate, or `null` when there is nothing to read.
 *
 * Never throws and never blocks for long: a gate that can break the terminal is worse
 * than no gate, because the failure it prevents is a wrong number and the failure it
 * would cause is no answer at all.
 */
export async function readGate(origin: string): Promise<GateReadout | null> {
  try {
    const res = await fetch(`${origin}/api/sentience`, {
      headers: { Accept: 'application/json' },
      cache: 'no-store',
      signal: AbortSignal.timeout(GATE_TIMEOUT_MS),
    })
    if (!res.ok) return null

    const data = (await res.json()) as Record<string, unknown>
    const verdict = String(data.gate ?? '').toLowerCase()
    if (!isVerdict(verdict)) return null

    return {
      verdict,
      psi: Number(data.psi) || 0,
      sentience: Number(data.sentience) || 0,
      bottleneck: typeof data.bottleneck === 'string' ? data.bottleneck : 'unknown',
      note: typeof data.note === 'string' ? data.note : '',
      inputs: (data.inputs as Record<string, unknown>) ?? {},
    }
  } catch {
    return null
  }
}

/**
 * Explain a HOLD to the operator in terms of the thing that is actually wrong.
 *
 * "Integrated cognition Ψ is below threshold" is true and useless. The operator needs to
 * know their sniper's metrics went cold, which is both the cause and the fix.
 */
export function holdExplanation(gate: GateReadout): string {
  const age = Number(gate.inputs?.metrics_age_secs)
  const running = gate.inputs?.sniper_running === true

  if (Number.isFinite(age) && age > 60) {
    const mins = Math.round(age / 60)
    return running
      ? `The lock file says the sniper is running, but it has not written metrics in ` +
          `${mins} minute${mins === 1 ? '' : 's'}. Either it is wedged or the lock is ` +
          `stale — the numbers on this API describe a session that has stopped moving.`
      : `The sniper is stopped and its last metrics are ${mins} minute${
          mins === 1 ? '' : 's'
        } old. Anything I said about current positions or PnL would describe a finished session.`
  }

  if (gate.inputs?.state_files_present === 0) {
    return 'None of the sniper state files are readable, so there is nothing to answer from.'
  }

  return `Bot state is not trustworthy enough to answer from right now (bottleneck: ${gate.bottleneck}).`
}

/**
 * What replaces the state block on a HOLD.
 *
 * HOLD used to end the turn with a 409 before the model ran. It no longer does, and the
 * distinction is worth being precise about: the rule being enforced is *no stale figure
 * ever reaches the model*, not *do not speak*. Withholding the block enforces that
 * completely — there is nothing left to misreport — while refusing outright enforced it
 * by going dark during the exact fault the terminal exists to explain. "The lock says
 * running but it has not written metrics in 985 minutes" is the most useful sentence
 * available in that state, and the old path could not produce it.
 *
 * Phrased as an absence rather than a warning, because she genuinely has nothing this
 * turn — no block, no tools. A warning implies data she is being asked to distrust, which
 * is the framing that produces a confident paragraph with a hedge bolted on the front.
 */
export function holdInstruction(gate: GateReadout): string {
  return [
    '[COGNITIVE GATE: HOLD] You have no bot data this turn. No SCEMATICA STATE block was',
    'attached and no tools are available to you, because the state on disk cannot be',
    'trusted right now:',
    '',
    holdExplanation(gate),
    '',
    'If the question touches the bot at all, say that plainly in your own words and give',
    'the reason above. Cite no figure for PnL, positions, trades, filters or the agent —',
    'you have none, and any number earlier in this conversation is from a previous turn',
    'and is now stale. Questions that do not depend on live bot state — trading concepts,',
    'the Scematica codebase, anything general — answer normally.',
  ].join('\n')
}

/** The extra instruction a CAUTION verdict adds to the system prompt. */
export function cautionInstruction(gate: GateReadout): string {
  return [
    `[COGNITIVE GATE: CAUTION] Ψ=${gate.psi.toFixed(3)}, bottleneck: ${gate.bottleneck}.`,
    'The bot state you were given is partial or going stale. Answer, but say which parts',
    'you are unsure of, and do not present a figure as current unless the block supports it.',
  ].join(' ')
}

/** Feed the answer back so the loop advances. Fire-and-forget by design. */
export async function recordAnswer(origin: string, text: string): Promise<void> {
  try {
    await fetch(`${origin}/api/sentience/observe`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ text: text.slice(0, 4_000) }),
      signal: AbortSignal.timeout(GATE_TIMEOUT_MS),
    })
  } catch {
    // The loop not advancing is a degraded gate, not a failed turn. Never surfaced.
  }
}
