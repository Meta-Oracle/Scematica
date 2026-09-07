import { generate } from '../lib/scemaworld/generate.ts'
import { newGame, tick } from '../lib/scemaworld/game.ts'
import * as Enemy from '../lib/scemaworld/enemy.ts'
import { clusterOf } from '../lib/scemaworld/clusters.ts'
import * as Respawn from '../lib/scemaworld/respawn.ts'
import { readFileSync } from 'node:fs'
const d0 = '../scematica-omni/crates/scema-nft/fixtures'
const world = JSON.parse(readFileSync(d0 + '/parity-world.json', 'utf8'))
const digest = readFileSync(d0 + '/parity-digest.txt', 'utf8').trim()
const s = generate(world, digest)

const caps = (g) => g.swarm.craft.filter((c) => c.alive && c.spec.capital && clusterOf(c.id) === null)
const byClass = (g) => {
  const o = {}
  for (const c of caps(g)) o[`${c.faction}:${c.spec.id}`] = (o[`${c.faction}:${c.spec.id}`] ?? 0) + 1
  return o
}

let g = newGame(s)
console.log('at load:', JSON.stringify(byClass(g)), '| nextCapitalMs', g.waves.nextCapitalMs)
// Kill every roster capital, both sides.
g = { ...g, swarm: { ...g.swarm, craft: g.swarm.craft.map((c) =>
  c.spec.capital && clusterOf(c.id) === null ? { ...c, alive: false } : c) } }
console.log('after purge:', JSON.stringify(byClass(g)))

// Run the REAL tick, with a clock that starts where a page-load clock would.
const dt = 1 / 10
const t0 = 30_000 // the player spent 30s on the opening page
for (let f = 1; f <= 10 * 60 * 10; f += 1) {
  const nowMs = t0 + f * dt * 1000
  g = tick(g, s, { keys: new Set(), firing: false, dt, nowMs })
  g = { ...g, lost: false, ship: { ...g.ship, hull: 9e9, shield: 9e9 } }
  if (f % (60 * 10) === 0) {
    console.log(`t+${(f * dt).toFixed(0)}s`, JSON.stringify(byClass(g)),
      '| next', Math.round(g.waves.nextCapitalMs), '| now', Math.round(nowMs),
      '| waves', JSON.stringify(g.waves.raiderCapitals), JSON.stringify(g.waves.marshalCapitals))
  }
}
