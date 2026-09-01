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
import { readWorldCommitment, readEmbeddedRecord } from '@/lib/omni/raster'
import { explain, fetchWorld, matchesRequest, retryable } from '@/lib/scemaworld/vault'
import { generate, type Space } from '@/lib/scemaworld/generate'
import { drawList, boundaryLabel, sensorLabel } from '@/lib/scemaworld/view'
import {
  contact as nearestContact, dynamicOf, inhibited, jumpRefusal, newGame, purchase, route, tick,
  useService, type GameState,
} from '@/lib/scemaworld/game'
import { progress as jumpProgress } from '@/lib/scemaworld/hyper'
import { bearingLabel, fixOn, nearest, rangeLabel } from '@/lib/scemaworld/nav'
import {
  MAX_LEVEL, UPGRADES, fuelCapacity, hullMax, jumpCapacity, shieldMax, upgradeCost,
  type Component,
} from '@/lib/scemaworld/ship'
import { EXTENT, servicesOf } from '@/lib/scemaworld/generate'
import { FAR_PLANE, JUMP_INHIBIT, NEAR_PLANE } from '@/lib/scemaworld/scale'
import { fuelCapacity as _fc } from '@/lib/scemaworld/ship'
import {
  camera as makeCamera,
  forward,
  mul,
  perspective,
  viewRotation,
  rotate,
  translate,
  view,
  type Camera,
} from '@/lib/scemaworld/camera'
import { createRenderer, type Renderer } from '@/lib/scemaworld/gl'
import {
  fire,
  newCombat,
  selected,
  step,
  switchWeapon,
  threatLabel,
  type Combat,
} from '@/lib/scemaworld/weapons'

interface Loaded {
  name: string
  space: Space
  verification: Verification
}


export function ScemaWorldTerminal() {
  const canvasRef = useRef<HTMLCanvasElement | null>(null)
  const [loaded, setLoaded] = useState<Loaded | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [flying, setFlying] = useState(false)
  const [speed, setSpeed] = useState(0)

  // Mutable flight state, deliberately outside React. A camera in state would re-render the
  // tree sixty times a second to move a number the DOM never reads.
  const keys = useRef<Set<string>>(new Set())
  const renderer = useRef<Renderer | null>(null)
  const firing = useRef(false)
  const game = useRef<GameState | null>(null)
  /** A shallow copy of the tick state, pushed to React a few times a second for the HUD. */
  const [hud, setHud] = useState<GameState | null>(null)
  const [weapon, setWeapon] = useState('AUTO LASER')
  const [market, setMarket] = useState(false)

  const [ticket, setTicket] = useState<string | null>(null)
  const [vault, setVault] = useState('')
  const [holder, setHolder] = useState('')
  const [vaultMsg, setVaultMsg] = useState<string | null>(null)
  const [vaultRetryable, setVaultRetryable] = useState(false)
  const [fetching, setFetching] = useState(false)

  /** Load a world the player holds the token for. The only network call in the game. */
  const fromVault = useCallback(
    async (commitment: string) => {
      setVaultMsg(null)
      setFetching(true)
      try {
        const r = await fetchWorld(vault, commitment, holder)
        if (r.kind !== 'ok') {
          setVaultMsg(explain(r))
          setVaultRetryable(retryable(r))
          return
        }
        // The vault served bytes; it did not certify them. Verified here exactly as a
        // dropped file is, and bound to the commitment that was actually requested — no
        // signature on the record itself can say it is the one you asked for.
        const verification = await verifyRecordText(r.record.text, webSha256)
        const source = await plateSourceFromText(r.record.text, webSha256)
        const bad = matchesRequest(commitment, source.digest, verification)
        if (bad) {
          setVaultMsg(explain(bad))
          setVaultRetryable(false)
          return
        }
        setTicket(null)
        setLoaded({
          name: `${commitment.slice(0, 12)}… from vault`,
          space: generate(source.world, source.digest),
          verification,
        })
      } catch (e) {
        setVaultMsg(e instanceof Error ? e.message : String(e))
        setVaultRetryable(true)
      } finally {
        setFetching(false)
      }
    },
    [vault, holder],
  )

  const flyRecordText = useCallback(async (text: string, name: string) => {
    const verification = await verifyRecordText(text, webSha256)
    // Drawn from the raw text for the same reason the digest is: a `JSON.parse` round trip
    // collapses Rust's `0.0` to `0` and would change the commitment the space is seeded by.
    const source = await plateSourceFromText(text, webSha256)
    setLoaded({ name, space: generate(source.world, source.digest), verification })
  }, [])

  const load = useCallback(async (file: File) => {
    setError(null)
    setTicket(null)
    try {
      if (file.name.toLowerCase().endsWith('.png') || file.type === 'image/png') {
        const bytes = new Uint8Array(await file.arrayBuffer())
        // An image written by `scema nft <record>` carries the record itself, so it flies
        // with no vault and no network. Read from the raw chunk bytes and handed straight to
        // the same verifier a dropped `.json` goes through — an embedded record gets no more
        // trust than a file, because the image is not a signature.
        const embedded = readEmbeddedRecord(bytes)
        if (embedded) {
          await flyRecordText(embedded, file.name)
          return
        }
        // Otherwise the image only *names* a world. That is still useful — it is a claim
        // ticket for a vault — but the space needs the objects, the signals and the blind
        // spots, and none of those survive rasterisation.
        const commitment = readWorldCommitment(bytes)
        setLoaded(null)
        if (!commitment) {
          setError(
            'That PNG carries no world commitment. Images written by `scema nft` name the ' +
              'record they derive from; this one does not, so there is nothing to look up.'
          )
        } else {
          setTicket(commitment)
        }
        return
      }
      const text = await file.text()
      await flyRecordText(text, file.name)
    } catch (e) {
      setLoaded(null)
      setError(e instanceof Error ? e.message : String(e))
    }
  }, [flyRecordText])

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
    // The sky is a function of the commitment, so it is built once per world rather than per
    // frame — and two players holding the same record see the same stars.
    r.sky(loaded.space.seed)
    game.current = newGame(loaded.space)
    setHud(game.current)

    let last = 0
    const frame = (t: number) => {
      const dt = last === 0 ? 0.016 : Math.min((t - last) / 1000, 0.1)
      last = t

      const g = game.current
      if (g) {
        // One pure transition. Everything that used to live inline here — flight, fuel,
        // weapons, the enemy — is now testable without a GPU, which is how the missing
        // projectile draw finally became a catchable bug rather than a mystery.
        game.current = tick(g, loaded.space, {
          keys: keys.current,
          firing: firing.current,
          dt,
          nowMs: t,
        })
      }

      const w = canvas.clientWidth
      const h = canvas.clientHeight
      if (canvas.width !== w || canvas.height !== h) {
        canvas.width = w
        canvas.height = h
      }

      // Re-uploaded every frame. The original uploaded once, so shots, moving craft and
      // destroyed contacts never appeared — the scene was a still photograph of the record.
      const live = game.current
      if (live) {
        r.upload(drawList(loaded.space, dynamicOf(live, loaded.space)))
        // The far plane covers the whole generated sector. It used to be gated by sensor range,
        // which put a wall of fog around a volume the entire design is about the size of.
        const proj = perspective(1.15, w / Math.max(1, h), NEAR_PLANE, FAR_PLANE)
        r.draw(mul(proj, view(live.camera)), viewRotation(live.camera), w, h)
      }

      raf = requestAnimationFrame(frame)
    }
    raf = requestAnimationFrame(frame)

    // The HUD reads a snapshot on a timer rather than on every frame: re-rendering the React
    // tree sixty times a second to move a fuel gauge is how a game loop becomes a slideshow.
    const pulse = window.setInterval(() => setHud(game.current), 120)
    return () => {
      cancelAnimationFrame(raf)
      window.clearInterval(pulse)
      r.dispose()
      renderer.current = null
    }
  }, [loaded])

  // Mouse. Right fires, left switches — and the context menu is suppressed only over the
  // canvas, so right-click still works normally everywhere else on the page.
  useEffect(() => {
    const canvas = canvasRef.current
    if (!canvas || !loaded) return
    const down = (e: MouseEvent) => {
      if (e.button === 2) {
        e.preventDefault()
        firing.current = true
      } else if (e.button === 0) {
        const g = game.current
        if (g) {
          game.current = { ...g, combat: switchWeapon(g.combat) }
          setWeapon(selected(game.current.combat).name)
        }
      }
    }
    const up = (e: MouseEvent) => {
      if (e.button === 2) firing.current = false
    }
    const menu = (e: Event) => e.preventDefault()
    canvas.addEventListener('mousedown', down)
    window.addEventListener('mouseup', up)
    canvas.addEventListener('contextmenu', menu)
    return () => {
      canvas.removeEventListener('mousedown', down)
      window.removeEventListener('mouseup', up)
      canvas.removeEventListener('contextmenu', menu)
    }
  }, [loaded])

  // Keyboard. Attached to the window so the canvas does not need focus to fly.
  useEffect(() => {
    const down = (e: KeyboardEvent) => {
      // Arrows and space scroll the page otherwise, which moves the HUD out from under the
      // canvas mid-flight.
      if (e.code === 'Space' || e.code.startsWith('Arrow')) e.preventDefault()
      keys.current.add(e.code)

      const g = game.current
      if (!g || !loaded) return
      // Services are single presses rather than held keys, so they are handled here rather
      // than in the tick — a held `F` should refuel once, not sixty times a second.
      if (e.code === 'KeyF') game.current = useService(g, 'refuel')
      else if (e.code === 'KeyR') game.current = useService(g, 'repair')
      else if (e.code === 'KeyV') game.current = useService(g, 'scavenge')
      else if (e.code === 'KeyM') setMarket((m) => !m)
      // The nav computer. With a thousand nodes over a sector this size, a destination you
      // cannot select is a destination you reach by luck.
      else if (e.code === 'Digit1') game.current = route(g, loaded.space, 'refuel')
      else if (e.code === 'Digit2') game.current = route(g, loaded.space, 'repair')
      else if (e.code === 'Digit3') game.current = route(g, loaded.space, 'trade')
      else if (e.code === 'Digit4') game.current = route(g, loaded.space, 'scavenge')
      else if (e.code === 'Digit0') game.current = { ...g, waypoint: null, notice: 'waypoint cleared' }
      if (game.current !== g) setHud(game.current)
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
          <a href="/omni" className="text-omni-dim underline-offset-4 hover:underline">
            verify a record
          </a>
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
              accept="application/json,.json,image/png,.png"
              className="hidden"
              onChange={(e) => {
                const f = e.target.files?.[0]
                if (f) void load(f)
              }}
            />
            <span className="text-omni-text">Choose a record — or a PNG that names one…</span>
          </label>
          {error && <p className="text-omni-invalid">{error}</p>}
          {ticket && (
            <div className="rounded border border-omni-border-hi p-4">
              <div className="text-omni-accent">This image names a world.</div>
              <div className="mt-1 break-all font-mono text-xs text-omni-text">{ticket}</div>
              <p className="mt-2 text-omni-dim">
                A PNG is a claim ticket, not a map. It says which record it was drawn from; it
                does not carry the record — the objects, signals and blind spots that make the
                space do not survive rasterisation. Drop that record here, or fetch it from a
                vault you hold the token for.
              </p>
              <div className="mt-3 flex flex-wrap gap-2">
                <input
                  value={vault}
                  onChange={(e) => setVault(e.target.value)}
                  placeholder="vault url, e.g. http://127.0.0.1:7843"
                  className="min-w-[16rem] flex-1 rounded border border-omni-border bg-black px-2 py-1 font-mono text-xs text-omni-text"
                />
                <input
                  value={holder}
                  onChange={(e) => setHolder(e.target.value)}
                  placeholder="your address"
                  className="min-w-[10rem] rounded border border-omni-border bg-black px-2 py-1 font-mono text-xs text-omni-text"
                />
                <button
                  type="button"
                  disabled={!vault || !holder || fetching}
                  onClick={() => void fromVault(ticket)}
                  className="rounded border border-omni-border-hi px-3 py-1 text-omni-text hover:border-omni-accent disabled:opacity-40"
                >
                  {fetching ? 'fetching…' : 'Fetch from vault'}
                </button>
              </div>
              {vaultMsg && (
                <p className={`mt-2 ${vaultRetryable ? 'text-omni-stale' : 'text-omni-invalid'}`}>
                  {vaultMsg}
                </p>
              )}
            </div>
          )}
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
            <Row k="throttle" v={`${Math.round((hud?.throttle ?? 0) * 100)}%`} />
            <Row
              k="fuel"
              v={
                hud
                  ? `${Math.round(hud.ship.fuel)}/${fuelCapacity(hud.ship.levels.tanks)}`
                  : '—'
              }
              alarm={!!hud && hud.ship.fuel <= 0}
            />
            <Row
              k="jump"
              v={hud ? `${hud.ship.jumpFuel}/${jumpCapacity(hud.ship.levels.drive)}` : '—'}
              alarm={!!hud && hud.ship.jumpFuel <= 0}
            />
            <Row k="salvage" v={hud ? String(hud.ship.salvage) : '—'} />
            <Row k="weapon" v={weapon} />
            <Row
              k="ammo"
              v={hud ? (selected(hud.combat).magazine === null ? '∞' : String(hud.combat.photonsLeft)) : '—'}
            />
            <Row k="destroyed" v={hud ? String(hud.combat.destroyed.length) : '—'} />
          </div>

          {/*
            Shields over hull, both as bars rather than fractions.
            A number is read; a bar is *seen*, and in a fight there is no time to read. The two
            are stacked in that order and coloured differently because they mean different
            things: the shield is a buffer that comes back, the hull is health that does not,
            and a player who cannot tell at a glance which one is being eaten cannot decide
            whether to press or break off — which is the only decision combat here is about.
          */}
          {hud && flying && (
            <div className="pointer-events-none absolute bottom-6 left-1/2 w-72 -translate-x-1/2 space-y-1 font-mono text-[11px]">
              <Gauge
                label="SHIELD"
                value={hud.ship.shield}
                max={shieldMax(hud.ship.levels.shields)}
                tone={hud.ship.shield <= 0 ? 'down' : 'shield'}
              />
              <Gauge
                label="HULL"
                value={hud.ship.hull}
                max={hullMax(hud.ship.levels.hull)}
                tone={hud.ship.hull < hullMax(hud.ship.levels.hull) * 0.3 ? 'down' : 'hull'}
              />
              {(() => {
                const charge = jumpProgress(hud.drive, hud.ship.levels.drive)
                if (hud.drive.phase === 'idle' && charge === 0) return null
                return (
                  <Gauge
                    label={hud.drive.phase === 'inhibited' ? 'JUMP INHIBITED' : 'JUMP'}
                    value={hud.drive.phase === 'inhibited' ? 1 : charge}
                    max={1}
                    tone={hud.drive.phase === 'inhibited' ? 'down' : 'jump'}
                  />
                )
              })()}
            </div>
          )}

          {/*
            The sensor readout. A ghost's threat still reads an em dash while it is shooting at
            you — the pressure to put a number there is strongest exactly here, and inventing one
            is what the whole project exists not to do.
          */}
          {hud && flying && nearestContact(hud) && (
            <div className="pointer-events-none absolute right-5 top-1/2 -translate-y-1/2 text-right font-mono text-[11px]">
              {(() => {
                const c = nearestContact(hud)!
                return (
                  <>
                    <div className="text-omni-dim">NEAREST</div>
                    <div className={c.range < JUMP_INHIBIT ? 'text-omni-invalid' : 'text-omni-text'}>
                      {c.spec.label}
                    </div>
                    <div className="text-omni-dim">{rangeLabel(c.range)}</div>
                    <div className="text-omni-dim">{c.behaviour.toUpperCase()}</div>
                  </>
                )
              })()}
            </div>
          )}

          {!flying && (
            <div className="pointer-events-none absolute inset-x-0 bottom-8 text-center font-mono text-xs text-omni-dim">
              <div>
                W/S pitch · A/D yaw · Q/E roll · ↑/↓ throttle level · X full stop ·
                ←/→ + SPACE/SHIFT thrusters
              </div>
              <div className="mt-1">
                RIGHT CLICK fire · LEFT CLICK switch weapon · F refuel · R repair · V scavenge ·
                M market
              </div>
              <div className="mt-1">
                1 route to fuel · 2 repair · 3 market · 4 salvage · 0 clear waypoint
              </div>
              <div className="mt-1 text-omni-accent">
                HOLD J to jump to the waypoint — the drive will not spin up with hostiles in
                range
              </div>
              <div className="mt-1">click the view to begin</div>
            </div>
          )}

          <div className="pointer-events-none absolute bottom-4 left-5 space-y-0.5 font-mono text-[11px]">
            {/*
              The nav computer. Range and bearing only — never a verdict about what is there,
              which is why it will route you to a phantom and label it one.
            */}
            {hud && (
              <div className="mb-2 space-y-0.5">
                <div className="text-omni-dim">NAV</div>
                {(() => {
                  const fix = hud.waypoint === null ? null : fixOn(s, hud.camera, hud.waypoint)
                  if (!fix) {
                    const near = nearest(s, hud.camera, 'refuel', 1)[0]
                    return (
                      <div className="text-omni-dim">
                        no waypoint —{' '}
                        {near
                          ? `nearest fuel ${rangeLabel(near.range)} ${bearingLabel(near)}`
                          : 'this sector has nowhere to refuel'}
                      </div>
                    )
                  }
                  return (
                    <div>
                      <span className="text-omni-valid">{truncate(fix.node.label, 26)}</span>{' '}
                      <span className="text-omni-dim">({fix.node.kind})</span>{' '}
                      <span className="text-omni-text">{rangeLabel(fix.range)}</span>{' '}
                      <span className={fix.ahead > 0.7 ? 'text-omni-valid' : 'text-omni-dim'}>
                        {bearingLabel(fix)}
                      </span>
                      {fix.node.kind === 'phantom' && (
                        <span className="text-omni-dim"> — simulated, may not be there</span>
                      )}
                    </div>
                  )
                })()}
              </div>
            )}

            {s.raiders.length > 0 && (
              <div className="text-omni-dim">
                {hud ? livingRaiders(hud) : s.raiders.length} raiders — not in the record
              </div>
            )}

            {s.contacts.length > 0 && (
              <>
              <div className="text-omni-dim">CONTACTS — threat as the record reported it</div>
              {s.contacts.slice(0, 6).map((c) => (
                <div key={c.id}>
                  <span className={c.hostility === 'hostile' ? 'text-omni-absent' : 'text-omni-valid'}>
                    {c.hostility === 'hostile' ? 'HOSTILE' : 'SALVAGE'}
                  </span>{' '}
                  <span className={c.solid ? 'text-omni-text' : 'text-omni-dim'}>
                    {threatLabel(c)}
                  </span>{' '}
                  <span className="text-omni-dim">
                    {c.solid ? '' : 'ghost — nobody measured this '}
                    {truncate(c.label, 30)}
                  </span>
                </div>
              ))}
              </>
            )}
          </div>

          {/*
            A damage vignette rather than a camera shake. Shaking the *canvas* would move the
            crosshair, which punishes the player twice for one hit — being shot should feel
            violent, not make aiming unfair.
          */}
          {hud && hud.shake > 0.02 && (
            <div
              className="pointer-events-none absolute inset-0"
              style={{
                boxShadow: `inset 0 0 ${Math.round(60 + hud.shake * 140)}px rgba(255,60,60,${(
                  hud.shake * 0.55
                ).toFixed(3)})`,
              }}
            />
          )}

          {hud?.notice && (
            <div className="pointer-events-none absolute inset-x-0 top-24 text-center font-mono text-xs text-omni-accent">
              {hud.notice}
            </div>
          )}

          {hud?.nearby && !market && (
            <div className="pointer-events-none absolute inset-x-0 bottom-20 text-center font-mono text-xs">
              <span className="text-omni-text">{hud.nearby.label}</span>{' '}
              <span className="text-omni-dim">
                ({hud.nearby.kind}) —{' '}
                {servicesOf(hud.nearby.kind)
                  .map((x) =>
                    x === 'refuel' ? 'F refuel' : x === 'repair' ? 'R repair'
                      : x === 'scavenge' ? 'V scavenge' : 'M market',
                  )
                  .join(' · ')}
              </span>
            </div>
          )}

          {market && hud && (
            <div className="absolute inset-x-0 bottom-16 mx-auto max-w-2xl rounded border border-omni-border-hi bg-black/90 p-4 font-mono text-xs">
              <div className="mb-2 flex items-baseline gap-3">
                <b className="text-omni-accent">OUTFITTING</b>
                <span className="text-omni-dim">
                  {hud.nearby && servicesOf(hud.nearby.kind).includes('trade')
                    ? hud.nearby.label
                    : 'no market in range — fly to a dock or a market'}
                </span>
                <span className="ml-auto text-omni-text">{hud.ship.salvage} salvage</span>
              </div>
              <div className="grid grid-cols-2 gap-1 sm:grid-cols-3">
                {(Object.keys(UPGRADES) as Component[]).map((c) => {
                  const lvl = hud.ship.levels[c]
                  const cost = upgradeCost(c, lvl)
                  const can =
                    cost !== null &&
                    hud.ship.salvage >= cost &&
                    !!hud.nearby &&
                    servicesOf(hud.nearby.kind).includes('trade')
                  return (
                    <button
                      key={c}
                      type="button"
                      disabled={!can}
                      onClick={() => {
                        const g = game.current
                        if (!g) return
                        game.current = purchase(g, c)
                        setHud(game.current)
                      }}
                      className="rounded border border-omni-border px-2 py-1 text-left hover:border-omni-accent disabled:opacity-40"
                    >
                      <div className="text-omni-text">
                        {UPGRADES[c].label}{' '}
                        <span className="text-omni-dim">
                          {'▰'.repeat(lvl)}
                          {'▱'.repeat(MAX_LEVEL - lvl)}
                        </span>
                      </div>
                      <div className="text-omni-dim">{UPGRADES[c].effect}</div>
                      <div className={can ? 'text-omni-valid' : 'text-omni-dim'}>
                        {cost === null ? 'maxed' : `${cost} salvage`}
                      </div>
                    </button>
                  )
                })}
              </div>
              <div className="mt-2 text-omni-dim">
                Salvage comes from destroying hostiles and stripping derelicts — never from
                anything the record reports. A world with more blind spots is not worth more;
                it is worth the same and is harder to survive.
              </div>
            </div>
          )}

          {hud?.lost && (
            <div className="absolute inset-0 flex items-center justify-center bg-black/70 font-mono">
              <div className="text-center">
                <div className="text-lg text-omni-invalid">HULL BREACH</div>
                <div className="mt-2 text-omni-dim">reload the record to fly it again</div>
              </div>
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

/** Raiders still flying, for the sensor line. Counts the swarm, not the record. */
function livingRaiders(g: GameState): number {
  return g.swarm.craft.filter((c) => c.alive && c.id.startsWith('raider:')).length
}

/**
 * One HUD bar.
 *
 * The bar is the message and the number is the footnote, which is the opposite of the rest of
 * this project's readouts — and correct here, because the reader is being shot at. `tone` names
 * a role and `globals.css` owns the colour, same rule as everywhere else.
 */
function Gauge({
  label,
  value,
  max,
  tone,
}: {
  label: string
  value: number
  max: number
  tone: 'hull' | 'shield' | 'jump' | 'down'
}) {
  const pct = max <= 0 ? 0 : Math.max(0, Math.min(1, value / max))
  const colour = {
    hull: 'bg-omni-accent',
    shield: 'bg-omni-valid',
    jump: 'bg-omni-text',
    down: 'bg-omni-invalid',
  }[tone]
  return (
    <div>
      <div className="flex justify-between text-omni-dim">
        <span>{label}</span>
        <span>{max === 1 ? `${Math.round(pct * 100)}%` : `${Math.round(value)}/${Math.round(max)}`}</span>
      </div>
      <div className="h-1.5 w-full bg-omni-border">
        <div className={`h-full ${colour}`} style={{ width: `${pct * 100}%` }} />
      </div>
    </div>
  )
}

function truncate(sIn: string, n: number): string {
  return sIn.length <= n ? sIn : `${sIn.slice(0, n - 1)}…`
}

function Row({ k, v, alarm }: { k: string; v: string; alarm?: boolean }) {
  return (
    <div>
      <span className="text-omni-dim">{k} </span>
      <span className={alarm ? 'text-omni-absent' : 'text-omni-text'}>{v}</span>
    </div>
  )
}
