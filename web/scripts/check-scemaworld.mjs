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

function check(name, fn) {
  try {
    fn()
    console.log(`  ok   ${name}`)
    pass += 1
  } catch (e) {
    console.log(`  FAIL ${name}\n       ${e.message}`)
    fail += 1
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

check('unknown sensor range gives a null draw distance, never a default', () => {
  // Picking a number here would tell the player the map is small when the truth is that
  // nobody measured it.
  const list = drawList(generate({ ...world, objects: [] }, digest))
  assert(list.far === null, `far was ${list.far}`)
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

check('a contact takes its durability in hits and then is destroyed once', () => {
  const s = generate(world, digest)
  const target = { ...firstSolid(s), at: { x: 0, y: 0, z: -100000 } }
  const need = durability(s.seed, target.id)
  let c = newCombat()
  let destroyed = 0
  for (let i = 0; i < need + 6; i += 1) {
    c = fire(c, { x: 0, y: 0, z: 0 }, { x: 0, y: 0, z: -1 }, i * 200, [target])
    const r = step(c, 0.6, [target], s.seed)
    c = r.combat
    destroyed += r.hits.filter((h) => h.destroyed).length
  }
  assert(destroyed === 1, `destroyed fired ${destroyed} times`)
  assert(c.destroyed.includes(target.id), 'the contact was never marked destroyed')
})

console.log(`\n${pass}/${pass + fail} checks passed`)
process.exit(fail === 0 ? 0 : 1)
