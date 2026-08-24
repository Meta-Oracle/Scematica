'use client'

import { useMemo } from 'react'

import {
  CENTER,
  PSI_ARC,
  RADIUS,
  READOUT,
  VIEW,
  arcPath,
  polar,
  sigilView,
  ticks,
  type SigilTelemetry,
} from '@/lib/scylar/sigil'

// The instrument ring drawn around the portrait.
//
// Everything it shows is a real reading: Ψ from the gate, coverage from the last omni
// result, one node per subsystem with its actual state, and a trace of real token arrivals.
// Nothing here animates to look busy — see the header of `lib/scylar/sigil.ts` for why an
// idle trace that wiggles would be a fabricated readout, and why an unmeasured gauge draws a
// dashed full sweep rather than a zero-length arc.
//
// ## Why the geometry is not in this file
//
// Same split as `expressions.ts` and the avatar: `sigil.ts` is pure and testable without a
// browser, this is placement. A path string built inline here would be untestable, and the
// one rule worth testing — that unmeasured and measured-zero produce visibly different
// output — is precisely the kind that rots inside a component.
//
// ## Why the animation is in CSS
//
// Three reasons. It composites on the GPU without a rAF loop, so the ring costs nothing
// while she is idle; `prefers-reduced-motion` can switch the whole thing off in one place;
// and the palette rule holds — this file names classes and roles, `globals.css` owns every
// hex. The durations arrive as custom properties because they are *derived from state*
// (`motionFor`), which CSS cannot compute.
//
// SMIL is deliberately avoided: `<animateTransform>` ignores `prefers-reduced-motion`
// entirely, and an operator who asked the OS for less motion should not have to argue with
// a decorative ring about it.

interface Props {
  telemetry: SigilTelemetry
  /** Matches the portrait's rendered width; the ring is drawn in its own square. */
  size: number
  reduceMotion: boolean
}

export function ScylarSigil({ telemetry, size, reduceMotion }: Props) {
  const view = useMemo(() => sigilView(telemetry), [telemetry])

  // Static geometry — no state in it, so it never rebuilds.
  const tickMarks = useMemo(() => ticks(60, RADIUS.ticks, 4), [])

  const { motion, psi } = view

  // The pulse ring sweeps outward from the portrait edge. One element; the keyframes carry
  // the travel, so nothing here has to know how far.
  const pulsing = motion.pulse && !reduceMotion

  return (
    <svg
      className="scylar-sigil"
      viewBox={`0 0 ${VIEW} ${VIEW}`}
      width={size}
      height={size}
      // Decorative in the accessibility tree: every value it shows is also stated in the
      // readout line under the portrait and in the terminal's own badges. Announcing a
      // rotating ring of two-letter labels would be noise, not information.
      aria-hidden
      focusable="false"
      style={
        {
          '--sigil-spin': `${motion.spinSecs}s`,
          '--sigil-counter': `${motion.counterSecs}s`,
          '--sigil-intensity': reduceMotion ? 0.4 : motion.intensity,
        } as React.CSSProperties
      }
      data-motion={reduceMotion ? 'reduced' : 'full'}
      data-status={view.status}
    >
      <defs>
        {/* Radial falloff for the inner field. Stop colours come from CSS variables so the
            palette stays in one file — `stopColor` on a var() is well supported and is the
            only way to keep a gradient theme-aware without duplicating the hexes here. */}
        <radialGradient id="scylar-sigil-field" cx="50%" cy="50%" r="50%">
          <stop offset="55%" stopColor="var(--s-violet)" stopOpacity="0" />
          <stop offset="88%" stopColor="var(--s-violet)" stopOpacity="0.10" />
          <stop offset="100%" stopColor="var(--s-violet)" stopOpacity="0" />
        </radialGradient>

        {/* The rotating conic wedge that reads as a scanner sweep. A gradient rather than a
            drawn shape so it fades at both edges instead of ending in a hard line. */}
        <linearGradient id="scylar-sigil-sweep" x1="0" y1="0" x2="1" y2="0">
          <stop offset="0%" stopColor="var(--s-violet-hi)" stopOpacity="0" />
          <stop offset="100%" stopColor="var(--s-violet-hi)" stopOpacity="0.55" />
        </linearGradient>

        <filter id="scylar-sigil-glow" x="-40%" y="-40%" width="180%" height="180%">
          <feGaussianBlur stdDeviation="1.6" result="blur" />
          <feMerge>
            <feMergeNode in="blur" />
            <feMergeNode in="SourceGraphic" />
          </feMerge>
        </filter>
      </defs>

      <circle cx={CENTER} cy={CENTER} r={RADIUS.outer} fill="url(#scylar-sigil-field)" />

      {/* ── outer ring: ticks, rotating ──────────────────────────────────────── */}
      <g className="scylar-sigil-spin" style={{ transformOrigin: `${CENTER}px ${CENTER}px` }}>
        <circle
          className="scylar-sigil-rim"
          cx={CENTER}
          cy={CENTER}
          r={RADIUS.outer}
          fill="none"
        />
        {tickMarks.map((t, i) => (
          <line
            key={i}
            className={t.major ? 'scylar-sigil-tick is-major' : 'scylar-sigil-tick'}
            x1={t.x1}
            y1={t.y1}
            x2={t.x2}
            y2={t.y2}
          />
        ))}
        {/* The sweep: a quarter-arc of the rim, brightened. Rides the same rotation, so it
            never drifts out of phase with the ticks it is passing over. */}
        <path
          className="scylar-sigil-sweep"
          d={arcPath(RADIUS.ticks, 0, 88)}
          fill="none"
          stroke="url(#scylar-sigil-sweep)"
        />
      </g>

      {/* ── inner ring: counter-rotating, dashed ─────────────────────────────── */}
      <g className="scylar-sigil-counter" style={{ transformOrigin: `${CENTER}px ${CENTER}px` }}>
        <circle
          className="scylar-sigil-dash"
          cx={CENTER}
          cy={CENTER}
          r={RADIUS.inner}
          fill="none"
        />
      </g>

      {/* ── Ψ gauge ──────────────────────────────────────────────────────────── */}
      <path className="scylar-sigil-track" d={psi.track} fill="none" />
      {psi.d && (
        <path
          className={psi.ghost ? 'scylar-sigil-gauge is-ghost' : 'scylar-sigil-gauge'}
          d={psi.d}
          fill="none"
          filter={psi.ghost ? undefined : 'url(#scylar-sigil-glow)'}
        />
      )}
      {/* The cap dot marks where a measured value ended. Omitted for a ghost, because a cap
          on a full dashed sweep would read as Ψ = 1.00 — the most misleading possible value
          to imply for a term nobody measured. */}
      {psi.measured && psi.d && (
        <circle
          className="scylar-sigil-cap"
          r={2.4}
          {...capAt(psi.label)}
        />
      )}

      {/* ── channel nodes ────────────────────────────────────────────────────── */}
      {view.channels.map((c) => (
        <g key={c.id} className="scylar-sigil-node" data-role={c.role}>
          <circle cx={c.x} cy={c.y} r={7} className="scylar-sigil-node-ring" />
          <circle cx={c.x} cy={c.y} r={2.6} className="scylar-sigil-node-core" />
          <text x={c.x} y={c.y + 15} className="scylar-sigil-node-label" textAnchor="middle">
            {c.label}
          </text>
        </g>
      ))}

      {/* ── readout stack ────────────────────────────────────────────────────
          Trace, coverage, status — top to bottom in the column the Ψ arc leaves open at
          the bottom, and the column `CHANNEL_START` keeps the channel nodes out of. The
          y positions live in `READOUT` so the two files cannot drift into a collision. */}
      <g transform={`translate(${CENTER - READOUT.trace.w / 2}, ${READOUT.trace.y - READOUT.trace.h / 2})`}>
        {/* Token-arrival trace. Flat when nothing has streamed — see `tracePoints`. */}
        <polyline className="scylar-sigil-trace" points={view.trace} fill="none" />
      </g>

      <text x={CENTER} y={READOUT.status.y} className="scylar-sigil-status" textAnchor="middle">
        {view.status}
      </text>

      {/* Ψ readout. `—` when unmeasured, and it is the text that disambiguates a measured
          zero from an unmeasured term — the arc alone cannot. */}
      <text x={CENTER} y={READOUT.psi.y} className="scylar-sigil-psi" textAnchor="middle">
        Ψ {psi.label}
      </text>

      {/* Coverage: one cell per term, never a proportional bar. `∅` for absent, which is a
          different statement from 0/9 and has to look like one. */}
      <g transform={`translate(${CENTER}, ${READOUT.coverage.y})`}>
        {view.coverage ? (
          <>
            {view.coverage.cells.map((filled, i) => {
              const span = view.coverage!.cells.length
              const w = 4
              const gap = 1.6
              const x = -((span * (w + gap) - gap) / 2) + i * (w + gap)
              return (
                <rect
                  key={i}
                  x={x}
                  y={-3}
                  width={w}
                  height={6}
                  className={filled ? 'scylar-sigil-cell is-filled' : 'scylar-sigil-cell'}
                />
              )
            })}
            <text x={0} y={14} className="scylar-sigil-cov" textAnchor="middle">
              {view.coverage.label}
            </text>
          </>
        ) : (
          <text x={0} y={3} className="scylar-sigil-cov" textAnchor="middle">
            ∅
          </text>
        )}
      </g>

      {pulsing && (
        <circle
          className="scylar-sigil-pulse"
          cx={CENTER}
          cy={CENTER}
          r={RADIUS.channels}
          fill="none"
        />
      )}
    </svg>
  )
}

/**
 * Where the value arc ended, for the cap dot.
 *
 * Recomputed from the label rather than threaded through `Gauge`: the label is the value,
 * formatted by the one function allowed to format it, so deriving the position from it means
 * the dot cannot land somewhere the text disagrees with. A ghost never reaches here.
 */
function capAt(label: string): { cx: number; cy: number } {
  const v = Number(label)
  const sweep = PSI_ARC.end - PSI_ARC.start
  const p = polar(RADIUS.gauge, PSI_ARC.start + sweep * (Number.isFinite(v) ? v : 0))
  return { cx: p.x, cy: p.y }
}
