/**
 * What kinds of thing fly here, and what each one is made of.
 *
 * One flat table. A class is a *silhouette plus a statline*, and both belong in the same place:
 * the moment a renderer decides that the fast one is the small triangle, the table has two
 * homes and they drift. `view.ts` reads `shape` from here and `gl.ts` only places geometry.
 *
 * ## The design the numbers encode
 *
 * A dogfight is a contest of **turn rate against speed**, not of hit points. So every class
 * trades one for the other along a single line — an interceptor turns twice as fast as it
 * flies, a destroyer barely turns at all — and the player sits deliberately near the fast end
 * of speed and the middle of turn. That is what makes "get behind it" a skill rather than a
 * dice roll, and what makes a capital ship a *place* you fight at rather than a duel.
 *
 * ## Shields absorb, hull decides
 *
 * Every armed thing carries both. Shields regenerate after a lull, so a fight has a rhythm:
 * break contact, recover, re-engage. Hull does not, which is why hull is the number the HUD
 * calls health and shields are the number it calls a buffer. A design where the regenerating
 * bar was the primary one would make every fight a war of attrition against a clock.
 *
 * ## Detection range is half of what makes a sector feel alive
 *
 * Every `aggro` here was raised by half again, and `LIFE_LASER` with it so the line the reach
 * invariant draws through this table survives (`scale.ts`). The reason is measured rather than
 * felt: with the old figures, three minutes of play produced **not one round fired within 0.05
 * EXTENT of the player** — everything armed noticed everything else too late and too locally, so
 * the sector's fights happened in scattered private pockets nobody was near. A craft that sees
 * further engages sooner, follows further, and drags its fight across the volume, which is what
 * puts an engagement in front of somebody.
 *
 * ## Bounties are flat per class and come from the act
 *
 * Never from anything the record reports. See `ship.ts` — this is where the rule would be
 * easiest to break, because scaling a bounty by a signal's magnitude is one line and would
 * quietly pay somebody to inflate a record.
 */

import { EXTENT, LIFE_ENEMY_SHOT, LIFE_ENEMY_SHOT_MAX, SPEED_ENEMY_SHOT,
} from './scale.ts'

/**
 * A silhouette. `gl.ts` owns one mesh per value; nothing else may choose one.
 *
 * Nodes have their own now, rather than all being spheres in different colours. A market and a
 * rift used to be the same ball, so the vocabulary the record carries arrived as a *palette* —
 * which fails the same way colour-only distinctions fail everywhere else in this project.
 */
export type Shape =
  /**
   * A faction citadel, one shape per tier.
   *
   * Three entries rather than a parameterised one because the mesh registry in `gl.ts` uploads a
   * fixed set of buffers once — a shape whose geometry depended on an instance's data would have
   * to be re-uploaded per draw, which is the one thing the instanced path exists to avoid.
   */
  | 'citadel1'
  | 'citadel2'
  | 'citadel3'
  | 'sphere'
  | 'shell'
  | 'bolt'
  | 'interceptor'
  | 'gunship'
  | 'capital'
  | 'dreadnought'
  | 'corvette'
  | 'marauder'
  /**
   * The player's own medium and capital silhouettes.
   *
   * Deliberately not shared with the hostile war hulls. A hull drawn at five times its own
   * size reads as a rendering fault rather than as a bigger ship, and a ship indistinguishable
   * from the thing shooting at you is a poor thing to identify with — an argument that gets
   * stronger as the hulls get bigger, since a capital is on screen for the whole session.
   */
  | 'cruiser'
  | 'bulwark'
  | 'sovereign'
  | 'station'
  | 'market'
  | 'dock'
  | 'depot'
  | 'derelict'
  | 'rift'
  | 'phantom'
  | 'marker'
  | 'origin'

export type ClassId =
  | 'skiff'
  | 'interceptor'
  | 'lancer'
  | 'gunship'
  | 'frigate'
  | 'destroyer'
  | 'warfighter'
  | 'dreadnought'
  | 'leviathan'
  | 'titan'
  | 'courier'
  | 'freighter'
  | 'marshal'
  | 'warden'
  | 'bastion'

export interface ClassSpec {
  id: ClassId
  label: string
  shape: Shape
  /** Drawn size. */
  radius: number
  /** Hull points. The primary health — it does not come back. */
  hull: number
  /** Shield points. Absorbs first, regenerates after `SHIELD_DELAY_MS` without a hit. */
  shield: number
  shieldRegen: number
  /** Units per second at full burn. */
  speed: number
  /** Radians per second. The number that decides a dogfight. */
  turn: number
  /** How close before it notices you. */
  aggro: number
  /** The range it tries to hold. */
  standoff: number
  damage: number
  cooldownMs: number
  /** Shots per trigger pull. A burst is what makes a hit feel like an event. */
  burst: number
  /** Salvage for a kill. Flat per class — never from the record. */
  bounty: number
  /** True for something too big to dogfight: it lumbers and fires broadsides. */
  capital: boolean
}

const S = (f: number) => Math.round(EXTENT * f)

/**
 * Firing arc, as the minimum `dot(facing, aim)` a craft needs before it will shoot.
 *
 * ## Why capitals were never firing at all
 *
 * There was one threshold, 0.985 — about ten degrees off the nose — and it is the right number
 * for a fighter, whose guns are bolted to the hull. Applied to a capital it is a bug with no
 * symptom: a dreadnought turns at 0.05 radians per second and a titan at 0.014, so bringing a
 * nose onto anything that is moving takes upward of a minute and in practice never completes.
 * Capitals therefore sat in each other's engagement envelopes, both in `attack`, and neither ever
 * fired a shot. The sector's two war fleets ignored each other completely and the reason was a
 * constant that nobody had thought to make per-class.
 *
 * A capital does not aim by turning. It has **turrets**, and the honest model of that is a wide
 * arc: it fires at anything forward of its beam. Behind the beam is a real blind spot and it is
 * the counterplay — the old rule was "stay outside a ten-degree cone", which a fighter satisfies
 * by accident and which therefore made capitals harmless to a moving target rather than
 * dangerous to a careless one.
 */
export const ARC_NOSE = 0.985

/**
 * A capital's arc: everything forward of the beam, with a blind cone astern.
 *
 * Not omnidirectional. A turret ring that covers its own engines would leave nowhere to be safe,
 * and "get behind it" is the manoeuvre that has to keep working — it is what makes a leviathan a
 * problem to solve rather than a damage check.
 */
export const ARC_TURRET = 0.35

/**
 * How close something has to be, in multiples of a capital's own radius, before its turrets can
 * no longer be brought onto it.
 *
 * A turret ring cannot depress onto its own hull. That is physically true and it is also the one
 * piece of counterplay a capital needs to have: the manoeuvre that beats a leviathan is to get
 * *inside* it, among the superstructure, where nothing can be aimed at you — which is already the
 * stated design in `game.ts`'s ramming note ("get inside its guns' arc, where a hull that turns at
 * a twentieth of a radian per second cannot bring anything to bear"). It was stated there and
 * implemented nowhere, because with a ten-degree arc the guns never fired at anything anyway.
 *
 * It costs nothing in a capital duel — two of them fight at a substantial fraction of a sector,
 * nowhere near each other's hulls.
 */
export const TURRET_MIN_RANGE = 1.3

/** The arc a class fires within. */
export function arcOf(spec: ClassSpec): number {
  return spec.capital ? ARC_TURRET : ARC_NOSE
}

/**
 * How long a round from this class stays alive.
 *
 * **A class's rounds must be able to cross the range it is willing to fire at.** They could not,
 * and it was invisible: every enemy round lived 0.8 seconds and travelled at a sector every 3.4
 * seconds, so nothing could reach beyond about a quarter of a sector — while a capital's guns
 * were gated at three to four times that. Capitals in range of each other, on target, firing,
 * and every round evaporating in empty space. Nothing errored and nothing looked wrong; the two
 * fleets simply never damaged each other.
 *
 * Derived from `aggro` rather than tuned per class, so the gate and the projectile cannot drift
 * apart again — widening a class's engagement range now lengthens its rounds by construction.
 * The floor keeps a fighter's rounds exactly as short as they were.
 */
export function shotLifeOf(spec: ClassSpec): number {
  const needed = (spec.aggro / SPEED_ENEMY_SHOT) * 1.25
  return Math.min(LIFE_ENEMY_SHOT_MAX, Math.max(LIFE_ENEMY_SHOT, needed))
}

/** Whether a class can engage something at this range. Only capitals have a minimum. */
export function canEngageAt(spec: ClassSpec, range: number): boolean {
  return !spec.capital || range > spec.radius * TURRET_MIN_RANGE
}


/**
 * ## Everything here is three times as durable as it was, and the photon ladder did not move
 *
 * "Far more formidable" is a request about *how long a fight lasts*, and the only honest way to
 * grant it is to say which number carries it and what had to move with it. Every `hull`, `shield`
 * and `shieldRegen` in this table is exactly three times its previous value, and `PHOTON.damage`
 * in `weapons.ts` is three times its previous value — so **`PHOTONS_TO_KILL` is unchanged, to the
 * round number, for every one of the fifteen classes.** That is not a coincidence, it is the point:
 * the ladder is a thing a pilot holds in their head ("eight for a titan, six for a dreadnought,
 * one for a fighter") and scaling durability without scaling the warhead would have quietly turned
 * a titan into a twenty-four-photon problem, which is not a plan, it is a wall.
 *
 * What genuinely changes is **laser** time-to-kill, which is what the tier of buff was aimed at.
 * See `weapons.ts::LASER`.
 *
 * ## Per-shot `damage` did not move at all, and that was measured rather than chosen
 *
 * The first attempt raised it by a quarter alongside the durability, on the reasoning that
 * "formidable" ought to mean dangerous as well as durable. It did not survive contact: a stock
 * ship parked in a raider wing died before finishing its first kill, in a scenario that has passed
 * since the sector had wings in it at all.
 *
 * The arithmetic is obvious once it is written down. **Durability is already a lethality
 * multiplier**, because it multiplies how long you spend inside the envelope: a fight that takes
 * three times as long delivers three times the incoming fire whatever the per-shot figure is. The
 * player's own hull is not tripled — a stock ship carries 140 points — so tripling durability had
 * already raised the effective difficulty by about three, and a damage multiplier on top of that
 * compounds rather than adds.
 *
 * So the sector's craft take three times the killing and hit exactly as hard as they always did.
 * Anyone who wants them deadlier as well should move the *player's* durability in the same edit
 * and state the ratio, rather than discovering it in a fight.
 */
export const CLASSES: Record<ClassId, ClassSpec> = {
  // Barely armed, and the only thing in the sector you can reliably run down. It exists so a
  // new player has something to win against — a game whose first encounter is unwinnable
  // teaches the wrong lesson about every later one.
  skiff: {
    id: 'skiff', label: 'SKIFF', shape: 'interceptor',
    radius: S(0.0022), hull: 42, shield: 0, shieldRegen: 0,
    speed: S(1 / 20), turn: 1.5, aggro: S(0.075), standoff: S(0.012),
    damage: 4, cooldownMs: 1400, burst: 1, bounty: 15, capital: false,
  },
  // The knife fighter. Turns faster than you, so it will get behind — the answer is to break
  // away and come back on your terms, not to out-turn it.
  interceptor: {
    id: 'interceptor', label: 'INTERCEPTOR', shape: 'interceptor',
    radius: S(0.0028), hull: 66, shield: 36, shieldRegen: 12,
    speed: S(1 / 15), turn: 2.4, aggro: S(0.112), standoff: S(0.009),
    damage: 6, cooldownMs: 620, burst: 2, bounty: 30, capital: false,
  },
  // Fast and fragile, fights at a distance. Punishes sitting still.
  lancer: {
    id: 'lancer', label: 'LANCER', shape: 'interceptor',
    radius: S(0.0032), hull: 60, shield: 66, shieldRegen: 18,
    speed: S(1 / 14), turn: 1.8, aggro: S(0.15), standoff: S(0.028),
    damage: 11, cooldownMs: 1500, burst: 1, bounty: 45, capital: false,
  },
  // Slow, heavily shielded, hits hard in bursts. You beat it by out-turning it, which is the
  // first time the turn/speed trade is the whole answer.
  gunship: {
    id: 'gunship', label: 'GUNSHIP', shape: 'gunship',
    radius: S(0.0055), hull: 480, shield: 300, shieldRegen: 24,
    speed: S(1 / 26), turn: 0.85, aggro: S(0.135), standoff: S(0.016),
    damage: 9, cooldownMs: 900, burst: 3, bounty: 90, capital: false,
  },
  // The smallest capital. It does not chase and it does not need to.
  frigate: {
    id: 'frigate', label: 'FRIGATE', shape: 'capital',
    radius: S(0.028), hull: 840, shield: 540, shieldRegen: 36,
    speed: S(1 / 90), turn: 0.22, aggro: S(0.225), standoff: S(0.055),
    damage: 16, cooldownMs: 1100, burst: 4, bounty: 350, capital: true,
  },
  // Star-destroyer class: four times a station across, and visible from most of the sector.
  destroyer: {
    id: 'destroyer', label: 'DESTROYER', shape: 'capital',
    radius: S(0.075), hull: 1380, shield: 900, shieldRegen: 60,
    speed: S(1 / 150), turn: 0.09, aggro: S(0.30), standoff: S(0.1),
    damage: 24, cooldownMs: 800, burst: 6, bounty: 1200, capital: true,
  },
  /**
   * The warfighter: the medium tier, and the rung the ladder was missing.
   *
   * Between a destroyer and a dreadnought there was nothing — a fivefold jump in effective hit
   * points and a threefold one in radius, so a sector went from "a thing four fighters can
   * handle" straight to "a capital". Four warheads is the middle of the ladder and this is the
   * hull that sits there.
   *
   * **Not a capital**, which is the load-bearing part: it can be *rolled* into a raider wing
   * (`classFor`), where the capitals cannot. So a wing can now arrive with something genuinely
   * dangerous at its centre without the sector having to hand out one of its six placed capitals,
   * and "a capital is placed, a fighter is rolled" survives untouched.
   */
  warfighter: {
    id: 'warfighter', label: 'WARFIGHTER', shape: 'capital',
    radius: S(0.045), hull: 1740, shield: 1140, shieldRegen: 54,
    // Aggro sits **below** a laser's reach (0.20 extents) and below `MIN_ARRIVAL`, which are the
    // two lines every non-capital has to stay under: a fighter you cannot engage from outside its
    // awareness breaks "a laser outranges every fighter and no capital", and one whose awareness
    // exceeds the arrival distance means a reinforcement can resolve already hunting you. At 0.24
    // it broke both, and neither is visible from the statline alone.
    speed: S(1 / 90), turn: 0.16, aggro: S(0.17), standoff: S(0.07),
    damage: 20, cooldownMs: 820, burst: 5, bounty: 700, capital: false,
  },

  // War-class. Fifteen stations end to end.
  //
  // The broadside used to one-shot a fully upgraded ship — ten rounds of sixty-two is six
  // hundred damage against a maximum of five hundred and fifty, so no amount of hull made it
  // survivable and there was no tactic to find. A capital has to be beaten by *manoeuvre*: it
  // turns at a twentieth of a radian per second, so a fighter that keeps moving laterally is
  // never in its arc, and one that flies straight at it deserves what it gets. Per-shot damage
  // is now low enough that being caught once is a mistake rather than the end.
  dreadnought: {
    id: 'dreadnought', label: 'DREADNOUGHT', shape: 'dreadnought',
    radius: S(0.135), hull: 2580, shield: 1740, shieldRegen: 66,
    speed: S(1 / 260), turn: 0.05, aggro: S(0.39), standoff: S(0.16),
    damage: 15, cooldownMs: 900, burst: 8, bounty: 4000, capital: true,
  },
  // The largest thing in any sector, and rare enough that most worlds have none. At this size
  // the ribbing on the hull is doing real work: it is the only cue for how far away it is, and
  // without it a leviathan at range reads as a nearby triangle.
  //
  // Beating one takes minutes of sustained fire and constant lateral movement. That is the
  // intent: perseverance and a manoeuvre, not a bigger number.
  leviathan: {
    id: 'leviathan', label: 'LEVIATHAN', shape: 'dreadnought',
    radius: S(0.24), hull: 3030, shield: 2010, shieldRegen: 78,
    speed: S(1 / 420), turn: 0.028, aggro: S(0.51), standoff: S(0.26),
    damage: 21, cooldownMs: 800, burst: 10, bounty: 14000, capital: true,
  },

  // The titan. One per sector at most, and most sectors have none.
  //
  // A third of the sector across: from the far side it is a shape on the sky rather than a
  // contact, and closing on one takes long enough that the decision to do it is made well before
  // you arrive. Beating it is a marauder, every component at maximum, and several minutes of
  // holding a firing solution while never once flying straight.
  //
  // Its per-shot damage is deliberately *lower* than a leviathan's. The threat is the volume of
  // fire and the time you must survive inside it, not a number that deletes you — a one-shot is
  // not difficulty, it is a coin toss with extra steps.
  titan: {
    id: 'titan', label: 'TITAN', shape: 'dreadnought',
    radius: S(0.4), hull: 3450, shield: 2310, shieldRegen: 90,
    speed: S(1 / 700), turn: 0.014, aggro: S(0.60), standoff: S(0.4),
    damage: 18, cooldownMs: 500, burst: 14, bounty: 45000, capital: true,
  },

  // ── traffic ────────────────────────────────────────────────────────────────
  // Not hostile, and `classFor` never rolls them: they are placed by `factions.ts` on routes
  // between real service nodes. They carry statlines because they can be shot at — a ship you
  // cannot destroy is scenery, and scenery that dodges is worse than none.

  // Neon blue. Small, quick, and everywhere; the commonest thing in the sector and the reason
  // the markets look like they are for something.
  courier: {
    id: 'courier', label: 'COURIER', shape: 'interceptor',
    radius: S(0.0024), hull: 42, shield: 24, shieldRegen: 9,
    speed: S(1 / 13), turn: 2.0, aggro: S(0.09), standoff: S(0.01),
    damage: 0, cooldownMs: 9_999, burst: 0, bounty: 0, capital: false,
  },
  // Blue. Slow and heavy, running fuel between depots. Visible from a long way off, which is
  // most of its job: a sector where the only distant silhouettes are threats is a tense one for
  // the wrong reason.
  freighter: {
    id: 'freighter', label: 'FREIGHTER', shape: 'gunship',
    radius: S(0.009), hull: 450, shield: 300, shieldRegen: 18,
    speed: S(1 / 42), turn: 0.6, aggro: S(0.09), standoff: S(0.014),
    damage: 0, cooldownMs: 9_999, burst: 0, bounty: 0, capital: false,
  },
  // Yellow. Anti-raider patrol: hunts raiders, ignores the player, and fights whether or not
  // anyone is watching. Deliberately a match for an interceptor rather than an overmatch — a
  // patrol that always wins makes the sector safe, which is not the point of having one.
  marshal: {
    id: 'marshal', label: 'MARSHAL', shape: 'interceptor',
    radius: S(0.0032), hull: 78, shield: 60, shieldRegen: 18,
    speed: S(1 / 14), turn: 2.2, aggro: S(0.165), standoff: S(0.012),
    damage: 8, cooldownMs: 700, burst: 2, bounty: 0, capital: false,
  },

  // ── the patrol's own war classes ───────────────────────────────────────────
  //
  // The sector's capitals used to be exclusively hostile, which made a marshal patrol a
  // gesture: eighteen interceptors against a roster that includes a leviathan is not a police
  // force, it is a rounding error, and every large silhouette on the horizon meant exactly one
  // thing. Giving the yellow faction a dreadnought and a titan of its own changes what a distant
  // capital *is* — a question rather than an answer — and it is what makes a firefight between
  // two other parties worth flying toward and watching.
  //
  // They mirror the hostile war classes rather than exceeding them. A patrol that outguns
  // everything it meets makes the sector safe, which is not the point of having one; the
  // interesting outcome is a leviathan and a warden grinding each other down over minutes while
  // the player decides whether to intervene, and on which side.
  //
  // **Bounty is zero, and that is a rule rather than a balance choice.** These are the good guys.
  // A payout for killing one would put a price on the faction that exists to make the sector feel
  // policed, and the reward rule (`ship.ts`) is about where salvage may come from at all.

  // The marshal dreadnought. What answers a raider capital.
  warden: {
    id: 'warden', label: 'WARDEN', shape: 'dreadnought',
    radius: S(0.135), hull: 2580, shield: 1740, shieldRegen: 66,
    speed: S(1 / 260), turn: 0.05, aggro: S(0.39), standoff: S(0.16),
    damage: 15, cooldownMs: 900, burst: 8, bounty: 4000, capital: true,
  },
  // The marshal titan. One per sector, and the only thing out here that a hostile titan has to
  // take seriously. Finding the two of them already engaged is the sight this whole faction
  // exists to produce.
  bastion: {
    id: 'bastion', label: 'BASTION', shape: 'dreadnought',
    radius: S(0.4), hull: 3450, shield: 2310, shieldRegen: 90,
    speed: S(1 / 700), turn: 0.014, aggro: S(0.60), standoff: S(0.4),
    damage: 18, cooldownMs: 500, burst: 14, bounty: 45000, capital: true,
  },
}

/**
 * The classes `classFor` may roll — the hostile roster only.
 *
 * Traffic is placed by `factions.ts` and never rolled, so it is excluded here. Keeping one list
 * of "everything" and filtering at each use site is how a courier eventually turns up in a raider
 * wing, which would be both a gameplay bug and a lie about who is out there.
 */
export const CLASS_IDS: ClassId[] = [
  'skiff', 'interceptor', 'lancer', 'gunship', 'frigate', 'destroyer', 'warfighter',
  'dreadnought', 'leviathan', 'titan',
]

/** Every class, hostile and otherwise. */
export const ALL_CLASS_IDS: ClassId[] = [
  ...CLASS_IDS, 'courier', 'freighter', 'marshal', 'warden', 'bastion',
]

/**
 * The patrol's war classes, placed as a garrison rather than rolled.
 *
 * Same treatment as the hostile capitals: a fixed roster the sector carries, so a record can
 * neither buy itself a quieter sector nor a better-defended one.
 */
export const MARSHAL_CAPITAL_IDS: ClassId[] = ['warden', 'bastion']

/**
 * How many photon warheads it takes to destroy each class.
 *
 * ## Why durability is written as a photon count
 *
 * It was written as raw hull and shield numbers, and the numbers had drifted into a game nobody
 * could finish: a titan carried 23,000 effective points against a 240-point warhead, so killing
 * one took **96 photons** — twelve full magazines off the largest hull in the game, with a trip
 * back to a station between each. A leviathan took 29 and a dreadnought 10. The comments described
 * this as "perseverance and a manoeuvre"; in play it is a wall, and the difference between the two
 * is exactly the number in this table.
 *
 * So the ladder is the definition and the statline is derived from it. A player can hold "eight
 * for a titan, six for a dreadnought, four for a warfighter, two for a heavy fighter, one for a
 * fighter" in their head, which makes a magazine a *plan* — and a plan is the thing a capital
 * fight was missing. `check:scemaworld` asserts `ceil(ehp / PHOTON.damage)` equals this for every
 * class, so a statline edited without the table fails rather than quietly moving the ladder.
 *
 * The starting magazine is eight, which is the titan's number on purpose: the largest thing in the
 * sector is beatable by a pilot who has spent nothing, provided every round lands. Nothing else
 * about a titan changed — it still turns like a continent and fires like one.
 */
export const PHOTONS_TO_KILL: Record<ClassId, number> = {
  // Fighters. One warhead, and mostly they should die to the laser before it is worth spending.
  skiff: 1,
  interceptor: 1,
  lancer: 1,
  courier: 1,
  marshal: 1,
  // Heavy fighters. Two — the one rung that genuinely had to move, since a gunship used to die to
  // a single warhead like everything below it.
  gunship: 2,
  frigate: 2,
  // Traffic. A freighter is a big soft target; a courier is paper.
  freighter: 2,
  // Warfighters. Four. `destroyer` was already sitting at this weight; `warfighter` is the new
  // class the tier is named for.
  destroyer: 4,
  warfighter: 4,
  // Capitals.
  dreadnought: 6,
  warden: 6,
  leviathan: 7,
  titan: 8,
  bastion: 8,
}

/** Milliseconds without taking a hit before shields begin to come back. */
export const SHIELD_DELAY_MS = 4_200

/**
 * Pick a class from a seed-derived roll.
 *
 * The distribution is fixed and lives here rather than in `raiders.ts`, so the sector's
 * composition is one table rather than a chain of thresholds somebody has to reconstruct.
 * Capitals are rare because a sector with several is a sector you cannot cross.
 */
export function classFor(roll: number): ClassSpec {
  // Per *mille*, not per cent. The roll is an integer, so a hundred buckets cannot express
  // anything rarer than one in a hundred — the titan sat behind `r < 99.6` against a value that
  // is only ever a whole number, and could never be rolled at all. Same shape as the bug that
  // hid both capitals behind a six-valued hash, and it survived one round of tests because the
  // threshold *looks* like it works.
  const r = roll % 1000
  if (r < 240) return CLASSES.skiff
  if (r < 540) return CLASSES.interceptor
  if (r < 730) return CLASSES.lancer
  if (r < 865) return CLASSES.gunship
  if (r < 930) return CLASSES.frigate
  if (r < 962) return CLASSES.destroyer
  if (r < 988) return CLASSES.warfighter
  if (r < 994) return CLASSES.dreadnought
  if (r < 999) return CLASSES.leviathan
  return CLASSES.titan
}
