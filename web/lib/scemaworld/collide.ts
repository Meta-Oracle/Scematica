/**
 * Collision, and where the epistemic distinction lives now that nodes are open structures.
 *
 * Pure and grid-accelerated. No GL, no clock, no camera — `check:scemaworld` pins all of it.
 *
 * ## Nodes do not block flight
 *
 * They did, briefly, and the node radii were then increased by half again on a sector two and a
 * half times larger. Solid obstacles at that size are not scenery, they are a maze: a sector
 * whose landmarks are also walls is one where the interesting thing about a market is that it is
 * in the way. They are drawn as open wireframe structures (`meshes.ts`) and you fly through them.
 *
 * ## So the distinction moved, rather than disappearing
 *
 * A sector contains six kinds of thing the observer actually *perceived* — station, dock, depot,
 * market, origin, derelict — and three that are statements about the limits of what it perceived.
 * A **phantom** is a station the observer modelled rather than saw. A **marker** is a place it
 * looked and found nothing. A **rift** is a region it could not read at all.
 *
 * Flying through the first six puts something on your sensors. Flying through the last three puts
 * nothing there, and the HUD says so in as many words. That is the same claim as before — the
 * game simulates what was observed, and there is nothing here to register because nothing was
 * observed — expressed as a *reading* rather than as a wall. It is arguably the better home for
 * it: a wall is a fact about the world, and a sensor return is a fact about what somebody knows.
 *
 * `registers` is the predicate. `collidesWith` is kept as its alias for the one thing that still
 * blocks a projectile — nothing, currently — and to keep the vocabulary in one place.
 *
 * ## What still collides
 *
 * Craft against each other, and craft against the player. A ship you can fly through is a ship
 * that is not there, and at these closing speeds a passing interceptor should be an event.
 *
 * ## Why a grid
 *
 * Kept for the node queries the nav and sensor paths still make: a thousand nodes against
 * seventy craft is a hundred thousand distance tests a frame if done naively. Nodes never move,
 * so it is built once per space.
 */

import { servicesOf, type Node, type NodeKind, type Space, type Vec3 } from './generate.ts'
import { EXTENT, R_NODE_MAX } from './scale.ts'

/**
 * Whether flying through a node puts anything on your sensors.
 *
 * The six kinds the observer perceived do. The three that are statements about the *limits* of
 * what it perceived do not — and the difference is the whole of the epistemics at the point a
 * player acts on them.
 */
export function registers(kind: NodeKind): boolean {
  switch (kind) {
    case 'phantom':
    case 'marker':
    case 'rift':
      return false
    default:
      return true
  }
}

/**
 * Alias for `registers`, kept so the grid and its callers keep one vocabulary.
 *
 * Nodes no longer block anything, so nothing reads this as "will stop a ship". It survives
 * because the grid still needs to know which nodes are *things* and which are annotations.
 */
export const collidesWith = registers

/** What the HUD says when you fly through something. */
export function passageNote(kind: NodeKind, label: string): string {
  switch (kind) {
    case 'phantom':
      return 'nothing on sensors — that station was modelled, not observed'
    case 'marker':
      return 'nothing on sensors — the observer looked here and found nothing'
    case 'rift':
      return 'a blind spot — nobody could read this region'
    default:
      return `passing through ${label}`
  }
}

/** The old name, kept for the permeable kinds only. */
export function permeableNote(kind: NodeKind): string {
  return registers(kind) ? '' : passageNote(kind, '')
}

// ── the grid ──────────────────────────────────────────────────────────────────

/**
 * Cell size.
 *
 * Comfortably larger than twice the biggest node radius, so a query only ever has to look at the
 * cells its own bounding box touches. Smaller cells would mean more of them and no fewer tests.
 */
const CELL = Math.round(EXTENT * 0.03)

function key(x: number, y: number, z: number): string {
  return `${Math.floor(x / CELL)},${Math.floor(y / CELL)},${Math.floor(z / CELL)}`
}

export interface Obstacle {
  at: Vec3
  radius: number
  node: Node
}

export interface Grid {
  cells: Map<string, Obstacle[]>
  /** The largest radius in the grid, so a query knows how far to widen its box. */
  maxRadius: number
}

/**
 * Build the obstacle grid for a space. Once per world — nodes do not move.
 *
 * Radii come from the same table `view.ts` draws with, because a hit test that disagrees with the
 * picture is the worst kind of bug in a game: the player is told they hit something they can see
 * they missed, or flies through something visibly in the way.
 */
export function gridOf(space: Space, radiusOf: (n: Node) => number): Grid {
  const cells = new Map<string, Obstacle[]>()
  let maxRadius = 0
  for (const n of space.nodes) {
    if (!collidesWith(n.kind)) continue
    const radius = radiusOf(n)
    maxRadius = Math.max(maxRadius, radius)
    const k = key(n.at.x, n.at.y, n.at.z)
    const bucket = cells.get(k)
    if (bucket) bucket.push({ at: n.at, radius, node: n })
    else cells.set(k, [{ at: n.at, radius, node: n }])
  }
  return { cells, maxRadius }
}

/** Every obstacle whose cell overlaps the box around a segment, widened by `pad`. */
function near(grid: Grid, a: Vec3, b: Vec3, pad: number): Obstacle[] {
  const reach = pad + grid.maxRadius
  const lo = {
    x: Math.min(a.x, b.x) - reach,
    y: Math.min(a.y, b.y) - reach,
    z: Math.min(a.z, b.z) - reach,
  }
  const hi = {
    x: Math.max(a.x, b.x) + reach,
    y: Math.max(a.y, b.y) + reach,
    z: Math.max(a.z, b.z) + reach,
  }
  const out: Obstacle[] = []
  for (let cx = Math.floor(lo.x / CELL); cx <= Math.floor(hi.x / CELL); cx += 1) {
    for (let cy = Math.floor(lo.y / CELL); cy <= Math.floor(hi.y / CELL); cy += 1) {
      for (let cz = Math.floor(lo.z / CELL); cz <= Math.floor(hi.z / CELL); cz += 1) {
        const bucket = grid.cells.get(`${cx},${cy},${cz}`)
        // Appended one at a time rather than spread. `push(...bucket)` builds an argument list
        // per bucket and this runs once per craft per frame; on a sector where a query can touch
        // hundreds of cells it was a measurable share of the tick.
        if (bucket) for (let i = 0; i < bucket.length; i += 1) out.push(bucket[i])
      }
    }
  }
  return out
}

// ── geometry ──────────────────────────────────────────────────────────────────

function sub(a: Vec3, b: Vec3): Vec3 {
  return { x: a.x - b.x, y: a.y - b.y, z: a.z - b.z }
}
function len(v: Vec3): number {
  return Math.hypot(v.x, v.y, v.z)
}
function dot(a: Vec3, b: Vec3): number {
  return a.x * b.x + a.y * b.y + a.z * b.z
}
function norm(v: Vec3): Vec3 {
  const l = len(v) || 1
  return { x: v.x / l, y: v.y / l, z: v.z / l }
}

/** Closest point on segment `a→b` to `p`, and the distance to it. */
export function closestOnSegment(p: Vec3, a: Vec3, b: Vec3): { at: Vec3; dist: number; t: number } {
  const ab = sub(b, a)
  const l2 = dot(ab, ab)
  if (l2 === 0) return { at: a, dist: len(sub(p, a)), t: 0 }
  const t = Math.max(0, Math.min(1, dot(sub(p, a), ab) / l2))
  const at = { x: a.x + ab.x * t, y: a.y + ab.y * t, z: a.z + ab.z * t }
  return { at, dist: len(sub(p, at)), t }
}

// ── sweeps ────────────────────────────────────────────────────────────────────

export interface Impact {
  obstacle: Obstacle
  /** Where along the move it happened, 0..1. */
  t: number
}

/**
 * The first obstacle a swept sphere of `radius` meets travelling `from → to`.
 *
 * **Swept, never an endpoint test.** A bolt covers a few million units per frame and a station is
 * a couple of million across, so an endpoint check misses the station entirely and the shot
 * arrives on the far side — which reads as the geometry being fake rather than as a bug. Same
 * reasoning as the projectile hit test in `weapons.ts`, and the same mistake made once already.
 *
 * Returns the *earliest* impact rather than any impact, so a shot fired down a line of stations
 * stops at the first one.
 */
export function sweep(grid: Grid, from: Vec3, to: Vec3, radius: number): Impact | null {
  let best: Impact | null = null
  for (const o of near(grid, from, to, radius)) {
    const { dist, t } = closestOnSegment(o.at, from, to)
    if (dist <= o.radius + radius && (!best || t < best.t)) best = { obstacle: o, t }
  }
  return best
}

export interface Resolved {
  at: Vec3
  /** The obstacle struck, if any. */
  hit: Obstacle | null
  /** Impact speed along the surface normal, in units per second. Zero when nothing was struck. */
  impact: number
}

/**
 * Move a body of `radius` from `from` toward `to`, stopping at the first solid surface.
 *
 * Placed *on* the surface rather than at the point of contact, and nudged out by a whisker. A
 * body left exactly touching re-collides on the next frame at zero speed and sticks — the
 * classic way a collision system becomes flypaper, and the reason a player who clips a station
 * would otherwise never get free of it.
 *
 * Sliding is deliberately not implemented. A ship that slid along a station would be a ship the
 * player is no longer flying, and at these speeds a hard stop plus a damage figure is both more
 * legible and more honest about what just happened.
 */
export function resolve(grid: Grid, from: Vec3, to: Vec3, radius: number, dt: number): Resolved {
  const hit = sweep(grid, from, to, radius)
  if (!hit) return { at: to, hit: null, impact: 0 }

  const o = hit.obstacle
  // The normal is taken at the point of *contact*, not at the start of the move. Using the start
  // makes a long tangential pass report a head-on normal, so a graze at cruise costs as much as
  // flying nose-first into a dock — which makes collisions feel arbitrary, which is worse than
  // not having them.
  //
  // Two ways that point degenerates, and both happen in practice. A move aimed exactly at the
  // centre puts the closest approach *on* the centre; and a body that is already there — which
  // is where every record-signal craft starts, since contacts sit on nodes — has no offset at
  // all. Either way there is no direction to push along, `norm` of a zero vector is a zero
  // vector, and the body is placed at the obstacle's centre and stays there forever. Falling
  // back to the pre-move position works because it is outside by construction; falling back
  // again to a fixed axis covers the case where even that is the centre.
  const contact = closestOnSegment(o.at, from, to).at
  let offset = sub(contact, o.at)
  if (len(offset) < 1e-6) offset = sub(from, o.at)
  if (len(offset) < 1e-6) offset = { x: 1, y: 0, z: 0 }
  const away = norm(offset)
  const surface = o.radius + radius
  // A whisker past the surface, proportional to the surface itself so it scales with the sector.
  const clear = surface * 1.02
  const at = {
    x: o.at.x + away.x * clear,
    y: o.at.y + away.y * clear,
    z: o.at.z + away.z * clear,
  }
  // Closing speed along the normal, which is what an impact costs. A graze at high speed along a
  // tangent is not the same event as flying nose-first into a dock, and charging both the same
  // would make near-misses feel arbitrary.
  const travel = sub(to, from)
  const closing = Math.max(0, -dot(travel, away)) / Math.max(dt, 1e-6)
  return { at, hit: o, impact: closing }
}

// ── craft against each other, and against the furniture ───────────────────────

export interface Mover {
  at: Vec3
  radius: number
}

/**
 * How far apart two craft are kept, as a multiple of their combined radii.
 *
 * Above 1 so a formation reads as a formation rather than as a pile. Below 2 so a wing still
 * arrives together — separation that is too strong turns four craft into a slowly expanding
 * cloud, which is the failure mode that looks like the AI having lost interest.
 */
export const SEPARATION = 1.35

/**
 * How much of an overlap is corrected per frame.
 *
 * Not all of it. Craft can start deeply overlapped — a raider wing shares an anchor, and traffic
 * is placed on the nodes the record's own signals also sit on — and resolving that in one step
 * displaced ships by twenty million units on the first frame, which on screen is a scatter
 * explosion the instant a world loads. A quarter per frame converges in well under a second and
 * never teleports anything.
 */
export const SEPARATION_RATE = 0.25

/**
 * Positional separation for a list of movers.
 *
 * Returns a displacement per mover, to be applied by the caller — this file never mutates. It is
 * a *positional* correction rather than a steering force on purpose: a craft that merely steers
 * away still interpenetrates while it turns, and two wireframes occupying one point is the exact
 * thing that reads as cheap.
 *
 * ## It was O(n²), and the comment saying that was fine has stopped being true
 *
 * The old note read: *"O(n²) within the swarm, which is seventy craft: five thousand tests, once a
 * frame, on numbers already in cache. A grid here would be machinery guarding nothing."* Every
 * word of that was correct when it was written and the premise moved out from under it. The sector
 * now carries three standing firefight clusters (`clusters.ts`) on top of the scattered roster —
 * **230 craft**, so twenty-six thousand pair tests a frame — and the whole tick was measured at
 * 6.78ms against a 16.7ms budget and a 4ms assertion. It is the exact failure the comment warned
 * about in its own last sentence, arriving from the direction it was not watching.
 *
 * ## Sweep on one axis, with the big hulls handled separately
 *
 * A uniform grid is the obvious answer and it is the wrong one here, because the radii span four
 * orders of magnitude: a cell sized for a titan holds the entire sector's fighters, and a cell
 * sized for an interceptor means a titan touches thousands of them. So:
 *
 * - **Big movers** (a handful of capitals) are tested against everything. Eleven times 230 is
 *   nothing, and it sidesteps the whole problem of a huge object in a fine index.
 * - **Everything else** is swept along `x`. Sorted once, and for each mover the scan stops at the
 *   first neighbour further away in `x` than the largest separation a small pair can want — which
 *   at fighter radii is a rounding error against the sector, so almost nothing survives.
 *
 * ## Determinism is preserved exactly, not approximately
 *
 * Floating-point addition is not associative, so accumulating the same pushes in a different order
 * gives different bits. The sector's *generation* is pure of this file, but a fight that diverges
 * between two machines because an optimisation reordered a sum is not a trade worth making
 * silently. So the two passes only decide **which pairs to test**; the surviving pairs are then
 * sorted back into the original `(i, j)` order and applied in exactly the sequence the nested
 * loops would have applied them. The result is bit-identical to the O(n²) version, and
 * `check:scemaworld` pins that against a brute-force reference rather than trusting this note.
 */
export function separate(movers: Mover[]): Vec3[] {
  const n = movers.length
  const push: Vec3[] = movers.map(() => ({ x: 0, y: 0, z: 0 }))
  if (n < 2) return push

  // The split point. Derived from the movers actually present rather than from a constant: a
  // "big" hull is one far larger than the median, and hard-coding a radius here would silently
  // stop splitting anything the first time the class table moved.
  let maxR = 0
  for (const m of movers) if (m.radius > maxR) maxR = m.radius
  const bigAt = maxR / 8

  const small: number[] = []
  const big: number[] = []
  for (let i = 0; i < n; i += 1) (movers[i].radius >= bigAt ? big : small).push(i)

  const pairs: [number, number][] = []
  const seen = new Set<number>()
  const add = (i: number, j: number) => {
    const a = i < j ? i : j
    const b = i < j ? j : i
    // Big-against-big is reachable from both passes; the key keeps it one pair. `n` fits in the
    // low half of a double comfortably at these sizes.
    const key = a * n + b
    if (seen.has(key)) return
    seen.add(key)
    pairs.push([a, b])
  }

  for (const i of big) for (let j = 0; j < n; j += 1) if (j !== i) add(i, j)

  // The sweep. `smallMax` bounds how far apart two small movers can be and still touch, so the
  // inner scan can stop rather than run to the end of the array.
  let smallMax = 0
  for (const i of small) if (movers[i].radius > smallMax) smallMax = movers[i].radius
  const window = smallMax * 2 * SEPARATION
  const order = small.slice().sort((a, b) => movers[a].at.x - movers[b].at.x)
  for (let a = 0; a < order.length; a += 1) {
    const i = order[a]
    for (let b = a + 1; b < order.length; b += 1) {
      const j = order[b]
      if (movers[j].at.x - movers[i].at.x > window) break
      add(i, j)
    }
  }

  // Back into the order the nested loops would have used, so the arithmetic is identical.
  pairs.sort((p, q) => (p[0] - q[0]) || (p[1] - q[1]))

  for (const [i, j] of pairs) {
    const a = movers[i]
    const b = movers[j]
    const d = sub(b.at, a.at)
    const want = (a.radius + b.radius) * SEPARATION
    const dist = len(d)
    if (dist >= want) continue
    // Two craft at exactly the same point have no direction to separate along. Any axis will
    // do; what matters is that they pick one and that the choice is deterministic, because two
    // players holding the same record must see the same fight.
    const dir = dist < 1e-6 ? { x: 1, y: 0, z: 0 } : norm(d)
    const half = ((want - dist) / 2) * SEPARATION_RATE
    push[i] = { x: push[i].x - dir.x * half, y: push[i].y - dir.y * half, z: push[i].z - dir.z * half }
    push[j] = { x: push[j].x + dir.x * half, y: push[j].y + dir.y * half, z: push[j].z + dir.z * half }
  }
  return push
}

/**
 * The brute-force separation, kept only so the fast path can be checked against it.
 *
 * Exported rather than duplicated in the test file: a reference implementation that lives beside
 * the thing it verifies cannot drift away from the contract it is asserting, and one copied into a
 * check script eventually does.
 */
export function separateSlow(movers: Mover[]): Vec3[] {
  const push: Vec3[] = movers.map(() => ({ x: 0, y: 0, z: 0 }))
  for (let i = 0; i < movers.length; i += 1) {
    for (let j = i + 1; j < movers.length; j += 1) {
      const a = movers[i]
      const b = movers[j]
      const d = sub(b.at, a.at)
      const want = (a.radius + b.radius) * SEPARATION
      const dist = len(d)
      if (dist >= want) continue
      const dir = dist < 1e-6 ? { x: 1, y: 0, z: 0 } : norm(d)
      const half = ((want - dist) / 2) * SEPARATION_RATE
      push[i] = { x: push[i].x - dir.x * half, y: push[i].y - dir.y * half, z: push[i].z - dir.z * half }
      push[j] = { x: push[j].x + dir.x * half, y: push[j].y + dir.y * half, z: push[j].z + dir.z * half }
    }
  }
  return push
}

/**
 * Bend a desired heading around the nearest obstacle ahead. Returns `want` when the way is clear.
 *
 * Steering rather than stopping, because a craft that halts at a station has visibly given up on
 * the fight, and because avoidance that happens *early* is what looks like piloting. The probe is
 * a lookahead in *seconds*, so a destroyer and an interceptor each begin turning at the point
 * where they can still make it, with no per-class tuning.
 *
 * ## Why it blends instead of returning the sidestep
 *
 * The first version returned the pure lateral direction, and it produced a permanent orbit: the
 * craft turned fully broadside, flew sideways until the obstacle left its probe, turned back
 * toward its target, re-acquired the obstacle, and repeated. A test caught it as a pursuer frozen
 * at nineteen million units that never closed and never gave up — which on screen would have
 * looked like an enemy politely declining to fight.
 *
 * So the sidestep is mixed into the desired heading in proportion to how *imminent* the obstacle
 * is. Far away it barely bends; about to hit, it is almost entirely lateral. The craft therefore
 * always keeps some component of its actual intent, which is what lets it come out the far side
 * rather than circling the near one.
 */
export function steerAround(
  grid: Grid,
  at: Vec3,
  facing: Vec3,
  want: Vec3,
  radius: number,
  speed: number,
  lookaheadSecs = 1.6,
): { dir: Vec3; urgency: number } {
  const probe = Math.max(radius * 4, speed * lookaheadSecs)
  const ahead = { x: at.x + facing.x * probe, y: at.y + facing.y * probe, z: at.z + facing.z * probe }
  const hit = sweep(grid, at, ahead, radius)
  if (!hit) return { dir: want, urgency: 0 }

  const o = hit.obstacle
  // Steer along the component of the obstacle's offset perpendicular to the current heading —
  // the shortest way around rather than a reversal. Reversing would have a craft that meets a
  // station bounce back down its own approach, which reads as a glitch rather than as flying.
  const toObstacle = sub(o.at, at)
  const along = dot(toObstacle, facing)
  const lateral = {
    x: toObstacle.x - facing.x * along,
    y: toObstacle.y - facing.y * along,
    z: toObstacle.z - facing.z * along,
  }
  // Dead ahead, exactly: no lateral component exists. Pick a deterministic one rather than
  // dividing by zero — the same reasoning as the antiparallel case in `enemy.ts::turnToward`,
  // and the same bug if it is skipped, since a craft aimed precisely at a station flies into it.
  const side = len(lateral) < 1e-6
    ? norm({ x: -facing.y, y: facing.x, z: 0 })
    : norm({ x: -lateral.x, y: -lateral.y, z: -lateral.z })

  // `hit.t` is how far along the probe the obstacle sits. Squared so the bend stays gentle until
  // it genuinely matters, then arrives quickly.
  const urgency = (1 - hit.t) ** 2
  const dir = norm({
    x: want.x * (1 - urgency) + side.x * urgency,
    y: want.y * (1 - urgency) + side.y * urgency,
    z: want.z * (1 - urgency) + side.z * urgency,
  })
  return { dir, urgency }
}

/**
 * The node whose volume this segment passed through, if any. Nearest first.
 *
 * Covers **every** kind, observed and unobserved alike, because the interesting output is now
 * which of the two it was — `passageNote` turns that into the sentence the HUD shows. Restricting
 * this to the unobserved kinds would make "nothing on sensors" the only message the mechanic ever
 * produces, and a distinction only one side of which is ever visible is not a distinction the
 * player can learn.
 */
export function crossed(space: Space, from: Vec3, to: Vec3, radius: number): Node | null {
  let best: Node | null = null
  let bestT = Infinity
  const reach = radius + R_NODE_MAX
  for (const n of space.nodes) {
    // Cheap reject before the segment maths: a sector has a thousand nodes, this runs every
    // frame, and most of them are nowhere near.
    // Chebyshev reject against the actual reach rather than a fixed slice of the sector. The
    // fixed slice was 6% of a volume that has since grown two and a half times, so it admitted
    // hundreds of nodes per frame to do exact segment maths on.
    if (Math.abs(n.at.x - from.x) > reach) continue
    if (Math.abs(n.at.y - from.y) > reach) continue
    if (Math.abs(n.at.z - from.z) > reach) continue
    const { dist, t } = closestOnSegment(n.at, from, to)
    if (dist <= radius && t < bestT) {
      best = n
      bestT = t
    }
  }
  return best
}

/** The old name. Reports only the kinds that register nothing. */
export function passedThrough(space: Space, from: Vec3, to: Vec3, radius: number): Node | null {
  const n = crossed(space, from, to, radius)
  return n && !registers(n.kind) ? n : null
}

/** Convenience: is this node one you can dock with *and* fly into? */
export function isStation(n: Node): boolean {
  return collidesWith(n.kind) && servicesOf(n.kind).length > 0
}
