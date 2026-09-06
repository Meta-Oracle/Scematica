/**
 * Sound, synthesised on the spot. No assets, no files, nothing to fetch.
 *
 * ## Why procedural rather than samples
 *
 * Three reasons, in the order they matter.
 *
 * **There is nowhere to put a sample.** `/scema-world` has no server side — that is a standing
 * claim of the page, not an implementation detail — and the record it flies arrives from the
 * reader's own disk. A pack of `.ogg` files would be the only thing on the page fetched from
 * anywhere, and the first thing to fail behind a corporate proxy.
 *
 * **A wireframe game wants synthetic sound.** Everything visible here is lines on black; sampled
 * explosions would be the audio equivalent of pasting a photograph into a diagram.
 *
 * **It is a few hundred bytes.** An oscillator, a noise burst and an envelope cover everything
 * this game does, and the whole module costs less than one compressed sample.
 *
 * ## The rules
 *
 * - **Nothing plays until the player asks.** Browsers suspend an `AudioContext` created outside a
 *   gesture, and a page that starts a context on load gets a console warning and silence — which
 *   presents as "the sound is broken" rather than as "the browser refused". `resume()` is called
 *   from a real event, and until then `ready` is false and every `play` is a no-op.
 * - **A hard voice cap, not a hope.** A cluster firefight resolves dozens of hits a second
 *   (`clusters.ts`), and every one of them would like a sound. Past `MAX_VOICES` the oldest is
 *   stopped: the alternative is a hundred simultaneous oscillators, which is both a CPU problem
 *   and — because gain sums — an unbearable one.
 * - **Distance gates before the cap does.** A detonation a third of a sector away is not audible,
 *   and gating on range is what stops the cap ever being the thing that decides *which* sounds you
 *   hear. A cap that is doing the choosing is a mixer, and a mixer that picks by arrival order
 *   picks wrong.
 * - **Mute is remembered, and it is the default nowhere.** `localStorage`, wrapped in try/catch
 *   like every other read of it in this project: a private window throws on access, and a game
 *   that crashes because it could not remember a volume setting is worse than one that forgets.
 *
 * Pure of the record, like `fx.ts` and `arrivals.ts`: a laser sounds the same in every sector,
 * because it is a fact about a gun rather than about what somebody perceived.
 */

/** Every sound this game makes. A closed list — a name that is not here cannot be played. */
export type Cue =
  | 'laser'
  | 'photon'
  | 'shield'
  | 'hull'
  | 'kill'
  | 'jump-charge'
  | 'jump-fire'
  | 'dock'
  | 'warn'

/**
 * The most voices that may sound at once.
 *
 * Sixteen. Enough that a busy exchange is busy; few enough that the sum of their gains cannot
 * clip the master and turn a firefight into a wall of noise.
 */
export const MAX_VOICES = 16

/** Beyond this, in world units, a cue is not played at all. Passed in by the caller. */
export const AUDIBLE = 1

interface Voice {
  stop: (t: number) => void
  startedAt: number
}

export interface Engine {
  /** True once a gesture has resumed the context. Until then every `play` does nothing. */
  ready: boolean
  muted: boolean
  play: (cue: Cue, gain?: number) => void
  /** Call from a real user gesture — a click or a keypress. Safe to call repeatedly. */
  resume: () => Promise<void>
  setMuted: (m: boolean) => void
  /** Release the context. Called when the canvas unmounts. */
  close: () => void
}

const KEY = 'scemaworld.muted'

function rememberedMute(): boolean {
  try {
    return globalThis.localStorage?.getItem(KEY) === '1'
  } catch {
    // A private window throws on access. Forgetting a preference is not a reason to fail.
    return false
  }
}

/**
 * A dead engine, for a browser with no WebAudio and for the check script.
 *
 * Returned rather than `null` so no caller has to branch. A silent engine that answers every
 * method is the same shape as a working one, which is the difference between "sound is off" and
 * "sound is a special case every call site handles".
 */
export function silentEngine(): Engine {
  return {
    ready: false,
    muted: true,
    play: () => {},
    resume: async () => {},
    setMuted: () => {},
    close: () => {},
  }
}

/**
 * Build the engine. Returns a silent one when there is no WebAudio.
 *
 * The context is created here and **suspended** — creating it is allowed anywhere, resuming it is
 * not. That split is what lets the rest of the game hold an engine from the first frame while
 * still obeying the gesture rule.
 */
export function createEngine(): Engine {
  const Ctx =
    typeof globalThis !== 'undefined'
      ? ((globalThis as unknown as { AudioContext?: typeof AudioContext }).AudioContext ??
        (globalThis as unknown as { webkitAudioContext?: typeof AudioContext }).webkitAudioContext)
      : undefined
  if (!Ctx) return silentEngine()

  let ctx: AudioContext
  try {
    ctx = new Ctx()
  } catch {
    return silentEngine()
  }

  const master = ctx.createGain()
  master.gain.value = 0.35
  master.connect(ctx.destination)

  const voices: Voice[] = []
  const engine: Engine = {
    ready: false,
    muted: rememberedMute(),
    resume: async () => {
      try {
        await ctx.resume()
        engine.ready = ctx.state === 'running'
      } catch {
        engine.ready = false
      }
    },
    setMuted: (m) => {
      engine.muted = m
      master.gain.value = m ? 0 : 0.35
      try {
        globalThis.localStorage?.setItem(KEY, m ? '1' : '0')
      } catch {
        /* see `rememberedMute` */
      }
    },
    close: () => {
      try {
        void ctx.close()
      } catch {
        /* closing an already-closed context throws in some browsers */
      }
    },
    play: (cue, gain = 1) => {
      if (!engine.ready || engine.muted || gain <= 0) return
      const now = ctx.currentTime
      // The cap, enforced by stopping the oldest. Not by refusing the newest: the sound you are
      // most likely to need is the one that just happened.
      while (voices.length >= MAX_VOICES) {
        const v = voices.shift()
        v?.stop(now)
      }
      const v = voiceFor(ctx, master, cue, now, gain)
      if (v) voices.push(v)
      // Sweep anything that finished. Cheap, and it keeps the array from being a leak in a long
      // session — the cap bounds concurrency, not lifetime.
      for (let i = voices.length - 1; i >= 0; i -= 1) {
        if (now - voices[i].startedAt > 3) voices.splice(i, 1)
      }
    },
  }
  master.gain.value = engine.muted ? 0 : 0.35
  return engine
}

/** Shared noise, built once: a fresh buffer per shot is an allocation per gunshot. */
let noise: AudioBuffer | null = null
function noiseBuffer(ctx: AudioContext): AudioBuffer {
  if (noise && noise.sampleRate === ctx.sampleRate) return noise
  const n = Math.floor(ctx.sampleRate * 0.5)
  const buf = ctx.createBuffer(1, n, ctx.sampleRate)
  const d = buf.getChannelData(0)
  // A deterministic fill rather than `Math.random`, for the same reason `fx.ts` seeds its shards:
  // nothing here should differ between two machines without a reason, and a fixed noise bed is
  // one less thing that can.
  let h = 22695477
  for (let i = 0; i < n; i += 1) {
    h = (Math.imul(h, 1103515245) + 12345) >>> 0
    d[i] = (h / 0x80000000) - 1
  }
  noise = buf
  return buf
}

/**
 * One voice per cue.
 *
 * Each is an oscillator or a noise burst with an envelope, and the shapes are chosen so the cues
 * are distinguishable *without* being loud — a game whose feedback depends on volume is one people
 * play muted. A shield is a short high blip, hull is lower and rougher, a kill is a filtered noise
 * fall, and the jump is a rising tone that a player can time the commitment window against.
 */
function voiceFor(
  ctx: AudioContext,
  out: GainNode,
  cue: Cue,
  now: number,
  gain: number,
): Voice | null {
  const g = ctx.createGain()
  g.connect(out)

  const tone = (type: OscillatorType, f0: number, f1: number, dur: number, peak: number) => {
    const o = ctx.createOscillator()
    o.type = type
    o.frequency.setValueAtTime(f0, now)
    o.frequency.exponentialRampToValueAtTime(Math.max(1, f1), now + dur)
    g.gain.setValueAtTime(0.0001, now)
    g.gain.exponentialRampToValueAtTime(Math.max(0.0002, peak * gain), now + 0.008)
    g.gain.exponentialRampToValueAtTime(0.0001, now + dur)
    o.connect(g)
    o.start(now)
    o.stop(now + dur + 0.02)
    return { stop: (t: number) => { try { o.stop(t) } catch { /* already stopped */ } }, startedAt: now }
  }

  const burst = (dur: number, peak: number, cutoff: number, sweepTo: number) => {
    const src = ctx.createBufferSource()
    src.buffer = noiseBuffer(ctx)
    const filter = ctx.createBiquadFilter()
    filter.type = 'lowpass'
    filter.frequency.setValueAtTime(cutoff, now)
    filter.frequency.exponentialRampToValueAtTime(Math.max(60, sweepTo), now + dur)
    g.gain.setValueAtTime(Math.max(0.0002, peak * gain), now)
    g.gain.exponentialRampToValueAtTime(0.0001, now + dur)
    src.connect(filter)
    filter.connect(g)
    src.start(now)
    src.stop(now + dur + 0.02)
    return { stop: (t: number) => { try { src.stop(t) } catch { /* already stopped */ } }, startedAt: now }
  }

  switch (cue) {
    // Short, bright, and quiet. It fires nine times a second on a stock ship, so anything with a
    // tail becomes a drone within a second of holding the trigger.
    case 'laser':
      return tone('square', 1500, 620, 0.05, 0.09)
    // A single decisive round deserves weight the laser does not have.
    case 'photon':
      return tone('sawtooth', 320, 90, 0.28, 0.22)
    case 'shield':
      return tone('sine', 900, 1400, 0.09, 0.12)
    case 'hull':
      return burst(0.18, 0.3, 1800, 220)
    case 'kill':
      return burst(0.7, 0.45, 2600, 90)
    // The spin-up. Rising, so the commitment window is audible without looking at the readout —
    // which is the point of a two-and-a-half-second charge you are meant to be able to abort.
    case 'jump-charge':
      return tone('triangle', 110, 880, 2.4, 0.14)
    case 'jump-fire':
      return tone('sine', 1400, 60, 0.5, 0.3)
    case 'dock':
      return tone('sine', 440, 660, 0.14, 0.14)
    case 'warn':
      return tone('square', 220, 220, 0.16, 0.16)
    default:
      return null
  }
}

/**
 * How loud something at `range` should be, 0..1.
 *
 * Linear rather than inverse-square, and deliberately: an inverse-square falloff over a sector
 * eleven extents across makes everything but the thing touching you inaudible, which is physically
 * defensible and useless. What the gate is *for* is stopping a firefight on the far side of the
 * sector from filling the mixer, and a linear ramp to silence at `audible` does that while leaving
 * a distant exchange faintly present — which is the sound of a place being inhabited.
 */
export function attenuate(range: number, audible: number): number {
  if (!(audible > 0)) return 0
  return Math.max(0, Math.min(1, 1 - range / audible))
}
