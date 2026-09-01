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

Numbered, in dependency order. Each is shippable on its own.

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
| arrows | lateral and vertical thrusters |
| `SHIFT` / `CTRL` | main drive throttle up / down |
| right click | fire |
| left click | switch weapon |

Roll is on `Q`/`E` because it is the one rotational axis `WASD` leaves unreachable, and a
space game without roll has an up — which this one does not. The main drive is not in the
original scheme; a ship that cannot go forward is not a ship, so it is on `SHIFT`/`CTRL` and
the HUD says so.

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

### Arc 6 — Fleets

Many records, one session. A player's owned worlds become a cluster with lanes between them,
so a corpus is a galaxy. Needs nothing new from the runtime — it is Arc 1 run N times with a
join.

## What must never happen to this

- **No server-authoritative map.** The moment a server decides the space, the record stops
  being the map and the whole argument collapses.
- **No tuning constants that are not from the record.** A "feels better" multiplier is the
  first step to a random seed with a token stapled to it.
- **No economy.** See above. This is the one that will be argued for most often.
- **No blocking an invalid record.** Show it, mark it, let people look at a forgery. A verifier
  people cannot experiment with is one they stop believing.

## Running it

```console
$ cd web && npm run check:scemaworld     # the generator, 17 checks
$ cd web && npm run dev                  # /scema-world once Arc 2 lands
```
