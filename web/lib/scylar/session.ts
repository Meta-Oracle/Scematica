// Transcript persistence.
//
// A conversation that evaporates on refresh is a demo, not a tool — you cannot come back
// to what Scylar said about a position ten minutes ago, and the reflex to avoid closing
// the tab is exactly the wrong thing to train.
//
// localStorage rather than a server session: there is no account here, nothing to key a
// server record to, and the transcript is the operator's own. It never leaves the
// browser except as the history already being sent with each turn.
//
// Load is defensive on purpose. localStorage is writable by anything else on the origin
// and survives across deploys, so a stored value can be stale, truncated or garbage.
// Rendering is element-based (see markdown.ts) so a hostile string is not an injection
// risk — but a wrong *shape* would throw during render and blank the page, which is why
// every field is checked rather than cast.

export interface ToolUse {
  name: string
  ok: boolean
}

export interface Turn {
  role: 'user' | 'assistant'
  content: string
  /** Set once the turn completes, so the reaction fires once and not on every render. */
  done?: boolean
  /** What state the answer was actually given — `live`, `simulation`, `off`, … */
  context?: string
  /** Read-only tools she called for this answer, in the order they ran. */
  tools?: ToolUse[]
  /** Cognitive gate in force for this answer — `go`, `caution`, or absent. */
  gate?: string
  /**
   * A decision record she asked to seal, and the operator has not yet allowed.
   *
   * Held on the turn rather than in component state so it survives a reload: a proposal the
   * operator wanted to think about should still be there when they come back, and one they
   * already actioned should not reappear as if it were pending.
   */
  sealProposal?: { goal: string; ground: string[] }
  /** Set once the operator confirmed the proposal above and a record was written. */
  sealed?: { id: string; root: string }
}

const KEY = 'scylar.transcript.v1'
const CONTEXT_KEY = 'scylar.context.v1'
const VOICE_KEY = 'scylar.voice.v1'

/**
 * Turns kept across reloads.
 *
 * Larger than the 20 the API sends upstream: scrollback is free, prompt tokens are not.
 * The transcript you can read and the transcript the model sees are different lengths on
 * purpose.
 */
export const MAX_STORED_TURNS = 60

/** Per-turn cap. One runaway response should not fill the origin's storage quota. */
const MAX_TURN_CHARS = 24_000

function isToolUse(v: unknown): v is ToolUse {
  if (!v || typeof v !== 'object') return false
  const t = v as Record<string, unknown>
  return typeof t.name === 'string' && typeof t.ok === 'boolean'
}

function isTurn(v: unknown): v is Turn {
  if (!v || typeof v !== 'object') return false
  const t = v as Record<string, unknown>
  return (
    (t.role === 'user' || t.role === 'assistant') &&
    typeof t.content === 'string' &&
    (t.done === undefined || typeof t.done === 'boolean') &&
    (t.context === undefined || typeof t.context === 'string') &&
    (t.gate === undefined || typeof t.gate === 'string') &&
    (t.tools === undefined || (Array.isArray(t.tools) && t.tools.every(isToolUse))) &&
    (t.sealProposal === undefined || isSealProposal(t.sealProposal)) &&
    (t.sealed === undefined || isSealed(t.sealed))
  )
}

/**
 * A stored seal proposal.
 *
 * Checked field by field like everything else here, and for a sharper reason than the rest:
 * this one drives a control that writes. `localStorage` is writable by anything else on the
 * origin, so a stored proposal is untrusted input, and the goal in it is what would be sent
 * to the seal route if the operator clicked. A wrong *shape* would throw during render; a
 * wrong *value* would seal something nobody asked for. The route still requires its own
 * confirmation, but that is the second line, not the first.
 */
function isSealProposal(v: unknown): v is { goal: string; ground: string[] } {
  if (!v || typeof v !== 'object') return false
  const p = v as Record<string, unknown>
  return (
    typeof p.goal === 'string' &&
    p.goal.trim().length > 0 &&
    Array.isArray(p.ground) &&
    p.ground.every((g) => typeof g === 'string')
  )
}

function isSealed(v: unknown): v is { id: string; root: string } {
  if (!v || typeof v !== 'object') return false
  const p = v as Record<string, unknown>
  return typeof p.id === 'string' && typeof p.root === 'string'
}

/** Parse a stored transcript. Pure, so it can be tested without a browser. */
export function deserialise(raw: string | null): Turn[] {
  if (!raw) return []
  try {
    const parsed: unknown = JSON.parse(raw)
    if (!Array.isArray(parsed)) return []
    return parsed
      .filter(isTurn)
      .map((t) => ({ ...t, content: t.content.slice(0, MAX_TURN_CHARS) }))
      .slice(-MAX_STORED_TURNS)
  } catch {
    return []
  }
}

/** Serialise for storage. Drops incomplete turns — a half-streamed reply is not history. */
export function serialise(turns: Turn[]): string {
  return JSON.stringify(
    turns
      .filter((t) => t.role === 'user' || t.done)
      .slice(-MAX_STORED_TURNS)
      .map((t) => ({ ...t, content: t.content.slice(0, MAX_TURN_CHARS) })),
  )
}

export function loadTranscript(): Turn[] {
  if (typeof window === 'undefined') return []
  try {
    return deserialise(window.localStorage.getItem(KEY))
  } catch {
    // Storage disabled (private mode, embedded webview). Not worth surfacing: the
    // terminal works exactly the same, it just forgets.
    return []
  }
}

export function saveTranscript(turns: Turn[]): void {
  if (typeof window === 'undefined') return
  try {
    window.localStorage.setItem(KEY, serialise(turns))
  } catch {
    // Quota exceeded or storage disabled.
  }
}

export function clearTranscript(): void {
  if (typeof window === 'undefined') return
  try {
    window.localStorage.removeItem(KEY)
  } catch {
    /* nothing to clear */
  }
}

/** Context toggle, remembered separately so clearing the transcript doesn't reset it. */
export function loadContextPref(fallback: boolean): boolean {
  if (typeof window === 'undefined') return fallback
  try {
    const v = window.localStorage.getItem(CONTEXT_KEY)
    return v === null ? fallback : v === '1'
  } catch {
    return fallback
  }
}

export function saveContextPref(enabled: boolean): void {
  if (typeof window === 'undefined') return
  try {
    window.localStorage.setItem(CONTEXT_KEY, enabled ? '1' : '0')
  } catch {
    /* storage disabled */
  }
}

/**
 * Voice toggle. Defaults **off**, unlike context.
 *
 * Audio that starts by itself on page load is hostile — the operator may be on a call,
 * or have the tab open next to a trading terminal. Opting in is a keystroke; opting out
 * after being startled is a scramble for the mute key.
 */
export function loadVoicePref(): boolean {
  if (typeof window === 'undefined') return false
  try {
    return window.localStorage.getItem(VOICE_KEY) === '1'
  } catch {
    return false
  }
}

export function saveVoicePref(enabled: boolean): void {
  if (typeof window === 'undefined') return
  try {
    window.localStorage.setItem(VOICE_KEY, enabled ? '1' : '0')
  } catch {
    /* storage disabled */
  }
}
