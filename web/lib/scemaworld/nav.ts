/**
 * Navigation: finding anything at all in three hundred and seventy-five million units of space.
 *
 * ## Why this is not a nicety
 *
 * Enlarging the sector fixed one complaint and created a worse one. A thousand stations spread
 * over a volume that takes forty seconds to cross at full throttle are, from the cockpit, a
 * black screen with a few points of light in it. Every service in the game — refuelling,
 * repair, the market — sits on a node you have to *reach*, and with nothing but a viewport
 * there is no way to reach one deliberately. You fly, you run dry, and the sector's size reads
 * as emptiness rather than as scale.
 *
 * So: a target computer. Nearest services by kind, a selected waypoint, and a bearing that says
 * whether the thing is ahead of you or behind. This is what converts distance into a journey,
 * which was the point of the distance.
 *
 * ## The one rule it must not break
 *
 * The nav computer reports **range and bearing**, which are geometry, and never a verdict about
 * what is there. It will happily route you to a `phantom` — a station the observer *modelled*
 * rather than saw — because the record says it might be there and the honest thing is to let
 * you go and find out. What it does do is carry the node's kind through, so the HUD can render
 * a phantom as a phantom. A nav computer that silently filtered out unreliable destinations
 * would be making the record's uncertainty invisible at exactly the moment you act on it.
 */

import { servicesOf, type Node, type Service, type Space, type Vec3 } from './generate.ts'
import { forward, type Camera } from './camera.ts'

export interface Fix {
  node: Node
  /** Metres — well, units — from the ship. */
  range: number
  /**
   * Cosine of the angle between the nose and the target: 1 dead ahead, −1 directly behind.
   *
   * A cosine rather than degrees because that is what the renderer and the HUD both want, and
   * converting to an angle here would mean converting back.
   */
  ahead: number
}

function dist(a: Vec3, b: readonly [number, number, number]): number {
  return Math.hypot(a.x - b[0], a.y - b[1], a.z - b[2])
}

/** Where the ship is, as a point. */
function at(camera: Camera): Vec3 {
  return { x: camera.position[0], y: camera.position[1], z: camera.position[2] }
}

/** Cosine between the nose and the bearing to `target`. */
export function ahead(camera: Camera, target: Vec3): number {
  const f = forward(camera)
  const dx = target.x - camera.position[0]
  const dy = target.y - camera.position[1]
  const dz = target.z - camera.position[2]
  const l = Math.hypot(dx, dy, dz)
  if (l === 0) return 1
  return (f[0] * dx + f[1] * dy + f[2] * dz) / l
}

/**
 * The nearest `count` nodes offering `service`, nearest first.
 *
 * Linear over the node list, which is a thousand entries and runs in well under a millisecond;
 * a spatial index here would be a structure to keep correct in exchange for nothing.
 */
export function nearest(space: Space, camera: Camera, service: Service, count = 3): Fix[] {
  const p = at(camera)
  const fixes: Fix[] = []
  for (const n of space.nodes) {
    if (!servicesOf(n.kind).includes(service)) continue
    fixes.push({ node: n, range: dist(n.at, camera.position), ahead: ahead(camera, n.at) })
  }
  fixes.sort((a, b) => a.range - b.range)
  return fixes.slice(0, count)
}

/** A fix on one specific node, or null if it is gone. */
export function fixOn(space: Space, camera: Camera, nodeId: number): Fix | null {
  const n = space.nodes.find((x) => x.id === nodeId)
  if (!n) return null
  return { node: n, range: dist(n.at, camera.position), ahead: ahead(camera, n.at) }
}

/**
 * Bearing as a short string for the HUD.
 *
 * Deliberately coarse. A precise angle would invite flying by the number instead of by the
 * window, and the window is the game.
 */
export function bearingLabel(fix: Fix): string {
  if (fix.ahead > 0.985) return 'ON NOSE'
  if (fix.ahead > 0.7) return 'AHEAD'
  if (fix.ahead > 0.2) return 'OFF BOW'
  if (fix.ahead > -0.2) return 'ABEAM'
  if (fix.ahead > -0.7) return 'OFF QUARTER'
  return 'ASTERN'
}

/** Range as a short string. Kilo-units, then mega — a raw nine-digit number is unreadable. */
export function rangeLabel(range: number): string {
  if (range >= 1e6) return `${(range / 1e6).toFixed(1)}Mm`
  if (range >= 1e3) return `${Math.round(range / 1e3)}km`
  return `${Math.round(range)}m`
}

/**
 * Cycle to the next waypoint of a given service.
 *
 * Returns the node id, or null when the sector has none of that service at all — which is a
 * real state, not an error. A world whose observer perceived nothing live has no docks, and the
 * honest answer to "route me to a dock" is that there is not one.
 */
export function cycle(space: Space, camera: Camera, service: Service, current: number | null): number | null {
  const fixes = nearest(space, camera, service, 8)
  if (fixes.length === 0) return null
  const i = fixes.findIndex((f) => f.node.id === current)
  return fixes[(i + 1) % fixes.length].node.id
}
