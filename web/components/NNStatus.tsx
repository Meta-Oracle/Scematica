'use client'

import { useEffect, useState } from 'react'
import { api } from '@/lib/api'
import type { NNAdvice, NNStats } from '@/lib/types'

export function NNStatus() {
  const [data, setData] = useState<NNStats | null>(null)
  const [advice, setAdvice] = useState<NNAdvice | null>(null)

  useEffect(() => {
    let alive = true
    async function poll() {
      const [stats, nextAdvice] = await Promise.all([api.nn(), api.nnAdvice()])
      if (!alive) return
      if (stats) setData(stats)
      if (nextAdvice) setAdvice(nextAdvice)
    }
    poll()
    const iv = setInterval(poll, 10_000)
    return () => { alive = false; clearInterval(iv) }
  }, [])

  if (!data) return null

  const epsilon     = data.epsilon ?? 1
  const stepCount   = data.step_count ?? 0
  const replaySize  = data.replay_size ?? 0
  const totalReward = data.total_reward ?? 0
  const epsilonPct  = (epsilon * 100).toFixed(1)
  const advisorColor = data.ready_to_advise ? 'text-scema-green' : 'text-scema-amber'
  const action = advice?.action ?? 'NoAdvice'
  const confidence = ((advice?.confidence ?? 0) * 100).toFixed(1)
  const qValues = advice?.q_values ?? []

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
          <span className="text-scema-text font-bold tabular-nums">{stepCount.toLocaleString()}</span>
        </div>
        <div className="flex flex-col items-center py-2 px-3 gap-0.5">
          <span className="text-scema-dim tracking-wider text-xs">ε</span>
          <span className="text-scema-amber font-bold tabular-nums">{epsilonPct}%</span>
        </div>
        <div className="flex flex-col items-center py-2 px-3 gap-0.5">
          <span className="text-scema-dim tracking-wider text-xs">REPLAY</span>
          <span className="text-scema-text font-bold tabular-nums">{replaySize}</span>
        </div>
        <div className="flex flex-col items-center py-2 px-3 gap-0.5">
          <span className="text-scema-dim tracking-wider text-xs">REWARD</span>
          <span className={`font-bold tabular-nums ${totalReward >= 0 ? 'text-scema-green' : 'text-scema-red-hi'}`}>
            {totalReward >= 0 ? '+' : ''}{(totalReward / 1000).toFixed(1)}k
          </span>
        </div>
      </div>

      <div className="grid grid-cols-1 lg:grid-cols-3 border-t border-scema-border text-xs">
        <div className="px-3 py-2 border-b lg:border-b-0 lg:border-r border-scema-border">
          <span className="block text-scema-dim tracking-wider">CURRENT ACTION</span>
          <span className={`font-bold tabular-nums ${action.startsWith('Buy') ? 'text-scema-green' : action.startsWith('Sell') ? 'text-scema-red-hi' : 'text-scema-amber'}`}>
            {action}
          </span>
          <span className="ml-2 text-scema-muted">{confidence}% confidence</span>
        </div>
        <div className="px-3 py-2 border-b lg:border-b-0 lg:border-r border-scema-border lg:col-span-2">
          <span className="block text-scema-dim tracking-wider">REASON</span>
          <span className="text-scema-text line-clamp-2">{advice?.top_reason ?? 'Waiting for DQ* advice snapshot'}</span>
        </div>
      </div>

      {qValues.length > 0 && (
        <div className="grid grid-cols-5 divide-x divide-scema-border border-t border-scema-border text-xs">
          {qValues.slice(0, 5).map(([label, value]) => (
            <div key={label} className="flex flex-col items-center py-2 px-2 gap-0.5 min-w-0">
              <span className="text-scema-dim tracking-wider text-[0.6rem] truncate max-w-full">{label}</span>
              <span className="text-scema-text font-bold tabular-nums">{value.toFixed(3)}</span>
            </div>
          ))}
        </div>
      )}

      {/* Epsilon progress bar */}
      <div className="px-3 pb-2 pt-1">
        <div className="flex justify-between text-xs text-scema-dim mb-1">
          <span>Explore → Exploit</span>
          <span>{epsilonPct}% random</span>
        </div>
        <div className="w-full h-0.5 bg-scema-dim">
          <div
            className="h-full bg-gradient-to-r from-scema-red-hi to-scema-red transition-all duration-1000"
            style={{ width: `${epsilon * 100}%` }}
          />
        </div>
      </div>
    </div>
  )
}
