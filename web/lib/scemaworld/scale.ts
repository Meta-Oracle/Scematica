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
export const EXTENT = 3_200_000 * UNIT

// ── distances, as fractions of the sector ────────────────────────────────────

/**
 * Node radii.
 *
 * Bigger both ways: the sector itself is two and a half times what it was, *and* these are a
 * larger fraction of it. A station used to be six tenths of a percent of the sector — a landmark
 * in the arithmetic and a dot on the screen. They are now structures you fly *through*, which is
 * what `collide.ts` stopped making solid and what `meshes.ts` gives a silhouette to.
 */
export const R_STATION = Math.round(EXTENT * 0.009)
export const R_ORIGIN = Math.round(EXTENT * 0.016)
export const R_MARKET = Math.round(EXTENT * 0.014)
export const R_DOCK = Math.round(EXTENT * 0.012)
export const R_DEPOT = Math.round(EXTENT * 0.0085)
export const R_DERELICT = Math.round(EXTENT * 0.008)
export const R_PHANTOM = Math.round(EXTENT * 0.0075)
export const R_RIFT = Math.round(EXTENT * 0.013)
export const R_MARKER = Math.round(EXTENT * 0.005)

/** The largest a node can be, so docking range can be checked against it. */
export const R_NODE_MAX = R_ORIGIN

/** Base radius of a contact, before its magnitude is added. */
export const R_CONTACT = Math.round(EXTENT * 0.0024)
/** How much a full-magnitude signal adds to that. Size, never damage — see `weapons.ts`. */
export const R_CONTACT_SPAN = Math.round(EXTENT * 0.004)

export const R_LASER = Math.round(EXTENT * 0.0008)
export const R_PHOTON = Math.round(EXTENT * 0.0016)

/** The player's own hull, for taking hits. */
export const R_PLAYER = Math.round(EXTENT * 0.0035)

/**
 * How close you must be to a node to use its services.
 *
 * **Must comfortably exceed the largest node.** It did not, and that was the whole of the
 * reported "refuelling does not work": the origin market's own radius plus the ship's put the
 * hull at 0.014 of the sector from the centre, and docking range was 0.019 — a shell two
 * thousandths of a sector thick, crossed in a twentieth of a second at cruise. The ship spawned
 * *outside* it, so the first station a new player ever sees reported `nothing in range`.
 *
 * A test now pins the relationship rather than the number.
 */
export const DOCK_RANGE = Math.round(EXTENT * 0.055)

/**
 * Default sensor and engagement ranges.
 *
 * Every armed class overrides both from `classes.ts` — a destroyer sees further than a skiff,
 * which is most of what makes the sector feel layered. These remain as the fallback and as the
 * scale the class table is written against.
 */
export const AGGRO_RANGE = Math.round(EXTENT * 0.075)
export const ENGAGE_RANGE = Math.round(EXTENT * 0.014)

/**
 * How much further everything *sees* than it engages.
 *
 * Detection was the same number as aggression, so a sector was quiet until something was already
 * on you. Sensor contact should arrive long before a fight does — that gap is where the decision
 * to fight or leave actually lives, and without it there is no decision, only an ambush.
 */
export const SENSOR_MULTIPLIER = 3.4

/**
 * Nearest a fractal branch may place two nodes.
 *
 * Raised hard, and it is the number that decides whether a sector reads as *expansive* or as a
 * dense knot floating in a void. Growing `EXTENT` alone does not spread a fractal out: the trunk
 * gets longer and the twigs stay exactly as close together, so the sector gains empty margin and
 * the part you actually fly through is as cluttered as it was. Cutting the recursion earlier is
 * what puts distance *between the things*.
 */
export const MIN_BRANCH = Math.round(EXTENT * 0.014)

/**
 * Smallest gap the generator will tolerate between any two nodes.
 *
 * A second, blunter guarantee on top of `MIN_BRANCH`. Branches from different parents can land
 * near each other however conservative the recursion is, and two stations a few hundred thousand
 * units apart in a sector three thousand million across is the specific thing that reads as
 * clustering.
 */
export const MIN_NODE_GAP = Math.round(EXTENT * 0.025)

// ── speeds, as sector crossings ──────────────────────────────────────────────

/**
 * Top speed of a stock engine: a sector width in eleven seconds.
 *
 * Roughly four times what it was, on a sector nearly twice as large. The volume stopped being
 * too small and started being too *slow* — the distance was right and crossing it was a chore,
 * which is the same complaint arriving from the other side. Long hauls are the jump drive's
 * job now (`hyper.ts`); the main drive is for closing, and for getting out of a fight.
 */
export const SPEED_SHIP = Math.round(EXTENT / 11)
export const SPEED_SHIP_PER_LEVEL = Math.round(EXTENT / 30)

/**
 * Lateral and vertical thrusters.
 *
 * Fast enough to matter in a dogfight — a jink sideways is how you break a firing solution,
 * and a thruster that only helps you dock makes combat a pure turning contest.
 */
export const SPEED_THRUST = Math.round(EXTENT / 30)

/**
 * A laser crosses the sector in nine seconds and lives for half of one, so its reach is a bit
 * over a third of `AGGRO_RANGE`. You close to fight; you do not snipe from the edge of sensors.
 */
export const SPEED_LASER = Math.round(EXTENT / 2.2)
export const LIFE_LASER = 0.3

/** A photon missile is slower and lives far longer, so it reaches — and it tracks. */
export const SPEED_PHOTON = Math.round(EXTENT / 6)
export const LIFE_PHOTON = 2.0

/** Enemy fire. Faster than any ship, so closing does not make you safe. */
export const SPEED_ENEMY_SHOT = Math.round(EXTENT / 3.4)
export const LIFE_ENEMY_SHOT = 0.55

/**
 * Hostile craft, base and per-tier.
 *
 * Deliberately below `SPEED_SHIP`, even at the top tier: **disengaging must always be possible.**
 * A game where the only answer to a fight you are losing is to die is a game that punishes
 * exploration, and exploring is the whole activity here.
 */
export const SPEED_CRAFT = Math.round(EXTENT / 20)
export const SPEED_CRAFT_PER_TIER = Math.round(EXTENT / 120)

// ── the camera ───────────────────────────────────────────────────────────────

/**
 * Near plane. Far is the sensor range, up to about `1.15 * EXTENT`, which puts the depth ratio
 * near 2000:1 — inside what a 24-bit depth buffer holds without visible fighting between two
 * stations at the far edge.
 */
export const NEAR_PLANE = Math.round(EXTENT * 0.0009)

/**
 * The far plane, always. **Draw distance is no longer a function of sensor range.**
 *
 * Sensor range used to gate it, and the result was a wall of fog at the edge of a volume the
 * whole design is about the size of — you could not see the sector you were flying in. Now the
 * far plane covers the entire generated space (the fractal reaches roughly 1.6 extents along
 * its longest axis, so this clears it) and legibility expresses itself where it belongs: in
 * what the record *knows*, not in how far the window sees.
 *
 * The depth ratio against `NEAR_PLANE` is about 2000:1, inside what a 24-bit depth buffer
 * holds without two distant stations fighting over the same pixel.
 */
export const FAR_PLANE = Math.round(EXTENT * 1.9)

// ── projectiles as objects rather than points ────────────────────────────────

/**
 * How long a bolt is drawn, as a multiple of its radius.
 *
 * A shot was a sphere, and a sphere travelling at half the sector per second is a dot that
 * teleports. A cylinder along the direction of travel is what makes a tracer read as *fast*
 * rather than as a flicker: the eye gets a streak to follow, and the streak points back at
 * where the shot came from, which is the most useful thing on screen in a fight.
 */
export const BOLT_LENGTH = 16
/** The additive halo, as a multiple of the core radius. Glow, drawn as geometry. */
export const BOLT_GLOW = 3.6

// ── the jump drive ───────────────────────────────────────────────────────────

/**
 * The jump drive turns a big sector from a commute into a map.
 *
 * Crossing at full burn takes eleven seconds and used to take forty, and neither is the point:
 * the interesting decision is *which* of a thousand nodes to be at, and a travel time long
 * enough to be felt turns that decision into a chore. So a jump is near-instant, costs a
 * separate and scarce fuel, takes time to charge, and **cannot be charged with a hostile
 * inside sensor range** — which is what stops it being an escape button and makes committing
 * to a fight mean something.
 */
export const JUMP_CHARGE_MS = 2_600
/** How close a hostile may be before the drive refuses to spin up. */
export const JUMP_INHIBIT = Math.round(EXTENT * 0.055)
/** Where you arrive relative to the target, so a jump never lands you inside a station. */
export const JUMP_STANDOFF = Math.round(EXTENT * 0.012)
