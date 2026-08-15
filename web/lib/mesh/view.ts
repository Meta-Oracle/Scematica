// Pure view logic for the mesh: where each node sits, and what colour it earns.
//
// Everything here is a pure function of the payload. No React, no DOM, no fetch — so the
// layout arithmetic and, more importantly, the colour *rules* are testable without a
// browser (`npm run check:mesh`).
//
// THE COLOUR RULE, which is the part worth protecting. Colour on this page is a claim
// about how much the reader may trust a number, and it is assigned in one place — here —
// so it cannot drift per component:
//
//   live      the unit is reporting now; its numbers are actionable
//   stale     the unit reported once and has gone quiet; numbers are history
//   absent    the unit cannot be seen at all; there are no numbers
//   veto      this unit is actively stopping the system
//
// Provenance outranks verdict for everything except an active veto from a live source.
// A stale node that says "PASS" has not passed anything recently, and painting it the
// same green as a live pass is the exact error the whole feature exists to prevent.

import type { Mesh, MeshEdge, MeshNode, NodeKind, Provenance } from './types'

/** Column index per node kind. Mirrors `NodeKind::layer()` in Rust, which is
 *  authoritative. Declared as a total Record so adding a kind to `NodeKind` without
 *  placing it here is a compile error rather than a node silently stacking in column 0. */
export const LAYER: Record<NodeKind, number> = {
  listener: 0,
  filter: 1,
  scorer: 1,
  breaker: 2,
  learner: 3,
  reasoner: 3,
  gate: 3,
  executor: 4,
  peer: 5,
}

export const LAYER_TITLES = ['Ingest', 'Filter', 'Risk', 'Cognition', 'Execution', 'Mesh']

/** The semantic tones. Names, never hex — hex lives in the palette map below and in
 *  `tailwind.config.ts`, and render code must ask for a tone. */
export type Tone = 'live' | 'stale' | 'absent' | 'veto' | 'simulated'

export const TONE_HEX: Record<Tone, string> = {
  live: '#4ade9b',
  stale: '#f5b544',
  absent: '#3a4266',
  veto: '#ff5d7d',
  simulated: '#7c9cff',
}

export function statusOf(p: Provenance): 'live' | 'stale' | 'absent' | 'simulated' {
  return p.kind
}

/**
 * The tone a node earns.
 *
 * An active veto is the only condition allowed to override provenance, and only from a
 * live source: a veto recovered from a three-month-old file is history, and painting it
 * alarm-red sends an operator hunting a gate that may have opened long ago.
 */
export function toneFor(node: MeshNode): Tone {
  if (node.verdict === 'veto' && node.provenance.kind === 'live') return 'veto'
  return statusOf(node.provenance)
}

/** Human age for a provenance, or null when there is nothing to age. */
export function ageLabel(p: Provenance): string | null {
  if (p.kind === 'absent' || p.kind === 'simulated') return null
  return humanise(p.age_secs)
}

export function humanise(secs: number): string {
  if (secs < 60) return `${Math.floor(secs)}s`
  if (secs < 3600) return `${Math.floor(secs / 60)}m`
  if (secs < 86400) return `${Math.floor(secs / 3600)}h`
  return `${Math.floor(secs / 86400)}d`
}

/**
 * Is this edge actively stopping flow?
 *
 * Tri-state on purpose. `null` (unreadable) must reach the renderer intact so it can draw
 * a dashed "unknown" gate rather than an open one — an unexamined veto is not a cleared
 * veto, the same rule the Rust `Edge::is_blocking` follows.
 */
export function edgeBlocking(edge: MeshEdge): boolean | null {
  if (edge.kind !== 'veto') return false
  return edge.active
}

// ── layout ───────────────────────────────────────────────────────────────────

export interface Placed {
  node: MeshNode
  x: number
  y: number
}

export interface PlacedEdge {
  edge: MeshEdge
  x1: number
  y1: number
  x2: number
  y2: number
  /** True when the edge points leftwards, i.e. against the flow — feedback, not signal. */
  backwards: boolean
}

export interface Layout {
  placed: Placed[]
  edges: PlacedEdge[]
  width: number
  height: number
  /** Column x-centres that actually contain nodes, for the header rail. */
  columns: { layer: number; x: number; title: string }[]
}

export const NODE_W = 168
export const NODE_H = 62
const COL_GAP = 92
const ROW_GAP = 22

/**
 * Place nodes in columns by layer, stacked and vertically centred within each column.
 *
 * Deterministic and order-stable: nodes keep the order the collector emitted them in, so
 * a node does not jump rows between polls when an unrelated one appears. Layout that
 * reshuffles on every refresh is unreadable regardless of how good it looks in a
 * screenshot.
 */
export function layout(mesh: Mesh): Layout {
  const byLayer = new Map<number, MeshNode[]>()
  for (const n of mesh.nodes) {
    const l = LAYER[n.kind] ?? 0
    const arr = byLayer.get(l)
    if (arr) arr.push(n)
    else byLayer.set(l, [n])
  }

  const layers = [...byLayer.keys()].sort((a, b) => a - b)
  const tallest = Math.max(1, ...layers.map(l => byLayer.get(l)!.length))
  const height = tallest * (NODE_H + ROW_GAP) + ROW_GAP + 34
  const width = layers.length * (NODE_W + COL_GAP) + COL_GAP

  const pos = new Map<string, { x: number; y: number }>()
  const placed: Placed[] = []
  const columns: Layout['columns'] = []

  layers.forEach((l, col) => {
    const nodes = byLayer.get(l)!
    const x = COL_GAP / 2 + col * (NODE_W + COL_GAP)
    const colHeight = nodes.length * (NODE_H + ROW_GAP) - ROW_GAP
    const top = 34 + (height - 34 - colHeight) / 2
    columns.push({ layer: l, x: x + NODE_W / 2, title: LAYER_TITLES[l] ?? `L${l}` })
    nodes.forEach((n, i) => {
      const y = top + i * (NODE_H + ROW_GAP)
      pos.set(n.id, { x, y })
      placed.push({ node: n, x, y })
    })
  })

  const edges: PlacedEdge[] = []
  for (const e of mesh.edges) {
    const a = pos.get(e.from)
    const b = pos.get(e.to)
    // A dangling edge is dropped rather than drawn into empty space. The Rust
    // `Mesh::validate` is what should have caught it; silently drawing a line to (0,0)
    // would make a broken topology look like a real connection.
    if (!a || !b) continue
    const backwards = b.x < a.x
    edges.push({
      edge: e,
      x1: a.x + NODE_W,
      y1: a.y + NODE_H / 2,
      x2: b.x,
      y2: b.y + NODE_H / 2,
      backwards,
    })
  }

  return { placed, edges, width, height, columns }
}

/** Cubic bezier between two placed points, flattened horizontally so parallel edges in
 *  the same channel stay distinguishable rather than overlapping as straight lines. */
export function edgePath(e: PlacedEdge): string {
  const dx = Math.max(36, Math.abs(e.x2 - e.x1) * 0.5)
  return `M ${e.x1} ${e.y1} C ${e.x1 + dx} ${e.y1}, ${e.x2 - dx} ${e.y2}, ${e.x2} ${e.y2}`
}

/** The single line that belongs above the graph. */
export function visibilityLabel(mesh: Mesh): string {
  const s = mesh.summary
  const pct = Math.round(s.visibility * 100)
  return `${pct}% visible · ${s.nodes_live} live · ${s.nodes_stale} stale · ${s.nodes_absent} unseen`
}
