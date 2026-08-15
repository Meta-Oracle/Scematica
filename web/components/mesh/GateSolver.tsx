'use client'

import { useMemo, useState } from 'react'

import {
  TAU_PSI,
  TAU_PSI_FULL,
  dominantConstraint,
  effective,
  recompute,
  sensitivities,
  type Overrides,
} from '@/lib/mesh/gate'
import type { Cognition, Term } from '@/lib/mesh/types'

// The gate as an instrument rather than a readout.
//
// Ψ = C · K · (1 − R) is a pure function, so the browser can answer "what would it take to
// open this?" without a round trip and without the bot ever having to be in that state.
// Drag a term; Ψ moves; the sensitivity ranking reorders live.
//
// THE ONE RULE. A counterfactual must never be mistakable for an observation. The moment
// any override exists the panel changes colour, the heading changes word, every touched
// row is marked, and the observed value stays on screen beside the hypothetical one. The
// reset is always one click away and always visible.

const PSI_TRACK_W = 100

export function GateSolver({ cognition: c }: { cognition: Cognition }) {
  const [overrides, setOverrides] = useState<Overrides>({})
  const [coherence, setCoherence] = useState<number | undefined>(undefined)

  const live = useMemo(() => recompute(c, {}), [c])
  const now = useMemo(() => recompute(c, overrides, coherence), [c, overrides, coherence])
  const sens = useMemo(() => sensitivities(c, overrides), [c, overrides])
  const constraint = useMemo(() => dominantConstraint(c, overrides), [c, overrides])

  const dirty = now.dirty
  const terms = [...c.confidence_terms, ...c.risk.components]

  const set = (symbol: string, v: number) => setOverrides(o => ({ ...o, [symbol]: v }))
  const clear = (symbol: string) =>
    setOverrides(o => {
      const next = { ...o }
      delete next[symbol]
      return next
    })
  const reset = () => {
    setOverrides({})
    setCoherence(undefined)
  }

  return (
    <section
      className={`mx-5 mb-5 border ${dirty ? 'border-mesh-glow/70' : 'border-mesh-border'}`}
    >
      <header className="px-4 py-3 border-b border-mesh-border flex items-baseline justify-between gap-4 flex-wrap">
        <div>
          <span className={`text-[10px] uppercase tracking-wider ${dirty ? 'text-mesh-glow' : 'text-mesh-dim'}`}>
            {dirty ? 'Counterfactual — not observed state' : 'Gate solver · drag any term'}
          </span>
          <div className="text-mesh-muted text-[11px] mt-0.5">
            Ψ = C · K · (1 − R) recomputed in the browser. Nothing here touches the bot.
          </div>
        </div>
        {dirty && (
          <button
            onClick={reset}
            className="px-3 py-1 text-[10px] uppercase tracking-wider border border-mesh-glow/60 text-mesh-glow hover:bg-mesh-hi"
          >
            reset to observed
          </button>
        )}
      </header>

      {/* Ψ, with the observed value pinned beside it whenever they differ. */}
      <div className="px-4 py-3 border-b border-mesh-border">
        <div className="flex items-end gap-4 flex-wrap">
          <div>
            <div className="text-[10px] text-mesh-dim uppercase tracking-wider">Ψ</div>
            <div className={`text-3xl tabular-nums ${dirty ? 'text-mesh-glow' : 'text-mesh-text'}`}>
              {now.psi.toFixed(3)}
            </div>
          </div>
          {dirty && (
            <div className="pb-1">
              <div className="text-[10px] text-mesh-dim uppercase tracking-wider">observed</div>
              <div className="text-lg tabular-nums text-mesh-muted">{live.psi.toFixed(3)}</div>
            </div>
          )}
          <div className="pb-1">
            <div className="text-[10px] text-mesh-dim uppercase tracking-wider">verdict</div>
            <div className="text-sm uppercase tracking-wider text-mesh-text">{now.verdict}</div>
          </div>
          <div className="pb-1 flex gap-4 text-[11px] text-mesh-muted tabular-nums">
            <span>C {now.confidence.toFixed(3)}</span>
            <span>K {now.coherence.toFixed(3)}</span>
            <span>R {now.risk.toFixed(3)}</span>
          </div>
        </div>

        <PsiScale psi={now.psi} observed={live.psi} dirty={dirty} />

        <p className={`text-[11px] mt-2 ${dirty ? 'text-mesh-glow' : 'text-mesh-muted'}`}>{constraint}</p>
      </div>

      {/* K has no term list — it comes from the live subsystems — so it gets its own
          control, and only as a counterfactual. */}
      <div className="px-4 py-2.5 border-b border-mesh-border/60 flex items-center gap-3 flex-wrap">
        <span className="text-[10px] text-mesh-accent w-16">K</span>
        <span className="text-[10px] text-mesh-dim w-10">§31</span>
        <input
          type="range"
          min={0}
          max={1}
          step={0.01}
          value={coherence ?? c.coherence.value}
          onChange={e => setCoherence(Number(e.target.value))}
          className="flex-1 min-w-[140px] accent-mesh-glow"
          aria-label="coherence counterfactual"
        />
        <span className="text-[11px] tabular-nums w-12 text-mesh-text">
          {(coherence ?? c.coherence.value).toFixed(2)}
        </span>
        <span className="text-[10px] text-mesh-dim flex-1 min-w-[180px]">
          {c.coherence.subsystems} live subsystem{c.coherence.subsystems === 1 ? '' : 's'} —{' '}
          {c.coherence.approximation ? 'discrete approximation of D(Yᵢ,Ȳ)' : 'exact'}
        </span>
      </div>

      <div className="divide-y divide-mesh-border/40">
        {terms.map(t => (
          <TermSlider
            key={`${t.section}-${t.symbol}`}
            term={t}
            overrides={overrides}
            gradient={sens.find(s => s.symbol === t.symbol)?.gradient ?? 0}
            onChange={v => set(t.symbol, v)}
            onClear={() => clear(t.symbol)}
          />
        ))}
      </div>

      <p className="px-4 py-2.5 text-[10px] text-mesh-dim border-t border-mesh-border">
        Dragging an <span className="text-mesh-absent">unmeasured</span> term makes it count as
        measured, which changes the denominator of the risk mean. That is a real property of
        the design, not a quirk: instrumenting a healthy subsystem can raise reported risk,
        because the average was previously taken over fewer things.
      </p>
    </section>
  )
}

/** Ψ against its two thresholds, so the number has somewhere to stand. */
function PsiScale({ psi, observed, dirty }: { psi: number; observed: number; dirty: boolean }) {
  const pct = (v: number) => `${Math.max(0, Math.min(1, v)) * 100}%`
  return (
    <div className="mt-3 relative h-6">
      <div className="absolute inset-x-0 top-2 h-1.5 bg-mesh-hi" />
      <div className="absolute top-2 h-1.5 bg-mesh-veto/40" style={{ left: 0, width: pct(TAU_PSI) }} />
      <div
        className="absolute top-2 h-1.5 bg-mesh-stale/40"
        style={{ left: pct(TAU_PSI), width: pct(TAU_PSI_FULL - TAU_PSI) }}
      />
      <div
        className="absolute top-2 h-1.5 bg-mesh-live/40"
        style={{ left: pct(TAU_PSI_FULL), width: pct(1 - TAU_PSI_FULL) }}
      />

      {dirty && (
        <div
          className="absolute top-1 w-px h-3.5 bg-mesh-muted"
          style={{ left: pct(observed) }}
          title={`observed ${observed.toFixed(3)}`}
        />
      )}
      <div
        className={`absolute top-0 w-0.5 h-5 ${dirty ? 'bg-mesh-glow' : 'bg-mesh-text'}`}
        style={{ left: pct(psi) }}
      />

      <span className="absolute top-[22px] text-[9px] text-mesh-dim" style={{ left: pct(TAU_PSI) }}>
        τ_Ψ
      </span>
      <span className="absolute top-[22px] text-[9px] text-mesh-dim" style={{ left: pct(TAU_PSI_FULL) }}>
        τ_full
      </span>
    </div>
  )
}

function TermSlider({
  term,
  overrides,
  gradient,
  onChange,
  onClear,
}: {
  term: Term
  overrides: Overrides
  gradient: number
  onChange: (v: number) => void
  onClear: () => void
}) {
  const e = effective(term, overrides)
  const touched = overrides[term.symbol] !== undefined
  // Bar length is |∂Ψ/∂term| relative to a full unit of Ψ per unit of term.
  const leverage = Math.min(1, Math.abs(gradient))

  return (
    <div className={`px-4 py-2 ${touched ? 'bg-mesh-hi/50' : ''}`}>
      <div className="flex items-center gap-3 flex-wrap">
        <span className="text-[11px] text-mesh-accent w-16 shrink-0">{term.symbol}</span>
        <span className="text-[10px] text-mesh-border-hi w-10 shrink-0">{term.section}</span>

        <input
          type="range"
          min={0}
          max={1}
          step={0.01}
          value={e.value}
          onChange={ev => onChange(Number(ev.target.value))}
          className="flex-1 min-w-[140px] accent-mesh-glow"
          aria-label={`${term.name} counterfactual`}
        />

        <span className={`text-[11px] tabular-nums w-12 shrink-0 ${touched ? 'text-mesh-glow' : 'text-mesh-text'}`}>
          {e.value.toFixed(2)}
        </span>

        {/* Observed value stays visible whenever it has been overridden. */}
        {touched && (
          <span className="text-[10px] text-mesh-muted tabular-nums shrink-0" title="observed">
            was {term.value.toFixed(2)}
          </span>
        )}

        <span
          className={`text-[9px] uppercase tracking-wider w-[74px] shrink-0 ${
            touched ? 'text-mesh-glow' : e.measured ? 'text-mesh-live' : 'text-mesh-absent'
          }`}
        >
          {touched ? 'hypothetical' : e.measured ? 'measured' : 'unmeasured'}
        </span>

        {/* Leverage: how hard this term is pushing on Ψ right now. */}
        <span className="shrink-0 flex items-center gap-1 w-[86px]" title={`∂Ψ/∂${term.symbol} = ${gradient.toFixed(3)}`}>
          <span className="inline-block h-1 bg-mesh-hi" style={{ width: PSI_TRACK_W * 0.5 }}>
            <span
              className={`block h-1 ${gradient < 0 ? 'bg-mesh-veto' : 'bg-mesh-live'}`}
              style={{ width: leverage * PSI_TRACK_W * 0.5 }}
            />
          </span>
        </span>

        {touched && (
          <button onClick={onClear} className="text-[10px] text-mesh-dim hover:text-mesh-text shrink-0">
            ↺
          </button>
        )}
      </div>
      <div className="text-[10px] text-mesh-dim mt-0.5 pl-[104px]">{term.note}</div>
    </div>
  )
}
