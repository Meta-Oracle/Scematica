/**
 * Sparks and detonations, as line segments.
 *
 * ## Why particles here are lines and not sprites
 *
 * Every visible thing in this game is a wireframe (`meshes.ts`) drawn on an additive pass with
 * depth writes off, and there is no post-processing and no texture anywhere in the renderer. A
 * billboarded sprite would be the only textured object on screen, would need its own program, its
 * own blend state and its own sorting — and would still look like a decal pasted over a line
 * drawing.
 *
 * A **shard** costs none of that. It is two points and a brightness, which is exactly what
 * `Segment` already is, so sparks and explosions ride the pass the bolts and the hyperspace
 * streaks already ride. They inherit the one property that makes that pass work: brightness sums
 * where shards overlap, so a dense burst clips to white at its core without anybody computing a
 * core.
 *
 * ## Pure, seeded, and blind to the record
 *
 * Nothing here reads a world. A hit looks the same in every sector, because it is a fact about a
 * collision rather than about what somebody perceived — the same rule `arrivals.ts` states for
 * hyperspace entries.
 *
 * Every burst is a pure function of `(seed, id, kind, age)`. Two consequences, both wanted: the
 * same explosion looks the same on two machines, and a burst needs no per-particle state on
 * `GameState` — the tick stores an origin and a start time, and the renderer derives the shards.
 * Storing a thousand particles and integrating them would be the obvious design and would put a
 * per-frame allocation loop in a tick that is already the frame's largest cost.
 */

import type { Vec3 } from './generate.ts'
import { R_PHOTON } from './scale.ts'

/** One drawn shard: a short bright line, fading with the burst. */
export interface Shard {
  from: Vec3
  to: Vec3
  alpha: number
  /** Which event this came from, so the renderer can colour it without re-deriving anything. */
  kind: BurstKind
}

/**
 * What produced a burst. The three differ in *shape*, not merely in size, because a player has to
 * be able to tell them apart out of the corner of an eye.
 */
export type BurstKind =
  /** A shield soaking a hit: a tight, short-lived flare that does not throw debris. */
  | 'shield'
  /** A round reaching hull: fewer shards, longer, thrown outward — it reads as material leaving. */
  | 'hull'
  /** A photon detonating, or a craft dying. The big one. */
  | 'detonation'

export interface Burst {
  /** Where it happened. */
  at: Vec3
  kind: BurstKind
  /** When it started, in the tick's own clock. */
  startedMs: number
  /** Stable per burst, so the same explosion draws the same shards on every machine. */
  seed: number
}

/** How long each kind lives, in milliseconds. */
export const BURST_MS: Record<BurstKind, number> = {
  // Short. A shield flare that outlasts the shot reads as damage rather than as absorption, which
  // is the one distinction the whole shield mechanic rests on (`classes.ts`).
  shield: 180,
  hull: 420,
  // Long enough to be an event and short enough not to obscure the fight it happened in.
  detonation: 900,
}

/** How many shards each kind throws. */
export const BURST_SHARDS: Record<BurstKind, number> = {
  shield: 10,
  hull: 14,
  detonation: 34,
}

/**
 * How far the shards reach, as a multiple of a photon's own radius.
 *
 * Expressed against `R_PHOTON` rather than as a fraction of the sector, because a burst is a
 * *ship-scale* event: it has to read next to a hull, not next to a station. Every other distance in
 * the game is a fraction of `EXTENT` for the opposite reason, and mixing the two is how a spark
 * ends up either invisible or the size of a market.
 */
export const BURST_REACH: Record<BurstKind, number> = {
  shield: 4,
  hull: 9,
  detonation: 26,
}

/**
 * A cheap deterministic scalar in [0,1) from three integers.
 *
 * Hand-rolled rather than `Rng`, and for the reason `respawn.ts::bearing` gives: the generator is a
 * *stream*, and drawing from it here would desynchronise the placement streams other modules seek
 * through by index. This needs three uncorrelated numbers from one burst id, not a sequence.
 */
function hash(a: number, b: number, c: number): number {
  let h = 2166136261 >>> 0
  for (const v of [a, b, c]) {
    h ^= v & 0xff
    h = Math.imul(h, 16777619) >>> 0
    h ^= (v >>> 8) & 0xff
    h = Math.imul(h, 16777619) >>> 0
    h ^= (v >>> 16) & 0xff
    h = Math.imul(h, 16777619) >>> 0
  }
  return (h >>> 8) / 0x1000000
}

/** How far through its life a burst is, 0..1. Past 1 it is finished and draws nothing. */
export function ageOf(b: Burst, nowMs: number): number {
  return (nowMs - b.startedMs) / BURST_MS[b.kind]
}

/**
 * The shards of one burst at the current time.
 *
 * Returns an empty array once the burst is over, so a caller can filter on `length` rather than
 * carrying a separate liveness flag — one authority for whether a burst still exists.
 *
 * The shards **decelerate** (`t` eased by a square root) rather than travelling at a constant
 * speed. Debris that moves linearly for its whole life reads as a firework; debris that leaps and
 * then slows reads as something coming apart, which is the difference between a burst that looks
 * like an event and one that looks like an effect.
 */
export function shardsOf(b: Burst, nowMs: number): Shard[] {
  const t = ageOf(b, nowMs)
  if (t < 0 || t >= 1) return []
  const n = BURST_SHARDS[b.kind]
  const reach = R_PHOTON * BURST_REACH[b.kind]
  // Fast at first, then slowing. `sqrt` is the cheapest curve with the right shape.
  const spread = reach * Math.sqrt(t)
  // Each shard is a streak whose length shrinks as it slows — a fast fragment smears, a slow one
  // does not, which is the same cue `SPEED_LASER`'s bolt length carries.
  const len = reach * 0.28 * (1 - t)
  const out: Shard[] = []
  for (let i = 0; i < n; i += 1) {
    // A direction on the sphere, from the burst's own seed. Not `Math.random`: two machines
    // drawing the same explosion differently is the same class of defect as two machines
    // generating different sectors.
    const u = hash(b.seed, i, 1) * 2 - 1
    const a = hash(b.seed, i, 2) * Math.PI * 2
    const r = Math.sqrt(Math.max(0, 1 - u * u))
    const dx = r * Math.cos(a)
    const dy = r * Math.sin(a)
    const dz = u
    // Uneven speeds, or every shard sits on one expanding shell and the burst reads as a bubble.
    const v = 0.55 + hash(b.seed, i, 3) * 0.45
    const head = spread * v
    const tail = Math.max(0, head - len)
    out.push({
      from: {
        x: Math.round(b.at.x + dx * tail),
        y: Math.round(b.at.y + dy * tail),
        z: Math.round(b.at.z + dz * tail),
      },
      to: {
        x: Math.round(b.at.x + dx * head),
        y: Math.round(b.at.y + dy * head),
        z: Math.round(b.at.z + dz * head),
      },
      // Bright immediately and fading out. The opposite of a hyperspace entry, which brightens as
      // it closes — an arrival is a thing about to happen and a burst is a thing that just did.
      alpha: Math.max(0, 1 - t) * (b.kind === 'detonation' ? 1 : 0.8),
      kind: b.kind,
    })
  }
  return out
}

/**
 * How many bursts may be alive at once.
 *
 * A cap rather than a hope. A cluster firefight resolves dozens of hits a second, and an
 * uncapped list is an unbounded allocation whose symptom is the frame rate — the same failure the
 * projectile lifetime change had to be defended against with an axis reject. Oldest first, because
 * the newest burst is the one the player is looking at.
 */
export const MAX_BURSTS = 48

/** Add a burst, dropping the oldest if the cap is reached. */
export function add(bursts: Burst[], b: Burst): Burst[] {
  const next = bursts.length >= MAX_BURSTS ? bursts.slice(bursts.length - MAX_BURSTS + 1) : bursts
  return [...next, b]
}

/** Drop everything that has finished. Called once a tick, so the list cannot grow without bound. */
export function live(bursts: Burst[], nowMs: number): Burst[] {
  return bursts.filter((b) => ageOf(b, nowMs) < 1)
}
