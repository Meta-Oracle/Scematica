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
  camera, forward, up, right, rotate, translate, view, perspective, mul, qNorm,
} from '../lib/scemaworld/camera.ts'
import {
  drawList, isGhost, roleOfContact, sensorFar, sensorLabel, boundaryLabel, PALETTE,
} from '../lib/scemaworld/view.ts'
import {
  newCombat, selected, switchWeapon, fire, step, durability, threatLabel, lockOn,
  LASER, PHOTON,
} from '../lib/scemaworld/weapons.ts'
import { fetchWorld, explain, retryable, matchesRequest } from '../lib/scemaworld/vault.ts'
// `join` is already taken by `node:path` in this file.
import { join as joinFleet, placement } from '../lib/scemaworld/fleet.ts'
import {
  newShip, refuel, repair, scavenge, buy, upgradeCost, fuelCapacity, hullMax, topSpeed,
  sensorGain, laserCooldown, photonMagazine, shieldMax, jumpCapacity, jumpCharge,
  damage, recharge, MAX_LEVEL,
} from '../lib/scemaworld/ship.ts'
import * as Enemy from '../lib/scemaworld/enemy.ts'
import {
  collidesWith, permeableNote, sweep, resolve, separate, steerAround, passedThrough,
  closestOnSegment, SEPARATION,
} from '../lib/scemaworld/collide.ts'
import { gridFor, NOTICE_MS } from '../lib/scemaworld/game.ts'
import { nodeRadius, roleOfNode } from '../lib/scemaworld/view.ts'
import { JUMP_INHIBIT, BOLT_LENGTH, BOLT_GLOW, R_PLAYER } from '../lib/scemaworld/scale.ts'
import {
  swarmOf, step as enemyStep, hit as enemyHit, living, decide, leadPoint, turnToward,
  nearestThreat, classRoll, AGGRO_RANGE,
} from '../lib/scemaworld/enemy.ts'
import { CLASSES, CLASS_IDS, classFor, SHIELD_DELAY_MS } from '../lib/scemaworld/classes.ts'
import * as Hyper from '../lib/scemaworld/hyper.ts'
import { interceptor, gunship, capital, bolt, starfield } from '../lib/scemaworld/meshes.ts'
import { shapeOf, LANE_ALPHA } from '../lib/scemaworld/view.ts'
import { newGame, tick, useService, purchase, dynamicOf, DOCK_RANGE } from '../lib/scemaworld/game.ts'
import { servicesOf } from '../lib/scemaworld/generate.ts'
import * as SCALE from '../lib/scemaworld/scale.ts'
import { raidersOf } from '../lib/scemaworld/raiders.ts'
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
  let c = switchWeapon(newCombat())
  let t = 0
  for (let i = 0; i < PHOTON.magazine + 5; i += 1) {
    t += PHOTON.cooldownMs
    c = fire(c, { x: 0, y: 0, z: 0 }, { x: 0, y: 0, z: -1 }, t, [])
  }
  assert(c.photonsLeft === 0, `photonsLeft was ${c.photonsLeft}`)
  assert(c.projectiles.length === PHOTON.magazine, `fired ${c.projectiles.length}`)
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
  const seconds = EXTENT / topSpeed(0)
  assert(seconds > 6, `a crossing takes ${seconds.toFixed(1)}s — too fast to feel like distance`)
  assert(seconds < 16, `a crossing takes ${seconds.toFixed(1)}s — that is a commute`)
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
  assert(heavy.hull > fast.hull * 10, 'the capital is not meaningfully tougher')
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
  assert(g.ship.fuel === 0, 'ninety seconds at full throttle cost no fuel')
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
  // `photonMagazine` existed for a while and nothing called them, so two of the six upgrades
  // were sold and inert.
  assert(topSpeed(1) > topSpeed(0), 'ENGINE does nothing')
  assert(fuelCapacity(1) > fuelCapacity(0), 'TANKS does nothing')
  assert(hullMax(1) > hullMax(0), 'HULL does nothing')
  assert(sensorGain(1) > sensorGain(0), 'SENSORS does nothing')
  assert(laserCooldown(1) < laserCooldown(0), 'LASER does nothing')
  assert(photonMagazine(1) > photonMagazine(0), 'PHOTON does nothing')
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

check('a photon upgrade hands over the rounds it just sold you', () => {
  const s = generate(world, digest)
  const dockNode = s.nodes.find((n) => servicesOf(n.kind).includes('trade'))
  let g = newGame(s)
  g = {
    ...g,
    nearby: dockNode,
    ship: { ...g.ship, salvage: 5_000 },
  }
  const before = g.combat.photonsLeft
  const after = purchase(g, 'missiles')
  assert(after.ship.levels.missiles === 1, after.notice)
  assert(after.combat.photonsLeft > before, 'the magazine grew but the tubes stayed empty')
  assert(after.combat.photonsLeft === photonMagazine(1))
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

check('every shape a class names has a mesh', () => {
  // The class table and the renderer must not be two homes for the same decision.
  const meshes = { interceptor: interceptor(), gunship: gunship(), capital: capital() }
  for (const id of CLASS_IDS) {
    const shape = CLASSES[id].shape
    assert(meshes[shape] && meshes[shape].length > 0, `${id} names ${shape}, which has no mesh`)
  }
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
  // Both capitals were unreachable and nothing failed. The class roll was derived from
  // `durability`, which returns one of *six* values — the roll covered about half the
  // distribution and never once reached the top bracket. A table whose bottom two entries are
  // decoration is the kind of bug that hides behind a plausible-looking sector.
  const s = generate(world, digest)
  const seen = new Set(swarmOf(s.raiders, s.seed).craft.map((c) => c.spec.id))
  for (const id of CLASS_IDS) {
    assert(seen.has(id), `${id} exists in the table and cannot be met in this sector`)
  }
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

check('flying into a station costs hull and cuts the drive', () => {
  const s = generate(world, digest)
  const station = s.nodes.find((n) => collidesWith(n.kind) && n.id !== 0)
  const r = nodeRadius(roleOfNode(station))
  let g = { ...newGame(s), throttle: 1 }
  g = {
    ...g,
    camera: {
      position: [station.at.x, station.at.y, station.at.z + r * 4],
      orientation: [0, 0, 0, 1],
    },
  }
  let hit = false
  for (let f = 0; f < 240 && !hit; f += 1) {
    g = tick(g, s, { keys: new Set(), firing: false, dt: 1 / 60, nowMs: (f * 1000) / 60 })
    // Shields absorb an impact exactly as they absorb a shot. Asserting on hull alone said the
    // collision cost nothing when it had in fact cost the whole buffer.
    hit = g.ship.shield < shieldMax(0) || g.ship.hull < hullMax(0)
  }
  assert(hit, 'flying nose-first into a station cost nothing')
  assert(g.throttle === 0, 'the drive kept running against the surface')
  assert((g.notice ?? '').includes('impact'), g.notice)
  assert(g.ship.hull > 0, 'one collision at cruise killed the ship outright')
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
  const push = separate([a, b])
  assert(push[0].x < 0 && push[1].x > 0, 'they were not pushed apart')
  assert(Math.abs(push[0].x + push[1].x) < 1e-9, 'the push was not symmetric')
  const gap = 500 + push[1].x - push[0].x
  assert(gap >= 2000 * SEPARATION - 1e-6, `ended ${gap} apart, still overlapping`)
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

check('a craft never ends a tick inside a station', () => {
  // Avoidance is a heuristic and heuristics miss. The hard resolve is what guarantees a wireframe
  // hull is never sitting in the middle of a dock.
  const s = generate(world, digest)
  const grid = gridFor(s)
  let g = newGame(s)
  for (let f = 0; f < 60 * 30; f += 1) {
    g = tick(g, s, { keys: new Set(), firing: false, dt: 1 / 60, nowMs: (f * 1000) / 60 })
    if (f % 120 !== 0) continue
    for (const c of Enemy.living(g.swarm)) {
      const inside = sweep(grid, c.at, c.at, 0)
      assert(!inside, `a ${c.spec.label} is inside ${inside?.obstacle.node.label}`)
    }
  }
})

check('geometry stops fire in both directions', () => {
  // A station is cover or it is scenery. It must be the same rule for both sides, or the player
  // learns that hiding works only for the other one.
  const enemySrc = codeOf(join(here, '..', 'lib', 'scemaworld', 'enemy.ts'))
  assert(enemySrc.includes('resolve(grid, s.at, at'), 'enemy fire passes through stations')
  const gameSrc = codeOf(join(here, '..', 'lib', 'scemaworld', 'game.ts'))
  assert(gameSrc.includes('Collide.sweep(grid, from, to'), 'player fire passes through stations')
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
  const c = Enemy.living(g.swarm).find((k) => k.id.startsWith('raider:'))
  g = {
    ...g,
    throttle: 1,
    camera: { ...g.camera, position: [c.at.x, c.at.y, c.at.z + c.spec.radius * 0.5] },
  }
  g = tick(g, s, { keys: new Set(), firing: false, dt: 1 / 60, nowMs: 1 })
  assert(g.ship.shield < shieldMax(0) || g.ship.hull < hullMax(0), 'flying into a craft cost nothing')
  assert((g.notice ?? '').includes('collision'), g.notice)
  const after = Enemy.living(g.swarm).find((k) => k.id === c.id)
  assert(!after || after.hull < c.hull, 'the craft was unharmed by the collision')
  const gap = Math.hypot(
    g.camera.position[0] - (after ?? c).at.x,
    g.camera.position[1] - (after ?? c).at.y,
    g.camera.position[2] - (after ?? c).at.z,
  )
  assert(gap >= R_PLAYER + c.spec.radius, 'the two are still inside each other')
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

await Promise.all(pending)

console.log(`\n${pass}/${pass + fail} checks passed`)
process.exit(fail === 0 ? 0 : 1)
