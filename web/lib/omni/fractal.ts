/**
 * The world as a fractal. A port of `scema_nft::fractal`.
 *
 * Rust is authoritative and this must produce the **same bytes** — the same requirement as
 * `nft.ts` against `plate.rs`, and harder to hold, because a recursion amplifies any
 * disagreement. A one-ULP difference at the root is a visibly different tree by the fourth
 * level, so there is no float arithmetic in the growth at all.
 *
 * Four things carry that:
 *
 * 1. **No trigonometry.** Angles are whole degrees through the shared integer sine table in
 *    `nft.ts`.
 * 2. **Integer milliunits.** Lengths and coordinates never leave `i64`-shaped integers.
 * 3. **A 32-bit xorshift**, not 64-bit. JavaScript numbers are doubles; `>>> 0` and
 *    `Math.imul` reproduce u32 wrapping exactly, where u64 would need `BigInt` and give two
 *    implementations somewhere to disagree.
 * 4. **A fixed recursion order**, with the RNG consumed as encountered. Draw the children in
 *    a different sequence and every subsequent draw takes a different number — a different
 *    tree from the same world.
 *
 * The only floats are in `growthOf`, where they are immediately rounded: `Math.round` and
 * Rust's `f64::round` agree on positive values, and every value there is a non-negative
 * fraction of a small positive range.
 */

import {
  PALETTE,
  VIEW,
  divRound,
  esc,
  fmt,
  shortDigest,
  sinMicro,
  truncate,
  type Role,
} from './nft.ts'
import { renderPng, type Anchor, type Prim } from './raster.ts'
import type { WorldState } from './types.ts'

const UNIT = 1000

/** Where the trunk starts, in milliunits. */
const ROOT = { x: (VIEW * UNIT) / 2, y: 492 * UNIT }
const TRUNK = 108 * UNIT
const MIN_LEN = 4 * UNIT
const MAX_DEPTH = 9
const MAX_SEGMENTS = 6000

// ── the sine table lives in nft.ts; step() is the one primitive this needs ────

interface Pt {
  x: number
  y: number
}

// The sine table lives in `nft.ts` and is imported rather than duplicated. Two copies of a
// table are two things that can drift, and that table is the reason this port can be
// byte-exact at all.
function cosMicro(deg: number): number {
  return sinMicro(deg + 90)
}

function step(origin: Pt, lenMu: number, deg: number): Pt {
  const a = deg - 90
  return {
    x: origin.x + divRound(lenMu * cosMicro(a), 1_000_000),
    y: origin.y + divRound(lenMu * sinMicro(a), 1_000_000),
  }
}

function pt(p: Pt): string {
  return `${fmt(p.x)} ${fmt(p.y)}`
}

// ── rng ───────────────────────────────────────────────────────────────────────

/**
 * 32-bit xorshift seeded from the world's commitment.
 *
 * `>>> 0` after every step, because JavaScript's bitwise operators work on *signed* int32
 * and Rust's are on `u32`. Without it the state goes negative and diverges immediately.
 */
export class Rng {
  private s: number

  constructor(hex: string) {
    let seed = 0
    for (const c of Array.from(hex).slice(0, 8)) {
      const d = parseInt(c, 16)
      seed = (Math.imul(seed, 16) + (Number.isNaN(d) ? 0 : d)) >>> 0
    }
    // xorshift has a fixed point at zero and would return zero forever, collapsing every
    // world onto one form.
    this.s = seed === 0 ? 0x9e3779b9 : seed
  }

  next(): number {
    let x = this.s
    x = (x ^ (x << 13)) >>> 0
    x = (x ^ (x >>> 17)) >>> 0
    x = (x ^ (x << 5)) >>> 0
    this.s = x
    return x
  }

  below(n: number): number {
    return n === 0 ? 0 : this.next() % n
  }

  jitter(spread: number): number {
    if (spread <= 0) return 0
    return this.below(spread * 2 + 1) - spread
  }
}

// ── growth ────────────────────────────────────────────────────────────────────

export interface Growth {
  depth: number
  arity: number
  spread: number
  decay: number
  cuts: number
  unbounded: boolean
}

function extentFraction(w: WorldState): number | null {
  const t = w.extent.total
  if (t === null || t === undefined) return null
  if (t === 0) return 1
  return Math.min(1, w.extent.observed / t)
}

function legibility(w: WorldState): number {
  if (w.objects.length === 0) return 0
  return w.objects.filter((o) => o.provenance.kind === 'live').length / w.objects.length
}

/** Every number here traces to something counted. Mirrors `growth_of`. */
export function growthOf(w: WorldState): Growth {
  const signals = w.signals.length
  const risks = w.signals.filter((s) => s.polarity === 'risk').length
  const opportunities = w.signals.filter((s) => s.polarity === 'opportunity').length

  const f = extentFraction(w)
  const depth =
    f === null ? MAX_DEPTH : Math.min(MAX_DEPTH, Math.max(3, Math.round(3 + f * (MAX_DEPTH - 3))))

  const arity = signals >= 12 ? 4 : signals >= 5 ? 3 : 2

  const total = Math.max(1, risks + opportunities)
  const riskShare = divRound(risks * 100, total)
  const spread = 18 + divRound(riskShare * 22, 100)

  const decay = 66 + Math.round(legibility(w) * 10)

  return {
    depth,
    arity,
    spread,
    decay,
    // A count, never a rate. A per-node probability compounds down the recursion and makes
    // the form claim more ignorance than the observer reported.
    cuts: w.blind_spots.length,
    unbounded: f === null,
  }
}

/**
 * The single level cuts land on, and its size.
 *
 * One level, so a cut can never sit inside another cut's subtree and go unreached. The
 * shallowest level with room to spare wins — shallow is more visible — but the level must
 * hold at least three nodes per cut, or cutting annihilates the form.
 */
export function cutLevel(g: Growth): [number, number] {
  const SURVIVAL = 3
  const deepest = Math.max(1, g.depth - 1)
  for (let l = 1; l <= deepest; l += 1) {
    const size = Math.pow(g.arity, Math.min(l, 20))
    if (size >= g.cuts * SURVIVAL) return [l, size]
  }
  return [deepest, Math.pow(g.arity, Math.min(deepest, 20))]
}

/** How many limbs are actually cut, and whether that is fewer than were reported. */
export function plannedCuts(g: Growth): [number, boolean] {
  const [, size] = cutLevel(g)
  const room = Math.max(1, Math.floor(size / 3))
  const n = Math.min(g.cuts, room)
  return [n, n < g.cuts]
}

function cutSet(g: Growth, rng: Rng): number[] {
  const [, nodes] = cutLevel(g)
  const [want] = plannedCuts(g)
  const chosen: number[] = []
  let guard = 0
  while (chosen.length < want && guard < want * 16 + 64) {
    const pick = rng.below(nodes)
    if (!chosen.includes(pick)) chosen.push(pick)
    guard += 1
  }
  chosen.sort((a, b) => a - b)
  return chosen
}

// ── drawing ───────────────────────────────────────────────────────────────────

/**
 * A text element, and the primitive that mirrors it.
 *
 * Returned as a pair so a caller cannot emit one without the other — a legend that appeared
 * in the SVG and not in the PNG would leave the raster looking like a picture rather than a
 * reading. Same reason as `text_pair` in `fractal.rs`.
 */
function textPair(
  x: number,
  y: number,
  size: number,
  role: Role,
  anchor: Anchor,
  body: string,
): [string, Prim] {
  const el =
    `<text x="${x}" y="${y}" ` +
    `font-family="ui-monospace,SFMono-Regular,Menlo,Consolas,monospace" ` +
    `font-size="${size}" fill="${PALETTE[role]}" text-anchor="${anchor}">${esc(body)}</text>`
  return [el, { kind: 'text', at: [x * UNIT, y * UNIT], size, rgb: PALETTE[role], anchor, body }]
}

/** Both renderings' inputs, from one traversal. */
export interface Scene {
  svg: string
  prims: Prim[]
}

/**
 * Draw the growth once, returning both renderings' inputs.
 *
 * The primitive list is accumulated *alongside* the SVG string rather than replacing it, so
 * the raster walks the identical growth and the SVG output does not change by a byte — which
 * the parity fixture would catch immediately if it did.
 */
export function scene(w: WorldState, digestHex: string): Scene {
  const g = growthOf(w)
  const rng = new Rng(digestHex)
  const cuts = cutSet(g, rng)
  const [cutAt] = cutLevel(g)

  let body = ''
  let segments = 0
  let severed = 0
  const tips: Pt[] = []
  const prims: Prim[] = []

  const line = (a: Pt, b: Pt, widthMu: number, role: Role, dashed: boolean) => {
    if (segments >= MAX_SEGMENTS) return
    segments += 1
    prims.push({
      kind: 'line',
      a: [a.x, a.y],
      b: [b.x, b.y],
      widthMu: Math.max(widthMu, 400),
      rgb: PALETTE[role],
      dashed,
    })
    const dash = dashed ? ' stroke-dasharray="3 4"' : ''
    body +=
      `<path d="M ${pt(a)} L ${pt(b)}" stroke="${PALETTE[role]}" ` +
      `stroke-width="${fmt(Math.max(widthMu, 400))}" stroke-linecap="round" fill="none"${dash}/>`
  }

  const branch = (from: Pt, angle: number, len: number, depth: number, pos: number) => {
    if (depth === 0 || len < MIN_LEN || segments >= MAX_SEGMENTS) {
      tips.push(from)
      return
    }

    // Identity for cutting is position within the level, threaded down — not a visit
    // counter, which shifts when a cut removes a subtree.
    const level = g.depth - depth
    const me = level === cutAt ? pos : null

    if (me !== null && cuts.includes(me)) {
      const stub = step(from, Math.trunc(len / 3), angle)
      line(from, stub, depth * 420, 'absent', true)
      severed += 1
      return
    }

    const jitter = rng.jitter(4)
    const to = step(from, len, angle + jitter)

    const frontier = g.unbounded && depth === 1
    line(from, to, depth * 340, frontier ? 'stale' : 'measured', frontier)

    const arity = g.arity
    const nextLen = divRound(len * g.decay, 100)
    for (let i = 0; i < arity; i += 1) {
      const offset = arity === 1 ? 0 : -g.spread + divRound(2 * g.spread * i, arity - 1)
      branch(to, angle + offset, nextLen, depth - 1, pos * arity + i)
    }
  }

  branch(ROOT, 0, TRUNK, g.depth, 0)

  // Marks, drawn after the branches so they sit on top. Shape carries polarity; hollow means
  // the magnitude was estimated rather than counted.
  const n = Math.max(1, w.signals.length)
  for (let i = 0; i < w.signals.length; i += 1) {
    const sig = w.signals[i]
    const p = tips[Math.trunc((i * tips.length) / n)]
    if (!p) continue
    const role: Role = sig.polarity === 'risk' ? 'risk' : 'opportunity'
    const fill = sig.measured ? PALETTE[role] : 'none'
    if (sig.polarity === 'opportunity') {
      prims.push({
        kind: 'disc',
        c: [p.x, p.y],
        rMu: 2600,
        rgb: PALETTE[role],
        filled: sig.measured,
      })
      body += `<circle cx="${fmt(p.x)}" cy="${fmt(p.y)}" r="2.6" fill="${fill}" stroke="${PALETTE[role]}" stroke-width="1.2"/>`
    } else {
      const a = { x: p.x, y: p.y - 3 * UNIT }
      const b = { x: p.x - 2600, y: p.y + 1800 }
      const c = { x: p.x + 2600, y: p.y + 1800 }
      prims.push({
        kind: 'tri',
        a: [a.x, a.y],
        b: [b.x, b.y],
        c: [c.x, c.y],
        rgb: PALETTE[role],
        filled: sig.measured,
      })
      body += `<path d="M ${pt(a)} L ${pt(b)} L ${pt(c)} Z" fill="${fill}" stroke="${PALETTE[role]}" stroke-width="1.2"/>`
    }
  }

  const aria = `Scematica Omni world growth for ${w.entity.label} (${w.entity.kind}), observed by ${w.observer}`
  let s =
    `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 ${VIEW} ${VIEW}" ` +
    `width="${VIEW}" height="${VIEW}" role="img" aria-label="${esc(aria)}">`
  s += `<rect x="0" y="0" width="${VIEW}" height="${VIEW}" fill="${PALETTE.ground}"/>`
  s +=
    `<rect x="8" y="8" width="${VIEW - 16}" height="${VIEW - 16}" ` +
    `fill="none" stroke="${PALETTE.frame}" stroke-width="1"/>`

  // The frame, as primitives too. It is chrome rather than data, but the claim this module
  // makes is that the two renderings depict the same thing — and a border present in one and
  // absent in the other is a small, avoidable way for that to be false.
  const lo = 8 * UNIT
  const hi = (VIEW - 8) * UNIT
  const frame: [[number, number], [number, number]][] = [
    [
      [lo, lo],
      [hi, lo],
    ],
    [
      [hi, lo],
      [hi, hi],
    ],
    [
      [hi, hi],
      [lo, hi],
    ],
    [
      [lo, hi],
      [lo, lo],
    ],
  ]
  for (const [a, b] of frame) {
    prims.push({ kind: 'line', a, b, widthMu: 1000, rgb: PALETTE.frame, dashed: false })
  }

  s += body

  const t = w.extent.total
  const extent =
    t === null || t === undefined
      ? `EXTENT ${w.extent.observed} · UNBOUNDED`
      : `EXTENT ${w.extent.observed}/${t}`

  const [, capped] = plannedCuts(g)
  const blind =
    w.blind_spots.length === 0
      ? 'NO BLIND SPOTS'
      : capped
        ? `${w.blind_spots.length} BLIND SPOT(S) · ${severed} LIMB(S) CUT (CAPPED)`
        : `${w.blind_spots.length} BLIND SPOT(S) · ${severed} LIMB(S) CUT`

  const measured = w.signals.filter((x) => x.measured).length
  const coverage = w.signals.length === 0 ? 'COVERAGE ∅' : `COVERAGE ${measured}/${w.signals.length}`

  const legend: [number, number, number, Role, Anchor, string][] = [
    [24, 34, 17, 'heading', 'start', truncate(w.entity.label, 30)],
    [24, 52, 10, 'label', 'start', `${w.entity.kind} · ${w.domain} · ${truncate(w.observer, 28)}`],
    [24, 460, 10, 'label', 'start', extent],
    [VIEW - 24, 460, 10, w.blind_spots.length === 0 ? 'label' : 'absent', 'end', blind],
    [24, 478, 10, 'label', 'start', coverage],
    [
      VIEW - 24,
      478,
      10,
      'label',
      'end',
      `depth ${g.depth} · arity ${g.arity} · spread ${g.spread}°`,
    ],
    [24, 496, 10, 'claim', 'start', `world ${shortDigest(digestHex)}`],
  ]
  for (const [x, y, size, role, anchor, bodyText] of legend) {
    const [el, prim] = textPair(x, y, size, role, anchor, bodyText)
    s += el
    prims.push(prim)
  }

  return { svg: s + '</svg>', prims }
}

/** Draw a world as a fractal growth. `digestHex` seeds the form and is printed on it. */
export function renderFractal(w: WorldState, digestHex: string): string {
  return scene(w, digestHex).svg
}

/**
 * The same growth as a PNG.
 *
 * Rasterised from the identical primitive list the SVG is built from, so the two cannot
 * depict different trees — and byte-identical to what `scema nft --png` writes, which is what
 * makes the image a derivative of the record rather than of whichever runtime drew it.
 */
export function renderFractalPng(
  w: WorldState,
  digestHex: string,
  size: number,
): Uint8Array<ArrayBuffer> {
  return renderPng(scene(w, digestHex).prims, VIEW, size, PALETTE.ground, digestHex)
}
