/**
 * The nav map: a sector five thousand million units across, on a square of glass.
 *
 * Pure geometry. No SVG, no DOM, no colours — the component places rectangles and `view.ts` owns
 * the palette, exactly as `gl.ts` does for the world. That split is what lets the projection be
 * tested without a browser, and it is the only reason a map that silently mis-plots a station is
 * catchable at all.
 *
 * ## Why a top-down projection and not a scale model
 *
 * The sector is a volume and the map is a plane, so something has to be thrown away. Throwing
 * away `y` and reporting it separately is the honest choice: a plotted position is then exactly
 * true in two axes and *stated* as unknown in the third, rather than being a perspective picture
 * in which everything is a little bit wrong and nothing says so. Each blip carries its own
 * `above` — how far off the map's plane it really is — and the component draws that as a tick.
 *
 * ## The map is a control, not a readout
 *
 * `pick` turns a click back into the nearest node. That is the whole reason the map exists: with
 * four hundred nodes over five thousand million units, cycling waypoints with a key is a way to
 * arrive somewhere by luck. Choosing one on a map is a way to arrive on purpose.
 */

import type { Node, Space, Vec3 } from './generate.ts'
import { servicesOf } from './generate.ts'
import type { Faction } from './factions.ts'
import { EXTENT } from './scale.ts'

/** Zoom steps, as a fraction of `EXTENT` shown from the centre to the edge of the map. */
export const ZOOMS = [0.08, 0.2, 0.5, 1.2, 2.6] as const

/** The default: wide enough to see the neighbourhood, tight enough to tell two docks apart. */
export const DEFAULT_ZOOM = 2

/** One thing drawn on the map, in map coordinates from −1 to 1. */
export interface Blip {
  /** −1..1 across the map. Off-map blips are omitted, never clamped to the edge. */
  x: number
  y: number
  /**
   * How far above (positive) or below (negative) the map plane the thing really is, as a
   * fraction of the shown radius.
   *
   * The map throws away one axis and this is what it says instead of pretending it did not. A
   * station plotted next to you that is two hundred million units above is not next to you.
   */
  above: number
  kind: 'node' | 'craft' | 'waypoint'
  /** Node kind or faction, for the component to colour by. */
  tone: string
  label: string
  /** Node id, so a click can be turned back into a waypoint. */
  id: number | null
}

export interface MapView {
  blips: Blip[]
  /** World units from the map centre to its edge. */
  radius: number
  /** The ship's heading as an angle in radians, measured on the map plane. */
  heading: number
}

function project(at: Vec3, centre: Vec3, radius: number): { x: number; y: number; above: number } {
  return {
    x: (at.x - centre.x) / radius,
    y: (at.z - centre.z) / radius,
    above: (at.y - centre.y) / radius,
  }
}

export interface MapInput {
  space: Space
  /** Where the ship is. The map is always centred on it. */
  at: Vec3
  /** The ship's forward vector, for the heading needle. */
  facing: Vec3
  zoom: number
  waypoint: number | null
  craft: { id: string; at: Vec3; faction: Faction; label: string }[]
  /** Cap on plotted nodes, nearest first. */
  limit?: number
}

/**
 * Build the map.
 *
 * Nodes are capped and taken nearest-first rather than filtered by kind. Filtering to "important"
 * kinds is the obvious economy and it is wrong: the map would then show a sector that is mostly
 * markets and docks, which is not the sector you are flying through. A dense cluster of ordinary
 * stations *is* information.
 */
export function build(input: MapInput): MapView {
  const { space, at, facing, zoom, waypoint, craft } = input
  const radius = Math.round(EXTENT * (ZOOMS[zoom] ?? ZOOMS[DEFAULT_ZOOM]))
  const limit = input.limit ?? 160
  const blips: Blip[] = []

  const near = space.nodes
    .map((n) => ({ n, d: Math.hypot(n.at.x - at.x, n.at.y - at.y, n.at.z - at.z) }))
    .filter((e) => e.d < radius * 1.6)
    .sort((a, b) => a.d - b.d)
    .slice(0, limit)

  for (const { n } of near) {
    const p = project(n.at, at, radius)
    // Off the square is omitted rather than clamped. A blip pinned to the edge is a claim that
    // something is *there*, and the thing it is claiming about is somewhere else entirely.
    if (Math.abs(p.x) > 1 || Math.abs(p.y) > 1) continue
    blips.push({ ...p, kind: 'node', tone: n.kind, label: n.label, id: n.id })
  }

  for (const c of craft) {
    const p = project(c.at, at, radius)
    if (Math.abs(p.x) > 1 || Math.abs(p.y) > 1) continue
    blips.push({ ...p, kind: 'craft', tone: c.faction, label: c.label, id: null })
  }

  if (waypoint !== null) {
    const node = space.nodes.find((n) => n.id === waypoint)
    if (node) {
      const p = project(node.at, at, radius)
      // The waypoint is the exception to the off-map rule, and deliberately so: it is the one
      // blip whose *direction* matters more than its position, and a waypoint that vanishes when
      // you zoom in is a waypoint that stops guiding you at the moment it should be guiding you
      // most. Clamped to the rim, and the component draws it as an arrow rather than a dot.
      const scale = Math.max(1, Math.abs(p.x), Math.abs(p.y))
      blips.push({
        x: p.x / scale,
        y: p.y / scale,
        above: p.above,
        kind: 'waypoint',
        tone: node.kind,
        label: node.label,
        id: node.id,
      })
    }
  }

  // Heading on the map plane. `atan2(x, z)` rather than `(z, x)` so zero points up the screen,
  // which is the direction the map draws as "ahead".
  return { blips, radius, heading: Math.atan2(facing.x, facing.z) }
}

/**
 * The node nearest a click, or null when the click was on empty space.
 *
 * `mx`/`my` are map coordinates in −1..1. The tolerance is generous because the alternative is a
 * map you have to aim at, and a control that punishes imprecision is one people stop using.
 */
export function pick(view: MapView, mx: number, my: number, tolerance = 0.06): Blip | null {
  let best: Blip | null = null
  let bestD = tolerance
  for (const b of view.blips) {
    if (b.id === null) continue
    const d = Math.hypot(b.x - mx, b.y - my)
    if (d < bestD) {
      bestD = d
      best = b
    }
  }
  return best
}

/** Map scale as a readable string, so the reader knows what a square is worth. */
export function scaleLabel(radius: number): string {
  return radius >= 1e9 ? `${(radius / 1e9).toFixed(1)}Gm` : `${Math.round(radius / 1e6)}Mm`
}

/** Whether a node offers anything, so the map can mark the ones worth flying to. */
export function serviced(n: Node): boolean {
  return servicesOf(n.kind).length > 0
}
