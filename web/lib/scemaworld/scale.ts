/**
 * One scale for the whole sector.
 *
 * ## Why this file exists
 *
 * The sector was enlarged sixty-fold and everything *else* stayed where it was. Positions moved;
 * station radii, weapon speeds, sensor ranges and aggro ranges did not, because they lived as
 * bare `26_000 * UNIT` literals in five modules that each declared their own `UNIT`. The result
 * was a space that was technically enormous and unplayable in every specific way: stations were
 * sixteen-thousand-unit specks scattered through a three-hundred-million-unit void, a laser's
 * whole range was a fifth of the distance at which an enemy first notices you, and the first
 * contact was placed exactly where the player spawns.
 *
 * None of that reproduced in a test, because every test asserted a *relationship* between
 * constants that had all moved together — or rather, hadn't.
 *
 * So distances are declared here as **fractions of the sector**, and speeds as **how long it
 * takes to cross one**. Read that way a number carries its own review: `EXTENT / 26` is "a bit
 * under half a minute to cross the sector", which is a claim you can argue with. `9_000 * UNIT`
 * is not.
 *
 * Nothing outside this file may write `* UNIT` for a distance in the world. Ship-scale sizes
 * (a projectile, a hull) are the exception and are declared here too.
 */

/** The base integer unit. Positions are integers in these, so no platform disagrees. */
export const UNIT = 1000

/**
 * Half-width of a generated sector.
 *
 * The fractal's trunk is `EXTENT / 2` and its branches carry outward, so a real sector spans
 * appreciably more than this in its longest axis — around 1.5×. Treat `EXTENT` as the scale of
 * the place, not as its bounding box.
 */
export const EXTENT = 240_000 * UNIT

// ── distances, as fractions of the sector ────────────────────────────────────

/** Radius of an ordinary station. About 0.6% of the sector: a landmark, not a speck. */
export const R_STATION = Math.round(EXTENT * 0.0062)
export const R_ORIGIN = Math.round(EXTENT * 0.0105)
export const R_MARKET = Math.round(EXTENT * 0.0088)
export const R_DOCK = Math.round(EXTENT * 0.0076)
export const R_DEPOT = Math.round(EXTENT * 0.0058)
export const R_DERELICT = Math.round(EXTENT * 0.0054)
export const R_PHANTOM = Math.round(EXTENT * 0.005)
export const R_RIFT = Math.round(EXTENT * 0.008)
export const R_MARKER = Math.round(EXTENT * 0.0035)

/** Base radius of a contact, before its magnitude is added. */
export const R_CONTACT = Math.round(EXTENT * 0.0024)
/** How much a full-magnitude signal adds to that. Size, never damage — see `weapons.ts`. */
export const R_CONTACT_SPAN = Math.round(EXTENT * 0.004)

export const R_LASER = Math.round(EXTENT * 0.0008)
export const R_PHOTON = Math.round(EXTENT * 0.0016)

/** The player's own hull, for taking hits. */
export const R_PLAYER = Math.round(EXTENT * 0.0035)

/** How close you must be to a node to use its services — about three station radii. */
export const DOCK_RANGE = Math.round(EXTENT * 0.019)

/** How close a hostile must be before it notices you. ~6% of the sector. */
export const AGGRO_RANGE = Math.round(EXTENT * 0.06)
/** How close it then tries to get. Inside this it holds station rather than ramming. */
export const ENGAGE_RANGE = Math.round(EXTENT * 0.012)

/** Nearest a fractal branch may place two nodes. Below this the sector reads as a clump. */
export const MIN_BRANCH = Math.round(EXTENT * 0.00375)

// ── speeds, as sector crossings ──────────────────────────────────────────────

/**
 * Top speed of a stock engine: a sector width in twenty-six seconds, so the real long axis
 * takes about forty. Fully upgraded is a little over twice that.
 */
export const SPEED_SHIP = Math.round(EXTENT / 26)
export const SPEED_SHIP_PER_LEVEL = Math.round(EXTENT / 57)

/** Lateral thrusters. Deliberately slow: they are for docking, not for travel. */
export const SPEED_THRUST = Math.round(EXTENT / 110)

/**
 * A laser crosses the sector in nine seconds and lives for half of one, so its reach is a bit
 * over a third of `AGGRO_RANGE`. You close to fight; you do not snipe from the edge of sensors.
 */
export const SPEED_LASER = Math.round(EXTENT / 9)
export const LIFE_LASER = 0.62

/** A photon missile is slower and lives far longer, so it reaches — and it tracks. */
export const SPEED_PHOTON = Math.round(EXTENT / 21)
export const LIFE_PHOTON = 3.4

/** Enemy fire. Faster than any ship, so closing does not make you safe. */
export const SPEED_ENEMY_SHOT = Math.round(EXTENT / 15)
export const LIFE_ENEMY_SHOT = 2.2

/**
 * Hostile craft, base and per-tier.
 *
 * Deliberately below `SPEED_SHIP`, even at the top tier: **disengaging must always be possible.**
 * A game where the only answer to a fight you are losing is to die is a game that punishes
 * exploration, and exploring is the whole activity here.
 */
export const SPEED_CRAFT = Math.round(EXTENT / 62)
export const SPEED_CRAFT_PER_TIER = Math.round(EXTENT / 260)

// ── the camera ───────────────────────────────────────────────────────────────

/**
 * Near plane. Far is the sensor range, up to about `1.15 * EXTENT`, which puts the depth ratio
 * near 2000:1 — inside what a 24-bit depth buffer holds without visible fighting between two
 * stations at the far edge.
 */
export const NEAR_PLANE = Math.round(EXTENT * 0.0006)
