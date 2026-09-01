# Scema-World

**A space exploration and combat game where the map is a sealed decision record.**

Not a metaphor. `web/lib/scemaworld/generate.ts` takes a record and returns the volume: the
stations, the lanes between them, the contacts, and how far you can see. Same record, same
space, on every machine, forever — because the world tree is already a deterministic function
of the record's commitment, proven byte-for-byte between Rust and the browser.

That property is what makes this a game rather than a demo. Two players holding the same
record fly the same space **without a server and without trusting each other**. The record is
the map, and it is also the proof that the map is what it claims to be.

## Why this is worth building, and the trap it avoids

The tempting version of "NFT game" is a token that unlocks cosmetics, with a random seed
underneath and the artefact as decoration. Scema-World cannot be that, because the seed *is*
the artefact — change one byte of the record and the digest changes, the space changes, and
`scema verify` says which field moved.

The second temptation is an economy. There is none here, and that is a design commitment with
a reason rather than a scoping decision: **anything that priced a world would make a record's
content worth misreporting.** A producer with an incentive to lie is the one failure this
project cannot absorb — the whole runtime is arranged so an observer reports what it could not
read. Attach a yield to `blind_spots` and you have paid somebody to hide them. So: no currency,
no market, no yield. A `check:scemaworld` test asserts the model carries none of those fields
and that the commitment is written down rather than merely true.

## The epistemics are the mechanics

Every quantity in the space comes from something an observer actually reported. This is the
whole design and it is why the game is interesting rather than arbitrary:

| In the record | In the space |
|---|---|
| `blind_spots` (a count) | a **rift** — a lane that ends, one per blind spot, never a rate |
| signal `measured: false` | a **ghost contact** — reads on sensors, may not be there |
| signal `polarity: risk` | hostile |
| signal `polarity: opportunity` | salvage |
| `Provenance::Live` | an active station |
| `Provenance::Stale` | a derelict — still there, no longer answering |
| `Provenance::Absent` | a marker where something should be and is not |
| `Provenance::Simulated` | a phantom |
| legibility | **sensor range** — an illegible world is literally dark |
| `extent.total === null` | the volume has no known boundary, and says so |
| growth `depth` / `arity` / `spread` | how deep, how branching, how wide the map is |

The ghost contact is the em-dash rule as a game mechanic, and it is the one to keep. An
estimated signal rendered as a solid enemy would be exactly the lie the runtime refuses — in
the one place a player would act on it. A ghost must look like something you are being told
not to trust.

Sensor range has the same shape. A world with no perceived objects has range `null` — *unknown*,
not zero. "You cannot see" and "nobody knows how far you can see" are different facts, and a
renderer must show them differently.

**A well-observed world is a safer place to fly.** That falls out of the mapping rather than
being designed in, and it is the sentence that makes the whole thing cohere.

## Arcs

Numbered, in dependency order. Each is shippable on its own. **All six are built.**

### Arc 1 — The volume *(done)*

`lib/scemaworld/generate.ts`. Record → nodes, lanes, contacts, sensor range. Pure, integer
coordinates, no clock, no randomness. 17 checks in `npm run check:scemaworld`, including that
the generator reads no clock, that a rift is a dead end with nothing beyond it, and that the
model has no economy in it.

Integer coordinates for the same reason the PNG rasteriser has no floats: two machines that
disagree about where a station is are not playing the same game.

### Arc 2 — The renderer *(done)*

Hand-rolled WebGL, no engine dependency. In keeping with the rasteriser, the HTTP server and
the PNG encoder, and for the same reason — a rendering that depends on which library drew it
is not a derivative of the record.

Lanes as lines, stations as instanced geometry, ghosts drawn differently from solids and
visibly so. Pointer-lock flight, six degrees of freedom. Sensor range as a real draw distance,
so an illegible world is *experienced* as dark rather than labelled dark.

### Arc 3 — Loading a record *(done)*

Drop a `.json` on the page, exactly as `/omni` does — `FileReader`, no upload, verified in the
browser with `verifyRecordText`. A tampered record still generates a space and the HUD says
`INVALID` unmissably. It is not blocked: seeing what a forged map looks like is more
instructive than being refused one.

**A PNG is a claim ticket, not a map**, and the first version of this document got that wrong.
It claimed the digest could be "recovered from an owned image" — it cannot. The plate draws
only a *shortened* digest as glyphs, and pixels are not invertible. Worse, the image could not
say which record it derived from at all.

Fixed properly: the encoder writes a `tEXt` chunk keyed `scema.world` carrying the full
commitment, in both `raster.rs` and `raster.ts`, so byte-parity is preserved and the fixture
still matches. `readWorldCommitment` walks the chunk table rather than scanning the bytes — a
scan would eventually find something hex-shaped in the pixel data and hand back a commitment
nobody wrote.

The ticket names a world; it does not contain one. The objects, signals and blind spots that
make the space do not survive rasterisation, so a PNG tells you *which* record to fetch. Which
is exactly what Arc 5 is for.

### Arc 4 — Combat *(done)*

Two weapons. **Automatic lasers** — unlimited, held fire, fast and straight. **Photon
missiles** — twelve, one per press, and they steer toward a lock. Right click fires, left
click switches.

Magnitude drives size and aggression. **Durability comes from the seed**, never from the
reported magnitude, and that is the load-bearing decision: a hit-point pool derived from a
signal's magnitude would hand anybody who can write a record a reason to understate it. Same
record, same fight; a smaller reported number does not buy an easier enemy.

**A ghost never resolves.** An estimated signal is one the observer counted but whose
magnitude it guessed — the thing is there, its size is unknown, and nothing in the record can
settle that, so neither does the game. Its threat reads `—` while you are fighting it and
after you have destroyed it. Resolving a ghost on first hit into a known number would play
better and would be the em-dash bug with a game-design justification: the number would be
invented, and the player would act on it as though somebody had measured it.

A photon will lock a ghost as readily as a solid. Refusing to would leak the answer — the
player would learn from the targeting computer what the record does not know.

## Controls

| | |
|---|---|
| `W` `A` `S` `D` | pitch and yaw — aim the ship |
| `Q` `E` | roll |
| `↑` `↓` | **throttle level** up / down |
| `X` | full stop |
| `←` `→` `SPACE` `SHIFT` | lateral and vertical thrusters |
| right click | fire |
| left click | switch weapon |
| `F` `R` `V` | refuel · repair · scavenge, at a node in range |
| `M` | market — spend salvage on the ship |
| `1` `2` `3` `4` | route to fuel · repair · market · salvage |
| `0` | clear the waypoint |

Roll is on `Q`/`E` because it is the one rotational axis `WASD` leaves unreachable, and a
space game without roll has an up — which this one does not.

**Throttle is a level, not a button.** The first version accelerated only while a key was
held, so the ship was a car with the pedal being tapped. A vessel has a cruise setting, and at
this sector's size that is the difference between flying and steering: you set 40% and go and
do something else. `X` cuts the drive, because winding down from cruise at 0.9/s is a long
wait when a station is coming up fast.

## Arc 7 — A sector rather than a diagram *(done)*

Arcs 1–6 built a correct game that did not play. Four things were wrong and they had one
cause between them.

**Nothing was drawn but the record.** The draw list was uploaded once, outside the frame loop,
and `view.ts` had no projectile handling at all. Shots were created, stepped, resolved and hit
things; craft moved; destroyed contacts were marked — and the screen was a still photograph of
the record for the whole session. Every symptom the player reported ("the lasers do not work",
"the photon missiles do not work") was this. **`lib/scemaworld/game.ts`** now holds the whole
tick as one pure function, which is the only reason that class of bug is catchable: the
regression test fires a shot and asserts it is in the draw list, with no GPU involved.

**The sector was enlarged sixty-fold and nothing else moved.** Positions scaled; station radii,
weapon reach, sensor range and aggro range stayed at the tuning for a volume 1/60th the size,
because they were bare `26_000 * UNIT` literals in five modules that each declared their own
`UNIT`. The result was technically enormous and unplayable in every specific way — stations
were sixteen-thousand-unit specks in a three-hundred-and-seventy-five-million-unit void, and a
laser's entire range was a fifth of the distance at which an enemy first noticed you. Nothing
failed, because every test compared constants that had all failed to move together.

`lib/scemaworld/scale.ts` is the fix and the note explaining it. Distances are declared as
**fractions of the sector**, speeds as **how long it takes to cross one**. Read that way a
number carries its own review: `EXTENT / 26` is "a bit under half a minute to cross", which is
a claim you can argue with. `9_000 * UNIT` is not. A test asserts no other module in the
directory contains the token `UNIT` at all.

**The record's first signal was placed on the origin node, which is where the player spawns.**
The game opened with a hostile inside the cockpit and the first shot hit before leaving the
muzzle.

**A sector had five things in it.** Hostiles came only from the record's signals, and a record
has a handful. `lib/scemaworld/raiders.ts` gives the sector its own population — and getting
its density rule right took two attempts, which is worth recording. Raiders were placed at a
rate *per node*, which looks uniform and is not: blind spots are placed into the sector as rift
nodes and reported extent drives the fractal's depth, so a record claiming less of both grew a
smaller node list and **bought a quieter sector**. That is the `ship.ts` rule broken —
misreporting rewarded — and the test caught what the reasoning had not. Raiders now touch the
node list not at all: a fixed forty, scattered through the volume at seed-derived positions.
Every world is exactly as dangerous as every other and only the arrangement varies.

A raider carries `unlogged: true` and lives in `space.raiders`, never in `space.contacts`. It
is not a claim about anything — it is furniture the game placed — and the one thing that must
not happen is furniture becoming indistinguishable from a signal somebody counted. It draws
orange rather than red and the HUD says *not in the record*. Its threat reads as a real number,
unlike a ghost's, and that is not an inconsistency: the em dash marks a quantity **the record
left unmeasured**, and the record makes no statement about a raider at all. The inversion is a
good one to play — the things the record described are the uncertain ones.

**And you could not find anything.** Enlarging the volume fixed one complaint and created a
worse one: a thousand stations spread over forty seconds of travel are, from the cockpit, a
black screen with a few points of light in it, and every service in the game sits on a node you
have to *reach*. `lib/scemaworld/nav.ts` is a target computer — nearest services by kind, a
cycling waypoint, range and bearing. It reports **geometry and never a verdict**: it will route
you to a `phantom`, a station the observer *modelled* rather than saw, and label it one. A nav
computer that filtered out unreliable destinations would make the record's uncertainty
invisible at exactly the moment the player acts on it.

### The economy rule, sharpened

Arc 4 said "no economy" and a test enforced it. The rule was aimed at a real failure and stated
too broadly. The failure is precisely this: **no quantity in the record may translate into a
reward.** Attach a payout to `blind_spots` and you have paid somebody to hide them; attach one
to magnitude and you have paid them to understate it.

So there is a ship, and fuel, and salvage, and six upgradeable components — and salvage is
earned from **what you do**: destroying a hostile, scavenging a derelict you flew out to. A
world with more blind spots is not worth more. It is worth the same and is harder to survive.
`check:scemaworld` asserts the reward function reads no record field, and asserts it twice —
once on `ship.ts` and once on `raiders.ts`, which is where it actually broke.

It stays single-player. No transfer, no price, no token: the moment salvage is worth something
outside the game, that paragraph stops holding.

### Arc 5 — Entitlement *(done)*

`lib/scemaworld/vault.ts`. Drop a PNG, get its commitment, fetch the record from a vault you
hold the token for, fly it. `scema-vault` and `scema-entitlement` already existed; this is the
client half and the only network call in the whole game.

The three-answer rule survives to the player. A 403 is a fact about the holder and does not
invite a retry; a **503 is undetermined and says so** — told "you do not own this", somebody
goes and buys a token they already have. A 404 says the gap belongs to the vault rather than
to the entitlement. An unreachable vault names the URL it tried, which is the lesson `/mesh`
paid for: collapsing every failure into one diagnosis is wrong exactly when a healthy service
is configured at a bad address.

**The vault is not trusted.** It serves bytes; it does not certify them. The record is verified
in the browser exactly as a dropped file is, *and* bound to the commitment that was requested —
no signature on a record can say it is the one you asked for, so a vault returning a different
world is caught rather than flown.

The token's utility is exactly this and nothing more: **the space it describes.**

### Arc 6 — Fleets *(done)*

`lib/scemaworld/fleet.ts`. Many records, one galaxy. Node ids are renumbered and contact ids
namespaced by world, because two records can legitimately carry the same signal id and a fleet
must not merge two different things into one target.

**Placement comes from each world's commitment, and the worlds are sorted by it before
placing.** So the galaxy is a function of *which* records are held, not of the order somebody
dropped them — two players comparing notes are describing the same arrangement. An index-based
layout would have been simpler and would have quietly made the map depend on a UI event order.

**One unknown sensor range makes the whole fleet unknown**, rather than taking the minimum of
the measured ones. That would report a confident figure computed over an incomplete set — the
coverage mistake, in a new place.

A bridge means only *these two records are both yours*. It is **not** a claim that the observed
things are related: a repository and a set of oracle feeds have nothing to do with each other,
and a lane between them must not suggest otherwise. Bridges get their own role so they cannot
be mistaken for lanes inside a world, which are structural.

## What must never happen to this

- **No server-authoritative map.** The moment a server decides the space, the record stops
  being the map and the whole argument collapses.
- **No tuning constants that are not from the record.** A "feels better" multiplier is the
  first step to a random seed with a token stapled to it.
- **No record quantity may set a payout.** The sharpened form of "no economy" — see Arc 7.
  This is the one that will be argued for most often, and the one that broke first.
- **No distance literals outside `scale.ts`.** The sixty-fold enlargement is not a thing that
  happens once.
- **No blocking an invalid record.** Show it, mark it, let people look at a forgery. A verifier
  people cannot experiment with is one they stop believing.

## Running it

```console
$ cd web && npm run check:scemaworld     # generator, scale, combat, nav, a whole flight — 107
$ cd web && npm run dev                  # /scema-world
```

The last three checks are end-to-end and are the ones that would have caught Arc 7's bugs:
ninety seconds of held throttle must put a shot on screen for more than a thousand frames and
cross the sector; thirty seconds of point-blank fire must destroy a raider and pay a flat
bounty; and an autopilot must be able to route to a depot, fly there, and refuel — because a
fuel economy in a volume you cannot navigate is not an economy, it is a timer.
