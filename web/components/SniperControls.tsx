'use client'

import { useEffect, useState, useCallback, useRef } from 'react'
import { apiFetch } from '@/lib/net'

// ── types ─────────────────────────────────────────────────────────────────────

const RATE_MODES = ['bearish','micro','safe','balanced','aggressive','degen','bullish','moon'] as const
type RateMode = typeof RATE_MODES[number]

const BUILDER_MODES = ['off','growth','builder','super_builder'] as const
type BuilderMode = typeof BUILDER_MODES[number]

interface ControlState {
  sell_mode:          boolean
  dump_mode:          boolean
  rate_mode:          RateMode
  high_speed:         boolean
  moon_chase:         boolean
  builder_mode:       BuilderMode
  builder_target_sol: number
  tp_pct:             number
  sl_pct:             number
  multiplier:         number
}

// ── rate mode table ───────────────────────────────────────────────────────────

interface RateInfo { label: string; mult: string; tp: string; sl: string; dim: string; active: string }

const RATE_INFO: Record<RateMode, RateInfo> = {
  bearish:    { label:'BEARISH',    mult:'0.3×', tp:'45%',   sl:'8%',  dim:'border-scema-dim text-scema-dim',    active:'border-scema-green/70 text-scema-green bg-scema-green/10 shadow-[0_0_10px_rgba(0,204,68,0.2)]' },
  micro:      { label:'MICRO',      mult:'0.1×', tp:'60%',   sl:'10%', dim:'border-scema-dim text-scema-dim',    active:'border-scema-muted/70 text-scema-muted bg-scema-dim/20' },
  safe:       { label:'SAFE',       mult:'0.5×', tp:'75%',   sl:'10%', dim:'border-scema-dim text-scema-dim',    active:'border-scema-green/70 text-scema-green bg-scema-green/10 shadow-[0_0_10px_rgba(0,204,68,0.2)]' },
  balanced:   { label:'BALANCED',   mult:'1×',   tp:'150%',  sl:'15%', dim:'border-scema-dim text-scema-dim',    active:'border-scema-text/60 text-scema-text bg-scema-dim/20' },
  aggressive: { label:'AGGRESSIVE', mult:'2×',   tp:'300%',  sl:'25%', dim:'border-scema-dim text-scema-dim',    active:'border-scema-amber/70 text-scema-amber bg-scema-amber/10 shadow-[0_0_10px_rgba(255,170,0,0.2)]' },
  degen:      { label:'DEGEN',      mult:'4×',   tp:'450%',  sl:'40%', dim:'border-scema-dim text-scema-dim',    active:'border-scema-amber/80 text-scema-amber bg-scema-amber/15 shadow-[0_0_12px_rgba(255,170,0,0.3)]' },
  bullish:    { label:'BULLISH',    mult:'6×',   tp:'750%',  sl:'50%', dim:'border-scema-dim text-scema-dim',    active:'border-scema-red/70 text-scema-red-hi bg-scema-red/10 shadow-[0_0_12px_rgba(255,32,32,0.3)]' },
  moon:       { label:'MOON',       mult:'8×',   tp:'1200%', sl:'60%', dim:'border-scema-dim text-scema-dim',    active:'border-scema-red/80 text-scema-red-hi bg-scema-red/15 shadow-[0_0_16px_rgba(255,32,32,0.45)] animate-flicker' },
}

// ── builder mode table ────────────────────────────────────────────────────────

interface BuilderInfo { label: string; target: string; desc: string; key: string; dim: string; active: string }

const BUILDER_INFO: Record<BuilderMode, BuilderInfo> = {
  off:          { label:'OFF',         target:'',         desc:'config.toml defaults',             key:'O', dim:'border-scema-dim text-scema-dim',    active:'border-scema-muted/60 text-scema-muted bg-scema-dim/20' },
  growth:       { label:'GROWTH',      target:'0.2 SOL',  desc:'1.0–2.0× mild geometric',          key:'G', dim:'border-scema-dim text-scema-dim',    active:'border-scema-green/70 text-scema-green bg-scema-green/10 shadow-[0_0_10px_rgba(0,204,68,0.2)]' },
  builder:      { label:'BUILDER',     target:'1.0 SOL',  desc:'1.5–3.5× compounding (p^0.65)',    key:'J', dim:'border-scema-dim text-scema-dim',    active:'border-scema-amber/70 text-scema-amber bg-scema-amber/10 shadow-[0_0_10px_rgba(255,170,0,0.25)]' },
  super_builder:{ label:'SUPER',       target:'3.0 SOL',  desc:'2.0–8.0× parabolic + moon-chase', key:'K', dim:'border-scema-dim text-scema-dim',    active:'border-scema-red/70 text-scema-red-hi bg-scema-red/10 shadow-[0_0_12px_rgba(255,32,32,0.35)]' },
}

// ── api helper ────────────────────────────────────────────────────────────────

async function postControl(endpoint: string, body: object) {
  try {
    await apiFetch(endpoint, {
      method:  'POST',
      headers: { 'Content-Type': 'application/json' },
      body:    JSON.stringify(body),
    })
  } catch {}
}

// ── component ─────────────────────────────────────────────────────────────────

const DEFAULT_STATE: ControlState = {
  sell_mode: false, dump_mode: false, rate_mode: 'balanced',
  high_speed: false, moon_chase: false, builder_mode: 'off',
  builder_target_sol: 0, tp_pct: 150, sl_pct: 15, multiplier: 1,
}

export function SniperControls() {
  const [state, setState]   = useState<ControlState>(DEFAULT_STATE)
  const [lastKey, setLastKey] = useState('')
  const stateRef = useRef(state)
  useEffect(() => { stateRef.current = state }, [state])

  // ── poll live state every 2 s ───────────────────────────────────────────────
  useEffect(() => {
    let alive = true
    async function poll() {
      try {
        const r = await apiFetch('/api/controls')
        if (!alive || !r.ok) return
        const d = await r.json().catch(() => null)
        if (d) setState(d as ControlState)
      } catch {}
    }
    poll()
    const iv = setInterval(poll, 2_000)
    return () => { alive = false; clearInterval(iv) }
  }, [])

  // ── flash key indicator ─────────────────────────────────────────────────────
  function flashKey(k: string) {
    setLastKey(k)
    setTimeout(() => setLastKey(''), 800)
  }

  // ── toggle actions ──────────────────────────────────────────────────────────
  const toggleSellMode = useCallback(async () => {
    const next = !stateRef.current.sell_mode
    setState(s => ({ ...s, sell_mode: next }))
    await postControl('/api/controls/sell-mode', { enabled: next })
  }, [])

  const toggleDumpMode = useCallback(async () => {
    const next = !stateRef.current.dump_mode
    setState(s => ({ ...s, dump_mode: next }))
    await postControl('/api/controls/dump-mode', { enabled: next })
  }, [])

  const toggleHighSpeed = useCallback(async () => {
    const next = !stateRef.current.high_speed
    setState(s => ({ ...s, high_speed: next }))
    await postControl('/api/controls/high-speed', { enabled: next })
  }, [])

  const toggleMoonChase = useCallback(async () => {
    const next = !stateRef.current.moon_chase
    setState(s => ({ ...s, moon_chase: next }))
    await postControl('/api/controls/moon-chase', { enabled: next })
  }, [])

  // ── rate mode ───────────────────────────────────────────────────────────────
  const setRateMode = useCallback(async (mode: RateMode) => {
    setState(s => ({ ...s, rate_mode: mode }))
    await postControl('/api/controls/rate-mode', { mode })
  }, [])

  const cycleRateMode = useCallback(async () => {
    const idx  = RATE_MODES.indexOf(stateRef.current.rate_mode)
    const next = RATE_MODES[(idx + 1) % RATE_MODES.length]
    await setRateMode(next)
  }, [setRateMode])

  // ── builder mode (click active = turn off) ──────────────────────────────────
  const setBuilderMode = useCallback(async (mode: BuilderMode) => {
    const next = stateRef.current.builder_mode === mode && mode !== 'off' ? 'off' : mode
    setState(s => ({
      ...s,
      builder_mode: next,
      builder_target_sol: next === 'growth' ? 0.2 : next === 'builder' ? 1.0 : next === 'super_builder' ? 3.0 : 0,
    }))
    await postControl('/api/controls/builder-mode', { mode: next })
  }, [])

  // ── keyboard shortcuts ──────────────────────────────────────────────────────
  useEffect(() => {
    function onKey(e: KeyboardEvent) {
      if (e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement) return
      const key = e.key.toLowerCase()
      flashKey(key)
      switch (key) {
        case 's': toggleSellMode();  break
        case 'd': toggleDumpMode();  break
        case 'h': toggleHighSpeed(); break
        case 'm': toggleMoonChase(); break
        case 'r': cycleRateMode();   break
        case '1': setRateMode('bearish');    break
        case '2': setRateMode('micro');      break
        case '3': setRateMode('safe');       break
        case '4': setRateMode('balanced');   break
        case '5': setRateMode('aggressive'); break
        case '6': setRateMode('degen');      break
        case '7': setRateMode('bullish');    break
        case '8': setRateMode('moon');       break
        case 'o': setBuilderMode('off');          break
        case 'g': setBuilderMode('growth');       break
        case 'j': setBuilderMode('builder');      break
        case 'k': setBuilderMode('super_builder'); break
      }
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [toggleSellMode, toggleDumpMode, toggleHighSpeed, toggleMoonChase, cycleRateMode, setRateMode, setBuilderMode])

  const activeRate    = RATE_INFO[state.rate_mode]
  const activeBuilder = BUILDER_INFO[state.builder_mode]

  return (
    <div className="panel flex flex-col gap-0">

      {/* ── header ─────────────────────────────────────────────────────────── */}
      <div className="panel-header justify-between">
        <span>Sniper Controls</span>
        <div className="flex items-center gap-3">
          {lastKey && (
            <span className="text-scema-red-hi text-xs font-bold animate-fade-in font-mono">
              [{lastKey.toUpperCase()}]
            </span>
          )}
          <span className="text-scema-dim text-xs hidden sm:inline">
            S·D·H·M·R · 1-8 · O·G·J·K
          </span>
        </div>
      </div>

      {/* ── Section 1: Emergency toggles ─────────────────────────────────── */}
      <div className="px-3 pt-3 pb-1">
        <p className="text-scema-dim text-xs tracking-widest uppercase mb-2">◈ Emergency</p>
        <div className="flex flex-wrap gap-2">

          <button
            onClick={toggleSellMode}
            title="[S] Pause new buys, sell all open positions"
            className={`flex flex-col items-center gap-1 px-4 py-2.5 border transition-all duration-150 min-w-[76px] ${
              state.sell_mode
                ? 'border-scema-amber text-scema-amber bg-scema-amber/10 shadow-[0_0_10px_rgba(255,170,0,0.3)]'
                : 'border-scema-dim text-scema-dim hover:border-scema-amber hover:text-scema-amber'
            }`}
          >
            <span className="text-lg leading-none">{state.sell_mode ? '🔴' : '⏸'}</span>
            <span className="text-xs font-bold tracking-widest">SELL</span>
            <span className="text-xs text-scema-dim">[S]</span>
          </button>

          <button
            onClick={toggleDumpMode}
            title="[D] Force-sell everything with min_out=0 (no slippage protection)"
            className={`flex flex-col items-center gap-1 px-4 py-2.5 border transition-all duration-150 min-w-[76px] ${
              state.dump_mode
                ? 'border-scema-red-hi text-scema-red-hi bg-scema-red/20 shadow-[0_0_14px_rgba(255,32,32,0.4)] animate-flicker'
                : 'border-scema-dim text-scema-dim hover:border-scema-red-hi hover:text-scema-red-hi'
            }`}
          >
            <span className="text-lg leading-none">💥</span>
            <span className="text-xs font-bold tracking-widest">DUMP</span>
            <span className="text-xs text-scema-dim">[D]</span>
          </button>

        </div>
      </div>

      {/* ── Section 2: Speed & momentum toggles ──────────────────────────── */}
      <div className="px-3 pt-2 pb-1">
        <p className="text-scema-dim text-xs tracking-widest uppercase mb-2">◈ Speed & Momentum</p>
        <div className="flex flex-wrap gap-2">

          <button
            onClick={toggleHighSpeed}
            title="[H] Bypass filters, AI scoring, and pool checks for max-speed entry"
            className={`flex flex-col items-center gap-1 px-4 py-2.5 border transition-all duration-150 min-w-[76px] ${
              state.high_speed
                ? 'border-scema-red text-scema-red-hi bg-scema-red/10 shadow-[0_0_10px_rgba(255,32,32,0.2)]'
                : 'border-scema-dim text-scema-dim hover:border-scema-red hover:text-scema-red-hi'
            }`}
          >
            <span className="text-lg leading-none">⚡</span>
            <span className="text-xs font-bold tracking-widest">FAST</span>
            <span className="text-xs text-scema-dim">[H]</span>
          </button>

          <button
            onClick={toggleMoonChase}
            title="[M] Aggressive momentum: 8 escalations × 1.75×, 25% pullback, 3%/check threshold"
            className={`flex flex-col items-center gap-1 px-4 py-2.5 border transition-all duration-150 min-w-[76px] ${
              state.moon_chase
                ? 'border-scema-red-hi text-scema-red-hi bg-scema-red/15 shadow-[0_0_14px_rgba(255,32,32,0.35)] animate-flicker'
                : 'border-scema-dim text-scema-dim hover:border-scema-red-hi hover:text-scema-red-hi'
            }`}
          >
            <span className="text-lg leading-none">🌙</span>
            <span className="text-xs font-bold tracking-widest">MOON</span>
            <span className="text-xs text-scema-dim">[M]</span>
          </button>

        </div>
      </div>

      {/* ── Section 3: Rate mode ─────────────────────────────────────────── */}
      <div className="px-3 pt-2 pb-1">
        <div className="flex items-center justify-between mb-2">
          <p className="text-scema-dim text-xs tracking-widest uppercase">◈ Rate Mode</p>
          <span className="text-xs text-scema-dim">[R] cycle</span>
        </div>

        <div className="grid grid-cols-4 gap-1.5 mb-2">
          {RATE_MODES.map((mode, i) => {
            const info    = RATE_INFO[mode]
            const isActive = state.rate_mode === mode
            return (
              <button
                key={mode}
                onClick={() => setRateMode(mode)}
                title={`[${i + 1}] ${info.label} — ${info.mult} | TP ${info.tp} | SL ${info.sl}`}
                className={`flex flex-col items-center gap-0.5 py-2 px-1 border transition-all duration-150 ${
                  isActive ? info.active : `${info.dim} hover:opacity-70`
                }`}
              >
                <span className="text-xs font-bold tracking-wider">{info.label}</span>
                <span className="text-xs tabular-nums opacity-80">{info.mult}</span>
                <span className="text-xs text-scema-dim">[{i + 1}]</span>
              </button>
            )
          })}
        </div>

        {/* Active rate mode info bar */}
        <div className="flex items-center justify-between text-xs border border-scema-border/40 bg-scema-dim/5 px-2.5 py-1.5">
          <span className="text-scema-dim">Active:</span>
          <span className="font-bold tracking-wider" style={{ color: 'inherit' }}>
            {activeRate.label}
          </span>
          <span className="text-scema-muted tabular-nums">{activeRate.mult}</span>
          <span className="text-scema-dim">TP <span className="text-scema-green tabular-nums">{activeRate.tp}</span></span>
          <span className="text-scema-dim">SL <span className="text-scema-red-hi tabular-nums">{activeRate.sl}</span></span>
        </div>
      </div>

      {/* ── Section 4: Builder mode ─────────────────────────────────────── */}
      <div className="px-3 pt-2 pb-3">
        <p className="text-scema-dim text-xs tracking-widest uppercase mb-2">◈ Builder Mode</p>
        <div className="grid grid-cols-4 gap-1.5 mb-2">
          {BUILDER_MODES.map(mode => {
            const info    = BUILDER_INFO[mode]
            const isActive = state.builder_mode === mode
            return (
              <button
                key={mode}
                onClick={() => setBuilderMode(mode)}
                title={`[${info.key}] ${info.label}${info.target ? ` — target ${info.target}` : ''}: ${info.desc}`}
                className={`flex flex-col items-center gap-0.5 py-2 px-1 border transition-all duration-150 ${
                  isActive ? info.active : `${info.dim} hover:opacity-70`
                }`}
              >
                <span className="text-xs font-bold tracking-wider">{info.label}</span>
                {info.target
                  ? <span className="text-xs tabular-nums opacity-80">{info.target}</span>
                  : <span className="text-xs opacity-40">—</span>
                }
                <span className="text-xs text-scema-dim">[{info.key}]</span>
              </button>
            )
          })}
        </div>

        {/* Builder active description */}
        {state.builder_mode !== 'off' && (
          <div className="text-xs border border-scema-border/40 bg-scema-dim/5 px-2.5 py-1.5 flex items-center justify-between gap-2">
            <span className="text-scema-dim">{activeBuilder.label}</span>
            <span className="text-scema-muted">{activeBuilder.desc}</span>
            <span className="text-scema-dim shrink-0">
              target: <span className="text-scema-amber tabular-nums">{activeBuilder.target}</span>
            </span>
          </div>
        )}
      </div>

      {/* ── Active state warnings ─────────────────────────────────────────── */}
      {(state.sell_mode || state.dump_mode || state.high_speed || state.moon_chase) && (
        <div className="border-t border-scema-border px-3 py-2 flex flex-wrap gap-x-4 gap-y-1 text-xs">
          {state.sell_mode  && <span className="text-scema-amber">⚠ SELL MODE — no new buys</span>}
          {state.dump_mode  && <span className="text-scema-red-hi animate-flicker">🚨 DUMP MODE — min_out=0</span>}
          {state.high_speed && <span className="text-scema-red">⚡ HIGH SPEED — filters bypassed</span>}
          {state.moon_chase && <span className="text-scema-red-hi">🌙 MOON CHASE — 8 escalations × 1.75×</span>}
        </div>
      )}
    </div>
  )
}
