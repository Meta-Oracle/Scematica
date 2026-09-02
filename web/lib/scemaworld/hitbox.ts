/**
 * Hitboxes that match the hulls they are drawn from.
 *
 * ## Why a sphere was wrong, and wrong in the worst direction
 *
 * Every craft was hit-tested as a sphere of its class radius. A hull is not a sphere: the
 * `dreadnought` mesh reaches **2.1** along its nose and **0.72** across, so a sphere of radius 1
 * misses the whole prow and the whole stern while covering a great deal of empty space beside the
 * ship. The bigger the ship the worse it gets, which is exactly backwards — and it is why fire
 * aimed squarely at the middle of a visible war hull kept missing.
 *
 * A capsule along the hull's own axis fixes both halves at once. It is the right shape for
 * everything here (these are long ships), it costs one segment-to-segment distance, and it makes
 * "aimed at the picture" and "hit" the same thing.
 *
 * ## The extents are measured, never declared
 *
 * `HITBOX` is computed from the actual vertex data at module load. A hand-written table would be
 * a second description of each shape, and the two would drift the first time a silhouette was
 * tweaked — at which point the hit test and the picture disagree again. This project has paid for
 * that failure twice already: once when craft started being sized by class while `weapons.ts`
 * still sized them from a signal's magnitude, and once here.
 */

import * as Mesh from './meshes.ts'
import type { Shape } from './classes.ts'
import type { Vec3 } from './generate.ts'

/** Extents per silhouette, in local units, measured from the meshes themselves. */
export const HITBOX: Record<Shape, Mesh.Bounds> = {
  // A sphere is its own bound. Contacts and the waypoint are drawn this way and are genuinely
  // round, so the capsule degenerates to the sphere it should be.
  sphere: { ahead: 1, behind: 1, cross: 1 },
  shell: { ahead: 1, behind: 1, cross: 1 },
  // A bolt is tested by its own sweep in `weapons.ts`; it is never a target.
  bolt: { ahead: 1, behind: 1, cross: 1 },
  interceptor: Mesh.boundsOf(Mesh.interceptor()),
  gunship: Mesh.boundsOf(Mesh.gunship()),
  capital: Mesh.boundsOf(Mesh.capital()),
  dreadnought: Mesh.boundsOf(Mesh.dreadnought()),
  station: Mesh.boundsOf(Mesh.station()),
  market: Mesh.boundsOf(Mesh.market()),
  dock: Mesh.boundsOf(Mesh.dock()),
  depot: Mesh.boundsOf(Mesh.depot()),
  derelict: Mesh.boundsOf(Mesh.derelict()),
  rift: Mesh.boundsOf(Mesh.rift()),
  phantom: Mesh.boundsOf(Mesh.phantom()),
  marker: Mesh.boundsOf(Mesh.marker()),
  origin: Mesh.boundsOf(Mesh.origin()),
}

/** A craft's hittable volume: a capsule along its facing. */
export interface Capsule {
  /** Nose end of the axis. */
  head: Vec3
  /** Stern end. */
  tail: Vec3
  /** Distance from the axis that counts as a hit. */
  radius: number
}

function unit(v: Vec3): Vec3 {
  const l = Math.hypot(v.x, v.y, v.z) || 1
  return { x: v.x / l, y: v.y / l, z: v.z / l }
}

/**
 * The capsule for a craft at `at`, facing `facing`, drawn at `scale`.
 *
 * `scale` is the class radius — the same number `view.ts` hands the renderer — so the capsule and
 * the silhouette are the same object described twice by one source.
 */
export function capsuleOf(at: Vec3, facing: Vec3, scale: number, shape: Shape): Capsule {
  const b = HITBOX[shape]
  const f = unit(facing)
  return {
    head: { x: at.x + f.x * b.ahead * scale, y: at.y + f.y * b.ahead * scale, z: at.z + f.z * b.ahead * scale },
    tail: { x: at.x - f.x * b.behind * scale, y: at.y - f.y * b.behind * scale, z: at.z - f.z * b.behind * scale },
    radius: b.cross * scale,
  }
}

/**
 * Shortest distance between segments `p→q` and `r→s`.
 *
 * Both ends matter. The shot is a segment because it covers millions of units per frame and an
 * endpoint test would tunnel; the target is a segment because a war hull is long. Testing a
 * *point* against the hull axis — the obvious simplification — reintroduces tunnelling on exactly
 * the ships this exists to make hittable.
 *
 * The clamped-parameter form rather than the closed-form solve: it degenerates gracefully when
 * either segment has zero length, which happens constantly (a stationary craft, a shot resolved
 * in a frame of zero dt) and which the closed form divides by.
 */
export function segmentDistance(p: Vec3, q: Vec3, r: Vec3, s: Vec3): number {
  const d1 = { x: q.x - p.x, y: q.y - p.y, z: q.z - p.z }
  const d2 = { x: s.x - r.x, y: s.y - r.y, z: s.z - r.z }
  const d12 = { x: p.x - r.x, y: p.y - r.y, z: p.z - r.z }

  const a = d1.x * d1.x + d1.y * d1.y + d1.z * d1.z
  const e = d2.x * d2.x + d2.y * d2.y + d2.z * d2.z
  const f = d2.x * d12.x + d2.y * d12.y + d2.z * d12.z

  let t1 = 0
  let t2 = 0
  const EPS = 1e-9

  if (a <= EPS && e <= EPS) {
    // Two points.
    return Math.hypot(d12.x, d12.y, d12.z)
  }
  if (a <= EPS) {
    t2 = Math.max(0, Math.min(1, f / e))
  } else {
    const c = d1.x * d12.x + d1.y * d12.y + d1.z * d12.z
    if (e <= EPS) {
      t1 = Math.max(0, Math.min(1, -c / a))
    } else {
      const b = d1.x * d2.x + d1.y * d2.y + d1.z * d2.z
      const denom = a * e - b * b
      t1 = denom > EPS ? Math.max(0, Math.min(1, (b * f - c * e) / denom)) : 0
      t2 = (b * t1 + f) / e
      if (t2 < 0) {
        t2 = 0
        t1 = Math.max(0, Math.min(1, -c / a))
      } else if (t2 > 1) {
        t2 = 1
        t1 = Math.max(0, Math.min(1, (b - c) / a))
      }
    }
  }

  const c1 = { x: p.x + d1.x * t1, y: p.y + d1.y * t1, z: p.z + d1.z * t1 }
  const c2 = { x: r.x + d2.x * t2, y: r.y + d2.y * t2, z: r.z + d2.z * t2 }
  return Math.hypot(c1.x - c2.x, c1.y - c2.y, c1.z - c2.z)
}

/** Whether a shot travelling `from → to` with calibre `calibre` strikes `cap`. */
export function strikes(cap: Capsule, from: Vec3, to: Vec3, calibre: number): boolean {
  return segmentDistance(from, to, cap.tail, cap.head) <= cap.radius + calibre
}
