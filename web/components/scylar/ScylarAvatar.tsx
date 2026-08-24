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
import type { SigilTelemetry } from '@/lib/scylar/sigil'
import { ScylarSigil } from './ScylarSigil'

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
  /**
   * Generated portraits to use in place of the sprites, per expression.
   *
   * A *source* override and nothing more: the state machine still chooses which
   * expression is showing and how she is posed, and a missing entry falls back to the
   * sprite for that expression. Keeping generation out of the timing path is what stops
   * a network call that takes seconds from being able to stutter a 90ms mouth flap. See
   * `lib/scylar/usePortraits.ts`.
   */
  portraits?: Partial<Record<Expression, string>>
  /**
   * Live readings for the instrument ring, minus the phase — which this component already
   * owns and would otherwise be passed twice, with two chances to disagree.
   *
   * Optional: with no telemetry the ring is not drawn at all. Drawing an empty one would
   * mean rendering an unmeasured Ψ, a `∅` coverage and four dark channels around a portrait
   * on a page that simply never wired it up, which reads as a system in trouble rather than
   * as one nobody asked for a reading from.
   */
  telemetry?: Omit<SigilTelemetry, 'phase'>
}

export function ScylarAvatar({ phase, size = 420, portraits, telemetry }: Props) {
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
      // `size` is the natural width; `max-width: 100%` lets it give way on a narrow
      // chassis, and `aspect-ratio` keeps it square as it does.
      //
      // Not `width: 100%; max-width: size` — that collapses. This sits in a shrink-to-fit
      // flex column, so the column sizes to its content while the content asks for a
      // percentage of the column: circular, resolved as zero, and the portrait vanishes
      // into a 130px sliver.
      style={
        {
          width: size,
          maxWidth: '100%',
          height: 'auto',
          aspectRatio: '1 / 1',
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
        {EXPRESSIONS.map((expression) => {
          // A generated portrait is a single object URL with no 2x variant, so `srcSet`
          // is dropped rather than pointed at the sprite — mixing the two would let the
          // browser pick the sprite on a high-DPI screen and quietly undo the override.
          const generated = portraits?.[expression]
          return (
          <img
            key={expression}
            src={generated ?? spriteSrc(expression, 512)}
            srcSet={
              generated
                ? undefined
                : `${spriteSrc(expression, 512)} 1x, ${spriteSrc(expression, 1024)} 2x`
            }
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
          )
        })}
      </div>

      {/* The instrument ring. Absolutely positioned over the whole portrait rather than
          around it, because the stage is a fixed square and a ring drawn outside it would
          be clipped by the panel — the outer radius is inset far enough that it reads as a
          frame on the portrait rather than a halo behind it.

          Mounted between the sprites and the holo layer so the scanlines pass over it: the
          ring is part of the projection, not an overlay on the page. */}
      {telemetry && (
        <div className="scylar-sigil-layer absolute inset-0" aria-hidden>
          <ScylarSigil
            telemetry={{ ...telemetry, phase }}
            size={size}
            reduceMotion={reduceMotion}
          />
        </div>
      )}

      {/* The projection artefacts: scanlines, the interlace sweep, and the chromatic
          fringe. A real element rather than a third pseudo-element because
          `.scylar-portrait` has already spent `::before` on the rim light and `::after`
          on the vignette — and because this layer must sit *above* the cross-fading
          sprites while those two bracket them. */}
      <div className="scylar-holo" aria-hidden />

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
