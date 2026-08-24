// The sigil — geometry and state for Scylar's instrument ring.
//
// The three sprites can only change her face. Everything else she is doing — which
// subsystems answered, how fresh the bot state is, how much of a score was actually
// measured, whether tokens are arriving — has no face to show it on. The ring is where that
// goes: an SVG instrument drawn around the portrait, reading out real telemetry.
//
// ## The rule that shapes the whole file
//
// **An unmeasured gauge must not look like a measured zero.** This is the em-dash rule in
// vector form, and it is the reason `gaugeArc` returns a discriminated result instead of a
// number. A zero-length arc is what you get for Ψ = 0.00 *and* for "nobody measured Ψ", and
// on screen those are the same picture — which would make the ring a prettier version of
// exactly the failure `scema_policy::render` exists to prevent. So an unmeasured gauge draws
// the **full sweep as a dashed ghost** and labels itself `—`, and a measured zero draws
// nothing and labels itself `0.00`. Different shape, different text, no ambiguity.
//
// The second rule follows the console: **coverage is one cell per term, never a proportional
// bar**. A bar renders 2/5 and 4/10 identically, and the denominator is the number that
// matters. `coverageCells` returns one boolean per term; an empty coverage is `∅`.
//
// The third is the theme rule from `scema-tui` and `lib/mesh/view.ts`: **this file names a
// role, never a colour**. Hexes live in CSS. `channelRole` maps a state to a role name and
// is the only thing allowed to make that decision.
//
// Pure — no DOM, no clock, no randomness. `check:scylar` pins every function here, which is
// the point of separating it from the component at all.

import type { AvatarPhase } from './expressions.ts'

// ── geometry ───────────────────────────────────────────────────────────────────

/** The SVG viewBox is square and centred; everything below is in that space. */
export const VIEW = 200
export const CENTER = VIEW / 2

export interface Point {
  x: number
  y: number
}

/**
 * Polar to cartesian, with 0° at twelve o'clock and angles increasing clockwise.
 *
 * SVG's native 0° is at three o'clock and y grows downward, which makes every hand-written
 * arc off by 90° and mirrored. Absorbing that here means no call site ever has to think
 * about it, and the tests read as "0 is the top".
 */
export function polar(radius: number, angleDeg: number, center = CENTER): Point {
  const rad = ((angleDeg - 90) * Math.PI) / 180
  return {
    x: center + radius * Math.cos(rad),
    y: center + radius * Math.sin(rad),
  }
}

/** Round to 3dp so path strings are stable across platforms and diffable in a snapshot. */
function r3(n: number): number {
  return Math.round(n * 1000) / 1000
}

/**
 * An arc path from `startDeg` to `endDeg`, clockwise.
 *
 * Returns `''` for a zero-or-negative sweep rather than a degenerate `A` command — an arc
 * whose endpoints coincide is rendered by SVG as *nothing at all* on some engines and as a
 * full circle on others, and a full circle is the single worst thing a zero gauge could
 * draw.
 */
export function arcPath(radius: number, startDeg: number, endDeg: number, center = CENTER): string {
  const sweep = endDeg - startDeg
  if (sweep <= 0) return ''
  // A sweep of exactly 360 has coincident endpoints; two half-arcs draw it unambiguously.
  if (sweep >= 360) {
    const top = polar(radius, 0, center)
    const bottom = polar(radius, 180, center)
    return (
      `M ${r3(top.x)} ${r3(top.y)} ` +
      `A ${radius} ${radius} 0 0 1 ${r3(bottom.x)} ${r3(bottom.y)} ` +
      `A ${radius} ${radius} 0 0 1 ${r3(top.x)} ${r3(top.y)}`
    )
  }
  const a = polar(radius, startDeg, center)
  const b = polar(radius, endDeg, center)
  const largeArc = sweep > 180 ? 1 : 0
  return `M ${r3(a.x)} ${r3(a.y)} A ${radius} ${radius} 0 ${largeArc} 1 ${r3(b.x)} ${r3(b.y)}`
}

export interface Tick {
  x1: number
  y1: number
  x2: number
  y2: number
  /** Every `majorEvery`-th tick is longer. Purely visual rhythm. */
  major: boolean
}

/** Evenly spaced radial ticks around a full circle. */
export function ticks(count: number, radius: number, length = 5, majorEvery = 5): Tick[] {
  const out: Tick[] = []
  for (let i = 0; i < count; i++) {
    const angle = (360 / count) * i
    const major = i % majorEvery === 0
    const inner = polar(radius - (major ? length * 1.9 : length), angle)
    const outer = polar(radius, angle)
    out.push({ x1: r3(inner.x), y1: r3(inner.y), x2: r3(outer.x), y2: r3(outer.y), major })
  }
  return out
}

// ── gauges ─────────────────────────────────────────────────────────────────────

/**
 * A gauge reading. `measured: false` is a first-class state, not a zero.
 *
 * `ghost` tells the renderer to draw `d` as a dashed unfilled sweep rather than as a value.
 * Carried on the result rather than left for the component to infer from `measured`, because
 * an inference made in a renderer is one a second renderer will make differently.
 */
export interface Gauge {
  /** Path for the value arc. Empty when the value is a measured zero. */
  d: string
  /** Path for the full track behind it. Always drawn. */
  track: string
  measured: boolean
  /** Draw `d` as a dashed ghost — set only when unmeasured. */
  ghost: boolean
  /** The readout beside it. `—` when unmeasured, two decimals when measured. */
  label: string
}

/**
 * Build a gauge over a 0..1 value on the arc from `startDeg` to `endDeg`.
 *
 * `null` means unmeasured and produces the ghost sweep. Values outside 0..1 are clamped —
 * a Ψ of 1.04 from a rounding artefact should not wrap the arc past its own start, which
 * would read as a much *lower* value than it is.
 */
export function gaugeArc(
  value: number | null,
  radius: number,
  startDeg: number,
  endDeg: number,
): Gauge {
  const track = arcPath(radius, startDeg, endDeg)

  if (value === null || !Number.isFinite(value)) {
    return { d: track, track, measured: false, ghost: true, label: '—' }
  }

  const v = Math.max(0, Math.min(1, value))
  return {
    d: arcPath(radius, startDeg, startDeg + (endDeg - startDeg) * v),
    track,
    measured: true,
    ghost: false,
    label: v.toFixed(2),
  }
}

/**
 * Coverage as one cell per term — never a proportional bar.
 *
 * Same rule as the console's `▰▰▰▱▱` meter, and for the same reason: a bar renders 2/5 and
 * 4/10 identically, and the denominator is the number that matters. `null` for an absent
 * coverage, which the renderer draws as `∅` rather than as an empty meter.
 */
export function coverageCells(
  measured: number,
  total: number,
): { cells: boolean[]; label: string } | null {
  if (!Number.isFinite(measured) || !Number.isFinite(total) || total <= 0) return null
  const m = Math.max(0, Math.min(total, Math.floor(measured)))
  const t = Math.floor(total)
  return {
    cells: Array.from({ length: t }, (_, i) => i < m),
    label: `${m}/${t}`,
  }
}

// ── channels ───────────────────────────────────────────────────────────────────

/**
 * A subsystem shown as a node on the ring.
 *
 * `held` and `dark` are deliberately separate: a Ψ HOLD means the channel read perfectly
 * well and its data was withheld, which is a different fact from a channel that did not
 * answer, and the ring is one of the two places an operator would notice the difference
 * without reading a paragraph.
 */
export type ChannelState = 'open' | 'dark' | 'held' | 'simulated'

export interface Channel {
  id: string
  /** Two or three characters. The ring is 200 units across; a word does not fit. */
  label: string
  state: ChannelState
  title: string
}

/**
 * The role a channel state maps to. **The only place this decision is made.**
 *
 * Roles, not colours — same rule as `theme.rs` in the console and `toneFor` in `lib/mesh`.
 * The CSS owns the hexes, so a palette change moves one file and the meaning stays put.
 */
export function channelRole(state: ChannelState): 'live' | 'idle' | 'warn' | 'sim' {
  switch (state) {
    case 'open':
      return 'live'
    case 'dark':
      return 'idle'
    case 'held':
      return 'warn'
    case 'simulated':
      return 'sim'
  }
}

/**
 * Evenly distribute channels around the ring, clockwise from `startDeg`.
 *
 * `startDeg` is not decoration. The vertical axis is spoken for — the Ψ readout sits above
 * the portrait and the trace, coverage meter and status word stack below it — so a node
 * placed at dead top or dead bottom lands on top of a number. `CHANNEL_START` puts four
 * nodes on the diagonals and leaves that column clear.
 */
export function channelPositions(count: number, radius: number, startDeg = 0): Point[] {
  if (count <= 0) return []
  return Array.from({ length: count }, (_, i) => {
    const p = polar(radius, startDeg + (360 / count) * i)
    return { x: r3(p.x), y: r3(p.y) }
  })
}

// ── motion ─────────────────────────────────────────────────────────────────────

/**
 * How the ring moves for a phase.
 *
 * Motion is a *claim* here, not decoration: the ring spins faster while tokens are arriving
 * because tokens really are arriving. Nothing on this ring animates to look busy. An idle
 * page runs the slowest rotation in the set and no pulse at all, which is why a stopped
 * stream is visible from across the room.
 */
export interface Motion {
  /** Seconds for one full rotation of the outer ring. */
  spinSecs: number
  /** Seconds for one counter-rotation of the inner ring. Opposed for parallax. */
  counterSecs: number
  /** Whether the radial pulse runs at all. */
  pulse: boolean
  /** Overall intensity 0..1, driving opacity of the whole instrument layer. */
  intensity: number
}

const MOTION: Record<'idle' | 'thinking' | 'speaking' | 'reacting', Motion> = {
  // Powered on, not addressed. Slow enough to read as breathing rather than as a loading
  // spinner, which is the wrong idea to plant on an idle page.
  idle: { spinSecs: 96, counterSecs: 140, pulse: false, intensity: 0.34 },
  // Waiting on the model: the ring accelerates before a single token exists, which is the
  // only honest thing on screen that says "the request left".
  thinking: { spinSecs: 26, counterSecs: 38, pulse: true, intensity: 0.72 },
  speaking: { spinSecs: 44, counterSecs: 62, pulse: true, intensity: 1 },
  reacting: { spinSecs: 60, counterSecs: 88, pulse: false, intensity: 0.86 },
}

export function motionFor(phase: AvatarPhase): Motion {
  switch (phase.kind) {
    case 'idle':
      return MOTION.idle
    case 'thinking':
      return MOTION.thinking
    case 'streaming':
    case 'voicing':
      return MOTION.speaking
    case 'settled':
      return phase.positive ? MOTION.reacting : MOTION.idle
  }
}

/**
 * The token-rate trace, as a polyline over a fixed window.
 *
 * Honest by construction: it is a function of `samples`, which the terminal fills from real
 * token arrivals. With no samples it returns a **flat line**, not a plausible-looking
 * squiggle — an idle trace that wiggles is a fabricated readout, and this whole system is
 * built on not shipping one of those.
 *
 * `width` and `height` are in viewBox units; the trace is drawn into its own band.
 */
export function tracePoints(
  samples: readonly number[],
  width: number,
  height: number,
  slots = 32,
): string {
  const y0 = height / 2
  if (slots < 2) return ''
  if (samples.length === 0) {
    return `0,${r3(y0)} ${r3(width)},${r3(y0)}`
  }

  // Newest samples on the right; a shorter history pads flat on the left rather than
  // stretching, so a trace that has just started reads as "just started".
  const win = samples.slice(-slots)
  const peak = Math.max(1, ...win)
  const pad = slots - win.length
  const pts: string[] = []
  for (let i = 0; i < slots; i++) {
    const x = (width / (slots - 1)) * i
    const v = i < pad ? 0 : win[i - pad] / peak
    pts.push(`${r3(x)},${r3(y0 - v * (height / 2 - 1))}`)
  }
  return pts.join(' ')
}

// ── the whole readout ──────────────────────────────────────────────────────────

export interface SigilTelemetry {
  phase: AvatarPhase
  /** Ψ from the gate, or null when it was not consulted. */
  psi: number | null
  /** Gate verdict, for the ring's own state label. */
  verdict: 'go' | 'caution' | 'hold' | null
  /** Coverage of the last omni result, when there was one. */
  coverage: { measured: number; total: number } | null
  channels: Channel[]
  /** Token arrivals per animation slot, newest last. Empty when nothing has streamed. */
  trace: readonly number[]
}

export interface SigilView {
  motion: Motion
  psi: Gauge
  coverage: { cells: boolean[]; label: string } | null
  channels: (Channel & Point & { role: ReturnType<typeof channelRole> })[]
  trace: string
  /** Short status word under the ring. Never invented — derived from what is above it. */
  status: string
}

/** Ring radii, exported so the component and the tests agree on one set of numbers. */
export const RADIUS = {
  outer: 94,
  ticks: 88,
  gauge: 78,
  channels: 66,
  inner: 54,
} as const

/** Ψ sweeps the top three quarters, leaving the bottom clear for the readout text. */
export const PSI_ARC = { start: -135, end: 135 } as const

/**
 * Where the channel nodes start, and the layout of the readout stack beneath the portrait.
 *
 * Kept here rather than in the component so the collision rules are stated once and checked
 * once: the bands must not overlap each other, and no channel node may land in the column
 * they occupy. With four channels a 45° start puts every node on a diagonal.
 */
export const CHANNEL_START = 45

export const READOUT = {
  /** Token trace: a band centred on this y, this wide and this tall. */
  trace: { y: 146, w: 80, h: 16 },
  /** Coverage meter, centred horizontally. */
  coverage: { y: 168 },
  /** The status word. */
  status: { y: 192 },
  /** The Ψ figure, above the portrait's eyeline. */
  psi: { y: 38 },
} as const

/**
 * Derive everything the renderer needs. Pure, so a snapshot of this is a snapshot of the
 * ring's meaning without rendering a single element.
 */
export function sigilView(
  t: SigilTelemetry,
  traceBand: { w: number; h: number } = READOUT.trace,
): SigilView {
  const positions = channelPositions(t.channels.length, RADIUS.channels, CHANNEL_START)
  return {
    motion: motionFor(t.phase),
    psi: gaugeArc(t.psi, RADIUS.gauge, PSI_ARC.start, PSI_ARC.end),
    coverage: t.coverage ? coverageCells(t.coverage.measured, t.coverage.total) : null,
    channels: t.channels.map((c, i) => ({
      ...c,
      ...(positions[i] ?? { x: CENTER, y: CENTER }),
      role: channelRole(c.state),
    })),
    trace: tracePoints(t.trace, traceBand.w, traceBand.h),
    status: statusWord(t),
  }
}

/**
 * The word under the ring.
 *
 * Ordered by what overrides what: a HOLD outranks anything the phase is doing, because it is
 * a statement about whether the answer being streamed can be trusted at all. Below that, the
 * phase; and at rest, whether she has any live channel at all — "OFFLINE" on an idle page
 * with every channel dark is the single most useful thing that spot can say.
 */
function statusWord(t: SigilTelemetry): string {
  if (t.verdict === 'hold') return 'HELD'
  switch (t.phase.kind) {
    case 'thinking':
      return 'THINKING'
    case 'streaming':
      return 'SPEAKING'
    case 'voicing':
      return 'VOICING'
    case 'settled':
    case 'idle':
      break
  }
  if (t.verdict === 'caution') return 'CAUTION'
  const anyOpen = t.channels.some((c) => c.state === 'open')
  if (!anyOpen) return t.channels.length ? 'OFFLINE' : 'READY'
  return t.channels.some((c) => c.state === 'simulated') ? 'SIMULATED' : 'READY'
}
