'use client'

import { useEffect, useRef, useState } from 'react'
import { api } from '@/lib/api'

const LEVEL_COLOR: Record<string, string> = {
  INFO:  'text-scema-text',
  WARN:  'text-scema-amber',
  ERROR: 'text-scema-red-hi',
  DEBUG: 'text-scema-dim',
}

function parseLine(line: string) {
  // Full ISO:   2026-05-18T22:28:03.228Z  INFO  target: msg
  let m = line.match(/^(\d{4}-\d{2}-\d{2}T([\d:]{8})[\d.]*Z)\s+(INFO|WARN|ERROR|DEBUG)\s+(\S+):\s+(.*)/)
  if (m) {
    const [,, time, level, target, msg] = m
    return { time, level, target, msg }
  }
  // Short time: 22:28:03.228163Z  INFO  target: msg  (tracing omits date on same-day lines)
  m = line.match(/^(\d{1,2}:\d{2}:\d{2})[\d.]*Z?\s+(INFO|WARN|ERROR|DEBUG)\s+(\S+):\s+(.*)/)
  if (m) {
    const [, time, level, target, msg] = m
    return { time, level, target, msg }
  }
  return { time: '', level: 'INFO', target: '', msg: line }
}

export function LogStream() {
  const [lines, setLines] = useState<string[]>([])
  const [paused, setPaused] = useState(false)
  const [filter, setFilter] = useState('')
  const [pinned, setPinned] = useState(true)
  const containerRef = useRef<HTMLDivElement>(null)
  const bottomRef = useRef<HTMLDivElement>(null)
  const pausedRef = useRef(false)

  useEffect(() => { pausedRef.current = paused }, [paused])

  useEffect(() => {
    let alive = true
    async function poll() {
      if (pausedRef.current) return
      const r = await api.logs(80)
      if (!alive || !r) return
      setLines(r.lines)
    }
    poll()
    const iv = setInterval(poll, 2000)
    return () => { alive = false; clearInterval(iv) }
  }, [])

  useEffect(() => {
    if (pinned && !paused && bottomRef.current) {
      bottomRef.current.scrollIntoView({ behavior: 'instant' })
    }
  }, [lines, pinned, paused])

  function handleScroll() {
    const el = containerRef.current
    if (!el) return
    const atBottom = el.scrollHeight - el.scrollTop - el.clientHeight < 60
    setPinned(atBottom)
  }

  const displayed = filter
    ? lines.filter(l => l.toLowerCase().includes(filter.toLowerCase()))
    : lines

  return (
    <div className="panel flex flex-col h-full">
      <div className="panel-header justify-between">
        <span>Log Stream</span>
        <div className="flex items-center gap-2 ml-auto">
          <input
            type="text"
            placeholder="filter…"
            value={filter}
            onChange={e => setFilter(e.target.value)}
            className="bg-transparent border border-scema-border text-scema-text text-xs px-2 py-0.5 w-28 focus:outline-none focus:border-scema-red-dim placeholder:text-scema-dim"
          />
          {!pinned && !paused && (
            <button
              onClick={() => {
                setPinned(true)
                bottomRef.current?.scrollIntoView({ behavior: 'smooth' })
              }}
              className="text-xs px-2 py-0.5 border border-scema-red-dim text-scema-red-hi hover:bg-scema-red/10 transition-colors"
            >
              ↓ BOTTOM
            </button>
          )}
          <button
            onClick={() => setPaused(p => !p)}
            className={`text-xs px-2 py-0.5 border transition-colors ${
              paused
                ? 'border-scema-amber text-scema-amber'
                : 'border-scema-dim text-scema-dim hover:border-scema-muted hover:text-scema-muted'
            }`}
          >
            {paused ? '▶ RESUME' : '⏸ PAUSE'}
          </button>
        </div>
      </div>

      <div
        ref={containerRef}
        onScroll={handleScroll}
        className="flex-1 overflow-y-auto p-2 text-xs leading-relaxed"
      >
        {displayed.length === 0 ? (
          <div className="text-scema-dim text-center py-4">
            <span className="cursor">AWAITING LOG DATA</span>
          </div>
        ) : displayed.map((line, i) => {
          const { time, level, msg } = parseLine(line)
          const color = LEVEL_COLOR[level] || 'text-scema-text'
          return (
            <div key={i} className="flex gap-2 hover:bg-scema-red-bg/10 px-1 py-0.5 font-mono">
              <span className="text-scema-dim shrink-0 tabular-nums w-16">{time || '──:──:──'}</span>
              <span className={`shrink-0 w-10 ${color} font-bold`}>{level}</span>
              <span className={`${color} break-all`}>{msg}</span>
            </div>
          )
        })}
        <div ref={bottomRef} />
      </div>
    </div>
  )
}
