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

/**
 * Bolt radii. **Small and bright, not big and dim.**
 *
 * These were three and a half times larger, and the cost only became visible once the sector
 * had firefights in it that the player is not part of. A fat bolt at this scale is a *blob*: at
 * any distance where you can see two craft trading fire, the rounds are wider than the ships
 * and the engagement reads as two smudges overlapping. Shrinking the core and pushing the
 * brightness up (`BOLT_GLOW` below, and the core multiplier in `gl.ts`) gives the opposite —
 * a hairline tracer that is legible from across the sector because it is *bright*, not because
 * it is large. Additive blending is what makes that trade available at all: brightness sums
 * where a shot overlaps its own halo, so a thin line still clips to white.
 *
 * A photon stays visibly fatter than a laser. It is one round out of at most six and it needs
 * to look like the event it is.
 */
export const R_LASER = Math.round(EXTENT * 0.00022)
export const R_PHOTON = Math.round(EXTENT * 0.0009)

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
export const AGGRO_RANGE = Math.round(EXTENT * 0.112)
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
 * A laser crosses the sector in a little over two seconds and lives for just under half of one, so
 * its reach is about **0.20 of the sector — roughly 1.8 times `AGGRO_RANGE`**.
 *
 * Both figures were raised by half again when detection range was (see `classes.ts`), *together*,
 * so the ratio and the design line below are unchanged. That is the whole discipline this comment
 * exists to enforce: the numbers may move, the relationship may not move silently.
 *
 * ## What that buys, stated as the property it actually is
 *
 * The reach outranges the awareness of **every fighter class and no capital**: a skiff, an
 * interceptor, a lancer and a gunship can all be engaged from beyond the range at which they
 * would notice you, while a frigate and everything above it sees you first. So a patient pilot
 * can pick off escorts at range, and nothing larger than a gunship can be approached unseen.
 * `check:scemaworld` pins both halves of that, because it is a *line* through the class table
 * and one new statline could quietly cross it.
 *
 * ## Why this comment is worded like a claim under test
 *
 * It previously read "a laser crosses the sector in nine seconds and lives for half of one, so
 * its reach is a bit over a third of `AGGRO_RANGE`. You close to fight; you do not snipe from
 * the edge of sensors." Every clause of that was false: the crossing was 2.2 seconds, the life
 * 0.3, and the reach 1.8 times the aggro range rather than a third of it — so the design
 * statement in the last sentence was the exact opposite of what the constants did.
 *
 * Nothing had gone wrong with the game. `SPEED_LASER` and `AGGRO_RANGE` were each changed for
 * good reasons, months apart, and the sentence describing their relationship was not — because
 * no test was reading it. A figure in a comment is a claim, and a claim with no test is a claim
 * that will be wrong eventually; the only question is when somebody notices. Found by an
 * external audit rather than by this repository, which is the point.
 */
export const SPEED_LASER = Math.round(EXTENT / 2.2)
export const LIFE_LASER = 0.45

/** A photon missile is slower and lives far longer, so it reaches — and it tracks. */
export const SPEED_PHOTON = Math.round(EXTENT / 6)
export const LIFE_PHOTON = 2.0

/** Enemy fire. Faster than any ship, so closing does not make you safe. */
export const SPEED_ENEMY_SHOT = Math.round(EXTENT / 3.4)
/**
 * Raised with `LIFE_LASER` and for the same reason: everything now detects and engages half again
 * as far, and a round that expired before it covered the new engagement band would mean craft
 * shooting at each other and never connecting. A tracer that dies short is also the least legible
 * possible firefight — the streaks stop halfway to their target.
 */
export const LIFE_ENEMY_SHOT = 0.8

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
export const BOLT_LENGTH = 34

/**
 * The smallest a bolt's cross-section may appear, in pixels of screen height.
 *
 * A floor, enforced in the vertex shader (`gl.ts`), and the fix for the thing that made the
 * sector's firefights invisible. Measured before it existed: over three minutes, **not one round
 * was fired within 0.05 EXTENT of the player**, 77% of all fire happened beyond 0.4 EXTENT, and at
 * that range a bolt core projects to 0.82 pixels. The rounds were on screen and could not be seen,
 * and "smaller and brighter" had improved only the half of that which was not the problem.
 *
 * Three pixels rather than one: a single pixel of an additively-blended tracer moving at half a
 * sector per second flickers between samples and reads as noise, which is worse than nothing
 * because the player learns to ignore it.
 *
 * Only the cross-section is floored. The *length* stays in world units, so a distant round is a
 * thin streak pointing back at whoever fired it rather than a ball that grows as it recedes.
 */
export const BOLT_MIN_PX = 3
/**
 * The additive halo, as a multiple of the core radius. Glow, drawn as geometry.
 *
 * Raised alongside the shrunken `R_LASER`, and the two are one decision. The halo is what
 * carries a bolt at distance — the core is a hairline and would vanish on its own — so a
 * smaller core needs a proportionally wider halo to stay visible, while the *lit area* still
 * ends up far smaller than the old fat bolt's. That is the whole trick: legible because it is
 * bright, not because it is big.
 */
export const BOLT_GLOW = 5.2

// ── the course line ─────────────────────────────────────────────────────────

/**
 * How many glowing dashes mark the course to a waypoint.
 *
 * Dashes rather than one long line, and it is not decoration. A solid line to a destination four
 * hundred million units away is a bright bar across the middle of the window that hides whatever
 * is behind it — and what is behind it is the direction you are flying. A dashed line gives the
 * same guidance and occupies a fraction of the screen; the gaps are where you see the sector.
 *
 * They also give **distance**. Fixed spacing means the dashes crowd together as they recede, so
 * the line reads as a road going away rather than as an overlay drawn on glass.
 */
export const COURSE_DASHES = 34

/** How far along the course the dashes stop, so the last one does not sit inside the station. */
export const COURSE_CLEAR = 0.94

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
