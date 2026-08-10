'use client'

import { useEffect, useMemo, useRef, useState } from 'react'

import {
  EXPRESSIONS,
  EXPRESSION_CROSSFADE_MS,
  FLAP_CROSSFADE_MS,
  PRESENCE_EASE_MS,
  REACTION_HOLD_MS,
  type AvatarPhase,
  type Expression,
  presenceFor,
  spriteFor,
  spriteSrc,
} from '@/lib/scylar/expressions'

// The portrait. All three sprites are mounted at once and cross-faded by opacity —
// never swapped by changing `src`.
//
// That is the whole trick: a `src` swap on a 200 KB image shows a blank frame while the
// browser fetches and decodes, which at speech rate is a strobe. Stacked layers make
// every sprite decoded and resident before the first flap, and the transition becomes a
// GPU-composited opacity change that never touches layout.
//
// Two animations run at different speeds and must not be merged. The *flap* is fast and
// cyclic (which sprite). The *presence* — scale, lift, glow — is slow and one-shot (how
// she carries herself). Driving both off one duration is what makes an avatar look
// either twitchy or sedated; see `presenceFor` for why the split lives in the state
// machine rather than here.

interface Props {
  phase: AvatarPhase
  /** Rendered width in CSS pixels. The 1024 asset covers 2x displays via srcSet. */
  size?: number
}

export function ScylarAvatar({ phase, size = 420 }: Props) {
  const [flapSprite, setFlapSprite] = useState<Expression>('idle')
  const [settledFor, setSettledFor] = useState(0)
  const reduceMotion = usePrefersReducedMotion()

  // Drive the mouth flap from rAF while streaming. Timers stop the moment streaming
  // ends, so an idle page runs no animation loop at all.
  //
  // The loop stores the *sprite*, not the elapsed time. Elapsed time changes every
  // frame and would re-render at 60Hz for a value that only changes ~11 times a second;
  // storing the derived string lets React bail out on the frames where nothing moved.
  const streaming = phase.kind === 'streaming'
  useEffect(() => {
    if (!streaming || reduceMotion) return
    let raf = 0
    const start = performance.now()
    const loop = (now: number) => {
      setFlapSprite(spriteFor({ kind: 'streaming', elapsedMs: now - start }))
      raf = requestAnimationFrame(loop)
    }
    raf = requestAnimationFrame(loop)
    return () => cancelAnimationFrame(raf)
  }, [streaming, reduceMotion])

  // Speech-driven mouth. One open-close per word, restarted on each boundary — the
  // terminal pushes a new phase object per word, so `phase` in the dependency list *is*
  // the word change. Depending on the object identity is deliberate here and wrong for
  // the timer flap above, which must not restart on every render.
  useEffect(() => {
    if (phase.kind !== 'voicing' || reduceMotion) return
    const { wordMs, sinceWordMs } = phase
    let raf = 0
    const start = performance.now()
    const loop = (now: number) => {
      setFlapSprite(
        spriteFor({ kind: 'voicing', sinceWordMs: sinceWordMs + (now - start), wordMs }),
      )
      raf = requestAnimationFrame(loop)
    }
    raf = requestAnimationFrame(loop)
    return () => cancelAnimationFrame(raf)
  }, [phase, reduceMotion])

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
      if (phase.kind === 'streaming' || phase.kind === 'voicing') return 'talking'
      if (phase.kind === 'settled') return phase.positive && settledFor === 0 ? 'joyous' : 'idle'
      return 'idle'
    }
    switch (phase.kind) {
      case 'streaming':
      case 'voicing':
        return flapSprite
      case 'settled':
        return spriteFor({ kind: 'settled', positive: phase.positive, sinceMs: settledFor })
      default:
        return spriteFor(phase)
    }
  }, [phase, flapSprite, settledFor, reduceMotion])

  const presence = useMemo(
    () =>
      presenceFor(
        phase.kind === 'settled'
          ? { kind: 'settled', positive: phase.positive, sinceMs: settledFor }
          : phase,
      ),
    [phase, settledFor],
  )

  // Fast inside the flap cycle, slow for a change of mood. Using the slow duration
  // while the mouth is cycling would leave both sprites permanently half-lit.
  const fadeMs =
    reduceMotion
      ? 0
      : streaming || phase.kind === 'voicing'
        ? FLAP_CROSSFADE_MS
        : EXPRESSION_CROSSFADE_MS

  return (
    <div
      className="scylar-portrait relative select-none"
      data-phase={phase.kind}
      data-expression={active}
      // `--s-glow` is read by `.scylar-portrait::before` for the halo. Passing it as a
      // variable keeps the glow's colour and falloff a CSS concern; this component only
      // states how strongly she is lit.
      style={
        {
          width: size,
          height: size,
          '--s-glow': reduceMotion ? 0.2 : presence.glow,
        } as React.CSSProperties
      }
    >
      {/* Separate element from the glow host so the transform does not also scale the
          halo's blur radius, which would make the glow pump with every pose change. */}
      <div
        className="scylar-portrait-body absolute inset-0"
        style={{
          transform: `translateY(${reduceMotion ? 0 : presence.lift}px) scale(${
            reduceMotion ? 1 : presence.scale
          })`,
          transition: reduceMotion
            ? 'none'
            : `transform ${PRESENCE_EASE_MS}ms cubic-bezier(0.22, 1, 0.36, 1)`,
        }}
      >
        {EXPRESSIONS.map((expression) => (
          <img
            key={expression}
            src={spriteSrc(expression, 512)}
            srcSet={`${spriteSrc(expression, 512)} 1x, ${spriteSrc(expression, 1024)} 2x`}
            alt={expression === active ? `Scylar, ${expression}` : ''}
            // Only the visible layer is announced; the other two are decorative
            // duplicates of the same character and would be read out as repeated images.
            aria-hidden={expression !== active}
            draggable={false}
            width={size}
            height={size}
            className="absolute inset-0 h-full w-full object-cover"
            style={{
              opacity: expression === active ? 1 : 0,
              transition: `opacity ${fadeMs}ms ease-in-out`,
            }}
          />
        ))}
      </div>

      {/* Thinking indicator. She holds a closed mouth while waiting on the model, so
          without this the UI looks frozen between send and first token. */}
      {phase.kind === 'thinking' && (
        <div className="absolute bottom-3 left-1/2 z-10 flex -translate-x-1/2 gap-1.5">
          {[0, 1, 2].map((i) => (
            <span
              key={i}
              className="scylar-think-dot h-1.5 w-1.5 rounded-full"
              style={{ animationDelay: `${i * 200}ms` }}
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
