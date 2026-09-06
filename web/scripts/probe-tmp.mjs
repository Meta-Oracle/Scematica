import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { dirname, join } from 'node:path'
import { generate } from '../lib/scemaworld/generate.ts'
import { newGame, tick } from '../lib/scemaworld/game.ts'

const here = dirname(fileURLToPath(import.meta.url))
const nftDir = join(here, '..', '..', 'scematica-omni', 'crates', 'scema-nft', 'fixtures')
const world = JSON.parse(readFileSync(join(nftDir, 'parity-world.json'), 'utf8'))
const digest = readFileSync(join(nftDir, 'parity-digest.txt'), 'utf8').trim()
const s = generate(world, digest)

let g = newGame(s)
const caps = () =>
  g.swarm.craft.filter((c) => c.spec.capital).map((c) => ({
    id: c.id, f: c.faction, k: c.spec.id, alive: c.alive,
    hull: Math.round(c.hull), max: c.spec.hull,
  }))

// How far apart are the opposing capitals, against how far they can see?
{
  const cs = g.swarm.craft.filter((c) => c.spec.capital && c.alive)
  for (const a of cs) {
    let best = Infinity, who = null
    for (const b of cs) {
      if (b.faction === a.faction) continue
      const d = Math.hypot(b.at.x-a.at.x, b.at.y-a.at.y, b.at.z-a.at.z)
      if (d < best) { best = d; who = b }
    }
    console.log(`  ${a.faction.padEnd(8)} ${a.spec.id.padEnd(12)} nearest enemy capital ${Math.round(best/1e9)}e9  aggro ${Math.round(a.spec.aggro/1e9)}e9  target=${a.target}`)
  }
  // And the nearest opposing craft of ANY class, which is what it would actually shoot.
  for (const a of cs) {
    let best = Infinity
    for (const b of g.swarm.craft) {
      if (!b.alive || b.faction === a.faction) continue
      if (b.faction !== 'raider' && b.faction !== 'marshal') continue
      const d = Math.hypot(b.at.x-a.at.x, b.at.y-a.at.y, b.at.z-a.at.z)
      if (d < best) best = d
    }
    console.log(`  ${a.faction.padEnd(8)} ${a.spec.id.padEnd(12)} nearest ANY opponent ${Math.round(best/1e9)}e9 (aggro ${Math.round(a.spec.aggro/1e9)}e9)`)
  }
}

const start = caps()
console.log('capitals at t=0:')
for (const c of start) console.log('  ', c.f.padEnd(8), c.k.padEnd(12), `${c.hull}/${c.max}`)

const dt = 1 / 20
let capShots = 0
for (let i = 1; i <= Math.round(600 / dt); i += 1) {
  const before = g.swarm.shots?.length ?? 0
  g = tick(g, s, { keys: new Set(), firing: false, dt, nowMs: i * dt * 1000 })
  g = { ...g, lost: false, ship: { ...g.ship, hull: 9e9, shield: 9e9 } }
  if (i % Math.round(120 / dt) === 0) {
    for (const c of g.swarm.craft.filter((x) => x.spec.capital && x.alive)) {
      let best = Infinity
      for (const b of g.swarm.craft) {
        if (!b.alive || !((b.faction==='raider')!==(c.faction==='raider'))) continue
        if (b.faction!=='raider' && b.faction!=='marshal') continue
        const d = Math.hypot(b.at.x-c.at.x,b.at.y-c.at.y,b.at.z-c.at.z)
        if (d<best) best=d
      }
      console.log('    ', c.faction.padEnd(8), c.spec.id.padEnd(12),
        'beh='+String(c.behaviour).padEnd(9), 'tgt='+String(c.target).slice(0,22).padEnd(24),
        'nearestFoe='+(best/3.2e9).toFixed(2)+'E', 'aggro='+(c.spec.aggro/3.2e9).toFixed(2)+'E')
    }
    const now = caps()
    const dmg = start.map((a, j) => a.hull - now[j].hull).reduce((x, y) => x + y, 0)
    const dead = now.filter((c) => !c.alive).length
    console.log(`t=${Math.round(i * dt)}s  total capital damage taken ${dmg}  destroyed ${dead}`)
  }
}
console.log('\ncapitals at the end:')
for (const c of caps()) console.log('  ', c.f.padEnd(8), c.k.padEnd(12), `${c.hull}/${c.max}`, c.alive ? '' : 'DESTROYED')
