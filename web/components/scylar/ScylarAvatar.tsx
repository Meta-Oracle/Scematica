'use client'

import { useEffect, useMemo, useRef, useState } from 'react'

import {
  CROSSFADE_MS,
  EXPRESSIONS,
  REACTION_HOLD_MS,
  type AvatarPhase,
  type Expression,
  spriteFor,
  spriteSrc,
} from '@/lib/scylar/expressions'

// The portrait. All three sprites are mounted at once and cross-faded by opacity —
// never swapped by changing `src`.
//
// That is the whole trick: a `src` swap on a 200 KB image shows a blank frame while the
// browser fetches and decodes, which at 9 flaps/second is a strobe. Stacked layers make
// every sprite decoded and resident before the first flap, and the transition becomes a
// GPU-composited opacity change that never touches layout.

interface Props {
  phase: AvatarPhase
  /** Rendered width in CSS pixels. The 1024 asset covers 2x displays via srcSet. */
  size?: number
}

export function ScylarAvatar({ phase, size = 420 }: Props) {
  const [flapTick, setFlapTick] = useState(0)
  const [settledFor, setSettledFor] = useState(0)
  const reduceMotion = usePrefersReducedMotion()

  // Drive the mouth flap from rAF while streaming. Timers stop the moment streaming
  // ends, so an idle page runs no animation loop at all.
  const streaming = phase.kind === 'streaming'
  useEffect(() => {
    if (!streaming || reduceMotion) return
    let raf = 0
    const start = performance.now()
    const loop = (now: number) => {
      setFlapTick(now - start)
      raf = requestAnimationFrame(loop)
    }
    raf = requestAnimationFrame(loop)
    return () => cancelAnimationFrame(raf)
  }, [streaming, reduceMotion])

  // Decay the reaction back to idle. One timeout, not a loop — the only thing that
  // changes at the end of the hold is a single boolean.
  const settled = phase.kind === 'settled'
  const positive = settled && phase.positive
  useEffect(() => {
    if (!settled) {
      setSettledFor(0)
      return
    }
    setSettledFor(0)
    if (!positive) return
    const t = setTimeout(() => setSettledFor(REACTION_HOLD_MS), REACTION_HOLD_MS)
    return () => clearTimeout(t)
  }, [settled, positive])

  const active: Expression = useMemo(() => {
    // Reduced motion: keep the expression meaningful but hold it still. She still
    // reacts and still opens her mouth to speak — she just doesn't flap.
    if (reduceMotion) {
      if (phase.kind === 'streaming') return 'talking'
      if (phase.kind === 'settled') return phase.positive && settledFor === 0 ? 'joyous' : 'idle'
      return 'idle'
    }
    switch (phase.kind) {
      case 'streaming':
        return spriteFor({ kind: 'streaming', elapsedMs: flapTick })
      case 'settled':
        return spriteFor({ kind: 'settled', positive: phase.positive, sinceMs: settledFor })
      default:
        return spriteFor(phase)
    }
  }, [phase, flapTick, settledFor, reduceMotion])

  return (
    <div
      className="scylar-portrait relative select-none"
      style={{ width: size, height: size }}
      data-phase={phase.kind}
      data-expression={active}
    >
      {EXPRESSIONS.map((expression) => (
        <img
          key={expression}
          src={spriteSrc(expression, 512)}
          srcSet={`${spriteSrc(expression, 512)} 1x, ${spriteSrc(expression, 1024)} 2x`}
          alt={expression === active ? `Scylar, ${expression}` : ''}
          // Only the visible layer is announced; the other two are decorative duplicates
          // of the same character and would be read out as repeated images.
          aria-hidden={expression !== active}
          draggable={false}
          width={size}
          height={size}
          className="absolute inset-0 h-full w-full object-cover"
          style={{
            opacity: expression === active ? 1 : 0,
            transition: `opacity ${reduceMotion ? 0 : CROSSFADE_MS}ms linear`,
          }}
        />
      ))}

      {/* Thinking indicator. She holds a closed mouth while waiting on the model, so
          without this the UI looks frozen between send and first token. */}
      {phase.kind === 'thinking' && (
        <div className="absolute bottom-3 left-1/2 flex -translate-x-1/2 gap-1.5">
          {[0, 1, 2].map((i) => (
            <span
              key={i}
              className="scylar-think-dot h-1.5 w-1.5 rounded-full"
              style={{ animationDelay: `${i * 160}ms` }}
            />
          ))}
        </div>
      )}
    </div>
  )
}

/** Tracks `prefers-reduced-motion`, including changes made while the page is open. */
function usePrefersReducedMotion(): boolean {
  const [reduce, setReduce] = useState(false)
  const mq = useRef<MediaQueryList | null>(null)

  useEffect(() => {
    mq.current = window.matchMedia('(prefers-reduced-motion: reduce)')
    setReduce(mq.current.matches)
    const onChange = (e: MediaQueryListEvent) => setReduce(e.matches)
    mq.current.addEventListener('change', onChange)
    return () => mq.current?.removeEventListener('change', onChange)
  }, [])

  return reduce
}
