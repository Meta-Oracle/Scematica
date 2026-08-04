'use client'

import { useEffect, useState } from 'react'
import { api } from '@/lib/api'
import type { TournamentSnapshot } from '@/lib/types'

const VARIANT_LABEL: Record<string, string> = {
  conservative: 'CONSERVATIVE',
  balanced: 'BALANCED',
  aggressive: 'AGGRESSIVE',
}

// Deep Q*™ multi-agent tournament — 3 Dueling Double-DQN variants (conservative /
// balanced / aggressive hyperparameters) train on the same live experience stream in
// parallel paper-trading mode; every eval_freq steps the highest-`total_reward`
// variant is promoted to primary and actually sizes/vetoes buys. This is the part of
// the stack that's genuinely proprietary — not a static rule table, an agent that's
// still competing with itself in the background.
export function Tournament() {
  const [data, setData] = useState<TournamentSnapshot | null>(null)

  useEffect(() => {
    let alive = true
    async function poll() {
      const r = await api.tournament()
      if (alive) setData(r)
    }
    poll()
    const iv = setInterval(poll, 10_000)
    return () => { alive = false; clearInterval(iv) }
  }, [])

  if (!data) return null

  const names   = data.agent_names ?? []
  const rewards = data.agent_total_rewards ?? []
  const epsilons = data.agent_epsilons ?? []
  const maxAbs  = Math.max(1, ...rewards.map(r => Math.abs(r)))
  const evalPct = data.eval_freq > 0
    ? Math.min(100, (data.steps_since_eval / data.eval_freq) * 100)
    : 0

  return (
    <div className="panel flex flex-col gap-0">
      <div className="panel-header justify-between">
        <span>Deep Q*™ Tournament</span>
        <span className="text-scema-dim normal-case tracking-normal">
          next promotion check {evalPct.toFixed(0)}%
        </span>
      </div>

      <div className="flex flex-col divide-y divide-scema-border">
        {names.map((name, i) => {
          const isPrimary = i === data.primary_idx
          const reward = rewards[i] ?? 0
          const epsilon = epsilons[i] ?? 1
          const barPct = (Math.abs(reward) / maxAbs) * 100
          const positive = reward >= 0

          return (
            <div key={name} className="flex items-center gap-3 px-3 py-2 text-xs">
              <div className="w-24 shrink-0 flex items-center gap-1.5">
                {isPrimary && <span className="text-scema-red-hi" title="promoted — currently sizing live buys">★</span>}
                <span className={isPrimary ? 'text-scema-text font-bold' : 'text-scema-muted'}>
                  {VARIANT_LABEL[name] ?? name.toUpperCase()}
                </span>
              </div>

              <div className="flex-1 h-2 bg-scema-dim/20 relative overflow-hidden">
                <div
                  className={`h-full transition-all duration-700 ${positive ? 'bg-scema-green' : 'bg-scema-red-hi'}`}
                  style={{ width: `${Math.max(2, barPct)}%` }}
                />
              </div>

              <span className={`w-20 shrink-0 text-right tabular-nums font-mono font-bold ${positive ? 'text-scema-green' : 'text-scema-red-hi'}`}>
                {positive ? '+' : ''}{reward.toFixed(0)}
              </span>

              <span className="w-14 shrink-0 text-right tabular-nums font-mono text-scema-dim">
                ε {(epsilon * 100).toFixed(0)}%
              </span>
            </div>
          )
        })}
      </div>

      <div className="px-3 py-1.5 border-t border-scema-border text-[0.65rem] text-scema-dim">
        Highest-reward variant is promoted to primary automatically — it's the one sizing entries and vetoing buys right now.
      </div>
    </div>
  )
}
