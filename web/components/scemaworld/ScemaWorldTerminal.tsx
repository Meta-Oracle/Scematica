'use client'

/**
 * Scema-World — fly a sealed decision record.
 *
 * Like `/omni`, there is **no server side**: the record is read with `FileReader`, verified
 * with WebCrypto in the reader's own browser, and turned into a space by pure code. Nothing
 * is uploaded and there is nothing to phone home to. A game whose map had to be fetched would
 * not be the same claim at all — the whole point is that the record *is* the map.
 *
 * This component places a canvas, reads input, and paints a HUD. Every rule about what a
 * thing looks like lives in `lib/scemaworld/view.ts`, and every matrix comes from
 * `camera.ts`. If a colour or an `isGhost` appears here, the rule has leaked.
 */

import { useCallback, useEffect, useRef, useState } from 'react'

import { verifyRecordText, webSha256, type Verification } from '@/lib/omni/verify'
import { plateSourceFromText } from '@/lib/omni/nft'
import { generate, type Space } from '@/lib/scemaworld/generate'
import { drawList, boundaryLabel, sensorLabel } from '@/lib/scemaworld/view'
import {
  camera as makeCamera,
  mul,
  perspective,
  rotate,
  translate,
  view,
  type Camera,
} from '@/lib/scemaworld/camera'
import { createRenderer, type Renderer } from '@/lib/scemaworld/gl'

interface Loaded {
  name: string
  space: Space
  verification: Verification
}

const UNIT = 1000

export function ScemaWorldTerminal() {
  const canvasRef = useRef<HTMLCanvasElement | null>(null)
  const [loaded, setLoaded] = useState<Loaded | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [flying, setFlying] = useState(false)
  const [speed, setSpeed] = useState(0)

  // Mutable flight state, deliberately outside React. A camera in state would re-render the
  // tree sixty times a second to move a number the DOM never reads.
  const cam = useRef<Camera>(makeCamera([0, 200 * UNIT, 900 * UNIT]))
  const keys = useRef<Set<string>>(new Set())
  const renderer = useRef<Renderer | null>(null)
  const throttle = useRef(0)

  const load = useCallback(async (file: File) => {
    setError(null)
    try {
      const text = await file.text()
      const verification = await verifyRecordText(text, webSha256)
      // Drawn from the raw text for the same reason the digest is: a `JSON.parse` round trip
      // collapses Rust's `0.0` to `0` and would change the commitment the space is seeded by.
      const source = await plateSourceFromText(text, webSha256)
      const space = generate(source.world, source.digest)
      setLoaded({ name: file.name, space, verification })
      cam.current = makeCamera([0, 200 * UNIT, 900 * UNIT])
    } catch (e) {
      setLoaded(null)
      setError(e instanceof Error ? e.message : String(e))
    }
  }, [])

  // Renderer lifecycle.
  useEffect(() => {
    const canvas = canvasRef.current
    if (!canvas || !loaded) return
    let raf = 0
    try {
      renderer.current = createRenderer(canvas)
    } catch (e) {
      setError(e instanceof Error ? e.message : 'WebGL2 unavailable')
      return
    }
    const r = renderer.current
    const list = drawList(loaded.space)
    r.upload(list)

    let last = 0
    const frame = (t: number) => {
      const dt = last === 0 ? 0.016 : Math.min((t - last) / 1000, 0.1)
      last = t

      // ── input ────────────────────────────────────────────────────────────
      const k = keys.current
      const rate = 1.4 * dt
      let c = cam.current
      const pitch = (k.has('KeyS') ? 1 : 0) - (k.has('KeyW') ? 1 : 0)
      const yaw = (k.has('KeyA') ? 1 : 0) - (k.has('KeyD') ? 1 : 0)
      const roll = (k.has('KeyQ') ? 1 : 0) - (k.has('KeyE') ? 1 : 0)
      if (pitch || yaw || roll) c = rotate(c, pitch * rate, yaw * rate, roll * rate)

      const accel = (k.has('ShiftLeft') ? 1 : 0) - (k.has('Space') ? 1 : 0)
      throttle.current = Math.max(0, Math.min(1, throttle.current + accel * dt * 1.5))
      const v = throttle.current * 900 * UNIT
      if (v > 0) c = translate(c, [0, 0, -v * dt])
      cam.current = c

      // ── draw ─────────────────────────────────────────────────────────────
      const w = canvas.clientWidth
      const h = canvas.clientHeight
      if (canvas.width !== w || canvas.height !== h) {
        canvas.width = w
        canvas.height = h
      }
      // Unknown sensor range does not get a made-up default: the far plane falls back to the
      // full extent so the player sees everything there is, rather than an invented horizon.
      const far = list.far ?? 6000 * UNIT
      const proj = perspective(1.15, w / Math.max(1, h), 8 * UNIT, far)
      r.draw(mul(proj, view(c)), w, h)

      raf = requestAnimationFrame(frame)
    }
    raf = requestAnimationFrame(frame)

    const tick = window.setInterval(() => setSpeed(Math.round(throttle.current * 100)), 120)
    return () => {
      cancelAnimationFrame(raf)
      window.clearInterval(tick)
      r.dispose()
      renderer.current = null
    }
  }, [loaded])

  // Keyboard. Attached to the window so the canvas does not need focus to fly.
  useEffect(() => {
    const down = (e: KeyboardEvent) => {
      if (e.code === 'Space') e.preventDefault()
      keys.current.add(e.code)
    }
    const upKey = (e: KeyboardEvent) => keys.current.delete(e.code)
    const blur = () => keys.current.clear()
    window.addEventListener('keydown', down)
    window.addEventListener('keyup', upKey)
    window.addEventListener('blur', blur)
    return () => {
      window.removeEventListener('keydown', down)
      window.removeEventListener('keyup', upKey)
      window.removeEventListener('blur', blur)
    }
  }, [])

  const s = loaded?.space

  return (
    <div className="min-h-screen bg-black text-sm text-omni-text">
      <header className="border-b border-omni-border px-5 py-3">
        <div className="flex flex-wrap items-baseline gap-3">
          <b className="tracking-widest text-omni-accent">SCEMA-WORLD</b>
          <span className="text-omni-dim">the record is the map</span>
          <span className="ml-auto text-omni-dim">
            no upload · the space is computed in this tab
          </span>
        </div>
      </header>

      {!loaded && (
        <div className="mx-auto max-w-2xl space-y-4 p-8">
          <p className="text-omni-muted">
            Drop a sealed decision record — anything <code>scema decide</code> wrote under{' '}
            <code>.scema/decisions/</code>. The world tree it committed to becomes the volume
            you fly. The same record produces the same space on every machine, which is why
            this needs no server and no account.
          </p>
          <label className="block cursor-pointer rounded border border-dashed border-omni-border-hi p-8 text-center hover:border-omni-accent">
            <input
              type="file"
              accept="application/json,.json"
              className="hidden"
              onChange={(e) => {
                const f = e.target.files?.[0]
                if (f) void load(f)
              }}
            />
            <span className="text-omni-text">Choose a record…</span>
          </label>
          {error && <p className="text-omni-invalid">{error}</p>}
          <ul className="space-y-1 text-omni-dim">
            <li>
              <span className="text-omni-text">Blind spots</span> become rifts — lanes that
              end. One per blind spot the observer reported.
            </li>
            <li>
              <span className="text-omni-text">Estimated signals</span> become ghost contacts.
              They read on sensors and may not be there.
            </li>
            <li>
              <span className="text-omni-text">Legibility</span> is your sensor range. A world
              nobody read well is literally dark.
            </li>
          </ul>
        </div>
      )}

      {loaded && s && (
        <div className="relative">
          <canvas
            ref={canvasRef}
            className="block h-[calc(100vh-3.25rem)] w-full"
            onClick={() => setFlying(true)}
          />

          <div className="pointer-events-none absolute left-5 top-4 space-y-1 font-mono text-xs">
            <div className="text-omni-accent">{loaded.name}</div>
            <div className="text-omni-dim">world {s.seed.slice(0, 16)}…</div>
            <div
              className={
                loaded.verification.valid ? 'text-omni-valid' : 'text-omni-invalid'
              }
            >
              {loaded.verification.valid ? 'VERIFIED' : 'INVALID — this record was edited'}
            </div>
          </div>

          <div className="pointer-events-none absolute right-5 top-4 space-y-1 text-right font-mono text-xs">
            <Row k="sensors" v={sensorLabel(s)} />
            <Row k="boundary" v={boundaryLabel(s)} />
            <Row k="stations" v={String(s.nodes.length)} />
            <Row k="contacts" v={String(s.contacts.length)} />
            <Row
              k="rifts"
              v={`${s.rifts}${s.riftsCapped ? ' (capped)' : ''}`}
              alarm={s.rifts > 0}
            />
            <Row k="throttle" v={`${speed}%`} />
          </div>

          {!flying && (
            <div className="pointer-events-none absolute inset-x-0 bottom-8 text-center font-mono text-xs text-omni-dim">
              <div>W/S pitch · A/D yaw · Q/E roll · SHIFT throttle up · SPACE throttle down</div>
              <div className="mt-1">click the view to begin</div>
            </div>
          )}

          {!loaded.verification.valid && (
            <div className="pointer-events-none absolute inset-x-0 bottom-24 mx-auto max-w-xl rounded border border-omni-invalid bg-black/80 p-3 text-center text-xs text-omni-invalid">
              This record does not match its own commitment, so this space is not the one it
              claims to describe. It is drawn anyway — a forgery you cannot look at is one
              nobody learns to recognise.
            </div>
          )}
        </div>
      )}
    </div>
  )
}

function Row({ k, v, alarm }: { k: string; v: string; alarm?: boolean }) {
  return (
    <div>
      <span className="text-omni-dim">{k} </span>
      <span className={alarm ? 'text-omni-absent' : 'text-omni-text'}>{v}</span>
    </div>
  )
}
