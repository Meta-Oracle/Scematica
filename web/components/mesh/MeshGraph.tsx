'use client'

import { useMemo } from 'react'

import {
  NODE_H,
  NODE_W,
  TONE_HEX,
  ageLabel,
  edgeBlocking,
  edgeKey,
  edgePath,
  layout,
  toneFor,
  trace,
} from '@/lib/mesh/view'
import type { Mesh, MeshEdge, MeshNode } from '@/lib/mesh/types'

// The graph itself. Presentational and controlled — it owns no data and no timer.
//
// Three rendering rules that carry meaning and must not be traded for looks:
//
//   1. An ABSENT node is drawn as a hole: dashed outline, no fill, no values. It must not
//      resemble a node that is present and quiet, because "unseen" and "idle" are the two
//      states this whole feature exists to keep apart.
//   2. An edge with `active === null` is drawn distinctly from `active === false`. An
//      unexamined veto is not a cleared one.
//   3. Motion is decoration only. Everything a moving element says is also written in
//      text somewhere, so reduced-motion and a screenshot lose nothing.

export interface MeshGraphProps {
  mesh: Mesh
  selected: string | null
  onSelect: (id: string | null) => void
}

export function MeshGraph({ mesh, selected, onSelect }: MeshGraphProps) {
  const { placed, edges, width, height, columns } = useMemo(() => layout(mesh), [mesh])

  // Selecting a node traces every unit that can reach it and every unit it can reach,
  // dimming the rest. On 22 nodes this is the difference between seeing the topology and
  // following it — select the Executor and what remains lit is exactly the set of units
  // with any say in whether a trade happened.
  const traced = useMemo(() => (selected ? trace(mesh, selected) : null), [mesh, selected])
  const nodeLit = (id: string) => !traced || traced.nodes.has(id)
  const edgeLit = (k: string) => !traced || traced.edges.has(k)

  return (
    <div className="overflow-x-auto">
      <svg
        viewBox={`0 0 ${width} ${height}`}
        width={width}
        height={height}
        role="img"
        aria-label="Scematica mesh topology"
        className="max-w-none"
      >
        <defs>
          <marker id="mesh-arrow" viewBox="0 0 8 8" refX="7" refY="4" markerWidth="6" markerHeight="6" orient="auto">
            <path d="M0,0 L8,4 L0,8 z" fill="#354a7d" />
          </marker>
          <marker id="mesh-arrow-block" viewBox="0 0 8 8" refX="7" refY="4" markerWidth="6" markerHeight="6" orient="auto">
            <path d="M0,0 L8,4 L0,8 z" fill={TONE_HEX.veto} />
          </marker>
        </defs>

        {columns.map(c => (
          <text
            key={c.layer}
            x={c.x}
            y={18}
            textAnchor="middle"
            fill="#545f8a"
            fontSize="10"
            letterSpacing="1.5"
          >
            {c.title.toUpperCase()}
          </text>
        ))}

        <g>
          {edges.map((pe, i) => (
            <g key={`${edgeKey(pe.edge)}-${i}`} opacity={edgeLit(edgeKey(pe.edge)) ? 1 : 0.12}>
              <EdgeLine d={edgePath(pe)} edge={pe.edge} />
            </g>
          ))}
        </g>

        <g>
          {placed.map(p => (
            <g key={p.node.id} opacity={nodeLit(p.node.id) ? 1 : 0.15}>
              <NodeBox
                node={p.node}
                x={p.x}
                y={p.y}
                selected={selected === p.node.id}
                onSelect={onSelect}
              />
            </g>
          ))}
        </g>
      </svg>
    </div>
  )
}

function EdgeLine({ d, edge }: { d: string; edge: MeshEdge }) {
  const blocking = edgeBlocking(edge)

  if (blocking === true) {
    return (
      <g className="mesh-edge-block">
        <path d={d} fill="none" stroke={TONE_HEX.veto} strokeWidth={2.5} markerEnd="url(#mesh-arrow-block)" />
      </g>
    )
  }

  // Unknown veto: dashed and grey, with no arrowhead. Visually "we did not look",
  // deliberately different from both a cleared gate and an active one.
  if (blocking === null) {
    return <path d={d} fill="none" stroke="#3a4266" strokeWidth={1.2} strokeDasharray="2 6" />
  }

  if (edge.kind === 'veto') {
    return <path d={d} fill="none" stroke="#1e2a4d" strokeWidth={1} />
  }

  if (edge.kind === 'promotion') {
    return <path d={d} fill="none" stroke={edge.active ? '#7c9cff' : '#1e2a4d'} strokeWidth={edge.active ? 1.8 : 1} strokeDasharray="6 4" />
  }

  if (edge.kind === 'gate') {
    return <path d={d} fill="none" stroke="#354a7d" strokeWidth={1.2} strokeDasharray="1 5" />
  }

  if (edge.kind === 'experience') {
    // Always inactive today — nothing wires it. Drawn faint so the shape of the future
    // architecture is visible without implying it is running.
    return <path d={d} fill="none" stroke="#252d4d" strokeWidth={1} strokeDasharray="4 8" opacity={0.5} />
  }

  return (
    <path
      d={d}
      fill="none"
      stroke={edge.active ? '#2f6ea8' : '#1e2a4d'}
      strokeWidth={edge.active ? 1.6 : 1}
      markerEnd="url(#mesh-arrow)"
      className={edge.active ? 'mesh-edge-flow' : undefined}
    />
  )
}

function NodeBox({
  node,
  x,
  y,
  selected,
  onSelect,
}: {
  node: MeshNode
  x: number
  y: number
  selected: boolean
  onSelect: (id: string | null) => void
}) {
  const tone = toneFor(node)
  const hex = TONE_HEX[tone]
  const absent = node.provenance.kind === 'absent'
  const age = ageLabel(node.provenance)

  return (
    <g
      transform={`translate(${x},${y})`}
      onClick={() => onSelect(selected ? null : node.id)}
      style={{ cursor: 'pointer' }}
      className={tone === 'live' ? 'mesh-node-live' : undefined}
    >
      <rect
        width={NODE_W}
        height={NODE_H}
        rx={3}
        fill={absent ? 'transparent' : '#0a0e1f'}
        stroke={selected ? '#52e5ff' : hex}
        strokeWidth={selected ? 2 : absent ? 1 : 1.4}
        strokeDasharray={absent ? '3 4' : undefined}
        opacity={absent ? 0.55 : 1}
      />

      {/* Tone bar: the fastest read on the card, and it encodes provenance, not health. */}
      <rect x={0} y={0} width={3} height={NODE_H} fill={hex} opacity={absent ? 0.5 : 1} />

      <text x={12} y={20} fill={absent ? '#545f8a' : '#dfe6ff'} fontSize="12">
        {node.label.length > 20 ? `${node.label.slice(0, 19)}…` : node.label}
      </text>

      <text x={12} y={36} fill={hex} fontSize="9" letterSpacing="1">
        {absent ? 'UNSEEN' : node.verdict.toUpperCase()}
        {age ? ` · ${age}` : ''}
      </text>

      {/* Activity bar only when a value was measured. `null` renders nothing at all — an
          empty bar would read as "measured, and it is zero". */}
      {node.activity !== null && !absent && (
        <>
          <rect x={12} y={46} width={NODE_W - 24} height={3} fill="#131a33" />
          <rect
            x={12}
            y={46}
            width={Math.max(0, Math.min(1, node.activity)) * (NODE_W - 24)}
            height={3}
            fill={hex}
          />
        </>
      )}
    </g>
  )
}
