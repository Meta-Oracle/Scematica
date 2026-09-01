/**
 * Collision: what is solid, what is not, and why the difference is an epistemic claim.
 *
 * Pure and grid-accelerated. No GL, no clock, no camera — `check:scemaworld` pins all of it.
 *
 * ## The rule that decides what you can hit
 *
 * A sector contains six kinds of thing the observer actually *perceived* — stations, docks,
 * depots, markets, the origin, a derelict — and three kinds that are statements about the limits
 * of what it perceived. A **phantom** is a station the observer modelled rather than saw. A
 * **marker** is a place it looked and found nothing. A **rift** is a region it could not read at
 * all.
 *
 * The first six are solid. The last three are not, and this is the only interesting decision in
 * the file.
 *
 * The temptation is to make everything solid, because a mirage you fly through "feels like a
 * bug". But making a phantom solid is the game asserting that something is there, on the strength
 * of a record that explicitly says nobody looked and saw it. Making it permeable is not the
 * opposite claim: the game simulates what was *observed*, and there is nothing here to hit
 * because nobody observed anything — which is a fact about the record, not about reality. A rift
 * is the sharpest case: it is a hole in somebody's knowledge, and a hole cannot be run into.
 *
 * So the HUD says so, in as many words, the moment you pass through one. That sentence is the
 * mechanic. A player who flies through a station and is told *"nothing here — this was modelled,
 * not observed"* has learned what a provenance is, from the cockpit, in a way no legend achieves.
 *
 * ## Why a grid
 *
 * A thousand nodes against seventy craft and a few dozen bolts is a hundred thousand distance
 * tests a frame if done naively, every frame, forever. The nodes never move, so the grid is built
 * once per space and queried; craft are few enough to test pairwise within a neighbourhood.
 */

import { servicesOf, type Node, type NodeKind, type Space, type Vec3 } from './generate.ts'
import { EXTENT } from './scale.ts'

/**
 * Whether a node kind is a thing you can run into.
 *
 * Separate from `Body.solid` in `view.ts` on purpose, even though the two nearly agree. That one
 * decides how a thing is *drawn* — hollow means "may not be there" — and this one decides whether
 * it is *there*. They answer different questions and a rift is exactly where they diverge: it is
 * drawn as a visible object because a blind spot is worth seeing, and it is not solid because it
 * is a gap in a record rather than a thing in space.
 */
export function collidesWith(kind: NodeKind): boolean {
  switch (kind) {
    case 'phantom':
    case 'marker':
    case 'rift':
      return false
    default:
      return true
  }
}

/** What the HUD says when you pass through something that was never observed. */
export function permeableNote(kind: NodeKind): string {
  switch (kind) {
    case 'phantom':
      return 'nothing here — that station was modelled, not observed'
    case 'marker':
      return 'nothing here — the observer looked and found nothing'
    case 'rift':
      return 'a blind spot — nobody could read this region'
    default:
      return ''
  }
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
        if (bucket) out.push(...bucket)
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
 * Positional separation for a list of movers.
 *
 * Returns a displacement per mover, to be applied by the caller — this file never mutates. It is
 * a *positional* correction rather than a steering force on purpose: a craft that merely steers
 * away still interpenetrates while it turns, and two wireframes occupying one point is the exact
 * thing that reads as cheap.
 *
 * O(n²) within the swarm, which is seventy craft: five thousand tests, once a frame, on numbers
 * already in cache. A grid here would be machinery guarding nothing.
 */
export function separate(movers: Mover[]): Vec3[] {
  const push: Vec3[] = movers.map(() => ({ x: 0, y: 0, z: 0 }))
  for (let i = 0; i < movers.length; i += 1) {
    for (let j = i + 1; j < movers.length; j += 1) {
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
      const half = (want - dist) / 2
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
 * The nearest permeable node the segment passes through, for the note the HUD shows.
 *
 * Deliberately a *separate* query from the solid grid. Building one grid with a flag would put
 * the two answers one boolean apart, and the whole point of the distinction is that they are
 * different kinds of claim.
 */
export function passedThrough(space: Space, from: Vec3, to: Vec3, radius: number): Node | null {
  let best: Node | null = null
  let bestT = Infinity
  for (const n of space.nodes) {
    if (collidesWith(n.kind)) continue
    // Cheap reject before the segment maths: the sector has a thousand nodes and this runs every
    // frame. Most of them are nowhere near.
    const rough = Math.max(Math.abs(n.at.x - from.x), Math.abs(n.at.y - from.y), Math.abs(n.at.z - from.z))
    if (rough > EXTENT * 0.05) continue
    const { dist, t } = closestOnSegment(n.at, from, to)
    if (dist <= radius * 6 && t < bestT) {
      best = n
      bestT = t
    }
  }
  return best
}

/** Convenience: is this node one you can dock with *and* fly into? */
export function isStation(n: Node): boolean {
  return collidesWith(n.kind) && servicesOf(n.kind).length > 0
}
