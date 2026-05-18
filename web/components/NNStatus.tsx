'use client'

import { useEffect, useState } from 'react'
import { api } from '@/lib/api'
import type { NNStats } from '@/lib/types'

export function NNStatus() {
  const [data, setData] = useState<NNStats | null>(null)

  useEffect(() => {
    let alive = true
    async function poll() {
      const r = await api.nn()
      if (alive && r) setData(r)
    }
    poll()
    const iv = setInterval(poll, 10_000)
    return () => { alive = false; clearInterval(iv) }
  }, [])

  if (!data) return null

  const epsilonPct = (data.epsilon * 100).toFixed(1)
  const advisorColor = data.ready_to_advise ? 'text-scema-green' : 'text-scema-amber'

  return (
    <div className="panel flex flex-col gap-0">
      <div className="panel-header justify-between">
        <span>Deep Q* Agent</span>
        <span className={`text-xs ${advisorColor}`}>
          {data.ready_to_advise ? '● ADVISING' : '○ TRAINING'}
        </span>
      </div>
      <div className="grid grid-cols-4 divide-x divide-scema-border text-xs">
        <div className="flex flex-col items-center py-2 px-3 gap-0.5">
          <span className="text-scema-dim tracking-wider text-xs">STEPS</span>
          <span className="text-scema-text font-bold tabular-nums">{data.step_count.toLocaleString()}</span>
        </div>
        <div className="flex flex-col items-center py-2 px-3 gap-0.5">
          <span className="text-scema-dim tracking-wider text-xs">ε</span>
          <span className="text-scema-amber font-bold tabular-nums">{epsilonPct}%</span>
        </div>
        <div className="flex flex-col items-center py-2 px-3 gap-0.5">
          <span className="text-scema-dim tracking-wider text-xs">REPLAY</span>
          <span className="text-scema-text font-bold tabular-nums">{data.replay_size}</span>
        </div>
        <div className="flex flex-col items-center py-2 px-3 gap-0.5">
          <span className="text-scema-dim tracking-wider text-xs">REWARD</span>
          <span className={`font-bold tabular-nums ${data.total_reward >= 0 ? 'text-scema-green' : 'text-scema-red-hi'}`}>
            {data.total_reward >= 0 ? '+' : ''}{(data.total_reward / 1000).toFixed(1)}k
          </span>
        </div>
      </div>
      {/* Epsilon progress bar */}
      <div className="px-3 pb-2 pt-1">
        <div className="flex justify-between text-xs text-scema-dim mb-1">
          <span>Explore → Exploit</span>
          <span>{epsilonPct}% random</span>
        </div>
        <div className="w-full h-0.5 bg-scema-dim">
          <div
            className="h-full bg-gradient-to-r from-scema-red-hi to-scema-red transition-all duration-1000"
            style={{ width: `${data.epsilon * 100}%` }}
          />
        </div>
      </div>
    </div>
  )
}
