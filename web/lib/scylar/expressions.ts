// Scylar's expression state machine.
//
// Three sprites, no alpha channel, no rig. Everything the avatar "does" is a choice
// of which of the three to show and when to swap. Keeping that choice here — as pure
// functions over an explicit state — is what makes it testable without a browser and
// what stops the timing constants from scattering across components.
//
// The source art is 1254x1254 24bpp RGB with the background baked in, so the avatar is
// a framed portrait rather than a character composited over the page. That is a
// property of the assets, not a styling decision: there is no alpha to composite with.

/** The three available sprites. File stems under `public/scylar/`. */
export type Expression = 'idle' | 'talking' | 'joyous'

export const EXPRESSIONS: readonly Expression[] = ['idle', 'talking', 'joyous'] as const

/** What the terminal is doing. The sprite is derived from this, never set directly. */
export type AvatarPhase =
  /** Waiting for the operator. */
  | { kind: 'idle' }
  /** Request sent, no tokens yet — she is listening, not speaking. */
  | { kind: 'thinking' }
  /** Tokens arriving; `elapsedMs` drives the mouth flap. */
  | { kind: 'streaming'; elapsedMs: number }
  /** Response finished; `positive` decides whether she reacts. */
  | { kind: 'settled'; positive: boolean; sinceMs: number }

/**
 * Mouth-flap period. `idle` is closed-mouth and `talking` is open-mouth, so alternating
 * them is a two-frame flap — the same trick VTuber overlays use.
 *
 * 110ms/frame ≈ 9 flaps/sec. Faster reads as chattering rather than speech; slower
 * reads as a stutter. True viseme lip-sync is not reachable with three flat images and
 * no alpha — that needs a layered mouth set or a Live2D rig.
 */
export const FLAP_PERIOD_MS = 110

/** How long the `joyous` reaction holds before decaying back to idle. */
export const REACTION_HOLD_MS = 2600

/**
 * Crossfade duration between sprites. The three images share pose, lighting and
 * framing, so a fade reads as one character changing expression; a hard cut reads as
 * three different pictures. Long enough to be seen, short enough not to lag the flap.
 */
export const CROSSFADE_MS = 150

/**
 * The sprite to display for a phase. Pure — same input, same output, no clock reads.
 *
 * Callers pass elapsed time rather than letting this call `Date.now()`, so tests can
 * step the animation deterministically and the component can drive it from rAF.
 */
export function spriteFor(phase: AvatarPhase): Expression {
  switch (phase.kind) {
    case 'idle':
      return 'idle'

    // Deliberately not the flap: she has not been given anything to say yet. Flapping
    // while waiting on the model makes her look like she is talking over you.
    case 'thinking':
      return 'idle'

    case 'streaming':
      return Math.floor(phase.elapsedMs / FLAP_PERIOD_MS) % 2 === 0 ? 'talking' : 'idle'

    case 'settled':
      return phase.positive && phase.sinceMs < REACTION_HOLD_MS ? 'joyous' : 'idle'
  }
}

/**
 * Heuristic sentiment for the reaction beat — **not** authoritative scoring.
 *
 * This decides one thing: whether Scylar smiles for ~2.6s after answering. A wrong
 * call costs a slightly odd expression, never a wrong answer, so a keyword pass is the
 * right weight of tool. Swap in a real classifier if the reaction ever gates anything
 * that matters.
 */
export function readsPositive(text: string): boolean {
  const t = text.toLowerCase()

  // Checked first: "no problem" and "not a problem" contain "problem", and hedges like
  // "unfortunately" outrank an incidental "thanks" elsewhere in the sentence.
  const negative = [
    'error',
    'failed',
    'failure',
    'sorry',
    'unfortunately',
    "can't",
    'cannot',
    'unable to',
    'went wrong',
    'invalid',
  ]
  if (negative.some((w) => t.includes(w))) return false

  const positive = [
    'great',
    'nice',
    'excellent',
    'perfect',
    'love',
    'glad',
    'happy',
    'awesome',
    'congrat',
    'well done',
    'good news',
    'success',
    ':)',
    'welcome',
  ]
  return positive.some((w) => t.includes(w))
}

/** Preload order — `idle` first because it is the state the page opens in. */
export const PRELOAD_ORDER: readonly Expression[] = ['idle', 'talking', 'joyous'] as const

/** Public path for a sprite at a given width. Mirrors `scripts/scylar-assets.mjs`. */
export function spriteSrc(expression: Expression, width: 512 | 1024 = 512): string {
  return `/scylar/${expression}-${width}.webp`
}
