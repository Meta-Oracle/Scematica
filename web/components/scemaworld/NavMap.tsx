'use client'

/**
 * The nav map. Places rectangles; decides nothing.
 *
 * All the geometry is in `lib/scemaworld/navmap.ts`, which is pure and tested. This file turns
 * blips into SVG and clicks back into node ids — the same split as `gl.ts` against `view.ts`, and
 * for the same reason: a map that silently mis-plots a station has to be catchable without a
 * browser.
 *
 * The one rule it carries on its own: **an off-plane blip is marked**. The map throws away the
 * vertical axis, and a station plotted next to you that is two hundred million units above is not
 * next to you. Every blip draws a tick whose length is its true offset, so "close on the map" and
 * "close" are visibly different claims.
 */

import { useMemo } from 'react'
import { build, pick, scaleLabel, ZOOMS, type Blip, type MapView } from '@/lib/scemaworld/navmap'
import type { Space } from '@/lib/scemaworld/generate'
import type { Faction } from '@/lib/scemaworld/factions'

/**
 * A blip's colour, by what it is.
 *
 * Mirrors `PALETTE` in `view.ts`, which stays the authority. A map that disagreed with the window
 * about who is friendly would be worse than no map at all.
 */
const TONE: Record<string, string> = {
  origin: '#a86bff',
  station: '#8cc7ff',
  dock: '#6bdbf2',
  depot: '#57add1',
  market: '#f2c761',
  derelict: '#8c8069',
  marker: '#4d475c',
  phantom: '#6b5c99',
  rift: '#70668f',
  raider: '#ff8c33',
  courier: '#59d9ff',
  freighter: '#5c8cf2',
  marshal: '#ffe64d',
}

export interface NavMapProps {
  space: Space
  at: { x: number; y: number; z: number }
  facing: { x: number; y: number; z: number }
  zoom: number
  waypoint: number | null
  craft: { id: string; at: { x: number; y: number; z: number }; faction: Faction; label: string }[]
  onPick: (nodeId: number) => void
  onZoom: (next: number) => void
}

const SIZE = 200

export function NavMap(props: NavMapProps) {
  const { space, at, facing, zoom, waypoint, craft, onPick, onZoom } = props

  const view: MapView = useMemo(
    () => build({ space, at, facing, zoom, waypoint, craft }),
    [space, at, facing, zoom, waypoint, craft],
  )

  // Map coordinates are −1..1 with +y as *north*; SVG y grows downward, so it is negated once
  // here rather than in the projection. Doing it in `navmap.ts` would bake a rendering
  // convention into geometry that a different surface might draw the other way up.
  const px = (b: Blip) => ({ cx: (b.x + 1) * (SIZE / 2), cy: (1 - b.y) * (SIZE / 2) })

  function onClick(e: React.MouseEvent<SVGSVGElement>) {
    const r = e.currentTarget.getBoundingClientRect()
    const mx = ((e.clientX - r.left) / r.width) * 2 - 1
    const my = 1 - ((e.clientY - r.top) / r.height) * 2
    const hit = pick(view, mx, my)
    if (hit && hit.id !== null) onPick(hit.id)
  }

  return (
    <div className="rounded border border-omni-border bg-black/85 p-2 font-mono text-[10px]">
      <div className="mb-1 flex items-baseline justify-between text-omni-dim">
        <span>NAV</span>
        <span>{scaleLabel(view.radius)}</span>
      </div>
      <svg
        width={SIZE}
        height={SIZE}
        viewBox={`0 0 ${SIZE} ${SIZE}`}
        onClick={onClick}
        className="cursor-crosshair"
      >
        <rect x={0} y={0} width={SIZE} height={SIZE} fill="#05040a" />
        {/* Range rings at a half and a quarter, so a distance can be read off rather than guessed. */}
        {[0.25, 0.5, 0.75, 1].map((r) => (
          <circle
            key={r}
            cx={SIZE / 2}
            cy={SIZE / 2}
            r={(SIZE / 2) * r}
            fill="none"
            stroke="#221d33"
            strokeWidth={1}
          />
        ))}
        <line x1={SIZE / 2} y1={0} x2={SIZE / 2} y2={SIZE} stroke="#181428" strokeWidth={1} />
        <line x1={0} y1={SIZE / 2} x2={SIZE} y2={SIZE / 2} stroke="#181428" strokeWidth={1} />

        {view.blips.map((b, i) => {
          const { cx, cy } = px(b)
          const tone = TONE[b.tone] ?? '#8888aa'
          if (b.kind === 'waypoint') {
            return (
              <g key={`wp${i}`}>
                <circle cx={cx} cy={cy} r={6} fill="none" stroke="#4dff73" strokeWidth={1.5} />
                <circle cx={cx} cy={cy} r={2} fill="#4dff73" />
              </g>
            )
          }
          // The off-plane tick. Its length is the true vertical offset, so a blip that looks
          // adjacent and is a long way above says so without needing to be read.
          const tick = Math.max(-1, Math.min(1, b.above)) * (SIZE / 2)
          return (
            <g key={i}>
              {Math.abs(b.above) > 0.04 && (
                <line
                  x1={cx}
                  y1={cy}
                  x2={cx}
                  y2={cy - tick}
                  stroke={tone}
                  strokeWidth={0.6}
                  opacity={0.35}
                />
              )}
              <circle
                cx={cx}
                cy={cy}
                r={b.kind === 'craft' ? 1.8 : 2.6}
                fill={b.kind === 'craft' ? tone : 'none'}
                stroke={tone}
                strokeWidth={1}
              >
                <title>{b.label}</title>
              </circle>
            </g>
          )
        })}

        {/* The ship, always dead centre, with a needle for heading. */}
        <g transform={`translate(${SIZE / 2} ${SIZE / 2}) rotate(${(-view.heading * 180) / Math.PI})`}>
          <path d="M 0 -7 L 4 5 L 0 2 L -4 5 Z" fill="#e8e4ff" />
        </g>
      </svg>
      <div className="mt-1 flex items-center justify-between text-omni-dim">
        <span>click a blip to set course</span>
        <span className="flex gap-1">
          {ZOOMS.map((_, i) => (
            <button
              key={i}
              type="button"
              onClick={() => onZoom(i)}
              className={`px-1 ${i === zoom ? 'text-omni-accent' : 'hover:text-omni-text'}`}
            >
              {i + 1}
            </button>
          ))}
        </span>
      </div>
    </div>
  )
}
