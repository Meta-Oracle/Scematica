/**
 * Contracts: what a faction will pay you to go and do.
 *
 * Pure. No clock, no input, no GL, and — the load-bearing one — **no record field anywhere near a
 * reward**. `check:scemaworld` asserts that by source scan, alongside `ship.ts`, `raiders.ts`,
 * `factions.ts`, `respawn.ts`, `claim.ts` and `roles.ts`.
 *
 * ## Why a quest may not read the record, stated once more because this is where it is tempting
 *
 * The obvious quest is "destroy the six contacts this record reported" or "visit every rift". Both
 * are one line, both are more interesting than what is here, and both are the defect this project
 * has already shipped twice: attach a payout to `blind_spots` and you have paid somebody to hide
 * them; attach one to signal magnitude and you have paid them to understate it. A record that can
 * buy itself a fatter contract is a record worth forging.
 *
 * So a contract's *terms* — kind, target count, tier, reward — come from the seed, the faction, the
 * role and the contract's index, and from nothing else. Its **subject** may be a node, because a
 * node is a place and using the map is not the same as being paid by it: a haul to node 41 pays
 * what a haul pays whether node 41 is a market with nine signals or an empty waypoint.
 *
 * ## Progress is measured in acts
 *
 * A contract advances when you *do* something — destroy a craft, dock somewhere. Nothing here
 * inspects the world to decide whether you have "found" enough; there is no state a player could
 * reach by flying carefully and reading a panel. That is the same asymmetry as `calibration.rs`
 * and `claim.ts`: the game pays for what it watched you do.
 *
 * ## Abandonment is free and refusal is a first-class answer
 *
 * You may drop a contract at any time with no penalty, and a faction that will not deal with your
 * role says so with a reason rather than showing an empty board. A locked board with no
 * explanation is indistinguishable from a broken one — the same lesson as the station panel that
 * refused three services in a notice that faded in three seconds.
 */

import { Rng } from '../omni/fractal.ts'
import type { Faction } from './factions.ts'
import type { Node } from './generate.ts'
import { roleOf, servedBy, type RoleId } from './roles.ts'

/**
 * What a contract asks for.
 *
 * Four kinds, and each maps onto a thing the game already simulates rather than onto a counter
 * bolted beside it — a bounty resolves on the same kill the swarm already reports, a haul on the
 * same docking the station panel already detects. A quest kind with its own private mechanic is a
 * second game running next to the first.
 */
export type QuestKind =
  /** Destroy `count` craft of the contract's faction. The bread and butter of a hunting role. */
  | 'bounty'
  /** Destroy `count` **capital** hulls. Rare, slow, and the only contract worth a hull upgrade. */
  | 'capital'
  /** Dock at `from`, then at `to`. Trade. */
  | 'haul'
  /** A haul the patrol scans for: pays far better and puts the marshals on you while it is live. */
  | 'contraband'

export interface Quest {
  /** Stable within a world: `${faction}:${role}:${index}`. Used for accept/complete bookkeeping. */
  id: string
  kind: QuestKind
  /** Who is offering, and therefore who has to be willing to deal with your role. */
  faction: Faction
  /** Plain text, built from the terms rather than written per quest. */
  title: string
  /** How many kills or legs are required. */
  count: number
  /** Salvage paid on completion. */
  reward: number
  /** Node ids for a haul. `null` for a hunting contract. */
  from: number | null
  to: number | null
  /**
   * The faction whose craft count toward a `bounty` or `capital` contract.
   *
   * Held separately from `faction` because a marshal citadel pays you to hunt **raiders**, not
   * marshals. Conflating the two made every bounty contract self-referential in the first draft.
   */
  quarry: Faction | null
}

/** A contract in progress. */
export interface Active {
  quest: Quest
  /** Kills so far, or legs of a haul completed. */
  progress: number
  /** For a haul: whether the pickup has been made. */
  picked: boolean
}

export interface QuestState {
  /** At most one contract at a time. A board of parallel objectives is a checklist, not a job. */
  active: Active | null
  /** Ids completed, so a board never re-offers finished work. */
  done: string[]
  /** Total salvage earned through contracts, for the record and for the HUD. */
  earned: number
}

export function newQuests(): QuestState {
  return { active: null, done: [], earned: 0 }
}

/**
 * How many contracts a faction board shows at once.
 *
 * Three. Enough that the board is a choice; few enough that it is read rather than scanned.
 */
export const BOARD_SIZE = 3

/**
 * Reward per unit of work, by kind.
 *
 * Flat tables so a change is visible in a diff, and expressed per *unit* so the reward for a
 * contract is `rate x count` and cannot drift away from the effort it asks for. Contraband is
 * where the money is, and the reason is on the tin: it is the only haul that makes an entire
 * faction hostile for as long as it is in your hold.
 */
const RATE: Record<QuestKind, number> = {
  bounty: 120,
  capital: 2600,
  haul: 260,
  contraband: 900,
}

/** Which kinds a role may be offered, by the faction offering them. */
function kindsFor(role: RoleId, faction: Faction): QuestKind[] {
  const r = roleOf(role)
  const out: QuestKind[] = []
  // A hunting contract is only offered against something the role actually hunts — otherwise the
  // board would pay a trader for kills, which is the role distinction dissolving.
  if (r.hunts.length > 0) out.push('bounty', 'capital')
  if (role === 'trader' || role === 'smuggler') out.push('haul')
  if (role === 'smuggler' || role === 'pirate') out.push('contraband')
  // A marshal citadel does not commission contraband. Stated here rather than in the caller so
  // the board and the validator cannot disagree about it.
  return faction === 'marshal' ? out.filter((k) => k !== 'contraband') : out
}

/**
 * The board a faction shows a given role, in a given world.
 *
 * A pure function of `(seed, faction, role)` — deterministic, so two players holding one record
 * see the same offers, and reproducible, so a board can be regenerated rather than stored.
 * `done` only filters; it never changes the terms of what remains.
 */
export function board(
  seed: string,
  faction: Faction,
  role: RoleId,
  nodes: Node[],
  done: string[] = [],
): Quest[] {
  // **The refusal decides, not the caller.** `board` used to generate offers regardless and rely
  // on every caller checking `refusal` first, which is two sources of truth for one question —
  // and it showed: a marshal citadel happily generated "destroy 2 marshal capital hulls" for a
  // pirate it had already refused to open its rings to.
  if (refusal(faction, role) !== null) return []
  const kinds = kindsFor(role, faction)
  if (kinds.length === 0) return []

  // A fourth slice of the digest, so contracts share a stream with neither the fractal, the
  // raiders, nor the traffic. Adding a suffix would not do it — `Rng` reads eight hex characters.
  const rng = new Rng(seed.slice(0, 8) || seed)
  // Fold the faction and role in so a marshal board and a raider board are not the same list
  // wearing different labels.
  const salt = [...`${faction}:${role}`].reduce((a, c) => (a * 31 + c.charCodeAt(0)) % 997, 11)
  for (let i = 0; i < salt; i += 1) rng.below(1024)

  const out: Quest[] = []
  for (let i = 0; out.length < BOARD_SIZE && i < BOARD_SIZE * 4; i += 1) {
    // ## Every kind of work available here appears before any kind repeats
    //
    // Round-robin for the first pass, random after. A purely random draw is what it was, and on
    // the parity record it handed the smuggler three hauls and no contraband — the one kind the
    // entire role exists for, missing from its only board, decided by a coin. A board is also
    // simply more informative when it shows what *kinds* of work a station has rather than a
    // sample that might be three of the same.
    const kind = i < kinds.length ? kinds[i] : kinds[rng.below(kinds.length)]
    const q = build(kind, faction, role, i, rng, nodes)
    // Deduplicated by **content**, not by id: ids carry the index, so two identical "destroy 1
    // raider capital hull" offers had different ids and both survived. A board showing the same
    // job twice reads as a bug in the board rather than as a choice.
    const same = (a: Quest, b: Quest) =>
      a.kind === b.kind && a.count === b.count && a.from === b.from && a.to === b.to
    if (q && !done.includes(q.id) && !out.some((o) => same(o, q))) out.push(q)
  }
  return out
}

function build(
  kind: QuestKind,
  faction: Faction,
  role: RoleId,
  index: number,
  rng: Rng,
  nodes: Node[],
): Quest | null {
  const id = `${faction}:${role}:${index}`
  // The quarry is whatever the role hunts — never the offering faction, which would have a
  // citadel paying for its own hulls.
  const quarry = roleOf(role).hunts[0] ?? null

  if (kind === 'bounty') {
    if (!quarry) return null
    const count = 3 + rng.below(6)
    return {
      id, kind, faction, quarry, from: null, to: null, count,
      reward: RATE.bounty * count,
      title: `Destroy ${count} ${quarry} craft`,
    }
  }

  if (kind === 'capital') {
    if (!quarry) return null
    // One or two. A capital contract asking for four would be a contract nobody finishes.
    const count = 1 + rng.below(2)
    return {
      id, kind, faction, quarry, from: null, to: null, count,
      reward: RATE.capital * count,
      title: `Destroy ${count} ${quarry} capital ${count === 1 ? 'hull' : 'hulls'}`,
    }
  }

  // A haul needs two distinct places to exist. `serviceNodes` is a *use* of the map, not a read
  // of the record's claims — see the module note.
  const stops = nodes.filter((n) => n.services.includes('trade') || n.services.includes('refuel'))
  if (stops.length < 2) return null
  const a = stops[rng.below(stops.length)]
  let b = stops[rng.below(stops.length)]
  if (b.id === a.id) b = stops[(stops.indexOf(a) + 1) % stops.length]
  if (b.id === a.id) return null

  const rate = kind === 'contraband' ? RATE.contraband : RATE.haul
  return {
    id, kind, faction, quarry: null, from: a.id, to: b.id, count: 1,
    reward: rate,
    title:
      kind === 'contraband'
        ? `Run unmanifested cargo from ${a.label} to ${b.label}`
        : `Deliver cargo from ${a.label} to ${b.label}`,
  }
}

/**
 * The contract you are given before you have been anywhere.
 *
 * ## Why one is handed over at all
 *
 * A new pilot spawns with a full board of *nothing to do*. Contracts live at citadels, citadels
 * are scattered over a sector twelve extents across, and the panel that would tell you they exist
 * is one you have to already be docked at to see. The first session's actual shape was therefore
 * "fly somewhere, find out there was a reason to fly somewhere" — and the thing that makes an open
 * sector legible is not a tutorial, it is having a reason to point the ship at something.
 *
 * ## It obeys every rule the board obeys, and one more
 *
 * Terms come from `(seed, role, index)` and nothing else — no record field anywhere near the count
 * or the reward, exactly as `build` does, because an opening contract that paid more in a
 * richer-looking world would be the same corruption arriving at the friendlier end of the game.
 *
 * The extra rule is that it is **small**. It is a first job, not a campaign: the count is at the
 * bottom of the band and the subject is deliberately the sort of thing that is already happening
 * near the origin. A big opening contract is one a new player abandons, and abandoning the only
 * thing on your board teaches that the board is optional.
 *
 * Returns `null` for a role no faction will deal with in this world, which cannot happen today —
 * every role has a `welcome` faction — but is a real answer rather than a thrown error if that
 * ever changes.
 */
export function opening(seed: string, role: RoleId, nodes: Node[]): Quest | null {
  const r = roleOf(role)
  // The first faction that will actually deal with this role. `welcome` and not `hunts`: who takes
  // your money at a dock and who shoots at you in open space are different questions, and deriving
  // one from the other is the mistake `servedBy` exists to prevent.
  const faction = r.welcome[0]
  if (!faction || refusal(faction, role) !== null) return null
  const kinds = kindsFor(role, faction)
  if (kinds.length === 0) return null

  // Its own slice of the stream and its own salt, so the opening job is not simply the first entry
  // of the board the player will later find at a citadel — meeting the same contract twice, once
  // as a gift and once as an offer, reads as the board being broken.
  const rng = new Rng(seed.slice(0, 8) || seed)
  const salt = [...`opening:${role}`].reduce((a, c) => (a * 31 + c.charCodeAt(0)) % 997, 23)
  for (let i = 0; i < salt; i += 1) rng.below(1024)

  const q = build(kinds[0], faction, role, 0, rng, nodes)
  if (!q) return null
  return {
    ...q,
    // A distinct id namespace, so completing it never marks a citadel offer as done and the
    // dedupe in `board` cannot silently remove a job because the opening one resembled it.
    id: `opening:${role}`,
    // Smallest useful version of whatever kind it is. `Math.min` rather than a literal, so a
    // change to the bands in `build` carries here instead of leaving a number nobody looks at.
    count: Math.min(q.count, q.kind === 'capital' ? 1 : 3),
    reward: RATE[q.kind] * Math.min(q.count, q.kind === 'capital' ? 1 : 3),
    title: q.kind === 'bounty'
      ? `Destroy 3 ${q.quarry} craft`
      : q.kind === 'capital'
        ? `Destroy 1 ${q.quarry} capital hull`
        : q.title,
  }
}

/** The state a new pilot starts in: one small contract, already accepted. */
export function openingState(seed: string, role: RoleId, nodes: Node[]): QuestState {
  const q = opening(seed, role, nodes)
  return q ? { active: { quest: q, progress: 0, picked: false }, done: [], earned: 0 } : newQuests()
}

/** Why a faction will not deal with you. `null` when it will. */
export function refusal(faction: Faction, role: RoleId): string | null {
  const r = roleOf(role)
  // On `servedBy`, not on hostility — see `roles.ts::servedBy`. Who shoots at you in open space
  // and who takes your money at a dock are different questions, and deriving one from the other
  // left the smuggler with no board anywhere in the sector.
  if (!servedBy(faction, role)) {
    return `${faction} stations will not open to a ${r.label.toLowerCase()}`
  }
  if (kindsFor(role, faction).length === 0) {
    return `${faction} has no work for a ${r.label.toLowerCase()}`
  }
  return null
}

export function accept(state: QuestState, quest: Quest): QuestState {
  if (state.active) return state
  return { ...state, active: { quest, progress: 0, picked: false } }
}

/** Drop the contract. Free, and deliberately so — see the module note. */
export function abandon(state: QuestState): QuestState {
  return { ...state, active: null }
}

/**
 * A craft was destroyed by the player. Advances a hunting contract.
 *
 * Takes what the kill *was* rather than the craft, so this stays free of `enemy.ts` and testable
 * with two booleans.
 */
export function recordKill(
  state: QuestState,
  faction: Faction,
  capital: boolean,
): { state: QuestState; completed: Quest | null } {
  const a = state.active
  if (!a) return { state, completed: null }
  const q = a.quest
  if (q.kind !== 'bounty' && q.kind !== 'capital') return { state, completed: null }
  if (q.quarry !== faction) return { state, completed: null }
  if (q.kind === 'capital' && !capital) return { state, completed: null }

  const progress = a.progress + 1
  if (progress < q.count) {
    return { state: { ...state, active: { ...a, progress } }, completed: null }
  }
  return { state: settle(state, q), completed: q }
}

/**
 * The ship docked at a node. Advances a haul.
 *
 * Both legs are docking events, so a haul cannot be completed by flying past — the same
 * `DOCK_RANGE` the services use.
 */
export function recordDock(
  state: QuestState,
  nodeId: number,
): { state: QuestState; completed: Quest | null } {
  const a = state.active
  if (!a) return { state, completed: null }
  const q = a.quest
  if (q.kind !== 'haul' && q.kind !== 'contraband') return { state, completed: null }

  if (!a.picked) {
    if (nodeId !== q.from) return { state, completed: null }
    return { state: { ...state, active: { ...a, picked: true, progress: 1 } }, completed: null }
  }
  if (nodeId !== q.to) return { state, completed: null }
  return { state: settle(state, q), completed: q }
}

function settle(state: QuestState, q: Quest): QuestState {
  return { active: null, done: [...state.done, q.id], earned: state.earned + q.reward }
}

/**
 * Whether the patrol is currently scanning for your hold.
 *
 * A live contraband run makes the marshals hostile **for a role they would otherwise ignore**,
 * which is the entire mechanic: a pirate is always hunted by the patrol, but a trader carrying
 * one illegal crate is hunted only while carrying it. Read by `roles.ts`'s caller rather than by
 * `hostileToPlayer`, so the role table stays a table.
 */
export function carryingContraband(state: QuestState): boolean {
  const a = state.active
  return Boolean(a && a.quest.kind === 'contraband' && a.picked)
}

/** One line of progress, for the HUD. */
export function progressLabel(a: Active): string {
  const q = a.quest
  if (q.kind === 'haul' || q.kind === 'contraband') {
    return a.picked ? `cargo aboard — deliver to node ${q.to}` : `collect at node ${q.from}`
  }
  return `${a.progress} of ${q.count}`
}
