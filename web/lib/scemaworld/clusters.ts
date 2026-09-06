/**
 * Firefight clusters: three standing battles, in three different parts of the sector.
 *
 * ## What was missing
 *
 * The sector already had ambient violence — marshals hunt raiders whether or not anybody is
 * watching, and `respawn.ts` exists so that war does not run to completion. What it did not have
 * was violence you could *see from a distance and fly toward*. The roster is scattered through a
 * volume nearly twelve extents across (`SECTOR_REACH`), so an eighteen-wing raider force and an
 * eighteen-strong patrol meet in ones and twos, at random, wherever they happen to overlap. Every
 * individual fight is real and almost none of them is findable.
 *
 * A cluster is the opposite: a lot of both sides, in one place, already fighting. It gives the
 * sector something a scatter cannot — a **destination that is not a station**.
 *
 * ## Three, and why the count is a constant
 *
 * Same rule as `raiders.ts::WINGS` and `factions.ts::ROSTER`, and it is the rule this whole
 * project is built on: **nothing here reads the record.** Not the node count, not `blind_spots`,
 * not the extent. A world that reported more would otherwise buy itself a busier or a quieter
 * sector, and a producer with a reason to misreport is the one failure the design cannot absorb.
 * `check:scemaworld` asserts this file reads no record field.
 *
 * Three is also a play decision rather than a technical one: one is a set piece, five is a sector
 * where you cannot go anywhere without being in somebody's battle, and three means there is always
 * one you have not been to.
 *
 * ## Where they sit
 *
 * The first is **near** — reachable in the first minute or two, so a new pilot finds one without
 * being told it exists. The other two are **farther out but not distant**: far enough that getting
 * there is a decision about fuel, close enough that you would go. Radii are fractions of
 * `SECTOR_REACH`, never absolute, because this sector has been resized twice and both times a
 * hand-written distance survived the change while quietly meaning something else.
 *
 * Directions are drawn from the seed and then **forced apart**: three random directions in a
 * sphere are frequently two directions, and two clusters on top of each other is one cluster and
 * an empty sector. `SEPARATION` is the minimum angle between any two, enforced by rejection.
 */

import { Rng } from '../omni/fractal.ts'
import type { Contact, Vec3 } from './generate.ts'
import type { Civilian } from './factions.ts'
import { CLASSES, type ClassId } from './classes.ts'
import { AGGRO_RANGE, SECTOR_REACH } from './scale.ts'

/** How many standing battles the sector carries. A constant, never a function of the record. */
export const MAX_CLUSTERS = 3

/**
 * Where each cluster sits, as a fraction of `SECTOR_REACH` from the origin.
 *
 * The first is close enough to find by accident. The other two are a trip — "farther out but not
 * too far" is the whole brief, and 0.55 and 0.8 of the reach are between two and four minutes at
 * cruise, which is a journey rather than an expedition.
 */
export const CLUSTER_RADII = [0.22, 0.55, 0.8]

/**
 * Minimum angle between any two cluster bearings, in radians.
 *
 * Three unconstrained draws on a sphere land within 40 degrees of each other about a fifth of the
 * time, and two clusters that close are one cluster with a gap in it. Two radians is a little
 * under 115 degrees, which is as spread as three directions can reliably be.
 */
export const SEPARATION = 2.0

/** How wide a cluster is, as a multiple of engagement range. */
export const CLUSTER_SPREAD = 2.4

/**
 * The order of battle in one cluster.
 *
 * Deliberately lopsided and deliberately *not* a fair fight: raiders outnumber the patrol, and the
 * patrol brings a war class the raiders have to work around. A cluster that resolves in thirty
 * seconds is a corpse by the time you arrive, and one that is perfectly balanced never resolves at
 * all — what a player should find is a fight that is being slowly lost by somebody, so that
 * turning up matters.
 *
 * Both sides are named rather than rolled, for the same reason `raiders.ts::GARRISON` is: a roll
 * makes the sector's composition a lottery, and the interesting thing about a cluster is that
 * every one of them is a battle of a known shape.
 */
export const RAIDER_ORDER: ClassId[] = [
  'interceptor', 'interceptor', 'interceptor', 'interceptor', 'interceptor', 'interceptor',
  'lancer', 'lancer', 'lancer', 'lancer',
  'gunship', 'gunship', 'gunship',
  'skiff', 'skiff',
  'warfighter',
]

export const MARSHAL_ORDER: ClassId[] = [
  'marshal', 'marshal', 'marshal', 'marshal', 'marshal', 'marshal', 'marshal', 'marshal',
  'marshal', 'marshal', 'marshal', 'marshal',
  'warden',
]

/** How many craft a full cluster holds, both sides. Used by the top-up floor. */
export const CLUSTER_STRENGTH = RAIDER_ORDER.length + MARSHAL_ORDER.length

/**
 * A deterministic unit vector from a seed and an index.
 *
 * Hand-rolled rather than drawn from `Rng`: this needs three components from one integer, and the
 * generator is a *stream* — taking draws here would desynchronise the placement streams that
 * `raiders.ts` and `factions.ts` seek through by index. Same reasoning, and the same shape, as
 * `respawn.ts::bearing`.
 */
function bearing(seed: string, n: number): Vec3 {
  let h = 2166136261 >>> 0
  for (const t of [seed, ':cluster:', String(n)]) {
    for (let i = 0; i < t.length; i += 1) {
      h ^= t.charCodeAt(i)
      h = Math.imul(h, 16777619) >>> 0
    }
  }
  const x = ((h % 2003) - 1001) / 1001
  const y = (((h >>> 7) % 2003) - 1001) / 1001
  const z = (((h >>> 15) % 2003) - 1001) / 1001
  const l = Math.hypot(x, y, z) || 1
  return { x: x / l, y: y / l, z: z / l }
}

function angleBetween(a: Vec3, b: Vec3): number {
  const d = Math.max(-1, Math.min(1, a.x * b.x + a.y * b.y + a.z * b.z))
  return Math.acos(d)
}

/**
 * Where the sector's clusters are, in order: near first.
 *
 * Rejection-sampled against `SEPARATION` with a bounded number of attempts, and the bound is the
 * point: an unbounded search terminates on almost every seed and hangs on the one that matters.
 * When the budget runs out the best candidate so far is taken, which is a worse sector and not a
 * frozen one.
 */
export function clusterAnchors(seed: string): Vec3[] {
  const dirs: Vec3[] = []
  for (let i = 0; i < MAX_CLUSTERS; i += 1) {
    let best: Vec3 | null = null
    let bestGap = -1
    for (let attempt = 0; attempt < 48; attempt += 1) {
      const d = bearing(seed, i * 64 + attempt)
      const gap = dirs.length === 0 ? Math.PI : Math.min(...dirs.map((o) => angleBetween(o, d)))
      if (gap > bestGap) {
        best = d
        bestGap = gap
      }
      if (gap >= SEPARATION) break
    }
    dirs.push(best ?? { x: 0, y: 0, z: 1 })
  }
  return dirs.map((d, i) => {
    const r = SECTOR_REACH * CLUSTER_RADII[i]
    return { x: d.x * r, y: d.y * r, z: d.z * r }
  })
}

/** A position inside a cluster, scattered around its anchor. */
function around(rng: Rng, anchor: Vec3, spread: number): Vec3 {
  // Drawn small and scaled up. `Rng.below(n)` is `next() % n` on a 32-bit generator, so any `n`
  // past 2^32 silently returns an unrelated uniform — the defect that once exiled the whole
  // hostile roster into one corner of the sector. See `raiders.ts::scatter`.
  const pick = () => (rng.below(2001) - 1000) / 1000
  return {
    x: anchor.x + pick() * spread,
    y: anchor.y + pick() * spread,
    z: anchor.z + pick() * spread,
  }
}

/**
 * The hostile half of every cluster, as contacts the swarm can build craft from.
 *
 * `unlogged: true` on every one, exactly as `raiders.ts` does: **a raider is not a contact.**
 * Game furniture must never become indistinguishable from a signal somebody counted, and a cluster
 * is the most furniture-like thing in the sector.
 */
export function clusterRaiders(seed: string): Contact[] {
  // A slice of the digest nothing else uses. `Rng` reads only the first eight hex characters, so a
  // suffix would hand this the fractal's own stream and shift every node in the sector.
  const rng = new Rng(seed.slice(12, 20) || seed.slice(4, 12) || seed)
  const spread = AGGRO_RANGE * CLUSTER_SPREAD
  const out: Contact[] = []
  clusterAnchors(seed).forEach((anchor, c) => {
    RAIDER_ORDER.forEach((klass, i) => {
      out.push({
        id: `cluster:${c}:raider:${i}`,
        at: around(rng, anchor, spread),
        hostility: 'hostile',
        solid: true,
        magnitude: 0.5,
        label: 'RAIDER',
        unlogged: true,
        // Named, never rolled. `swarmOf` honours it.
        klass,
      })
    })
  })
  return out
}

/**
 * The patrol half. Civilians rather than contacts, because that is how `factions.ts` models
 * anything that is not a raider — and a cluster marshal has to be the same kind of thing as a
 * roster marshal or the two drift in how they fly.
 *
 * `destination: null` throughout: these are not running a route, they are in a battle. A marshal
 * handed a delivery route in the middle of a firefight sets off across the sector.
 */
export function clusterMarshals(seed: string): Civilian[] {
  const rng = new Rng(seed.slice(20, 28) || seed.slice(12, 20) || seed)
  const spread = AGGRO_RANGE * CLUSTER_SPREAD
  const out: Civilian[] = []
  clusterAnchors(seed).forEach((anchor, c) => {
    MARSHAL_ORDER.forEach((klass, i) => {
      out.push({
        id: `cluster:${c}:marshal:${i}`,
        faction: 'marshal',
        spec: CLASSES[klass],
        at: around(rng, anchor, spread),
        destination: null,
      })
    })
  })
  return out
}

/**
 * A **replacement** for one cluster, of a named class.
 *
 * Built here rather than inline in `respawn.ts`, and the reason is a rule rather than tidiness:
 * `check:scemaworld` scans `respawn.ts` for the record's own field names, because tying a floor or
 * an interval to `magnitude` or `blind_spots` is how a producer buys itself a quieter sector. A
 * cluster contact needs a `magnitude` field — the contract requires one, and 0.5 here is a
 * constant with nothing to do with any record — but the scan is textual and cannot tell a
 * constant from a read. It is right not to try: a scan that started reasoning about what a
 * mention *means* would stop catching the thing it exists for.
 *
 * So the literal lives in the file that is allowed to write one, and `respawn.ts` calls a
 * function. Same treatment `raiders.ts` already gets.
 */
export function clusterReplacement(
  index: number,
  faction: 'raider' | 'marshal',
  klass: ClassId,
  at: Vec3,
  wave: number,
): Contact {
  return {
    id: `cluster:${index}:${faction}:+${wave}`,
    at,
    hostility: 'hostile',
    solid: true,
    magnitude: 0.5,
    label: 'RAIDER',
    unlogged: true,
    klass,
  }
}

/** Which cluster an id belongs to, or `null` if it is not cluster furniture. */
export function clusterOf(id: string): number | null {
  const m = /^cluster:(\d+):/.exec(id)
  return m ? Number(m[1]) : null
}
