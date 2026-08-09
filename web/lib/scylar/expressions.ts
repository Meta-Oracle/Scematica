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
 * 180ms ≈ 5.5 flaps/sec, deliberately slower than the 9/sec this started at. At speech
 * rate the two frames blur into a single indistinct mouth; at this rate each open and
 * close is individually visible, which is what makes the flap read as *her* speaking
 * rather than as an image flickering. True viseme lip-sync is not reachable with three
 * flat images and no alpha — that needs a layered mouth set or a Live2D rig.
 */
export const FLAP_PERIOD_MS = 180

/**
 * Fraction of each flap cycle spent open-mouthed.
 *
 * Not 0.5. An even split reads as a metronome; weighting toward open gives the flap the
 * shape of a syllable — a quick close between held vowels — and is the other half of
 * why the slower period reads as pronounced rather than sluggish.
 */
export const FLAP_OPEN_RATIO = 0.58

/** How long the `joyous` reaction holds before decaying back to idle. */
export const REACTION_HOLD_MS = 4200

/**
 * Crossfade for the mouth flap.
 *
 * Must stay well under `FLAP_PERIOD_MS`: a fade longer than the period never finishes
 * before it reverses, leaving both sprites permanently half-lit — a blur, not a flap.
 * 85ms of the 180ms cycle fades, the remainder holds, so each frame is actually seen.
 */
export const FLAP_CROSSFADE_MS = 85

/**
 * Crossfade for a change of mood (idle ↔ joyous).
 *
 * Much slower than the flap on purpose. The three images share pose, lighting and
 * framing, so a long dissolve reads as one face changing its mind; a hard cut reads as
 * three different pictures. This one is not inside a repeating cycle, so it can take
 * the time the flap cannot.
 */
export const EXPRESSION_CROSSFADE_MS = 420

/** How long the body settles into a new pose. Slow enough to be felt as a movement. */
export const PRESENCE_EASE_MS = 620

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

    case 'streaming': {
      const t = (phase.elapsedMs % FLAP_PERIOD_MS) / FLAP_PERIOD_MS
      return t < FLAP_OPEN_RATIO ? 'talking' : 'idle'
    }

    case 'settled':
      return phase.positive && phase.sinceMs < REACTION_HOLD_MS ? 'joyous' : 'idle'
  }
}

/**
 * How the whole portrait carries itself, independent of which sprite is showing.
 *
 * The sprite set can only change her face. Everything else that sells a reaction —
 * leaning in to answer, drawing back to think, the glow lifting when she's pleased —
 * has to come from the container, because there is no second pose to swap to. Splitting
 * it out this way means the flap stays a fast two-frame cycle while the *body* moves on
 * a much slower curve, which is what stops "more pronounced" from becoming "twitchier".
 */
export interface Presence {
  /** Container scale. Small numbers: past ~1.06 the framing visibly crops. */
  scale: number
  /** Vertical offset in CSS px. Negative is up, toward the viewer. */
  lift: number
  /** Rim-glow strength, 0..1. Drives opacity of the violet halo, not a colour. */
  glow: number
}

const PRESENCE: Record<'idle' | 'thinking' | 'streaming' | 'reacting', Presence> = {
  // Resting. Not zero glow — she is powered on, just not addressed.
  idle: { scale: 1, lift: 0, glow: 0.16 },
  // Withdrawn a fraction. Reads as consideration, and gives the lean-in something to
  // move away from: streaming from a neutral pose is half the travel.
  thinking: { scale: 0.985, lift: 3, glow: 0.34 },
  streaming: { scale: 1.028, lift: -5, glow: 0.62 },
  reacting: { scale: 1.055, lift: -9, glow: 1 },
}

/** Pure — the presence for a phase. Same input, same output, no clock reads. */
export function presenceFor(phase: AvatarPhase): Presence {
  switch (phase.kind) {
    case 'idle':
      return PRESENCE.idle
    case 'thinking':
      return PRESENCE.thinking
    case 'streaming':
      return PRESENCE.streaming
    case 'settled':
      // Decays with the sprite, so the body and the face finish reacting together.
      return phase.positive && phase.sinceMs < REACTION_HOLD_MS
        ? PRESENCE.reacting
        : PRESENCE.idle
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
