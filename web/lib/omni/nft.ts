/**
 * The world plate, in the browser. A port of the Rust crate `scema-nft`.
 *
 * Rust is authoritative and this file is a copy, with the same status as
 * `lib/omni/canonical.ts` against `canonical.rs` — and a harder requirement than the render
 * rule in `view.ts`. There, three implementations share a *rule* and each is tested
 * separately. Here the two implementations must produce **the same bytes**, because the
 * plate is derived from a decision record and an image that depends on which runtime drew
 * it is not a derivative of anything. `npm run check:omni` compares this file's output
 * against `scematica-omni/crates/scema-nft/fixtures/parity-plate.svg`, which carries Rust's
 * answer, and fails on one differing character.
 *
 * ## The four things that make byte-parity possible
 *
 * Every one of these is a place where the obvious implementation silently diverges:
 *
 * 1. **No trigonometry.** `Math.sin` and Rust's `f64::sin` are not required to agree in the
 *    last place, and a one-ULP difference survives rounding whenever it straddles a tie.
 *    Both sides index the same integer sine table at whole degrees.
 * 2. **No decimal formatting of floats.** `toFixed` and Rust's `{:.3}` break ties
 *    differently. Coordinates are integers in thousandths of a unit and are formatted by
 *    integer arithmetic.
 * 3. **Rounding is half away from zero, spelled out.** `Math.round` rounds half toward
 *    positive infinity, which disagrees with Rust's `f64::round` on every negative tie.
 * 4. **Text is measured in code points, and base64 encodes UTF-8 bytes.** `length` and
 *    `slice` count UTF-16 units, and `btoa` mangles anything above U+00FF — either would
 *    make a label with an accent in it render differently here than in Rust.
 *
 * Multiplication, addition, subtraction, division and `sqrt` are correctly rounded by
 * IEEE-754 and are safe to share. `exp`, `log`, `pow` and the trigonometric family are not.
 *
 * ## What this file does not do
 *
 * It does not score the world, and there is no rarity, tier or rank anywhere in the
 * metadata. Every quantity drawn is one an observer reported; a rank invented here would be
 * a number of the right shape with nothing behind it, laundered through a signed artefact.
 * `check:omni` asserts the absence.
 */

import { canonicalBytes, fieldBytes, parseCanonical, toHex } from './canonical.ts'
import type { Sha256 } from './verify.ts'
import type { Provenance, Signal, WorldState } from './types.ts'

// ── palette ───────────────────────────────────────────────────────────────────
//
// The one place in this file with a colour in it, mirroring `scema-nft/src/palette.rs`,
// which is itself a port of `scema-tui/src/theme.rs`. Drawing code names a role.

export const PALETTE = {
  ground: '#08060f',
  field: '#0f0b1a',
  chrome: '#2a1f45',
  frame: '#6d40c4',
  body: '#e6e0f5',
  label: '#8a81a8',
  ghost: '#544c6c',
  measured: '#a96bff',
  heading: '#cba6ff',
  claim: '#7dd3fc',
  risk: '#ff7b9c',
  opportunity: '#86e5c0',
  stale: '#f2b15c',
  /**
   * A counted absence — an `absent` object, a reported blind spot.
   *
   * Lighter than `ghost`, and the one place this palette diverges from `scema-tui`, which
   * maps both to the same ink. Right in a terminal, where an absence is the word `ABSENT`
   * and a dim word is still a word; wrong here, where a 3px dashed arc in `ghost` against
   * this ground is not recessive but invisible — and an invisible segment in a composition
   * ring silently shrinks the denominator.
   *
   * They are also not the same kind of claim. `ghost` stands in for something nobody
   * measured; an absent object was *counted*. Somebody looked, failed, and recorded the
   * failure, which is a measurement about ignorance.
   */
  absent: '#6f6690',
} as const

export type Role = keyof typeof PALETTE

// ── geometry ──────────────────────────────────────────────────────────────────

export const VIEW = 512
const UNIT = 1000
const CENTER = (VIEW * UNIT) / 2
const MICRO = 1_000_000

/** `sin(d°) * 1e6` for `d` in `0..=90`. Identical to `SIN_MICRO` in `geom.rs`. */
const SIN_MICRO = [
  0, 17452, 34899, 52336, 69756, 87156, 104528, 121869, 139173, 156434, 173648, 190809,
  207912, 224951, 241922, 258819, 275637, 292372, 309017, 325568, 342020, 358368, 374607,
  390731, 406737, 422618, 438371, 453990, 469472, 484810, 500000, 515038, 529919, 544639,
  559193, 573576, 587785, 601815, 615661, 629320, 642788, 656059, 669131, 681998, 694658,
  707107, 719340, 731354, 743145, 754710, 766044, 777146, 788011, 798636, 809017, 819152,
  829038, 838671, 848048, 857167, 866025, 874620, 882948, 891007, 898794, 906308, 913545,
  920505, 927184, 933580, 939693, 945519, 951057, 956305, 961262, 965926, 970296, 974370,
  978148, 981627, 984808, 987688, 990268, 992546, 994522, 996195, 997564, 998630, 999391,
  999848, 1_000_000,
]

export function sinMicro(deg: number): number {
  const d = ((deg % 360) + 360) % 360
  if (d <= 90) return SIN_MICRO[d]
  if (d <= 180) return SIN_MICRO[180 - d]
  if (d <= 270) return -SIN_MICRO[d - 180]
  return -SIN_MICRO[360 - d]
}

export function cosMicro(deg: number): number {
  return sinMicro(deg + 90)
}

/**
 * Integer divide, rounding half away from zero.
 *
 * Written out rather than reaching for `Math.round`, which rounds half toward positive
 * infinity and therefore disagrees with `div_round` in `geom.rs` on every negative tie.
 */
export function divRound(num: number, den: number): number {
  const half = Math.trunc(den / 2)
  return num >= 0 ? Math.trunc((num + half) / den) : -Math.trunc((-num + half) / den)
}

export interface Pt {
  x: number
  y: number
}

/** Polar to cartesian, 0° at twelve o'clock, angles clockwise. */
export function polar(radiusMu: number, deg: number): Pt {
  const a = deg - 90
  return {
    x: CENTER + divRound(radiusMu * cosMicro(a), MICRO),
    y: CENTER + divRound(radiusMu * sinMicro(a), MICRO),
  }
}

/** A fraction in `[0,1]` as a span of milliunits. `NaN` is zero; the infinities clamp. */
export function scale(fraction: number, spanMu: number): number {
  if (Number.isNaN(fraction)) return 0
  const f = Math.min(1, Math.max(0, fraction))
  const v = f * spanMu
  return v >= 0 ? Math.floor(v + 0.5) : -Math.floor(-v + 0.5)
}

/** Milliunits as SVG text: `256500` becomes `256.5`, `256000` becomes `256`. */
export function fmt(mu: number): string {
  const neg = mu < 0
  const a = Math.abs(mu)
  const whole = Math.trunc(a / UNIT)
  const frac = a % UNIT
  const sign = neg && (whole !== 0 || frac !== 0) ? '-' : ''
  if (frac === 0) return `${sign}${whole}`
  let f = String(frac).padStart(3, '0')
  while (f.endsWith('0')) f = f.slice(0, -1)
  return `${sign}${whole}.${f}`
}

function pt(p: Pt): string {
  return `${fmt(p.x)} ${fmt(p.y)}`
}

/**
 * A clockwise arc at one radius.
 *
 * Empty for a sweep of zero or less. A degenerate `A` renders as nothing on some engines
 * and as a **full circle** on others, and a full circle is the worst thing a zero gauge
 * could draw — it is the picture of total coverage.
 */
export function arcPath(radiusMu: number, startDeg: number, endDeg: number): string {
  const sweep = endDeg - startDeg
  if (sweep <= 0) return ''
  const r = fmt(radiusMu)
  if (sweep >= 360) {
    const a = polar(radiusMu, 0)
    const b = polar(radiusMu, 180)
    return `M ${pt(a)} A ${r} ${r} 0 0 1 ${pt(b)} A ${r} ${r} 0 0 1 ${pt(a)}`
  }
  const a = polar(radiusMu, startDeg)
  const b = polar(radiusMu, endDeg)
  const large = sweep > 180 ? 1 : 0
  return `M ${pt(a)} A ${r} ${r} 0 ${large} 1 ${pt(b)}`
}

function spokePath(innerMu: number, outerMu: number, deg: number): string {
  return `M ${pt(polar(innerMu, deg))} L ${pt(polar(outerMu, deg))}`
}

// ── layout, mirroring plate.rs ────────────────────────────────────────────────

const R_EXTENT = 180 * UNIT
const R_NOTCH_IN = 168 * UNIT
const R_NOTCH_OUT = 192 * UNIT
const R_SPOKE_IN = 100 * UNIT
const R_SPOKE_MAX = 158 * UNIT
const R_PROVENANCE = 88 * UNIT
const R_LEGIBILITY_MAX = 58 * UNIT

const Y_TITLE = 34 * UNIT
const Y_SUBTITLE = 52 * UNIT
const Y_FOOT_1 = 460 * UNIT
const Y_FOOT_2 = 478 * UNIT
const Y_FOOT_3 = 496 * UNIT
const X_MARGIN = 24 * UNIT

/**
 * Most signals drawn — as spokes and as footer cells. **One cap for both**, so a single
 * disclosure covers the whole plate; two caps would mean a reader who counted spokes and
 * counted cells got different answers from the same picture.
 *
 * Whenever it bites, the footer says so. Truncating silently would emit a wrong count,
 * which is the one thing this workspace does not do.
 */
const MAX_SIGNALS = 32
const MAX_NOTCHES = 32

// ── text primitives ───────────────────────────────────────────────────────────

/** XML-escape. All five, `&` first, and control characters replaced rather than emitted. */
export function esc(s: string): string {
  let out = ''
  for (const c of s) {
    if (c === '&') out += '&amp;'
    else if (c === '<') out += '&lt;'
    else if (c === '>') out += '&gt;'
    else if (c === '"') out += '&quot;'
    else if (c === "'") out += '&apos;'
    else if (c.codePointAt(0)! < 0x20 && c !== '\t' && c !== '\n' && c !== '\r') out += ' '
    else out += c
  }
  return out
}

/**
 * Truncate to `n` **code points**.
 *
 * `Array.from` rather than `slice`: JavaScript's string indices count UTF-16 units, so an
 * emoji is two of them and a label containing one would truncate at a different place than
 * it does in Rust — a one-byte divergence in the finished SVG.
 */
export function truncate(s: string, n: number): string {
  const chars = Array.from(s)
  if (chars.length <= n) return s
  return chars.slice(0, Math.max(0, n - 1)).join('') + '…'
}

/** Two decimal places by integer arithmetic, matching `fixed2` in `plate.rs`. */
export function fixed2(v: number): string {
  if (Number.isNaN(v)) return '—'
  const scaled = v * 100
  const n = scaled >= 0 ? Math.floor(scaled + 0.5) : -Math.floor(-scaled + 0.5)
  const neg = n < 0
  const a = Math.abs(n)
  return `${neg ? '-' : ''}${Math.trunc(a / 100)}.${String(a % 100).padStart(2, '0')}`
}

/** Twelve hex characters, grouped. An index into the record, never a substitute for verify. */
export function shortDigest(hex: string): string {
  const c = Array.from(hex).slice(0, 12)
  if (c.length < 12) return c.join('')
  return `${c.slice(0, 4).join('')}·${c.slice(4, 8).join('')}·${c.slice(8, 12).join('')}`
}

function text(
  x: number,
  y: number,
  size: number,
  role: Role,
  anchor: string,
  body: string,
): string {
  return (
    `<text x="${fmt(x)}" y="${fmt(y)}" ` +
    `font-family="ui-monospace,SFMono-Regular,Menlo,Consolas,monospace" ` +
    `font-size="${size}" fill="${PALETTE[role]}" text-anchor="${anchor}">${esc(body)}</text>`
  )
}

// ── world helpers ─────────────────────────────────────────────────────────────

function provenanceKind(p: Provenance): string {
  return p.kind
}

function counts(w: WorldState): [number, number, number, number] {
  let live = 0
  let stale = 0
  let absent = 0
  let simulated = 0
  for (const o of w.objects) {
    const k = provenanceKind(o.provenance)
    if (k === 'live') live += 1
    else if (k === 'stale') stale += 1
    else if (k === 'absent') absent += 1
    else if (k === 'simulated') simulated += 1
  }
  return [live, stale, absent, simulated]
}

/**
 * The share of observed objects that may be acted on.
 *
 * Mirrors `WorldState::legibility`, including that an empty world scores `0.0` rather than
 * `1.0` — and including that this number therefore cannot distinguish an empty world from
 * an illegible one. The picture does that, in `legibilityCore`.
 */
export function legibility(w: WorldState): number {
  if (w.objects.length === 0) return 0
  const live = w.objects.filter((o) => provenanceKind(o.provenance) === 'live').length
  return live / w.objects.length
}

function extentFraction(w: WorldState): number | null {
  const t = w.extent.total
  if (t === null || t === undefined) return null
  if (t === 0) return 1
  return Math.min(1, w.extent.observed / t)
}

// ── the plate ─────────────────────────────────────────────────────────────────

/** Draw a world. `digestHex` is rendered verbatim and never recomputed here. */
export function renderSvg(w: WorldState, digestHex: string): string {
  let s = ''

  const aria = `Scematica Omni world plate for ${w.entity.label} (${w.entity.kind}), observed by ${w.observer}`
  s +=
    `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 ${VIEW} ${VIEW}" ` +
    `width="${VIEW}" height="${VIEW}" role="img" aria-label="${esc(aria)}">`

  s += `<rect x="0" y="0" width="${VIEW}" height="${VIEW}" fill="${PALETTE.ground}"/>`
  s +=
    `<rect x="8" y="8" width="${VIEW - 16}" height="${VIEW - 16}" ` +
    `fill="none" stroke="${PALETTE.frame}" stroke-width="1"/>`

  s += header(w)
  s += extentRing(w)
  s += blindSpotNotches(w)
  s += signalSpokes(w)
  s += provenanceRing(w)
  s += legibilityCore(w)
  s += footer(w, digestHex)

  return s + '</svg>'
}

function header(w: WorldState): string {
  return (
    text(X_MARGIN, Y_TITLE, 18, 'heading', 'start', truncate(w.entity.label, 30)) +
    text(
      X_MARGIN,
      Y_SUBTITLE,
      10,
      'label',
      'start',
      `${w.entity.kind} · ${w.domain} · ${truncate(w.observer, 28)}`,
    )
  )
}

/**
 * `extent.total === null` is not a zero and not a full ring: the observer does not know the
 * denominator. Full sweep, dashed, and the footer says UNBOUNDED. A solid ring there would
 * claim total coverage, the exact inverse of what the producer said.
 */
function extentRing(w: WorldState): string {
  const c = fmt(CENTER)
  const r = fmt(R_EXTENT)
  let s = `<circle cx="${c}" cy="${c}" r="${r}" fill="none" stroke="${PALETTE.chrome}" stroke-width="5"/>`

  const f = extentFraction(w)
  if (f === null) {
    s +=
      `<circle cx="${c}" cy="${c}" r="${r}" fill="none" stroke="${PALETTE.stale}" ` +
      `stroke-width="5" stroke-dasharray="6 8" stroke-linecap="butt"/>`
  } else {
    const d = arcPath(R_EXTENT, 0, scale(f, 360))
    if (d !== '') {
      s += `<path d="${d}" fill="none" stroke="${PALETTE.measured}" stroke-width="5" stroke-linecap="butt"/>`
    }
  }
  return s
}

/** One notch per blind spot, cut through the extent ring. Ignorance is a hole. */
function blindSpotNotches(w: WorldState): string {
  if (w.blind_spots.length === 0) return ''
  const shown = Math.min(w.blind_spots.length, MAX_NOTCHES)
  let s = ''
  for (let i = 0; i < shown; i += 1) {
    const deg = divRound(360 * i, shown)
    s += `<path d="${spokePath(R_NOTCH_IN, R_NOTCH_OUT, deg)}" stroke="${PALETTE.absent}" stroke-width="3" stroke-dasharray="3 3"/>`
  }
  return s
}

/**
 * One spoke per signal. Polarity is a shape (triangle / disc) and `measured` is a shape too
 * (solid and filled / dashed and hollow), so an estimate never draws as a count.
 */
function signalSpokes(w: WorldState): string {
  if (w.signals.length === 0) return ''
  const shown = Math.min(w.signals.length, MAX_SIGNALS)
  let s = ''
  for (let i = 0; i < shown; i += 1) {
    const sig: Signal = w.signals[i]
    const deg = divRound(360 * i, shown)
    const outer = R_SPOKE_IN + scale(sig.magnitude, R_SPOKE_MAX - R_SPOKE_IN)
    const role: Role = sig.polarity === 'risk' ? 'risk' : 'opportunity'
    const dash = sig.measured ? '' : ' stroke-dasharray="4 3"'

    s += `<path d="${spokePath(R_SPOKE_IN, outer, deg)}" stroke="${PALETTE[role]}" stroke-width="2"${dash}/>`

    const fill = sig.measured ? PALETTE[role] : 'none'
    const cap = polar(outer, deg)
    if (sig.polarity === 'opportunity') {
      s += `<circle cx="${fmt(cap.x)}" cy="${fmt(cap.y)}" r="3.5" fill="${fill}" stroke="${PALETTE[role]}" stroke-width="1.5"/>`
    } else {
      const a = polar(outer + 5 * UNIT, deg)
      const b = polar(outer - 2 * UNIT, deg - 2)
      const c = polar(outer - 2 * UNIT, deg + 2)
      s += `<path d="M ${pt(a)} L ${pt(b)} L ${pt(c)} Z" fill="${fill}" stroke="${PALETTE[role]}" stroke-width="1.5"/>`
    }
  }
  return s
}

/** `absent` and `simulated` are dashed: nothing was read in those arcs, or nothing was real. */
function provenanceRing(w: WorldState): string {
  if (w.objects.length === 0) return ''
  const [live, stale, absent, simulated] = counts(w)
  const total = w.objects.length
  let s = ''
  let at = 0

  const segments: Array<[number, Role, boolean]> = [
    [live, 'opportunity', false],
    [stale, 'stale', false],
    [simulated, 'claim', true],
    [absent, 'absent', true],
  ]
  for (const [count, role, dashed] of segments) {
    if (count === 0) continue
    const sweep = divRound(360 * count, total)
    const d = arcPath(R_PROVENANCE, at, at + sweep)
    if (d !== '') {
      const dash = dashed ? ' stroke-dasharray="4 4"' : ''
      s += `<path d="${d}" fill="none" stroke="${PALETTE[role]}" stroke-width="3"${dash}/>`
    }
    at += sweep
  }
  return s
}

/**
 * The crate's central distinction, at its sharpest.
 *
 * `legibility` returns `0` for two different worlds — one where objects were observed and
 * none are actionable, and one where there were no objects at all. The number cannot tell
 * them apart, so the picture must: nothing-to-read draws a dashed ghost outline and `∅`, a
 * measured zero draws no disc at all and prints `0.00`.
 */
function legibilityCore(w: WorldState): string {
  const c = fmt(CENTER)
  if (w.objects.length === 0) {
    return (
      `<circle cx="${c}" cy="${c}" r="${fmt(R_LEGIBILITY_MAX)}" fill="none" ` +
      `stroke="${PALETTE.ghost}" stroke-width="2" stroke-dasharray="5 6"/>` +
      text(CENTER, CENTER + 7 * UNIT, 22, 'ghost', 'middle', '∅')
    )
  }

  const f = legibility(w)
  const r = scale(f, R_LEGIBILITY_MAX)
  let s = ''
  if (r > 0) {
    s +=
      `<circle cx="${c}" cy="${c}" r="${fmt(r)}" fill="${PALETTE.measured}" ` +
      `fill-opacity="0.18" stroke="${PALETTE.measured}" stroke-width="1.5"/>`
  }
  return s + text(CENTER, CENTER + 5 * UNIT, 16, 'body', 'middle', fixed2(f))
}

function footer(w: WorldState, digestHex: string): string {
  const t = w.extent.total
  const extent =
    t === null || t === undefined
      ? `EXTENT ${w.extent.observed} · UNBOUNDED`
      : `EXTENT ${w.extent.observed}/${t}`
  const blind =
    w.blind_spots.length === 0
      ? 'NO BLIND SPOTS'
      : w.blind_spots.length > MAX_NOTCHES
        ? `${w.blind_spots.length} BLIND SPOT(S) · ${MAX_NOTCHES} DRAWN`
        : `${w.blind_spots.length} BLIND SPOT(S)`

  let s = text(X_MARGIN, Y_FOOT_1, 10, 'label', 'start', extent)
  s += text(
    VIEW * UNIT - X_MARGIN,
    Y_FOOT_1,
    10,
    w.blind_spots.length === 0 ? 'label' : 'absent',
    'end',
    blind,
  )
  s += coverageCells(w)
  s += text(X_MARGIN, Y_FOOT_3, 10, 'claim', 'start', `world ${shortDigest(digestHex)}`)

  const [live, stale, absent, simulated] = counts(w)
  s += text(
    VIEW * UNIT - X_MARGIN,
    Y_FOOT_3,
    10,
    'label',
    'end',
    `L${live} S${stale} A${absent} M${simulated}`,
  )
  return s
}

/** One cell per signal. Never a proportional bar — a bar renders 2/5 and 4/10 identically. */
function coverageCells(w: WorldState): string {
  const measured = w.signals.filter((s) => s.measured).length
  const total = w.signals.length
  const label =
    total === 0
      ? 'COVERAGE ∅'
      : total > MAX_SIGNALS
        ? `COVERAGE ${measured}/${total} · ${MAX_SIGNALS} DRAWN`
        : `COVERAGE ${measured}/${total}`
  let s = text(X_MARGIN, Y_FOOT_2, 10, 'label', 'start', label)
  if (total === 0) return s

  const shown = Math.min(total, MAX_SIGNALS)
  const cell = 7 * UNIT
  const gap = 2 * UNIT
  const right = VIEW * UNIT - X_MARGIN
  const x0 = right - (shown * (cell + gap) - gap)
  const y = Y_FOOT_2 - 8 * UNIT

  for (let i = 0; i < shown; i += 1) {
    const x = x0 + i * (cell + gap)
    if (w.signals[i].measured) {
      s += `<rect x="${fmt(x)}" y="${fmt(y)}" width="${fmt(cell)}" height="${fmt(cell)}" fill="${PALETTE.measured}"/>`
    } else {
      s += `<rect x="${fmt(x)}" y="${fmt(y)}" width="${fmt(cell)}" height="${fmt(cell)}" fill="none" stroke="${PALETTE.ghost}" stroke-width="1"/>`
    }
  }
  return s
}

// ── metadata ──────────────────────────────────────────────────────────────────

/**
 * Base64 over UTF-8 bytes.
 *
 * Not `btoa`, which operates on a binary string and throws or mangles above U+00FF — an
 * observer name with an accent in it would encode differently here than in Rust, and the
 * token would differ from the one the CLI produces for the same world.
 */
export function base64(bytes: Uint8Array): string {
  const A = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/'
  let out = ''
  for (let i = 0; i < bytes.length; i += 3) {
    const rest = bytes.length - i
    const b0 = bytes[i]
    const b1 = rest > 1 ? bytes[i + 1] : 0
    const b2 = rest > 2 ? bytes[i + 2] : 0
    const n = (b0 << 16) | (b1 << 8) | b2
    out += A[(n >> 18) & 63]
    out += A[(n >> 12) & 63]
    out += rest > 1 ? A[(n >> 6) & 63] : '='
    out += rest > 2 ? A[n & 63] : '='
  }
  return out
}

/** The SVG as a self-contained `data:` URI. */
export function dataUri(svg: string): string {
  return `data:image/svg+xml;base64,${base64(new TextEncoder().encode(svg))}`
}

/**
 * ERC-721-shaped token metadata.
 *
 * No attribute may be a number nobody measured, and none may be a score. An unbounded
 * extent is the string `unbounded`; a world with no objects has legibility `∅`. A trait
 * list is rendered by software nobody here controls and `0` is what a missing field becomes
 * on the way — "Legibility: 0" for a world nobody looked at is a fabricated observation
 * with a card around it.
 */
export function metadataFor(
  w: WorldState,
  svg: string,
  digestHex: string,
  image?: string,
): Record<string, unknown> {
  const [live, stale, absent, simulated] = counts(w)
  const counted = w.signals.filter((s) => s.measured).length
  const estimated = w.signals.length - counted

  const leg: string = w.objects.length === 0 ? '∅' : fixed2(legibility(w))
  const t = w.extent.total
  const extent =
    t === null || t === undefined
      ? `${w.extent.observed} · unbounded`
      : `${w.extent.observed}/${t}`

  return {
    name: `Omni world · ${w.entity.label}`,
    description: description(w),
    image: image ?? dataUri(svg),
    external_url: w.entity.locator,
    attributes: [
      { trait_type: 'Domain', value: w.domain },
      { trait_type: 'Entity kind', value: w.entity.kind },
      { trait_type: 'Observer', value: w.observer },
      { trait_type: 'Extent', value: extent },
      { trait_type: 'Legibility', value: leg },
      { trait_type: 'Objects', value: w.objects.length },
      { trait_type: 'Signals counted', value: counted },
      { trait_type: 'Signals estimated', value: estimated },
      { trait_type: 'Blind spots', value: w.blind_spots.length },
      { trait_type: 'Live', value: live },
      { trait_type: 'Stale', value: stale },
      { trait_type: 'Absent', value: absent },
      { trait_type: 'Simulated', value: simulated },
      { trait_type: 'World schema', value: w.schema ?? 'undeclared' },
    ],
    scema: {
      world_commitment: digestHex,
      observed_at: w.observed_at,
      schema: w.schema ?? null,
    },
  }
}

function description(w: WorldState): string {
  let d =
    `A Scematica Omni world plate: the state of \`${w.entity.locator}\` as one observer found it at unix ${w.observed_at}, drawn to scale. ` +
    `Every mark is a measurement or the absence of one. Dashed means nobody measured it; a notch in the outer ring is something the observer could not read; a hollow cap is a magnitude that was estimated rather than counted.`
  d +=
    ` The commitment binds this plate to that world file. It does not prove the world was as described ` +
    `— provenance carries that — and it does not prove this is the only plate for it.`
  if (w.blind_spots.length > 0) {
    d += ` ${w.blind_spots.length} blind spot(s) were reported and are drawn as notches.`
  }
  return d
}

// ── loading ───────────────────────────────────────────────────────────────────

export type PlateSourceKind = 'world' | 'record'

export interface PlateSource {
  world: WorldState
  /** Hex digest of the canonical encoding of the world. */
  digest: string
  kind: PlateSourceKind
}

/**
 * Read a plate source from the **raw text** of a world file or a sealed record.
 *
 * Text, not a parsed object, and for the reason `OmniTerminal` already holds both: a
 * `JSON.parse` / `JSON.stringify` round trip collapses Rust's `0.0` to `0`, which moves it
 * from the FLOAT tag to the INTEGER tag in the canonical encoding and changes the digest.
 * Nothing would be wrong with the record — the round trip destroyed information the
 * encoding depends on. So the digest is taken from the original bytes.
 *
 * For a record, the **stored** `commitment.world` is used rather than a recomputed one. If
 * the record has been edited, the plate then carries a digest that does not match its own
 * world and `scema verify` says which field moved. Recomputing here would quietly paper
 * over exactly the tampering the commitment exists to expose.
 */
export async function plateSourceFromText(
  text: string,
  sha256: Sha256,
): Promise<PlateSource> {
  const parsed = JSON.parse(text) as Record<string, unknown>

  if (parsed.commitment !== undefined && parsed.world !== undefined) {
    const commitment = parsed.commitment as Record<string, unknown>
    const stored = typeof commitment.world === 'string' ? commitment.world : null
    const world = parsed.world as WorldState
    if (stored !== null) return { world, digest: stored, kind: 'record' }
    const bytes = fieldBytes(parseCanonical(text), 'world')
    if (bytes === null) throw new Error('record has a world field that cannot be encoded')
    return { world, digest: toHex(await sha256(bytes)), kind: 'record' }
  }

  if (parsed.entity !== undefined && parsed.observer !== undefined) {
    const world = parsed as unknown as WorldState
    const bytes = canonicalBytes(parseCanonical(text))
    return { world, digest: toHex(await sha256(bytes)), kind: 'world' }
  }

  throw new Error(
    'not a world or a decision record: expected either `observer` + `entity` ' +
      '(a WorldState, as `scema observe` prints) or `world` + `commitment` (a sealed record)',
  )
}
