/**
 * Keeping the sector populated.
 *
 * ## The problem
 *
 * A sector was a resource that depleted. Eighteen raider wings and eighteen marshals were placed
 * at generation and that was every hostile the world would ever contain, so an hour of play left
 * a volume with almost nothing in it — and the emptiness arrived *unevenly*, concentrated exactly
 * where the player had spent time, which reads as the game running out rather than as a place.
 *
 * The marshals made it worse rather than better. They hunt raiders whether or not anybody is
 * watching, which is the mechanic that makes the sector feel inhabited — and with a fixed
 * population it is also a mechanic that runs to completion. Left alone for long enough the patrol
 * wins, and the ambient violence that the whole faction exists to produce stops happening. The
 * two sides have to be *replenished* for a standing fight to be a standing fight.
 *
 * ## Three rules
 *
 * 1. **Reinforcements arrive far away, and on a timer.** A ship appearing inside sensor range is
 *    the single clearest way to tell a player that nothing they are looking at is real, and an
 *    instant refill means the sector cannot be thinned at all — clearing the space around you has
 *    to be worth something for a while.
 * 2. **Capitals come back slowly, one at a time, and never near you.** This rule used to read
 *    *never replaced*, and the argument for it was good — a leviathan you spent four minutes
 *    killing was the only lasting mark you could leave. What it cost was a sector that ran out of
 *    the only fights worth crossing it for, on both sides: once the last warden and the last
 *    bastion were gone, every large silhouette on the horizon was hostile again and the question
 *    the patrol's capitals exist to create stopped being asked. `CAPITAL_INTERVAL_MS` keeps both
 *    halves — a kill still buys minutes of a quieter sector, which is a *pace* rather than a
 *    permanent dent, and the same distinction the fighter floor had to learn. They are also the
 *    one reinforcement that does **not** warp in near the player: a titan's own awareness reaches
 *    0.60 of the sector, so one materialising at arrival range would be hunting you before its
 *    entry effect finished.
 * 3. **Nothing here reads the record.** Same rule as `raiders.ts` and `factions.ts`, in the place
 *    it would be easiest to break: tie the floor or the interval to `blind_spots` or to the node
 *    count and a producer has bought itself a quieter sector by misreporting. Floors and intervals
 *    are constants; only the seed decides *where* a wave appears. `check:scemaworld` asserts this
 *    file reads no record field.
 *
 * ## What determinism survives, stated precisely
 *
 * The sector's opening population is a pure function of the record and stays that way. Whether a
 * *reinforcement* happens at all cannot be: it depends on who died, which depends on how the
 * player flew. What is preserved is the sequence — wave `n` of a given record always has the same
 * composition and the same anchor — so two players holding one record still fly the same sector
 * made of the same things, and diverge only in when a wave shows up. That is the most that is
 * available, and pretending otherwise would be the more comfortable lie.
 */

import type { Space, Vec3 } from './generate.ts'
import * as Enemy from './enemy.ts'
import type { Swarm } from './enemy.ts'
import {
  civilianReinforcement, marshalReinforcement, strengthOf,
  MARSHAL_CAPITALS, MARSHAL_STRENGTH, TRAFFIC,
} from './factions.ts'
import {
  raiderWing, raiderCapital, garrisonStrength,
  GARRISON, RAIDER_FLOOR, RAIDER_STRENGTH, WINGS,
} from './raiders.ts'
import { ARRIVAL_MS, ARRIVAL_SPREAD, arrivalPoint, landed, type Arrival } from './arrivals.ts'
import {
  CLUSTER_SPREAD, CLUSTER_STRENGTH, MARSHAL_ORDER, MAX_CLUSTERS, RAIDER_ORDER,
  clusterAnchors, clusterOf, clusterReplacement,
} from './clusters.ts'
import { AGGRO_RANGE, SENSOR_MULTIPLIER } from './scale.ts'
import { ALL_CLASS_IDS, CLASSES, type ClassId } from './classes.ts'

/**
 * How far a reinforcement is placed when it does **not** warp in.
 *
 * Still the rule for the opening roster and the fallback when there is no player to warp relative
 * to. Beyond sensor range: a ship appearing there out of nothing would put a contact on the sensor
 * board with no cause, and the board is the one surface the player is trained to believe.
 *
 * Reinforcements during play use `arrivals.ts` instead, which supplies the cause. See that module
 * for why a witnessed warp-in dissolves the objection rather than trading against it.
 */
export const SPAWN_CLEARANCE = Math.round(AGGRO_RANGE * SENSOR_MULTIPLIER * 1.6)

/**
 * Milliseconds between raider wings.
 *
 * Was 22 seconds, which reads as an empty sector: at the sector's current size a player spends
 * most of a minute crossing it, so three quarters of a minute between encounters meant flying a
 * long way to meet nothing. Nine seconds keeps the sector busy without the arrivals overlapping
 * each other's entry effects.
 */
export const RAIDER_INTERVAL_MS = 9_000

/**
 * How many craft drop out of hyperspace together.
 *
 * Fewer than a generated wing carries, because these arrive *near* the player rather than
 * somewhere in the volume. Four hostiles materialising inside engagement range is an ambush the
 * player had no way to avoid; a wing announced by a visible entry is an encounter they can
 * decline.
 *
 * Four now rather than three, and the reason the number could move at all is that the arrivals are
 * genuinely *visible*: the cone was 72 degrees wide against a 66-degree field of view, so a wing
 * that was supposed to announce itself often materialised off-screen. With the entries where the
 * player is looking, a larger wing reads as a formation rather than as an ambush.
 */
const WARP_WING = 4

/**
 * How far apart, in milliseconds, the ships of one wing finish their entry.
 *
 * A wing used to share a single `dueMs`, so three hulls appeared on the same frame — which does
 * not read as a formation arriving, it reads as the sector gaining three ships at once. Staggering
 * it makes the entry an *event with a duration*: the streaks resolve one after another and the eye
 * gets to follow them.
 *
 * Small enough that the wing is unmistakably one wing. A longer stagger and it becomes three
 * separate arrivals that happen to share a bearing, which is a different and less useful reading.
 */
const WARP_STAGGER_MS = 220

/**
 * Milliseconds between raider wings when the sector is **contested** — below `RAIDER_FLOOR`.
 *
 * A gutted sector coming back at the cruising rate takes a quarter of an hour, which nobody
 * waits through: the measured recovery from a full purge reached the old target in nine minutes
 * and would have needed fifteen to reach a full complement. Surging below the floor makes the
 * deficit close at a pace somebody actually sees, while the ordinary trickle above it keeps a
 * cleared region cleared for long enough to be worth having cleared.
 */
export const RAIDER_SURGE_MS = 3_500

/** Milliseconds between marshal replacements. Shorter: they arrive singly, not four at a time. */
export const MARSHAL_INTERVAL_MS = 13_000

/**
 * Milliseconds between civilian replacements.
 *
 * Traffic is the largest population in the sector and the one with no defence, so it needs the
 * shortest interval of the three or it never keeps up with what the raiders take. It arrives
 * singly: a wing of couriers is not a thing.
 */
export const TRAFFIC_INTERVAL_MS = 7_000

/**
 * Milliseconds between capital replacements, **across both sides**.
 *
 * Two and a half minutes, and the figure is the whole of what keeps the old *never replaced* rule's
 * intent alive. A capital kill is minutes of work and it has to buy something; what it buys is a
 * quieter sector for a while, rather than a permanently smaller one. One shared timer rather than
 * one per faction, so the war classes stay genuinely rare: a full wipe of one side's four capitals
 * takes ten minutes to come back, and a wipe of both takes twenty.
 */
export const CAPITAL_INTERVAL_MS = 150_000

/**
 * Milliseconds between cluster reinforcements.
 *
 * Faster than a wing, because a cluster is meant to be a standing battle rather than an encounter
 * — the whole value of one is that it is *still going on* when you get there, and three sides of a
 * twenty-nine-craft fight burn through each other quickly.
 */
export const CLUSTER_INTERVAL_MS = 5_000

/**
 * How far below full a cluster has to fall before it is topped up, as a fraction.
 *
 * Not "any shortfall". A cluster that is instantly restored to full strength is a fight nobody can
 * affect, which makes it scenery — the exact thing this feature exists to stop the sector being.
 * Letting it run down to two thirds before reinforcing means a player who spends time in one can
 * see the battle turn, and can turn it.
 */
export const CLUSTER_FLOOR = 0.66

/**
 * How far from the player a replacement capital is placed.
 *
 * **Derived from the largest capital's own `aggro`, never written down as a number.** The whole
 * point of the clearance is that the ship is not already hunting you when it exists, and that
 * distance is a property of the biggest war class in the table — so adding a bigger one has to
 * move this by construction. A constant here is the shape of defect that stays invisible until
 * somebody adds the ship that outgrows it, which is the same failure as the arrival box that
 * covered the sector until `TRUNK` grew.
 *
 * Floored at `SPAWN_CLEARANCE` so it can never come out *below* the rule every other reinforcement
 * obeys.
 */
export const CAPITAL_CLEARANCE = Math.max(
  SPAWN_CLEARANCE,
  Math.round(Math.max(...ALL_CLASS_IDS.filter((id) => CLASSES[id].capital).map((id) => CLASSES[id].aggro)) * 1.6),
)

/**
 * How many waves of each have been raised, and when the next may be.
 *
 * `raiders` starts at `WINGS` because the sector opens with wings `0 .. WINGS-1` already placed —
 * the counter is the *next* index, so a respawned wing continues the same deterministic sequence
 * rather than re-raising one that already flew.
 */
export interface Waves {
  raiders: number
  marshals: number
  /**
   * Civilian waves raised, per `faction:class`.
   *
   * A record rather than a field per faction, because the roster is data and a shape that has to
   * grow a field every time somebody adds a class is a shape that stops being updated.
   */
  civilians: Record<string, number>
  /**
   * Capital waves raised, per class, per side.
   *
   * Keyed by class rather than counted per faction, because a replacement restores the class that
   * is *missing*: a dead bastion is answered by a bastion. A single counter per side would make a
   * warden's placement a function of how many bastions had died, which is both wrong and
   * untraceable when it goes wrong.
   */
  raiderCapitals: Record<string, number>
  marshalCapitals: Record<string, number>
  nextRaiderMs: number
  nextMarshalMs: number
  nextTrafficMs: number
  /** When a capital may next be replaced. One timer, shared by both sides — see the constant. */
  nextCapitalMs: number
  /**
   * Reinforcements sent to each cluster so far, per side.
   *
   * Per cluster, so a battle that has been fought over for ten minutes does not draw its
   * replacements from the same slot as one nobody has visited.
   */
  clusterWaves: Record<string, number>
  nextClusterMs: number
  /**
   * Craft mid-warp: drawn, not yet in the swarm.
   *
   * Here rather than on the swarm because an arrival is **not a craft**. It cannot be shot, cannot
   * shoot, and cannot be collided with — the honest reading of something that has not arrived, and
   * it removes the unpleasant case of killing a reinforcement before it finishes materialising.
   */
  arriving: Arrival[]
}

export function newWaves(): Waves {
  return {
    raiders: WINGS,
    marshals: 0,
    civilians: {},
    raiderCapitals: {},
    marshalCapitals: {},
    nextRaiderMs: -1e9,
    nextMarshalMs: -1e9,
    nextTrafficMs: -1e9,
    // **One interval, not `-1e9` and not `0`.** The other three open immediately, which is right
    // for them: a sector that starts a wing short should fill it. A capital roster starts complete,
    // so an already-open timer does nothing at all until the first capital dies — and then fires on
    // the very next frame, which makes the kill worth nothing. `0` looks closed and is not, because
    // the clock starts at zero: measured, a capital came back inside the first interval and the
    // test caught it. Starting one interval out costs nothing and makes the first replacement a
    // fixed interval after the kill rather than after the tick that noticed it.
    nextCapitalMs: CAPITAL_INTERVAL_MS,
    clusterWaves: {},
    nextClusterMs: -1e9,
    arriving: [],
  }
}

/**
 * A deterministic unit vector for a wave, so an entry bearing is a function of the seed and the
 * wave number rather than of a clock.
 *
 * Hand-rolled rather than reusing `Rng`: this needs three components from one integer, and the
 * generator is a *stream* — taking draws from it here would desynchronise the placement streams
 * that `raiders.ts` and `factions.ts` seek through by index.
 */
function bearing(seed: string, tag: string, n: number): Vec3 {
  let h = 2166136261 >>> 0
  for (const t of [seed, tag, String(n)]) {
    for (let i = 0; i < t.length; i += 1) {
      h ^= t.charCodeAt(i)
      h = Math.imul(h, 16777619) >>> 0
    }
  }
  const x = ((h % 17) - 8) / 8
  const y = (((h >>> 5) % 13) - 6) / 6
  const z = (((h >>> 10) % 19) - 9) / 9
  const l = Math.hypot(x, y, z) || 1
  return { x: x / l, y: y / l, z: z / l }
}

/** The wave-counter key for one roster line. */
function keyOf(faction: string, klass: string): string {
  return `${faction}:${klass}`
}

/** Which class a civilian faction flies, or null if it is not one the sector tops up. */
function classOfTraffic(faction: string): (typeof TRAFFIC)[number]['klass'] | null {
  return TRAFFIC.find((t) => t.faction === faction)?.klass ?? null
}

/**
 * Live craft of a faction, counting only a given class — capitals are excluded from a floor.
 *
 * **Cluster craft are excluded too**, and that exclusion is load-bearing. A cluster carries
 * twenty-nine craft (`clusters.ts::CLUSTER_STRENGTH`) and there are three of them, so counting
 * them against the scattered roster's strength would tell the sector it is permanently full — the
 * wings and the patrol would stop being replaced entirely, and the symptom would be an empty
 * sector with three busy corners. It is the same class of error as the fighter floor that was also
 * its own target: a counter measuring something other than what it is used to decide.
 */
function countOf(swarm: Swarm, faction: 'raider' | 'marshal', capitals: boolean): number {
  return swarm.craft.filter(
    (c) =>
      c.alive &&
      c.faction === faction &&
      c.spec.capital === capitals &&
      clusterOf(c.id) === null,
  ).length
}

/** How many craft are still flying in one cluster, both sides. */
function clusterCount(swarm: Swarm, index: number): number {
  return swarm.craft.filter((c) => c.alive && clusterOf(c.id) === index).length
}

export interface Replenished {
  swarm: Swarm
  waves: Waves
  /** A line for the HUD when something arrived, so a wave is announced rather than merely true. */
  notice: string | null
}

/**
 * Top the sector back up, at most one wave of each per call.
 *
 * Cheap enough to run every tick: two linear passes over a swarm of a couple of hundred entries,
 * and both are skipped entirely by the timer check on all but a handful of frames.
 */
export function replenish(
  swarm: Swarm,
  space: Space,
  seed: string,
  waves: Waves,
  playerAt: Vec3,
  playerFacing: Vec3,
  nowMs: number,
): Replenished {
  let out = swarm
  let notice: string | null = null
  let arriving = waves.arriving
  let raiders = waves.raiders
  let marshals = waves.marshals
  let nextRaiderMs = waves.nextRaiderMs
  let nextMarshalMs = waves.nextMarshalMs
  let nextTrafficMs = waves.nextTrafficMs ?? -1e9
  let civilians = waves.civilians ?? {}
  // `??` on every one of these, because a `Waves` written before capitals existed has none of
  // them — the same reason `civilians` is defensive above. A saved wave state must not be a reason
  // a session crashes.
  let raiderCapitals = waves.raiderCapitals ?? {}
  let marshalCapitals = waves.marshalCapitals ?? {}
  let nextCapitalMs = waves.nextCapitalMs ?? CAPITAL_INTERVAL_MS
  let clusterWaves = waves.clusterWaves ?? {}
  let nextClusterMs = waves.nextClusterMs ?? -1e9

  // ── anything that finished warping in becomes a craft ──────────────────────
  const due = arriving.filter((a) => landed(a, nowMs))
  if (due.length > 0) {
    arriving = arriving.filter((a) => !landed(a, nowMs))
    for (const a of due) {
      if (a.faction === 'raider') {
        // One craft, placed exactly where its streak ended. `raiderWing` still supplies the class
        // roll and the provenance, so an arrival is the same *kind* of thing as a raider that was
        // there at generation — it only got here differently. A clearance of zero because the
        // position is already decided.
        const one = raiderWing(seed, raiders, a.at, 0)
          .slice(0, 1)
          .map((c) => ({ ...c, id: a.id, at: a.at }))
        out = Enemy.reinforce(out, Enemy.swarmOf(one, seed).craft)
      } else if (a.faction === 'marshal') {
        const civ = marshalReinforcement(space, seed, marshals, a.at, 0)
        out = Enemy.reinforce(
          out,
          Enemy.withTraffic({ craft: [], shots: [] }, [{ ...civ, id: a.id, at: a.at }]).craft,
        )
      } else {
        // Traffic. It arrives with a destination, so a courier that drops out of hyperspace next
        // to you immediately sets off for somewhere — which is the difference between the sector
        // gaining a ship and the sector gaining a delivery.
        const klass = classOfTraffic(a.faction)
        if (klass) {
          const civ = civilianReinforcement(
            space, seed, a.faction, klass, civilians[keyOf(a.faction, klass)] ?? 0, a.at, 0,
          )
          out = Enemy.reinforce(
            out,
            Enemy.withTraffic({ craft: [], shots: [] }, [{ ...civ, id: a.id, at: a.at }]).craft,
          )
        }
      }
    }
  }

  // ── open a new entry when the sector is short ──────────────────────────────
  // Counted against what is already **on its way** as well as what is flying, or a wing is ordered
  // several times over while the first of it is still materialising.
  const pending = (f: string) => arriving.filter((a) => a.faction === f).length

  // Fighters only. A dead capital is not a shortfall — see rule 2.
  //
  // Against `RAIDER_STRENGTH`, the **full** complement, not `RAIDER_FLOOR`. The floor used to be
  // both the trigger and the target, so a cleared sector came back to 60% of what it started with
  // and stayed there: measured at 43 of 72, permanently, with nothing saying so. The floor now
  // decides the *pace* instead — below it the sector is contested and reinforcement surges.
  const shortRaiders = countOf(swarm, 'raider', false) + pending('raider')
  if (nowMs >= nextRaiderMs && shortRaiders < RAIDER_STRENGTH) {
    const dir = bearing(seed, ':raider-entry:', raiders)
    // A wing arrives as a wing: several entries at once along one bearing, so what the player sees
    // is a formation dropping out of hyperspace rather than a ship appearing.
    const wing: Arrival[] = []
    for (let i = 0; i < WARP_WING; i += 1) {
      const jitter = bearing(seed, ':raider-spread:', raiders * 8 + i)
      wing.push({
        id: `raider:warp:${raiders}:${i}`,
        faction: 'raider',
        at: arrivalPoint(playerAt, playerFacing, jitter, ARRIVAL_SPREAD),
        dir,
        dueMs: nowMs + ARRIVAL_MS + i * WARP_STAGGER_MS,
      })
    }
    arriving = [...arriving, ...wing]
    raiders += 1
    nextRaiderMs = nowMs + (shortRaiders < RAIDER_FLOOR ? RAIDER_SURGE_MS : RAIDER_INTERVAL_MS)
    notice = 'hyperspace signature — raider wing inbound'
  }

  if (
    nowMs >= nextMarshalMs &&
    countOf(swarm, 'marshal', false) + pending('marshal') < MARSHAL_STRENGTH
  ) {
    const jitter = bearing(seed, ':marshal-spread:', marshals)
    arriving = [
      ...arriving,
      {
        id: `marshal:warp:${marshals}`,
        faction: 'marshal',
        at: arrivalPoint(playerAt, playerFacing, jitter, ARRIVAL_SPREAD),
        dir: bearing(seed, ':marshal-entry:', marshals),
        dueMs: nowMs + ARRIVAL_MS,
      },
    ]
    marshals += 1
    nextMarshalMs = nowMs + MARSHAL_INTERVAL_MS
    // The raider line wins the notice if both fired this frame. A wing of hostiles is the one the
    // player has to act on, and two notices in one frame means only the last is read.
    notice = notice ?? 'hyperspace signature — patrol inbound'
  }

  // ── traffic ────────────────────────────────────────────────────────────────
  //
  // The population that had no replenishment path at all. Raiders hunt couriers and freighters,
  // so a sector left running lost all of both — 34 and 14 down to zero, permanently — and the two
  // factions that make the place look inhabited quietly stopped existing. Nothing anywhere said
  // so, because nothing was measuring it: the sector still had ships in it, they were just all
  // shooting at each other.
  //
  // **The faction furthest below its roster strength goes first**, rather than a fixed order.
  // Round-robin would spend a slot on a faction that is one short while another is wiped out, and
  // the wiped-out one is the one you can see is missing.
  if (nowMs >= nextTrafficMs) {
    let worst: { faction: string; klass: string; deficit: number } | null = null
    for (const t of TRAFFIC) {
      const want = strengthOf(t.faction, t.klass)
      // Cluster marshals excluded, for the same reason `countOf` excludes them: thirty-nine
      // patrol craft standing in three battles would otherwise report the patrol as three times
      // over strength, and the scattered marshals would never be replaced again.
      const have = swarm.craft.filter(
        (c) => c.alive && c.faction === t.faction && !c.spec.capital && clusterOf(c.id) === null,
      ).length
      const deficit = want - (have + pending(t.faction))
      if (deficit > 0 && (!worst || deficit > worst.deficit)) {
        worst = { faction: t.faction, klass: t.klass, deficit }
      }
    }
    if (worst) {
      const k = keyOf(worst.faction, worst.klass)
      const n = civilians[k] ?? 0
      arriving = [
        ...arriving,
        {
          id: `${worst.faction}:warp:${n}`,
          faction: worst.faction as Arrival['faction'],
          at: arrivalPoint(playerAt, playerFacing, bearing(seed, `:${k}-spread:`, n), ARRIVAL_SPREAD),
          dir: bearing(seed, `:${k}-entry:`, n),
          dueMs: nowMs + ARRIVAL_MS,
        },
      ]
      civilians = { ...civilians, [k]: n + 1 }
      nextTrafficMs = nowMs + TRAFFIC_INTERVAL_MS
      // Never the headline. A hostile wing and a patrol are both things the player has to decide
      // about; a courier arriving is the sector working, and a notice for it would push the two
      // that matter off the screen.
      notice = notice ?? null
    }
  }

  // ── the firefight clusters ─────────────────────────────────────────────────
  //
  // Three standing battles, kept standing. Without this a cluster is a fight that resolves once
  // and leaves a corpse in the map — which is worse than no cluster at all, because the sector
  // then contains a place that *used to be* interesting and gives no sign of it.
  //
  // **Reinforcements are held while the player is inside the cluster**, and that is the rule that
  // makes one worth fighting. Two reasons, and they point the same way. A craft materialising
  // inside sensor range is the clearest possible way to tell somebody that nothing they are
  // looking at is real — the rule the whole module is built on. And a battle that refills itself
  // while you are standing in it is a battle you cannot affect, so the only thing your presence
  // would change is how long you can be shot at. Fight in a cluster and it genuinely thins out;
  // leave, and it comes back.
  if (nowMs >= nextClusterMs) {
    const anchors = clusterAnchors(seed)
    let worst: { index: number; deficit: number } | null = null
    for (let i = 0; i < MAX_CLUSTERS; i += 1) {
      const a = anchors[i]
      const away = Math.hypot(playerAt.x - a.x, playerAt.y - a.y, playerAt.z - a.z)
      // Measured against the cluster's own extent plus the usual clearance, so "inside it" means
      // what it looks like rather than a bare radius.
      if (away < AGGRO_RANGE * CLUSTER_SPREAD + SPAWN_CLEARANCE) continue
      const deficit = Math.round(CLUSTER_STRENGTH * CLUSTER_FLOOR) - clusterCount(swarm, i)
      if (deficit > 0 && (!worst || deficit > worst.deficit)) worst = { index: i, deficit }
    }
    if (worst) {
      const i = worst.index
      // Whichever side is further below its own half of the order of battle. A single counter
      // would rebuild a cluster as one faction, and a cluster with one side in it is not a battle.
      const side = (f: 'raider' | 'marshal') =>
        swarm.craft.filter((c) => c.alive && clusterOf(c.id) === i && c.faction === f).length
      const raiderShort = RAIDER_ORDER.length - side('raider')
      const marshalShort = MARSHAL_ORDER.length - side('marshal')
      const faction: 'raider' | 'marshal' = raiderShort >= marshalShort ? 'raider' : 'marshal'
      const order = faction === 'raider' ? RAIDER_ORDER : MARSHAL_ORDER
      const key = `${i}:${faction}`
      const n = clusterWaves[key] ?? 0
      // The order of battle, cycled. A cluster's composition is a constant, so a replacement takes
      // the next slot in the same list rather than rolling — the sector's battles are all of a
      // known shape, which is what makes them readable from a distance.
      const klass = order[n % order.length]
      const dir = bearing(seed, `:cluster-${key}:`, n)
      const spread = AGGRO_RANGE * CLUSTER_SPREAD
      const at = {
        x: anchors[i].x + dir.x * spread,
        y: anchors[i].y + dir.y * spread,
        z: anchors[i].z + dir.z * spread,
      }
      if (faction === 'raider') {
        // The contact literal lives in `clusters.ts`, not here — see `clusterReplacement`. This
        // file is scanned for the record's own field names, and a `magnitude` written inline
        // would trip a check that cannot tell a constant from a read, and is right not to try.
        out = Enemy.reinforce(
          out,
          Enemy.swarmOf([clusterReplacement(i, faction, klass, at, n)], seed).craft,
        )
      } else {
        out = Enemy.reinforce(
          out,
          Enemy.withTraffic({ craft: [], shots: [] }, [
            { id: `cluster:${i}:marshal:+${n}`, faction: 'marshal', spec: CLASSES[klass], at, destination: null },
          ]).craft,
        )
      }
      clusterWaves = { ...clusterWaves, [key]: n + 1 }
    }
    nextClusterMs = nowMs + CLUSTER_INTERVAL_MS
  }

  // ── capitals ───────────────────────────────────────────────────────────────
  //
  // **They used to be never replaced**, and the reasoning was good: a leviathan you spent four
  // minutes killing is the only lasting mark you can leave on a sector. The cost was that the
  // sector ran out of the only fights worth crossing it for — on *both* sides, so once the last
  // warden and the last bastion were gone every large silhouette on the horizon was hostile again,
  // and the question the patrol's capitals exist to create stopped being asked.
  //
  // Three things keep the original reasoning intact rather than discarding it:
  //
  // 1. **One capital per `CAPITAL_INTERVAL_MS`, across both sides.** A kill buys minutes of a
  //    quieter sector. That is a *pace* rather than a permanent dent, which is the same distinction
  //    `RAIDER_STRENGTH` had to learn about the fighter floor.
  // 2. **The class that is missing is the class that returns.** A dead bastion is answered by a
  //    bastion. Cycling a roster would let the composition drift away from the one every sector is
  //    supposed to share, and the drift would be invisible.
  // 3. **They do not warp in near the player.** Every other reinforcement arrives through
  //    `arrivals.ts`, a few seconds of streaks at `ARRIVAL_SPREAD` from the nose — which is right
  //    for a wing you can decline and catastrophic for a titan, whose own `aggro` reaches 0.60 of
  //    the sector. A capital that materialised at arrival range would be engaging you before its
  //    entry effect finished. So a capital is placed the way the opening roster is: far outside
  //    sensor range, where nothing appears on a board with no cause.
  if (nowMs >= nextCapitalMs) {
    // The **larger deficit goes first**, across both factions, exactly as the traffic block picks
    // the faction furthest below strength. A fixed order spends the slot on a side that is one
    // short while the other is wiped out, and the wiped-out one is the one you can see is missing.
    let worst: { faction: 'raider' | 'marshal'; klass: ClassId; deficit: number } | null = null
    // **Cluster craft excluded**, the same exclusion `countOf` makes and for a sharper reason
    // here. Each cluster carries a `warden`, so three of them field three wardens between them —
    // exactly the patrol's roster strength. Counting those against the roster reported the
    // capitals as complete, and the three *scattered* wardens a player had killed were never
    // replaced: measured at 0 back of 3, with nothing saying so. A counter measuring a different
    // population from the one it is used to decide about is the shape of defect this file has now
    // produced twice.
    const consider = (faction: 'raider' | 'marshal', klass: ClassId, want: number) => {
      const have = swarm.craft.filter(
        (c) => c.alive && c.faction === faction && c.spec.id === klass && clusterOf(c.id) === null,
      ).length
      const deficit = want - have
      if (deficit > 0 && (!worst || deficit > worst.deficit)) worst = { faction, klass, deficit }
    }
    for (const klass of new Set(GARRISON)) consider('raider', klass, garrisonStrength(klass))
    for (const c of MARSHAL_CAPITALS) consider('marshal', c.klass, c.count)

    if (worst) {
      const { faction, klass } = worst as { faction: 'raider' | 'marshal'; klass: ClassId }
      const counters = faction === 'raider' ? raiderCapitals : marshalCapitals
      const n = counters[klass] ?? 0
      // Derived from the largest capital's own awareness rather than written down, so adding a
      // bigger war class moves the clearance with it. A constant here is the shape of bug that
      // does not appear until somebody adds the ship that outgrows it.
      const clear = CAPITAL_CLEARANCE
      if (faction === 'raider') {
        const one = raiderCapital(seed, klass, n, playerAt, clear)
        out = Enemy.reinforce(out, Enemy.swarmOf([one], seed).craft)
        raiderCapitals = { ...raiderCapitals, [klass]: n + 1 }
      } else {
        const civ = civilianReinforcement(space, seed, 'marshal', klass, n, playerAt, clear)
        out = Enemy.reinforce(out, Enemy.withTraffic({ craft: [], shots: [] }, [civ]).craft)
        marshalCapitals = { ...marshalCapitals, [klass]: n + 1 }
      }
      nextCapitalMs = nowMs + CAPITAL_INTERVAL_MS
      // **The headline, when it fires.** It outranks the wing and the patrol because it is the
      // rarest event the sector produces and the one most worth changing a plan over — and because
      // it is the only reinforcement the player cannot see arrive, so the line is the entire cue.
      // It says *distant* rather than implying a contact: the ship is beyond sensor range and the
      // board will not show it for a while, and a notice that read like a resolved contact would
      // be the game claiming a reading it has not taken.
      notice = `long-range signature — ${CLASSES[klass].label} under way, far outside sensor range`
    } else {
      // Nothing missing. Re-check on the ordinary cadence rather than every frame; the scan is two
      // passes over the swarm and there is no reason to pay for it sixty times a second.
      nextCapitalMs = nowMs + CAPITAL_INTERVAL_MS
    }
  }

  return {
    swarm: out,
    waves: {
      raiders,
      marshals,
      civilians,
      raiderCapitals,
      marshalCapitals,
      clusterWaves,
      nextClusterMs,
      nextRaiderMs,
      nextMarshalMs,
      nextTrafficMs,
      nextCapitalMs,
      arriving,
    },
    notice,
  }
}
