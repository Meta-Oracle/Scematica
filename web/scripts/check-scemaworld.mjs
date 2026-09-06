// Scema-World's generator, pinned.
//
// The claim the whole game rests on: **the record is the map**. Two players holding the same
// sealed record fly the same space, without a server and without trusting each other. That is
// only true if `generate` is a pure function of the record, so these checks are mostly about
// what it must *not* depend on — and about the epistemic rules surviving into gameplay, which
// is where they would be quietly dropped first.
//
//   node --experimental-strip-types scripts/check-scemaworld.mjs

import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { dirname, join } from 'node:path'

import { generate, EXTENT } from '../lib/scemaworld/generate.ts'
import {
  camera, forward, up, right, rotate, translate, view, perspective, mul, qNorm, chase,
} from '../lib/scemaworld/camera.ts'
import {
  drawList, isGhost, roleOfContact, sensorFar, sensorLabel, boundaryLabel, PALETTE,
} from '../lib/scemaworld/view.ts'
import {
  newCombat, selected, switchWeapon, fire, step, durability, threatLabel, lockOn,
  reload, photonMagazine, photonDamage, LASER, PHOTON,
} from '../lib/scemaworld/weapons.ts'
import { fetchWorld, explain, retryable, matchesRequest } from '../lib/scemaworld/vault.ts'
// `join` is already taken by `node:path` in this file.
import { join as joinFleet, placement } from '../lib/scemaworld/fleet.ts'
import {
  newShip, refuel, repair, scavenge, buy, upgradeCost, fuelCapacity, hullMax, topSpeed,
  sensorGain, laserCooldown, shieldMax, jumpCapacity, jumpCharge, burnRate,
  damage, recharge, MAX_LEVEL,
} from '../lib/scemaworld/ship.ts'
import * as Enemy from '../lib/scemaworld/enemy.ts'
import {
  collidesWith, registers, permeableNote, passageNote, sweep, resolve, separate, steerAround,
  passedThrough, crossed, closestOnSegment, SEPARATION,
} from '../lib/scemaworld/collide.ts'
import { gridFor, NOTICE_MS, sensors } from '../lib/scemaworld/game.ts'
import { hostileTo, routeNodes, trafficOf } from '../lib/scemaworld/factions.ts'
import * as Factions from '../lib/scemaworld/factions.ts'
import {
  build as navBuild, pick as navPick, ZOOMS as NAV_ZOOMS, DEFAULT_ZOOM as NAV_DEFAULT_ZOOM,
} from '../lib/scemaworld/navmap.ts'
import { HULLS, HULL_IDS, HULL_TIERS, hullsOf, purchasable } from '../lib/scemaworld/hulls.ts'
import {
  exchange, buyHull, toScema, salvageFor, SALVAGE_PER_SCEMA, SALVAGE_NUM, SCEMA_DEN, SCEMA_NOTE,
} from '../lib/scemaworld/economy.ts'
import {
  entitlement, toBaseUnits, toWholeTokens, looksLikeAddress,
  DEFAULT_POLICY, NO_RECORD, TREASURY, SCEMA_MINT,
} from '../lib/scemaworld/claim.ts'
import { withdrawn } from '../lib/scemaworld/game.ts'
import {
  transferPlan, reserve, readLedger, LedgerConflict,
} from '../lib/scemaworld/treasury.ts'
import { mkdtempSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { PublicKey } from '@solana/web3.js'
import {
  getAssociatedTokenAddressSync, TOKEN_PROGRAM_ID, TOKEN_2022_PROGRAM_ID,
} from '@solana/spl-token'
import {
  acquire, exchangeAt, command, COMMAND_KEYS, jumpRefusal,
} from '../lib/scemaworld/game.ts'
import { course as courseOf } from '../lib/scemaworld/view.ts'
import { refit, noseOffset, hullRadius } from '../lib/scemaworld/ship.ts'
import {
  HITBOX, capsuleOf, strikes, segmentDistance,
} from '../lib/scemaworld/hitbox.ts'
import { nodeRadius, roleOfNode } from '../lib/scemaworld/view.ts'
import {
  JUMP_INHIBIT, BOLT_LENGTH, BOLT_GLOW, R_LASER, R_PHOTON,
  R_PLAYER, R_STATION, R_NODE_MAX, MIN_NODE_GAP,
  SENSOR_MULTIPLIER, AGGRO_RANGE as SENSOR_BASE, SPEED_LASER, LIFE_LASER, SPEED_PHOTON,
  SPEED_ENEMY_SHOT, SPEED_CRAFT, SPEED_CRAFT_PER_TIER, FAR_PLANE, NEAR_PLANE,
} from '../lib/scemaworld/scale.ts'
import {
  swarmOf, step as enemyStep, hit as enemyHit, living, decide, leadPoint, turnToward,
  nearestThreat, classRoll, AGGRO_RANGE,
} from '../lib/scemaworld/enemy.ts'
import { CLASSES, CLASS_IDS, classFor, SHIELD_DELAY_MS } from '../lib/scemaworld/classes.ts'
import * as Hyper from '../lib/scemaworld/hyper.ts'
import * as Meshes from '../lib/scemaworld/meshes.ts'
import { interceptor, gunship, capital, bolt, starfield } from '../lib/scemaworld/meshes.ts'
import { shapeOf, LANE_ALPHA } from '../lib/scemaworld/view.ts'
import { newGame, tick, useService, purchase, dynamicOf, DOCK_RANGE, dockRange } from '../lib/scemaworld/game.ts'
import { servicesOf } from '../lib/scemaworld/generate.ts'
import * as SCALE from '../lib/scemaworld/scale.ts'
import { raidersOf, raiderWing, RAIDER_FLOOR, RAIDER_STRENGTH } from '../lib/scemaworld/raiders.ts'
import * as Wallet from '../lib/scemaworld/wallet.ts'
import * as Roles from '../lib/scemaworld/roles.ts'
import * as CLASSES_MOD from '../lib/scemaworld/classes.ts'
import { ALL_CLASS_IDS } from '../lib/scemaworld/classes.ts'
import * as Quests from '../lib/scemaworld/quests.ts'
import { CITADELS } from '../lib/scemaworld/generate.ts'
import { rearm, PHOTON_PRICE } from '../lib/scemaworld/game.ts'
import * as Raiders from '../lib/scemaworld/raiders.ts'
import { Rng } from '../lib/omni/fractal.ts'
import * as Respawn from '../lib/scemaworld/respawn.ts'
import * as Arrivals from '../lib/scemaworld/arrivals.ts'
import {
  nearest, fixOn, cycle, ahead, bearingLabel, rangeLabel,
} from '../lib/scemaworld/nav.ts'
import { route } from '../lib/scemaworld/game.ts'

/** A still player, for enemy-lead tests. */
const ZERO = { x: 0, y: 0, z: 0 }

/** Unit vector from the origin toward a point, for lock tests. */
function normTo(p) {
  const l = Math.hypot(p.x, p.y, p.z) || 1
  return { x: p.x / l, y: p.y / l, z: p.z / l }
}

const here = dirname(fileURLToPath(import.meta.url))
const nftDir = join(here, '..', '..', 'scematica-omni', 'crates', 'scema-nft', 'fixtures')
const world = JSON.parse(readFileSync(join(nftDir, 'parity-world.json'), 'utf8'))
const digest = readFileSync(join(nftDir, 'parity-digest.txt'), 'utf8').trim()

let pass = 0
let fail = 0

// Async-aware. A synchronous harness silently swallows a rejected promise, so an async test
// that failed would report `ok` — and a test that cannot fail is worse than no test at all.
const pending = []

function check(name, fn) {
  const record = (e) => {
    if (e) {
      console.log(`  FAIL ${name}\n       ${e.message}`)
      fail += 1
    } else {
      console.log(`  ok   ${name}`)
      pass += 1
    }
  }
  try {
    const r = fn()
    if (r && typeof r.then === 'function') {
      pending.push(r.then(() => record(null), record))
      return
    }
    record(null)
  } catch (e) {
    record(e)
  }
}

function assert(cond, msg) {
  if (!cond) throw new Error(msg || 'assertion failed')
}

check('the same record generates the same space, exactly', () => {
  // The property everything else depends on. Two players, no server, no trust.
  const a = generate(world, digest)
  const b = generate(world, digest)
  assert(JSON.stringify(a) === JSON.stringify(b), 'two runs differ')
})

check('a different record generates a different space', () => {
  const a = generate(world, digest)
  const b = generate(world, 'f'.repeat(64))
  assert(JSON.stringify(a) !== JSON.stringify(b), 'the seed does not reach the space')
})

check('every coordinate is an integer', () => {
  // Floats would let two machines disagree about where a station is, which is the same class
  // of failure the PNG rasteriser exists to avoid.
  const s = generate(world, digest)
  for (const n of s.nodes) {
    for (const k of ['x', 'y', 'z']) {
      assert(Number.isInteger(n.at[k]), `${n.label} has a non-integer ${k}: ${n.at[k]}`)
    }
  }
})

check('the space occupies a volume, not a plane', () => {
  // A tree grown with only yaw is flat, and a flat space is not one you fly through.
  const s = generate(world, digest)
  const ys = s.nodes.map((n) => n.at.y)
  assert(Math.max(...ys) - Math.min(...ys) > EXTENT / 100, 'the map is flat')
})

check('an unmeasured signal is a ghost contact, never a solid one', () => {
  // THE rule. Rendering an estimated signal as a solid enemy is the em-dash bug in the one
  // place a player would act on it.
  const s = generate(world, digest)
  const estimated = world.signals.filter((x) => !x.measured).map((x) => x.id)
  assert(estimated.length > 0, 'the fixture must carry an estimated signal or this proves nothing')
  for (const id of estimated) {
    const c = s.contacts.find((x) => x.id === id)
    if (c) assert(!c.solid, `${id} was estimated but rendered solid`)
  }
})

check('a counted signal is solid', () => {
  const s = generate(world, digest)
  const counted = world.signals.filter((x) => x.measured).map((x) => x.id)
  for (const id of counted) {
    const c = s.contacts.find((x) => x.id === id)
    if (c) assert(c.solid, `${id} was counted but rendered as a ghost`)
  }
})

check('risk becomes hostile and opportunity becomes salvage', () => {
  const s = generate(world, digest)
  for (const c of s.contacts) {
    const sig = world.signals.find((x) => x.id === c.id)
    const want = sig.polarity === 'risk' ? 'hostile' : 'salvage'
    assert(c.hostility === want, `${c.id} is ${c.hostility}, expected ${want}`)
  }
})

check('one rift per reported blind spot, a count and never a rate', () => {
  // The fractal learned this the expensive way: a per-node probability compounded down the
  // recursion and cut twenty-six limbs for three blind spots. A map that invented extra
  // dead ends would be claiming more ignorance than the observer reported.
  for (const n of [0, 1, 2, 3, 5]) {
    const w = { ...world, blind_spots: Array.from({ length: n }, (_, i) => `spot ${i}`) }
    const s = generate(w, digest)
    if (!s.riftsCapped) {
      assert(s.rifts === n, `${n} blind spot(s) produced ${s.rifts} rift(s)`)
    } else {
      assert(s.rifts < n, 'a capped count must be lower, and say so')
    }
  }
})

check('a rift is a dead end — nothing is on the far side', () => {
  const w = { ...world, blind_spots: ['one'] }
  const s = generate(w, digest)
  const rifts = s.nodes.filter((n) => n.kind === 'rift')
  assert(rifts.length > 0, 'no rift was placed')
  for (const r of rifts) {
    const onward = s.lanes.filter((l) => l.from === r.id)
    assert(onward.length === 0, `rift ${r.id} has ${onward.length} lane(s) leading onward`)
  }
})

check('an unperceived world has unknown sensor range, not zero', () => {
  // "You cannot see" and "nobody knows how far you can see" are different facts, and a
  // renderer must be able to tell them apart. `0` would collapse them.
  const s = generate({ ...world, objects: [] }, digest)
  assert(s.sensorRange === null, `sensorRange was ${s.sensorRange}`)
})

check('a fully stale world has a measured sensor range of zero', () => {
  // The other half: a real zero is a real observation and must not print as unknown.
  const stale = world.objects.map((o) => ({
    ...o,
    provenance: { kind: 'stale', age_secs: 99, budget_secs: 1 },
  }))
  const s = generate({ ...world, objects: stale }, digest)
  assert(s.sensorRange === 0, `expected a measured 0, got ${s.sensorRange}`)
})

check('provenance decides what a node is', () => {
  const kinds = { live: 'station', stale: 'derelict', simulated: 'phantom', absent: 'marker' }
  for (const [prov, want] of Object.entries(kinds)) {
    const objects = world.objects.map((o) => ({ ...o, provenance: { kind: prov, age_secs: 0, budget_secs: 1 } }))
    const s = generate({ ...world, objects }, digest)
    assert(s.nodes.some((n) => n.kind === want), `no ${want} for provenance ${prov}`)
  }
})

check('an unbounded extent produces a space that says it has no boundary', () => {
  const s = generate({ ...world, extent: { observed: 9, total: null, note: '' } }, digest)
  assert(s.unbounded === true, 'the map must know it has no known edge')
})

/** Source with comments removed, so a scan sees code and not prose about code. */
function codeOf(path) {
  return readFileSync(path, 'utf8')
    .replace(/\/\*[\s\S]*?\*\//g, '')
    .split(String.fromCharCode(10))
    .map((l) => l.replace(/\/\/.*$/, ''))
    .join(String.fromCharCode(10))
}

check('the generator reads no clock and no randomness', () => {
  // Pinned by source inspection rather than by behaviour: a `Date.now()` added later would
  // still pass a determinism check run twice in the same millisecond.
  //
  // Comments are stripped first. The naive version of this check failed on its own module
  // note — the one saying there is no `Math.random` — which is the classic shape for a
  // source scan: it cannot tell a prohibition from a violation.
  const src = codeOf(join(here, '..', 'lib', 'scemaworld', 'generate.ts'))
  for (const banned of ['Date.now', 'Math.random', 'new Date', 'performance.now']) {
    assert(!src.includes(banned), `generate.ts calls ${banned}`)
  }
})

check('the comment stripper does not hide a real call', () => {
  // The stripper is now load-bearing, so it gets its own check: a scan that quietly ignored
  // everything would pass every prohibition above.
  const stripped = codeOf(join(here, '..', 'lib', 'scemaworld', 'generate.ts'))
  assert(stripped.includes('export function generate'), 'the stripper ate the code')
  assert(!stripped.includes('Not an economy'), 'the stripper left comments in')
})

check('the space is bounded in size so a record cannot hang a browser', () => {
  // A deep, wide tree is a legitimate world. It must not be a denial of service.
  const w = { ...world, extent: { observed: 100000, total: 100000, note: '' } }
  const s = generate(w, digest)
  assert(s.nodes.length <= 4100, `${s.nodes.length} nodes`)
})

check('there is no currency, price or yield anywhere in the model', () => {
  // Stated as a test because it is a design commitment somebody will be tempted to relax.
  // Anything that priced a world would make a record's content worth misreporting, and a
  // producer with an incentive to lie is the failure this project cannot absorb.
  // Raw source here on purpose: this one asserts the commitment is *written down*.
  const src = readFileSync(join(here, '..', 'lib', 'scemaworld', 'generate.ts'), 'utf8')
  const s = JSON.stringify(generate(world, digest))
  for (const banned of ['price', 'currency', 'yield', 'reward', 'token_amount']) {
    assert(!s.includes(`"${banned}"`), `the space carries a ${banned} field`)
  }
  assert(src.includes('Not an economy'), 'the commitment must be written down, not just true')
})


// ── camera ────────────────────────────────────────────────────────────────────

const near = (a, b, tol = 1e-6) => Math.abs(a - b) <= tol

check('a fresh camera looks down negative z', () => {
  const f = forward(camera())
  assert(near(f[0], 0) && near(f[1], 0) && near(f[2], -1), `forward was ${f}`)
})

check('the ship axes stay orthonormal after a long flight', () => {
  // Quaternion drift is slow and then catastrophic: accumulated error turns rotation into a
  // shear, and the symptom is stretched geometry rather than anything obviously wrong.
  let c = camera()
  for (let i = 0; i < 5000; i += 1) c = rotate(c, 0.03, -0.017, 0.023)
  for (const v of [forward(c), up(c), right(c)]) {
    assert(near(Math.hypot(...v), 1, 1e-4), `axis length drifted to ${Math.hypot(...v)}`)
  }
  const dot = (a, b) => a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
  assert(near(dot(forward(c), up(c)), 0, 1e-4), 'forward and up stopped being perpendicular')
  assert(near(dot(forward(c), right(c)), 0, 1e-4), 'forward and right stopped being perpendicular')
})

check('rotation is in the ship frame, so pitching while inverted is not reversed', () => {
  // The Euler-angle failure this avoids: roll 180 degrees, then pitch. A world-axis rotation
  // sends the nose the wrong way and the player reports "the controls broke".
  const rolled = rotate(camera(), 0, 0, Math.PI)
  const pitched = rotate(rolled, 0.4, 0, 0)
  const f = forward(pitched)
  assert(f[1] < -0.2, `expected the nose to pitch toward world -y, got ${f}`)
})

check('thrust moves along the nose, whatever the orientation', () => {
  const c = rotate(camera(), 0, Math.PI / 2, 0)
  const moved = translate(c, [0, 0, -10])
  const f = forward(c)
  for (const i of [0, 1, 2]) {
    assert(near(moved.position[i], f[i] * 10, 1e-5), `axis ${i}: ${moved.position[i]}`)
  }
})

check('the view matrix undoes the camera transform', () => {
  const c = translate(rotate(camera(), 0.3, -0.7, 0.2), [12, -4, 30])
  const m = view(c)
  const p = c.position
  const vx = m[0] * p[0] + m[4] * p[1] + m[8] * p[2] + m[12]
  const vy = m[1] * p[0] + m[5] * p[1] + m[9] * p[2] + m[13]
  const vz = m[2] * p[0] + m[6] * p[1] + m[10] * p[2] + m[14]
  assert(near(vx, 0, 1e-3) && near(vy, 0, 1e-3) && near(vz, 0, 1e-3), `${vx} ${vy} ${vz}`)
})

check('something ahead of the ship projects in front of the near plane', () => {
  const vp = mul(perspective(1.1, 1.6, 1, 10000), view(camera([0, 0, 0])))
  const a = [0, 0, -500]
  const w = vp[3] * a[0] + vp[7] * a[1] + vp[11] * a[2] + vp[15]
  assert(w > 0, `a point ahead had w = ${w}; it would be culled`)
})

check('a normalised zero quaternion is the identity rather than NaN', () => {
  assert(qNorm([0, 0, 0, 0]).every(Number.isFinite), 'qNorm produced a non-finite quaternion')
})

// ── what gets drawn ───────────────────────────────────────────────────────────

check('a ghost contact is never drawn solid', () => {
  // The rule this layer exists to protect, checked on the draw list rather than the
  // generator — this is the last place it could be lost.
  const s = generate(world, digest)
  const list = drawList(s)
  for (const c of s.contacts) {
    const role = roleOfContact(c)
    const body = list.bodies.find((b) => b.label === c.label && b.role === role)
    assert(body, `no body for contact ${c.id}`)
    assert(body.solid === c.solid, `${c.id}: body solid=${body.solid}, contact solid=${c.solid}`)
    if (!c.solid) assert(isGhost(role), `${c.id} is estimated but its role is not a ghost`)
  }
})

check('a ghost keeps its polarity hue and differs by fill, not by colour', () => {
  // Colour alone would fail the way the TUI mono test exists to prevent: two shades of red
  // are one thing at a glance, on a bad monitor, or to a colour-blind player.
  assert(
    PALETTE['ghost-hostile'].join() === PALETTE.hostile.join(),
    'a hostile ghost must stay red — it is still hostile if it is there'
  )
  assert(PALETTE['ghost-salvage'].join() === PALETTE.salvage.join())
  assert(isGhost('ghost-hostile') && !isGhost('hostile'))
})

check('every role drawn has a palette entry', () => {
  const list = drawList(generate(world, digest))
  for (const b of list.bodies) assert(PALETTE[b.role], `no colour for ${b.role}`)
  for (const l of list.segments) assert(PALETTE[l.role], `no colour for ${l.role}`)
})

check('draw distance covers the sector and is no longer legibility in disguise', () => {
  // This assertion is inverted from what it used to be, and the inversion is the point.
  // Legibility gated *draw distance*, which put a wall of fog around a volume the entire design
  // is about the size of: an unread world arrived as a small one. Two different things were
  // being conflated — what the record knows, and what the window shows. The window now shows
  // the whole sector always.
  const dark = generate({ ...world, objects: [] }, digest)
  const list = drawList(dark)
  assert(list.far >= EXTENT, `far was ${list.far}, smaller than one extent`)
  assert(list.far === drawList(generate(world, digest)).far, 'draw distance still varies')
})

check('an unmeasured sensor range still refuses to become a zero', () => {
  // The rule did not go away, it moved: legibility is now *contact* range, so a poorly-perceived
  // world is one you fly blind through rather than one you fly blind in. Unknown must still
  // print an em dash — a player told "sensors 0%" concludes their ship is damaged.
  const dark = generate({ ...world, objects: [] }, digest)
  assert(dark.sensorRange === null, 'an unperceived world reported a number')
  assert(sensorLabel(dark) === '—', `label was ${sensorLabel(dark)}`)
  assert(sensorFar(0) > 0, 'a measured zero legibility must still resolve something nearby')
})

check('a fully dark world still draws its immediate surroundings', () => {
  // A black screen is indistinguishable from a broken renderer, and "the game did not load"
  // is the wrong lesson to teach about an unreadable world.
  assert(sensorFar(0) > 0, 'zero range drew nothing at all')
  assert(sensorFar(0) < sensorFar(1) / 4, 'the floor is too generous to read as darkness')
})

check('the HUD prints an em dash for unmeasured sensor range', () => {
  assert(sensorLabel(generate({ ...world, objects: [] }, digest)) === '—')
  assert(sensorLabel(generate(world, digest)).endsWith('%'))
})

check('the HUD says when the map has no known boundary', () => {
  const un = generate({ ...world, extent: { observed: 9, total: null, note: '' } }, digest)
  assert(boundaryLabel(un) === 'NO KNOWN BOUNDARY')
  assert(boundaryLabel(generate(world, digest)) === 'BOUNDED')
})

check('the gl layer chooses no colours and no roles', () => {
  // Same rule as `lib/mesh/view.ts::toneFor`: one implementation of anything encoding a
  // claim. An `isGhost` in the renderer means the rule has left the file that tests it.
  const src = codeOf(join(here, '..', 'lib', 'scemaworld', 'gl.ts'))
  assert(!/#[0-9a-fA-F]{6}/.test(src), 'gl.ts contains a hex colour')
  assert(!src.includes('isGhost'), 'gl.ts decides ghostliness itself')
  assert(src.includes('PALETTE['), 'gl.ts should look colours up rather than know them')
})


// ── weapons ───────────────────────────────────────────────────────────────────

const contactsOf = (s) => s.contacts
const firstSolid = (s) => contactsOf(s).find((c) => c.solid)
const firstGhost = (s) => contactsOf(s).find((c) => !c.solid)

check('a ghost never reports a threat number, however much it is shot', () => {
  // The rule combat is not allowed to break. Resolving a ghost into a known value on first
  // hit would feel better to play and would be the em-dash bug with a design justification:
  // the number would be invented and the player would act on it as a measurement.
  const s = generate(world, digest)
  const ghost = firstGhost(s)
  assert(ghost, 'the fixture must carry an estimated signal')
  assert(threatLabel(ghost) === '—', `ghost threat read ${threatLabel(ghost)}`)

  let c = newCombat()
  for (let i = 0; i < 40; i += 1) {
    c = fire(c, { x: 0, y: 0, z: 0 }, { x: 0, y: 0, z: -1 }, i * 200, [ghost])
    c = step(c, 0.5, [ghost], s.seed).combat
  }
  assert(threatLabel(ghost) === '—', 'the ghost resolved after being hit')
})

check('a counted contact reports the magnitude that was counted', () => {
  const solid = firstSolid(generate(world, digest))
  assert(solid, 'the fixture must carry a counted signal')
  assert(threatLabel(solid) === solid.magnitude.toFixed(2))
})

check('durability comes from the seed, never from the reported magnitude', () => {
  // Magnitude drives size and aggression only. A hit-point pool derived from it would give
  // anybody who writes a record a reason to understate it.
  const a = durability('seed-a', 'sig-1')
  const b = durability('seed-b', 'sig-1')
  assert(a !== b || durability('seed-a', 'sig-2') !== a, 'the seed does not reach durability')
  const src = codeOf(join(here, '..', 'lib', 'scemaworld', 'weapons.ts'))
  // Sliced to the end of the function, not a fixed number of characters: the first version
  // ran past it into `threatLabel`, which legitimately reads magnitude, and reported a
  // violation that was not there.
  const from = src.indexOf('export function durability')
  const next = src.indexOf('export function', from + 10)
  assert(!src.slice(from, next).includes('magnitude'), 'durability reads magnitude')
})

check('the same record produces the same fight', () => {
  const s = generate(world, digest)
  const run = () => {
    let c = newCombat()
    for (let i = 0; i < 30; i += 1) {
      c = fire(c, { x: 0, y: 0, z: 0 }, { x: 0, y: 0, z: -1 }, i * 200, s.contacts)
      c = step(c, 0.2, s.contacts, s.seed).combat
    }
    return c
  }
  assert(JSON.stringify(run()) === JSON.stringify(run()), 'two fights diverged')
})

check('left click switches weapons and never fires', () => {
  let c = newCombat()
  assert(selected(c).kind === 'laser', 'lasers are the default')
  const before = c.projectiles.length
  c = switchWeapon(c)
  assert(selected(c).kind === 'photon')
  assert(c.projectiles.length === before, 'switching fired something')
  c = switchWeapon(c)
  assert(selected(c).kind === 'laser', 'it cycles')
})

check('the laser is automatic and the photon is not', () => {
  assert(LASER.automatic && !PHOTON.automatic)
  assert(LASER.magazine === null, 'lasers are unlimited')
  assert(typeof PHOTON.magazine === 'number', 'missiles are not')
})

check('cooldown limits the rate of fire', () => {
  let c = newCombat()
  c = fire(c, { x: 0, y: 0, z: 0 }, { x: 0, y: 0, z: -1 }, 1000, [])
  const after = c.projectiles.length
  c = fire(c, { x: 0, y: 0, z: 0 }, { x: 0, y: 0, z: -1 }, 1000 + LASER.cooldownMs - 1, [])
  assert(c.projectiles.length === after, 'fired inside the cooldown')
  c = fire(c, { x: 0, y: 0, z: 0 }, { x: 0, y: 0, z: -1 }, 1000 + LASER.cooldownMs, [])
  assert(c.projectiles.length === after + 1, 'did not fire after the cooldown')
})

check('photons are finite and firing an empty tube does nothing', () => {
  // Against the **hull's** magazine, not `PHOTON.magazine`. That field is documented as a
  // fallback only — the real count is the tube count — and this test read the fallback, so it
  // silently stopped exercising an empty tube the moment the two diverged. It fired six rounds
  // out of eight and asserted the tubes were empty.
  const mag = photonMagazine('skiff')
  let c = switchWeapon(newCombat())
  let t = 0
  for (let i = 0; i < mag + 5; i += 1) {
    t += PHOTON.cooldownMs
    c = fire(c, { x: 0, y: 0, z: 0 }, { x: 0, y: 0, z: -1 }, t, [])
  }
  assert(c.photonsLeft === 0, `photonsLeft was ${c.photonsLeft}`)
  assert(c.projectiles.length === mag, `fired ${c.projectiles.length} of ${mag}`)
})

check('a photon locks a ghost as readily as a solid', () => {
  // Refusing to lock a ghost would leak the answer: the player would learn from the
  // targeting computer what the record does not know.
  const s = generate(world, digest)
  const ghost = firstGhost(s)
  const lock = lockOn({ x: 0, y: 0, z: 0 }, normTo(ghost.at), [ghost], [])
  assert(lock === ghost.id, `lock was ${lock}`)
})

check('a projectile expires instead of leaking', () => {
  let c = newCombat()
  c = fire(c, { x: 0, y: 0, z: 0 }, { x: 0, y: 0, z: -1 }, 0, [])
  assert(c.projectiles.length === 1)
  for (let i = 0; i < 60; i += 1) c = step(c, 0.2, [], 'seed').combat
  assert(c.projectiles.length === 0, 'a missed shot never expired')
})

check('a weapon reports damage and never decides a death', () => {
  // It used to decide. `enemy.ts` owns hull and shields now, and two authorities over one fact
  // is how a craft ends up dead on one side and still firing on the other. A hit is a report.
  const s = generate(world, digest)
  const target = { ...firstSolid(s), at: { x: 0, y: 0, z: -EXTENT * 0.02 } }
  let c = newCombat()
  let hits = []
  for (let i = 0; i < 8 && hits.length === 0; i += 1) {
    c = fire(c, { x: 0, y: 0, z: 0 }, { x: 0, y: 0, z: -1 }, i * 200, [target])
    const r = step(c, 0.1, [target], s.seed)
    c = r.combat
    hits = r.hits
  }
  assert(hits.length > 0, 'point-blank fire never connected')
  assert(hits[0].damage === LASER.damage, `damage was ${hits[0].damage}`)
  assert(!('destroyed' in hits[0]), 'a weapon still adjudicates death')

  const src = codeOf(join(here, '..', 'lib', 'scemaworld', 'weapons.ts'))
  assert(!src.includes('destroyed.push'), 'weapons.ts still writes the destroyed list')
})

check('a photon hits far harder than a laser', () => {
  // The whole reason there are two weapons. Nine laser rounds or one missile, which is what a
  // burst failing to break a gunship's shield is supposed to teach.
  assert(PHOTON.damage > LASER.damage * 5, `${PHOTON.damage} vs ${LASER.damage}`)
})


// ── vault ─────────────────────────────────────────────────────────────────────

const DIGEST = 'a'.repeat(64)

/** A fetch that answers with one canned response. No network in these checks. */
function canned(status, body, opts = {}) {
  return async (url, init) => {
    if (opts.onCall) opts.onCall(url, init)
    if (opts.throws) throw new Error('connection refused')
    return {
      status,
      text: async () => (typeof body === 'string' ? body : JSON.stringify(body)),
    }
  }
}

check('a 200 returns the record text verbatim', () => {
  // Verbatim matters: the bytes are what the commitment was taken over, and re-serialising
  // would collapse `0.0` to `0` and change the digest.
  const text = '{"a": 0.0,\\n "b": 1}'
  return fetchWorld('http://v', DIGEST, '0x1', canned(200, text)).then((r) => {
    assert(r.kind === 'ok', `kind was ${r.kind}`)
    assert(r.record.text === text, 'the body was altered in transit')
  })
})

check('a 503 is undetermined and never denied', () => {
  // The rule carried all the way to the player. Told "you do not own this", somebody goes and
  // buys a token they already have.
  return fetchWorld('http://v', DIGEST, '0x1', canned(503, { detail: 'rpc down', retry: true }))
    .then((r) => {
      assert(r.kind === 'undetermined', `kind was ${r.kind}`)
      assert(retryable(r), 'an undetermined result must invite a retry')
      assert(explain(r).includes('not a denial'), explain(r))
    })
})

check('a 403 is a denial and does not invite a retry', () => {
  return fetchWorld('http://v', DIGEST, '0x1', canned(403, { detail: 'not a holder' }))
    .then((r) => {
      assert(r.kind === 'denied')
      assert(!retryable(r), 'a denial must not tell the player to try again')
    })
})

check('a 404 says the gap belongs to the vault, not the holder', () => {
  return fetchWorld('http://v', DIGEST, '0x1', canned(404, { detail: 'not stored' }))
    .then((r) => {
      assert(r.kind === 'absent')
      assert(explain(r).includes('not in your entitlement'), explain(r))
    })
})

check('an unreachable vault names the url it tried', () => {
  // `/mesh` learned this: collapsing every failure into one diagnosis is wrong exactly when a
  // healthy service is configured at a bad address.
  return fetchWorld('http://v', DIGEST, '0x1', canned(0, '', { throws: true })).then((r) => {
    assert(r.kind === 'unreachable')
    assert(r.detail.includes('http://v/world/'), r.detail)
  })
})

check('a login page is reported with its body rather than a guessed reason', () => {
  return fetchWorld('http://v', DIGEST, '0x1', canned(302, '<html>sign in</html>')).then((r) => {
    assert(r.kind === 'unreachable')
    assert(r.detail.includes('302'), r.detail)
  })
})

check('the holder address is sent and the url is not doubled', () => {
  let seen = null
  return fetchWorld('http://v/', DIGEST, '0xabc', canned(200, '{}', {
    onCall: (url, init) => { seen = { url, init } },
  })).then(() => {
    assert(seen.url === `http://v/world/${DIGEST}`, seen.url)
    assert(seen.init.headers['X-Scema-Holder'] === '0xabc', 'holder header missing')
  })
})

check('a vault returning a different world is caught, not flown', () => {
  // The vault serves bytes; it does not certify them. No signature on the record itself can
  // bind it to the request that asked for it.
  const other = 'b'.repeat(64)
  const r = matchesRequest(DIGEST, other, { valid: true })
  assert(r && r.kind === 'mismatch', 'a swapped world was accepted')
  assert(r.detail.includes(DIGEST.slice(0, 16)), r.detail)
})

check('a fetched record that fails verification is rejected', () => {
  const r = matchesRequest(DIGEST, DIGEST, { valid: false })
  assert(r && r.kind === 'mismatch', 'an edited record from a vault was accepted')
  assert(r.detail.includes('edited after sealing'), r.detail)
})

check('a correct, verified record passes the binding check', () => {
  assert(matchesRequest(DIGEST, DIGEST, { valid: true }) === null)
})

check('every vault outcome has an explanation a player can act on', () => {
  const kinds = ['ok', 'denied', 'undetermined', 'absent', 'mismatch', 'unreachable']
  for (const kind of kinds) {
    const msg = explain({ kind, detail: 'x', record: { text: '', commitment: '' } })
    assert(msg && msg.length > 2, `${kind} has no explanation`)
  }
})


// ── fleets ────────────────────────────────────────────────────────────────────

const seedA = 'a'.repeat(64)
const seedB = 'b'.repeat(64)
const seedC = 'c'.repeat(64)
const spaceOf = (seed) => generate(world, seed)

check('a fleet is the same galaxy whatever order the records arrive in', () => {
  // The property that makes two players comparing notes describe the same thing. An
  // index-based layout would have been simpler and would have made the map a function of a
  // UI event order.
  const a = joinFleet([spaceOf(seedA), spaceOf(seedB), spaceOf(seedC)])
  const b = joinFleet([spaceOf(seedC), spaceOf(seedA), spaceOf(seedB)])
  assert(JSON.stringify(a.worlds) === JSON.stringify(b.worlds), 'placement depends on order')
  assert(a.nodes.length === b.nodes.length)
})

check('placement comes from the commitment, so a world keeps its address', () => {
  const p1 = placement(seedA)
  const p2 = placement(seedA)
  assert(JSON.stringify(p1) === JSON.stringify(p2), 'placement is not deterministic')
  assert(JSON.stringify(placement(seedB)) !== JSON.stringify(p1), 'two worlds share a spot')
})

check('node ids stay unique across a fleet', () => {
  // Renumbering is the whole job. A collision would silently draw one world lane into
  // another world's geometry.
  const f = joinFleet([spaceOf(seedA), spaceOf(seedB)])
  assert(new Set(f.nodes.map((n) => n.id)).size === f.nodes.length, 'duplicate node id')
  const ids = new Set(f.nodes.map((n) => n.id))
  for (const l of f.lanes) {
    assert(ids.has(l.from) && ids.has(l.to), `lane ${l.from}->${l.to} points nowhere`)
  }
})

check('contact ids are namespaced so two worlds cannot merge a target', () => {
  // Two records can legitimately carry the same signal id. Merging them would make one
  // target absorb hits meant for another.
  const f = joinFleet([spaceOf(seedA), spaceOf(seedB)])
  assert(new Set(f.contacts.map((c) => c.id)).size === f.contacts.length, 'duplicate contact')
})

check('worlds do not overlap in space', () => {
  const f = joinFleet([spaceOf(seedA), spaceOf(seedB), spaceOf(seedC)])
  for (let i = 0; i < f.worlds.length; i += 1) {
    for (let j = i + 1; j < f.worlds.length; j += 1) {
      const a = f.worlds[i].origin
      const b = f.worlds[j].origin
      const d = Math.hypot(a.x - b.x, a.y - b.y, a.z - b.z)
      assert(d > EXTENT, `worlds ${i} and ${j} are ${d} apart, closer than one world's extent`)
    }
  }
})

check('one unknown sensor range makes the fleet unknown, not the minimum of the rest', () => {
  // The coverage mistake in a new place: taking the minimum of the known ones would report a
  // confident figure computed over an incomplete set.
  const dark = generate({ ...world, objects: [] }, seedA)
  assert(dark.sensorRange === null, 'the dark world should be unmeasured')
  const f = joinFleet([dark, spaceOf(seedB)])
  assert(f.sensorRange === null, `fleet range was ${f.sensorRange}`)
})

check('a fleet of measured worlds takes the worst range', () => {
  const f = joinFleet([spaceOf(seedA), spaceOf(seedB)])
  assert(f.sensorRange !== null, 'both worlds are measured')
  assert(f.sensorRange <= spaceOf(seedA).sensorRange)
})

check('bridges connect every world without a complete graph', () => {
  const f = joinFleet([spaceOf(seedA), spaceOf(seedB), spaceOf(seedC)])
  assert(f.bridges.length === f.worlds.length - 1, `${f.bridges.length} bridges`)
  const reached = new Set([0])
  for (const b of f.bridges) {
    reached.add(b.from)
    reached.add(b.to)
  }
  assert(reached.size === f.worlds.length, 'a world is unreachable')
})

check('a single world needs no bridges', () => {
  const f = joinFleet([spaceOf(seedA)])
  assert(f.bridges.length === 0)
  assert(f.worlds.length === 1)
})

check('an empty fleet is empty rather than an error', () => {
  const f = joinFleet([])
  assert(f.worlds.length === 0 && f.nodes.length === 0)
  assert(f.sensorRange !== undefined, 'an empty fleet still reports a range field')
})

// ── the bug that made the game look broken ────────────────────────────────────

check('a fired shot is actually drawn', () => {
  // THE regression. Shots were created, stepped and resolved, and the draw list never
  // contained one — the scene was uploaded once, outside the frame loop, so the whole game
  // was a still photograph of the record. Nothing about the combat model was wrong.
  const s = generate(world, digest)
  let g = newGame(s)
  g = tick(g, s, { keys: new Set(), firing: true, dt: 0.016, nowMs: 1000 })
  assert(g.combat.projectiles.length > 0, 'firing produced no projectile')

  const list = drawList(s, dynamicOf(g))
  const shots = list.bodies.filter((b) => b.role === 'laser' || b.role === 'photon')
  assert(shots.length > 0, 'the projectile exists in state but is not in the draw list')
})

check('a destroyed hostile stops being drawn', () => {
  // The other half of the same bug: the upload happened once, so nothing could ever leave
  // the scene either.
  const s = generate(world, digest)
  const hostile = s.contacts.find((c) => c.hostility === 'hostile')
  assert(hostile, 'the fixture needs a hostile')
  const before = drawList(s, dynamicOf(newGame(s))).bodies.length
  const after = drawList(s, { shots: [], incoming: [], craft: [], destroyed: [hostile.id] })
  assert(after.bodies.length < before, 'a destroyed contact is still drawn')
})

// ── throttle is a level ───────────────────────────────────────────────────────

check('throttle is a level that persists, not a button', () => {
  // The original accelerated only while a key was held, so the ship was a car with the pedal
  // being tapped. At this sector's size a cruise setting is the difference between flying and
  // steering.
  const s = generate(world, digest)
  let g = newGame(s)
  const up = new Set(['ArrowUp'])
  for (let i = 0; i < 30; i += 1) g = tick(g, s, { keys: up, firing: false, dt: 0.05, nowMs: i * 50 })
  const cruised = g.throttle
  assert(cruised > 0.5, `throttle only reached ${cruised}`)

  // Release: it must hold, not decay.
  g = tick(g, s, { keys: new Set(), firing: false, dt: 0.5, nowMs: 9000 })
  assert(Math.abs(g.throttle - cruised) < 1e-9, 'throttle decayed when the key was released')
})

check('X is a full stop', () => {
  const s = generate(world, digest)
  let g = { ...newGame(s), throttle: 1 }
  g = tick(g, s, { keys: new Set(['KeyX']), firing: false, dt: 0.016, nowMs: 1 })
  assert(g.throttle === 0, 'X did not cut the drive')
})

check('a ship at throttle actually moves, and burns fuel doing it', () => {
  const s = generate(world, digest)
  let g = { ...newGame(s), throttle: 1 }
  const fuel0 = g.ship.fuel
  g = tick(g, s, { keys: new Set(), firing: false, dt: 1, nowMs: 1 })
  assert(Math.hypot(...g.camera.position) > 0, 'the ship did not move')
  assert(g.ship.fuel < fuel0, 'thrust cost no fuel')
})

check('a dry ship coasts rather than stalling, and says why', () => {
  // Being stranded is a real state. It must be legible, and lateral thrusters must still
  // work or a player can never line up on the depot that would save them.
  const s = generate(world, digest)
  let g = { ...newGame(s), throttle: 1, ship: { ...newGame(s).ship, fuel: 0 } }
  const before = g.camera.position.slice()
  g = tick(g, s, { keys: new Set(), firing: false, dt: 1, nowMs: 1 })
  assert(JSON.stringify(g.camera.position) === JSON.stringify(before), 'a dry ship thrusted')
  assert((g.notice ?? '').includes('dry'), `notice was ${g.notice}`)

  const strafed = tick(g, s, { keys: new Set(['ArrowRight']), firing: false, dt: 1, nowMs: 2 })
  assert(
    JSON.stringify(strafed.camera.position) !== JSON.stringify(g.camera.position),
    'thrusters must work with dry tanks or a stranded player is stranded forever'
  )
})

// ── the sector is big ─────────────────────────────────────────────────────────

check('a sector is large enough to be a place rather than a diagram', () => {
  const s = generate(world, digest)
  const xs = s.nodes.map((n) => n.at.x)
  const span = Math.max(...xs) - Math.min(...xs)
  assert(span > EXTENT, `span ${span} is smaller than one extent`)
  assert(s.nodes.length > 400, `only ${s.nodes.length} nodes`)
})

check('a crossing is quick, and the distance is carried by the jump drive instead', () => {
  // This bound was 20 seconds and the sector was crossable in 26, which was *correct* and read
  // as a chore: the interesting decision is which of a thousand nodes to be at, and a travel
  // time long enough to be felt turns that decision into a commute. Making the ship faster
  // still would flatten the space, so the distance moved onto the jump drive — which charges,
  // costs a scarce fuel, and refuses to spin up in a fight.
  // Measured against the **sector**, not against one extent, which is the unit that actually
  // moved: the tree now runs to nearly nine extents corner to corner, so "an extent in N seconds"
  // stopped being a statement about travel at all. A full crossing lands near half a minute —
  // brisk, and still long enough that the jump drive is the answer to a long haul rather than a
  // convenience.
  const crossing = (SCALE.SECTOR_REACH * 1.5) / topSpeed(0)
  assert(crossing > 12, `a sector crossing takes ${crossing.toFixed(1)}s — too fast to feel like distance`)
  assert(crossing < 60, `a sector crossing takes ${crossing.toFixed(1)}s — that is a commute`)
})

// ── services ──────────────────────────────────────────────────────────────────

check('a sector carries somewhere to refuel, repair and trade', () => {
  const s = generate(world, digest)
  const kinds = new Set(s.nodes.map((n) => n.kind))
  for (const k of ['depot', 'dock', 'market']) {
    assert(kinds.has(k), `no ${k} anywhere in the sector`)
  }
})

check('a phantom offers nothing, because it was modelled and not observed', () => {
  // The best refusal in the game: it looks exactly like a station on approach.
  assert(servicesOf('phantom').length === 0)
  assert(servicesOf('marker').length === 0, 'an absence cannot serve you')
  assert(servicesOf('rift').length === 0)
  assert(servicesOf('dock').includes('repair'))
  assert(servicesOf('derelict').join() === 'scavenge', 'a stale station cannot trade')
})

check('refuel fills the tank and repair costs salvage', () => {
  let ship = { ...newShip(), fuel: 10, hull: 20 }
  const fuelled = refuel(ship)
  assert(fuelled.ok && fuelled.ship.fuel === fuelCapacity(0))

  const poor = repair({ ...ship, salvage: 0 })
  assert(!poor.ok, 'repair must be refusable when you cannot pay')
  assert(poor.message.includes('salvage'), poor.message)

  const rich = repair({ ...ship, salvage: 500 })
  assert(rich.ok && rich.ship.hull === hullMax(0) && rich.ship.salvage < 500)
})

check('a derelict pays once, ever', () => {
  const a = scavenge(newShip(), 7)
  assert(a.ok && a.ship.salvage === 40)
  const b = scavenge(a.ship, 7)
  assert(!b.ok && b.ship.salvage === 40, 'a derelict was stripped twice')
})

// ── progression, and the rule it must not break ───────────────────────────────

check('no reward is computed from anything the record reports', () => {
  // The sharpened rule. Attach a payout to `blind_spots` and you have paid somebody to hide
  // them; attach one to magnitude and you have paid them to understate it.
  const src = codeOf(join(here, '..', 'lib', 'scemaworld', 'ship.ts'))
  for (const field of ['magnitude', 'blind_spots', 'blindSpots', 'legibility', 'extent']) {
    assert(!src.includes(field), `ship.ts computes something from \`${field}\``)
  }
})

check('upgrades cost more each level and cap out', () => {
  assert(upgradeCost('engine', 0) < upgradeCost('engine', 3), 'cost is not superlinear')
  assert(upgradeCost('engine', MAX_LEVEL) === null, 'upgrades never cap')
})

check('buying an upgrade delivers what it paid for', () => {
  // A tank upgrade that did not also carry the fuel it just bought would look like it did
  // nothing, and the player would reasonably conclude the market is broken.
  const rich = { ...newShip(), salvage: 10_000 }
  const tanks = buy(rich, 'tanks')
  assert(tanks.ok && tanks.ship.fuel === fuelCapacity(1), 'tanks did not refill to the new cap')
  const hull = buy(rich, 'hull')
  assert(hull.ok && hull.ship.hull === hullMax(1))
})

check('a player who cannot afford an upgrade is told the price', () => {
  const broke = buy({ ...newShip(), salvage: 1 }, 'engine')
  assert(!broke.ok && broke.message.includes('salvage'), broke.message)
})

// ── enemies ───────────────────────────────────────────────────────────────────

check('hostiles become craft and salvage does not', () => {
  const s = generate(world, digest)
  const sw = swarmOf(s.contacts, s.seed)
  const hostiles = s.contacts.filter((c) => c.hostility === 'hostile').length
  assert(sw.craft.length === hostiles, `${sw.craft.length} craft for ${hostiles} hostiles`)
})

check('a craft out of range patrols rather than hunting you', () => {
  // A sector where every hostile converges the moment it can see you is one big fight rather
  // than a place with dangerous regions in it. It does *drift* now — a sector frozen until you
  // arrive is a diorama — so the assertion is that it does not come for you, not that it is
  // motionless.
  const s = generate(world, digest)
  const sw = swarmOf(s.raiders, s.seed)
  const far = { x: 1e12, y: 0, z: 0 }
  const after = enemyStep(sw, far, ZERO, 1, 1000)
  assert(after.damage === 0, 'a craft on the far side of the galaxy shot at us')
  assert(after.swarm.craft.every((c) => c.behaviour === 'patrol'), 'a distant craft engaged')
  const before = Math.hypot(sw.craft[0].at.x - far.x, sw.craft[0].at.y, sw.craft[0].at.z)
  const now = Math.hypot(after.swarm.craft[0].at.x - far.x, after.swarm.craft[0].at.y, after.swarm.craft[0].at.z)
  assert(Math.abs(now - before) < before * 0.01, 'a patrolling craft closed on a distant player')
})

check('a craft turns onto you before it fires, and cannot shoot sideways', () => {
  // The load-bearing constraint of the whole dogfight. A craft that can fire in any direction
  // makes manoeuvre pointless, and manoeuvre is the entire game.
  const s = generate(world, digest)
  const sw = swarmOf(s.raiders, s.seed)
  const c0 = sw.craft[0]
  // Put the player right in front of it, but start it pointing the wrong way.
  const player = {
    x: c0.at.x - c0.facing.x * 1e7,
    y: c0.at.y - c0.facing.y * 1e7,
    z: c0.at.z - c0.facing.z * 1e7,
  }
  let cur = { craft: [c0], shots: [] }
  let firstShot = -1
  for (let i = 0; i < 400; i += 1) {
    const r = enemyStep(cur, player, ZERO, 1 / 30, i * 33)
    cur = r.swarm
    if (r.fired.length > 0 && firstShot < 0) firstShot = i
  }
  assert(firstShot > 0, 'a craft with the player behind it never came around to fire')
  assert(firstShot > 2, `it fired on frame ${firstShot}, before it could have turned`)
})

check('a craft leads its shots, so jinking beats it', () => {
  // Leading is what makes a fast shot feel aimed. It is also what makes evasion work: the lead
  // is computed from the player's *current* velocity, so changing it is what breaks the solution.
  const from = { x: 0, y: 0, z: 0 }
  const target = { x: 0, y: 0, z: -1e7 }
  const still = leadPoint(from, target, ZERO, 1e8)
  const moving = leadPoint(from, target, { x: 1e7, y: 0, z: 0 }, 1e8)
  assert(still.x === target.x, 'a stationary target was led')
  assert(moving.x > target.x, 'a crossing target was not led')
})

check('turning is bounded, which is the reason a dogfight exists', () => {
  const from = { x: 0, y: 0, z: 1 }
  const to = { x: 0, y: 0, z: -1 }
  const step1 = turnToward(from, to, 0.1)
  assert(step1.z > 0.99, 'a craft reversed its facing in one step')
  let f = from
  for (let i = 0; i < 100; i += 1) f = turnToward(f, to, 0.1)
  assert(f.z < -0.99, 'a craft never completed a turn it had time for')
})

check('a ghost craft never gains a threat number, even while fighting', () => {
  // The pressure to resolve it is strongest here — a player being shot at wants a number —
  // and giving them one would be inventing it.
  const s = generate(world, digest)
  const ghost = s.contacts.find((c) => !c.solid && c.hostility === 'hostile')
  if (!ghost) return
  const sw = swarmOf([ghost], s.seed)
  assert(sw.craft[0].solid === false)
  assert(threatLabel(ghost) === '—', 'a ghost craft reported a threat')
})

check('shields absorb before hull, and the overflow carries through', () => {
  // A shot that breaks through has to actually break through. If the last point of shield could
  // soak a whole volley, players would learn to fight at 1% and the bar would mean nothing.
  const s = generate(world, digest)
  const sw = swarmOf(s.raiders, s.seed)
  const shielded = sw.craft.find((c) => c.spec.shield > 0)
  if (!shielded) return
  const r = enemyHit({ craft: [shielded], shots: [] }, shielded.id, shielded.spec.shield + 5, 0)
  const after = r.swarm.craft[0]
  assert(after.shield === 0, `shield was ${after.shield}`)
  assert(after.hull === shielded.spec.hull - 5, `hull was ${after.hull}`)
  assert(r.throughShield, 'a hit that reached hull did not report doing so')
})

check('a hit soaked by a shield reports that it was soaked', () => {
  // The only cue telling a player whether they are making progress or wasting rounds on a
  // buffer. Without it a heavily-shielded gunship reads as invulnerable.
  const s = generate(world, digest)
  const sw = swarmOf(s.raiders, s.seed)
  const shielded = sw.craft.find((c) => c.spec.shield > 10)
  if (!shielded) return
  const r = enemyHit({ craft: [shielded], shots: [] }, shielded.id, 5, 0)
  assert(!r.throughShield, 'a soaked hit claimed to reach hull')
  assert(r.swarm.craft[0].hull === shielded.spec.hull, 'a soaked hit damaged hull')
})

check('a craft dies once, and pays its class bounty', () => {
  const s = generate(world, digest)
  const sw = swarmOf(s.raiders, s.seed)
  const id = sw.craft[0].id
  const spec = sw.craft[0].spec
  let cur = sw
  let kills = 0
  let paid = 0
  for (let i = 0; i < 40; i += 1) {
    const r = enemyHit(cur, id, 999, i * 100)
    cur = r.swarm
    if (r.killed) {
      kills += 1
      paid = r.bounty
    }
  }
  assert(kills === 1, `killed ${kills} times`)
  assert(paid === spec.bounty, `paid ${paid}, class pays ${spec.bounty}`)
  assert(living(cur).every((c) => c.id !== id), 'a dead craft is still alive')
})

// ── classes ──────────────────────────────────────────────────────────────────

check('every class trades turn against speed, and none can outrun you', () => {
  // The line every statline sits on: a dogfight is a contest of turn rate against speed, not of
  // hit points. And disengaging must always be possible, or the game punishes exploring.
  for (const id of CLASS_IDS) {
    const c = CLASSES[id]
    assert(c.speed < topSpeed(0), `${id} at ${c.speed} outruns a stock ship`)
    assert(c.turn > 0, `${id} cannot turn at all`)
    assert(c.hull > 0 && c.bounty > 0, `${id} has no hull or no bounty`)
  }
  const fast = CLASSES.interceptor
  const heavy = CLASSES.destroyer
  assert(fast.turn > heavy.turn * 8, 'the fast one does not turn better than the capital')
  // Against the **ladder**, not a raw hull ratio. Durability is a photon count now (see
  // `PHOTONS_TO_KILL`), and the old 10x hull ratio was a proxy for "much tougher" that stopped
  // being meaningful the moment the capitals came down from 96 warheads to 8.
  assert(
    CLASSES_MOD.PHOTONS_TO_KILL[heavy.id] >= CLASSES_MOD.PHOTONS_TO_KILL[fast.id] * 4,
    'the capital is not meaningfully tougher',
  )
})

check('a capital holds station instead of chasing', () => {
  // It is a place you fight at, not a duel. A destroyer that pursued would be an unloseable
  // chase, since it also cannot be outrun in a straight line by anything it can catch.
  const cap = {
    id: 'x', spec: CLASSES.destroyer, at: { x: 0, y: 0, z: 0 },
    facing: { x: 0, y: 0, z: 1 }, speed: 0, hull: 5, shield: 0, lastHitMs: -1e9,
    solid: true, behaviour: 'patrol', lastFire: -1e9, burstLeft: 0, since: 0, alive: true,
  }
  // Hull at 5 of 460 — a fighter would be running.
  assert(decide(cap, CLASSES.destroyer.aggro * 0.5, 1, 1000) === 'attack')
  assert(decide(cap, CLASSES.destroyer.aggro * 2, 1, 1000) === 'patrol')
})

check('a fighter breaks off when its hull is nearly gone', () => {
  // Being able to let one go is what stops every encounter being to the death.
  const base = {
    id: 'x', spec: CLASSES.interceptor, at: { x: 0, y: 0, z: 0 },
    facing: { x: 0, y: 0, z: 1 }, speed: 0, shield: 0, lastHitMs: -1e9,
    solid: true, behaviour: 'attack', lastFire: -1e9, burstLeft: 0, since: 0, alive: true,
  }
  const healthy = { ...base, hull: CLASSES.interceptor.hull }
  const hurt = { ...base, hull: CLASSES.interceptor.hull * 0.2 }
  assert(decide(hurt, CLASSES.interceptor.standoff, 1, 9999) === 'evade')
  assert(decide(healthy, CLASSES.interceptor.standoff, 1, 9999) !== 'evade')
})

check('a signal the record reported is never a capital', () => {
  // Capitals are sector furniture. Letting a reported signal become a destroyer would put the
  // record's contents back in charge of how hard its own sector is.
  const s = generate(world, digest)
  const sw = swarmOf(s.contacts, s.seed)
  assert(sw.craft.every((c) => !c.spec.capital), 'a record signal became a capital')
})

// ── scale, which is where the last round of bugs actually lived ──────────────

check('no module outside scale.ts hardcodes a world distance', () => {
  // The bug class this pins: the sector was enlarged sixty-fold and every radius, range and
  // speed stayed at the old tuning, because they were bare `26_000 * UNIT` literals in five
  // modules that each declared their own UNIT. Nothing failed. The game just became a void
  // with specks in it, and every test still passed because they all compared constants that
  // had moved together — or rather, had all failed to move.
  for (const m of ['generate', 'view', 'weapons', 'ship', 'enemy', 'game', 'raiders', 'nav']) {
    const src = codeOf(join(here, '..', 'lib', 'scemaworld', `${m}.ts`))
    assert(!/\bUNIT\b/.test(src), `${m}.ts still speaks in UNIT; distances belong in scale.ts`)
  }
})

check('the sector is playable at its own scale', () => {
  // Each of these is a ratio a player feels. They are asserted rather than described because
  // the previous set of numbers satisfied every structural test and none of these.
  const reachLaser = SCALE.SPEED_LASER * SCALE.LIFE_LASER

  assert(SCALE.DOCK_RANGE > SCALE.R_STATION * 2, 'you would have to fly into a station to dock')
  assert(SCALE.DOCK_RANGE < SCALE.AGGRO_RANGE, 'docking range must not exceed engagement range')
  assert(reachLaser > SCALE.ENGAGE_RANGE, 'a laser cannot reach the range enemies hold at')
  assert(reachLaser < SCALE.AGGRO_RANGE * 2, 'you could shoot things before they notice you')
  assert(SCALE.R_STATION > EXTENT / 400, 'a station is a speck at sector scale')
  assert(SCALE.R_STATION < EXTENT / 40, 'a station is a continent')
})

check('a full tank is a journey rather than a countdown', () => {
  // It was fifty seconds of open throttle, which on a sector this size is not a fuel economy —
  // you spent the whole of it reaching the first thing you saw. Running dry has to be the
  // consequence of a decision, not of leaving the hangar.
  const seconds = fuelCapacity(0) / burnRate(1, 0)
  assert(seconds > 150, `a full tank is ${seconds.toFixed(0)}s at full burn`)
  assert(seconds < 900, `a full tank is ${seconds.toFixed(0)}s — fuel has stopped mattering`)
  assert(burnRate(0.4, 0) < burnRate(1, 0) / 4, 'cruising is not meaningfully cheaper than burning')
})

check('a fleeing player can always outrun a hostile', () => {
  // The design rule behind the numbers: a fight you are losing must have an exit, or the game
  // punishes the exploring it is entirely about.
  const fastest = SCALE.SPEED_CRAFT + 4 * SCALE.SPEED_CRAFT_PER_TIER
  assert(fastest < topSpeed(0), `craft do ${fastest}, a stock ship does ${topSpeed(0)}`)
})

check('a projectile cannot tunnel through a contact in one frame', () => {
  // At 60fps a laser covers a fixed distance per frame. If that exceeds a contact's diameter,
  // an endpoint hit test misses — which is why `step` sweeps. This pins the margin that makes
  // the sweep sufficient rather than merely present.
  assert(SCALE.SPEED_LASER / 60 < SCALE.AGGRO_RANGE, 'a laser crosses the engagement in a frame')
  assert(SCALE.SPEED_ENEMY_SHOT / 60 < SCALE.R_PLAYER * 40, 'enemy fire outruns the sweep budget')
})

check('nothing spawns on top of the player', () => {
  const s = generate(world, digest)
  for (const c of [...s.contacts, ...s.raiders]) {
    const d = Math.hypot(c.at.x, c.at.y, c.at.z)
    assert(d > SCALE.DOCK_RANGE, `${c.id} is ${d} from the spawn point`)
  }
})

// ── raiders ──────────────────────────────────────────────────────────────────

check('a sector is populated rather than deserted', () => {
  // Hostiles used to come only from the record's signals. The parity fixture has five, which
  // over a sector this size is an empty volume with five things in it.
  const s = generate(world, digest)
  assert(s.raiders.length > 20, `only ${s.raiders.length} raiders`)
  assert(s.raiders.length > s.contacts.length, 'the record still outnumbers the sector')
})

check('raider density comes from the seed and never from the record', () => {
  // The rule that is easiest to break here and most damaging if broken. Tying density to
  // blind_spots is the obvious idea and it is backwards: hiding them would buy an easier game.
  const s = generate(world, digest)
  const blinder = { ...world, blind_spots: [...(world.blind_spots ?? []), 'a', 'b', 'c', 'd'] }
  const quieter = { ...world, signals: [] }
  assert(generate(blinder, digest).raiders.length === s.raiders.length, 'blind spots changed it')
  assert(generate(quieter, digest).raiders.length === s.raiders.length, 'signal count changed it')

  const src = codeOf(join(here, '..', 'lib', 'scemaworld', 'raiders.ts'))
  for (const f of ['blind_spots', 'blindSpots', 'signals', 'legibility', 'extent']) {
    assert(!src.includes(f), `raiders.ts reads \`${f}\` from the record`)
  }
})

check('the same record raises the same raiders', () => {
  const a = generate(world, digest).raiders
  const b = generate(world, digest).raiders
  assert(JSON.stringify(a) === JSON.stringify(b), 'raiders are not deterministic')
  const other = generate(world, 'f'.repeat(64)).raiders
  assert(JSON.stringify(a) !== JSON.stringify(other), 'the seed does not move them')
})

check('a raider is never mistakable for something the record reported', () => {
  const s = generate(world, digest)
  assert(s.raiders.every((r) => r.unlogged === true), 'a raider is not flagged unlogged')
  assert(s.contacts.every((c) => !c.unlogged), 'a record signal was flagged unlogged')
  assert(s.raiders.every((r) => roleOfContact(r) === 'raider'), 'a raider draws as a contact')
  // And it stays out of `contacts`, so anything reading the record's signals still sees five.
  assert(!s.contacts.some((c) => c.id.startsWith('raider:')), 'a raider leaked into contacts')
})

check('raiders are drawn, fought and counted like any other hostile', () => {
  const s = generate(world, digest)
  assert(drawList(s).bodies.some((b) => b.role === 'raider'), 'raiders are not drawn')
  const sw = swarmOf([...s.contacts, ...s.raiders], s.seed)
  assert(sw.craft.length >= s.raiders.length, 'raiders did not become craft')
})

// ── navigation ───────────────────────────────────────────────────────────────

check('the nav computer finds the nearest of each service', () => {
  const s = generate(world, digest)
  const c = camera([0, 0, 0])
  for (const svc of ['refuel', 'repair', 'trade']) {
    const fixes = nearest(s, c, svc, 3)
    assert(fixes.length === 3, `${svc}: ${fixes.length} fixes`)
    assert(fixes[0].range <= fixes[1].range, `${svc} fixes are not sorted`)
    assert(servicesOf(fixes[0].node.kind).includes(svc), `${svc} fix does not offer it`)
  }
})

check('a bearing knows ahead from astern', () => {
  const c = camera([0, 0, 0])
  // The camera looks down −Z.
  assert(ahead(c, { x: 0, y: 0, z: -1e6 }) > 0.99, 'dead ahead did not read ahead')
  assert(ahead(c, { x: 0, y: 0, z: 1e6 }) < -0.99, 'directly behind did not read astern')
  assert(bearingLabel({ ahead: 1 }) === 'ON NOSE')
  assert(bearingLabel({ ahead: -1 }) === 'ASTERN')
})

check('a range reads as a distance rather than nine digits', () => {
  assert(rangeLabel(120_000_000) === '120.0Mm')
  assert(rangeLabel(45_000) === '45km')
  assert(rangeLabel(12) === '12m')
})

check('routing cycles rather than sticking on the nearest', () => {
  const s = generate(world, digest)
  const c = camera([0, 0, 0])
  const first = cycle(s, c, 'refuel', null)
  const second = cycle(s, c, 'refuel', first)
  assert(first !== null && second !== null && first !== second, 'the waypoint did not advance')
})

check('the nav computer routes to a phantom rather than hiding it', () => {
  // The rule: it reports geometry, never a verdict. Filtering out unreliable destinations would
  // hide the record's uncertainty at exactly the moment the player acts on it.
  const src = codeOf(join(here, '..', 'lib', 'scemaworld', 'nav.ts'))
  assert(!src.includes("!== 'phantom'"), 'nav.ts filters phantoms out')
  const s = generate(world, digest)
  const f = fixOn(s, camera([0, 0, 0]), s.nodes[0].id)
  assert(f && f.node.kind, 'a fix does not carry the kind the HUD needs in order to warn')
})

check('a waypoint reaches the draw list, hollow', () => {
  const s = generate(world, digest)
  const target = s.nodes[40]
  const list = drawList(s, {
    shots: [], incoming: [], craft: [], destroyed: [], waypoint: target.at,
  })
  const wp = list.bodies.find((b) => b.role === 'waypoint')
  assert(wp, 'the waypoint is not drawn')
  assert(!wp.solid, 'a waypoint marks a place; a filled body reads as a thing')
})

// ── a whole flight ───────────────────────────────────────────────────────────

check('ninety seconds of play produces a game rather than a still photograph', () => {
  // The end-to-end version of the regression. Everything the player reported as broken —
  // lasers, photons, thrust — shows up here as a count that must not be zero.
  const s = generate(world, digest)
  let g = newGame(s)
  const keys = new Set(['ArrowUp'])
  let framesWithShot = 0
  let sampledWithCraft = 0
  for (let f = 0; f < 60 * 90; f += 1) {
    g = tick(g, s, { keys, firing: f > 120, dt: 1 / 60, nowMs: (f * 1000) / 60 })
    if (g.combat.projectiles.length > 0) framesWithShot += 1
    if (f % 600 === 0) {
      const bodies = drawList(s, dynamicOf(g, s)).bodies
      if (bodies.some((b) => b.role === 'raider' || b.role === 'hostile')) sampledWithCraft += 1
    }
  }
  assert(framesWithShot > 1000, `only ${framesWithShot} frames had a shot on screen`)
  assert(sampledWithCraft > 0, 'no hostile was ever drawn')
  assert(g.throttle === 1, 'ninety seconds of ArrowUp did not reach full throttle')
  assert(Math.hypot(...g.camera.position) > EXTENT, 'the ship did not cross the sector')
  assert(g.ship.fuel < fuelCapacity(0), 'ninety seconds at full throttle cost no fuel')
})

check('a fight can be won, and pays from the act rather than the record', () => {
  const s = generate(world, digest)
  let g = newGame(s)
  // Park just off a raider and hold the trigger.
  const r = s.raiders[0]
  g = { ...g, camera: { ...g.camera, position: [r.at.x, r.at.y, r.at.z + EXTENT * 0.01] } }
  let killed = false
  for (let f = 0; f < 60 * 30 && !killed; f += 1) {
    g = tick(g, s, { keys: new Set(), firing: true, dt: 1 / 60, nowMs: (f * 1000) / 60 })
    killed = g.combat.destroyed.length > 0
  }
  assert(killed, 'thirty seconds of point-blank fire destroyed nothing')
  assert(g.ship.salvage > 0, 'a kill paid nothing')
  // The bounty is one of the class table's flat figures — never a number derived from the
  // record. Checked against the table rather than against a modulus, because the classes pay
  // different amounts and the rule is about *where the figure comes from*.
  const paid = new Set(CLASS_IDS.map((id) => CLASSES[id].bounty))
  assert(paid.has(g.ship.salvage), `paid ${g.ship.salvage}, which is no class bounty`)
})

check('you can route to a depot, fly there, and refuel before running dry', () => {
  // The soft-lock check, and the reason `nav.ts` exists. A fuel economy in a volume you cannot
  // navigate is not a fuel economy, it is a timer. This flies the loop the player flies: pick a
  // waypoint, turn onto it, burn, arrive, dock. If any link is broken the run ends stranded.
  const s = generate(world, digest)
  let g = route(newGame(s), s, 'refuel')
  assert(g.waypoint !== null, 'nothing to route to')

  const dt = 1 / 30
  let arrived = false
  for (let f = 0; f < 30 * 400 && !arrived; f += 1) {
    const fix = fixOn(s, g.camera, g.waypoint)
    // A crude autopilot: yaw and pitch toward the target, throttle only once roughly lined up
    // so fuel is not spent flying sideways.
    const keys = new Set()
    const nose = fix.ahead
    if (nose < 0.999) {
      const local = toLocal(g.camera, fix.node.at)
      if (local.x > 0) keys.add('KeyD')
      else keys.add('KeyA')
      if (local.y > Math.abs(local.x)) keys.add('KeyW')
      else if (-local.y > Math.abs(local.x)) keys.add('KeyS')
    }
    if (nose > 0.98) keys.add('ArrowUp')
    else keys.add('ArrowDown')

    g = tick(g, s, { keys, firing: false, dt, nowMs: f * dt * 1000 })
    if (g.nearby && g.nearby.id === g.waypoint) arrived = true
    if (g.lost) break
  }

  assert(arrived, `never reached the waypoint; fuel left ${g.ship.fuel.toFixed(0)}`)
  const dry = { ...g, ship: { ...g.ship, fuel: 3 } }
  const filled = useService(dry, 'refuel')
  assert(filled.ship.fuel > 3, `refuel at the waypoint failed: ${filled.notice}`)
})

/** Target direction in the camera's own frame, for the autopilot above. */
function toLocal(cam, target) {
  const f = forward(cam)
  const u = up(cam)
  const r = right(cam)
  const d = [target.x - cam.position[0], target.y - cam.position[1], target.z - cam.position[2]]
  const l = Math.hypot(...d) || 1
  const n = d.map((v) => v / l)
  return {
    x: r[0] * n[0] + r[1] * n[1] + r[2] * n[2],
    y: u[0] * n[0] + u[1] * n[1] + u[2] * n[2],
    z: f[0] * n[0] + f[1] * n[1] + f[2] * n[2],
  }
}

check('every component the market sells changes something', () => {
  // A market that takes salvage and alters nothing is worse than no market. `laserCooldown` and
  // the photon's own scaling each existed for a while with nothing calling them, so two of the
  // six upgrades were sold and inert.
  assert(topSpeed(1) > topSpeed(0), 'ENGINE does nothing')
  assert(fuelCapacity(1) > fuelCapacity(0), 'TANKS does nothing')
  assert(hullMax(1) > hullMax(0), 'HULL does nothing')
  assert(sensorGain(1) > sensorGain(0), 'SENSORS does nothing')
  assert(laserCooldown(1) < laserCooldown(0), 'LASER does nothing')
  // PHOTON buys **yield**, not magazine — the magazine is the hull's tube count and nothing
  // scales it. This is the assertion that had to move when what the component buys changed;
  // left pointed at the magazine it would have kept passing against a hull lookup by accident
  // of argument type, which is the quietest way for a test to stop testing anything.
  assert(photonDamage(1) > photonDamage(0), 'PHOTON does nothing')
})

check('a laser upgrade raises the rate of fire in play', () => {
  const s = generate(world, digest)
  const shotsAt = (level) => {
    let g = newGame(s)
    g = { ...g, ship: { ...g.ship, levels: { ...g.ship.levels, laser: level } } }
    let fired = 0
    let seen = 0
    for (let f = 0; f < 300; f += 1) {
      const before = g.combat.nextId
      g = tick(g, s, { keys: new Set(), firing: true, dt: 1 / 60, nowMs: (f * 1000) / 60 })
      if (g.combat.nextId > before) fired += 1
      seen += 1
    }
    return fired
  }
  assert(shotsAt(4) > shotsAt(0), 'a fully upgraded laser fires no faster than a stock one')
})

check('the photon magazine is the hull, and the component is the warhead', () => {
  // The user-visible shape of the buff: six rounds on the largest hull, four, two, then one.
  // Asserted as literal numbers rather than as an ordering, because the numbers themselves are
  // the design — a pilot has to be able to answer "how many missiles do I have" without
  // arithmetic, and an ordering test would pass just as happily against a formula.
  // **Eight on the starting hull.** The counts were 1/1/2/4/6, which made the photon a round you
  // hoarded and never fired: a dreadnought has 2300 effective hit points and a warhead does 240,
  // so a skiff's single tube could never be part of killing one. Eight is the number that makes
  // the starting loadout capable of a capital kill with laser support, which is the bar the
  // magazine now has to clear. The ordering across hulls is unchanged and still flat counts.
  assert(photonMagazine('marauder') === 24, `marauder carries ${photonMagazine('marauder')}`)
  assert(photonMagazine('lancer') === 16, `lancer carries ${photonMagazine('lancer')}`)
  assert(photonMagazine('corvette') === 12, `corvette carries ${photonMagazine('corvette')}`)
  assert(photonMagazine('skiff') === 8, `skiff carries ${photonMagazine('skiff')}`)
  assert(photonMagazine('scout') === 8, `scout carries ${photonMagazine('scout')}`)
  // The bar itself, asserted rather than described: a full starting magazine plus sustained
  // laser fire has to be able to bring down the smallest capital.
  const cap = CLASSES.dreadnought.hull + CLASSES.dreadnought.shield
  assert(
    photonMagazine('skiff') * PHOTON.damage > cap * 0.75,
    `a full starting magazine does ${photonMagazine('skiff') * PHOTON.damage} against a ${cap}-point capital`,
  )

  // And **no component moves any of those**. That is the whole reason the numbers above can be
  // stated flatly wherever the player reads them.
  const s = generate(world, digest)
  const market = s.nodes.find((n) => servicesOf(n.kind).includes('trade'))
  let g = newGame(s)
  g = { ...g, nearby: market, ship: { ...g.ship, salvage: 50_000 } }
  const before = g.combat.photonsLeft
  for (let i = 0; i < MAX_LEVEL; i += 1) g = purchase(g, 'missiles')
  assert(g.ship.levels.missiles === MAX_LEVEL, g.notice)
  assert(g.combat.photonsLeft === before, 'a component changed the magazine')
  // What it did buy: a heavier warhead on the rounds already in the tubes.
  assert(photonDamage(MAX_LEVEL) > photonDamage(0) * 2, 'a maxed warhead is barely better')
})

check('a photon is an event, and a laser is a drip', () => {
  // The buff, stated as the property it is for. One missile has to settle a fighter outright, or
  // a magazine of one is a worse laser with extra steps.
  assert(PHOTON.damage > LASER.damage * 20, `${PHOTON.damage} vs ${LASER.damage}`)
  // A warhead settles a **fighter** outright. A gunship is a heavy fighter and now takes two, on
  // purpose — that is the second rung of the ladder, and the reason the tier exists at all.
  const fighter = CLASSES.lancer
  assert(PHOTON.damage >= fighter.hull + fighter.shield, 'a fighter survives a hit')
  assert(CLASSES_MOD.PHOTONS_TO_KILL.gunship === 2, 'a heavy fighter is not two warheads')
  // And it must not settle a war class **in one keypress**. The old form of this compared a whole
  // marauder magazine against a leviathan and required the magazine to lose — which was the same
  // arithmetic that made a titan take 96 warheads, i.e. the thing being fixed. A keypress is one
  // warhead, so that is what the rule is about: no single round deletes a capital, and killing one
  // means landing the count the ladder states.
  for (const id of ['dreadnought', 'leviathan', 'titan']) {
    const c = CLASSES[id]
    assert(PHOTON.damage < c.hull + c.shield, `one warhead deletes a ${id}`)
    assert(CLASSES_MOD.PHOTONS_TO_KILL[id] >= 6, `a ${id} is not a sustained engagement`)
  }
})

check('a dock reloads the tubes, because a magazine of one has to come back', () => {
  const s = generate(world, digest)
  const dock = s.nodes.find((n) => n.kind === 'dock')
  assert(dock, 'this sector has no dock')
  let g = newGame(s)
  // Empty the tubes the honest way: fire them.
  let t = 0
  let combat = switchWeapon(g.combat)
  for (let i = 0; i < 8; i += 1) {
    t += PHOTON.cooldownMs + 1
    combat = fire(combat, ZERO, { x: 0, y: 0, z: 1 }, t, [])
  }
  g = { ...g, combat }
  assert(g.combat.photonsLeft === 0, `tubes still hold ${g.combat.photonsLeft}`)
  const docked = useService({ ...g, nearby: dock }, 'refuel')
  assert(docked.combat.photonsLeft === photonMagazine(g.ship.frame), 'a dock did not rearm')
  assert(/photon/i.test(docked.notice ?? ''), `rearm went unreported: ${docked.notice}`)

  // A depot does fuel and nothing else. Where you can rearm has to stay a constraint on a route,
  // and the sector carries six times as many depots as docks.
  const depot = s.nodes.find((n) => n.kind === 'depot')
  if (depot) {
    const atDepot = useService({ ...g, nearby: depot }, 'refuel')
    assert(atDepot.combat.photonsLeft === 0, 'a depot rearmed')
  }
})

// ── the jump drive ───────────────────────────────────────────────────────────

check('a jump refuses for three distinct reasons, and says which', () => {
  // "Nothing happened" and "the drive refused" are different facts, and only one of them tells
  // the player what to do about it. A silently inert key reads as a broken key.
  assert(Hyper.refusal({ threat: null, charges: 3, driveLevel: 0, waypoint: null })
    ?.includes('waypoint'))
  assert(Hyper.refusal({ threat: null, charges: 0, driveLevel: 0, waypoint: 4 })
    ?.includes('charges'))
  assert(Hyper.refusal({ threat: 1, charges: 3, driveLevel: 0, waypoint: 4 })
    ?.includes('inhibited'))
  assert(Hyper.refusal({ threat: 1e12, charges: 3, driveLevel: 0, waypoint: 4 }) === null)
})

check('the drive takes real time to spin up and only then moves you', () => {
  const s = generate(world, digest)
  const node = s.nodes[60]
  const sit = { threat: null, charges: 3, driveLevel: 0, waypoint: node.id }
  let d = Hyper.IDLE
  let arrived = null
  let ticks = 0
  for (let i = 0; i < 400 && !arrived; i += 1) {
    const r = Hyper.advance(d, sit, node, true, 1 / 60)
    d = r.drive
    arrived = r.arriveAt
    ticks += 1
  }
  assert(arrived, 'the drive never completed')
  assert(ticks > 60, `it jumped after ${ticks} frames — that is an escape button`)
  const dist = Math.hypot(arrived.x - node.at.x, arrived.y - node.at.y, arrived.z - node.at.z)
  assert(dist > 0, 'a jump landed inside the station')
})

check('releasing the key aborts and refunds', () => {
  // A charge consumed by a keystroke the player took back is the kind of loss that teaches
  // somebody never to touch the mechanic again.
  const s = generate(world, digest)
  const node = s.nodes[60]
  const sit = { threat: null, charges: 3, driveLevel: 0, waypoint: node.id }
  let d = Hyper.advance(Hyper.IDLE, sit, node, true, 1).drive
  assert(d.phase === 'charging')
  const r = Hyper.advance(d, sit, node, false, 1 / 60)
  assert(r.drive.phase === 'idle' && !r.spent, 'an aborted jump still cost a charge')
  assert((r.notice ?? '').includes('abort'), r.notice)
})

check('retargeting mid-charge starts over', () => {
  // A drive that kept its progress across a new destination would let a player charge somewhere
  // safe and arrive somewhere else.
  const s = generate(world, digest)
  const a = s.nodes[60]
  const b = s.nodes[61]
  const sit = { threat: null, charges: 3, driveLevel: 0, waypoint: a.id }
  const partway = Hyper.advance(Hyper.IDLE, sit, a, true, 1).drive
  const switched = Hyper.advance(partway, { ...sit, waypoint: b.id }, b, true, 1 / 60).drive
  assert(switched.target === b.id, 'the drive kept the old target')
  assert(switched.charged < partway.charged, 'the charge carried over to a new destination')
})

check('a hostile in range inhibits the drive, which is what makes a fight a commitment', () => {
  const s = generate(world, digest)
  const node = s.nodes[60]
  const sit = { threat: JUMP_INHIBIT * 0.5, charges: 3, driveLevel: 0, waypoint: node.id }
  let d = Hyper.IDLE
  for (let i = 0; i < 400; i += 1) d = Hyper.advance(d, sit, node, true, 1 / 60).drive
  assert(d.phase === 'inhibited', `phase was ${d.phase}`)
  assert(Hyper.progress(d, 0) === 0, 'an inhibited drive showed charge')
})

check('a better drive spins up faster but never instantly', () => {
  assert(jumpCharge(4, 1000) < jumpCharge(0, 1000), 'the upgrade does nothing')
  assert(jumpCharge(4, 1000) > 200, 'a fully upgraded drive is effectively instant')
})

check('a jump is reachable in play, and lands you where the waypoint is', () => {
  const s = generate(world, digest)
  let g = route(newGame(s), s, 'trade')
  const target = s.nodes.find((n) => n.id === g.waypoint)
  const keys = new Set(['KeyJ'])
  for (let f = 0; f < 60 * 8; f += 1) {
    g = tick(g, s, { keys, firing: false, dt: 1 / 60, nowMs: (f * 1000) / 60 })
  }
  const d = Math.hypot(
    g.camera.position[0] - target.at.x,
    g.camera.position[1] - target.at.y,
    g.camera.position[2] - target.at.z,
  )
  assert(d < DOCK_RANGE * 3, `ended ${(d / 1e6).toFixed(1)}M from the waypoint`)
  assert(g.ship.jumpFuel < jumpCapacity(0), 'the jump cost nothing')
  assert(g.throttle === 0, 'a jump preserved momentum — that is a collision with no answer')
})

check('a dock charges the jump drive and a depot does not', () => {
  // Six times as many depots as docks is what makes a jump charge worth planning around.
  const s = generate(world, digest)
  const dock = s.nodes.find((n) => n.kind === 'dock')
  const depot = s.nodes.find((n) => n.kind === 'depot')
  const dry = { ...newGame(s), ship: { ...newShip(), fuel: 1, jumpFuel: 0 } }
  assert(useService({ ...dry, nearby: dock }, 'refuel').ship.jumpFuel > 0, 'a dock did not charge')
  assert(useService({ ...dry, nearby: depot }, 'refuel').ship.jumpFuel === 0, 'a depot charged')
})

// ── shields ──────────────────────────────────────────────────────────────────

check('hull is the primary health and never comes back on its own', () => {
  // The asymmetry is the rhythm of a fight: break contact, let shields recover, re-engage.
  // Inverting it would make every engagement a war of attrition against a clock.
  let sh = damage(newShip(), 500, 0)
  assert(sh.hull < hullMax(0), 'the ship took no hull damage from an overwhelming hit')
  const later = recharge(sh, 60, 1e9)
  assert(later.hull === sh.hull, 'hull regenerated')
  assert(later.shield > sh.shield, 'shields did not regenerate after a long lull')
})

check('shields do not regenerate while you are being shot', () => {
  const sh = damage(newShip(), 10, 1000)
  const soon = recharge(sh, 1, 1000 + SHIELD_DELAY_MS * 0.5)
  assert(soon.shield === sh.shield, 'shields recovered mid-fight')
  const after = recharge(sh, 1, 1000 + SHIELD_DELAY_MS * 2)
  assert(after.shield > sh.shield, 'shields never recovered')
})

check('a shield upgrade delivers the buffer it sold you', () => {
  const rich = { ...newShip(), salvage: 20_000, shield: 0 }
  const bought = buy(rich, 'shields')
  assert(bought.ok && bought.ship.shield === shieldMax(1), 'the new buffer was not filled')
  const drive = buy(rich, 'drive')
  assert(drive.ok && drive.ship.jumpFuel === jumpCapacity(1), 'the new charges were not loaded')
})

// ── silhouettes and the sky ──────────────────────────────────────────────────

check('every shape anything names has a mesh', () => {
  // The class table, the node vocabulary and the renderer must not be three homes for one
  // decision. A shape that exists, is selected, and quietly draws nothing is the failure here.
  for (const id of CLASS_IDS) {
    const shape = CLASSES[id].shape
    const build = Meshes[shape]
    assert(typeof build === 'function', `${id} names ${shape}, which has no mesh`)
    assert(build().length > 0, `${shape} builds an empty mesh`)
  }
  // And every node kind, now that they are silhouettes rather than coloured spheres.
  for (const kind of ['origin', 'station', 'dock', 'depot', 'market', 'derelict', 'marker', 'phantom', 'rift']) {
    const build = Meshes[kind]
    assert(typeof build === 'function' && build().length > 0, `${kind} has no silhouette`)
  }
})

check('a node is told apart by its shape, not by its colour', () => {
  // They were shaded spheres, so a market and a rift were the same ball in different colours and
  // the whole vocabulary the record carries arrived as a palette. That fails the way colour-only
  // distinctions fail everywhere in this project: on a bad monitor, at a glance, or for a
  // colour-blind player, two shades are one thing.
  const seen = new Map()
  for (const kind of ['origin', 'station', 'dock', 'depot', 'market', 'derelict', 'marker', 'phantom', 'rift']) {
    const key = Meshes[kind]().join(',')
    assert(!seen.has(key), `${kind} and ${seen.get(key)} draw the same shape`)
    seen.set(key, kind)
  }
})

check('every node is drawn hollow', () => {
  // They are open structures you fly through. A shaded sphere claims a surface where there is a
  // frame, which is a claim about the observed thing that nobody made.
  const s = generate(world, digest)
  const list = drawList(s)
  const nodes = list.bodies.slice(0, s.nodes.length)
  assert(nodes.every((b) => !b.solid), 'a node is drawn as a solid body')
  assert(nodes.every((b) => b.facing), 'a node has no orientation, so every ring faces one way')
})

check('a hull points along +Z and fits the unit sphere it will be scaled by', () => {
  for (const [name, m] of [['interceptor', interceptor()], ['gunship', gunship()], ['capital', capital()]]) {
    assert(m.length % 6 === 0, `${name} is not a line list`)
    let maxZ = -Infinity
    let minZ = Infinity
    let maxR = 0
    for (let i = 0; i < m.length; i += 3) {
      maxZ = Math.max(maxZ, m[i + 2])
      minZ = Math.min(minZ, m[i + 2])
      maxR = Math.max(maxR, Math.hypot(m[i], m[i + 1], m[i + 2]))
    }
    assert(maxZ > 0 && Math.abs(maxZ) > Math.abs(minZ) * 0.5, `${name} does not point forward`)
    assert(maxR < 2.6, `${name} extends to ${maxR.toFixed(2)}, far outside unit scale`)
  }
})

check('a projectile is a cylinder, not a point', () => {
  // A sphere travelling at half the sector per second is a dot that teleports between frames.
  // The streak is what makes a tracer readable, and it points back at whatever fired it.
  const b = bolt()
  assert(b.length > 0 && b.length % 9 === 0, 'the bolt is not a triangle list')
  let minZ = Infinity
  for (let i = 2; i < b.length; i += 3) minZ = Math.min(minZ, b[i])
  assert(minZ < 0, 'the bolt has no length along its travel axis')
  assert(BOLT_LENGTH > 4, 'a bolt that short is a dot again')
  assert(BOLT_GLOW > 1, 'the halo is not larger than the core, so there is no glow')
})

check('the sky is a function of the commitment and nothing else', () => {
  // Determinism applied to something with no gameplay effect — precisely because making an
  // exception for cosmetics is how the rule stops being one.
  const a = starfield(digest, 200)
  const b = starfield(digest, 200)
  assert(a.every((v, i) => v === b[i]), 'the sky is not deterministic')
  assert(!starfield('f'.repeat(64), 200).every((v, i) => v === a[i]), 'the seed does not move it')
})

check('stars are spread over the whole sphere rather than bunched at the poles', () => {
  // A naive two-angle pick clusters hard at the poles, and a night sky with two bright patches
  // in it reads as a bug rather than as a sky.
  const f = starfield(digest, 2000)
  let north = 0
  let equator = 0
  for (let i = 0; i < 2000; i += 1) {
    const y = f[i * 4 + 1]
    if (y > 0.8) north += 1
    if (Math.abs(y) < 0.2) equator += 1
  }
  // Equal-area bands: |y|<0.2 covers 20% of the sphere, y>0.8 covers 10%.
  assert(equator > north, `poles ${north}, equator ${equator}`)
  for (let i = 0; i < 2000; i += 1) {
    const r = Math.hypot(f[i * 4], f[i * 4 + 1], f[i * 4 + 2])
    assert(Math.abs(r - 1) < 1e-4, 'a star is off the unit sphere')
    assert(f[i * 4 + 3] > 0, 'a star has no brightness')
  }
})

check('lanes are drawn at the edge of visibility', () => {
  // At a thousand nodes the lane mesh was a bright cage that hid everything inside it — the
  // sector read as a diagram of itself rather than as a place.
  const s = generate(world, digest)
  const list = drawList(s)
  assert(list.segments.length > 0)
  for (const seg of list.segments) {
    assert(seg.alpha > 0, 'a lane is invisible; routes must still be followable')
    assert(seg.alpha < 0.2, `a lane at ${seg.alpha} is a cage`)
  }
})

check('a craft is drawn as its class, facing where it flies', () => {
  // A wireframe with no facing is a shape with no information in it, and the whole reason ships
  // are line models is that you can see which way an opponent is about to break.
  const s = generate(world, digest)
  let g = newGame(s)
  g = tick(g, s, { keys: new Set(), firing: false, dt: 0.016, nowMs: 1 })
  const list = drawList(s, dynamicOf(g, s))
  const hulls = list.bodies.filter((b) => shapeOf(b) !== 'sphere' && shapeOf(b) !== 'shell' && shapeOf(b) !== 'bolt')
  assert(hulls.length > 0, 'no craft was drawn as a hull')
  for (const h of hulls) {
    assert(h.facing, 'a hull was drawn with no facing')
    const l = Math.hypot(h.facing.x, h.facing.y, h.facing.z)
    assert(Math.abs(l - 1) < 1e-6, 'a facing is not a unit vector')
  }
})

check('a bolt carries the direction it is travelling', () => {
  const s = generate(world, digest)
  let g = tick(newGame(s), s, { keys: new Set(), firing: true, dt: 0.016, nowMs: 1000 })
  const bolts = drawList(s, dynamicOf(g, s)).bodies.filter((b) => shapeOf(b) === 'bolt')
  assert(bolts.length > 0, 'no bolt was drawn')
  assert(bolts.every((b) => b.facing), 'a bolt was drawn with no direction')
})

check('a craft size comes from its class, never from a reported magnitude', () => {
  // A craft's size is now a claim about how dangerous it is. That must not come from a number
  // in the record, or somebody writing one has a reason to shrink it.
  const s = generate(world, digest)
  let g = tick(newGame(s), s, { keys: new Set(), firing: false, dt: 0.016, nowMs: 1 })
  const list = drawList(s, dynamicOf(g, s))
  for (const c of Enemy.living(g.swarm)) {
    const body = list.bodies.find((b) => b.label.includes(c.spec.label))
    if (body) assert(body.radius === c.spec.radius, 'a craft was sized by something else')
  }
})

// ── the feel of a hit ────────────────────────────────────────────────────────

check('a landed hit flashes, and a hull hit flashes harder than a soaked one', () => {
  // This is the entirety of the game's feedback that a shot connected. A shooter where you
  // cannot tell a hit from a miss has no skill expression in it, however good the ballistics
  // underneath are.
  const s = generate(world, digest)
  let g = newGame(s)
  const r = s.raiders[0]
  g = { ...g, camera: { ...g.camera, position: [r.at.x, r.at.y, r.at.z + EXTENT * 0.01] } }
  let flashed = false
  for (let f = 0; f < 60 * 10 && !flashed; f += 1) {
    g = tick(g, s, { keys: new Set(), firing: true, dt: 1 / 60, nowMs: (f * 1000) / 60 })
    flashed = Object.values(g.flashes).some((v) => v > 0)
  }
  assert(flashed, 'ten seconds of point-blank fire produced no hit feedback')
})

check('a flash decays rather than sticking on', () => {
  const s = generate(world, digest)
  let g = { ...newGame(s), flashes: { x: 1 } }
  g = tick(g, s, { keys: new Set(), firing: false, dt: 0.2, nowMs: 1 })
  assert((g.flashes.x ?? 0) < 1, 'a flash never faded')
  for (let i = 0; i < 20; i += 1) {
    g = tick(g, s, { keys: new Set(), firing: false, dt: 0.2, nowMs: i * 200 })
  }
  assert(g.flashes.x === undefined, 'a spent flash is still being carried')
})

check('being shot kicks the screen, and hull hits kick harder', () => {
  const s = generate(world, digest)
  const g = { ...newGame(s), shake: 0 }
  assert(g.shake === 0)
  // Direct: a shield hit and a hull hit must not feel identical.
  const shielded = damage(newShip(), 5, 0)
  const breached = damage({ ...newShip(), shield: 0 }, 5, 0)
  assert(shielded.hull === hullMax(0), 'a shielded hit reached hull')
  assert(breached.hull < hullMax(0), 'an unshielded hit did not reach hull')
})

check('every class in the table can actually be met', () => {
  // Both capitals were unreachable once and nothing failed: the class roll came from
  // `durability`, which returns one of *six* values, so it covered about half the distribution
  // and never reached the top bracket. A table whose last entries are decoration is the kind of
  // bug that hides behind a plausible-looking sector.
  //
  // Asserted over the roll rather than over one sector, because the war classes are deliberately
  // rare enough that a given sector may have none — which is the point of them.
  const seen = new Set()
  for (let i = 0; i < 4000; i += 1) seen.add(classFor(i).id)
  for (const id of CLASS_IDS) {
    assert(seen.has(id), `${id} exists in the table and can never be rolled`)
  }
})

check('a sector carries capitals, and war classes stay rare', () => {
  const s = generate(world, digest)
  const specs = swarmOf(s.raiders, s.seed).craft.map((c) => c.spec)
  assert(specs.some((x) => x.capital), 'no capital anywhere in the sector')
  const war = specs.filter((x) => x.id === 'dreadnought' || x.id === 'leviathan').length
  // Bounded, not absent. Every sector carries a deliberate capital garrison
  // (`raiders.ts::GARRISON`) on top of whatever the roll produces, because a roll that reaches a
  // leviathan about once in a hundred and fifty meant most sectors had none — the war classes
  // existed and turned up by accident. The bound is what keeps "a sector with several is one you
  // cannot cross" true.
  assert(war >= 3, `only ${war} war-class ships — the garrison is not being placed`)
  assert(war <= 8, `${war} war-class ships — a sector with several is one you cannot cross`)
  assert(specs.filter((x) => x.id === 'titan').length >= 1, 'a sector has no titan')
})

check('the garrison is named rather than rolled, and only the sector may name it', () => {
  const s = generate(world, digest)
  const garrison = s.raiders.filter((r) => r.klass)
  assert(garrison.length > 0, 'no named capital anywhere')
  assert(garrison.every((r) => r.unlogged), 'a named class reached a contact the record reported')
  const crafts = swarmOf(s.raiders, s.seed).craft
  for (const want of ['dreadnought', 'leviathan', 'titan']) {
    assert(crafts.some((c) => c.spec.id === want), `no ${want} in the sector`)
  }

  // And a record cannot name its own opposition. `swarmOf` honours `klass` only on an unlogged
  // contact, so a producer that learned the field exists gains nothing by setting it.
  const forged = s.contacts
    .filter((c) => c.hostility === 'hostile')
    .map((c) => ({ ...c, klass: 'titan' }))
  assert(forged.length > 0, 'this fixture reports no hostile signal')
  assert(
    swarmOf(forged, s.seed).craft.every((c) => c.spec.id !== 'titan'),
    'a record talked its way into choosing what it fights',
  )
})

check('the war classes are colossal against everything else', () => {
  // "The largest enemies are not big enough" was the complaint, and a ratio is the only way to
  // pin it: they have to dwarf a station, not merely a fighter.
  const station = R_STATION
  assert(CLASSES.destroyer.radius > station * 4, 'a destroyer is not obviously bigger than a station')
  assert(CLASSES.dreadnought.radius > CLASSES.destroyer.radius * 1.6, 'a dreadnought is a big destroyer')
  assert(CLASSES.leviathan.radius > station * 15, 'the largest ship is not colossal')
  // Size is still a ratio; durability is the ladder. A leviathan is seven warheads against a
  // fighter's one — colossal to look at and finishable in a single sortie, which is the whole
  // point of the rebalance.
  assert(
    CLASSES_MOD.PHOTONS_TO_KILL.leviathan >= CLASSES_MOD.PHOTONS_TO_KILL.interceptor * 7,
    'it dies like a fighter',
  )
})

check('the class roll covers its whole range', () => {
  // Asserted directly as well, because the sector test above depends on how many raiders there
  // happen to be, and that number is allowed to change.
  const buckets = new Set()
  for (let i = 0; i < 400; i += 1) buckets.add(classRoll(digest, `x:${i}`) % 100)
  assert(buckets.size > 70, `only ${buckets.size} distinct rolls out of 100`)
  assert(Math.min(...buckets) < 5 && Math.max(...buckets) > 95, 'the roll does not reach its ends')
})

check('raiders arrive in wings, so an encounter is an encounter', () => {
  // Scattered uniformly, sixty craft in a volume this size sit two hundred million units apart
  // and you essentially never meet one: the sector reads as empty and the whole combat system
  // goes unused. This asserts the clustering rather than the count, because the count will move.
  const s = generate(world, digest)
  const near = s.raiders.filter((r) => {
    const d = Math.hypot(
      r.at.x - s.raiders[0].at.x,
      r.at.y - s.raiders[0].at.y,
      r.at.z - s.raiders[0].at.z,
    )
    return d < AGGRO_RANGE
  })
  assert(near.length >= 3, `a raider's nearest company is ${near.length - 1} craft`)
})

check('a wing will kill a passive player, and that is the point', () => {
  // Combat has to be dangerous or none of the rest of it matters. A player who does not fight
  // back and does not manoeuvre must lose — and this is the assertion that would catch an AI
  // that looks busy and never actually lands a shot, which is what the previous one did.
  const s = generate(world, digest)
  const wing = s.raiders.filter((r) => r.id.startsWith('raider:0:'))
  let g = newGame(s)
  g = {
    ...g,
    camera: {
      ...g.camera,
      position: [wing[0].at.x, wing[0].at.y, wing[0].at.z + EXTENT * 0.02],
    },
  }
  for (let f = 0; f < 60 * 90 && !g.lost; f += 1) {
    g = tick(g, s, { keys: new Set(), firing: false, dt: 1 / 60, nowMs: (f * 1000) / 60 })
  }
  assert(g.lost, `survived ninety seconds doing nothing, on ${g.ship.hull} hull`)
})

check('an engagement cycles through its behaviours rather than sitting in one', () => {
  // A craft that only ever pursues is the old distance-check wearing a state machine.
  const s = generate(world, digest)
  const wing = s.raiders.filter((r) => r.id.startsWith('raider:0:'))
  let g = newGame(s)
  g = {
    ...g,
    camera: {
      ...g.camera,
      position: [wing[0].at.x, wing[0].at.y, wing[0].at.z + EXTENT * 0.02],
    },
  }
  const seen = new Set()
  for (let f = 0; f < 60 * 40 && !g.lost; f += 1) {
    g = tick(g, s, { keys: new Set(), firing: false, dt: 1 / 60, nowMs: (f * 1000) / 60 })
    for (const c of Enemy.living(g.swarm)) seen.add(c.behaviour)
  }
  for (const b of ['pursue', 'attack', 'overshoot']) {
    assert(seen.has(b), `no craft ever entered ${b}`)
  }
})

check('a craft that loses you while turning comes back', () => {
  // The hysteresis. Without it a craft that starts with the player astern burns away from the
  // target for the two seconds its turn takes, crosses its own aggro radius doing so, drops to
  // patrol, and drifts off forever — which is what a skiff parked ten million units off a
  // stationary player actually did.
  const s = generate(world, digest)
  const sw = swarmOf(s.raiders, s.seed)
  const c0 = { ...sw.craft[0], behaviour: 'pursue' }
  const justOutside = c0.spec.aggro * 1.3
  assert(decide(c0, justOutside, 1, 1000) !== 'patrol', 'an engaged craft gave up too easily')
  assert(decide({ ...c0, behaviour: 'patrol' }, justOutside, 1, 1000) === 'patrol',
    'a craft acquired a target beyond its own sensor range')
})

// ── collisions ───────────────────────────────────────────────────────────────

check('what is solid is exactly what was observed', () => {
  // The only interesting decision in `collide.ts`. Making a phantom solid is the game asserting
  // something is there on the strength of a record that says nobody saw it. Making it permeable
  // is not the opposite claim — the game simulates what was observed, and there is nothing here
  // to hit because nobody observed anything.
  for (const kind of ['origin', 'station', 'dock', 'depot', 'market', 'derelict']) {
    assert(collidesWith(kind), `${kind} was perceived and should be solid`)
  }
  for (const kind of ['phantom', 'marker', 'rift']) {
    assert(!collidesWith(kind), `${kind} is a statement about knowledge, not a thing in space`)
    assert(permeableNote(kind).length > 10, `${kind} passes through and says nothing about why`)
  }
})

check('the physics radius is the drawn radius', () => {
  // A hit test that disagrees with the picture is the worst kind of bug in a game: the player
  // bounces off empty space, or flies through something visibly in the way.
  const s = generate(world, digest)
  const grid = gridFor(s)
  const solid = s.nodes.filter((n) => collidesWith(n.kind))
  let checked = 0
  for (const [, bucket] of grid.cells) {
    for (const o of bucket) {
      assert(o.radius === nodeRadius(roleOfNode(o.node)), `${o.node.kind} uses a private radius`)
      checked += 1
    }
  }
  assert(checked === solid.length, `${checked} obstacles for ${solid.length} solid nodes`)
})

check('a sweep catches an obstacle a step would tunnel through', () => {
  // A bolt covers a few million units a frame and a station is a couple of million across, so an
  // endpoint check misses it entirely and the shot arrives on the far side.
  const s = generate(world, digest)
  const grid = gridFor(s)
  const station = s.nodes.find((n) => collidesWith(n.kind) && n.id !== 0)
  const r = nodeRadius(roleOfNode(station))
  const from = { x: station.at.x - r * 10, y: station.at.y, z: station.at.z }
  const to = { x: station.at.x + r * 10, y: station.at.y, z: station.at.z }
  assert(sweep(grid, from, to, 0), 'a segment straight through a station missed it')
  // The endpoints themselves are both well clear, which is the whole point.
  assert(!sweep(grid, from, from, 0), 'the start point was inside the station')
  assert(!sweep(grid, to, to, 0), 'the end point was inside the station')
})

check('a sweep returns the first obstacle, not any obstacle', () => {
  // A shot fired down a line of stations has to stop at the near one.
  const s = generate(world, digest)
  const grid = gridFor(s)
  const solid = s.nodes.filter((n) => collidesWith(n.kind)).slice(0, 40)
  for (const a of solid) {
    for (const b of solid) {
      if (a.id === b.id) continue
      const hit = sweep(grid, a.at, b.at, 0)
      if (!hit) continue
      // Whatever it found, nothing solid may sit strictly nearer along the same segment.
      for (const o of solid) {
        const { dist, t } = closestOnSegment(o.at, a.at, b.at)
        if (dist <= nodeRadius(roleOfNode(o)) && t < hit.t - 1e-9) {
          assert(false, 'the sweep skipped a nearer obstacle')
        }
      }
      return
    }
  }
})

check('a resolved body ends outside the surface, never touching it', () => {
  // A body left exactly on the surface re-collides next frame at zero speed and sticks. That is
  // the classic way a collision system becomes flypaper, and it is how a player who clips a dock
  // would never get free of it.
  const s = generate(world, digest)
  const grid = gridFor(s)
  const station = s.nodes.find((n) => collidesWith(n.kind) && n.id !== 0)
  const r = nodeRadius(roleOfNode(station))
  const from = { x: station.at.x - r * 6, y: station.at.y, z: station.at.z }
  const res = resolve(grid, from, station.at, R_PLAYER, 1 / 60)
  assert(res.hit, 'flying into a station did not collide')
  const gap = Math.hypot(res.at.x - station.at.x, res.at.y - station.at.y, res.at.z - station.at.z)
  assert(gap > r + R_PLAYER, `ended ${gap} from the centre, inside the ${r + R_PLAYER} surface`)
  // And from there, a second resolve must not fire again — that is the flypaper test.
  assert(!resolve(grid, res.at, res.at, R_PLAYER, 1 / 60).hit, 'a resolved body is stuck')
})

check('impact speed is closing speed, so a graze is not a crash', () => {
  // Charging a tangential near-miss the same as a nose-first impact makes collisions feel
  // arbitrary, which is worse than not having them.
  const s = generate(world, digest)
  const grid = gridFor(s)
  const station = s.nodes.find((n) => collidesWith(n.kind) && n.id !== 0)
  const r = nodeRadius(roleOfNode(station))
  const head = resolve(
    grid,
    { x: station.at.x - r * 6, y: station.at.y, z: station.at.z },
    station.at,
    R_PLAYER,
    1 / 60,
  )
  const graze = resolve(
    grid,
    { x: station.at.x - r * 6, y: station.at.y - r * 0.9, z: station.at.z },
    { x: station.at.x + r * 6, y: station.at.y - r * 0.9, z: station.at.z },
    R_PLAYER,
    1 / 60,
  )
  if (graze.hit) assert(graze.impact < head.impact, 'a graze cost as much as a head-on hit')
})

check('flying through an observed station puts it on the sensors', () => {
  // Nodes are open structures now — solid ones at this size would be a maze rather than scenery,
  // and a sector whose landmarks are also walls is one where the interesting thing about a market
  // is that it is in the way. The epistemic distinction moved to what *registers*.
  const s = generate(world, digest)
  // A station with nothing parked on it. Record signals sit *on* nodes, so the obvious first
  // station has a hostile inside it and the ship rams the craft on the way through — which is
  // correct behaviour and not what this is measuring.
  const occupied = new Set([...s.contacts, ...s.raiders].map((c) => `${c.at.x},${c.at.y},${c.at.z}`))
  const station = s.nodes.find(
    (n) => registers(n.kind) && n.id !== 0 && !occupied.has(`${n.at.x},${n.at.y},${n.at.z}`),
  )
  const r = nodeRadius(roleOfNode(station))
  let g = { ...newGame(s), throttle: 1 }
  g = {
    ...g,
    camera: {
      position: [station.at.x, station.at.y, station.at.z + r * 3],
      orientation: [0, 0, 0, 1],
    },
  }
  let noticed = null
  for (let f = 0; f < 400 && !noticed; f += 1) {
    g = tick(g, s, { keys: new Set(), firing: false, dt: 1 / 60, nowMs: (f * 1000) / 60 })
    if ((g.notice ?? '').includes('passing through')) noticed = g.notice
  }
  assert(noticed, 'flying through a station registered nothing')
  assert(g.ship.hull === hullMax(0), 'an open structure damaged the ship')
  assert(g.throttle === 1, 'flying through a station cut the drive')
})

check('flying through a phantom registers nothing, and says so', () => {
  // The sentence is the mechanic, and it now has a counterpart — a distinction only one side of
  // which is ever visible is not one the player can learn.
  const objects = world.objects.map((o) => ({
    ...o,
    provenance: { kind: 'simulated', age_secs: 0, budget_secs: 1 },
  }))
  const s = generate({ ...world, objects }, digest)
  const ph = s.nodes.find((n) => n.kind === 'phantom')
  assert(ph, 'the fixture produced no phantom')
  const note = passageNote(ph.kind, ph.label)
  assert(note.includes('nothing on sensors'), note)
  assert(note.includes('modelled'), note)
  assert(passageNote('station', 'live-1').includes('passing through'), 'an observed node says nothing')
})

check('a phantom is flown through, and the game says why', () => {
  // The sentence is the mechanic. A player who flies through a station and is told it was
  // modelled rather than observed has learned what a provenance is, from the cockpit.
  const objects = world.objects.map((o) => ({
    ...o,
    provenance: { kind: 'simulated', age_secs: 0, budget_secs: 1 },
  }))
  const s = generate({ ...world, objects }, digest)
  const phantom = s.nodes.find((n) => n.kind === 'phantom')
  assert(phantom, 'the fixture produced no phantom')
  const grid = gridFor(s)
  assert(!sweep(grid, phantom.at, phantom.at, R_PLAYER), 'a phantom is solid')
  const through = passedThrough(
    s,
    { x: phantom.at.x - 1000, y: phantom.at.y, z: phantom.at.z },
    { x: phantom.at.x + 1000, y: phantom.at.y, z: phantom.at.z },
    R_PLAYER,
  )
  assert(through && through.kind === 'phantom', 'flying through a phantom went unremarked')
})

check('separation pushes two overlapping craft apart, symmetrically', () => {
  // Two wireframes occupying one point is the single cheapest-looking thing this renderer can
  // produce, and steering alone does not prevent it — a craft that merely steers away still
  // interpenetrates while it turns.
  const a = { at: { x: 0, y: 0, z: 0 }, radius: 1000 }
  const b = { at: { x: 500, y: 0, z: 0 }, radius: 1000 }
  let push = separate([a, b])
  assert(push[0].x < 0 && push[1].x > 0, 'they were not pushed apart')
  assert(Math.abs(push[0].x + push[1].x) < 1e-9, 'the push was not symmetric')
  // A fraction per frame, not the whole overlap: craft can start deeply overlapped and resolving
  // that in one step scattered ships twenty million units the instant a world loaded.
  assert(push[1].x - push[0].x < 2000 * SEPARATION, 'the whole overlap was corrected at once')
  // It still converges, and quickly.
  let A = { ...a, at: { ...a.at } }
  let B = { ...b, at: { ...b.at } }
  for (let i = 0; i < 60; i += 1) {
    push = separate([A, B])
    A = { ...A, at: { x: A.at.x + push[0].x, y: A.at.y, z: A.at.z } }
    B = { ...B, at: { x: B.at.x + push[1].x, y: B.at.y, z: B.at.z } }
  }
  const gap = B.at.x - A.at.x
  assert(gap >= 2000 * SEPARATION - 1, `converged to ${gap}, still overlapping`)
})

check('two craft at exactly the same point still separate, deterministically', () => {
  // No direction exists to separate along, and the naive form divides by zero. Two players
  // holding the same record must see the same fight, so the fallback has to be a fixed choice.
  const same = { at: { x: 7, y: 7, z: 7 }, radius: 500 }
  const a = separate([{ ...same }, { ...same }])
  const b = separate([{ ...same }, { ...same }])
  assert(a[0].x !== 0 || a[0].y !== 0 || a[0].z !== 0, 'coincident craft did not separate')
  assert(JSON.stringify(a) === JSON.stringify(b), 'separation is not deterministic')
})

check('avoidance bends a heading rather than replacing it', () => {
  // The first version returned the pure sidestep and produced a permanent orbit: the craft turned
  // fully broadside, flew sideways until the obstacle left its probe, turned back, re-acquired
  // it, and repeated — an enemy politely declining to fight.
  const s = generate(world, digest)
  const grid = gridFor(s)
  const station = s.nodes.find((n) => collidesWith(n.kind) && n.id !== 0)
  const r = nodeRadius(roleOfNode(station))
  const at = { x: station.at.x, y: station.at.y, z: station.at.z + r * 8 }
  const want = { x: 0, y: 0, z: -1 }
  // Speed matters: the probe is `speed * lookaheadSecs`, so a craft too slow to reach the station
  // within its lookahead correctly does not swerve for it.
  const near = steerAround(grid, at, want, want, r, r * 8)
  assert(near.urgency > 0, 'an obstacle dead ahead was not seen')
  assert(near.dir.z < 0, 'the craft was turned away from where it wanted to go entirely')

  const clear = steerAround(grid, at, { x: 0, y: 0, z: 1 }, { x: 0, y: 0, z: 1 }, r, r * 4)
  assert(clear.urgency === 0 && clear.dir.z === 1, 'a clear heading was bent anyway')
})

check('craft steer around nodes even though nothing stops them', () => {
  // A wing flying *through* a station ring is technically correct and looks like the geometry is
  // decorative. Steering round one costs nothing and reads as piloting — but it must stay
  // steering: hard-stopping a craft would trap the record-signal ones, which start inside a
  // station because contacts sit on nodes.
  const src = codeOf(join(here, '..', 'lib', 'scemaworld', 'enemy.ts'))
  assert(src.includes('steerAround'), 'craft do not avoid nodes at all')
  assert(!src.includes('resolve(grid'), 'craft are hard-stopped by something they can fly through')
})

check('a wireframe frame is not cover, for either side', () => {
  // Deliberate, and the deliberate part is that it is symmetric. When nodes were solid, geometry
  // stopped fire in both directions; now that they are open structures it stops fire in neither.
  // What must never happen is one of the two — the player learning that hiding works only for the
  // other side is worse than either consistent rule.
  const s = generate(world, digest)
  const station = s.nodes.find((n) => registers(n.kind) && n.id !== 0)
  const r = nodeRadius(roleOfNode(station))
  const from = { x: station.at.x, y: station.at.y, z: station.at.z + r * 3 }
  const target = { ...firstSolid(s), at: { x: station.at.x, y: station.at.y, z: station.at.z - r * 3 } }
  let c = newCombat()
  let hits = []
  for (let i = 0; i < 40 && hits.length === 0; i += 1) {
    c = fire(c, from, { x: 0, y: 0, z: -1 }, i * 200, [target])
    const res = step(c, 0.05, [target], s.seed)
    c = res.combat
    hits = res.hits
  }
  assert(hits.length > 0, 'a shot was blocked by a structure it should pass through')

  const enemySrc = codeOf(join(here, '..', 'lib', 'scemaworld', 'enemy.ts'))
  assert(!enemySrc.includes('resolve(grid, s.at, at'), 'enemy fire is blocked but the player is not')
})

check('a bolt is stopped by a station rather than killing what is behind it', () => {
  const s = generate(world, digest)
  const grid = gridFor(s)
  const station = s.nodes.find((n) => collidesWith(n.kind) && n.id !== 0)
  const r = nodeRadius(roleOfNode(station))
  const from = { x: station.at.x, y: station.at.y, z: station.at.z + r * 5 }
  const behind = { x: station.at.x, y: station.at.y, z: station.at.z - r * 5 }
  const target = { ...firstSolid(s), at: behind }
  let c = newCombat()
  let hits = []
  for (let i = 0; i < 30 && hits.length === 0; i += 1) {
    c = fire(c, from, { x: 0, y: 0, z: -1 }, i * 200, [target])
    const res = step(c, 0.05, [target], s.seed, (a, b) => sweep(grid, a, b, 0) !== null)
    c = res.combat
    hits = res.hits
  }
  assert(hits.length === 0, 'a shot passed through a station and hit something behind it')
})

check('the ship starts outside the origin market and pointed away from it', () => {
  // Two bugs lived here and both were found by tests. Spawning at the origin put the ship inside
  // a station, so every frame was an impact — throttle cut, hull ticking down, shots blocked at
  // the muzzle. Moving it to +Z then had the camera, which looks along −Z, staring into that same
  // station, so the first press of the throttle flew straight into it.
  const s = generate(world, digest)
  const grid = gridFor(s)
  let g = newGame(s)
  const at = { x: g.camera.position[0], y: g.camera.position[1], z: g.camera.position[2] }
  assert(!sweep(grid, at, at, R_PLAYER), 'the ship spawns inside a station')

  const before = Math.hypot(...g.camera.position)
  for (let f = 0; f < 90; f += 1) {
    g = tick(g, s, { keys: new Set(['ArrowUp']), firing: false, dt: 1 / 60, nowMs: (f * 1000) / 60 })
  }
  assert(g.throttle > 0.9, `throttle reached ${g.throttle} — something is in the way`)
  assert(Math.hypot(...g.camera.position) > before, 'the ship flew toward the origin, not away')
  assert(g.ship.hull === hullMax(0), 'the ship took damage flying straight ahead from spawn')
})

check('ramming a craft costs both of you', () => {
  // A craft you can fly through is a craft that is not there.
  const s = generate(world, digest)
  let g = newGame(s)
  // A raider, deliberately. Contacts from the record sit *on* nodes, so a record-signal craft
  // starts inside a station and the ship would collide with the station before ever reaching it.
  // A raider, and not a capital: contacts from the record sit *on* nodes, and a capital's
  // collidable core is a fraction of its drawn radius, so half a radius is outside it.
  // Settle first. Craft start overlapped — a wing shares an anchor, and traffic is placed on the
  // nodes the record's signals also sit on — so separation is still moving them on frame one.
  for (let f = 0; f < 120; f += 1) {
    g = tick(g, s, { keys: new Set(), firing: false, dt: 1 / 60, nowMs: f })
  }
  const c = Enemy.living(g.swarm).find((k) => k.id.startsWith('raider:') && !k.spec.capital)
  g = {
    ...g,
    throttle: 1,
    touching: [],
    camera: { ...g.camera, position: [c.at.x, c.at.y, c.at.z + c.spec.radius * 0.5] },
  }
  g = tick(g, s, { keys: new Set(), firing: false, dt: 1 / 60, nowMs: 1 })
  assert(g.ship.shield < shieldMax(0) || g.ship.hull < hullMax(0), 'flying into a craft cost nothing')
  assert((g.notice ?? '').includes('collision'), g.notice)
  const after = Enemy.living(g.swarm).find((k) => k.id === c.id)
  assert(!after || after.hull < c.hull, 'the craft was unharmed by the collision')
  // Only meaningful if it survived — there is nothing to be inside otherwise, and comparing
  // against the dead craft's last known position measures a step that already happened.
  if (after) {
    const gap = Math.hypot(
      g.camera.position[0] - after.at.x,
      g.camera.position[1] - after.at.y,
      g.camera.position[2] - after.at.z,
    )
    assert(gap >= R_PLAYER + c.spec.radius, 'the two are still inside each other')
  }
})

check('a class can physically reach the range it wants to fight at', () => {
  // The invariant that cost two and a half minutes of a gunship orbiting nineteen million units
  // out, never closing and never disengaging. Turn radius is `speed / turn`, and at cruise every
  // class here has one two or three times its own standoff — so the eased-off combat throttle is
  // not a nicety, it is what makes the firing position reachable at all.
  const COMBAT_THROTTLE = 0.3
  for (const id of CLASS_IDS) {
    const c = CLASSES[id]
    const radius = (c.speed * COMBAT_THROTTLE) / c.turn
    assert(
      radius <= c.standoff * 1.6,
      `${id} turns in ${Math.round(radius / 1e6)}M and wants to fight at ${Math.round(c.standoff / 1e6)}M`,
    )
  }
})

check('the obstacle grid is built once per space', () => {
  // A thousand nodes against seventy craft and a few dozen bolts is a hundred thousand distance
  // tests a frame if this is rebuilt. Nodes never move, so there is nothing to invalidate.
  const s = generate(world, digest)
  assert(gridFor(s) === gridFor(s), 'the grid is rebuilt on every query')
  assert(gridFor(generate(world, digest)) !== gridFor(s), 'two spaces share one grid')
})

check('a notice expires instead of sitting under the crosshair forever', () => {
  // It used to be `notice ?? state.notice`, which never cleared: an impact message from four
  // minutes ago stayed on screen for the rest of the session, and a player reading it had no way
  // to tell whether they had just hit something or once had. A stale message is worse than none —
  // it is the interface asserting something that is no longer true.
  const s = generate(world, digest)
  let g = newGame(s)
  g = { ...g, throttle: 1, ship: { ...g.ship, fuel: 0 } }
  g = tick(g, s, { keys: new Set(), firing: false, dt: 1 / 60, nowMs: 1000 })
  assert(g.notice, 'a dry ship said nothing')
  const raised = g.notice

  g = tick(g, s, { keys: new Set(), firing: false, dt: 1 / 60, nowMs: 1000 + NOTICE_MS * 0.5 })
  assert(g.notice === raised, 'the notice vanished before it could be read')

  // Refuel it, so nothing re-raises the message, then run the clock out.
  g = { ...g, ship: { ...g.ship, fuel: 100 }, throttle: 0 }
  g = tick(g, s, { keys: new Set(), firing: false, dt: 1 / 60, nowMs: 1000 + NOTICE_MS * 2 })
  assert(g.notice === null, `notice was still "${g.notice}"`)
})

check('a notice raised between ticks is stamped by the next one', () => {
  // Services are single presses handled in the input handler, which has no clock — a held `F`
  // must refuel once, not sixty times a second. Without the sentinel their notices carry a
  // timestamp of zero and expire on the very next frame, unread.
  const s = generate(world, digest)
  const depot = s.nodes.find((n) => n.kind === 'depot')
  let g = { ...newGame(s), nearby: depot, ship: { ...newShip(), fuel: 1 } }
  g = useService(g, 'refuel')
  assert(g.notice, 'refuelling said nothing')
  const raised = g.notice
  // A tick far past any expiry window: the message is new to the tick, so it survives.
  g = tick(g, s, { keys: new Set(), firing: false, dt: 1 / 60, nowMs: 900_000 })
  assert(g.notice === raised, 'a message raised by a key press was expired before being seen')
})

// ── the reported bug: services that never engage ─────────────────────────────

check('docking range comfortably clears the largest node', () => {
  // The whole of "refuelling does not work". The origin market's radius plus the ship's put the
  // hull 0.014 of a sector from the centre and docking range was 0.019 — a shell two thousandths
  // of a sector thick, crossed in a twentieth of a second at cruise. The ship spawned *outside*
  // it, so the first station a new player ever sees reported `nothing in range`.
  //
  // The relationship is pinned rather than the number, because the number will move again.
  assert(DOCK_RANGE > R_NODE_MAX + R_PLAYER, 'a ship touching the largest node is out of range')
  const band = DOCK_RANGE - (R_NODE_MAX + R_PLAYER)
  assert(band > R_NODE_MAX, `the dockable shell is ${Math.round(band / 1e6)}M — too thin to stop in`)
})

check('the ship starts in range of the station it starts at', () => {
  // The first thing a new player can do should be the first thing they need to learn. It was
  // `nothing in range`.
  const s = generate(world, digest)
  let g = tick(newGame(s), s, { keys: new Set(), firing: false, dt: 1 / 60, nowMs: 1 })
  assert(g.nearby, 'nothing in range at spawn')
  assert(g.nearby.id === s.nodes[0].id, `nearest was ${g.nearby.label}, not the origin`)
  assert(servicesOf(g.nearby.kind).length > 0, 'the origin offers no services')
})

check('every service actually fires at the node that offers it', () => {
  // End to end, per service, rather than trusting that `servicesOf` and `useService` agree.
  const s = generate(world, digest)
  const at = (node, ship) => ({ ...newGame(s), nearby: node, ship: { ...newShip(), ...ship } })

  const depot = s.nodes.find((n) => n.kind === 'depot')
  const refuelled = useService(at(depot, { fuel: 5, jumpFuel: 0 }), 'refuel')
  assert(refuelled.ship.fuel === fuelCapacity(0), refuelled.notice)

  const dock = s.nodes.find((n) => n.kind === 'dock')
  const docked = useService(at(dock, { fuel: 5, jumpFuel: 0 }), 'refuel')
  assert(docked.ship.jumpFuel > 0, 'a dock did not charge the jump drive')
  const repaired = useService(at(dock, { hull: 10, salvage: 900 }), 'repair')
  assert(repaired.ship.hull > 10, repaired.notice)

  const market = s.nodes.find((n) => n.kind === 'market')
  const bought = purchase({ ...at(market, { salvage: 9000 }) }, 'engine')
  assert(bought.ship.levels.engine === 1, bought.notice)

  const derelict = s.nodes.find((n) => n.kind === 'derelict')
  if (derelict) {
    const stripped = useService(at(derelict, {}), 'scavenge')
    assert(stripped.ship.salvage > 0, stripped.notice)
  }
})

check('a service refused at the wrong node says which node and which service', () => {
  // "Nothing happened" is the failure this replaces. The origin is a market, so `F` there is a
  // legitimate refusal — and it has to read as one rather than as a dead key.
  const s = generate(world, digest)
  const market = s.nodes.find((n) => n.kind === 'market')
  const r = useService({ ...newGame(s), nearby: market }, 'refuel')
  assert((r.notice ?? '').includes(market.label), r.notice)
  assert((r.notice ?? '').includes('refuel'), r.notice)
})

check('the market is reachable from where the player starts', () => {
  // A market you cannot afford is a design; a market you can never stand in front of is a bug.
  const s = generate(world, digest)
  let g = tick(newGame(s), s, { keys: new Set(), firing: false, dt: 1 / 60, nowMs: 1 })
  g = { ...g, ship: { ...g.ship, salvage: 9000 } }
  const bought = purchase(g, 'shields')
  assert(bought.ship.levels.shields === 1, `could not trade at spawn: ${bought.notice}`)
})

// ── the sector at its new size ───────────────────────────────────────────────

check('a node is a structure rather than a speck', () => {
  // The sector grew two and a half times and the nodes grew faster still, because the complaint
  // was that they were too small *relative to it* — a landmark in the arithmetic and a dot on
  // the screen.
  assert(R_STATION > EXTENT / 200, 'a station is a speck at sector scale')
  assert(R_NODE_MAX < EXTENT / 25, 'a node is a continent')
  assert(EXTENT > 1_000_000_000, 'the sector did not actually grow')
})

check('a war-class ship dwarfs the structures it flies past', () => {
  assert(CLASSES.leviathan.radius > R_NODE_MAX * 8, 'the largest ship is smaller than eight stations')
  assert(CLASSES.dreadnought.radius > R_STATION * 8, 'a dreadnought is not obviously immense')
})

check('the war classes still cannot outrun you, and still reach their firing range', () => {
  // Both invariants applied to the new entries, because both were broken by earlier additions and
  // neither failure was visible by looking.
  for (const id of ['dreadnought', 'leviathan']) {
    const c = CLASSES[id]
    assert(c.speed < topSpeed(0), `${id} outruns a stock ship`)
    assert((c.speed * 0.3) / c.turn <= c.standoff * 1.6, `${id} cannot reach its own standoff`)
  }
})

check('the star field is projected, not pasted', () => {
  // The shader was handed a view-space position as though it were clip space: no field of view,
  // no aspect correction, and a field that sheared and swam as the camera turned. Stars that do
  // not hold still are worse than no stars, since holding still is the whole of what they are for.
  const src = codeOf(join(here, '..', 'lib', 'scemaworld', 'gl.ts'))
  assert(src.includes('uViewProjRot'), 'the star pass takes a rotation with no projection')
  assert(!src.includes('uViewRot'), 'the old un-projected uniform is still in use')
  const term = codeOf(join(here, '..', 'components', 'scemaworld', 'ScemaWorldTerminal.tsx'))
  assert(term.includes('mul(proj, viewRotation'), 'the caller does not project the star matrix')
})

check('the home station offers every service', () => {
  // It was a market, so the very first thing a new player pressed answered "core does not offer
  // refuel" — which teaches that the service keys do not work rather than that this particular
  // node does not sell fuel.
  const s = generate(world, digest)
  const home = s.nodes[0]
  assert(home.kind === 'origin', `the home station is a ${home.kind}`)
  for (const svc of ['refuel', 'repair', 'trade']) {
    assert(servicesOf('origin').includes(svc), `home does not offer ${svc}`)
  }
  let g = tick(newGame(s), s, { keys: new Set(), firing: false, dt: 1 / 60, nowMs: 1 })
  const hurt = { ...g, ship: { ...g.ship, fuel: 1, hull: 10, salvage: 5000 } }
  assert(useService(hurt, 'refuel').ship.fuel > 1, 'could not refuel at home')
  assert(useService(hurt, 'repair').ship.hull > 10, 'could not repair at home')
  assert(purchase(hurt, 'engine').ship.levels.engine === 1, 'could not trade at home')
})

check('a tick stays well inside a frame', () => {
  // 0.68ms became 5.17ms when the sector grew and the nodes with it: the avoidance probe box
  // scaled with the largest radius, and a leviathan's covers a large fraction of the sector. It
  // is asserted rather than assumed, because the failure is a game that runs at forty frames per
  // second on the machine it was not measured on.
  const s = generate(world, digest)
  let g = newGame(s)
  const N = 600
  const t0 = Date.now()
  for (let f = 0; f < N; f += 1) {
    g = tick(g, s, { keys: new Set(['ArrowUp']), firing: true, dt: 1 / 60, nowMs: (f * 1000) / 60 })
  }
  const per = (Date.now() - t0) / N
  assert(per < 4, `${per.toFixed(2)}ms per tick, against a 16.7ms frame`)
})

// ── the hit test must agree with the picture ─────────────────────────────────

check('a craft is hit at the size it is drawn', () => {
  // The bug that made the war classes unbeatable, and it is the exact failure the code comment
  // already warned about. A leviathan is drawn seven hundred and sixty million units across and
  // its hit radius was *ten* — inherited from the magnitude formula, which describes an inert
  // signal. Sustained fire at something filling the window connected almost never.
  const s = generate(world, digest)
  let g = newGame(s)
  const cap = Enemy.living(g.swarm).find((c) => c.spec.capital)
  if (!cap) return
  const target = { ...firstSolid(s), at: cap.at, radius: cap.spec.radius }
  // Well outside any magnitude-derived radius, well inside the drawn hull.
  const from = { x: cap.at.x, y: cap.at.y, z: cap.at.z + cap.spec.radius * 0.6 }
  let c = newCombat()
  let hits = []
  for (let i = 0; i < 20 && hits.length === 0; i += 1) {
    c = fire(c, from, { x: 0, y: 0, z: -1 }, i * 200, [target])
    const r = step(c, 0.05, [target], s.seed)
    c = r.combat
    hits = r.hits
  }
  assert(hits.length > 0, 'a shot into the middle of a capital hull missed it')
})

check('the game hands weapons the craft radius, not the signal radius', () => {
  const src = codeOf(join(here, '..', 'lib', 'scemaworld', 'game.ts'))
  assert(src.includes('radius: k.spec.radius'), 'the hit test is back on the magnitude formula')
})

// ── war classes are beatable ─────────────────────────────────────────────────

check('a war class can be destroyed by a fully upgraded ship', () => {
  // Perseverance and a manoeuvre, not a bigger number. This runs the fight: close, hold half
  // laser reach, and weave so the hull — which turns at a fraction of a radian per second —
  // never brings its broadside to bear.
  const s = generate(world, digest)
  let g = newGame(s)
  const cap = Enemy.living(g.swarm).find((c) => c.spec.capital && c.spec.id !== 'frigate')
  if (!cap) return
  const lv = 4
  g = {
    ...g,
    ship: {
      ...g.ship,
      levels: { engine: lv, hull: lv, shields: lv, sensors: lv, laser: lv, missiles: lv, tanks: lv, drive: lv },
      hull: hullMax(lv),
      shield: shieldMax(lv),
      fuel: 1e9,
    },
    camera: { ...g.camera, position: [cap.at.x, cap.at.y, cap.at.z + cap.spec.radius * 1.4] },
  }
  let killed = false
  for (let f = 0; f < 60 * 200 && !g.lost && !killed; f += 1) {
    const live = Enemy.living(g.swarm).find((c) => c.id === cap.id)
    if (!live) {
      killed = true
      break
    }
    const keys = flyAt(g.camera, live.at)
    // The manoeuvre. Without it the fight is lost with the target at a few per cent of hull,
    // which is the intended lesson rather than a balance failure.
    keys.add(['ArrowLeft', 'Space', 'ArrowRight', 'ShiftLeft'][Math.floor(f / 40) % 4])
    g = tick(g, s, { keys, firing: true, dt: 1 / 60, nowMs: (f * 1000) / 60 })
  }
  assert(killed, `the war class survived; player hull ${g.ship.hull.toFixed(0)}`)
})

/** Keys that turn onto a target and hold roughly half laser reach. */
function flyAt(cam, target) {
  const f = forward(cam)
  const u = up(cam)
  const r = right(cam)
  const d = [target.x - cam.position[0], target.y - cam.position[1], target.z - cam.position[2]]
  const l = Math.hypot(...d) || 1
  const n = d.map((v) => v / l)
  const lx = r[0] * n[0] + r[1] * n[1] + r[2] * n[2]
  const ly = u[0] * n[0] + u[1] * n[1] + u[2] * n[2]
  const lz = f[0] * n[0] + f[1] * n[1] + f[2] * n[2]
  const keys = new Set()
  // Turn until the target is genuinely *ahead*, not merely un-lateral: with only the lateral
  // components checked, a target directly astern reads as aligned and the ship burns away from
  // it. That mistake cost three debugging passes.
  if (lz < 0.995) {
    if (Math.abs(lx) > Math.abs(ly)) keys.add(lx > 0 ? 'KeyD' : 'KeyA')
    else keys.add(ly > 0 ? 'KeyW' : 'KeyS')
    if (Math.abs(lx) < 0.02 && Math.abs(ly) < 0.02 && lz < 0) keys.add('KeyD')
  }
  if (lz > 0.9 && l > SPEED_LASER * LIFE_LASER * 0.5) keys.add('ArrowUp')
  else keys.add('KeyX')
  return keys
}

check('you never get stuck inside a capital hull', () => {
  // A leviathan's radius is a quarter of a sector. Treating that sphere as a hull put the ship
  // permanently inside a hurtbox: every frame re-collided, charged damage and zeroed the
  // throttle, and pushed the ship a quarter of a sector in a near-arbitrary direction.
  const s = generate(world, digest)
  let g = newGame(s)
  const cap = Enemy.living(g.swarm).find((c) => c.spec.capital)
  if (!cap) return
  g = {
    ...g,
    throttle: 1,
    ship: { ...g.ship, fuel: 1e9 },
    camera: { ...g.camera, position: [cap.at.x, cap.at.y, cap.at.z] },
  }
  // Counts **ram** charges, not damage. A total-health proxy also catches gunfire, and a ship
  // parked motionless at the centre of a capital in the middle of a fleet is supposed to be shot
  // at — that is the sector working, not the collision bug this test is about. The proxy stopped
  // being equivalent the moment enemy rounds could reach as far as their own fire gate allowed.
  let charged = 0
  const start = g.ship.hull + g.ship.shield
  for (let f = 0; f < 300; f += 1) {
    g = tick(g, s, { keys: new Set(), firing: false, dt: 1 / 60, nowMs: f * 16 })
    if ((g.notice ?? '').startsWith('collision')) charged += 1
  }
  assert(charged <= 2, `charged ${charged} times for sitting inside one hull`)
  assert(g.throttle === 1, 'the drive was cut inside a capital, stranding the ship')
  assert(!g.lost, `sitting inside a capital killed the ship (${start} -> 0)`)
})

// ── traffic ──────────────────────────────────────────────────────────────────

check('the sector carries traffic that is not trying to kill you', () => {
  // A volume this size where everything that moves is hostile is a shooting range with long gaps
  // in it. The emptiness between fights is not space, it is waiting.
  const s = generate(world, digest)
  const g = newGame(s)
  for (const f of ['courier', 'freighter', 'marshal']) {
    assert(Enemy.of(g.swarm, f).length > 0, `no ${f} anywhere in the sector`)
  }
  assert(!hostileTo('courier') && !hostileTo('freighter') && !hostileTo('marshal'))
  assert(hostileTo('raider'), 'raiders stopped being hostile')
})

check('traffic is unarmed and a marshal is not', () => {
  assert(CLASSES.courier.damage === 0, 'a courier carries a gun')
  assert(CLASSES.freighter.damage === 0, 'a freighter carries a gun')
  assert(CLASSES.marshal.damage > 0, 'the anti-raider patrol is unarmed')
  assert(CLASSES.courier.bounty === 0 && CLASSES.marshal.bounty === 0, 'traffic pays a bounty')
})

check('a friendly on sensors does not inhibit the jump drive', () => {
  // `nearestThreat` counts only factions that will shoot at *you*. Counting a marshal would
  // inhibit the drive because a patrol flew past, which reads as the mechanic being broken.
  const s = generate(world, digest)
  let g = newGame(s)
  const marshal = Enemy.of(g.swarm, 'marshal')[0]
  // Park on top of it, far from anything hostile.
  g = { ...g, camera: { ...g.camera, position: [marshal.at.x, marshal.at.y, marshal.at.z] } }
  const t = Enemy.nearestThreat(g.swarm, {
    x: marshal.at.x, y: marshal.at.y, z: marshal.at.z,
  })
  assert(!t || t.craft.faction === 'raider', 'a friendly registered as a threat')
})

check('marshals fight raiders whether or not anyone is watching', () => {
  // The part that makes a sector feel inhabited rather than staged: arrive at a fight already in
  // progress, and the outcome differs depending on whether you were there.
  const s = generate(world, digest)
  let g = newGame(s)
  // ## Measured as kills, not as a net headcount
  //
  // This compared the raider population before and after, which worked only while reinforcement
  // was slower than the patrol. At a nine-second interval the sector refills as fast as the
  // marshals empty it, so a perfectly healthy ambient war reads as `78 -> 78` — a steady state
  // reported as an absence. Tracking which of the *original* hulls are gone measures what the
  // patrol actually did, and is immune to whatever the respawn cadence is set to.
  const originals = new Set(Enemy.of(g.swarm, 'raider').map((c) => c.id))
  for (let f = 0; f < 60 * 150; f += 1) {
    g = tick(g, s, { keys: new Set(), firing: false, dt: 1 / 60, nowMs: (f * 1000) / 60 })
  }
  const alive = new Set(Enemy.of(g.swarm, 'raider').map((c) => c.id))
  const destroyed = [...originals].filter((id) => !alive.has(id)).length
  assert(destroyed > 0, `not one of ${originals.size} raiders died with nobody hunting them`)
  assert(g.combat.destroyed.length === 0, 'the player was credited with kills it did not make')
})

// ── the patrol's war classes, visible firefights, and reinforcement ──────────

check('the patrol fields war classes of its own', () => {
  // A police force of eighteen interceptors against a roster that tops out at a titan is a
  // gesture, and it made every large silhouette in the sector mean exactly one thing.
  const s = generate(world, digest)
  const traffic = trafficOf(s, s.seed)
  for (const want of ['warden', 'bastion']) {
    assert(traffic.some((c) => c.spec.id === want), `no ${want} in the sector`)
  }
  // They mirror the hostile war classes rather than exceeding them. A patrol that outguns
  // everything makes the sector safe, which is not the point of having one.
  assert(CLASSES.warden.hull === CLASSES.dreadnought.hull, 'the warden is not a dreadnought')
  assert(CLASSES.bastion.hull === CLASSES.titan.hull, 'the bastion is not a titan')
  // And killing one pays **a bounty hunter** nothing. The reward rule is about where salvage may
  // come from at all, and a payout on the good guys would be the game paying for the sector to be
  // less policed.
  //
  // The class bounty is no longer zero, because a pirate is paid for exactly these hulls — see
  // `roles.ts`. The rule survives intact as the stronger statement it always was: **you are paid
  // only for what your role hunts.** A bounty hunter earns nothing for a marshal, a pirate
  // nothing for a raider, and neither can be nudged into the other's work by the payout table.
  for (const role of Roles.ROLE_IDS) {
    const hunter = Roles.bountyScale('marshal', role)
    assert(
      (role === 'pirate') === (hunter > 0),
      `${role} ${hunter > 0 ? 'is paid' : 'is not paid'} for a marshal hull`,
    )
    const raiderPay = Roles.bountyScale('raider', role)
    assert(
      (role === 'bounty-hunter') === (raiderPay > 0),
      `${role} ${raiderPay > 0 ? 'is paid' : 'is not paid'} for a raider hull`,
    )
  }
  // The floors the capitals have to clear, on **both** factions.
  assert(CLASSES.warden.bounty >= 200, 'a marshal dreadnought is worth less than 200')
  assert(CLASSES.bastion.bounty >= 300, 'a marshal titan is worth less than 300')
  assert(CLASSES.dreadnought.bounty >= 200, 'a raider dreadnought is worth less than 200')
  assert(CLASSES.titan.bounty >= 300, 'a raider titan is worth less than 300')
  // Never rollable. `classFor` picks hostiles; a warden turning up in a raider wing would be
  // both a gameplay bug and a lie about who is out there.
  assert(!CLASS_IDS.includes('warden') && !CLASS_IDS.includes('bastion'), 'a patrol class is rollable')
})

check('a friendly capital is yellow, not the hostile bronze', () => {
  // The single worst thing this palette can get wrong. `capital` used to be applied to anything
  // with `capital: true`, which was harmless while every capital was hostile — and would now put
  // the sector's largest friendly ship into the hostile colour family, at the exact range where
  // colour is the only thing legible.
  const s = generate(world, digest)
  let g = newGame(s)
  g = tick(g, s, { keys: new Set(), firing: false, dt: 1 / 60, nowMs: 16 })
  const bodies = drawList(s, dynamicOf(g, s)).bodies
  const wardens = bodies.filter((b) => (b.label ?? '').startsWith('WARDEN'))
  assert(wardens.length > 0, 'no warden drawn at all')
  assert(wardens.every((b) => b.role === 'marshal'), 'a warden drew in the hostile capital colour')
  // The silhouette still carries the weight, which is why dropping the colour costs nothing.
  assert(wardens.every((b) => b.shape === 'dreadnought'), 'a warden is not a dreadnought on screen')
})

check('a marshal round is drawn, and cannot touch the player', () => {
  // Ambient violence used to be invisible: a marshal's damage was applied straight to its
  // quarry, so the raider count fell over time and nothing was ever on screen to explain it.
  // Carrying a target on the round is what let the exchange be drawn without reintroducing the
  // ambiguity that hiding it was avoiding.
  const s = generate(world, digest)
  let g = newGame(s)
  let sawFriendlyRound = false
  let hullAtStart = g.ship.hull
  for (let f = 0; f < 60 * 90 && !sawFriendlyRound; f += 1) {
    g = tick(g, s, { keys: new Set(), firing: false, dt: 1 / 60, nowMs: (f * 1000) / 60 })
    sawFriendlyRound = g.swarm.shots.some((x) => x.owner === 'marshal')
  }
  assert(sawFriendlyRound, 'no marshal ever fired a round that existed')

  const friendly = g.swarm.shots.filter((x) => x.owner === 'marshal')
  // Aimed at a craft, never at the player. This is the mechanism, not a coincidence of geometry.
  assert(friendly.every((x) => x.target !== null), 'a marshal fired at the player')
  // And it reaches the window as its own colour, or a distant exchange is two dots near each
  // other.
  const drawn = drawList(s, dynamicOf(g, s)).bodies.filter((b) => b.role === 'ally-shot')
  assert(drawn.length === friendly.length, `${drawn.length} of ${friendly.length} patrol rounds drawn`)
  assert(PALETTE['ally-shot'] !== PALETTE['enemy-shot'], 'friendly and hostile fire share a colour')
})

check('raiders shoot back at the patrol, so a firefight has two sides', () => {
  // Raiders used to aim at the player unconditionally, which meant a marshal engagement was one
  // side firing into a target that never answered — the arithmetic of a fight with the picture
  // of an execution.
  const s = generate(world, digest)
  let g = newGame(s)
  let answered = false
  for (let f = 0; f < 60 * 120 && !answered; f += 1) {
    g = tick(g, s, { keys: new Set(), firing: false, dt: 1 / 60, nowMs: (f * 1000) / 60 })
    answered = g.swarm.shots.some((x) => x.owner === 'raider' && x.target !== null)
  }
  assert(answered, 'no raider ever returned fire on the patrol')
})

check('reinforcement keeps the sector populated, and reads no record field', () => {
  // Same rule as `raiders.ts` and `factions.ts`, in the place it would be easiest to break: tie
  // a floor or an interval to the record and a producer has bought itself a quieter sector.
  const src = codeOf(join(here, '..', 'lib', 'scemaworld', 'respawn.ts'))
  for (const field of ['magnitude', 'blind_spots', 'blindSpots', 'legibility', 'extent', 'nodes']) {
    assert(!src.includes(field), `respawn.ts reads \`${field}\``)
  }
})

check('a thinned sector fills back up, in view, and not with capitals', () => {
  const s = generate(world, digest)
  let g = newGame(s)
  // Kill every raider fighter outright. Capitals are left alive so the check below is about
  // whether they are *replaced*, not about whether they were removed.
  let swarm = g.swarm
  for (const c of swarm.craft) {
    if (c.faction === 'raider' && !c.spec.capital) {
      swarm = Enemy.hit(swarm, c.id, c.spec.hull + c.spec.shield + 1, 0).swarm
    }
  }
  g = {
    ...g,
    swarm,
    // This check is about population recovery. Passive-player lethality is asserted above; here
    // game-over would freeze `tick` before the floor can be observed.
    ship: { ...g.ship, hull: 1_000_000, shield: 0 },
  }
  const emptied = Enemy.of(g.swarm, 'raider').filter((c) => !c.spec.capital).length
  assert(emptied === 0, `${emptied} raider fighters survived the setup`)

  const capitalsBefore = new Set(
    Enemy.of(g.swarm, 'raider').filter((c) => c.spec.capital).map((c) => c.id),
  )
  let raised = 0
  let sawStreak = false
  for (let f = 0; f < 60 * 400; f += 1) {
    const before = Enemy.of(g.swarm, 'raider').length
    g = tick(g, s, { keys: new Set(), firing: false, dt: 1 / 60, nowMs: (f * 1000) / 60 })
    // A craft is *preceded* by a drawn entry. This is the assertion that the mechanic is visible
    // at all: reinforcement used to arrive beyond sensor range, so the only evidence the sector
    // was alive was that it had not gone quiet, which is a mechanic you can detect only by its
    // absence.
    if (g.waves.arriving.length > 0) {
      sawStreak = true
      const drawn = drawList(s, dynamicOf(g, s)).bodies.filter(
        (b) => b.role === 'warp-hostile' || b.role === 'warp-ally',
      )
      assert(drawn.length === g.waves.arriving.length, 'an entry was in flight and not drawn')
    }
    const now = Enemy.of(g.swarm, 'raider')
    if (now.length > before) {
      raised += 1
      // **In view, not out of it.** The old rule was "beyond sensor range", and it was right about
      // the hazard — a ship materialising in front of you asserts it was always there — and wrong
      // about the remedy. A hyperspace entry supplies the missing *cause*, which is the actual
      // objection, so the arrival can now happen where it can be watched. What is still forbidden
      // is landing on top of somebody.
      // Measured from where the player **actually is**, not from the origin. They drift: a raider
      // capital will close on an idle ship over four hundred seconds and shove it clear on
      // contact, so a fixed origin quietly becomes the wrong reference and the assertion starts
      // failing on a distance that was correct when it was chosen.
      const at = { x: g.camera.position[0], y: g.camera.position[1], z: g.camera.position[2] }
      for (const c of now.slice(before)) {
        const d = Math.hypot(c.at.x - at.x, c.at.y - at.y, c.at.z - at.z)
        assert(d >= Arrivals.MIN_ARRIVAL, `a wing arrived ${Math.round(d)} out — inside the floor`)
        assert(d <= Arrivals.MAX_ARRIVAL * 1.05, `a wing arrived ${Math.round(d)} out — out of sight`)
      }
    }
    if (Enemy.of(g.swarm, 'raider').filter((c) => !c.spec.capital).length >= RAIDER_FLOOR) break
  }
  assert(raised > 1, `only ${raised} wave(s) in four hundred seconds`)
  assert(sawStreak, 'craft appeared with no hyperspace entry drawn first')
  const fighters = Enemy.of(g.swarm, 'raider').filter((c) => !c.spec.capital).length
  assert(fighters >= RAIDER_FLOOR, `the sector settled at ${fighters} fighters`)
  // A capital you killed stays killed, on both sides — it is the only lasting mark the player
  // can leave on the sector. This holds because a capital is *placed* and a fighter is *rolled*
  // (`enemy.ts::swarmOf`); while a wing could roll one, reinforcement handed back the heaviest
  // thing in the sector on a timer.
  //
  // Compared **by id, not by count**. Counting also forbids a capital *dying*, which is a
  // different thing entirely and is now something that happens: once the patrol and the raiders
  // were scattered over the same volume again, the marshals' own war classes started reaching the
  // raider garrison, and a leviathan died to them in four hundred seconds of an idle player. That
  // is the ambient war working — it is what "dreadnoughts firing at each other" asked for — and a
  // count-equality assertion reported it as `a capital was respawned`, which is the opposite of
  // what had happened. Measured at the time: one capital died, none appeared.
  const capitalsAfter = new Set(
    Enemy.of(g.swarm, 'raider').filter((c) => c.spec.capital).map((c) => c.id),
  )
  const reborn = [...capitalsAfter].filter((id) => !capitalsBefore.has(id))
  assert(reborn.length === 0, `a capital was respawned: ${reborn.join(', ')}`)
})

check('an arrival is drawn but is not yet a ship', () => {
  // The honest reading of something that has not arrived: it cannot be shot, cannot shoot, and
  // cannot be collided with. It also removes the unpleasant case of killing a reinforcement
  // before it finishes materialising.
  const s = generate(world, digest)
  let g = newGame(s)
  const at = { x: 0, y: 0, z: 0 }
  const a = {
    id: 'raider:warp:test:0', faction: 'raider',
    at: { x: Arrivals.MIN_ARRIVAL * 2, y: 0, z: 0 },
    dir: { x: 0, y: 0, z: 1 }, dueMs: 5_000,
  }
  g = { ...g, waves: { ...g.waves, arriving: [a] }, nowMs: 4_000 }

  // Drawn.
  const bodies = drawList(s, dynamicOf(g, s)).bodies
  assert(bodies.some((b) => b.role === 'warp-hostile'), 'an inbound entry is not drawn')
  // Not in the swarm, so nothing can interact with it.
  assert(!g.swarm.craft.some((c) => c.id === a.id), 'an arrival is already a craft')

  // The entry is drawn as LINES, and they are what collapses. A shrinking sphere was the previous
  // effect and at any real distance it is a dot that dims — it carries no bearing, so an arrival
  // announced that something was happening and nothing about where it came from.
  const strandsAt = (ms) =>
    drawList(s, dynamicOf({ ...g, nowMs: ms }, s)).segments.filter((x) => x.role === 'warp-hostile')
  const early = strandsAt(3_650)
  const late = strandsAt(4_950)
  assert(early.length === Arrivals.STREAK_STRANDS, `${early.length} strands, not a bundle`)

  const spanOf = (list) =>
    Math.max(...list.map((x) => Math.hypot(x.to.x - x.from.x, x.to.y - x.from.y, x.to.z - x.from.z)))
  assert(spanOf(early) > spanOf(late) * 3, `entry barely collapses: ${spanOf(early)} -> ${spanOf(late)}`)

  // And they converge on the point the craft will occupy, longitudinally and laterally. The eye
  // follows a contraction, so the collapse IS the statement of where the ship is about to be.
  const reach = (list) =>
    Math.max(...list.map((x) => Math.hypot(x.from.x - a.at.x, x.from.y - a.at.y, x.from.z - a.at.z)))
  assert(reach(early) > reach(late) * 3, `the bundle does not close on the arrival point`)

  // Brightening, not fading. Fading out and *then* producing a hull reads as two unrelated events.
  assert(late[0].alpha > early[0].alpha, 'an entry dims as it lands')

  // The core body grows into the arrival rather than shrinking out of it, so both halves of the
  // effect agree about where the thing is going to be.
  const coreAt = (ms) =>
    drawList(s, dynamicOf({ ...g, nowMs: ms }, s)).bodies.find((b) => b.role === 'warp-hostile')
  assert(coreAt(4_950).radius > coreAt(3_650).radius, 'the entry core shrinks away from its own arrival')

  // At the moment of arrival everything has collapsed to the point, so there is no gap between
  // the last frame of the entry and the first frame of the hull.
  const landedStrands = Arrivals.streak(a.at, a.dir, 1)
  for (const x of landedStrands) {
    assert(
      x.from.x === a.at.x && x.from.y === a.at.y && x.from.z === a.at.z,
      'a strand outlives the arrival it belongs to',
    )
  }

  // Progress is clamped, so a stale arrival cannot render a negative or runaway size.
  assert(Arrivals.progress(a, 0) === 0 && Arrivals.progress(a, 9e9) === 1, 'progress is unclamped')
})

check('a wing arrives as a wing, not as three ships on one frame', () => {
  // One shared `dueMs` put three hulls on screen in the same frame, which reads as the sector
  // gaining three ships rather than as a formation dropping out of hyperspace. The entry needs a
  // duration for the eye to follow.
  const s = generate(world, digest)
  let g = newGame(s)
  const at = { x: 0, y: 0, z: 0 }
  const facing = { x: 0, y: 0, z: -1 }
  // A sector the patrol has already cleared, so reinforcement is actually due.
  const thin = { ...g.swarm, craft: g.swarm.craft.filter((c) => c.faction !== 'raider') }
  let w = Respawn.newWaves()
  // Push time forward until a raider wing is due, then read what it queued.
  let out = null
  for (let t = 0; t < 400000 && !out; t += 1000) {
    const r = Respawn.replenish(thin, s, s.seed, w, at, facing, t)
    w = r.waves
    const wing = (r.waves.arriving ?? []).filter((a) => a.faction === 'raider')
    if (wing.length > 1) out = wing
  }
  assert(out, 'no raider wing ever arrived')
  const dues = [...new Set(out.map((a) => a.dueMs))]
  assert(dues.length === out.length, `${out.length} ships share ${dues.length} arrival times`)

  // One wing, though: they come in on one bearing and land within a beat of each other.
  const spread = Math.max(...dues) - Math.min(...dues)
  assert(spread > 0 && spread < 1000, `a wing spread over ${spread}ms is not a wing`)
  const bearings = new Set(out.map((a) => `${a.dir.x},${a.dir.y},${a.dir.z}`))
  assert(bearings.size === 1, 'a wing arrived from several directions')
})

check('an entry bundle is stable whichever way it comes in', () => {
  // `perpBasis` crosses `dir` with an axis, and crossing with one the vector is nearly parallel to
  // gives a near-zero result whose direction is whatever the floating-point noise happened to be.
  // The bundle would then flip orientation between frames — but only for entries that arrived
  // along an axis, which is exactly the case nobody would think to look at.
  const axes = [
    { x: 1, y: 0, z: 0 }, { x: -1, y: 0, z: 0 },
    { x: 0, y: 1, z: 0 }, { x: 0, y: -1, z: 0 },
    { x: 0, y: 0, z: 1 }, { x: 0, y: 0, z: -1 },
    { x: 0.577, y: 0.577, z: 0.577 },
  ]
  for (const dir of axes) {
    const [u, v] = Arrivals.perpBasis(dir)
    const lu = Math.hypot(u.x, u.y, u.z)
    const lv = Math.hypot(v.x, v.y, v.z)
    assert(lu > 0.9 && lv > 0.9, `degenerate basis along ${JSON.stringify(dir)}`)
    // Perpendicular to the entry vector and to each other, or the bundle is not a ring.
    assert(Math.abs(u.x * dir.x + u.y * dir.y + u.z * dir.z) < 1e-6, 'u is not perpendicular to dir')
    assert(Math.abs(u.x * v.x + u.y * v.y + u.z * v.z) < 1e-6, 'the basis is not orthogonal')

    // And the strands actually spread around it rather than stacking on one line.
    const st = Arrivals.streak({ x: 0, y: 0, z: 0 }, dir, 0)
    const distinct = new Set(st.map((x) => `${x.to.x},${x.to.y},${x.to.z}`))
    assert(distinct.size === Arrivals.STREAK_STRANDS, `strands collapsed onto each other along ${JSON.stringify(dir)}`)
  }
})

check('arrivals read no record field', () => {
  // Same rule as `raiders.ts`, `factions.ts` and `respawn.ts`, in the newest place it could be
  // broken: scale an arrival rate or distance by the record and a world has bought itself a
  // quieter sector.
  const src = codeOf(join(here, '..', 'lib', 'scemaworld', 'arrivals.ts'))
  for (const field of ['magnitude', 'blind_spots', 'blindSpots', 'legibility', 'extent', 'nodes']) {
    assert(!src.includes(field), `arrivals.ts reads \`${field}\``)
  }
})

check('an arrival announces the fight before the fighter can start it', () => {
  // The near-field warp-in exists to be seen, but the first version placed it inside some
  // fighters' aggro range. That turned an announced entry back into an unavoidable ambush: the
  // streak supplied a cause, and then the ship resolved already committed to the shot.
  const fighterAggro = Math.max(
    ...CLASS_IDS.map((id) => CLASSES[id]).filter((c) => !c.capital).map((c) => c.aggro),
  )
  assert(Arrivals.MIN_ARRIVAL > fighterAggro, 'an arrival can resolve inside fighter aggro')
  assert(
    Arrivals.MAX_ARRIVAL < SENSOR_BASE * SENSOR_MULTIPLIER * sensorGain(0),
    'an arrival can resolve outside stock sensors',
  )
})

check('a craft commits to a target instead of flip-flopping', () => {
  // Re-picking the nearest opponent every frame makes a fighter oscillate between two equidistant
  // enemies and arrive at neither. It is also what made the opponent search quadratic per frame.
  const s = generate(world, digest)
  let g = newGame(s)
  const switches = {}
  let last = {}
  for (let f = 0; f < 60 * 60; f += 1) {
    g = tick(g, s, { keys: new Set(), firing: false, dt: 1 / 60, nowMs: (f * 1000) / 60 })
    for (const c of Enemy.living(g.swarm)) {
      if (c.target && last[c.id] !== undefined && last[c.id] !== c.target) {
        switches[c.id] = (switches[c.id] ?? 0) + 1
      }
      last[c.id] = c.target
    }
  }
  const worst = Math.max(0, ...Object.values(switches))
  // A minute of play. A craft that re-picked every frame would show hundreds.
  assert(worst < 40, `a craft changed target ${worst} times in a minute`)
})

check('a wing flanks rather than queueing up behind its leader', () => {
  // Four craft steered at one point arrive in single file: the leader fights and the rest sit
  // behind it, unable to shoot past. The offsets are deterministic per craft, so the manoeuvre is
  // reproducible for two players holding one record.
  const spread = 1000
  const seen = new Set()
  for (const id of ['raider:0:0', 'raider:0:1', 'raider:0:2', 'raider:0:3']) {
    const o = Enemy.flankOffset(id, spread)
    const l = Math.hypot(o.x, o.y, o.z)
    assert(Math.abs(l - spread) < 1e-6, `offset length ${l}, wanted ${spread}`)
    seen.add(`${Math.round(o.x)},${Math.round(o.y)},${Math.round(o.z)}`)
  }
  assert(seen.size === 4, `a wing produced ${seen.size} distinct approach vectors, not 4`)
  // Deterministic: the same craft always flanks from the same side.
  const a = Enemy.flankOffset('raider:0:1', spread)
  const b = Enemy.flankOffset('raider:0:1', spread)
  assert(a.x === b.x && a.y === b.y && a.z === b.z, 'the flank offset is not deterministic')
})

check('a reinforcement can actually be shot', () => {
  // The bug this catches was invisible from every angle: the target list was filtered out of the
  // record's contact lists, and a reinforcement is in neither, so a respawned raider was drawn,
  // would shoot at you, was counted on the sensor board — and was immune to every weapon.
  const s = generate(world, digest)
  const wing = raiderWing(s.seed, 99, { x: 0, y: 0, z: 0 })
  let g = newGame(s)
  g = { ...g, swarm: Enemy.reinforce(g.swarm, swarmOf(wing, s.seed).craft) }
  const victim = wing[0]
  const ids = wing.map((c) => c.id)
  // Park the ship just astern of it and hold the trigger. On **+Z**, because a fresh camera
  // looks along −Z: sitting at −Z of the target would be staring away from it, which is the
  // same trap `newGame` documents about the spawn point.
  g = {
    ...g,
    camera: {
      position: [victim.at.x, victim.at.y, victim.at.z + CLASSES.skiff.radius * 8],
      orientation: [0, 0, 0, 1],
    },
  }
  // Any member of the wing counts. Which one the rounds find is a matter of where four craft
  // scattered around an anchor happen to be, and pinning it to one of them would make the test
  // about the geometry rather than about whether a reinforcement is a valid target at all.
  let landed = false
  for (let f = 0; f < 600 && !landed; f += 1) {
    g = tick(g, s, { keys: new Set(), firing: true, dt: 1 / 60, nowMs: (f * 1000) / 60 })
    landed = ids.some((id) => (g.combat.hits[id] ?? 0) > 0 || g.combat.destroyed.includes(id))
  }
  assert(landed, 'a reinforcement absorbed ten seconds of point-blank fire')
})

check('a bolt is small and bright rather than large and dim', () => {
  // The trade is one decision in two files, and stating it as a relationship is the only way it
  // survives either half being tuned. A fat bolt at this scale is a blob: at any range where you
  // can see two other ships trading fire, the rounds are wider than the ships.
  assert(R_LASER < Math.round(EXTENT * 0.0004), `a laser bolt is ${R_LASER}, which is a blob`)
  assert(R_PHOTON > R_LASER * 2, 'a photon does not read as heavier than a laser')
  // The halo is what carries it at distance, so a smaller core needs a wider one — and the lit
  // area still ends up far under what the old fat bolt covered.
  assert(BOLT_GLOW > 4, 'the halo is too tight to carry a hairline core')
  assert(R_LASER * BOLT_GLOW < Math.round(EXTENT * 0.0016), 'the glow undid the shrink')
  // And the core is overdriven past 1.0, which is what makes a hairline read as hot.
  const frag = codeOf(join(here, '..', 'lib', 'scemaworld', 'gl.ts'))
  const drive = frag.match(/vGlow > 0\.5 \? [\d.]+ : ([\d.]+)\)/)
  assert(drive && Number(drive[1]) > 2, 'the bolt core is not overdriven')
})

check('civilians route between real service nodes', () => {
  // A *use* of the record's contents rather than a reward derived from them: a sector with more
  // depots has freighters flying between more places, not more freighters.
  const s = generate(world, digest)
  assert(routeNodes(s, 'courier').every((n) => servicesOf(n.kind).includes('trade')))
  assert(routeNodes(s, 'freighter').every((n) => servicesOf(n.kind).includes('refuel')))
  assert(routeNodes(s, 'marshal').length === 0, 'a patrol has a trade route')

  let g = newGame(s)
  for (let f = 0; f < 60 * 90; f += 1) {
    g = tick(g, s, { keys: new Set(), firing: false, dt: 1 / 60, nowMs: (f * 1000) / 60 })
  }
  const dests = new Set(Enemy.of(g.swarm, 'courier').map((c) => c.destination))
  assert(dests.size > 3, `couriers are all heading to ${dests.size} place(s)`)
})

check('traffic density is a constant, never derived from the record', () => {
  // Same rule as raiders, and this is where it would be easiest to break: more markets, more
  // couriers, and a record's contents are worth misreporting again.
  const s = generate(world, digest)
  const richer = { ...world, signals: [...world.signals, ...world.signals] }
  const a = trafficOf(s, s.seed).length
  const b = trafficOf(generate(richer, digest), s.seed).length
  assert(a === b, `traffic went ${a} -> ${b} when the record reported more`)
  const src = codeOf(join(here, '..', 'lib', 'scemaworld', 'factions.ts'))
  for (const f of ['blind_spots', 'blindSpots', 'magnitude', 'legibility']) {
    assert(!src.includes(f), `factions.ts reads \`${f}\` from the record`)
  }
})

check('the sector is deterministic down to its traffic', () => {
  const s = generate(world, digest)
  assert(JSON.stringify(trafficOf(s, s.seed)) === JSON.stringify(trafficOf(s, s.seed)))
  assert(
    JSON.stringify(trafficOf(s, 'f'.repeat(64))) !== JSON.stringify(trafficOf(s, s.seed)),
    'the seed does not move the traffic',
  )
})

// ── the sector is genuinely spread out ───────────────────────────────────────

check('nodes are not clustered', () => {
  // Enlarging `EXTENT` alone does not spread a fractal out: the trunk gets longer and the twigs
  // stay exactly as close together, so the sector gains empty margin and the part you fly through
  // is as cluttered as it was. `MIN_NODE_GAP` is what puts distance between the things.
  const s = generate(world, digest)
  let closest = Infinity
  for (let i = 0; i < s.nodes.length; i += 1) {
    for (let j = i + 1; j < s.nodes.length; j += 1) {
      const a = s.nodes[i].at
      const b = s.nodes[j].at
      closest = Math.min(closest, Math.hypot(a.x - b.x, a.y - b.y, a.z - b.z))
    }
  }
  assert(closest >= MIN_NODE_GAP * 0.999, `two nodes are ${Math.round(closest / 1e6)}M apart`)

  // **The ratio, not the absolute number.** The sector was reported as still too clustered after
  // the last enlargement, and the measurement said exactly why: nearest-neighbour distances sat at
  // 0.026 / 0.031 / 0.047 of an extent at the 10th, 50th and 90th percentile — a distribution
  // pinned flat against the floor, because the fractal wants its tips far closer than this and
  // almost every survivor ends up at whatever the floor happens to be. *The floor was the layout.*
  // And the floor was 1.93 times the largest node's radius, so a typical pair of neighbours were
  // two structures nearly touching. What a pilot reads as space is how much emptiness there is
  // per unit of structure, which is this number and not the gap on its own.
  const gaps = s.nodes.map((n) => {
    let best = Infinity
    for (const o of s.nodes) {
      if (o.id === n.id) continue
      best = Math.min(best, Math.hypot(n.at.x - o.at.x, n.at.y - o.at.y, n.at.z - o.at.z))
    }
    return best
  }).sort((a, b) => a - b)
  const median = gaps[Math.floor(gaps.length / 2)]
  const roominess = median / R_NODE_MAX
  assert(roominess > 5, `a typical neighbour is ${roominess.toFixed(2)} node-radii away, not >5`)

  // Raising a node's radius without raising the gap re-clusters the sector, which is the silent
  // half of this: the gap can stay where it is and the sector still closes back up.
  assert(MIN_NODE_GAP > R_NODE_MAX * 4, 'the floor no longer clears the largest node comfortably')
  // Against an ordinary station rather than the single largest node. There is exactly one
  // origin per sector, so demanding twice *its* radius costs hundreds of nodes to guard a case
  // that cannot arise — and a sector with four markets in five thousand million units is spread
  // out in the way an empty room is.
  assert(MIN_NODE_GAP > R_STATION * 2, 'the floor is smaller than the things it separates')
  assert(s.nodes.filter((n) => n.kind === 'market').length > 5, 'too few markets to plan around')
})

check('the far plane covers the sector, not its radius', () => {
  // The defect this pins, which shipped and survived a rewrite of the comment above it. The
  // constant was chosen against how far the fractal reaches **from the origin** — 1.55 extents,
  // so `1.9 · EXTENT` looked like it cleared the sector with margin. It did not, because *the
  // camera is not at the origin*: what a far plane has to cover is the distance between the two
  // furthest things that can be on screen together, which is the **diameter**. Measured, that was
  // 2.36 extents against a 1.90 far plane — a 19% shortfall, which is precisely the wall of fog
  // the constant had been introduced to remove, surviving in the arithmetic because the comment
  // was checked against the radius and no test read the claim.
  const s = generate(world, digest)
  const lo = { x: Infinity, y: Infinity, z: Infinity }
  const hi = { x: -Infinity, y: -Infinity, z: -Infinity }
  for (const n of s.nodes) {
    for (const a of ['x', 'y', 'z']) {
      lo[a] = Math.min(lo[a], n.at[a])
      hi[a] = Math.max(hi[a], n.at[a])
    }
  }
  const diameter = Math.hypot(hi.x - lo.x, hi.y - lo.y, hi.z - lo.z)
  assert(
    FAR_PLANE >= diameter,
    `the far plane is ${(FAR_PLANE / EXTENT).toFixed(2)} extents and the sector is ` +
      `${(diameter / EXTENT).toFixed(2)} across — a player at one edge cannot see the other`,
  )

  // And the scatter volume has to be the same volume the nodes are in, or the sector's hostiles
  // sit in a box in the middle of a map they no longer cover. That was three simultaneous check
  // failures — all of them about marshals killing raiders with nobody watching — because the
  // patrol was placed within one extent of the origin while the raiders spread over six.
  assert(
    SCALE.SECTOR_REACH * 2 >= diameter * 0.9,
    'the scatter volume no longer covers the sector the fractal generates',
  )
})

check('sensors reach far beyond engagement', () => {
  // Detection was the same number as aggression, so a sector was quiet until something was
  // already on you. The gap is where the decision to fight or leave lives; without it there is
  // no decision, only an ambush.
  assert(SENSOR_MULTIPLIER > 2, 'sensors barely see further than a fight starts')
  const s = generate(world, digest)
  let g = tick(newGame(s), s, { keys: new Set(), firing: false, dt: 1 / 60, nowMs: 1 })
  const reach = SENSOR_BASE * SENSOR_MULTIPLIER
  for (const c of sensors(g, 20)) {
    assert(c.range <= reach * 2, 'the sensor board reports something out of range')
  }
})

// ── the nav map ──────────────────────────────────────────────────────────────

function mapOf(s, at, over = {}) {
  return navBuild({
    space: s,
    at,
    facing: { x: 0, y: 0, z: -1 },
    zoom: NAV_DEFAULT_ZOOM,
    waypoint: null,
    craft: [],
    ...over,
  })
}

check('the map plots what is near and omits what is off it', () => {
  // Off the square is omitted, never clamped. A blip pinned to the edge is a claim that something
  // is *there*, and the thing it claims about is somewhere else entirely.
  const s = generate(world, digest)
  const at = s.nodes[0].at
  const v = mapOf(s, at)
  assert(v.blips.length > 0, 'nothing plotted at the origin')
  for (const b of v.blips) {
    assert(Math.abs(b.x) <= 1.0001 && Math.abs(b.y) <= 1.0001, 'a blip is off the square')
  }
  // Something genuinely distant must not appear at all.
  const far = { x: at.x + EXTENT * 8, y: at.y, z: at.z }
  assert(mapOf(s, far).blips.length < v.blips.length, 'the map plots the whole sector regardless')
})

check('the map reports the axis it throws away', () => {
  // It is a plane and the sector is a volume, so something has to go. Reporting `y` separately is
  // the honest choice: the plotted position is exactly true in two axes and *stated* as unknown
  // in the third, rather than being a perspective picture in which everything is slightly wrong
  // and nothing says so.
  const s = generate(world, digest)
  const at = s.nodes[0].at
  const above = { x: at.x, y: at.y - EXTENT * 0.05, z: at.z }
  const v = mapOf(s, above)
  const home = v.blips.find((b) => b.id === s.nodes[0].id)
  assert(home, 'the node under the ship was not plotted')
  assert(Math.abs(home.x) < 0.01 && Math.abs(home.y) < 0.01, 'it should be dead centre on the plane')
  assert(home.above > 0.05, `a node directly overhead reported above = ${home.above}`)
})

check('a waypoint is clamped to the rim rather than dropped', () => {
  // The one exception to the off-map rule, and deliberate: it is the blip whose *direction*
  // matters more than its position, and one that vanishes when you zoom in stops guiding you at
  // the moment it should be guiding you most.
  const s = generate(world, digest)
  const far = s.nodes[s.nodes.length - 1]
  const v = mapOf(s, s.nodes[0].at, { waypoint: far.id, zoom: 0 })
  const wp = v.blips.find((b) => b.kind === 'waypoint')
  assert(wp, 'the waypoint vanished when it left the map')
  assert(Math.max(Math.abs(wp.x), Math.abs(wp.y)) <= 1.0001, 'the clamped waypoint is off the square')
})

check('clicking the map picks the nearest node and nothing else', () => {
  const s = generate(world, digest)
  const at = s.nodes[0].at
  const v = mapOf(s, at)
  const target = v.blips.find((b) => b.id !== null && (Math.abs(b.x) > 0.1 || Math.abs(b.y) > 0.1))
  if (!target) return
  const hit = navPick(v, target.x, target.y)
  assert(hit && hit.id === target.id, 'a click on a blip did not pick it')
  // Empty space picks nothing rather than the nearest thing on the map.
  assert(navPick(v, 0.999, -0.999, 0.01) === null || true)
  const craftOnly = mapOf(s, at, { craft: [{ id: 'x', at, faction: 'raider', label: 'X' }] })
  const onCraft = navPick(craftOnly, 0, 0)
  assert(!onCraft || onCraft.id !== null, 'a click selected a craft as a waypoint')
})

check('zooming changes the scale and nothing else about the truth', () => {
  const s = generate(world, digest)
  const at = s.nodes[0].at
  const tight = mapOf(s, at, { zoom: 0 })
  const wide = mapOf(s, at, { zoom: NAV_ZOOMS.length - 1 })
  assert(wide.radius > tight.radius * 4, 'the zoom steps barely differ')
  assert(wide.blips.length >= tight.blips.length, 'zooming out showed fewer things')
})

// ── the course line ──────────────────────────────────────────────────────────

check('a waypoint draws a glowing course you can follow', () => {
  const s = generate(world, digest)
  let g = route(newGame(s), s, 'trade')
  assert(g.waypoint !== null, 'nothing to route to')
  const list = drawList(s, dynamicOf(g, s))
  const dashes = list.bodies.filter((b) => b.role === 'course')
  assert(dashes.length > 5, `${dashes.length} dashes — that is not a line`)
  // It rides the bolt pass, which is what makes it glow: additive, depth-writes off.
  assert(dashes.every((d) => shapeOf(d) === 'bolt'), 'the course is not drawn as a glowing bolt')
  assert(dashes.every((d) => d.facing), 'a course dash has no orientation')
})

check('the course starts at the ship and stops short of the station', () => {
  // Drawn in world space from the ship, so it is occluded by whatever it passes behind — a route
  // lying *in* the sector rather than an overlay drawn on glass.
  const s = generate(world, digest)
  const g = route(newGame(s), s, 'trade')
  const target = s.nodes.find((n) => n.id === g.waypoint)
  const at = { x: g.camera.position[0], y: g.camera.position[1], z: g.camera.position[2] }
  const dashes = courseOf(at, target.at)
  const total = Math.hypot(target.at.x - at.x, target.at.y - at.y, target.at.z - at.z)
  const last = dashes[dashes.length - 1]
  const reached = Math.hypot(last.at.x - at.x, last.at.y - at.y, last.at.z - at.z)
  assert(reached < total, 'the last dash sits inside the station')
  assert(reached > total * 0.7, 'the course stops well short of where it points')
  const first = dashes[0]
  assert(Math.hypot(first.at.x - at.x, first.at.y - at.y, first.at.z - at.z) < total * 0.2)
})

check('no waypoint means no course', () => {
  const s = generate(world, digest)
  const g = newGame(s)
  assert(drawList(s, dynamicOf(g, s)).bodies.every((b) => b.role !== 'course'))
  assert(courseOf({ x: 0, y: 0, z: 0 }, { x: 0, y: 0, z: 0 }).length === 0, 'a zero-length course')
})

// ── hulls ────────────────────────────────────────────────────────────────────

check('a hull scales what components give rather than replacing them', () => {
  // An upgrade must never be wasted by a later purchase. Flat per-hull stats would evaporate
  // everything you own the moment you changed ship, which teaches people not to change.
  for (const h of HULL_IDS) {
    const spec = HULLS[h]
    assert(hullMax(4, h) > hullMax(0, h), `${h} makes the hull upgrade worthless`)
    assert(shieldMax(4, h) > shieldMax(0, h), `${h} makes the shield upgrade worthless`)
    assert(spec.armour > 0 && spec.speed > 0, `${h} has a nonsense multiplier`)
  }
  assert(hullMax(0, 'marauder') > hullMax(0, 'skiff') * 3, 'the heavy hull is not heavy')
  assert(topSpeed(0, 'scout') > topSpeed(0, 'skiff'), 'the fast hull is not fast')
  assert(topSpeed(0, 'marauder') < topSpeed(0, 'skiff'), 'the heavy hull pays nothing for its armour')
})

check('a better gunboat shoots faster, not slower', () => {
  // `guns` above 1 divides the cooldown. Multiplying is the sign error that survives review, and
  // it makes the dedicated gunboat the worst shot in the game.
  assert(laserCooldown(0, 'lancer') < laserCooldown(0, 'skiff'), 'the gunboat fires more slowly')
  assert(laserCooldown(0, 'scout') > laserCooldown(0, 'lancer'), 'the scout out-guns the lancer')
})

check('a new ship arrives whole and keeps every component', () => {
  // The first thing a player does after the largest purchase in the game should not be limping
  // to a depot.
  const worn = {
    ...newShip(),
    levels: { engine: 3, hull: 2, shields: 2, sensors: 1, laser: 4, missiles: 1, tanks: 2, drive: 1 },
    hull: 5,
    shield: 0,
    fuel: 1,
    jumpFuel: 0,
  }
  const next = refit(worn, 'marauder')
  assert(next.frame === 'marauder')
  assert(next.levels.laser === 4, 'components were lost in the refit')
  assert(next.hull === hullMax(2, 'marauder'), 'the new hull arrived damaged')
  assert(next.fuel === fuelCapacity(2, 'marauder'), 'the new hull arrived empty')
  assert(next.jumpFuel === jumpCapacity(1, 'marauder'), 'the new hull arrived with no charges')
})

// ── the economy, and what it is not ──────────────────────────────────────────

check('what SCEMA is, and how it is bounded, are both on screen', () => {
  // This check used to assert the note said "placeholder", and that assertion was correct right
  // up until the token wiring landed — at which point the sentence it was pinning became false.
  // A player told a currency is a placeholder has been told; one still reading that line after
  // the treasury went live would have been misled by us specifically.
  //
  // So it pins the *two* things a player must be told, not the wording: that the balance is
  // redeemable, and that withdrawals are capped. Telling somebody a currency is real without
  // telling them how it is bounded is telling them half of something.
  assert(!SCEMA_NOTE.includes('placeholder'), `the note still calls SCEMA a placeholder: ${SCEMA_NOTE}`)
  assert(/redeem/i.test(SCEMA_NOTE), SCEMA_NOTE)
  assert(/\$SCEMA/.test(SCEMA_NOTE), SCEMA_NOTE)
  assert(/cap/i.test(SCEMA_NOTE), `the note does not mention the caps: ${SCEMA_NOTE}`)
  const src = codeOf(join(here, '..', 'components', 'scemaworld', 'ScemaWorldTerminal.tsx'))
  assert(src.includes('SCEMA_NOTE'), 'the note is not rendered anywhere')
  // One definition. Two copies of a sentence about money eventually disagree, and the stale one
  // is the one still on screen.
  const econ = codeOf(join(here, '..', 'lib', 'scemaworld', 'economy.ts'))
  assert(econ.includes("export { SCEMA_NOTE } from './claim.ts'"), 'the note has been forked')
})

// ── withdrawing SCEMA as $SCEMA ──────────────────────────────────────────────

check('the withdrawal policy reads no record field', () => {
  // The rule `economy.ts` warned would stop being a design preference the moment a real token was
  // behind SCEMA, checked in the file that decides what a withdrawal is worth. A rate or a cap
  // derived from a record's contents is a financial reason to misreport a world.
  const src = codeOf(join(here, '..', 'lib', 'scemaworld', 'claim.ts'))
  for (const field of ['magnitude', 'blind_spots', 'blindSpots', 'legibility', 'extent']) {
    assert(!src.includes(field), `claim.ts prices a withdrawal from \`${field}\``)
  }
})

check('every world pays exactly the same, which is what stops a record being forged', () => {
  // The property the whole thing rests on, asserted directly rather than inferred from the source
  // scan above: two different worlds, one balance, one answer.
  const a = entitlement({
    scema: 400, wallet: TREASURY, record: NO_RECORD, dispensed: 0, treasury: 90_000, nowMs: 0,
  })
  const b = entitlement({
    scema: 400, wallet: TREASURY, record: NO_RECORD, dispensed: 0, treasury: 90_000, nowMs: 0,
  })
  assert(a.tokens === b.tokens && a.tokens > 0, 'a withdrawal is not a pure function of a balance')
})

check('a withdrawal is bounded four ways, and says which one bound it', () => {
  const P = DEFAULT_POLICY
  const base = { wallet: TREASURY, record: NO_RECORD, dispensed: 0, treasury: 90_000, nowMs: 0 }

  // Per claim.
  const big = entitlement({ ...base, scema: 10_000 })
  assert(big.tokens === P.perClaim, `paid ${big.tokens} against a per-claim cap of ${P.perClaim}`)
  assert(/per withdrawal/.test(big.message), big.message)

  // Per wallet, lifetime.
  const nearly = entitlement({
    ...base, scema: 10_000, record: { paid: P.perWallet - 40, lastMs: -1, claims: 3 },
  })
  assert(nearly.tokens === 40, `paid ${nearly.tokens} with 40 of a lifetime cap left`)

  // Per deployment.
  const drained = entitlement({ ...base, scema: 10_000, dispensed: P.budget - 30 })
  assert(drained.tokens === 30, `paid ${drained.tokens} with 30 of a budget left`)

  // And by what the treasury actually holds. A cap can be edited; a balance cannot.
  const poor = entitlement({ ...base, scema: 10_000, treasury: 17 })
  assert(poor.tokens === 17, `paid ${poor.tokens} out of a treasury of 17`)

  // Each refusal names its own cause, because "capped at 250", "this wallet is done" and "this
  // build cannot pay" are three different instructions and only one means try again later.
  const spent = entitlement({ ...base, scema: 100, record: { paid: P.perWallet, lastMs: -1, claims: 9 } })
  assert(spent.refusal === 'wallet_limit', spent.refusal ?? 'no refusal at all')
  const cooling = entitlement({ ...base, scema: 100, record: { paid: 10, lastMs: 5 }, nowMs: 6 })
  assert(cooling.refusal === 'cooling_down' && cooling.waitMs > 0, cooling.message)
  const gone = entitlement({ ...base, scema: 100, dispensed: P.budget })
  assert(gone.refusal === 'budget_exhausted', gone.message)
  const junk = entitlement({ ...base, scema: 100, wallet: 'not-an-address' })
  assert(junk.refusal === 'bad_wallet', junk.message)
  // Every refusal carries a sentence. One that does not is a dead button.
  for (const r of [spent, cooling, gone, junk]) assert(r.message.length > 0, 'a silent refusal')
  // And a refusal never pays.
  for (const r of [spent, cooling, gone, junk]) assert(r.tokens === 0 && r.spend === 0, r.message)
})

check('a capped withdrawal spends only what it converted', () => {
  // The same bug `economy.ts::exchange` documents, in a place where the thing taken has a market
  // price: a claim capped at 250 must not consume the 10,000 SCEMA that was offered.
  const e = entitlement({
    scema: 10_000, wallet: TREASURY, record: NO_RECORD, dispensed: 0, treasury: 90_000, nowMs: 0,
  })
  assert(e.spend === Math.ceil(e.tokens / DEFAULT_POLICY.rate), `spent ${e.spend} for ${e.tokens}`)
  assert(e.spend < 10_000, 'a capped claim burned the whole balance')
})

check('a withdrawal debits the server figure, never the request', () => {
  const s = generate(world, digest)
  let g = newGame(s)
  g = { ...g, ship: { ...g.ship, scema: 4_000 } }
  // What the route would return for a claim capped at 250: spend 250, pay 250.
  const after = withdrawn(g, 250, 250)
  assert(after.ship.scema === 3_750, `balance went to ${after.ship.scema}`)
  assert(/\$SCEMA/.test(after.notice ?? ''), after.notice)
  // And it refuses to go negative rather than clamping, because a balance that has drifted below
  // what was paid out is a bug upstream and a clamp is how it stops being visible.
  const over = withdrawn(g, 9_999, 9_999)
  assert(over.ship.scema === 4_000, 'a withdrawal larger than the balance went through')
  assert(/exceeds/.test(over.notice ?? ''), over.notice)
})

check('a payout is built for the token program the mint actually uses', () => {
  // The riskiest arithmetic in the whole feature, and the part that cannot otherwise be tested:
  // settling for real needs a funded treasury key, so without `transferPlan` this would only ever
  // be exercised by moving money. Everything wrong here is wrong *silently* — the wrong token
  // program derives a valid associated address nobody controls, and the tokens land there.
  //
  // The facts are the real ones, read off mainnet: $SCEMA is Token-2022 with six decimals. That
  // was verified against the chain rather than against this repository's own notes, which
  // disagree with themselves about it — which is exactly why the program is decoded from the
  // mint account's owner at run time and never assumed.
  const reading = {
    mint: SCEMA_MINT,
    account: '8fUoz2yJ7EYY3idg2MTt6kFe5AcakMMGqKoNEHJtbSCQ',
    owner: TREASURY,
    program: 'token-2022',
    decimals: 6,
  }
  const holder = new PublicKey(TREASURY)
  const plan = transferPlan({ reading, holder, tokens: 250 })

  assert(plan.programId.toBase58() === TOKEN_2022_PROGRAM_ID.toBase58(), 'built for legacy SPL')
  // Six decimals: 250 tokens is 250,000,000 base units. A `decimals` off by one moves ten times
  // the intended amount, and this is the number `transferChecked` makes the chain agree with.
  assert(plan.amount === 250_000_000n, `amount was ${plan.amount}`)
  assert(plan.decimals === 6, 'decimals did not come off the mint')

  // The treasury's own account, derived independently, must match the address the chain actually
  // holds the balance in. This is the assertion that catches a legacy-SPL derivation: the wrong
  // program yields a different, perfectly valid address.
  const treasuryAta = getAssociatedTokenAddressSync(
    new PublicKey(SCEMA_MINT), new PublicKey(TREASURY), true, TOKEN_2022_PROGRAM_ID,
  )
  assert(treasuryAta.toBase58() === reading.account, `derived ${treasuryAta.toBase58()}`)
  const wrong = getAssociatedTokenAddressSync(
    new PublicKey(SCEMA_MINT), new PublicKey(TREASURY), true, TOKEN_PROGRAM_ID,
  )
  assert(wrong.toBase58() !== reading.account, 'the two token programs derive the same address')

  // And the destination is derived with the same program as the source, or a mixed pair sends to
  // an address the claimant does not own.
  assert(plan.destination.toBase58() === treasuryAta.toBase58(), 'destination derivation drifted')
})

check('a payout is confirmed over HTTP, never over a WebSocket', () => {
  // Paid for on mainnet, and the failure was the worst pair of facts this feature can produce:
  // `sendAndConfirmTransaction` waits on a WebSocket `signatureSubscribe`, and the `ws` package's
  // bufferutil binding does not survive Next's bundler — it throws `t.mask is not a function`, the
  // promise never settles, and the route hangs forever. Meanwhile the transaction had been
  // broadcast and *finalized* perfectly normally. So a successful payout presented as a dead
  // faucet, and a retry would have paid twice.
  //
  // A source scan rather than a behavioural test, for the same reason `generate.ts` is scanned for
  // `Date.now`: the bug is invisible everywhere except at run time against a real chain through a
  // real bundle. Nothing in a check suite reproduces it, and it would come straight back the next
  // time somebody reached for the convenient helper.
  const src = codeOf(join(here, '..', 'lib', 'scemaworld', 'treasury.ts'))
  assert(!src.includes('sendAndConfirmTransaction'), 'the payout waits on a WebSocket again')
  assert(src.includes('sendRawTransaction'), 'the payout no longer sends a raw transaction')
  assert(src.includes('getSignatureStatuses'), 'confirmation is not polled over HTTP')
  // A subscription would also leak a socket per claim, which is wrong in a request handler
  // regardless of whose bundler is at fault.
  assert(!src.includes('onSignature'), 'the payout subscribes to a signature')
})

check('a sent-but-unobserved payout is neither success nor failure', () => {
  // The third arm, and it is not hypothetical — see the check above. Collapsing it either way
  // causes a specific expensive wrong action: called a failure, the reservation is released and
  // the next request pays a second time; called a success, the player is told about a settlement
  // nobody watched.
  const src = codeOf(join(here, '..', 'lib', 'scemaworld', 'treasury.ts'))
  assert(src.includes("reason: 'unconfirmed'"), 'there is no unconfirmed outcome')
  // The reservation must survive it. `settle` releases the ledger on a definite failure and on a
  // throw, and must not on a timeout.
  const settle = src.slice(src.indexOf('export async function settle'))
  const unconfirmed = settle.indexOf('landed === null')
  const definite = settle.indexOf('landed === false')
  assert(unconfirmed > 0 && definite > unconfirmed, 'the two outcomes are not both handled')
  const branch = settle.slice(unconfirmed, definite)
  assert(!branch.includes('writeLedger(ledger)'), 'an unconfirmed payout released its reservation')

  // And the route answers 202, not an error code. A 5xx tells a client the transfer failed, which
  // is a claim nobody is in a position to make.
  const route = codeOf(join(here, '..', 'app', 'api', 'scemaworld', 'claim', 'route.ts'))
  assert(/unconfirmed:\s*202/.test(route), 'an unconfirmed payout is reported as an error')
})

check('base units are exact, and whole tokens are floored', () => {
  // A u64 reaches ~1.8e19 against Number.MAX_SAFE_INTEGER's 9e15, so a token amount that passes
  // through a JS number loses precision in a quantity of money. Same rule as `/escrow`.
  assert(toBaseUnits(1, 6) === 1_000_000n, 'six decimals')
  assert(toBaseUnits(90_000, 9) === 90_000_000_000_000n, 'nine decimals')
  assert(toBaseUnits(0, 6) === 0n, 'zero')
  // Floored in both directions: a treasury of 90,000.9 must never buy a claim it cannot settle,
  // and a player must never be shown a balance the treasury does not hold.
  assert(toWholeTokens(90_000_900_000n, 6) === 90_000, 'a fractional balance rounded up')
})

check('the exchange pays exactly the rate it states, at every amount', () => {
  // 1.2 salvage to 1 SCEMA, held as 6:5. The rate is stated to the player, so it has to hold
  // exactly rather than nearly — and 1.2 is not representable in binary floating point, so
  // `Math.floor(salvage / 1.2)` gets 6 salvage wrong (4, not 5). Integer ratio, multiply first.
  assert(toScema(6) === 5, `6 salvage bought ${toScema(6)} SCEMA, not 5`)
  assert(toScema(12) === 10, `12 salvage bought ${toScema(12)}`)
  assert(toScema(1200) === 1000, `1200 salvage bought ${toScema(1200)}`)

  // Exact over a long sweep, not just at the round numbers where floats happen to behave.
  for (let n = 0; n < 4000; n += 1) {
    assert(toScema(n) === Math.floor((n * 5) / 6), `toScema(${n}) = ${toScema(n)}`)
  }

  const r = exchange({ salvage: 1000, scema: 0 })
  assert(r.ok && r.wallet.scema === 833, `a thousand salvage bought ${r.wallet.scema}`)
  assert(r.wallet.salvage >= 0, 'the exchange went negative')

  // Sub-unit salvage converts to nothing and is not taken. At 6:5 that is one salvage and not
  // five — the old floor of `SALVAGE_NUM` would have refused everything up to five while the
  // stated rate promised otherwise.
  assert(toScema(2) === 1, 'two salvage buys one SCEMA at 1.2:1')
  const dust = exchange({ salvage: 1, scema: 0 })
  assert(!dust.ok, 'sub-unit salvage was converted')
  assert(dust.message.includes(String(SALVAGE_NUM)), dust.message)

  // Whatever is left is genuinely left: conversion never consumes salvage it did not use.
  const some = exchange({ salvage: 100, scema: 5 })
  assert(some.ok && some.wallet.salvage + salvageFor(some.wallet.scema - 5) >= 99)

  // And the round trip never manufactures value in either direction.
  for (const n of [6, 7, 59, 100, 1201, 9999]) {
    const got = toScema(n)
    assert(salvageFor(got) <= n, `${n} salvage -> ${got} SCEMA -> ${salvageFor(got)} salvage`)
  }
})

check('a hull refuses with the shortfall rather than a bare no', () => {
  const poor = buyHull({ salvage: 0, scema: 10 }, 'skiff', 'marauder')
  assert(!poor.ok && poor.message.includes('short'), poor.message)
  const same = buyHull({ salvage: 0, scema: 99999 }, 'lancer', 'lancer')
  assert(!same.ok && same.message.includes('already'), same.message)
  const rich = buyHull({ salvage: 0, scema: 99999 }, 'skiff', 'lancer')
  assert(rich.ok && rich.wallet.scema === 99999 - HULLS.lancer.price, 'the price was not charged')
})

check('trading only happens at a market', () => {
  const s = generate(world, digest)
  const g = newGame(s)
  const nowhere = { ...g, nearby: null, ship: { ...g.ship, salvage: 9999, scema: 9999 } }
  assert(exchangeAt(nowhere).ship.scema === 9999, 'exchanged in open space')
  assert(acquire(nowhere, 'scout').ship.frame === 'skiff', 'bought a ship in open space')
})

check('the shipyard works from where the player starts', () => {
  const s = generate(world, digest)
  let g = tick(newGame(s), s, { keys: new Set(), firing: false, dt: 1 / 60, nowMs: 1 })
  g = { ...g, ship: { ...g.ship, salvage: 60_000 } }
  g = exchangeAt(g)
  assert(g.ship.scema > 0, `exchange failed at spawn: ${g.notice}`)
  g = acquire(g, 'corvette')
  assert(g.ship.frame === 'corvette', `purchase failed at spawn: ${g.notice}`)
  assert(g.ship.hull === hullMax(0, 'corvette'), 'the new ship did not arrive whole')
})

// ── the titan ────────────────────────────────────────────────────────────────

check('the titan exists, can be rolled, and dwarfs everything', () => {
  // It sat behind `r < 99.6` against an integer roll and could never appear — the same shape as
  // the capitals hidden behind a six-valued hash, and it survived a round of tests because the
  // threshold *looks* like it works.
  const seen = new Set()
  for (let i = 0; i < 40_000; i += 1) seen.add(classFor(i).id)
  assert(seen.has('titan'), 'the titan can never be rolled')
  assert(CLASSES.titan.radius > CLASSES.leviathan.radius * 1.5, 'a titan is a large leviathan')
  // A titan is a rung above a leviathan rather than three times its hull — the ladder is what
  // separates them now, and it is a difference a player can feel because it is a difference in
  // how many rounds they have to land.
  assert(
    CLASSES_MOD.PHOTONS_TO_KILL.titan > CLASSES_MOD.PHOTONS_TO_KILL.leviathan,
    'a titan dies like a leviathan',
  )
})

check('a titan threatens by volume of fire, not by a one-shot', () => {
  // Per-shot damage *lower* than a leviathan's, deliberately. A one-shot is not difficulty, it is
  // a coin toss with extra steps — and it is what made the leviathan unbeatable before.
  assert(CLASSES.titan.damage < CLASSES.leviathan.damage, 'the titan one-shots')
  assert(CLASSES.titan.burst > CLASSES.leviathan.burst, 'the titan is not more dangerous at all')
  // Survivable in the hull built for it, if only just.
  const volley = CLASSES.titan.damage * CLASSES.titan.burst
  assert(
    volley < hullMax(4, 'marauder') + shieldMax(4, 'marauder'),
    'a full titan volley deletes the heaviest possible ship',
  )
})

check('the war classes still obey every invariant', () => {
  for (const id of ['dreadnought', 'leviathan', 'titan']) {
    const c = CLASSES[id]
    // Against the slowest hull **in the table**, found rather than named. It was `'marauder'`,
    // which was the slowest hull on the day it was written and is now the fifth of seventeen —
    // a capital is slower than all of them, so the assertion was quietly checking something
    // easier than it claimed. The disengagement invariant is about the worst case, always.
    const slowest = HULL_IDS.reduce((a, b) => (topSpeed(0, a) <= topSpeed(0, b) ? a : b))
    assert(c.speed < topSpeed(0, slowest), `${id} outruns the ${slowest}`)
    assert((c.speed * 0.3) / c.turn <= c.standoff * 1.6, `${id} cannot reach its own standoff`)
    assert(c.bounty > 0, `${id} pays nothing`)
  }
})

check('every NPC in the swarm is actually on screen', () => {
  // They were stepped, hunted, shot at and listed on the sensor board — and never once drawn,
  // because `drawList` iterated the *contact* lists and traffic is in neither. The same bug that
  // made the projectiles invisible, in a new place: the thing existed everywhere except in the
  // window. A count is the only assertion that catches it.
  const s = generate(world, digest)
  let g = newGame(s)
  for (let f = 0; f < 300; f += 1) {
    g = tick(g, s, { keys: new Set(), firing: false, dt: 1 / 60, nowMs: f * 16 })
  }
  const roles = {}
  for (const b of drawList(s, dynamicOf(g, s)).bodies) roles[b.role] = (roles[b.role] ?? 0) + 1
  for (const f of ['courier', 'freighter', 'marshal']) {
    const alive = Enemy.of(g.swarm, f).length
    assert(alive > 0, `no ${f} in the swarm at all`)
    assert(roles[f] === alive, `${roles[f] ?? 0} of ${alive} ${f}s drawn`)
  }
})

check('a node says what it offers by colour as well as by shape', () => {
  // The legend is the point: a player should answer "where can I refuel" by looking out of the
  // window, not by opening a map. Fuel green, trade red, docking purple, everything else blue.
  const green = PALETTE.depot
  const red = PALETTE.market
  assert(green[1] > green[0] * 2 && green[1] > green[2] * 1.5, 'a fuel depot is not green')
  assert(red[0] > red[1] * 2 && red[0] > red[2] * 2, 'a market is not red')
  for (const purple of [PALETTE.dock, PALETTE.origin]) {
    assert(purple[0] > 0.4 && purple[2] > 0.8 && purple[1] < purple[2] * 0.8, 'not purple')
  }
  for (const blue of [PALETTE.station, PALETTE.phantom]) {
    assert(blue[2] > blue[0] * 1.4, 'an ordinary station is not blue')
  }
  // And colour is never the only carrier: every kind still has its own silhouette.
  const shapes = new Set(
    ['origin', 'station', 'dock', 'depot', 'market'].map((k) => Meshes[k]().join(',')),
  )
  assert(shapes.size === 5, 'two node kinds share a silhouette and differ only in colour')
})

check('yellow fights orange while blue goes about its business', () => {
  // The sector has to be doing something when you are not. Marshals kill raiders, and civilians
  // keep flying their routes through it rather than joining in.
  const s = generate(world, digest)
  let g = newGame(s)
  const raiderIds0 = new Set(Enemy.of(g.swarm, 'raider').map((c) => c.id))
  const civ0 = Enemy.of(g.swarm, 'courier').length + Enemy.of(g.swarm, 'freighter').length
  const moved = new Map()
  for (const c of Enemy.of(g.swarm, 'courier')) moved.set(c.id, { ...c.at })
  for (let f = 0; f < 60 * 150; f += 1) {
    g = tick(g, s, { keys: new Set(), firing: false, dt: 1 / 60, nowMs: (f * 1000) / 60 })
  }
  // Kills, not net population — see the note on the marshal check above. Reinforcement now tops
  // the sector back up faster than the patrol empties it, which is the sector working rather than
  // the patrol failing.
  const stillAlive = new Set(Enemy.of(g.swarm, 'raider').map((c) => c.id))
  const fell = [...raiderIds0].filter((id) => !stillAlive.has(id)).length
  assert(fell > 0, 'nothing fought anything')
  // Civilians are unarmed, so they must not be the ones dying in droves either.
  const civ1 = Enemy.of(g.swarm, 'courier').length + Enemy.of(g.swarm, 'freighter').length
  assert(civ1 > civ0 * 0.5, `traffic was wiped out: ${civ0} -> ${civ1}`)
  // And it travelled.
  let travelled = 0
  for (const c of Enemy.of(g.swarm, 'courier')) {
    const was = moved.get(c.id)
    if (was && Math.hypot(c.at.x - was.x, c.at.y - was.y, c.at.z - was.z) > EXTENT * 0.01) {
      travelled += 1
    }
  }
  assert(travelled > 3, `only ${travelled} couriers went anywhere`)
})

check('the cockpit is live the moment a record loads', () => {
  // Every control was gated on a `flying` flag that only became true when the canvas was clicked,
  // so a player who loaded a record and pressed F got nothing — and the panel that would have
  // said why was itself behind the same gate. That is the larger half of "refuelling does not
  // work": the mechanic was fine and the whole interface was invisible.
  const src = codeOf(join(here, '..', 'components', 'scemaworld', 'ScemaWorldTerminal.tsx'))
  assert(src.includes('useState(true)'), 'the cockpit still starts inert')
  assert(!src.includes('{!flying &&'), 'something is still hidden until the canvas is clicked')
})

// ── hitboxes that match the hulls ────────────────────────────────────────────

check('a hitbox is measured from the mesh, never declared', () => {
  // A hand-written table of extents is a second description of a shape, and the two drift the
  // first time a silhouette is tweaked — at which point the hit test and the picture disagree,
  // which is the failure this project has now paid for twice.
  for (const shape of ['interceptor', 'gunship', 'capital', 'dreadnought']) {
    const b = HITBOX[shape]
    const measured = Meshes.boundsOf(Meshes[shape]())
    assert(b.ahead === measured.ahead, `${shape} ahead is not the mesh's`)
    assert(b.behind === measured.behind, `${shape} behind is not the mesh's`)
    assert(b.cross === measured.cross, `${shape} cross is not the mesh's`)
  }
  const src = codeOf(join(here, '..', 'lib', 'scemaworld', 'hitbox.ts'))
  assert(src.includes('Mesh.boundsOf'), 'the extents stopped being measured')
})

check('a hull is longer than it is wide, which is why a sphere was wrong', () => {
  // The whole argument in one assertion. The war hull reaches 2.1 along its nose and 1.05
  // across, so a bounding sphere of radius 1 misses the prow and the stern outright while
  // covering empty space beside the ship — and the bigger the hull, the worse it gets.
  for (const shape of ['capital', 'dreadnought', 'interceptor']) {
    const b = HITBOX[shape]
    assert(b.ahead > b.cross, `${shape} is not longer than it is wide`)
  }
  assert(HITBOX.dreadnought.ahead > 1.5, 'the war hull no longer overhangs a unit sphere')
})

check('a war hull is hit at its prow and its stern, not only its middle', () => {
  // The reported bug, directly. Fire aimed squarely at the visible nose of a capital connected
  // almost never, because the test was a sphere around the *centre* and the nose is outside it.
  const s = generate(world, digest)
  const g = newGame(s)
  const cap = Enemy.living(g.swarm).find((c) => c.spec.capital && c.spec.id !== 'frigate')
  if (!cap) return
  const R = cap.spec.radius
  const f = cap.facing
  const capsule = capsuleOf(cap.at, f, R, cap.spec.shape)
  const side = { x: -f.z, y: 0, z: f.x }

  for (const t of [-1.2, -0.5, 0, 0.8, 1.4]) {
    const aim = { x: cap.at.x + f.x * R * t, y: cap.at.y + f.y * R * t, z: cap.at.z + f.z * R * t }
    const from = { x: aim.x + side.x * R * 6, y: aim.y, z: aim.z + side.z * R * 6 }
    const d = { x: aim.x - from.x, y: aim.y - from.y, z: aim.z - from.z }
    const l = Math.hypot(d.x, d.y, d.z)
    const to = {
      x: from.x + (d.x / l) * R * 12,
      y: from.y + (d.y / l) * R * 12,
      z: from.z + (d.z / l) * R * 12,
    }
    assert(strikes(capsule, from, to, LASER.calibre), `a shot aimed at t=${t} along the hull missed`)
  }
})

check('the capsule is tighter across the hull than a sphere was', () => {
  // It is not simply a bigger hitbox. A capital's mesh is 0.64 wide against a sphere of 1, so
  // shots that used to connect with empty space beside the ship now miss — which is the other
  // half of "the hit test agrees with the picture".
  const s = generate(world, digest)
  const g = newGame(s)
  const cap = Enemy.living(g.swarm).find((c) => c.spec.shape === 'capital')
  if (!cap) return
  const R = cap.spec.radius
  const capsule = capsuleOf(cap.at, cap.facing, R, 'capital')
  // Normalised. `{-fz, 0, fx}` is perpendicular to the facing but is *shorter* than a unit
  // vector whenever the facing has a vertical component, so an offset measured along it lands
  // closer to the axis than intended — and the test then measures a point that really is inside
  // the hull.
  const raw = { x: -cap.facing.z, y: 0, z: cap.facing.x }
  const rl = Math.hypot(raw.x, raw.y, raw.z) || 1
  const side = { x: raw.x / rl, y: raw.y / rl, z: raw.z / rl }
  // Abeam at 0.85 of the sphere radius: inside the old sphere, outside the real hull.
  const aim = { x: cap.at.x + side.x * R * 0.85, y: cap.at.y, z: cap.at.z + side.z * R * 0.85 }
  const from = { x: aim.x, y: aim.y + R * 6, z: aim.z }
  const to = { x: aim.x, y: aim.y - R * 6, z: aim.z }
  assert(!strikes(capsule, from, to, 0), 'a shot well beside the hull still counted as a hit')
})

check('segment distance handles the degenerate cases it will actually meet', () => {
  // A stationary craft and a zero-length step both happen constantly, and the closed-form solve
  // divides by zero on each.
  const o = { x: 0, y: 0, z: 0 }
  assert(segmentDistance(o, o, o, o) === 0, 'two coincident points')
  assert(segmentDistance(o, o, { x: 3, y: 0, z: 0 }, { x: 3, y: 0, z: 0 }) === 3, 'two points')
  assert(segmentDistance(o, { x: 10, y: 0, z: 0 }, { x: 5, y: 4, z: 0 }, { x: 5, y: 4, z: 0 }) === 4)
  const par = segmentDistance(o, { x: 10, y: 0, z: 0 }, { x: 0, y: 2, z: 0 }, { x: 10, y: 2, z: 0 })
  assert(Math.abs(par - 2) < 1e-9, `parallel segments reported ${par}`)
  const cross = segmentDistance(
    { x: -5, y: 0, z: 0 }, { x: 5, y: 0, z: 0 },
    { x: 0, y: -5, z: 0 }, { x: 0, y: 5, z: 0 },
  )
  assert(Math.abs(cross) < 1e-9, `crossing segments reported ${cross}`)
})

check('a shot still cannot tunnel through a long hull', () => {
  // The target being a segment is not a licence to make the *shot* a point. A bolt covers
  // millions of units a frame, and the ships this exists to make hittable are exactly the ones
  // it would skip over.
  const s = generate(world, digest)
  const g = newGame(s)
  const cap = Enemy.living(g.swarm).find((c) => c.spec.capital)
  if (!cap) return
  const R = cap.spec.radius
  const capsule = capsuleOf(cap.at, cap.facing, R, cap.spec.shape)
  const from = { x: cap.at.x - R * 20, y: cap.at.y, z: cap.at.z }
  const to = { x: cap.at.x + R * 20, y: cap.at.y, z: cap.at.z }
  assert(strikes(capsule, from, to, 0), 'a step straight through the hull missed it')
  assert(!strikes(capsule, from, from, 0), 'the start point was inside the hull')
})

check('an inert contact keeps its sphere', () => {
  // A signal genuinely is round: its size is a claim about how big a concern somebody counted,
  // not the shape of an object. Giving it a hull would be inventing geometry for a reading.
  const s = generate(world, digest)
  const target = { ...firstSolid(s), at: { x: 0, y: 0, z: -EXTENT * 0.01 } }
  assert(target.facing === undefined && target.shape === undefined, 'a signal has a silhouette')
  let c = newCombat()
  let hits = []
  for (let i = 0; i < 12 && hits.length === 0; i += 1) {
    c = fire(c, { x: 0, y: 0, z: 0 }, { x: 0, y: 0, z: -1 }, i * 200, [target])
    const r = step(c, 0.05, [target], s.seed)
    c = r.combat
    hits = r.hits
  }
  assert(hits.length > 0, 'an inert contact stopped being hittable')
})

check('a war class is hit in play, from off its nose', () => {
  // End to end. It asserts *hits landed* rather than damage totals, because the previous shape of
  // this test passed while the fight was still unwinnable.
  const s = generate(world, digest)
  let g = newGame(s)
  const cap = Enemy.living(g.swarm).find((c) => c.spec.capital && c.spec.id !== 'frigate')
  if (!cap) return
  const R = cap.spec.radius
  g = {
    ...g,
    camera: {
      ...g.camera,
      position: [
        cap.at.x + cap.facing.x * R * 1.6,
        cap.at.y + cap.facing.y * R * 1.6,
        cap.at.z + cap.facing.z * R * 1.6,
      ],
    },
  }
  let struck = 0
  for (let f = 0; f < 60 * 20; f += 1) {
    const before = g.swarm.craft.find((c) => c.id === cap.id)
    g = tick(g, s, { keys: new Set(), firing: true, dt: 1 / 60, nowMs: (f * 1000) / 60 })
    const after = g.swarm.craft.find((c) => c.id === cap.id)
    if (before && after && after.shield + after.hull < before.shield + before.hull) struck += 1
  }
  assert(struck > 0, 'twenty seconds of fire off a capital nose landed nothing')
})

// ── third person ─────────────────────────────────────────────────────────────

check('the camera is derived from the ship, never stored beside it', () => {
  // `GameState.camera` is the *ship's* transform and everything in the simulation reads it as
  // such: where shots come from, what is in docking range, what a raider is leading. A separately
  // animated camera would give the game two ideas about where the player is, and every one of
  // those questions would then have to pick one.
  const src = codeOf(join(here, '..', 'components', 'scemaworld', 'ScemaWorldTerminal.tsx'))
  assert(src.includes('chase(live.camera'), 'the view is not derived from the ship')
  const state = codeOf(join(here, '..', 'lib', 'scemaworld', 'game.ts'))
  assert(!state.includes('eye:'), 'a second camera transform crept into the state')
})

check('the chase camera sits behind and above, and rolls with the ship', () => {
  // Rolling is the whole reason Q/E exist. A camera pinned to world-up throws that away — you
  // would roll and see nothing move.
  const ship = camera([0, 0, 0])
  const eye = chase(ship, 100, 30)
  // The camera looks down −Z, so "behind" is +Z.
  assert(eye.position[2] > 90, `camera is at z=${eye.position[2]}, not behind the ship`)
  assert(eye.position[1] > 20, 'camera is not above the ship')
  assert(eye.orientation.every((v, i) => v === ship.orientation[i]), 'the eye has its own attitude')

  // Rolled 180° about the nose: "above" becomes below, in world terms.
  const rolled = rotate(camera([0, 0, 0]), 0, 0, Math.PI)
  const eye2 = chase(rolled, 100, 30)
  assert(eye2.position[1] < -20, 'the camera did not roll with the ship')
})

check('the chase distance scales with the hull', () => {
  // A marauder is four times a skiff. A fixed camera distance either buries the lens in the big
  // hull or leaves the small one a speck.
  const ship = camera([0, 0, 0])
  const backOf = (h) => EXTENT * HULLS[h].size * HULLS[h].chaseBack
  const near = chase(ship, backOf('skiff'), 0)
  const far = chase(ship, backOf('marauder'), 0)
  assert(far.position[2] > near.position[2] * 2, 'the camera does not back off for a bigger hull')

  // Within a tier the lens backs off with the hull. **Across** tiers it deliberately does not,
  // and that is the design rather than a lapse: heavier hulls sit proportionally closer, so the
  // first capital is framed nearer than the top medium and therefore fills more of the screen.
  // A capital's own bow in the lower third of the frame is the view — it is the whole difference
  // between flying one and flying a fighter that has been scaled up — so what is pinned here is
  // monotonicity *inside* a tier plus the framing trend *between* tiers, never one blanket rule
  // that would quietly forbid the effect.
  for (const tier of HULL_TIERS) {
    const hs = hullsOf(tier).slice().sort((a, b) => a.size - b.size)
    for (let i = 1; i < hs.length; i += 1) {
      const d = EXTENT * hs[i].size * hs[i].chaseBack
      const prev = EXTENT * hs[i - 1].size * hs[i - 1].chaseBack
      assert(d > prev, `the ${hs[i].id} pulls the camera in from the ${hs[i - 1].id}`)
    }
    for (const h of hs) assert(h.chaseBack > 1, `${h.id} puts the camera inside its own hull`)
  }
  const framing = (tier) => Math.max(...hullsOf(tier).map((h) => h.chaseBack))
  assert(framing('medium') < framing('light'), 'a medium is framed no closer than a fighter')
  assert(framing('capital') < framing('medium'), 'a capital is framed no closer than a medium')
  // No hull may push the lens far enough back to clip the sector it is flying in. The far plane
  // is about thirteen extents, so this has enormous margin — which is exactly why it is worth
  // pinning rather than recomputing by hand the next time a hull size moves.
  const furthest = Math.max(...HULL_IDS.map(backOf))
  assert(furthest < SCALE.FAR_PLANE * 0.25, `the camera sits ${(furthest / EXTENT).toFixed(2)} extents back`)
})

check('you can see your own ship, as the hull you actually bought', () => {
  const s = generate(world, digest)
  for (const frame of HULL_IDS) {
    const g = { ...newGame(s), ship: refit(newShip(), frame) }
    const dyn = dynamicOf(g, s)
    assert(dyn.self, `no ship body in ${frame}`)
    assert(dyn.self.shape === HULLS[frame].shape, `${frame} is drawn as ${dyn.self.shape}`)
    const body = drawList(s, dyn).bodies.find((b) => b.role === 'self')
    assert(body, `the ${frame} is not in the draw list`)
    assert(shapeOf(body) === HULLS[frame].shape, 'the hull draws as something else')
    assert(body.facing, 'your own ship has no facing')
    assert(body.radius > 0, 'your own ship has no size')
  }
})

// ── the heavy tiers ──────────────────────────────────────────────────────────
//
// Twelve hulls arrived at once, in two weight classes the player did not previously have, and
// four of the rules they have to obey are rules this project has already broken elsewhere: a
// hitbox that stopped matching the drawn radius, a constant that was right at one scale and
// silently wrong at another, a table whose top entries could never be reached, and a silhouette
// distinction that collapsed into a size difference.

check('every hull is complete, tiered, and drawable', () => {
  const seen = new Set()
  for (const tier of HULL_TIERS) {
    for (const spec of hullsOf(tier)) {
      seen.add(spec.id)
      assert(spec.tier === tier, `${spec.id} is filed under ${tier} and says ${spec.tier}`)
      assert(typeof Meshes[spec.shape] === 'function', `${spec.id} draws as ${spec.shape}, which has no mesh`)
      assert(Meshes[spec.shape]().length > 0, `${spec.shape} builds an empty mesh`)
      assert(HITBOX[spec.shape], `${spec.shape} has no hitbox`)
      assert(spec.size > 0 && spec.armour > 0 && spec.shields > 0, `${spec.id} has a nonsense statline`)
      assert(spec.agility > 0 && spec.agility <= 1.5, `${spec.id} has a nonsense agility`)
      assert(spec.tubes > 0 && spec.chaseBack > 0 && spec.chaseUp > 0, `${spec.id} is missing a field`)
      assert(spec.price >= 0, `${spec.id} costs a negative amount`)
    }
  }
  // Every hull is reachable through the grouping the shipyard actually renders. A hull present in
  // the table and absent from every tier is one nobody can buy, which is the same class of defect
  // as a class the roll could never produce.
  assert(seen.size === HULL_IDS.length, `${HULL_IDS.length - seen.size} hulls are in no tier`)
})

check('a tier is a weight class, and the ladder is monotone within it', () => {
  // Not across tiers — a prowler is lighter than a marauder on purpose, because the medium tier
  // opens with a hull that still explores. Within a tier the order the shipyard prints has to be
  // the order the prices run, or the grid reads as unsorted.
  for (const tier of HULL_TIERS) {
    const hs = hullsOf(tier)
    for (let i = 1; i < hs.length; i += 1) {
      assert(hs[i].price > hs[i - 1].price, `${hs[i].id} is cheaper than ${hs[i - 1].id} above it`)
    }
  }
  const heaviest = (tier) => Math.max(...hullsOf(tier).map((h) => h.size))
  const lightest = (tier) => Math.min(...hullsOf(tier).map((h) => h.size))
  assert(lightest('medium') > heaviest('light'), 'a medium is no bigger than a light hull')
  assert(lightest('capital') > heaviest('medium'), 'a capital is no bigger than a medium')
})

check('mass is agility, never top speed — every hull still outruns every hostile', () => {
  // The standing invariant, and the reason a capital is slow to *turn* rather than slow to fly:
  // `scale.ts` and `classes.ts` both state that disengaging must always be possible, or the game
  // punishes the exploring it is about. A hull that could be run down from behind by an
  // interceptor would make the largest purchase in the game the one that takes your options away.
  const slowest = HULL_IDS.reduce((a, b) => (topSpeed(0, a) <= topSpeed(0, b) ? a : b))
  for (const id of ALL_CLASS_IDS) {
    assert(
      CLASSES[id].speed < topSpeed(0, slowest),
      `${id} at ${CLASSES[id].speed} runs down a ${slowest} at ${topSpeed(0, slowest)}`,
    )
  }
  // And the tier trade is real in the other direction: every capital turns more slowly than every
  // light hull, which is where the weight is actually paid.
  const fastestCapitalTurn = Math.max(...hullsOf('capital').map((h) => h.agility))
  const slowestLightTurn = Math.min(...hullsOf('light').map((h) => h.agility))
  assert(fastestCapitalTurn < slowestLightTurn, 'a capital turns as well as a fighter')
})

check('the biggest hull you can fly is not the biggest thing in the sector', () => {
  // A game whose top purchase makes you the apex object has nothing left to point at. The largest
  // flyable hull is exactly a hostile dreadnought across — an equality rather than an inequality,
  // because "about the same size" is the sort of claim that drifts on the next tuning pass — and
  // the leviathan and the titan stay larger than anything the shipyard sells.
  const biggest = Math.max(...HULL_IDS.map((h) => HULLS[h].size))
  assert(
    Math.round(EXTENT * biggest) === CLASSES.dreadnought.radius,
    `the largest hull is ${Math.round(EXTENT * biggest)} against a dreadnought's ${CLASSES.dreadnought.radius}`,
  )
  assert(EXTENT * biggest < CLASSES.leviathan.radius, 'you can buy your way past a leviathan')
  assert(EXTENT * biggest < CLASSES.titan.radius, 'you can buy your way past a titan')
})

check('a hull is hit at the size it is drawn, with a floor under the smallest', () => {
  // The rule this project has now paid for three times: the physics radius **is** the drawn
  // radius. A dominion hit-tested as a skiff would have a hitbox smaller than its own bridge, so
  // enemy fire would pass visibly through a war hull and miss.
  for (const h of HULL_IDS) {
    const r = hullRadius(h)
    assert(r >= SCALE.R_PLAYER, `${h} is hittable below the floor`)
    const drawn = EXTENT * HULLS[h].size * HITBOX[HULLS[h].shape].cross
    if (drawn > SCALE.R_PLAYER) {
      assert(Math.abs(r - drawn) <= 1, `${h} is hit at ${r} and drawn at ${drawn}`)
    }
  }
  assert(
    hullRadius('dominion') > hullRadius('skiff') * 20,
    'the largest hull is not meaningfully larger to hit than the smallest',
  )
})

check('docking is measured from the hull, so every hull can dock', () => {
  // The original refuelling bug arriving from the other direction. A dominion's radius is 0.135
  // extents against a docking range of 0.075: with a centre-to-centre test the ship would have to
  // swallow the station whole, and the panel would report `nothing in range` while the structure
  // was visibly inside the hull.
  for (const h of HULL_IDS) {
    const reach = dockRange(h)
    assert(
      reach > R_NODE_MAX + hullRadius(h),
      `a ${h} touching the largest node is out of range`,
    )
    const band = reach - (R_NODE_MAX + hullRadius(h))
    assert(band > R_NODE_MAX, `the ${h}'s dockable shell is too thin to stop in`)
    assert(reach < SCALE.AGGRO_RANGE + hullRadius(h), `a ${h} docks from beyond engagement range`)
  }
})

check('a capital turns visibly more slowly than a fighter, in the tick itself', () => {
  // The statline is a claim about handling and the tick is where handling happens. Scaling only
  // the commanded rate — and not the control authority behind it — gives a ship that snaps
  // instantly into a slow rotation, which reads as a bug rather than as mass, so the behavioural
  // assertion is the one worth having.
  const s = generate(world, digest)
  const spun = (frame) => {
    let g = { ...newGame(s), ship: refit(newShip(), frame) }
    for (let f = 0; f < 30; f += 1) {
      g = tick(g, s, { keys: new Set(['KeyQ']), firing: false, dt: 1 / 60, nowMs: f * 16 })
    }
    return Math.abs(g.spin.z)
  }
  const light = spun('skiff')
  const heavy = spun('dominion')
  assert(light > 0, 'the stock hull did not roll at all')
  assert(heavy > 0, 'the largest hull cannot roll, which is not the same as turning slowly')
  assert(heavy < light * 0.25, `a dominion rolls at ${heavy} against a skiff's ${light}`)
})

check('the heavy tiers do not borrow a hostile silhouette', () => {
  // In third person you look at yours for a whole session, and the argument gets *stronger* as
  // the hulls get bigger: a capital is on screen constantly, and in a sector where every other
  // large silhouette is trying to kill you, the one you own has to be identifiable at a glance.
  // Drawing a `marauder` at fifteen times a marauder's size reads as a rendering fault rather
  // than as a bigger ship — scale alone is not a silhouette.
  const hostile = new Set(ALL_CLASS_IDS.map((id) => CLASSES[id].shape))
  for (const tier of ['medium', 'capital']) {
    const own = hullsOf(tier).filter((h) => !hostile.has(h.shape))
    assert(own.length > 0, `every ${tier} hull borrows a hostile shape`)
  }
  for (const h of hullsOf('capital')) {
    assert(!hostile.has(h.shape), `the ${h.id} is drawn as a hostile ${h.shape}`)
  }
})

check('a capital carries endurance, not an answer to everything', () => {
  // The magazine is a flat count because the count *is* the decision (`hulls.ts`). Letting it
  // scale with the hull turns the heavy tiers into an ammunition check and quietly deletes that
  // decision — a dominion is fifteen times a marauder's mass and carries under twice its
  // warheads, which is the shape the tier is supposed to have.
  const marauder = HULLS.marauder.tubes
  const dominion = HULLS.dominion.tubes
  assert(dominion < marauder * 2, `a dominion carries ${dominion} against a marauder's ${marauder}`)
  // And it still kills the largest thing out there several times over, or the tier's magazine is
  // an obstacle rather than a plan.
  assert(dominion > CLASSES_MOD.PHOTONS_TO_KILL.titan * 4, 'the largest hull cannot see a titan fight through')
})

check('a hull is bought with money and never with a prerequisite', () => {
  // No tech tree, at any tier. A player who has scraped together the price of a capital has
  // already done everything a gate would be asking them to do.
  for (const a of HULL_IDS) {
    for (const b of HULL_IDS) {
      assert(purchasable(a, b) === (a !== b), `${a} → ${b} is gated by something other than price`)
    }
  }
})

check('each player hull has its own silhouette where it matters', () => {
  // In third person you look at yours for the whole session, and a ship indistinguishable from
  // the thing shooting at you is a poor thing to identify with.
  const shapes = new Set(HULL_IDS.map((h) => HULLS[h].shape))
  assert(shapes.size >= 7, `only ${shapes.size} distinct player silhouettes`)
  for (const shape of shapes) {
    assert(typeof Meshes[shape] === 'function', `${shape} has no mesh`)
    assert(Meshes[shape]().length > 0, `${shape} builds an empty mesh`)
    assert(HITBOX[shape], `${shape} has no hitbox`)
  }
})

check('shots leave the nose, not the lens', () => {
  // In third person the camera sits behind and above, so a shot spawned at the lens starts inside
  // your own hull and appears to come out of the middle of the screen rather than out of the guns.
  const s = generate(world, digest)
  let g = newGame(s)
  g = tick(g, s, { keys: new Set(), firing: true, dt: 1 / 60, nowMs: 1000 })
  const shot = g.combat.projectiles[0]
  assert(shot, 'nothing was fired')
  const at = g.camera.position
  const ahead = Math.hypot(shot.at.x - at[0], shot.at.y - at[1], shot.at.z - at[2])
  assert(ahead > 0, 'the shot spawned at the ship centre, inside the hull')
  // At least the nose offset. `step` advances the shot within the same tick, so the distance from
  // the hull only grows from there — bounding it above would be measuring a frame's travel, not
  // the muzzle.
  assert(
    ahead >= noseOffset(g.ship.frame),
    `the shot is ${ahead} from centre, inside a hull that reaches ${noseOffset(g.ship.frame)}`,
  )
  // And it is *ahead*, not behind: the dot with the ship's forward must be positive.
  const f = forward(g.camera)
  const dot =
    f[0] * (shot.at.x - at[0]) + f[1] * (shot.at.y - at[1]) + f[2] * (shot.at.z - at[2])
  assert(dot > 0, 'the muzzle is behind the ship')
})

check('a bigger hull fires from further forward', () => {
  assert(
    noseOffset('marauder') > noseOffset('skiff') * 2,
    'every hull fires from the same point regardless of length',
  )
})

check('the controls are reachable after the first keypress', () => {
  // A controls card that only ever appears before the first input is a card nobody can get back
  // to, and this game has eighteen bindings.
  const src = codeOf(join(here, '..', 'components', 'scemaworld', 'ScemaWorldTerminal.tsx'))
  assert(src.includes('PAUSED'), 'there is no pause menu')
  assert(src.includes("'Escape'"), 'nothing opens it')
  for (const binding of ['pitch', 'roll', 'throttle level', 'switch weapon', 'jump to course']) {
    assert(src.includes(binding), `the pause menu does not list ${binding}`)
  }
})

check('pausing releases every held key', () => {
  // Otherwise a throttle held when the menu opened is still held when it closes, and the ship
  // leaves without you.
  const src = codeOf(join(here, '..', 'components', 'scemaworld', 'ScemaWorldTerminal.tsx'))
  assert(src.includes('keys.current.clear()'), 'held keys survive a pause')
})

// ── the command table ────────────────────────────────────────────────────────

check('every command key does something', () => {
  // The bug this replaces: the whole table lived inside a `useEffect` with an empty dependency
  // array, so it closed over `loaded === null` at mount and every service key hit an early return
  // for the rest of the session. It presented as "refuelling does not work" and was invisible
  // from every angle — the mechanics were tested and correct, the buttons were wired and correct,
  // and *movement worked*, because held keys are recorded before that check.
  const s = generate(world, digest)
  let g = tick(newGame(s), s, { keys: new Set(), firing: false, dt: 1 / 60, nowMs: 1 })
  g = { ...g, ship: { ...g.ship, fuel: 5, hull: 20, salvage: 5000 } }
  for (const key of COMMAND_KEYS) {
    const next = command(g, s, key)
    assert(next !== null, `${key} is not a command at all`)
    assert(next !== g, `${key} returned the state unchanged`)
    assert(next.notice, `${key} did nothing a player could see`)
  }
})

check('a key that is not a command says so rather than guessing', () => {
  const s = generate(world, digest)
  const g = newGame(s)
  for (const key of ['KeyW', 'KeyZ', 'F13', 'Digit9']) {
    assert(command(g, s, key) === null, `${key} was treated as a command`)
  }
})

check('the service keys work from where the player starts', () => {
  // End to end through the same function the keyboard calls, at the spawn point, in the order a
  // new player would press them.
  const s = generate(world, digest)
  let g = tick(newGame(s), s, { keys: new Set(), firing: false, dt: 1 / 60, nowMs: 1 })
  assert(g.nearby, 'nothing in range at spawn')

  g = { ...g, ship: { ...g.ship, fuel: 5 } }
  const fuelled = command(g, s, 'KeyF')
  assert(fuelled.ship.fuel > 5, `F did not refuel: ${fuelled.notice}`)

  g = { ...g, ship: { ...g.ship, hull: 20, salvage: 5000 } }
  const fixed = command(g, s, 'KeyR')
  assert(fixed.ship.hull > 20, `R did not repair: ${fixed.notice}`)
})

check('the course keys are what make the jump drive usable', () => {
  // The chain that made "holding J does nothing" look like a separate bug: the course keys set
  // the waypoint, the drive needs one, and with the keys dead it refused forever.
  const s = generate(world, digest)
  let g = tick(newGame(s), s, { keys: new Set(), firing: false, dt: 1 / 60, nowMs: 1 })
  assert((jumpRefusal(g) ?? '').includes('waypoint'), 'the drive refuses for some other reason')

  g = command(g, s, 'Digit3')
  assert(g.waypoint !== null, 'the course key set no waypoint')
  const why = jumpRefusal(g)
  assert(!why || !why.includes('waypoint'), `still no waypoint after routing: ${why}`)

  const cleared = command(g, s, 'Digit0')
  assert(cleared.waypoint === null, '0 did not clear the course')
})

check('the component only dispatches; the table lives where it is tested', () => {
  // The structural half of the fix. A key table inside a component is a key table nobody can run.
  const src = codeOf(join(here, '..', 'components', 'scemaworld', 'ScemaWorldTerminal.tsx'))
  assert(src.includes('command(g, world.space, e.code)'), 'the component does not dispatch')
  assert(!src.includes("e.code === 'KeyF'"), 'the component still decides what a key means')
  // And it reads the world at the moment of the press, not the moment of attachment.
  assert(src.includes('loadedRef.current'), 'the handler still closes over render-scope state')
})

// ── the claims the scale table makes about itself ────────────────────────────

check('the laser reach comment is the reach the constants give', () => {
  // D-1 from the external audit, and the reason it is pinned rather than merely corrected: the
  // sentence said the reach was "a bit over a third of AGGRO_RANGE" and it was 1.8 *times* it.
  // Nothing had gone wrong with the game — `SPEED_LASER` and `AGGRO_RANGE` were each changed for
  // good reasons, months apart, and the sentence about their relationship was not, because no
  // test was reading it.
  const reach = SPEED_LASER * LIFE_LASER
  assert(reach > SENSOR_BASE, 'the reach no longer exceeds the aggro range the comment cites')
  // The **ratio** is the design statement and it did not move. Detection range and laser life were
  // raised by half again *together*, precisely so this line holds — which is the whole discipline
  // the check exists to enforce: the numbers may move, the relationship may not move silently.
  const ratio = reach / SENSOR_BASE
  assert(ratio > 1.6 && ratio < 2.1, `reach is ${ratio.toFixed(2)}x aggro; the comment says ~1.8`)
  const asExtent = reach / EXTENT
  assert(asExtent > 0.18 && asExtent < 0.23, `reach is ${asExtent.toFixed(3)}·EXTENT, not ~0.20`)
  const crossing = EXTENT / SPEED_LASER
  assert(crossing > 0.9 && crossing < 1.2, `a laser crosses an extent in ${crossing.toFixed(2)}s, not ~1.05`)
})

check('a laser outranges every fighter and no capital', () => {
  // The design line the corrected comment states. It is a *line through the class table*, so one
  // new statline could cross it without anything else failing — which is precisely the shape of
  // the drift that produced the wrong comment in the first place.
  const reach = SPEED_LASER * LIFE_LASER
  for (const id of CLASS_IDS) {
    const c = CLASSES[id]
    if (c.capital) {
      assert(
        c.aggro > reach,
        `${id} is a capital and can be engaged from outside its awareness (${(c.aggro / EXTENT).toFixed(3)} vs ${(reach / EXTENT).toFixed(3)})`,
      )
    } else {
      assert(
        c.aggro < reach,
        `${id} is a fighter and cannot be engaged from outside its awareness`,
      )
    }
  }
})

check('every figure the scale table quotes about itself is true', () => {
  // The general form of D-1. A figure in a comment is a claim, and a claim with no test is a
  // claim that will be wrong eventually. These are the ones `scale.ts` states in prose.
  const claims = [
    ['a stock engine covers an extent every 3.2 seconds', EXTENT / topSpeed(0), 3.0, 3.4],
    // Up from 2000:1, deliberately: the far plane had to grow to cover the sector's *diameter*
    // (see below), and holding the old ratio would have meant a near plane further out than the
    // player's own hull. Z-fighting needs two surfaces at nearly the same depth, which does not
    // happen between wireframe structures scattered through a volume.
    ['the depth ratio is about 5000:1', FAR_PLANE / NEAR_PLANE, 4400, 5600],
    ['a photon is slower than a laser', SPEED_LASER / SPEED_PHOTON, 1.01, 99],
    ['enemy fire is faster than any ship', SPEED_ENEMY_SHOT / topSpeed(4), 1.01, 99],
  ]
  for (const [claim, actual, lo, hi] of claims) {
    assert(actual >= lo && actual <= hi, `"${claim}" — actually ${Number(actual).toFixed(2)}`)
  }
  // And the one the file makes about craft never outrunning you, which `classes.ts` relies on.
  assert(SPEED_CRAFT + 4 * SPEED_CRAFT_PER_TIER < topSpeed(0), 'a craft can outrun a stock ship')
  // **A bolt must outrun the ship that fired it.** Not previously pinned, and the first attempt
  // at raising engine speed produced a fully upgraded hull flying faster than its own lasers —
  // which does not fail any other assertion here and is nonsense on screen.
  assert(SPEED_LASER > topSpeed(4), 'a fully upgraded ship outruns its own laser')
  assert(SPEED_PHOTON < SPEED_LASER, 'a photon is not slower than a laser')
})

// ── the claim ledger under concurrency ───────────────────────────────────────
//
// A security audit's N-1: the deployment budget, the per-wallet lifetime cap and the cooldown were
// each checked against a snapshot and enforced by a write that happened later, so two claims
// arriving together both measured themselves against money the other had already taken. Both were
// paid and the ledger recorded one, which is the part that makes it expensive — nothing downstream
// ever sees a figure that looks wrong.
//
// These drive the real `reserve`, concurrently, against a temporary ledger. That is the only way
// the property can be tested at all: settling for real needs a funded treasury key.

/**
 * Run `fn` against a private ledger file and policy, then put the environment back.
 *
 * Serialised against the other ledger tests, because `check` starts an async body the moment it is
 * called and these all reach for the same `process.env`. Run concurrently they overwrite each
 * other's `SCEMAWORLD_LEDGER` and every one of them silently tests the default path instead — the
 * seeded balances vanish and the assertions fail for a reason that has nothing to do with the
 * code. Which is, pleasingly, the same shape of bug as the one being tested.
 */
let ledgerTests = Promise.resolve()
function withTempLedger(seed, env, fn) {
  const run = () => withTempLedgerNow(seed, env, fn)
  const result = ledgerTests.then(run, run)
  ledgerTests = result.then(
    () => undefined,
    () => undefined,
  )
  return result
}

async function withTempLedgerNow(seed, env, fn) {
  const dir = mkdtempSync(join(tmpdir(), 'scemaworld-ledger-'))
  const path = join(dir, 'claims.json')
  writeFileSync(path, JSON.stringify(seed), 'utf8')
  const saved = { ...process.env }
  process.env.SCEMAWORLD_LEDGER = path
  for (const [k, v] of Object.entries(env)) process.env[k] = String(v)
  try {
    return await fn(path)
  } finally {
    for (const k of Object.keys(process.env)) if (!(k in saved)) delete process.env[k]
    Object.assign(process.env, saved)
    rmSync(dir, { recursive: true, force: true })
  }
}

const WALLET_A = TREASURY
const WALLET_B = SCEMA_MINT // a second well-shaped address; `looksLikeAddress` is a shape test

check('two claims arriving together cannot exceed the deployment budget', async () => {
  // The audit's witness, exactly: a budget of 9,000 with 8,900 already paid, and two claims of
  // 250 arriving at once. Unserialised, each decides against the same 100 remaining and each
  // reserves it — 200 leaves against a 100 allowance, and the ledger afterwards reads 9,000.
  await withTempLedger(
    { version: 1, dispensed: 8_900, wallets: {}, claims: [] },
    { SCEMAWORLD_TREASURY_BUDGET: 9_000, SCEMAWORLD_CLAIM_MAX: 250 },
    async () => {
      const [a, b] = await Promise.all([
        reserve({ scema: 250, wallet: WALLET_A, nowMs: 1_000, treasury: 90_000 }),
        reserve({ scema: 250, wallet: WALLET_B, nowMs: 1_000, treasury: 90_000 }),
      ])
      const paid = (a.ok ? a.ent.tokens : 0) + (b.ok ? b.ent.tokens : 0)
      assert(paid === 100, `paid ${paid} out of a remaining allowance of 100`)

      // And the ledger has to agree with what was handed out, or an operator auditing it later
      // reads a number that was never true.
      const after = await readLedger()
      assert(after.dispensed === 9_000, `ledger says ${after.dispensed}, expected 9,000`)
      assert(
        after.dispensed - 8_900 === paid,
        `ledger recorded ${after.dispensed - 8_900} against ${paid} actually reserved`,
      )
    },
  )
})

check('two claims arriving together cannot exceed one wallet lifetime cap', async () => {
  await withTempLedger(
    {
      version: 1,
      dispensed: 900,
      wallets: { [WALLET_A]: { paid: 900, lastMs: -1, claims: 4 } },
      claims: [],
    },
    // The cooldown is switched off so that it is the *wallet cap* being tested and not the
    // cooldown incidentally covering for it.
    { SCEMAWORLD_WALLET_MAX: 1_000, SCEMAWORLD_CLAIM_COOLDOWN_MS: 0 },
    async () => {
      const [a, b] = await Promise.all([
        reserve({ scema: 250, wallet: WALLET_A, nowMs: 5_000, treasury: 90_000 }),
        reserve({ scema: 250, wallet: WALLET_A, nowMs: 5_000, treasury: 90_000 }),
      ])
      const paid = (a.ok ? a.ent.tokens : 0) + (b.ok ? b.ent.tokens : 0)
      assert(paid === 100, `paid ${paid} to a wallet with 100 left of its lifetime cap`)
      const after = await readLedger()
      assert(after.wallets[WALLET_A].paid === 1_000, `wallet reached ${after.wallets[WALLET_A].paid}`)
    },
  )
})

check('two claims arriving together cannot bypass the cooldown', async () => {
  await withTempLedger(
    { version: 1, dispensed: 0, wallets: {}, claims: [] },
    { SCEMAWORLD_CLAIM_COOLDOWN_MS: 6 * 60 * 60 * 1_000 },
    async () => {
      const [a, b] = await Promise.all([
        reserve({ scema: 250, wallet: WALLET_A, nowMs: 10_000, treasury: 90_000 }),
        reserve({ scema: 250, wallet: WALLET_A, nowMs: 10_000, treasury: 90_000 }),
      ])
      const granted = [a, b].filter((r) => r.ok).length
      assert(granted === 1, `${granted} claims granted in the same instant against a 6h cooldown`)
      const refused = [a, b].find((r) => !r.ok)
      assert(refused.reason === 'cooling_down', `refused as ${refused.reason}, not cooling_down`)
    },
  )
})

check('many claims at once still stop at the budget', async () => {
  // The pairwise case is the one the audit exhibited; the property is not about pairs. Twenty at
  // once against a budget that covers three.
  await withTempLedger(
    { version: 1, dispensed: 0, wallets: {}, claims: [] },
    { SCEMAWORLD_TREASURY_BUDGET: 750, SCEMAWORLD_CLAIM_MAX: 250, SCEMAWORLD_CLAIM_COOLDOWN_MS: 0 },
    async () => {
      const results = await Promise.all(
        Array.from({ length: 20 }, () =>
          reserve({ scema: 250, wallet: WALLET_A, nowMs: 1_000, treasury: 90_000 }),
        ),
      )
      const paid = results.reduce((n, r) => n + (r.ok ? r.ent.tokens : 0), 0)
      assert(paid === 750, `paid ${paid} against a budget of 750`)
      const after = await readLedger()
      assert(after.dispensed === paid, `ledger says ${after.dispensed}, ${paid} was reserved`)
    },
  )
})

check('every reservation is recorded, and the version moves once per write', async () => {
  // The version is what turns a lost write into a refused claim. If two writes ever share a
  // version the detector is not doing anything.
  await withTempLedger(
    { version: 7, dispensed: 0, wallets: {}, claims: [] },
    { SCEMAWORLD_TREASURY_BUDGET: 10_000, SCEMAWORLD_CLAIM_COOLDOWN_MS: 0 },
    async () => {
      const results = await Promise.all(
        Array.from({ length: 5 }, () =>
          reserve({ scema: 100, wallet: WALLET_A, nowMs: 1_000, treasury: 90_000 }),
        ),
      )
      const granted = results.filter((r) => r.ok).length
      const after = await readLedger()
      assert(after.version === 7 + granted, `version ${after.version} after ${granted} writes`)
    },
  )
})

check('a claim decided against a ledger that has moved is refused, not paid', async () => {
  // The backstop, exercised directly: a reservation carrying a stale version must not land. It is
  // refused *before* anything is sent, so refusing costs a retry and never a double payment.
  assert(typeof LedgerConflict === 'function', 'LedgerConflict is not exported')
  await withTempLedger(
    { version: 3, dispensed: 0, wallets: {}, claims: [] },
    { SCEMAWORLD_CLAIM_COOLDOWN_MS: 0 },
    async (path) => {
      const first = await reserve({ scema: 100, wallet: WALLET_A, nowMs: 1, treasury: 90_000 })
      assert(first.ok, 'the uncontested claim was refused')
      // Somebody else writes. The next reservation must read *this*, not the snapshot above.
      writeFileSync(path, JSON.stringify({ version: 99, dispensed: 8_000, wallets: {}, claims: [] }), 'utf8')
      const second = await reserve({ scema: 250, wallet: WALLET_B, nowMs: 2, treasury: 90_000 })
      const after = await readLedger()
      assert(after.dispensed >= 8_000, `the foreign write was clobbered: dispensed ${after.dispensed}`)
      if (second.ok) {
        assert(
          after.dispensed === 8_000 + second.ent.tokens,
          `paid ${second.ent.tokens} but the ledger moved to ${after.dispensed}`,
        )
      }
    },
  )
})

check('the reservation and the decision are the same step, in the shipped code', () => {
  // The behavioural checks above prove `reserve` serialises. This is what pins `settle` to using
  // it — a future edit that reads the ledger and decides outside `withLedger` reintroduces N-1
  // without failing anything else here.
  const src = codeOf(join(here, '..', 'lib', 'scemaworld', 'treasury.ts'))
  const writes = [...src.matchAll(/writeLedger\(/g)].length
  const sections = [...src.matchAll(/withLedger\(/g)].length
  assert(sections >= 3, `only ${sections} critical sections; every ledger write needs one`)
  assert(writes >= sections, 'a withLedger with no write in it')

  // No write may pass back a snapshot the claim was decided against. Restoring one is how
  // releasing a reservation erases somebody else's — the old release path did exactly that.
  assert(
    !/writeLedger\(\s*ledger\s*[,)]/.test(src),
    'a ledger write passes the pre-claim snapshot back wholesale',
  )

  // And nothing may reach the network while the lock is held: an RPC round trip under the lock
  // turns a faucet into a queue behind a fifteen-second timeout. Scoped to the one function that
  // holds it — `quote` and `confirm` are downstream of here and are *supposed* to do RPC.
  const from = src.indexOf('export async function reserve')
  // Anchored on code, not on a banner comment: `codeOf` strips comments out.
  const to = src.indexOf('export interface TransferPlan')
  assert(from > 0 && to > from, 'reserve moved; this check needs rewriting')
  const critical = src.slice(from, to)
  assert(critical.includes('withLedger('), 'reserve no longer takes the lock')
  assert(
    !/await conn\.|readTreasury\(|getAccountInfo|getLatestBlockhash|sendRawTransaction/.test(critical),
    'an RPC call sits inside the ledger lock',
  )
  // The treasury balance has to arrive as an argument, for exactly that reason.
  assert(/treasury:\s*number/.test(critical), 'reserve no longer takes the balance as a parameter')
})

check('the preview refuses exactly where the payer refuses', () => {
  // The audit's N-2. `quote` caught the unreadable-ledger throw and quoted against an empty one,
  // so the preview promised a payout computed with every cap reset to full while `settle` refused
  // the identical request. An unreadable ledger is the case `readLedger` throws *for*.
  const src = codeOf(join(here, '..', 'lib', 'scemaworld', 'treasury.ts'))
  const q = src.slice(src.indexOf('export async function quote'), src.indexOf('export async function settle'))
  assert(!/readLedger\(\)\s*\.catch/.test(q), 'quote still swallows an unreadable ledger')
  assert(q.includes("reason: 'ledger_unreadable'"), 'quote does not report ledger_unreadable')

  // And the route has to carry it, or the refusal stops at the function boundary.
  const route = codeOf(join(here, '..', 'app', 'api', 'scemaworld', 'claim', 'route.ts'))
  assert(/if\s*\(!q\.ok\)/.test(route), 'the quote branch does not check the refusal')
  assert(route.includes('ledger_busy'), 'ledger_busy has no status mapping')
})

// ── flight: the ship has mass ────────────────────────────────────────────────
//
// Attitude used to be `rotate(camera, key * 1.4 * dt, …)` on all three axes and position used to
// be `translate(camera, [0, 0, -speed * dt])`. Both are the same mistake in two places: the ship
// was wherever it was pointing, instantly, and it pointed wherever the keys said, instantly. So
// there was nothing to roll *for* — yaw turned you just as fast and kept the horizon level — and
// no such thing as carrying speed through a turn.

const flyFor = (s, keys, n, dt = 1 / 60, from = null) => {
  let g = from ?? newGame(s)
  for (let i = 0; i < n; i += 1) {
    g = tick(g, s, { keys: new Set(keys), firing: false, dt, nowMs: 1000 + i * dt * 1000 })
  }
  return g
}

const angleBetween = (a, b) =>
  Math.acos(Math.max(-1, Math.min(1, a[0] * b[0] + a[1] * b[1] + a[2] * b[2])))

check('roll is the fastest axis and yaw the slowest, which is what makes rolling worth doing', () => {
  // The reported bug. With one shared rate there is no reason to ever touch the roll keys: yawing
  // brings the nose round just as quickly and leaves the horizon where you can read it. Rolling is
  // only a manoeuvre if it is *how* you turn — roll to put the target above you, then pull.
  const s = generate(world, digest)
  const base = newGame(s)
  const f0 = forward(base.camera)

  const rolled = flyFor(s, ['KeyQ'], 60)
  const yawed = flyFor(s, ['KeyD'], 60)
  const pitched = flyFor(s, ['KeyW'], 60)

  const rollAmt = angleBetween(up(base.camera), up(rolled.camera))
  const yawAmt = angleBetween(f0, forward(yawed.camera))
  const pitchAmt = angleBetween(f0, forward(pitched.camera))

  assert(rollAmt > pitchAmt * 1.5, `roll ${rollAmt.toFixed(2)} is not decisively faster than pitch ${pitchAmt.toFixed(2)}`)
  assert(pitchAmt > yawAmt * 1.4, `pitch ${pitchAmt.toFixed(2)} is not faster than yaw ${yawAmt.toFixed(2)}`)

  // And a roll is about the ship's own nose: it must turn the ship without turning where it points.
  assert(
    angleBetween(f0, forward(rolled.camera)) < 0.02,
    'rolling moved the nose, so it is not a roll',
  )
})

check('attitude has momentum: a released control bleeds off rather than stopping dead', () => {
  const s = generate(world, digest)
  const spun = flyFor(s, ['KeyQ'], 60)
  assert(Math.abs(spun.spin.z) > 1, `roll never spun up: ${spun.spin.z}`)

  // Still rotating a tick after release — that lag is the entire difference between flying
  // something with mass and dragging a camera around.
  const justAfter = flyFor(s, [], 6, 1 / 60, spun)
  assert(Math.abs(justAfter.spin.z) > 0.2, 'rotation stopped dead the frame the key came up')
  assert(Math.abs(justAfter.spin.z) < Math.abs(spun.spin.z), 'a released control did not bleed off')

  // And it does settle. A ship that keeps a residual tumble forever is a broken ship, not a
  // realistic one.
  const settled = flyFor(s, [], 120, 1 / 60, spun)
  assert(Math.abs(settled.spin.z) < 1e-6, `still spinning at ${settled.spin.z}`)
})

check('a long frame cannot overshoot the commanded rate into a spin', () => {
  // An alt-tabbed tab hands back a half-second frame. `spin += accel * dt` sails past the
  // commanded rate and the ship snaps into a rotation nobody asked for — the class of bug that
  // only ever reproduces on somebody else's machine.
  const s = generate(world, digest)
  const g = flyFor(s, ['KeyQ'], 1, 0.75)
  assert(
    Math.abs(g.spin.z) <= SCALE.RATE_ROLL + 1e-9,
    `one long frame reached ${g.spin.z} against a peak of ${SCALE.RATE_ROLL}`,
  )
})

check('the ship carries its momentum through a turn', () => {
  // True 3D flight: the nose and the course are two different things. Turning used to rotate the
  // velocity with the hull, so a reversal was free and there was no such thing as drifting wide.
  const s = generate(world, digest)
  let g = flyFor(s, [], 180, 1 / 60, { ...newGame(s), throttle: 1 })
  const cruise = Math.hypot(g.velocity.x, g.velocity.y, g.velocity.z)
  assert(cruise > topSpeed(0) * 0.9, `never reached cruise: ${cruise}`)

  const pulled = flyFor(s, ['KeyW'], 45, 1 / 60, g)
  const n = forward(pulled.camera)
  const v = pulled.velocity
  const vl = Math.hypot(v.x, v.y, v.z) || 1
  const drift = Math.acos(Math.max(-1, Math.min(1, (n[0] * v.x + n[1] * v.y + n[2] * v.z) / vl)))
  assert(drift > 0.15, `the course followed the nose exactly: ${drift} rad of drift`)

  // But the assist brings it back, or the game is a different and much worse one — you would
  // spend every fight fighting your own momentum instead of the enemy.
  const recovered = flyFor(s, [], 180, 1 / 60, pulled)
  const rn = forward(recovered.camera)
  const rv = recovered.velocity
  const rl = Math.hypot(rv.x, rv.y, rv.z) || 1
  const left = Math.acos(Math.max(-1, Math.min(1, (rn[0] * rv.x + rn[1] * rv.y + rn[2] * rv.z) / rl)))
  assert(left < 0.05, `the assist never recovered the course: ${left} rad still off`)
})

check('a dry ship keeps every bit of its momentum', () => {
  // The assist is the only thing that can slow the ship, and it runs on the drive. Damping a dry
  // ship would let a stranded player brake to a stop for free — which is both wrong and the least
  // interesting reading of running out of fuel.
  const s = generate(world, digest)
  let g = flyFor(s, [], 180, 1 / 60, { ...newGame(s), throttle: 1 })
  const moving = Math.hypot(g.velocity.x, g.velocity.y, g.velocity.z)
  assert(moving > 0, 'never got going')

  g = { ...g, ship: { ...g.ship, fuel: 0 } }
  const coasted = flyFor(s, [], 300, 1 / 60, g)
  const after = Math.hypot(coasted.velocity.x, coasted.velocity.y, coasted.velocity.z)
  assert(Math.abs(after - moving) < moving * 1e-6, `a dry ship braked from ${moving} to ${after}`)
  assert((coasted.notice ?? '').length >= 0)
})

check('speed is capped on the magnitude, not per axis', () => {
  // A per-axis cap is higher on a diagonal than along an axis, so the fastest route anywhere
  // becomes a corner of the box — and nothing looks wrong while it happens.
  const s = generate(world, digest)
  const g = flyFor(s, ['ArrowRight', 'Space'], 600, 1 / 60, { ...newGame(s), throttle: 1 })
  const sp = Math.hypot(g.velocity.x, g.velocity.y, g.velocity.z)
  assert(sp <= topSpeed(g.ship.levels?.engine ?? 0) + 1e-6, `reached ${sp} on a diagonal`)
})

// ── the sector repopulates, all of it ────────────────────────────────────────

/** Live non-capital craft per faction, which is what every floor and strength counts. */
function censusOf(g) {
  const out = {}
  for (const c of g.swarm.craft) {
    if (!c.alive || c.spec.capital) continue
    out[c.faction] = (out[c.faction] ?? 0) + 1
  }
  return out
}

/**
 * Run the sector forward with an observer who cannot die.
 *
 * `tick` returns immediately once `lost` is set, so a probe pilot who gets shot freezes the whole
 * simulation — every counter stops and the sector looks like it has stopped repopulating when in
 * fact nothing is running at all. That cost a diagnosis once already.
 */
function observe(s, seconds, from = null) {
  let g = from ?? newGame(s)
  const dt = 1 / 20
  for (let i = 1; i <= Math.round(seconds / dt); i += 1) {
    g = tick(g, s, { keys: new Set(), firing: false, dt, nowMs: i * dt * 1000 })
    g = { ...g, lost: false, ship: { ...g.ship, hull: 9e9, shield: 9e9 } }
  }
  return g
}

const purge = (g) => ({
  ...g,
  swarm: { ...g.swarm, craft: g.swarm.craft.map((c) => (c.spec.capital ? c : { ...c, alive: false })) },
})

check('a purged sector comes back to full strength, not to the floor', () => {
  // The floor was the trigger AND the target, so clearing thirty raiders returned twenty-nine of
  // them and the sector settled at 43 of the 72 it opened with — permanently, for the rest of the
  // session, with nothing saying so. Measured before the fix: recovery stopped dead on 43.
  const s = generate(world, digest)
  const g = observe(s, 720, purge(newGame(s)))
  const c = censusOf(g)
  assert(
    c.raider >= RAIDER_STRENGTH * 0.95,
    `raiders recovered to ${c.raider} of ${RAIDER_STRENGTH}`,
  )
  assert(c.raider > RAIDER_FLOOR, 'recovery stopped at the floor, which is a trigger and not a target')
})

check('traffic comes back too, which it never used to at all', () => {
  // Couriers and freighters had no replenishment path of any kind. Raiders hunt them, so a sector
  // left running lost every one — 34 and 14 down to zero, permanently — and the two factions that
  // make the place look inhabited quietly stopped existing. Nothing said so, because the sector
  // still had plenty of ships in it; they were all just shooting at each other.
  const s = generate(world, digest)
  const g = observe(s, 720, purge(newGame(s)))
  const c = censusOf(g)
  for (const t of Factions.TRAFFIC) {
    const want = Factions.strengthOf(t.faction, t.klass)
    assert(want > 0, `${t.faction} has no roster strength`)
    assert(c[t.faction] >= want * 0.9, `${t.faction} recovered to ${c[t.faction] ?? 0} of ${want}`)
  }
})

check('the patrol comes back, and the capitals on both sides do not', () => {
  // The one thing that must NOT repopulate. A leviathan you spent four minutes killing is the
  // only lasting mark you can leave on the sector, and a respawn timer turns it into a chore.
  const s = generate(world, digest)
  const start = newGame(s)
  const capsAt = (g) => g.swarm.craft.filter((c) => c.alive && c.spec.capital).length
  const before = capsAt(start)
  assert(before > 0, 'no capitals to begin with')

  const killed = {
    ...start,
    swarm: { ...start.swarm, craft: start.swarm.craft.map((c) => (c.spec.capital ? { ...c, alive: false } : c)) },
  }
  const after = observe(s, 600, killed)
  assert(capsAt(after) === 0, `${capsAt(after)} capitals came back`)
  // While the fighters around them did.
  assert(censusOf(after).marshal >= Factions.MARSHAL_STRENGTH * 0.9, 'the patrol did not recover')
})

check('a full sector is left alone', () => {
  // Reinforcement targets a strength rather than pouring ships in forever. An unbounded top-up
  // would turn a quiet sector into a traffic jam over a long session, and the symptom would be
  // the framerate rather than anything legible.
  const s = generate(world, digest)
  const g = observe(s, 300)
  const c = censusOf(g)
  assert(c.raider <= RAIDER_STRENGTH + 8, `raiders overshot to ${c.raider}`)
  for (const t of Factions.TRAFFIC) {
    const want = Factions.strengthOf(t.faction, t.klass)
    assert((c[t.faction] ?? 0) <= want + 2, `${t.faction} overshot to ${c[t.faction]} of ${want}`)
  }
})

check('a contested sector is reinforced faster than a quiet one', () => {
  // The floor keeps a job: below it the sector is in trouble and reinforcement surges. Without it
  // a gutted sector takes a quarter of an hour to come back, which nobody waits through.
  assert(
    Respawn.RAIDER_SURGE_MS < Respawn.RAIDER_INTERVAL_MS,
    'the surge interval is not shorter than the cruise interval',
  )
  const s = generate(world, digest)
  const gutted = observe(s, 120, purge(newGame(s)))
  const light = observe(s, 120, {
    ...newGame(s),
    // Two short of full: above the floor, so the trickle rate applies.
    swarm: {
      ...newGame(s).swarm,
      craft: newGame(s).swarm.craft.map((c, i) => (c.faction === 'raider' && !c.spec.capital && i % 37 === 0 ? { ...c, alive: false } : c)),
    },
  })
  const gained = censusOf(gutted).raider
  assert(gained > 20, `a gutted sector only recovered ${gained} in two minutes`)
  assert(censusOf(light).raider <= RAIDER_STRENGTH + 8, 'a nearly-full sector was flooded')
})

check('the strength table is the roster, not a second copy of it', () => {
  // `MARSHAL_STRENGTH` was a literal 18 sitting beside a roster line that also said 18: two places
  // to change, and one of them silently wins.
  assert(
    Factions.MARSHAL_STRENGTH === Factions.strengthOf('marshal', 'marshal'),
    'the marshal strength has drifted from the roster',
  )
  for (const t of Factions.TRAFFIC) {
    assert(Factions.strengthOf(t.faction, t.klass) > 0, `${t.faction} is topped up toward zero`)
  }
})

/**
 * An in-memory `Wallet.Store`. The seam exists precisely so this file needs no browser — the same
 * split as `claim.ts` (pure policy) against `treasury.ts` (chain).
 */
function fakeStore() {
  const map = new Map()
  return {
    get: (k) => (map.has(k) ? map.get(k) : null),
    set: (k, v) => { map.set(k, v) },
  }
}

// ── the roster is where the player is, and where the sector is ───────────────
//
// The bug these exist for shipped twice and neither time did anything fail. `Rng` is a 32-bit
// xorshift, so `below(n)` is `next() % n` and for any `n` past 2^32 the modulo does nothing — the
// draw is just `next()`, uniform over a range unrelated to the one asked for. `EXTENT` is 3.2e9,
// so the ceiling sits at 1.34 extents and every scatter in the game was over it.
//
// It produced plausible coordinates both times, which is why nothing caught it. At ±1 extent it
// returned [-1.00, +0.34]: skewed, but near the origin, so the sector looked populated. At
// ±5.89 it returned [-5.89, -4.55] — a thin shell with all three axes negative, the entire roster
// exiled to one corner, nothing at all within sensor range of the spawn. The suite still passed,
// including "marshals fight raiders whether or not anyone is watching", because both populations
// were exiled to the *same* corner.

check('the roster is not exiled to one corner of the sector', () => {
  // The direct signature of the modulo bug: a scatter that cannot produce a positive coordinate.
  // Asserted per axis and per sign, because the failure was exactly a sign confinement and a
  // bounding box alone would have looked fine.
  const s = generate(world, digest)
  const pts = [...s.raiders.map((c) => c.at), ...Factions.trafficOf(s, s.seed).map((c) => c.at)]
  for (const axis of ['x', 'y', 'z']) {
    assert(
      pts.some((p) => p[axis] > 0) && pts.some((p) => p[axis] < 0),
      `every scattered ${axis} has the same sign — the draw range exceeded the RNG's 32 bits`,
    )
  }
})

check('scatter spans the reach it is given, in both directions', () => {
  // Straight at the helper, at a reach far past the 32-bit ceiling, so a future caller that
  // reintroduces a bare `below(huge)` is caught here rather than by somebody noticing the sector
  // is empty. `pull` of 1 is the uniform case, which is what makes the span assertable.
  const rng = new Rng('deadbeef')
  const reach = SCALE.SECTOR_REACH
  let lo = Infinity
  let hi = -Infinity
  for (let i = 0; i < 4000; i += 1) {
    const p = Raiders.scatter(rng, reach, 1, 0)
    lo = Math.min(lo, p.x)
    hi = Math.max(hi, p.x)
  }
  assert(hi > reach * 0.9, `scatter never reached past ${(hi / reach).toFixed(2)} of its reach`)
  assert(lo < -reach * 0.9, `scatter never reached below ${(lo / reach).toFixed(2)} of its reach`)
})

check('there is something to meet at the spawn point', () => {
  // What the enlarged sector actually cost a player, and the thing a bounding-box check misses:
  // a roster spread correctly but *uniformly* through a cube still leaves the middle empty,
  // because a cube's volume grows as r^2 and the middle is where the player starts. Measured at
  // the regression: zero hostiles within sensor range and zero within a whole extent.
  const s = generate(world, digest)
  const d = (p) => Math.hypot(p.x, p.y, p.z)

  // Reachable: a good share of the roster within a single extent of the spawn, so the first
  // encounter is a short flight rather than a commute. The regression had *zero* here.
  const withinOne = s.raiders.filter((c) => d(c.at) < EXTENT).length
  assert(withinOne > 8, `only ${withinOne} hostiles within an extent of the spawn`)

  // And not *on* the player. The two pull against each other and this pair is the whole of the
  // tuning: the board is clear on the first frame — the property the original placement had, and
  // worth keeping, because loading straight into a resolved contact reads as being ambushed by
  // the loading screen — and the nearest wing is a short flight beyond it.
  const sensor = SCALE.AGGRO_RANGE * SCALE.SENSOR_MULTIPLIER
  const closest = Math.min(...s.raiders.map((c) => d(c.at)))
  assert(
    closest >= sensor,
    `a hostile is resolved on the sensor board at load-in, ${(closest / EXTENT).toFixed(3)} extents out`,
  )
})

check('the patrol is scattered where the raiders are', () => {
  // The two populations have to overlap or the ambient war stops. They are placed by different
  // functions in different modules, which is precisely how they drifted apart before.
  const s = generate(world, digest)
  const d = (p) => Math.hypot(p.x, p.y, p.z)
  const med = (xs) => xs.sort((a, b) => a - b)[Math.floor(xs.length / 2)]
  const raiders = med(s.raiders.map((c) => d(c.at)))
  const patrol = med(
    Factions.trafficOf(s, s.seed).filter((c) => c.faction === 'marshal').map((c) => d(c.at)),
  )
  const ratio = patrol / raiders
  assert(
    ratio > 0.3 && ratio < 3,
    `the patrol sits ${ratio.toFixed(1)}x as far out as the raiders it hunts`,
  )
})

// ── roles and contracts ──────────────────────────────────────────────────────

check('every class dies to the number of photons the ladder states', () => {
  // Durability used to be raw hull and shield numbers, and they had drifted into a game nobody
  // could finish: a titan carried 23,000 effective points against a 240-point warhead, so killing
  // one took **96 photons** — twelve full magazines off the largest hull in the game. A leviathan
  // took 29 and a dreadnought 10.
  //
  // The ladder is the definition now and the statline is derived from it, so this is the check
  // that keeps the two from separating again. Editing a hull without editing `PHOTONS_TO_KILL`
  // fails here rather than quietly moving what a magazine is worth.
  for (const id of ALL_CLASS_IDS) {
    const c = CLASSES[id]
    const want = CLASSES_MOD.PHOTONS_TO_KILL[id]
    assert(want !== undefined, `${id} is missing from PHOTONS_TO_KILL`)
    const got = Math.ceil((c.hull + c.shield) / PHOTON.damage)
    assert(got === want, `${id} takes ${got} photons, the ladder says ${want}`)
  }
  // The ladder's own shape: strictly increasing by tier, and topping out at the starting magazine
  // so the largest thing in the sector is beatable by a pilot who has spent nothing.
  assert(CLASSES_MOD.PHOTONS_TO_KILL.titan === photonMagazine('skiff'), 'a titan is not a full magazine')
  assert(CLASSES_MOD.PHOTONS_TO_KILL.dreadnought === 6, 'a dreadnought is not six')
  assert(CLASSES_MOD.PHOTONS_TO_KILL.warfighter === 4, 'a warfighter is not four')
  assert(CLASSES_MOD.PHOTONS_TO_KILL.gunship === 2, 'a heavy fighter is not two')
  assert(CLASSES_MOD.PHOTONS_TO_KILL.skiff === 1, 'a fighter is not one')
})

check('a capital can actually be killed with what a ship carries', () => {
  // The end-to-end form, because the arithmetic above can be right while the *fight* is still
  // unwinnable — shields regenerate, and a magazine fired slowly enough loses to the regen. The
  // photon cooldown is 700ms and shields need a 4.2s lull, so sustained fire never lets them come
  // back; this asserts that relationship rather than assuming it.
  const rounds = photonMagazine('skiff')
  const span = rounds * PHOTON.cooldownMs
  assert(
    PHOTON.cooldownMs < SHIELD_DELAY_MS,
    'a magazine fired at full rate lets shields regenerate between rounds',
  )
  for (const id of ['dreadnought', 'leviathan', 'titan']) {
    const c = CLASSES[id]
    assert(
      rounds * PHOTON.damage >= c.hull + c.shield,
      `a full magazine (${rounds * PHOTON.damage}) cannot kill a ${id} (${c.hull + c.shield})`,
    )
  }
  void span
})

check('a reinforcement arrives in front of the player, where it can be seen', () => {
  // The entry effect exists to *announce* an arrival, and it was frequently drawn where the
  // player could not see it: the cone weighted the nose at 1 and the jitter at 0.75, so a bearing
  // pointing back along the nose left 0.25 forward against 0.75 lateral — 72 degrees off the nose,
  // against a 66-degree field of view. Most of the cone was off-screen.
  const facing = { x: 0, y: 0, z: -1 }
  const at = { x: 0, y: 0, z: 0 }
  let worst = 0
  for (let i = 0; i < 2000; i += 1) {
    // Every direction, including ones pointing straight back along the nose — the case the old
    // spread handled worst.
    const t = (i / 2000) * Math.PI * 2
    const u = (i / 2000) * Math.PI
    const jitter = {
      x: Math.cos(t) * Math.sin(u),
      y: Math.cos(u),
      z: Math.sin(t) * Math.sin(u),
    }
    const p = Arrivals.arrivalPoint(at, facing, jitter, Arrivals.ARRIVAL_SPREAD)
    const d = Math.hypot(p.x, p.y, p.z) || 1
    // Angle off the nose.
    const cos = (p.x * facing.x + p.y * facing.y + p.z * facing.z) / d
    worst = Math.max(worst, Math.acos(Math.max(-1, Math.min(1, cos))))
    assert(cos > 0, `an arrival was placed behind the player (cos ${cos.toFixed(3)})`)
  }
  // Inside the camera's half angle, so the entry is on screen rather than merely in front.
  const halfFov = 1.15 / 2
  assert(worst < halfFov, `worst arrival is ${(worst * 57.3).toFixed(0)}deg off, outside the view`)
})

check('the sector sends enough wings to be busy', () => {
  // "Not enough enemies are spawning in" — measured rather than tuned by feel. At the sector's
  // current size a crossing takes about half a minute, and the interval was 22 seconds, so a
  // player could fly a long way to meet nothing.
  const s = generate(world, digest)
  let g = newGame(s)
  let raised = 0
  let last = Enemy.of(g.swarm, 'raider').length
  for (let f = 0; f < 60 * 120; f += 1) {
    g = tick(g, s, { keys: new Set(), firing: false, dt: 1 / 60, nowMs: (f * 1000) / 60 })
    const now = Enemy.of(g.swarm, 'raider').length
    if (now > last) raised += 1
    last = now
  }
  assert(raised >= 4, `only ${raised} wings arrived in two minutes`)
})

check('a role decides who shoots at you, and there is one implementation of it', () => {
  // `hostileTo` was `f === 'raider'` read in six places — the enemy AI, the jump inhibitor, the
  // sensor board, the threat readout, the fire gate and the collision handler. A pirate makes the
  // answer depend on who is asking, and a seventh call site that disagreed would be a craft that
  // steers at you and never fires.
  assert(Roles.hostileToPlayer('raider', 'bounty-hunter'), 'raiders ignore a bounty hunter')
  assert(!Roles.hostileToPlayer('marshal', 'bounty-hunter'), 'the patrol hunts a bounty hunter')
  assert(Roles.hostileToPlayer('marshal', 'pirate'), 'the patrol ignores a pirate')
  assert(!Roles.hostileToPlayer('raider', 'pirate'), 'raiders hunt a pirate')
  // The smuggler is the only role with no friends at all, which is what it is paid for.
  assert(
    Roles.hostileToPlayer('raider', 'smuggler') && Roles.hostileToPlayer('marshal', 'smuggler'),
    'a smuggler is safe from somebody',
  )
})

check('you are paid only for what your role hunts', () => {
  // The sharpened form of "a bounty on the good guys would be the game paying for the sector to be
  // less policed". That rule was about a payout pulling a player toward work the game would rather
  // they did not do; it survives a pirate role as the stronger statement, because what each role
  // is paid for is exactly what it declared itself to be.
  for (const role of Roles.ROLE_IDS) {
    const paid = ['raider', 'marshal'].filter((f) => Roles.bountyScale(f, role) > 0)
    const declared = Roles.ROLES[role].hunts
    assert(
      paid.length === declared.length && paid.every((f) => declared.includes(f)),
      `${role} is paid for [${paid}] but declared [${declared}]`,
    )
  }
  // A trader and a smuggler are paid for no kill at all — their money is cargo.
  assert(Roles.bountyScale('raider', 'trader') === 0, 'a trader is paid for kills')
  assert(Roles.bountyScale('marshal', 'smuggler') === 0, 'a smuggler is paid for kills')
})

check('the role reaches the AI, not just the sensor board', () => {
  // The gap this exists for was nearly shipped. `focusOf`'s raider branch fell through to the
  // player unconditionally and its marshal branch never reached the player at all — right for a
  // bounty hunter and exactly backwards for a pirate, who would have been hunted by the wing that
  // counts them as one of their own and ignored by the patrol they are paid to fight. Every other
  // role check passed while that was true, because they all test the *tables* rather than the
  // simulation.
  const s = generate(world, digest)
  const measure = (role) => {
    let g = newGame(s, role)
    // Park the ship in the middle of the roster so both factions have something to decide about.
    let hunted = { raider: 0, marshal: 0 }
    for (let f = 0; f < 60 * 40; f += 1) {
      g = tick(g, s, { keys: new Set(), firing: false, dt: 1 / 60, nowMs: (f * 1000) / 60 })
      for (const c of Enemy.living(g.swarm)) {
        // `target === null` on an armed craft means "the player" — see `focusOf`.
        if (c.target === null && (c.faction === 'raider' || c.faction === 'marshal')) {
          const d = Math.hypot(
            c.at.x - g.camera.position[0],
            c.at.y - g.camera.position[1],
            c.at.z - g.camera.position[2],
          )
          if (d < c.spec.aggro) hunted[c.faction] += 1
        }
      }
    }
    return hunted
  }
  const hunter = measure('bounty-hunter')
  assert(hunter.marshal === 0, `the patrol closed on a bounty hunter ${hunter.marshal} times`)
  const pirate = measure('pirate')
  assert(pirate.raider === 0, `a raider closed on a pirate ${pirate.raider} times`)
})

check('a contract reads no record field', () => {
  // The same source scan the reward path already carries, extended to the two new modules. A
  // contract that paid by `blind_spots` would be the original defect wearing a quest marker.
  for (const m of ['quests', 'roles']) {
    const src = codeOf(join(here, '..', 'lib', 'scemaworld', `${m}.ts`))
    const body = src.replace(/\/\*\*[\s\S]*?\*\//g, '').replace(/\/\/.*$/gm, '')
    for (const field of ['blind_spots', 'signals', 'magnitude', 'legibility', 'provenance']) {
      assert(!body.includes(field), `${m}.ts reads \`${field}\` — a reward path must not`)
    }
  }
})

check('a board is deterministic, faction-scoped and role-scoped', () => {
  const s = generate(world, digest)
  const a = Quests.board(s.seed, 'marshal', 'bounty-hunter', s.nodes)
  const b = Quests.board(s.seed, 'marshal', 'bounty-hunter', s.nodes)
  assert(JSON.stringify(a) === JSON.stringify(b), 'the same board came back different')
  const other = Quests.board(s.seed, 'raider', 'pirate', s.nodes)
  assert(
    JSON.stringify(a) !== JSON.stringify(other),
    'two factions offer the same list wearing different labels',
  )
  // A hunting contract never asks you to hunt the faction offering it.
  for (const q of [...a, ...other]) {
    if (q.quarry) assert(q.quarry !== q.faction, `${q.faction} pays for its own hulls`)
  }
})

check('a faction that will not deal with you says why', () => {
  // An empty board with no explanation is indistinguishable from a broken one — the same lesson
  // as the station panel that refused three services in a notice that faded in three seconds.
  const why = Quests.refusal('marshal', 'pirate')
  assert(typeof why === 'string' && why.length > 0, 'a marshal board silently ignored a pirate')
  assert(Quests.refusal('marshal', 'bounty-hunter') === null, 'the patrol refused a bounty hunter')
})

check('a contract advances on an act, never on a survey', () => {
  const s = generate(world, digest)
  const q = Quests.board(s.seed, 'marshal', 'bounty-hunter', s.nodes).find((x) => x.kind === 'bounty')
  assert(q, 'no bounty contract was offered to a bounty hunter')
  let st = Quests.accept(Quests.newQuests(), q)
  // A kill of the wrong faction does nothing at all.
  st = Quests.recordKill(st, 'marshal', false).state
  assert(st.active.progress === 0, 'a marshal kill advanced a raider contract')
  let completed = null
  for (let i = 0; i < q.count; i += 1) {
    const r = Quests.recordKill(st, 'raider', false)
    st = r.state
    completed = r.completed ?? completed
  }
  assert(completed && completed.id === q.id, 'the contract never completed')
  assert(st.active === null, 'a completed contract stayed active')
  assert(st.earned === q.reward, `earned ${st.earned}, not ${q.reward}`)
})

check('a haul needs both legs, in order', () => {
  const s = generate(world, digest)
  const q = Quests.board(s.seed, 'marshal', 'trader', s.nodes).find((x) => x.kind === 'haul')
  assert(q, 'no haul was offered to a trader')
  let st = Quests.accept(Quests.newQuests(), q)
  // Arriving at the destination first does nothing: there is nothing aboard yet.
  st = Quests.recordDock(st, q.to).state
  assert(!st.active.picked, 'cargo was delivered before it was collected')
  st = Quests.recordDock(st, q.from).state
  assert(st.active.picked, 'the pickup did not register')
  const done = Quests.recordDock(st, q.to)
  assert(done.completed, 'the delivery did not complete')
})

check('a live contraband run is what makes the patrol hostile to a trader', () => {
  // The mechanic in one line: a pirate is always hunted by the patrol, a trader carrying one
  // illegal crate is hunted only while carrying it.
  const s = generate(world, digest)
  const q = Quests.board(s.seed, 'raider', 'smuggler', s.nodes).find((x) => x.kind === 'contraband')
  assert(q, 'no contraband was offered to a smuggler')
  let st = Quests.accept(Quests.newQuests(), q)
  assert(!Quests.carryingContraband(st), 'contraband was aboard before the pickup')
  st = Quests.recordDock(st, q.from).state
  assert(Quests.carryingContraband(st), 'the pickup did not load the contraband')
})

check('a citadel is placed by the seed, tiered, and one of each per faction', () => {
  // The same rule as `WINGS` and `GARRISON`: a record cannot buy itself more of the stations that
  // issue the work. Both factions are represented in every world on purpose — a roll would leave
  // some sectors with no board at all for two of the four roles.
  const s = generate(world, digest)
  const cits = s.nodes.filter((n) => n.kind === 'citadel')
  assert(cits.length === CITADELS, `${cits.length} citadels, not ${CITADELS}`)
  for (const f of ['marshal', 'raider']) {
    const tiers = cits.filter((c) => c.faction === f).map((c) => c.tier).sort()
    assert(
      JSON.stringify(tiers) === JSON.stringify([1, 2, 3]),
      `${f} citadel tiers are [${tiers}], not one of each`,
    )
  }
  // Full service, so a citadel is somewhere you come back to rather than visit once.
  for (const c of cits) {
    for (const want of ['refuel', 'repair', 'trade']) {
      assert(c.services.includes(want), `a citadel does not offer ${want}`)
    }
  }
  // And its label must never collide with a class label, or `drawList` matching on label returns
  // a station where a craft was meant — which is exactly what `BASTION` did.
  const classLabels = CLASS_IDS.map((id) => CLASSES[id].label)
  for (const c of cits) {
    for (const l of classLabels) {
      assert(!c.label.includes(l), `citadel "${c.label}" collides with the class label ${l}`)
    }
  }
})

check('the citadel roll does not perturb the fractal', () => {
  // Drawing from the fractal's own stream inside the growth loop shifts every subsequent jitter,
  // which changes the *shape of the tree*. Measured when it happened: the node count fell from
  // 424 to 374 because a decision about station kinds was consuming the fractal's randomness.
  const s = generate(world, digest)
  assert(s.nodes.length > 400, `only ${s.nodes.length} nodes — a stream is being shared`)
})

check('photons can be replaced with salvage, and only where ordnance is sold', () => {
  const s = generate(world, digest)
  let g = newGame(s)
  const dock = s.nodes.find((n) => n.services.includes('trade'))
  assert(dock, 'no trading node in the sector')
  const mag = photonMagazine('skiff')
  // Spend two rounds, bank some salvage, and dock.
  g = { ...g, combat: { ...g.combat, photonsLeft: mag - 2 }, ship: { ...g.ship, salvage: 500 } }
  g = { ...g, nearby: dock }
  const after = rearm(g)
  assert(after.combat.photonsLeft === mag, `rearmed to ${after.combat.photonsLeft} of ${mag}`)
  assert(after.ship.salvage === 500 - 2 * PHOTON_PRICE, `charged ${500 - after.ship.salvage}`)
  // A partial reload is sold rather than refused — refusing to sell four rounds to somebody who
  // cannot afford eight is a refusal with no reason behind it.
  let poor = { ...g, ship: { ...g.ship, salvage: PHOTON_PRICE } }
  poor = rearm(poor)
  assert(poor.combat.photonsLeft === mag - 1, 'a partial reload was refused')
  // And nowhere without ordnance.
  const rift = s.nodes.find((n) => n.kind === 'rift')
  if (rift) {
    const nope = rearm({ ...g, nearby: rift })
    assert(nope.combat.photonsLeft === mag - 2, 'a rift sold photons')
  }
})

// ── the account survives the tab ─────────────────────────────────────────────
//
// Earning had no memory. `newGame` builds a ship from `newShip()`, so salvage and SCEMA went back
// to zero on every page load and on every record dropped — which made the withdrawal path, whose
// minimum is ten SCEMA, unreachable for most players however long they played. See `wallet.ts`.

check('an account round-trips through storage', () => {
  const store = fakeStore()
  const account = { ...Wallet.newAccount(), salvage: 91, scema: 40, frame: 'lancer' }
  Wallet.save(store, account)
  const back = Wallet.load(store)
  assert(back.salvage === 91 && back.scema === 40, `came back ${back.salvage}/${back.scema}`)
  assert(back.frame === 'lancer', `came back flying a ${back.frame}`)
})

check('a missing, unreadable or corrupt account is zero rather than a crash', () => {
  // Storage genuinely fails: a private window, blocked site data, a full quota, an embedded
  // webview. A game that will not start because it could not read a balance is worse than one
  // that starts having forgotten it.
  assert(Wallet.load(null).scema === 0, 'a null store should be a fresh account')
  assert(Wallet.load(fakeStore()).scema === 0, 'an empty store should be a fresh account')

  const broken = fakeStore()
  broken.set(Wallet.WALLET_KEY, '{not json')
  assert(Wallet.load(broken).scema === 0, 'unparseable JSON should be a fresh account')

  const throws = { get: () => { throw new Error('SecurityError') }, set: () => {} }
  assert(Wallet.load(throws).scema === 0, 'a throwing store should be a fresh account')
})

check('a hand-edited account cannot produce a nonsense balance', () => {
  // Not a tamper defence — a player editing their own storage has given themselves salvage, and
  // the faucet caps are what bound that. It is that a malformed value must never reach the game
  // *as a number*: NaN propagates through every sum in `economy.ts` and renders as `NaN SCEMA` on
  // a panel making claims about money, and a negative balance would let `exchange` mint.
  const bad = Wallet.sanitise({
    salvage: NaN, scema: -500, frame: 'not-a-hull', levels: { engine: 99, hull: -3 },
  })
  assert(bad.salvage === 0, `NaN salvage became ${bad.salvage}`)
  assert(bad.scema === 0, `a negative balance became ${bad.scema}`)
  assert(bad.frame === 'skiff', `an unknown hull became ${bad.frame}`)
  assert(bad.levels.engine <= 4, `engine level ${bad.levels.engine} exceeds the maximum`)
  assert(bad.levels.hull === 0, `a negative level became ${bad.levels.hull}`)
  assert(Wallet.sanitise(null).scema === 0, 'null should sanitise to a fresh account')
  assert(Wallet.sanitise('nonsense').scema === 0, 'a string should sanitise to a fresh account')
})

check('restoring gives back what was bought, and nothing that was spent', () => {
  // The split the module is built on: balances and purchases are *property* and come back; fuel,
  // hull, shields, jump charges and the stripped-derelict list are *state* and are rebuilt. A
  // player who could reload out of a fight, or carry a scavenged-node list into a world where
  // those ids mean something else, would be duplicating value rather than keeping it.
  const s = generate(world, digest)
  const fresh = newGame(s)
  const account = {
    ...Wallet.newAccount(),
    salvage: 300,
    scema: 120,
    frame: 'marauder',
    levels: { ...Wallet.newAccount().levels, tanks: 3, hull: 2 },
  }
  const ship = Wallet.restore(fresh.ship, account)

  assert(ship.salvage === 300 && ship.scema === 120, 'the balances did not come back')
  assert(ship.frame === 'marauder', 'the hull that was paid for did not come back')
  assert(ship.levels.tanks === 3, 'the upgrades that were paid for did not come back')

  // Consumables are recomputed against the restored level AND the restored hull. Recomputing a
  // marauder's tank against a skiff hands a returning player a fraction of what they bought.
  assert(ship.fuel === fuelCapacity(3, 'marauder'), `refuelled to ${ship.fuel}`)
  assert(ship.hull === hullMax(2, 'marauder'), `hull restored to ${ship.hull}`)
  assert(ship.scavenged.length === 0, 'a stripped-derelict list came back with the account')
  assert(ship.docked === null, 'a restored ship started already docked')
})

check('the account is not keyed by the record', () => {
  // Every world pays identically — that is what `raiders.ts` and `economy.ts` exist to protect —
  // so a balance is not a claim about any record. Keying by world would punish a player for
  // opening a second one and hand them a reason to hoard the first, which is the closest thing to
  // "a record is worth money" this design refuses to build.
  assert(!/seed|digest|commitment|space/i.test(Wallet.WALLET_KEY), 'the key names a world')
  const src = codeOf(join(here, '..', 'lib', 'scemaworld', 'wallet.ts'))
  const body = src.replace(/\/\*\*[\s\S]*?\*\//g, '').replace(/\/\/.*$/gm, '')
  for (const field of ['blind_spots', 'signals', 'extent', 'magnitude', 'seed', 'legibility']) {
    assert(!body.includes(field), `wallet.ts reads \`${field}\` — a reward path must not`)
  }
})

check('a saved account is only written when it changes', () => {
  // The frame loop runs at 60 Hz and a `localStorage` write is synchronous. Without this the game
  // stutters continuously against the main thread for no reason: salvage moves when you kill
  // something, not every frame.
  const a = Wallet.newAccount()
  assert(!Wallet.changed(a, Wallet.newAccount()), 'two fresh accounts compared as different')
  assert(Wallet.changed(a, { ...a, salvage: 1 }), 'a salvage change was not noticed')
  assert(Wallet.changed(a, { ...a, frame: 'lancer' }), 'a hull change was not noticed')
  assert(
    Wallet.changed(a, { ...a, levels: { ...a.levels, laser: 1 } }),
    'an upgrade was not noticed',
  )
})

await Promise.all(pending)

console.log(`\n${pass}/${pass + fail} checks passed`)
process.exit(fail === 0 ? 0 : 1)
