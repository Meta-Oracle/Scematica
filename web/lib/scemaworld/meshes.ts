/**
 * The shapes, as vertex data. Pure arithmetic — no GL type appears in this file.
 *
 * Kept out of `gl.ts` so a silhouette can be tested without a canvas, and because the hulls are
 * the one part of the renderer that is a *design* decision rather than plumbing: an interceptor
 * has to read as an interceptor at a glance, from any angle, at a distance where it is forty
 * pixels across.
 *
 * ## Why the ships are line models
 *
 * A shaded solid at these sizes is a grey blob with a highlight on it. A wireframe reads its own
 * silhouette at any distance, needs no lighting to be legible, and — the reason that matters
 * here — makes **facing** obvious, which is the single most important thing to know about an
 * opponent in a dogfight. You cannot tell which way a sphere is pointing.
 *
 * Every hull points along **+Z** and is normalised to roughly unit radius, so `gl.ts` can build
 * one basis from a facing vector and scale by a single radius.
 */

/** A line list: pairs of points, `[x,y,z, x,y,z, ...]`. */
export type Wire = Float32Array

/**
 * The extent of a mesh along its own axes, in local units.
 *
 * `ahead` and `behind` are along +Z (the nose) and −Z; `cross` is the largest distance from the
 * axis. A hull is not a sphere and the difference is not cosmetic: `dreadnought` reaches 2.1
 * forward and 0.72 sideways, so a bounding sphere of radius 1 misses the prow entirely and
 * covers a great deal of empty space beside the ship.
 */
export interface Bounds {
  ahead: number
  behind: number
  cross: number
}

/**
 * Measure a mesh.
 *
 * **Measured, never declared.** A hand-written table of extents is a second description of a
 * shape, and the two drift the first time a silhouette is tweaked — at which point the hit test
 * and the picture disagree, which is the failure this project has now paid for twice. Deriving
 * the numbers from the vertex data means a mesh edit moves the hitbox with it, by construction.
 */
export function boundsOf(w: Wire): Bounds {
  let ahead = 0
  let behind = 0
  let cross = 0
  for (let i = 0; i < w.length; i += 3) {
    const z = w[i + 2]
    if (z > ahead) ahead = z
    if (-z > behind) behind = -z
    const r = Math.hypot(w[i], w[i + 1])
    if (r > cross) cross = r
  }
  return { ahead, behind, cross }
}

function wire(points: number[][], edges: [number, number][]): Wire {
  const out: number[] = []
  for (const [a, b] of edges) {
    out.push(...points[a], ...points[b])
  }
  return new Float32Array(out)
}

/**
 * A fighter: a dart with swept wings and a tail fin.
 *
 * Deliberately asymmetric top-to-bottom. A shape with a distinguishable "up" lets a player read
 * an opponent's roll, which is what tells you which way it is about to break.
 */
export function interceptor(): Wire {
  const p: number[][] = [
    [0, 0, 1.35], // 0 nose
    [-0.85, -0.12, -0.7], // 1 port wingtip
    [0.85, -0.12, -0.7], // 2 starboard wingtip
    [0, 0.16, -0.55], // 3 spine
    [0, -0.1, -0.45], // 4 belly
    [0, 0.62, -0.85], // 5 fin
    [-0.3, -0.05, -0.8], // 6 port engine
    [0.3, -0.05, -0.8], // 7 starboard engine
  ]
  const e: [number, number][] = [
    [0, 1], [0, 2], [0, 3], [0, 4],
    [1, 3], [2, 3], [1, 4], [2, 4],
    [1, 6], [2, 7], [6, 7],
    [3, 5], [5, 6], [5, 7],
  ]
  let n = p.length
  // ## The raider signature: nothing lines up
  //
  // This and `gunship` are the two oldest meshes in the file and were the two thinnest — fourteen
  // and twenty-three segments against a family that now runs to sixty. What they gain is not detail
  // for its own sake but the thing every other class now says about itself: **who built it**. An
  // engine on one side larger than the other, a plate bolted over one wing root, an aerial welded
  // to the fin. A freehold hull was not built as a set, and the patrol's hulls are regular
  // precisely so that irregularity reads as a faction rather than as a modelling slip.
  const big = ring(p, 6, 0.17, -0.8, 'xy')
  for (let i = 0; i < 6; i += 1) { p[big + i][0] -= 0.3; p[big + i][1] -= 0.05 }
  const bigEnd = ring(p, 6, 0.2, -1.15, 'xy')
  for (let i = 0; i < 6; i += 1) { p[bigEnd + i][0] -= 0.3; p[bigEnd + i][1] -= 0.05 }
  e.push(...loop(big, 6), ...loop(bigEnd, 6))
  for (let i = 0; i < 6; i += 1) e.push([big + i, bigEnd + i])
  n = p.length
  const small = ring(p, 5, 0.11, -0.8, 'xy')
  for (let i = 0; i < 5; i += 1) { p[small + i][0] += 0.3; p[small + i][1] -= 0.05 }
  const smallEnd = ring(p, 5, 0.13, -1.0, 'xy')
  for (let i = 0; i < 5; i += 1) { p[smallEnd + i][0] += 0.3; p[smallEnd + i][1] -= 0.05 }
  e.push(...loop(small, 5), ...loop(smallEnd, 5))
  for (let i = 0; i < 5; i += 1) e.push([small + i, smallEnd + i])
  n = p.length
  // A patch plate over the port wing root, and an aerial off the fin.
  p.push([-0.5, -0.02, -0.35], [-0.2, 0.04, -0.3], [-0.24, -0.08, -0.62], [-0.54, -0.14, -0.66])
  e.push([n, n + 1], [n + 1, n + 2], [n + 2, n + 3], [n + 3, n], [n, n + 2])
  n += 4
  p.push([0.06, 0.9, -0.8], [0, 0.62, -0.85])
  e.push([n, n + 1], [n + 1, 5])
  n += 2
  return wire(p, e)
}

/**
 * A gunship: blunter, wider, with a visible weapon boom on each flank.
 *
 * Reads as *heavy* because it is short and broad where the interceptor is long and thin, which
 * is a silhouette difference rather than a size difference — the two are distinguishable even
 * when one is far away and the other is close.
 */
export function gunship(): Wire {
  const p: number[][] = [
    [0, 0, 1.0], // 0 prow
    [-0.55, 0.3, 0.2], [0.55, 0.3, 0.2], [-0.55, -0.3, 0.2], [0.55, -0.3, 0.2],
    [-0.5, 0.25, -0.9], [0.5, 0.25, -0.9], [-0.5, -0.25, -0.9], [0.5, -0.25, -0.9],
    [-1.0, 0, 0.55], [1.0, 0, 0.55], [-1.0, 0, -0.5], [1.0, 0, -0.5],
  ]
  const e: [number, number][] = [
    [0, 1], [0, 2], [0, 3], [0, 4],
    [1, 2], [3, 4], [1, 3], [2, 4],
    [1, 5], [2, 6], [3, 7], [4, 8],
    [5, 6], [7, 8], [5, 7], [6, 8],
    [9, 11], [10, 12], [9, 1], [10, 2], [11, 5], [12, 6], [9, 3], [10, 4],
  ]
  let n = p.length
  // The raider heavy fighter, given the same signature as the rest of its family: mismatched
  // engines, one outboard tank rather than a matched pair, and armour patched on off-centre.
  for (const [s, r, back] of [[-1, 0.2, -1.1], [1, 0.15, -0.9]] as [number, number, number][]) {
    const b = ring(p, 6, r, -0.9, 'xy')
    for (let i = 0; i < 6; i += 1) p[b + i][0] += s * 0.28
    const f = ring(p, 6, r * 1.2, back - 0.3, 'xy')
    for (let i = 0; i < 6; i += 1) p[f + i][0] += s * 0.28
    e.push(...loop(b, 6), ...loop(f, 6))
    for (let i = 0; i < 6; i += 1) e.push([b + i, f + i])
    n = p.length
  }
  const t0 = ring(p, 5, 0.16, 0.3, 'xy')
  for (let i = 0; i < 5; i += 1) { p[t0 + i][0] -= 0.78; p[t0 + i][1] -= 0.18 }
  const t1 = ring(p, 5, 0.16, -0.55, 'xy')
  for (let i = 0; i < 5; i += 1) { p[t1 + i][0] -= 0.78; p[t1 + i][1] -= 0.18 }
  e.push(...loop(t0, 5), ...loop(t1, 5))
  for (let i = 0; i < 5; i += 1) e.push([t0 + i, t1 + i])
  n = p.length
  p.push([-0.78, -0.18, -0.1], [-0.5, -0.06, -0.1])
  e.push([n, n + 1])
  n += 2
  p.push([-0.3, 0.3, 0.55], [0.16, 0.32, 0.6], [0.2, 0.06, 0.42], [-0.26, 0.04, 0.38])
  e.push([n, n + 1], [n + 1, n + 2], [n + 2, n + 3], [n + 3, n], [n, n + 2])
  n += 4
  return wire(p, e)
}

/**
 * A capital: a long wedge with a dorsal superstructure and a ribbed spine.
 *
 * The ribs are the point. A smooth wedge at capital scale has no sense of size — nothing on it
 * tells you how far away it is — and the ribs give the eye a repeated feature to judge distance
 * by. It is the same trick a corridor of identical doors plays, and it is what makes a destroyer
 * read as *enormous* rather than as a near triangle.
 */
export function capital(): Wire {
  const p: number[][] = [
    [0, 0, 1.6], // 0 prow
    [-0.62, 0.14, -1.0], // 1
    [0.62, 0.14, -1.0], // 2
    [-0.5, -0.2, -1.0], // 3
    [0.5, -0.2, -1.0], // 4
    [0, 0.45, -0.55], // 5 tower
    [0, 0.45, -0.95], // 6
    [-0.2, 0.16, -1.05], // 7 engines
    [0.2, 0.16, -1.05],
    [-0.2, -0.1, -1.05],
    [0.2, -0.1, -1.05],
  ]
  const edges: [number, number][] = [
    [0, 1], [0, 2], [0, 3], [0, 4],
    [1, 2], [3, 4], [1, 3], [2, 4],
    [5, 6], [5, 1], [5, 2], [6, 1], [6, 2],
    [7, 8], [9, 10], [7, 9], [8, 10],
  ]
  // Ribs: rings across the hull at intervals down its length.
  let next = p.length
  for (let i = 1; i <= 5; i += 1) {
    const t = i / 6
    const z = 1.6 + t * -2.6
    const w = 0.62 * t + 0.06
    const hTop = 0.14 * t + 0.02
    const hBot = -0.2 * t - 0.02
    p.push([-w, hTop, z], [w, hTop, z], [w, hBot, z], [-w, hBot, z])
    edges.push([next, next + 1], [next + 1, next + 2], [next + 2, next + 3], [next + 3, next])
    next += 4
  }
  return wire(p, edges)
}

/**
 * A corvette: the player's all-rounder.
 *
 * Chunkier than an interceptor and clearly a *hull* rather than a dart, with a visible cockpit
 * spine and four engine nacelles. Player ships get their own shapes because in third person you
 * are looking at yours for the whole session, and reusing an enemy silhouette would make the
 * thing you identify with indistinguishable from the thing shooting at you.
 */
export function corvette(): Wire {
  const p: number[][] = [
    [0, 0.02, 1.25], // 0 nose
    [-0.34, 0.16, 0.35], [0.34, 0.16, 0.35], [-0.34, -0.16, 0.35], [0.34, -0.16, 0.35],
    [-0.42, 0.14, -0.75], [0.42, 0.14, -0.75], [-0.42, -0.14, -0.75], [0.42, -0.14, -0.75],
    [0, 0.34, -0.1], // 9 cockpit spine
    [-0.8, 0, -0.2], [0.8, 0, -0.2], // 10,11 wingtips
    [-0.8, 0, -0.7], [0.8, 0, -0.7], // 12,13
  ]
  const e: [number, number][] = [
    [0, 1], [0, 2], [0, 3], [0, 4],
    [1, 2], [3, 4], [1, 3], [2, 4],
    [1, 5], [2, 6], [3, 7], [4, 8],
    [5, 6], [7, 8], [5, 7], [6, 8],
    [9, 1], [9, 2], [9, 5], [9, 6],
    [10, 12], [11, 13], [10, 1], [11, 2], [12, 5], [13, 6],
  ]
  let n = p.length

  // ## The identity: a wing fence on each side
  //
  // The all-rounder's problem is that having no speciality also means having no feature, and the
  // first version leaned on that — a plain box with four nacelles, which is what every "generic
  // ship" looks like. A **fence** standing off each wing gives it a profile from directly above and
  // below, which is the angle a ship in a turning fight is most often seen from, and costs it
  // nothing in character: a fence is what you put on a wing that has to work at every speed.
  for (const s of [-1, 1]) {
    p.push([s * 0.6, 0, -0.25], [s * 0.6, 0.3, -0.35], [s * 0.6, 0.3, -0.62], [s * 0.6, 0, -0.62])
    e.push([n, n + 1], [n + 1, n + 2], [n + 2, n + 3], [n + 3, n])
    n += 4
    p.push([s * 0.6, -0.24, -0.35], [s * 0.6, -0.24, -0.62])
    e.push([n, n - 4], [n + 1, n - 1], [n, n + 1])
    n += 2
  }

  // A canopy, forward of the spine. It is the only hull in the light tier with somewhere to sit
  // that reads as a place rather than as a vertex.
  p.push([-0.14, 0.24, 0.55], [0.14, 0.24, 0.55], [0.12, 0.3, 0.1], [-0.12, 0.3, 0.1])
  e.push([n, n + 1], [n + 1, n + 2], [n + 2, n + 3], [n + 3, n], [n, 1], [n + 1, 2], [n + 2, 9], [n + 3, 9])
  n += 4

  // Belly hardpoints: two short pylons under the hull, so the underside is not a flat panel.
  for (const s of [-1, 1]) {
    p.push([s * 0.2, -0.2, 0.1], [s * 0.2, -0.34, 0.15], [s * 0.2, -0.34, -0.3], [s * 0.2, -0.2, -0.35])
    e.push([n, n + 1], [n + 1, n + 2], [n + 2, n + 3], [n + 3, n])
    n += 4
  }

  // Four nacelles at the stern, each a short box with a nozzle ring rather than a bare line.
  for (const [x, y] of [[-0.28, 0.1], [0.28, 0.1], [-0.28, -0.1], [0.28, -0.1]] as [number, number][]) {
    const first = ring(p, 5, 0.09, -0.75, 'xy')
    for (let i = 0; i < 5; i += 1) { p[first + i][0] += x; p[first + i][1] += y }
    const back = ring(p, 5, 0.11, -1.05, 'xy')
    for (let i = 0; i < 5; i += 1) { p[back + i][0] += x; p[back + i][1] += y }
    e.push(...loop(first, 5), ...loop(back, 5))
    for (let i = 0; i < 5; i += 1) e.push([first + i, back + i])
    n = p.length
  }
  return wire(p, e)
}

/**
 * A marauder: the heaviest thing a player can fly.
 *
 * Broad, slab-sided and ribbed, so it reads as *mass*. Deliberately close in feel to a capital
 * without being one — you are meant to look at it and believe it can stand in front of a titan.
 */
export function marauder(): Wire {
  const p: number[][] = [
    [0, 0, 1.3],
    [-0.55, 0.28, 0.3], [0.55, 0.28, 0.3], [-0.55, -0.28, 0.3], [0.55, -0.28, 0.3],
    [-0.7, 0.3, -0.95], [0.7, 0.3, -0.95], [-0.7, -0.3, -0.95], [0.7, -0.3, -0.95],
    [0, 0.6, -0.3], [0, -0.6, -0.3],
    [-1.05, 0.05, -0.3], [1.05, 0.05, -0.3],
  ]
  const e: [number, number][] = [
    [0, 1], [0, 2], [0, 3], [0, 4],
    [1, 2], [3, 4], [1, 3], [2, 4],
    [1, 5], [2, 6], [3, 7], [4, 8],
    [5, 6], [7, 8], [5, 7], [6, 8],
    [9, 1], [9, 2], [9, 5], [9, 6],
    [10, 3], [10, 4], [10, 7], [10, 8],
    [11, 1], [11, 3], [11, 5], [12, 2], [12, 4], [12, 6],
  ]
  let n = p.length

  // ## The identity: outboard armour slabs
  //
  // It is meant to read as *mass* and it was doing so with four ribs, which is the same cue every
  // capital uses and half as many of them. Two slabs standing proud of the flanks — plate carried
  // outside the hull rather than as part of it — is a feature nothing else here has, and it is the
  // honest picture of a ship whose whole argument is that it can be hit.
  for (const s of [-1, 1]) {
    const b = n
    p.push(
      [s * 0.78, 0.34, 0.2], [s * 0.96, 0.3, 0.0], [s * 0.96, 0.3, -0.8], [s * 0.78, 0.34, -0.95],
      [s * 0.78, -0.34, 0.2], [s * 0.96, -0.3, 0.0], [s * 0.96, -0.3, -0.8], [s * 0.78, -0.34, -0.95],
    )
    e.push(
      [b, b + 1], [b + 1, b + 2], [b + 2, b + 3],
      [b + 4, b + 5], [b + 5, b + 6], [b + 6, b + 7],
      [b, b + 4], [b + 1, b + 5], [b + 2, b + 6], [b + 3, b + 7],
      [b, 1], [b + 3, 5], [b + 4, 3], [b + 7, 7],
    )
    n += 8
  }

  // Ribs across the dorsal surface, and now the ventral one too — a slab-sided hull that is ribbed
  // on one face only reads as having a top and no bottom.
  for (let i = 1; i <= 5; i += 1) {
    const t = i / 6
    const z = 0.3 - t * 1.25
    const w = 0.55 + 0.15 * t
    p.push([-w, 0.28, z], [w, 0.28, z], [-w, -0.28, z], [w, -0.28, z])
    e.push([n, n + 1], [n + 2, n + 3], [n, n + 2], [n + 1, n + 3])
    n += 4
  }

  // A blunt prow plate: the nose is a *face*, not a point, on the one light hull built to ram.
  p.push([-0.22, 0.14, 1.2], [0.22, 0.14, 1.2], [0.22, -0.14, 1.2], [-0.22, -0.14, 1.2])
  e.push([n, n + 1], [n + 1, n + 2], [n + 2, n + 3], [n + 3, n], [n, 0], [n + 1, 0], [n + 2, 0], [n + 3, 0])
  n += 4

  // Twin heavy exhausts.
  for (const s of [-1, 1]) {
    const first = ring(p, 6, 0.2, -0.95, 'xy')
    for (let i = 0; i < 6; i += 1) p[first + i][0] += s * 0.34
    const back = ring(p, 6, 0.24, -1.3, 'xy')
    for (let i = 0; i < 6; i += 1) p[back + i][0] += s * 0.34
    e.push(...loop(first, 6), ...loop(back, 6))
    for (let i = 0; i < 6; i += 1) e.push([first + i, back + i])
    n = p.length
  }
  return wire(p, e)
}

/**
 * A war hull: a dreadnought or a leviathan.
 *
 * Not the capital mesh scaled up, and the difference matters. A shape only ever seen very large
 * has to carry *more* detail, not the same detail stretched — at this size the eye is close
 * enough to individual features to notice their absence, and a plain wedge fifteen stations long
 * reads as a flat triangle rather than as a hull.
 *
 * So it is longer in proportion, deeply ribbed along its whole length, and carries a dorsal
 * spine, flanking sponsons and an engine bank. The ribs are load-bearing for the same reason as
 * on the capital and more so: they are the only cue for how far away the thing is, and without
 * them a leviathan at range is indistinguishable from a fighter nearby.
 */
export function dreadnought(): Wire {
  const p: number[][] = [
    [0, 0, 2.1], // 0 prow
    [-0.5, 0.12, 1.2], [0.5, 0.12, 1.2], [-0.42, -0.16, 1.2], [0.42, -0.16, 1.2],
    [-0.72, 0.16, -1.5], [0.72, 0.16, -1.5], [-0.6, -0.24, -1.5], [0.6, -0.24, -1.5],
  ]
  const e: [number, number][] = [
    [0, 1], [0, 2], [0, 3], [0, 4],
    [1, 2], [3, 4], [1, 3], [2, 4],
    [1, 5], [2, 6], [3, 7], [4, 8],
    [5, 6], [7, 8], [5, 7], [6, 8],
  ]

  // Ribs down the whole length. Twelve, not five: a hull this long needs the repetition to read.
  let next = p.length
  for (let i = 1; i <= 12; i += 1) {
    const t = i / 13
    const z = 2.1 - t * 3.6
    const w = 0.16 + 0.56 * t
    const top = 0.1 + 0.06 * t
    const bot = -0.12 - 0.12 * t
    p.push([-w, top, z], [w, top, z], [w, bot, z], [-w, bot, z])
    e.push([next, next + 1], [next + 1, next + 2], [next + 2, next + 3], [next + 3, next])
    // Every third rib gets a dorsal fin, which breaks the silhouette along the top edge.
    if (i % 3 === 0) {
      p.push([0, top + 0.34, z])
      e.push([next + 4, next], [next + 4, next + 1])
      next += 1
    }
    next += 4
  }

  // Dorsal spine, running the length of the ship above the ribs.
  const spine = p.length
  p.push([0, 0.42, 1.4], [0, 0.5, -0.2], [0, 0.44, -1.4])
  e.push([spine, spine + 1], [spine + 1, spine + 2])

  // Sponsons: weapon blisters on each flank, at two thirds of the way back.
  const spon = p.length
  p.push([-1.05, 0, -0.5], [-1.05, 0, -1.1], [1.05, 0, -0.5], [1.05, 0, -1.1])
  e.push([spon, spon + 1], [spon + 2, spon + 3], [spon, 5], [spon + 2, 6], [spon + 1, 7], [spon + 3, 8])

  // Engine bank at the stern.
  const eng = p.length
  for (const x of [-0.42, -0.14, 0.14, 0.42]) p.push([x, -0.02, -1.5], [x, -0.02, -1.8])
  for (let i = 0; i < 4; i += 1) e.push([eng + i * 2, eng + i * 2 + 1])
  e.push([eng + 1, eng + 3], [eng + 3, eng + 5], [eng + 5, eng + 7])

  return wire(p, e)
}

/**
 * A cruiser: the medium tier's own silhouette.
 *
 * The tier between a gunboat and a capital had no shape of its own, and borrowing one would have
 * put a `marauder` on screen at five times a marauder's size — which reads as a rendering fault
 * rather than as a bigger ship, because scale alone is not a silhouette. What separates this from
 * everything below it is **outriggers**: two nacelles carried away from the hull on booms, a
 * feature no fighter has the room for and no capital bothers with.
 *
 * Long, narrow, and asymmetric top-to-bottom like the fighters, so roll still reads.
 */
export function cruiser(): Wire {
  const p: number[][] = [
    [0, 0.04, 1.6], // 0 prow
    [-0.26, 0.18, 0.55], [0.26, 0.18, 0.55], [-0.26, -0.14, 0.55], [0.26, -0.14, 0.55],
    [-0.32, 0.16, -1.05], [0.32, 0.16, -1.05], [-0.32, -0.16, -1.05], [0.32, -0.16, -1.05],
    [0, 0.46, 0.1], // 9 command tower
    [0, -0.34, -0.5], // 10 keel
  ]
  const e: [number, number][] = [
    [0, 1], [0, 2], [0, 3], [0, 4],
    [1, 2], [3, 4], [1, 3], [2, 4],
    [1, 5], [2, 6], [3, 7], [4, 8],
    [5, 6], [7, 8], [5, 7], [6, 8],
    [9, 1], [9, 2], [9, 5], [9, 6],
    [10, 7], [10, 8], [10, 3], [10, 4],
  ]
  let n = p.length
  // The outriggers, with the nacelles now round and banded — this is the top of the medium tier
  // and the last hull that is still a ship, so it should be the best-drawn of them.
  for (const s of [-1, 1]) {
    p.push([s * 0.3, 0.02, -0.2], [s * 0.92, 0.02, -0.2])
    e.push([n, n + 1])
    n += 2
    for (const z of [0.5, 0.05, -0.45, -0.95]) {
      const r = ring(p, 6, 0.13, z, 'xy')
      for (let i = 0; i < 6; i += 1) p[r + i][0] += s * 0.92
      e.push(...loop(r, 6))
      n = p.length
    }
    for (const [dx, dy] of [[0.13, 0], [-0.13, 0], [0, 0.13], [0, -0.13]] as [number, number][]) {
      p.push([s * 0.92 + dx, dy, 0.5], [s * 0.92 + dx, dy, -0.95])
      e.push([n, n + 1])
      n += 2
    }
    // A pylon fairing where the boom meets the nacelle.
    p.push([s * 0.72, 0.1, -0.05], [s * 0.72, 0.1, -0.4], [s * 0.72, -0.08, -0.05], [s * 0.72, -0.08, -0.4])
    e.push([n, n + 1], [n + 2, n + 3], [n, n + 2], [n + 1, n + 3])
    n += 4
  }
  // Beam ribs.
  for (let i = 1; i <= 4; i += 1) {
    const t = i / 5
    const z = 0.55 - t * 1.6
    const w = 0.26 + 0.06 * t
    p.push([-w, 0.17, z], [w, 0.17, z], [w, -0.15, z], [-w, -0.15, z])
    e.push([n, n + 1], [n + 1, n + 2], [n + 2, n + 3], [n + 3, n])
    n += 4
  }
  // The tower gets a mast and a bridge box, so the tallest point is not a single vertex.
  p.push([-0.12, 0.44, 0.3], [0.12, 0.44, 0.3], [0.12, 0.44, -0.15], [-0.12, 0.44, -0.15], [0, 0.72, 0.05])
  e.push([n, n + 1], [n + 1, n + 2], [n + 2, n + 3], [n + 3, n], [n + 4, n], [n + 4, n + 1], [n + 4, n + 2], [n + 4, n + 3], [n + 4, 9])
  n += 5
  return wire(p, e)
}

/**
 * A bulwark: the first hull a player can fly that is genuinely a capital.
 *
 * Ribbed **across the beam** rather than along the length, which is the whole reason it is a
 * separate mesh from `dreadnought` rather than that one drawn larger. The hostile war hulls are
 * long-ribbed and read as a spine receding away from you; this one reads as a wall coming toward
 * you. In a sector where every other large silhouette is trying to kill you, the ship you own has
 * to be identifiable at a glance — the same argument the player fighters already make, and it
 * gets stronger as the hulls get bigger rather than weaker.
 *
 * Wide, flat, deep-keeled, with armoured shoulder blocks at the bow.
 */
export function bulwark(): Wire {
  const p: number[][] = [
    [0, 0, 1.45], // 0 prow
    [-0.62, 0.2, 0.55], [0.62, 0.2, 0.55], [-0.62, -0.26, 0.55], [0.62, -0.26, 0.55],
    [-0.86, 0.22, -1.15], [0.86, 0.22, -1.15], [-0.86, -0.28, -1.15], [0.86, -0.28, -1.15],
    [0, 0.52, -0.15], // 9 bridge
    [0, -0.62, -0.35], // 10 keel
  ]
  const e: [number, number][] = [
    [0, 1], [0, 2], [0, 3], [0, 4],
    [1, 2], [3, 4], [1, 3], [2, 4],
    [1, 5], [2, 6], [3, 7], [4, 8],
    [5, 6], [7, 8], [5, 7], [6, 8],
    [9, 1], [9, 2], [9, 5], [9, 6],
    [10, 3], [10, 4], [10, 7], [10, 8],
  ]
  let n = p.length

  // ## The hangar mouth
  //
  // A rectangular opening in the bow you can see *through* — the identifying feature, and the one
  // thing in the sector that says "this ship carries other ships". At the range a capital is first
  // seen, an outline with a hole in it is a different object from an outline without one, which is
  // more information than any amount of surface detail buys.
  const mouth = n
  p.push(
    [-0.3, 0.1, 1.15], [0.3, 0.1, 1.15], [0.3, -0.14, 1.15], [-0.3, -0.14, 1.15],
    [-0.24, 0.06, 0.6], [0.24, 0.06, 0.6], [0.24, -0.1, 0.6], [-0.24, -0.1, 0.6],
  )
  e.push(
    [mouth, mouth + 1], [mouth + 1, mouth + 2], [mouth + 2, mouth + 3], [mouth + 3, mouth],
    [mouth + 4, mouth + 5], [mouth + 5, mouth + 6], [mouth + 6, mouth + 7], [mouth + 7, mouth + 4],
    [mouth, mouth + 4], [mouth + 1, mouth + 5], [mouth + 2, mouth + 6], [mouth + 3, mouth + 7],
  )
  n = p.length

  // Shoulder blocks: armour boxes either side of the prow, and the feature that reads first at
  // the range a capital is usually seen from.
  for (const s of [-1, 1]) {
    p.push(
      [s * 0.5, 0.24, 0.95], [s * 0.86, 0.24, 0.75],
      [s * 0.5, -0.1, 0.95], [s * 0.86, -0.1, 0.75],
    )
    e.push([n, n + 1], [n + 2, n + 3], [n, n + 2], [n + 1, n + 3], [n + 1, s < 0 ? 5 : 6])
    n += 4
  }

  // Transverse ribs: hoops around the beam, spaced along the hull, each with a dorsal and a
  // ventral spur. A wall rather than a spine.
  for (let i = 1; i <= 8; i += 1) {
    const t = i / 9
    const z = 0.55 - t * 1.7
    const w = 0.62 + 0.24 * t
    const top = 0.2 + 0.02 * t
    const bot = -0.26 - 0.02 * t
    p.push(
      [-w, top, z], [w, top, z], [w, bot, z], [-w, bot, z],
      [0, top + 0.22, z], [0, bot - 0.26, z],
    )
    e.push(
      [n, n + 1], [n + 1, n + 2], [n + 2, n + 3], [n + 3, n],
      [n + 4, n], [n + 4, n + 1], [n + 5, n + 2], [n + 5, n + 3],
    )
    n += 6
  }

  // Flank batteries: blisters down each side, at the ribs. A wall that is only a wall has nothing
  // to shoot with, and at this size the eye goes looking for the guns.
  for (const s of [-1, 1]) {
    for (const z of [0.25, -0.25, -0.75]) {
      p.push([s * 0.9, 0.06, z + 0.12], [s * 1.02, 0.06, z], [s * 0.9, 0.06, z - 0.12], [s * 0.9, -0.1, z])
      e.push([n, n + 1], [n + 1, n + 2], [n, n + 3], [n + 2, n + 3], [n + 1, n + 3])
      n += 4
    }
  }

  // Stepped superstructure aft of the bridge, so the dorsal line is not one flat run.
  p.push([-0.2, 0.58, -0.5], [0.2, 0.58, -0.5], [0.2, 0.58, -0.9], [-0.2, 0.58, -0.9])
  e.push([n, n + 1], [n + 1, n + 2], [n + 2, n + 3], [n + 3, n], [n, 9], [n + 1, 9])
  n += 4

  // Engine bank: four wide nozzles across the stern, each a short box rather than a line.
  for (const x of [-0.6, -0.2, 0.2, 0.6]) {
    p.push([x - 0.1, 0.06, -1.15], [x + 0.1, 0.06, -1.15], [x + 0.1, -0.1, -1.15], [x - 0.1, -0.1, -1.15])
    p.push([x - 0.1, 0.06, -1.5], [x + 0.1, 0.06, -1.5], [x + 0.1, -0.1, -1.5], [x - 0.1, -0.1, -1.5])
    e.push(
      [n, n + 1], [n + 1, n + 2], [n + 2, n + 3], [n + 3, n],
      [n + 4, n + 5], [n + 5, n + 6], [n + 6, n + 7], [n + 7, n + 4],
      [n, n + 4], [n + 1, n + 5], [n + 2, n + 6], [n + 3, n + 7],
    )
    n += 8
  }
  // Launch rails along the dorsal surface, feeding the hangar. The mouth is the identity and it
  // was a mouth with nothing behind it.
  for (const s of [-1, 1]) {
    const rail = n
    p.push([s * 0.2, 0.34, 0.5], [s * 0.2, 0.34, -0.9], [s * 0.34, 0.28, 0.5], [s * 0.34, 0.28, -0.9])
    e.push([rail, rail + 1], [rail + 2, rail + 3], [rail, rail + 2], [rail + 1, rail + 3])
    n += 4
    for (const z of [0.2, -0.2, -0.6]) {
      p.push([s * 0.27, 0.42, z], [s * 0.2, 0.34, z], [s * 0.34, 0.28, z])
      e.push([n, n + 1], [n, n + 2], [n + 1, n + 2])
      n += 3
    }
  }
  // Ventral armour belt, stepped along the keel.
  for (const z of [0.2, -0.3, -0.8]) {
    p.push([-0.5, -0.4, z], [0.5, -0.4, z], [0.34, -0.56, z], [-0.34, -0.56, z])
    e.push([n, n + 1], [n + 1, n + 2], [n + 2, n + 3], [n + 3, n])
    n += 4
  }

  return wire(p, e)
}

/**
 * A sovereign: the largest hull anybody can own.
 *
 * A spinal ship. The whole thing is one axis with a command tower forward, three rib clusters
 * along the keel, flank galleries and an engine cage at the stern. At the size this is drawn the
 * eye is close enough to individual features that their absence reads as a lack of detail rather
 * than as distance — the argument `dreadnought` already makes, and it applies harder here,
 * because this is the hull the player looks at for a whole session.
 *
 * The rib clusters are grouped rather than evenly spread, which is what gives a hull this long a
 * bow, a waist and a stern instead of one undifferentiated run. The engine cage is deliberately
 * open: a solid block at this scale is a smudge, where a cage keeps interior lines and stays
 * legible both when it fills a third of the screen and when the ship is a speck on somebody
 * else's sensor board.
 */
export function sovereign(): Wire {
  const p: number[][] = [
    [0, 0, 2.0], // 0 prow
    [-0.4, 0.16, 1.05], [0.4, 0.16, 1.05], [-0.4, -0.2, 1.05], [0.4, -0.2, 1.05],
    [-0.7, 0.2, -1.35], [0.7, 0.2, -1.35], [-0.7, -0.26, -1.35], [0.7, -0.26, -1.35],
  ]
  const e: [number, number][] = [
    [0, 1], [0, 2], [0, 3], [0, 4],
    [1, 2], [3, 4], [1, 3], [2, 4],
    [1, 5], [2, 6], [3, 7], [4, 8],
    [5, 6], [7, 8], [5, 7], [6, 8],
  ]
  let n = p.length

  // A ram at the prow: three collars stepping back from the point. A spinal ship's bow is the part
  // most often seen first, and a bare vertex is the one place a wireframe looks unfinished.
  for (const [z, r] of [[1.85, 0.1], [1.6, 0.2], [1.3, 0.3]] as [number, number][]) {
    const first = ring(p, 8, r, z, 'xy')
    e.push(...loop(first, 8))
    e.push([first, 0], [first + 2, 0], [first + 4, 0], [first + 6, 0])
  }
  n = p.length

  // Command tower, forward and tall. On a hull too long to see both ends of at once, the bridge
  // is the thing a pilot orients by.
  p.push([0, 0.24, 0.9], [0, 0.86, 0.5], [-0.24, 0.7, 0.6], [0.24, 0.7, 0.6], [0, 0.3, 0.1])
  e.push([n, n + 1], [n + 1, n + 2], [n + 1, n + 3], [n + 2, n + 4], [n + 3, n + 4], [n + 1, n + 4])
  n += 5
  // Antenna masts off the tower, so the tallest point is a mast rather than a block.
  p.push([0, 1.25, 0.5], [-0.12, 1.05, 0.35], [0.12, 1.05, 0.35])
  e.push([n, n - 4], [n + 1, n - 4], [n + 2, n - 4])
  n += 3

  // Three rib clusters rather than one continuous run: a bow, a waist and a stern.
  for (const z0 of [0.85, -0.15, -1.0]) {
    for (let i = 0; i < 4; i += 1) {
      const z = z0 - i * 0.16
      const t = (2.0 - z) / 3.35
      const w = 0.36 + 0.36 * t
      const top = 0.15 + 0.06 * t
      const bot = -0.18 - 0.09 * t
      p.push([-w, top, z], [w, top, z], [w, bot, z], [-w, bot, z])
      e.push([n, n + 1], [n + 1, n + 2], [n + 2, n + 3], [n + 3, n])
      // Every second rib carries a dorsal fin, which breaks the top edge along the whole hull.
      if (i % 2 === 0) {
        p.push([0, top + 0.3, z])
        e.push([n + 4, n], [n + 4, n + 1])
        n += 1
      }
      n += 4
    }
  }

  // Flank galleries: long weapon runs down each side, with muzzle ports.
  for (const s of [-1, 1]) {
    const g = n
    p.push([s * 0.98, 0.02, 0.5], [s * 0.98, 0.02, -1.0], [s * 0.72, 0.14, 0.5], [s * 0.72, 0.14, -1.0])
    e.push([g, g + 1], [g + 2, g + 3], [g, g + 2], [g + 1, g + 3])
    n += 4
    for (const z of [0.35, 0.0, -0.35, -0.7]) {
      p.push([s * 1.1, 0.02, z], [s * 0.98, 0.02, z])
      e.push([n, n + 1])
      n += 2
    }
  }

  // Two engine cages, a large ring and a smaller one inside it, braced fore and aft.
  for (const [r, count] of [[0.52, 6], [0.28, 4]] as [number, number][]) {
    const front = ring(p, count, r, -1.35, 'xy')
    const back = ring(p, count, r, -1.95, 'xy')
    e.push(...loop(front, count), ...loop(back, count))
    for (let i = 0; i < count; i += 1) e.push([front + i, back + i])
  }
  // Hangar bays down each flank: three openings a fighter could leave through, which is what a
  // fleet's spine is *for* and the one thing the hull did not say about itself.
  for (const s of [-1, 1]) {
    for (const z of [0.5, -0.2, -0.9]) {
      const b = n
      p.push(
        [s * 0.74, 0.06, z + 0.18], [s * 0.74, 0.06, z - 0.18],
        [s * 0.74, -0.14, z - 0.18], [s * 0.74, -0.14, z + 0.18],
        [s * 0.56, 0.02, z + 0.12], [s * 0.56, 0.02, z - 0.12],
        [s * 0.56, -0.1, z - 0.12], [s * 0.56, -0.1, z + 0.12],
      )
      e.push(
        [b, b + 1], [b + 1, b + 2], [b + 2, b + 3], [b + 3, b],
        [b + 4, b + 5], [b + 5, b + 6], [b + 6, b + 7], [b + 7, b + 4],
        [b, b + 4], [b + 1, b + 5], [b + 2, b + 6], [b + 3, b + 7],
      )
      n += 8
    }
  }
  // Dorsal turret barbettes: four rings along the spine, so the top of the hull carries weapons
  // rather than only a line.
  for (const z of [0.9, 0.2, -0.5, -1.1]) {
    const t = ring(p, 6, 0.11, z, 'xy')
    for (let i = 0; i < 6; i += 1) p[t + i][1] += 0.3
    e.push(...loop(t, 6))
    n = p.length
  }

  return wire(p, e)
}

/**
 * ## The stations
 *
 * Nodes used to be shaded spheres, which told you a thing was there and nothing else: a market
 * and a rift were the same ball in different colours, so the whole vocabulary the record carries
 * arrived as a palette. They are wireframes now, for the same reason the ships are — a silhouette
 * reads at any distance and carries information a colour cannot — and each kind has its own.
 *
 * They are also **open structures you fly through** rather than obstacles (see `collide.ts`), so
 * the interiors are drawn: a frame you can pass inside ought to look like one.
 *
 * All are radius ~1 and axis-aligned; `gl.ts` scales by the node's own radius.
 */

/** A ring of `n` points on a circle in a named plane, appended to `p`. Returns the first index. */
function ring(
  p: number[][],
  n: number,
  radius: number,
  offset: number,
  plane: 'xy' | 'xz' | 'yz',
): number {
  const first = p.length
  for (let i = 0; i < n; i += 1) {
    const a = (i / n) * Math.PI * 2
    const c = Math.cos(a) * radius
    const d = Math.sin(a) * radius
    if (plane === 'xy') p.push([c, d, offset])
    else if (plane === 'xz') p.push([c, offset, d])
    else p.push([offset, c, d])
  }
  return first
}

/** Edges closing a ring of `n` points starting at `first`. */
function loop(first: number, n: number): [number, number][] {
  const e: [number, number][] = []
  for (let i = 0; i < n; i += 1) e.push([first + i, first + ((i + 1) % n)])
  return e
}

/** Spokes between two rings of equal length. */
function rungs(a: number, b: number, n: number, every = 1): [number, number][] {
  const e: [number, number][] = []
  for (let i = 0; i < n; i += every) e.push([a + i, b + i])
  return e
}

/**
 * A station: a habitation ring on a spindle.
 *
 * The most recognisable shape in the vocabulary, and deliberately the one with a clear *axis* — a
 * ring seen edge-on is a line, so a station reports its orientation from any angle, which is what
 * stops a field of them reading as identical blobs.
 */
export function station(): Wire {
  const p: number[][] = []
  const e: [number, number][] = []
  const outer = ring(p, 12, 1, 0, 'xy')
  const inner = ring(p, 12, 0.72, 0, 'xy')
  e.push(...loop(outer, 12), ...loop(inner, 12), ...rungs(outer, inner, 12))
  const spindle = p.length
  p.push([0, 0, -0.55], [0, 0, 0.55])
  e.push([spindle, spindle + 1])
  for (let i = 0; i < 12; i += 3) e.push([spindle, outer + i], [spindle + 1, outer + i])
  return wire(p, e)
}

/**
 * A market: a hexagonal trading platform with a raised core.
 *
 * Flat and wide where a station is a ring on an axis. The two are told apart in silhouette from
 * across the sector, which is the entire point of giving each kind its own.
 */
export function market(): Wire {
  const p: number[][] = []
  const e: [number, number][] = []
  const top = ring(p, 6, 1, 0.22, 'xz')
  const bot = ring(p, 6, 1, -0.22, 'xz')
  const core = ring(p, 6, 0.34, 0, 'xz')
  e.push(...loop(top, 6), ...loop(bot, 6), ...loop(core, 6), ...rungs(top, bot, 6))
  for (let i = 0; i < 6; i += 1) e.push([core + i, top + i], [core + i, bot + i])
  const mast = p.length
  p.push([0, 0.85, 0], [0, -0.85, 0])
  e.push([mast, core], [mast, core + 2], [mast, core + 4], [mast + 1, core + 1], [mast + 1, core + 3])
  return wire(p, e)
}

/**
 * A dock: an open cradle. Two walls, a gantry, and a gap you can fly into.
 *
 * The gap is the design. A dock is the one node the player has business *inside*, and a shape
 * that reads as open invites the approach the service key rewards.
 */
export function dock(): Wire {
  const p: number[][] = [
    [-1, 0.3, -0.7], [-1, 0.3, 0.7], [-1, -0.3, 0.7], [-1, -0.3, -0.7],
    [1, 0.3, -0.7], [1, 0.3, 0.7], [1, -0.3, 0.7], [1, -0.3, -0.7],
    [-0.35, 0.75, 0], [0.35, 0.75, 0],
    [-0.35, -0.75, 0], [0.35, -0.75, 0],
  ]
  const e: [number, number][] = [
    [0, 1], [1, 2], [2, 3], [3, 0],
    [4, 5], [5, 6], [6, 7], [7, 4],
    [0, 4], [3, 7],
    [8, 9], [8, 0], [9, 4], [8, 1], [9, 5],
    [10, 11], [10, 3], [11, 7], [10, 2], [11, 6],
  ]
  return wire(p, e)
}

/** A depot: three tanks in a brace. Squat, and obviously a container for something. */
export function depot(): Wire {
  const p: number[][] = []
  const e: [number, number][] = []
  for (const [ox, oy] of [[-0.45, 0.35], [0.45, 0.35], [0, -0.45]]) {
    const a = p.length
    for (const z of [-0.6, 0.6]) {
      for (let i = 0; i < 6; i += 1) {
        const t = (i / 6) * Math.PI * 2
        p.push([ox + Math.cos(t) * 0.34, oy + Math.sin(t) * 0.34, z])
      }
    }
    e.push(...loop(a, 6), ...loop(a + 6, 6), ...rungs(a, a + 6, 6, 2))
  }
  const brace = p.length
  p.push([-0.45, 0.35, 0], [0.45, 0.35, 0], [0, -0.45, 0])
  e.push([brace, brace + 1], [brace + 1, brace + 2], [brace + 2, brace])
  return wire(p, e)
}

/**
 * A derelict: a station ring, broken.
 *
 * Literally the station with segments missing and the spindle snapped. That it is *recognisably*
 * the same object is the point — a derelict is a station whose reading went stale, and the
 * silhouette should say so before the colour does.
 */
export function derelict(): Wire {
  const p: number[][] = []
  const e: [number, number][] = []
  const outer = ring(p, 12, 1, 0, 'xy')
  for (const [a, b] of loop(outer, 12)) {
    // Three gaps at fixed positions, so every derelict in a sector is broken the same way and the
    // shape stays a *kind* rather than an individual.
    const i = a - outer
    if (i === 2 || i === 6 || i === 9) continue
    e.push([a, b])
  }
  const stub = p.length
  p.push([0, 0, -0.4], [0.15, 0.1, 0.25])
  e.push([stub, stub + 1], [stub, outer], [stub, outer + 4])
  return wire(p, e)
}

/**
 * A rift: a jagged shell around nothing.
 *
 * Irregular where every other node is symmetric, and deliberately *empty* through the middle. A
 * rift is a blind spot — a region nobody could read — so the shape has to be a boundary with no
 * object inside it. A core here would be the em-dash bug in geometry: a claim about the contents
 * of a place the record explicitly could not see.
 */
export function rift(): Wire {
  const p: number[][] = []
  const e: [number, number][] = []
  const n = 10
  for (let i = 0; i < n; i += 1) {
    const t = (i / n) * Math.PI * 2
    // A fixed jitter table rather than a random one: two players holding the same record see the
    // same sector, cosmetics included.
    const r = 0.7 + ((i * 37) % 11) / 22
    p.push([Math.cos(t) * r, Math.sin(t) * r, ((i % 3) - 1) * 0.35])
  }
  for (let i = 0; i < n; i += 1) e.push([i, (i + 1) % n], [i, (i + 4) % n])
  return wire(p, e)
}

/**
 * A phantom: a station ring drawn as a dotted skeleton.
 *
 * Identifiable as a station *and* obviously incomplete. The observer modelled the thing rather
 * than seeing it, and a dotted outline is the honest rendering of a shape somebody inferred.
 */
export function phantom(): Wire {
  const p: number[][] = []
  const e: [number, number][] = []
  const outer = ring(p, 12, 1, 0, 'xy')
  for (let i = 0; i < 12; i += 3) e.push([outer + i, outer + ((i + 1) % 12)])
  const axis = p.length
  p.push([0, 0, -0.4], [0, 0, 0.4])
  e.push([axis, axis + 1])
  return wire(p, e)
}

/**
 * A marker: a survey cross, and nothing else.
 *
 * The observer looked here and found nothing. There is no structure to draw because no structure
 * was observed, so what is drawn is the *act of having looked* — a mark, not a building.
 */
export function marker(): Wire {
  const p: number[][] = [
    [-1, 0, 0], [1, 0, 0], [0, -1, 0], [0, 1, 0], [0, 0, -1], [0, 0, 1],
  ]
  return wire(p, [[0, 1], [2, 3], [4, 5]])
}

/** The origin: a market held inside a station ring. Where you start, and the only node that is both. */
export function origin(): Wire {
  const p: number[][] = []
  const e: [number, number][] = []
  const outer = ring(p, 16, 1, 0, 'xy')
  const mid = ring(p, 16, 0.8, 0, 'xy')
  e.push(...loop(outer, 16), ...loop(mid, 16), ...rungs(outer, mid, 16, 2))
  const inner = ring(p, 8, 0.42, 0, 'xz')
  e.push(...loop(inner, 8))
  for (let i = 0; i < 8; i += 2) e.push([inner + i, mid + i * 2])
  const axis = p.length
  p.push([0, 0, -0.9], [0, 0, 0.9])
  e.push([axis, axis + 1], [axis, inner], [axis + 1, inner + 4])
  return wire(p, e)
}

/**
 * A faction citadel: concentric rings on a common axis, one per tier.
 *
 * ## Why rings rather than a bigger station
 *
 * Every other node in the vocabulary is one silhouette that says *what* it is. A citadel has to
 * say what it is **and** how important it is, from a range where no label is legible — a tier-3
 * seat and a tier-1 outpost carry different contracts and a player routing across a sector needs
 * to pick between them on sight. Counting rings is the one readout that survives distance: it is
 * a *count*, and a count reads correctly at any size, where a diameter only reads correctly next
 * to something else to compare it against.
 *
 * That is the same reasoning as the coverage meter being one cell per term rather than a
 * proportional bar, and the severed limbs being one per blind spot rather than a rate.
 *
 * The rings are on **different axes**, alternating, so the structure reads as a volume from any
 * approach instead of collapsing to a set of parallel lines when seen edge-on — the same trap
 * `station()` avoids by giving its ring a spindle.
 *
 * Radii are fractions of 1 and the outermost is always 1, so a citadel of any tier occupies the
 * radius the physics and the renderer agree on (`nodeRadius`). A tier that grew the outer ring
 * would make the hit shell disagree with the picture, which is a rule this codebase has already
 * paid for twice.
 */
export function citadel(tier = 3): Wire {
  const p: number[][] = []
  const e: [number, number][] = []
  const rings = Math.max(1, Math.min(3, tier))

  for (let r = 0; r < rings; r += 1) {
    // Outermost first, stepping inward by a fixed fraction. `CITADEL_RING_GAP` lives in
    // `scale.ts` with every other distance, but this one is a *proportion of the mesh* rather
    // than a world length, so it is expressed here as the unit-sphere fraction it is.
    const radius = 1 - r * 0.26
    const plane = r % 2 === 0 ? 'xy' : 'xz'
    const seg = 16
    const outer = ring(p, seg, radius, 0, plane as 'xy' | 'xz')
    const inner = ring(p, seg, radius * 0.88, 0, plane as 'xy' | 'xz')
    e.push(...loop(outer, seg), ...loop(inner, seg), ...rungs(outer, inner, seg, 2))
  }

  // A core, so the middle is not empty at close range, and an axis the rings hang from.
  const core = p.length
  p.push([0, 0, -0.34], [0, 0, 0.34], [-0.2, 0, 0], [0.2, 0, 0], [0, -0.2, 0], [0, 0.2, 0])
  e.push([core, core + 1], [core + 2, core + 3], [core + 4, core + 5])
  e.push([core, core + 2], [core, core + 4], [core + 1, core + 3], [core + 1, core + 5])
  return wire(p, e)
}

/**
 * A cylinder along +Z, from the origin to `-length`, radius 1.
 *
 * The bolt. A projectile used to be a sphere, and a sphere moving at half a sector per second is
 * a dot that teleports between frames — the player sees a flicker and cannot tell what fired or
 * from where. A cylinder along the direction of travel gives the eye a streak to follow, and the
 * streak points back at its origin, which is the most useful thing on screen in a fight.
 *
 * Triangles, not lines: a bolt is the one thing here that should look solid and hot.
 */
export function bolt(sides = 8, length = 1): Float32Array {
  const out: number[] = []
  for (let i = 0; i < sides; i += 1) {
    const a0 = (i / sides) * Math.PI * 2
    const a1 = ((i + 1) / sides) * Math.PI * 2
    const c0 = [Math.cos(a0), Math.sin(a0)]
    const c1 = [Math.cos(a1), Math.sin(a1)]
    // Side quad, as two triangles.
    out.push(c0[0], c0[1], 0, c1[0], c1[1], 0, c1[0], c1[1], -length)
    out.push(c0[0], c0[1], 0, c1[0], c1[1], -length, c0[0], c0[1], -length)
    // A cap at the leading end so a head-on bolt is not an empty tube.
    out.push(0, 0, -length, c0[0], c0[1], -length, c1[0], c1[1], -length)
  }
  return new Float32Array(out)
}

/** A unit icosphere, subdivided once. Cheap, and round enough at the sizes drawn here. */
export function icosphere(): Float32Array {
  const t = (1 + Math.sqrt(5)) / 2
  const base: number[][] = [
    [-1, t, 0], [1, t, 0], [-1, -t, 0], [1, -t, 0],
    [0, -1, t], [0, 1, t], [0, -1, -t], [0, 1, -t],
    [t, 0, -1], [t, 0, 1], [-t, 0, -1], [-t, 0, 1],
  ].map(([x, y, z]) => {
    const l = Math.hypot(x, y, z)
    return [x / l, y / l, z / l]
  })
  const faces = [
    [0, 11, 5], [0, 5, 1], [0, 1, 7], [0, 7, 10], [0, 10, 11],
    [1, 5, 9], [5, 11, 4], [11, 10, 2], [10, 7, 6], [7, 1, 8],
    [3, 9, 4], [3, 4, 2], [3, 2, 6], [3, 6, 8], [3, 8, 9],
    [4, 9, 5], [2, 4, 11], [6, 2, 10], [8, 6, 7], [9, 8, 1],
  ]
  const out: number[] = []
  const mid = (a: number[], b: number[]) => {
    const m = [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
    const l = Math.hypot(m[0], m[1], m[2])
    return [m[0] / l, m[1] / l, m[2] / l]
  }
  for (const [i, j, k] of faces) {
    const a = base[i]
    const b = base[j]
    const c = base[k]
    const ab = mid(a, b)
    const bc = mid(b, c)
    const ca = mid(c, a)
    for (const tri of [[a, ab, ca], [ab, b, bc], [ca, bc, c], [ab, bc, ca]]) {
      for (const v of tri) out.push(v[0], v[1], v[2])
    }
  }
  return new Float32Array(out)
}

/**
 * A fixed field of background stars, on a unit sphere.
 *
 * ## Why they are drawn at all
 *
 * Without them the void is literally a black rectangle, and a black rectangle has no sense of
 * rotation: pitch and yaw produce no visible change until something enters frame, so the ship
 * feels like it is sitting still while numbers change. Stars are what turn a rotation into a
 * *motion*. They are the cheapest thing in this whole file and they do more for the feeling of
 * being somewhere than anything else in it.
 *
 * ## Why they are seeded, and why they never move
 *
 * Seeded from the world commitment, so two players holding the same record see the same sky —
 * the determinism rule, applied to something that has no gameplay effect precisely because
 * making an exception for cosmetics is how the rule stops being one.
 *
 * They are drawn at a fixed distance with the camera's translation removed, so they never
 * parallax. Parallaxing them would make them *objects*, and a star you could fly to is a claim
 * about the sector the record never made.
 */
export function starfield(seed: string, count = 1400): Float32Array {
  // A small xorshift, local so this file depends on nothing.
  let s = 0
  for (const ch of Array.from(seed).slice(0, 8)) {
    const d = parseInt(ch, 16)
    s = (Math.imul(s, 16) + (Number.isNaN(d) ? 0 : d)) >>> 0
  }
  if (s === 0) s = 0x9e3779b9
  const next = () => {
    s = (s ^ (s << 13)) >>> 0
    s = (s ^ (s >>> 17)) >>> 0
    s = (s ^ (s << 5)) >>> 0
    return s
  }

  const out = new Float32Array(count * 4)
  for (let i = 0; i < count; i += 1) {
    // Uniform on the sphere: a naive two-angle pick clusters hard at the poles, and a night sky
    // with two bright patches in it looks like a bug rather than a sky.
    const u = (next() % 100000) / 100000
    const v = (next() % 100000) / 100000
    const theta = u * Math.PI * 2
    const z = v * 2 - 1
    const r = Math.sqrt(Math.max(0, 1 - z * z))
    out[i * 4] = Math.cos(theta) * r
    out[i * 4 + 1] = z
    out[i * 4 + 2] = Math.sin(theta) * r
    // Brightness. Heavily weighted toward the faint end — a sky of equally bright points reads
    // as noise, and the few bright ones are what the eye actually navigates by.
    const b = (next() % 1000) / 1000
    out[i * 4 + 3] = 0.18 + b * b * b * 0.82
  }
  return out
}

/**
 * ## Every player hull has its own silhouette
 *
 * Seventeen hulls previously shared seven shapes, so a scout and a skiff were the same dart and
 * three capitals were the same spinal ship at three sizes. Scale is not a silhouette — the note on
 * `cruiser` already says so about borrowing an *enemy* shape, and the argument is exactly as strong
 * between two hulls a player can own. In third person you look at yours for a whole session, and
 * the shipyard is a choice between *ships*; if two of them differ only by a number, the choice is a
 * spreadsheet.
 *
 * Each of the twelve below carries one feature nothing else has, and the feature is chosen to read
 * at the distance the hull is usually seen from. A fighter is seen close and can afford fine
 * detail; a capital is seen filling the frame and needs *structure*.
 */

/**
 * The skiff: the hull you arrived in. Deliberately the plainest thing in the game.
 *
 * A blunt wedge with one fin and two stubby wings. It has no feature, and that is its feature —
 * the starter should look like something you will replace, and every silhouette above it is more
 * interesting on purpose.
 */
export function skiff(): Wire {
  const p: number[][] = [
    [0, 0, 1.0],
    [-0.55, -0.08, -0.6], [0.55, -0.08, -0.6],
    [0, 0.1, -0.5],
    [0, 0.5, -0.7],
    [-0.18, -0.02, -0.75], [0.18, -0.02, -0.75],
  ]
  const e: [number, number][] = [
    [0, 1], [0, 2], [0, 3],
    [1, 3], [2, 3], [1, 2],
    [3, 4], [4, 5], [4, 6], [5, 6],
    [1, 5], [2, 6],
  ]
  let n = p.length
  // Plain is not the same as sparse. The starter should look like something you will replace, and
  // it was reading as something that had not been finished — twelve segments against a light
  // tier that runs to thirty-four. What it gets is *ordinary* detail: a cockpit, a pair of
  // exhausts and a keel, none of which is a feature anything else lacks.
  p.push([0, 0.16, 0.35], [-0.1, 0.06, 0.5], [0.1, 0.06, 0.5], [0, 0.02, 0.15])
  e.push([n, n + 1], [n, n + 2], [n + 1, n + 3], [n + 2, n + 3], [n, 3], [n + 3, 0])
  n += 4
  for (const s of [-1, 1]) {
    p.push([s * 0.16, -0.02, -0.75], [s * 0.16, -0.02, -0.95], [s * 0.24, 0.04, -0.85])
    e.push([n, n + 1], [n, n + 2], [n + 1, n + 2])
    n += 3
  }
  p.push([0, -0.22, -0.2], [0, -0.22, -0.7])
  e.push([n, n + 1], [n, 1], [n, 2], [n + 1, 5], [n + 1, 6])
  // Panel lines. Plain is the design; sparse was the defect. Three frames and a pair of strakes
  // give the starter the same *kind* of surface every other hull has, without giving it a feature
  // any of them lack — which is the whole point of the ship you are meant to replace.
  for (let i = 1; i <= 3; i += 1) {
    const t = i / 4
    const z = 0.7 - t * 1.2
    const w = 0.2 + 0.28 * t
    p.push([-w, 0.06, z], [w, 0.06, z], [w * 0.8, -0.06, z], [-w * 0.8, -0.06, z])
    e.push([n, n + 1], [n + 1, n + 2], [n + 2, n + 3], [n + 3, n])
    n += 4
  }
  for (const s of [-1, 1]) {
    p.push([s * 0.16, 0.04, 0.75], [s * 0.42, 0, -0.55])
    e.push([n, n + 1])
    n += 2
    // Wingtip lights, which is the cheapest thing that makes a small hull read as *maintained*.
    p.push([s * 0.55, -0.08, -0.6], [s * 0.62, -0.04, -0.5])
    e.push([n, n + 1])
    n += 2
  }

  return wire(p, e)
}

/**
 * The dart: the scout. Long, thin, and almost nothing.
 *
 * Twice the length of anything else its size and a fraction of the beam, with forward canards
 * rather than swept wings — the one shape here that reads as *fast* standing still. Fragility is a
 * silhouette decision as much as a statline: there is visibly no room in it for armour.
 */
export function dart(): Wire {
  const p: number[][] = [
    [0, 0, 1.9],
    [-0.1, 0.05, 0.6], [0.1, 0.05, 0.6], [0, -0.08, 0.6],
    [-0.12, 0.05, -1.0], [0.12, 0.05, -1.0], [0, -0.1, -1.0],
    [-0.62, 0.02, 0.75], [0.62, 0.02, 0.75],
    [0, -0.02, -1.35],
  ]
  const e: [number, number][] = [
    [0, 1], [0, 2], [0, 3],
    [1, 2], [1, 3], [2, 3],
    [1, 4], [2, 5], [3, 6],
    [4, 5], [4, 6], [5, 6],
    [7, 1], [7, 3], [8, 2], [8, 3],
    [9, 4], [9, 5], [9, 6],
  ]
  let n = p.length
  // Bands down the needle. A shape this thin has almost no outline to read, so the length has to
  // be legible some other way — the bands are the only cue for how long it is, and therefore for
  // how fast it is going when it crosses your view.
  for (const t of [0.2, 0.45, 0.7]) {
    const z = 0.6 - t * 1.6
    const first = ring(p, 5, 0.11, z, 'xy')
    e.push(...loop(first, 5))
    n = p.length
  }
  // Canard tips swept back to the body, so the canards read as surfaces rather than as spikes.
  e.push([7, 4], [8, 5])
  p.push([-0.5, 0.02, 0.35], [0.5, 0.02, 0.35])
  e.push([n, 7], [n, 1], [n + 1, 8], [n + 1, 2])
  n += 2
  // A single trailing nacelle with a nozzle, because the whole hull is one engine and a bare
  // vertex at the stern is the one place a needle looks unfinished.
  const bell = ring(p, 5, 0.09, -1.2, 'xy')
  const flare = ring(p, 5, 0.13, -1.45, 'xy')
  e.push(...loop(bell, 5), ...loop(flare, 5))
  for (let i = 0; i < 5; i += 1) e.push([bell + i, flare + i])
  return wire(p, e)
}

/**
 * The lance: the light gunboat.
 *
 * A spike down the axis, further forward than any nose here, with the hull hung behind it and twin
 * gun pods slung underneath. It reads as a weapon that grew a ship rather than a ship carrying
 * weapons, which is the statline said in geometry.
 */
export function lance(): Wire {
  const p: number[][] = [
    [0, 0, 1.75],
    [0, 0, 0.55],
    [-0.36, 0.2, 0.35], [0.36, 0.2, 0.35], [-0.36, -0.2, 0.35], [0.36, -0.2, 0.35],
    [-0.4, 0.18, -0.85], [0.4, 0.18, -0.85], [-0.4, -0.18, -0.85], [0.4, -0.18, -0.85],
    [-0.5, -0.42, 0.1], [-0.5, -0.42, -0.7],
    [0.5, -0.42, 0.1], [0.5, -0.42, -0.7],
  ]
  const e: [number, number][] = [
    [0, 1],
    [1, 2], [1, 3], [1, 4], [1, 5],
    [2, 3], [4, 5], [2, 4], [3, 5],
    [2, 6], [3, 7], [4, 8], [5, 9],
    [6, 7], [8, 9], [6, 8], [7, 9],
    [10, 11], [12, 13], [10, 4], [11, 8], [12, 5], [13, 9],
  ]
  let n = p.length
  // Collars where the spike enters the hull. A spike is the whole identity of this ship and it was
  // a bare line — the join is exactly where a naive model looks glued on rather than mounted.
  for (const [z, r] of [[1.4, 0.07], [1.1, 0.11], [0.8, 0.15]] as [number, number][]) {
    const first = ring(p, 6, r, z, 'xy')
    e.push(...loop(first, 6))
    e.push([first, 0], [first + 3, 0])
    n = p.length
  }
  // Muzzles on the gun pods, so the pods read as guns and not as fuel.
  for (const s of [-1, 1]) {
    p.push([s * 0.5, -0.42, 0.34], [s * 0.42, -0.36, 0.16], [s * 0.58, -0.36, 0.16])
    e.push([n, n + 1], [n, n + 2], [n + 1, n + 2], [n, s < 0 ? 10 : 12])
    n += 3
  }
  // Two exhausts, offset, so the stern is not a flat quad.
  for (const s of [-1, 1]) {
    p.push([s * 0.2, 0, -0.85], [s * 0.2, 0, -1.1], [s * 0.3, 0.08, -0.98], [s * 0.1, 0.08, -0.98])
    e.push([n, n + 1], [n + 1, n + 2], [n + 1, n + 3], [n + 2, n + 3], [n, n + 2], [n, n + 3])
    n += 4
  }
  return wire(p, e)
}

/**
 * The prowler: the medium that still explores.
 *
 * A long ventral sensor boom, hanging below the hull and reaching further forward than the nose.
 * Nothing else in the game has anything below its centreline, so this reads from any angle and
 * from underneath — which is where a ship you are chasing is seen from.
 */
export function prowler(): Wire {
  const p: number[][] = [
    [0, 0.05, 1.35],
    [-0.28, 0.18, 0.35], [0.28, 0.18, 0.35], [-0.28, -0.1, 0.35], [0.28, -0.1, 0.35],
    [-0.34, 0.16, -0.95], [0.34, 0.16, -0.95], [-0.34, -0.14, -0.95], [0.34, -0.14, -0.95],
    [0, 0.44, -0.2],
    [0, -0.62, 1.55], [0, -0.5, 0.2], [0, -0.46, -0.7],
    [-0.7, 0.02, -0.5], [0.7, 0.02, -0.5],
  ]
  const e: [number, number][] = [
    [0, 1], [0, 2], [0, 3], [0, 4],
    [1, 2], [3, 4], [1, 3], [2, 4],
    [1, 5], [2, 6], [3, 7], [4, 8],
    [5, 6], [7, 8], [5, 7], [6, 8],
    [9, 1], [9, 2], [9, 5], [9, 6],
    [10, 11], [11, 12], [11, 3], [11, 4], [12, 7], [12, 8], [10, 0],
    [13, 5], [13, 7], [14, 6], [14, 8],
  ]
  let n = p.length

  // The boom is the identity, so it gets the detail: a dish at the tip and three collars along it.
  // A bare line hanging under a hull reads as a mistake; a line with instruments on it reads as
  // the reason the ship is shaped that way.
  const dish = ring(p, 8, 0.22, 1.5, 'xy')
  for (let i = 0; i < 8; i += 1) { p[dish + i][1] -= 0.6; p[dish + i][2] += 0.05 }
  e.push(...loop(dish, 8))
  for (let i = 0; i < 8; i += 2) e.push([dish + i, 10])
  n = p.length
  for (const [z, r] of [[1.0, 0.09], [0.5, 0.1], [-0.2, 0.11]] as [number, number][]) {
    const first = ring(p, 5, r, z, 'xy')
    for (let i = 0; i < 5; i += 1) p[first + i][1] -= 0.5
    e.push(...loop(first, 5))
    n = p.length
  }

  // Swept dorsal fins either side of the spine, angled back — the ship reads as *listening* from
  // above and *fast* from the side, which is the medium the tier opens with.
  for (const s of [-1, 1]) {
    p.push([s * 0.12, 0.42, -0.1], [s * 0.34, 0.66, -0.5], [s * 0.34, 0.66, -0.85], [s * 0.12, 0.36, -0.7])
    e.push([n, n + 1], [n + 1, n + 2], [n + 2, n + 3], [n + 3, n])
    n += 4
  }

  // Hull ribs, and a pair of exhausts.
  for (let i = 1; i <= 4; i += 1) {
    const t = i / 5
    const z = 0.35 - t * 1.3
    const w = 0.28 + 0.06 * t
    p.push([-w, 0.17, z], [w, 0.17, z], [w, -0.12, z], [-w, -0.12, z])
    e.push([n, n + 1], [n + 1, n + 2], [n + 2, n + 3], [n + 3, n])
    n += 4
  }
  for (const s of [-1, 1]) {
    const first = ring(p, 5, 0.12, -0.95, 'xy')
    for (let i = 0; i < 5; i += 1) { p[first + i][0] += s * 0.18; p[first + i][1] += 0.02 }
    const back = ring(p, 5, 0.14, -1.25, 'xy')
    for (let i = 0; i < 5; i += 1) { p[back + i][0] += s * 0.18; p[back + i][1] += 0.02 }
    e.push(...loop(first, 5), ...loop(back, 5))
    for (let i = 0; i < 5; i += 1) e.push([first + i, back + i])
    n = p.length
  }
  return wire(p, e)
}

/**
 * The halberd: the medium gun platform.
 *
 * Two barrels, longer than the hull between them, and almost no ship. A gun platform that happens
 * to fly is a phrase in the shipyard and a shape here, which is the point of giving it one.
 */
export function halberd(): Wire {
  const p: number[][] = [
    [0, 0, 0.5],
    [-0.24, 0.22, 0.1], [0.24, 0.22, 0.1], [-0.24, -0.22, 0.1], [0.24, -0.22, 0.1],
    [-0.3, 0.2, -0.95], [0.3, 0.2, -0.95], [-0.3, -0.2, -0.95], [0.3, -0.2, -0.95],
  ]
  const e: [number, number][] = [
    [0, 1], [0, 2], [0, 3], [0, 4],
    [1, 2], [3, 4], [1, 3], [2, 4],
    [1, 5], [2, 6], [3, 7], [4, 8],
    [5, 6], [7, 8], [5, 7], [6, 8],
  ]
  let n = p.length
  // The barrels, now round sections with bands rather than square tubes — a gun platform's guns
  // should be the best-drawn thing on it.
  for (const s of [-1, 1]) {
    for (const z of [1.7, 1.15, 0.6, 0.05, -0.5]) {
      const first = ring(p, 7, 0.15, z, 'xy')
      for (let i = 0; i < 7; i += 1) p[first + i][0] += s * 0.52
      e.push(...loop(first, 7))
      n = p.length
    }
    // Four rails running the length of each barrel, so it reads as a tube and not as a stack.
    for (const [dx, dy] of [[0.15, 0], [-0.15, 0], [0, 0.15], [0, -0.15]] as [number, number][]) {
      p.push([s * 0.52 + dx, dy, 1.7], [s * 0.52 + dx, dy, -0.5])
      e.push([n, n + 1])
      n += 2
    }
    // A muzzle flare and a breech block, which is where a barrel meets a ship.
    const flare = ring(p, 7, 0.24, 1.7, 'xy')
    for (let i = 0; i < 7; i += 1) p[flare + i][0] += s * 0.52
    e.push(...loop(flare, 7))
    for (let i = 0; i < 7; i += 1) e.push([flare + i, flare - 7 + i])
    n = p.length
    p.push([s * 0.34, 0.2, -0.35], [s * 0.7, 0.2, -0.35], [s * 0.7, -0.2, -0.35], [s * 0.34, -0.2, -0.35])
    p.push([s * 0.34, 0.2, -0.85], [s * 0.7, 0.2, -0.85], [s * 0.7, -0.2, -0.85], [s * 0.34, -0.2, -0.85])
    e.push(
      [n, n + 1], [n + 1, n + 2], [n + 2, n + 3], [n + 3, n],
      [n + 4, n + 5], [n + 5, n + 6], [n + 6, n + 7], [n + 7, n + 4],
      [n, n + 4], [n + 1, n + 5], [n + 2, n + 6], [n + 3, n + 7],
      [n, 1], [n + 3, 3],
    )
    n += 8
  }
  // A small bridge slung under the hull between the guns, and a stern block.
  p.push([-0.16, -0.3, -0.2], [0.16, -0.3, -0.2], [0.16, -0.3, -0.6], [-0.16, -0.3, -0.6], [0, -0.16, -0.4])
  e.push([n, n + 1], [n + 1, n + 2], [n + 2, n + 3], [n + 3, n], [n + 4, n], [n + 4, n + 1], [n + 4, n + 2], [n + 4, n + 3])
  n += 5
  for (const x of [-0.14, 0.14]) {
    const first = ring(p, 5, 0.12, -0.95, 'xy')
    for (let i = 0; i < 5; i += 1) p[first + i][0] += x
    const back = ring(p, 5, 0.14, -1.25, 'xy')
    for (let i = 0; i < 5; i += 1) p[back + i][0] += x
    e.push(...loop(first, 5), ...loop(back, 5))
    for (let i = 0; i < 5; i += 1) e.push([first + i, back + i])
    n = p.length
  }
  return wire(p, e)
}

/**
 * The rampart: the medium brawler.
 *
 * A flat ram plate across the whole bow — the only forward-facing *surface* in the game, where
 * every other hull comes to a point. Armour first, said as a shape: it is built to be hit, and it
 * has an obvious face to be hit on.
 */
export function rampart(): Wire {
  const p: number[][] = [
    [-0.72, 0.4, 0.8], [0.72, 0.4, 0.8], [0.72, -0.4, 0.8], [-0.72, -0.4, 0.8],
    [-0.5, 0.3, 0.2], [0.5, 0.3, 0.2], [0.5, -0.3, 0.2], [-0.5, -0.3, 0.2],
    [-0.56, 0.28, -1.0], [0.56, 0.28, -1.0], [0.56, -0.28, -1.0], [-0.56, -0.28, -1.0],
  ]
  const e: [number, number][] = [
    [0, 1], [1, 2], [2, 3], [3, 0],
    [0, 4], [1, 5], [2, 6], [3, 7],
    [4, 5], [5, 6], [6, 7], [7, 4],
    [4, 8], [5, 9], [6, 10], [7, 11],
    [8, 9], [9, 10], [10, 11], [11, 8],
    [0, 2], [1, 3],
  ]
  let n = p.length

  // ## The plate is the ship, so the plate carries the detail
  //
  // A ram plate braced by two diagonals is a *panel*; what it has to read as is armour. Three
  // horizontal ribs and a vertical keel across the face give it thickness at any angle, and the
  // corner bosses give it something to catch light on — the whole hull is a claim about being hit,
  // and a flat quad does not make it.
  for (const t of [0.3, 0.5, 0.7]) {
    const y = 0.4 - t * 0.8
    p.push([-0.72, y, 0.8], [0.72, y, 0.8])
    e.push([n, n + 1])
    n += 2
  }
  p.push([0, 0.4, 0.8], [0, -0.4, 0.8])
  e.push([n, n + 1])
  n += 2
  for (const [sx, sy] of [[-1, 1], [1, 1], [1, -1], [-1, -1]] as [number, number][]) {
    p.push([sx * 0.72, sy * 0.4, 0.8], [sx * 0.86, sy * 0.48, 0.62], [sx * 0.86, sy * 0.48, 0.2])
    e.push([n, n + 1], [n + 1, n + 2], [n + 2, sx < 0 ? (sy > 0 ? 4 : 7) : (sy > 0 ? 5 : 6)])
    n += 3
  }

  // Shoulder blocks either side, now boxed rather than four lines.
  for (const s of [-1, 1]) {
    const b = n
    p.push(
      [s * 0.86, 0.24, 0.6], [s * 0.86, -0.24, 0.6], [s * 0.86, 0.24, -0.6], [s * 0.86, -0.24, -0.6],
      [s * 0.6, 0.28, 0.6], [s * 0.6, -0.28, 0.6], [s * 0.6, 0.28, -0.6], [s * 0.6, -0.28, -0.6],
    )
    e.push(
      [b, b + 1], [b + 2, b + 3], [b, b + 2], [b + 1, b + 3],
      [b + 4, b], [b + 5, b + 1], [b + 6, b + 2], [b + 7, b + 3],
      [b + 4, b + 6], [b + 5, b + 7],
    )
    n += 8
  }

  // Dorsal spine and a stern engine block: a slab needs a top edge that is not a straight line.
  p.push([0, 0.42, 0.1], [0, 0.42, -0.5], [-0.18, 0.3, -0.2], [0.18, 0.3, -0.2])
  e.push([n, n + 1], [n, n + 2], [n, n + 3], [n + 1, n + 2], [n + 1, n + 3])
  n += 4
  for (const x of [-0.3, 0.0, 0.3]) {
    const first = ring(p, 5, 0.13, -1.0, 'xy')
    for (let i = 0; i < 5; i += 1) p[first + i][0] += x
    const back = ring(p, 5, 0.15, -1.28, 'xy')
    for (let i = 0; i < 5; i += 1) p[back + i][0] += x
    e.push(...loop(first, 5), ...loop(back, 5))
    for (let i = 0; i < 5; i += 1) e.push([first + i, back + i])
    n = p.length
  }
  return wire(p, e)
}

/**
 * The aegis: shields that come back.
 *
 * Three vanes at a hundred and twenty degrees around a small core, and a hull with **rotational**
 * symmetry rather than the bilateral symmetry everything else here has. It is the one silhouette a
 * player cannot read a roll from, which is honest: this is the hull that does not care which way
 * up it is because it is not trying to out-turn anything.
 */
export function aegis(): Wire {
  const p: number[][] = [
    [0, 0, 1.05],
    [0, 0, -0.9],
  ]
  const e: [number, number][] = []
  let n = p.length
  const tips: number[] = []
  for (let i = 0; i < 3; i += 1) {
    const a = (i / 3) * Math.PI * 2
    const cx = Math.cos(a)
    const cy = Math.sin(a)
    // Root, mid, tip: a vane that sweeps outward and back.
    p.push([cx * 0.22, cy * 0.22, 0.45])
    p.push([cx * 0.78, cy * 0.78, -0.1])
    p.push([cx * 0.62, cy * 0.62, -0.85])
    e.push([0, n], [n, n + 1], [n + 1, n + 2], [n + 2, 1], [n, 1])
    // Each vane is a *panel*, not a spar: an inner edge running parallel to the outer one, so the
    // ship reads as three surfaces rather than three wires. The shields are the whole hull, and a
    // wire frame cannot carry that claim.
    p.push([cx * 0.4, cy * 0.4, 0.15], [cx * 0.52, cy * 0.52, -0.5])
    e.push([n + 3, n], [n + 3, n + 1], [n + 4, n + 1], [n + 4, n + 2], [n + 3, n + 4])
    tips.push(n + 1)
    n += 5
  }
  // Tie the vane tips together, so the ship reads as a frame rather than three separate fins.
  e.push([tips[0], tips[1]], [tips[1], tips[2]], [tips[2], tips[0]])
  // A core between the vanes: two rings around the axis, which is what the vanes are protecting
  // and the only part of this hull that can be destroyed.
  for (const z of [0.3, -0.4]) {
    const first = ring(p, 6, 0.18, z, 'xy')
    e.push(...loop(first, 6))
    e.push([first, 0], [first + 3, 1])
    n = p.length
  }
  // Emitter heads at each vane tip, and a brace ring binding the three of them. The hull's whole
  // claim is that the shields are the ship, and three bare spars do not make it — an emitter is
  // the part a shield would come *from*, and the ring is what stops the vanes reading as separate.
  for (let i = 0; i < 3; i += 1) {
    const a = (i / 3) * Math.PI * 2
    const cx = Math.cos(a)
    const cy = Math.sin(a)
    const head = n
    p.push(
      [cx * 0.72, cy * 0.72, 0.05], [cx * 0.9, cy * 0.9, -0.15],
      [cx * 0.72, cy * 0.72, -0.35], [cx * 0.62, cy * 0.62, -0.15],
    )
    e.push([head, head + 1], [head + 1, head + 2], [head + 2, head + 3], [head + 3, head],
           [head, head + 2], [head + 1, head + 3])
    n += 4
  }
  const brace = ring(p, 9, 0.66, -0.2, 'xy')
  e.push(...loop(brace, 9))
  n = p.length
  // A nose ring and a stern nozzle, so the axis has ends rather than points.
  const nose = ring(p, 6, 0.13, 0.75, 'xy')
  e.push(...loop(nose, 6))
  for (let i = 0; i < 6; i += 1) e.push([nose + i, 0])
  const bell = ring(p, 6, 0.16, -0.9, 'xy')
  const flare = ring(p, 6, 0.2, -1.15, 'xy')
  e.push(...loop(bell, 6), ...loop(flare, 6))
  for (let i = 0; i < 6; i += 1) e.push([bell + i, flare + i], [bell + i, 1])
  n = p.length

  return wire(p, e)
}

/**
 * The carrack: the deep hull.
 *
 * A bare spine with three cargo rings threaded onto it. Nothing else here is *open* — every other
 * hull is a solid-looking frame — so a ship you can see through reads as a carrier at any range,
 * and the ring count is a cue for how much of it is hold rather than ship.
 */
export function carrack(): Wire {
  const p: number[][] = [
    [0, 0, 1.5],
    [0, 0, -1.25],
    [-0.16, 0.16, 1.1], [0.16, 0.16, 1.1], [0.16, -0.16, 1.1], [-0.16, -0.16, 1.1],
  ]
  const e: [number, number][] = [
    [0, 2], [0, 3], [0, 4], [0, 5],
    [2, 3], [3, 4], [4, 5], [5, 2],
    [2, 1], [3, 1], [4, 1], [5, 1],
  ]
  let n = p.length
  // Four rails down the spine, so the ship you can see through still has a *ship* in the middle.
  for (const [dx, dy] of [[0.09, 0.09], [-0.09, 0.09], [0.09, -0.09], [-0.09, -0.09]] as [number, number][]) {
    p.push([dx, dy, 1.1], [dx, dy, -1.2])
    e.push([n, n + 1])
    n += 2
  }
  // Cargo rings, now paired — each hold is two hoops with longitudinals between them, which is
  // what makes it read as a *volume* being carried rather than as a hoop threaded on a stick.
  for (const z of [0.7, -0.05, -0.8]) {
    const front = ring(p, 10, 0.72, z + 0.16, 'xy')
    const back = ring(p, 10, 0.72, z - 0.16, 'xy')
    e.push(...loop(front, 10), ...loop(back, 10))
    for (let i = 0; i < 10; i += 1) e.push([front + i, back + i])
    for (const i of [0, 2, 5, 7]) e.push([front + i, 0], [back + i, 1])
    n = p.length
  }
  // A bridge forward of the first hold: the only part of the hull with a crew in it.
  p.push([-0.16, 0.3, 1.25], [0.16, 0.3, 1.25], [0.16, 0.3, 0.95], [-0.16, 0.3, 0.95], [0, 0.16, 1.1])
  e.push([n, n + 1], [n + 1, n + 2], [n + 2, n + 3], [n + 3, n], [n + 4, n], [n + 4, n + 1], [n + 4, n + 2], [n + 4, n + 3])
  n += 5
  // Engine bell, with a flare.
  const bell = ring(p, 8, 0.24, -1.25, 'xy')
  const flare = ring(p, 8, 0.34, -1.65, 'xy')
  e.push(...loop(bell, 8), ...loop(flare, 8))
  for (let i = 0; i < 8; i += 1) e.push([bell + i, flare + i], [bell + i, 1])
  return wire(p, e)
}

/**
 * The monitor: the siege capital.
 *
 * One gun, on the axis, longer than the ship behind it. Aim it before you commit is the line in the
 * shipyard, and this is that line as a shape — there is visibly no way to bring it to bear except
 * by turning the whole hull.
 */
export function monitor(): Wire {
  const p: number[][] = [
    [-0.6, 0.3, -0.2], [0.6, 0.3, -0.2], [0.6, -0.3, -0.2], [-0.6, -0.3, -0.2],
    [-0.72, 0.34, -1.4], [0.72, 0.34, -1.4], [0.72, -0.34, -1.4], [-0.72, -0.34, -1.4],
  ]
  const e: [number, number][] = [
    [0, 1], [1, 2], [2, 3], [3, 0],
    [4, 5], [5, 6], [6, 7], [7, 4],
    [0, 4], [1, 5], [2, 6], [3, 7],
  ]
  let n = p.length

  // ## The recoil housing
  //
  // Three concentric collars where the barrel enters the hull, stepping outward as they go aft.
  // A gun this size has to visibly *absorb* something, and the collars are what tell a viewer the
  // barrel is mounted rather than glued on — the join is where a naive model looks wrong.
  for (const [z, r] of [[0.1, 0.44], [-0.25, 0.56], [-0.6, 0.66]] as [number, number][]) {
    const first = ring(p, 10, r, z, 'xy')
    e.push(...loop(first, 10))
    if (n > p.length - 10) { /* unreachable; keeps `n` honest below */ }
    n = p.length
  }

  // The barrel: a long square section on the axis with reinforcing bands, and a vented muzzle.
  const bx = 0.26
  const barrelFront = 2.15
  const barrelBack = -0.2
  const corners: [number, number][] = [[-1, 1], [1, 1], [1, -1], [-1, -1]]
  const bFirst = p.length
  for (const [dx, dy] of corners) {
    p.push([dx * bx, dy * bx, barrelFront], [dx * bx, dy * bx, barrelBack])
  }
  for (let i = 0; i < 4; i += 1) {
    e.push([bFirst + i * 2, bFirst + i * 2 + 1])
    const j = (i + 1) % 4
    e.push([bFirst + i * 2, bFirst + j * 2], [bFirst + i * 2 + 1, bFirst + j * 2 + 1])
  }
  for (const z of [2.0, 1.45, 0.9, 0.35]) {
    const first = ring(p, 8, 0.34, z, 'xy')
    e.push(...loop(first, 8))
  }
  // Muzzle brake: four vanes standing off the bore at the mouth.
  n = p.length
  for (const [dx, dy] of corners) {
    p.push([dx * 0.3, dy * 0.3, barrelFront], [dx * 0.52, dy * 0.52, barrelFront - 0.22])
    e.push([n, n + 1])
    n += 2
  }

  // Stabiliser fins in a cross at the stern. A hull that cannot turn still has to look like it is
  // *trying* to hold an axis, and four fins say that from any roll.
  for (const [dx, dy] of [[0, 1], [0, -1], [1, 0], [-1, 0]] as [number, number][]) {
    p.push(
      [dx * 0.7, dy * 0.36, -1.0], [dx * 1.25, dy * 0.7, -1.45],
      [dx * 1.25, dy * 0.7, -1.9], [dx * 0.7, dy * 0.36, -1.55],
    )
    e.push([n, n + 1], [n + 1, n + 2], [n + 2, n + 3], [n + 3, n])
    n += 4
  }

  // The bridge, tucked under the barrel rather than on top of it — the one place on this hull a
  // crew could see anything from.
  p.push([-0.22, -0.42, -0.35], [0.22, -0.42, -0.35], [0.22, -0.42, -0.8], [-0.22, -0.42, -0.8], [0, -0.3, -0.55])
  e.push([n, n + 1], [n + 1, n + 2], [n + 2, n + 3], [n + 3, n], [n + 4, n], [n + 4, n + 1], [n + 4, n + 2], [n + 4, n + 3])
  n += 5

  // Twin engines, offset below the axis so the thrust line reads as being under the gun.
  for (const s of [-1, 1]) {
    const first = ring(p, 6, 0.2, -1.4, 'xy')
    for (let i = 0; i < 6; i += 1) p[first + i][0] += s * 0.34
    for (let i = 0; i < 6; i += 1) p[first + i][1] -= 0.12
    e.push(...loop(first, 6))
    const back = ring(p, 6, 0.2, -1.85, 'xy')
    for (let i = 0; i < 6; i += 1) p[back + i][0] += s * 0.34
    for (let i = 0; i < 6; i += 1) p[back + i][1] -= 0.12
    e.push(...loop(back, 6))
    for (let i = 0; i < 6; i += 1) e.push([first + i, back + i])
  }
  // Ammunition handling: two feed rails running from the stern up to the breech, with hoists. The
  // hull's whole argument is that it carries one enormous gun, and a gun with no visible way of
  // being loaded is a prop. It is also what lifts the least-drawn capital clear of the mediums —
  // a tier that fills the frame must out-detail the tier that does not.
  for (const s of [-1, 1]) {
    const rail = n
    p.push([s * 0.42, 0.34, -0.3], [s * 0.42, 0.34, -1.35], [s * 0.5, 0.22, -0.3], [s * 0.5, 0.22, -1.35])
    e.push([rail, rail + 1], [rail + 2, rail + 3], [rail, rail + 2], [rail + 1, rail + 3])
    n += 4
    for (const z of [-0.5, -0.8, -1.1]) {
      p.push([s * 0.42, 0.34, z], [s * 0.5, 0.22, z], [s * 0.46, 0.44, z])
      e.push([n, n + 1], [n, n + 2], [n + 1, n + 2])
      n += 3
    }
  }
  // Armour belt along the flanks, stepped, so the hull under the gun has a profile of its own.
  for (const s of [-1, 1]) {
    for (const z of [-0.35, -0.7, -1.05]) {
      p.push([s * 0.64, 0.18, z], [s * 0.78, 0.1, z], [s * 0.78, -0.1, z], [s * 0.64, -0.18, z])
      e.push([n, n + 1], [n + 1, n + 2], [n + 2, n + 3])
      n += 4
    }
  }
  // Point-defence turrets on the dorsal surface: three small rings, because a siege hull that
  // cannot turn needs something that can.
  for (const z of [-0.4, -0.75, -1.1]) {
    const t = ring(p, 5, 0.09, z, 'xy')
    for (let i = 0; i < 5; i += 1) p[t + i][1] += 0.36
    e.push(...loop(t, 5))
    n = p.length
  }

  // Blast shielding around the muzzle: two standoff hoops on struts, ahead of the brake. A gun
  // this size vents somewhere, and this is also what lifts the least-drawn capital clear of the
  // busiest light hull — the tier that fills the frame must out-detail the tier that does not, and
  // that is pinned rather than assumed.
  for (const [z, r] of [[2.45, 0.42], [2.75, 0.5]] as [number, number][]) {
    const hoop = ring(p, 8, r, z, 'xy')
    e.push(...loop(hoop, 8))
    for (let i = 0; i < 8; i += 2) e.push([hoop + i, hoop + ((i + 1) % 8)])
    n = p.length
  }
  for (const [dx, dy] of [[1, 0], [-1, 0], [0, 1], [0, -1]] as [number, number][]) {
    p.push([dx * 0.34, dy * 0.34, 2.15], [dx * 0.46, dy * 0.46, 2.45], [dx * 0.5, dy * 0.5, 2.75])
    e.push([n, n + 1], [n + 1, n + 2])
    n += 3
  }

  return wire(p, e)
}

/**
 * The vanguard: the capital that can still leave.
 *
 * Forward-swept wings — the only ones in the game that rake the wrong way — and four engine pods in
 * a diamond at the stern. It reads as *going somewhere*, which is what separates it from the two
 * siege hulls at its own weight.
 */
export function vanguard(): Wire {
  const p: number[][] = [
    [0, 0, 1.6],
    [-0.34, 0.22, 0.5], [0.34, 0.22, 0.5], [-0.34, -0.22, 0.5], [0.34, -0.22, 0.5],
    [-0.42, 0.2, -1.0], [0.42, 0.2, -1.0], [-0.42, -0.2, -1.0], [0.42, -0.2, -1.0],
  ]
  const e: [number, number][] = [
    [0, 1], [0, 2], [0, 3], [0, 4],
    [1, 2], [3, 4], [1, 3], [2, 4],
    [1, 5], [2, 6], [3, 7], [4, 8],
    [5, 6], [7, 8], [5, 7], [6, 8],
  ]
  let n = p.length

  // ## Forward-swept wings, with a nacelle on each tip
  //
  // The only wings in the game that rake the *wrong* way, and the reason this hull reads as going
  // somewhere while the two siege capitals at its weight read as being parked. A tip nacelle is
  // what stops a forward sweep looking like a mistake: it gives the wing a reason to reach forward.
  for (const s of [-1, 1]) {
    const root = n
    p.push(
      [s * 0.36, 0.06, 0.35], [s * 0.36, 0.06, -0.7],
      [s * 1.25, 0.06, 0.95], [s * 1.25, 0.06, -0.35],
    )
    e.push([root, root + 2], [root + 1, root + 3], [root + 2, root + 3], [root, root + 1])
    n += 4
    // The nacelle: a box running fore-and-aft on the tip.
    const nac = n
    p.push(
      [s * 1.14, 0.14, 1.1], [s * 1.36, 0.14, 1.1], [s * 1.36, -0.06, 1.1], [s * 1.14, -0.06, 1.1],
      [s * 1.14, 0.14, -0.5], [s * 1.36, 0.14, -0.5], [s * 1.36, -0.06, -0.5], [s * 1.14, -0.06, -0.5],
    )
    e.push(
      [nac, nac + 1], [nac + 1, nac + 2], [nac + 2, nac + 3], [nac + 3, nac],
      [nac + 4, nac + 5], [nac + 5, nac + 6], [nac + 6, nac + 7], [nac + 7, nac + 4],
      [nac, nac + 4], [nac + 1, nac + 5], [nac + 2, nac + 6], [nac + 3, nac + 7],
    )
    n += 8
    // Radiator vanes along the flank: the ship is all drive, and it has to look like it sheds heat.
    for (const z of [0.1, -0.3, -0.7]) {
      p.push([s * 0.44, 0.24, z], [s * 0.7, 0.42, z], [s * 0.7, 0.42, z - 0.16], [s * 0.44, 0.24, z - 0.16])
      e.push([n, n + 1], [n + 1, n + 2], [n + 2, n + 3], [n + 3, n])
      n += 4
    }
  }

  // Dorsal spine, running most of the length above the hull.
  const spine = n
  p.push([0, 0.36, 0.8], [0, 0.56, 0.1], [0, 0.58, -0.6], [0, 0.4, -1.0])
  e.push([spine, spine + 1], [spine + 1, spine + 2], [spine + 2, spine + 3], [spine + 1, 1], [spine + 1, 2], [spine + 2, 5], [spine + 2, 6])
  n += 4

  // Four engines in a diamond, each a proper bell rather than a line.
  for (const [dx, dy] of [[0, 0.42], [0.42, 0], [0, -0.42], [-0.42, 0]] as [number, number][]) {
    const first = ring(p, 6, 0.17, -1.0, 'xy')
    for (let i = 0; i < 6; i += 1) { p[first + i][0] += dx; p[first + i][1] += dy }
    const back = ring(p, 6, 0.23, -1.55, 'xy')
    for (let i = 0; i < 6; i += 1) { p[back + i][0] += dx; p[back + i][1] += dy }
    e.push(...loop(first, 6), ...loop(back, 6))
    for (let i = 0; i < 6; i += 1) e.push([first + i, back + i])
    n = p.length
  }
  // Hull plating: four longitudinal strakes and a set of frames, so the body between the wings is
  // not a bare box. This is the tier that fills the frame, and it was the least-drawn hull in it.
  for (let i = 1; i <= 5; i += 1) {
    const t = i / 6
    const z = 0.5 - t * 1.5
    const w = 0.34 + 0.08 * t
    const h = 0.22 - 0.02 * t
    p.push([-w, h, z], [w, h, z], [w, -h, z], [-w, -h, z])
    e.push([n, n + 1], [n + 1, n + 2], [n + 2, n + 3], [n + 3, n])
    n += 4
  }
  for (const [dx, dy] of [[0.34, 0.2], [-0.34, 0.2], [0.34, -0.2], [-0.34, -0.2]] as [number, number][]) {
    p.push([dx, dy, 0.5], [dx * 1.2, dy, -1.0])
    e.push([n, n + 1])
    n += 2
  }
  // Intakes at the shoulders: this hull is all drive, and a drive with nothing feeding it is a
  // silhouette that stops making sense the moment you look for the reason it is shaped that way.
  for (const s of [-1, 1]) {
    const first = ring(p, 6, 0.14, 0.55, 'xy')
    for (let i = 0; i < 6; i += 1) { p[first + i][0] += s * 0.3; p[first + i][1] += 0.16 }
    const back = ring(p, 6, 0.11, 0.15, 'xy')
    for (let i = 0; i < 6; i += 1) { p[back + i][0] += s * 0.3; p[back + i][1] += 0.16 }
    e.push(...loop(first, 6), ...loop(back, 6))
    for (let i = 0; i < 6; i += 1) e.push([first + i, back + i])
    n = p.length
  }
  // A ventral fin, so the hull is not symmetric top to bottom and roll stays readable.
  p.push([0, -0.26, -0.3], [0, -0.62, -0.75], [0, -0.62, -1.0], [0, -0.24, -0.95])
  e.push([n, n + 1], [n + 1, n + 2], [n + 2, n + 3], [n + 3, n])
  n += 4

  return wire(p, e)
}

/**
 * The suzerain: a catamaran.
 *
 * **Two** spines, running parallel with the bridge slung between them, cross-braced along their
 * length. It is the only hull here without a single centreline, which is what makes it readable
 * beside the sovereign at the same weight — one is a spear, this is a gate.
 */
export function suzerain(): Wire {
  const p: number[][] = []
  const e: [number, number][] = []
  // The two hulls, each ribbed along its own length.
  for (const s of [-1, 1]) {
    const b = p.length
    p.push(
      [s * 0.62, 0, 1.8],
      [s * 0.42, 0.2, 0.9], [s * 0.82, 0.2, 0.9], [s * 0.82, -0.2, 0.9], [s * 0.42, -0.2, 0.9],
      [s * 0.42, 0.24, -1.4], [s * 0.86, 0.24, -1.4], [s * 0.86, -0.24, -1.4], [s * 0.42, -0.24, -1.4],
      [s * 0.64, 0, -1.85],
    )
    e.push(
      [b, b + 1], [b, b + 2], [b, b + 3], [b, b + 4],
      [b + 1, b + 2], [b + 2, b + 3], [b + 3, b + 4], [b + 4, b + 1],
      [b + 1, b + 5], [b + 2, b + 6], [b + 3, b + 7], [b + 4, b + 8],
      [b + 5, b + 6], [b + 6, b + 7], [b + 7, b + 8], [b + 8, b + 5],
      [b + 5, b + 9], [b + 6, b + 9], [b + 7, b + 9], [b + 8, b + 9],
    )
    let n = p.length
    for (let i = 1; i <= 5; i += 1) {
      const t = i / 6
      const z = 0.9 - t * 2.3
      const w = 0.21 + 0.03 * t
      const h = 0.2 + 0.04 * t
      p.push([s * 0.62 - w, h, z], [s * 0.62 + w, h, z], [s * 0.62 + w, -h, z], [s * 0.62 - w, -h, z])
      e.push([n, n + 1], [n + 1, n + 2], [n + 2, n + 3], [n + 3, n])
      n += 4
    }
    // Outboard sponsons: guns on the *outside* faces, which is the only place a catamaran has
    // clear arcs.
    for (const z of [0.5, -0.2, -0.9]) {
      p.push([s * 1.02, 0.06, z + 0.12], [s * 1.14, 0.06, z], [s * 1.02, 0.06, z - 0.12], [s * 1.02, -0.08, z])
      e.push([n, n + 1], [n + 1, n + 2], [n, n + 3], [n + 2, n + 3], [n + 1, n + 3])
      n += 4
    }
  }

  let n = p.length
  // Cross-braces between the hulls, five of them, so the gap reads as *structure* rather than as
  // two ships flying in formation — which is what three braces looked like.
  for (const z of [1.1, 0.5, -0.1, -0.7, -1.3]) {
    p.push([-0.42, 0.12, z], [0.42, 0.12, z], [-0.42, -0.12, z], [0.42, -0.12, z])
    e.push([n, n + 1], [n + 2, n + 3], [n, n + 2], [n + 1, n + 3], [n, n + 3], [n + 1, n + 2])
    n += 4
  }

  // ## The spinal weapon, slung between the hulls
  //
  // The identifying feature and the reason the catamaran exists: a gun too long to mount on either
  // hull, carried in the gap. Nothing else in the game has anything *between* two structures, so
  // this reads at any range where the two hulls are separable.
  const gun = n
  p.push([0, 0.02, 2.1], [0, 0.02, -1.2])
  e.push([gun, gun + 1])
  n += 2
  for (const z of [1.9, 1.2, 0.5, -0.2, -0.9]) {
    const first = ring(p, 6, 0.16, z, 'xy')
    e.push(...loop(first, 6))
    e.push([first, gun], [first + 3, gun])
    n = p.length
  }

  // Twin bridges, one on each hull, so neither is the ship's single head.
  for (const s of [-1, 1]) {
    p.push([s * 0.62, 0.44, 0.3], [s * 0.62, 0.44, -0.4], [s * 0.5, 0.24, -0.05], [s * 0.74, 0.24, -0.05])
    e.push([n, n + 1], [n, n + 2], [n, n + 3], [n + 1, n + 2], [n + 1, n + 3], [n + 2, n + 3])
    n += 4
  }
  // Rail gantries along the inner faces of both hulls, carrying the spinal gun's feed. A weapon
  // slung between two ships has to be *held* by them, and two hulls flanking a floating tube is
  // the version of this shape that looks unfinished.
  for (const s of [-1, 1]) {
    const g = n
    p.push([s * 0.34, 0.16, 1.5], [s * 0.34, 0.16, -1.0], [s * 0.34, -0.16, 1.5], [s * 0.34, -0.16, -1.0])
    e.push([g, g + 1], [g + 2, g + 3], [g, g + 2], [g + 1, g + 3])
    n += 4
    for (const z of [1.2, 0.4, -0.4]) {
      p.push([s * 0.34, 0.16, z], [s * 0.12, 0.04, z], [s * 0.34, -0.16, z])
      e.push([n, n + 1], [n + 1, n + 2])
      n += 3
    }
  }
  // Dorsal turrets on each hull, so both halves are armed independently of the spinal gun.
  for (const s of [-1, 1]) {
    for (const z of [0.6, -0.6]) {
      const t = ring(p, 5, 0.1, z, 'xy')
      for (let i = 0; i < 5; i += 1) { p[t + i][0] += s * 0.62; p[t + i][1] += 0.3 }
      e.push(...loop(t, 5))
      n = p.length
    }
  }

  return wire(p, e)
}

/**
 * The dominion: the endgame hull, and **the largest thing in the sector**.
 *
 * ## The rule this deliberately reverses
 *
 * The heavy tiers landed with a stated rule — *you never become the biggest thing out here* — and
 * an equality pinned in `check:scemaworld` holding the largest flyable hull to exactly a hostile
 * dreadnought's radius. The reasoning was that a game whose top purchase makes you the apex object
 * has nothing left to point at.
 *
 * That is now overruled, on purpose and with the cost named rather than hidden: the endgame hull is
 * **larger than a titan**. What it buys is the one thing the old rule refused, which is an ending —
 * a purchase that is visibly the end of the ladder rather than another rung on it. What it costs is
 * exactly what the old note said: the sector no longer contains anything bigger than you, so the
 * silhouette on the horizon stops being a question. The mitigation is that a titan remains the
 * hardest thing in it to *kill* — size and threat were never the same axis, and eight warheads is
 * eight warheads whoever is flying past.
 *
 * ## The shape
 *
 * A spinal core inside a **ring**, braced by four outriggers. Nothing else in the game is annular —
 * the citadels are, and they are stations — so at any distance where a hull is a few pixels this
 * one is the only ship that reads as having a hole in it. That is deliberate: at the size it is
 * drawn, an outline is all anyone gets, and an outline nobody else shares is the whole job.
 */
export function dominion(): Wire {
  const p: number[][] = [
    // The spinal core, running the full length.
    [0, 0, 2.0],
    [-0.2, 0.2, 1.1], [0.2, 0.2, 1.1], [0.2, -0.2, 1.1], [-0.2, -0.2, 1.1],
    [-0.26, 0.26, -1.5], [0.26, 0.26, -1.5], [0.26, -0.26, -1.5], [-0.26, -0.26, -1.5],
    [0, 0, -2.0],
  ]
  const e: [number, number][] = [
    [0, 1], [0, 2], [0, 3], [0, 4],
    [1, 2], [2, 3], [3, 4], [4, 1],
    [1, 5], [2, 6], [3, 7], [4, 8],
    [5, 6], [6, 7], [7, 8], [8, 5],
    [5, 9], [6, 9], [7, 9], [8, 9],
  ]

  // ## Two rings, and the second one is canted
  //
  // The outer ring is the silhouette — the only annular ship in the sector, so at the range where
  // a hull is a handful of pixels this is the one with a hole in it. The **inner ring is tilted**,
  // which is what stops the pair reading as a single thick band: two circles on one plane are one
  // circle, and two at an angle are unmistakably a structure. It also gives the hull a readable
  // orientation from head-on, where a ring alone is identical in every roll.
  const outer = ring(p, 20, 1.0, -0.1, 'xy')
  e.push(...loop(outer, 20))
  const outerBack = ring(p, 20, 1.0, -0.34, 'xy')
  e.push(...loop(outerBack, 20))
  for (let i = 0; i < 20; i += 1) e.push([outer + i, outerBack + i])

  const inner = ring(p, 16, 0.74, 0, 'xy')
  // Cant it: shear z with x, so the ring tips about the vertical axis.
  for (let i = 0; i < 16; i += 1) p[inner + i][2] += p[inner + i][0] * 0.42 - 0.1
  e.push(...loop(inner, 16))
  for (let i = 0; i < 16; i += 4) e.push([inner + i, outer + i + (i % 5)])

  // Four radial pylons carrying the ring off the core, each a braced tower rather than a strut —
  // a line from a ring to a spine reads as a wire holding a hoop.
  let n = p.length
  for (const [dx, dy] of [[0, 1], [1, 0], [0, -1], [-1, 0]] as [number, number][]) {
    const base = n
    p.push(
      [dx * 0.24, dy * 0.24, 0.1], [dx * 0.24, dy * 0.24, -0.4],
      [dx * 0.92, dy * 0.92, 0.05], [dx * 0.92, dy * 0.92, -0.4],
      [dx * 0.58, dy * 0.58, 0.35], [dx * 0.58, dy * 0.58, -0.65],
    )
    e.push(
      [base, base + 2], [base + 1, base + 3], [base, base + 1], [base + 2, base + 3],
      [base + 4, base], [base + 4, base + 2], [base + 5, base + 1], [base + 5, base + 3],
    )
    n += 6
  }

  // A lance at the prow: the ship's own axial weapon, banded, reaching past the nose.
  const lanceTip = n
  p.push([0, 0, 2.75])
  e.push([lanceTip, 0])
  n += 1
  for (const z of [2.6, 2.35, 2.1]) {
    const first = ring(p, 6, 0.1 + (2.6 - z) * 0.18, z, 'xy')
    e.push(...loop(first, 6))
    e.push([first, lanceTip], [first + 3, lanceTip])
    n = p.length
  }

  // Two command towers, fore and aft of the ring, so the hull is not symmetric end to end.
  for (const [z, h] of [[1.3, 0.95], [-1.1, 0.7]] as [number, number][]) {
    p.push([0, 0.42, z], [0, h, z - 0.35], [-0.24, h * 0.75, z - 0.5], [0.24, h * 0.75, z - 0.5], [0, 0.3, z - 0.7])
    e.push([n, n + 1], [n + 1, n + 2], [n + 1, n + 3], [n + 2, n + 4], [n + 3, n + 4], [n + 1, n + 4])
    n += 5
  }

  // Engine cluster at the stern: six bells in a ring, matching the hull's own geometry, each with
  // a nozzle collar.
  const bells = ring(p, 6, 0.5, -1.6, 'xy')
  const nozz = ring(p, 6, 0.58, -2.1, 'xy')
  const flare = ring(p, 6, 0.66, -2.35, 'xy')
  for (let i = 0; i < 6; i += 1) e.push([bells + i, nozz + i], [nozz + i, flare + i])
  e.push(...loop(bells, 6), ...loop(nozz, 6), ...loop(flare, 6))

  // Ribs along the core. At this size the eye is close enough to notice their absence.
  n = p.length
  for (let i = 1; i <= 10; i += 1) {
    const t = i / 11
    const z = 1.1 - t * 2.6
    const w = 0.2 + 0.08 * t
    p.push([-w, w, z], [w, w, z], [w, -w, z], [-w, -w, z])
    e.push([n, n + 1], [n + 1, n + 2], [n + 2, n + 3], [n + 3, n])
    n += 4
  }
  // ## The endgame hull is the most drawn thing in the game, and that is a rule
  //
  // A ladder whose top rung is not visibly the top has no top. It briefly was not — the
  // `sovereign` overtook it once hangar bays were added — which is the ordinary way a "biggest and
  // best" claim rots: everything else gets a pass and the flagship does not. `check:scemaworld`
  // pins it now.
  //
  // A ring of hangar mouths around the inner face of the torus: the ship carries a fleet, and the
  // only surface it could launch one from is the one facing its own axis.
  for (let i = 0; i < 6; i += 1) {
    const a = (i / 6) * Math.PI * 2 + 0.26
    const cx = Math.cos(a)
    const cy = Math.sin(a)
    const b = n
    p.push(
      [cx * 0.86, cy * 0.86, 0.02], [cx * 0.86, cy * 0.86, -0.24],
      [cx * 0.7, cy * 0.7, -0.24], [cx * 0.7, cy * 0.7, 0.02],
    )
    e.push([b, b + 1], [b + 1, b + 2], [b + 2, b + 3], [b + 3, b], [b, b + 2])
    n += 4
  }
  // Turret barbettes along the core, above and below, so the spine is armed along its whole run.
  for (const z of [1.0, 0.4, -0.6, -1.2]) {
    for (const s of [1, -1]) {
      const t = ring(p, 6, 0.1, z, 'xy')
      for (let i = 0; i < 6; i += 1) p[t + i][1] += s * 0.3
      e.push(...loop(t, 6))
      n = p.length
    }
  }
  // Radiator fins between the pylons, on the diagonals: a hull this size sheds heat somewhere.
  for (const [dx, dy] of [[0.7, 0.7], [-0.7, 0.7], [0.7, -0.7], [-0.7, -0.7]] as [number, number][]) {
    p.push([dx * 0.42, dy * 0.42, -0.5], [dx * 0.95, dy * 0.95, -0.85],
           [dx * 0.95, dy * 0.95, -1.25], [dx * 0.42, dy * 0.42, -1.0])
    e.push([n, n + 1], [n + 1, n + 2], [n + 2, n + 3], [n + 3, n], [n, n + 2])
    n += 4
  }

  return wire(p, e)
}

/**
 * ## The hostile and civilian classes get their own silhouettes too
 *
 * Fifteen classes shared four shapes. A courier, a marshal and a raider interceptor were the *same
 * dart*; a leviathan, a titan, a warden and a bastion were the same war hull at four sizes. Colour
 * was doing all the work, and this project's own rule — stated in `view.ts`, in `theme.rs`, in
 * `alchem_link.theme` — is that **colour is decoration and never the message**. It is at its most
 * load-bearing here, because the question a silhouette has to answer is *is that coming for me*,
 * and the answer arrives from the corner of an eye at a range where hue is unreliable.
 *
 * Each family reads as its faction before it reads as its class:
 *
 * - **Raiders** are asymmetric and over-engined. Nothing on them lines up, because nothing about a
 *   freehold hull was built as a set.
 * - **The patrol** is blocky, symmetrical and institutional, and every one of them carries the
 *   same blade fin — a marshal is recognisable as a marshal before you can tell which marshal.
 * - **Civilians** carry visible cargo and no guns, so a courier reads as *not a threat* at a range
 *   where its colour is a couple of pixels.
 */

/** A raider skiff: scrap, with an engine far too large for it. */
export function raiderSkiff(): Wire {
  const p: number[][] = [
    [0, 0.04, 1.0], [-0.4, 0.14, 0.1], [0.34, -0.1, 0.15], [-0.3, -0.16, -0.2], [0.42, 0.18, -0.3],
    [0, 0, -0.5],
  ]
  const e: [number, number][] = [
    [0, 1], [0, 2], [0, 3], [0, 4],
    [1, 3], [2, 4], [1, 4], [2, 3],
    [1, 5], [2, 5], [3, 5], [4, 5],
  ]
  let n = p.length
  const bell = ring(p, 6, 0.3, -0.55, 'xy')
  const flare = ring(p, 6, 0.38, -0.95, 'xy')
  e.push(...loop(bell, 6), ...loop(flare, 6))
  for (let i = 0; i < 6; i += 1) e.push([bell + i, flare + i], [bell + i, 5])
  n = p.length
  // One fin, on one side. A hull with a feature it has only half of is the cheapest way to read
  // "put together out of what was available".
  p.push([-0.42, 0.5, -0.35], [-0.2, 0.16, -0.1], [-0.5, 0.2, -0.6])
  e.push([n, n + 1], [n, n + 2], [n + 1, n + 2], [n + 1, 1])
  return wire(p, e)
}

/** A raider lancer: a long standoff hull built around a nose gun. */
export function raiderLancer(): Wire {
  const p: number[][] = [
    [0, 0, 1.5], [0, 0, 0.35],
    [-0.2, 0.16, 0.2], [0.2, 0.16, 0.2], [0.2, -0.16, 0.2], [-0.2, -0.16, 0.2],
    [-0.26, 0.14, -0.9], [0.26, 0.14, -0.9], [0.26, -0.14, -0.9], [-0.26, -0.14, -0.9],
  ]
  const e: [number, number][] = [
    [0, 1], [1, 2], [1, 3], [1, 4], [1, 5],
    [2, 3], [3, 4], [4, 5], [5, 2],
    [2, 6], [3, 7], [4, 8], [5, 9],
    [6, 7], [7, 8], [8, 9], [9, 6],
  ]
  let n = p.length
  for (const z of [1.2, 0.85]) {
    const r = ring(p, 5, 0.1, z, 'xy')
    e.push(...loop(r, 5))
    e.push([r, 0], [r + 2, 0])
    n = p.length
  }
  // Twin outboard tanks on struts, mismatched lengths — the raider signature.
  for (const [s, back] of [[-1, -0.95], [1, -0.75]] as [number, number][]) {
    p.push([s * 0.28, -0.06, -0.2], [s * 0.6, -0.14, -0.2])
    e.push([n, n + 1])
    n += 2
    const a = ring(p, 5, 0.12, 0.1, 'xy')
    for (let i = 0; i < 5; i += 1) { p[a + i][0] += s * 0.6; p[a + i][1] -= 0.14 }
    const b = ring(p, 5, 0.12, back, 'xy')
    for (let i = 0; i < 5; i += 1) { p[b + i][0] += s * 0.6; p[b + i][1] -= 0.14 }
    e.push(...loop(a, 5), ...loop(b, 5))
    for (let i = 0; i < 5; i += 1) e.push([a + i, b + i])
    n = p.length
  }
  return wire(p, e)
}

/** A frigate: the smallest capital. A blunt wedge with a dorsal gun deck. */
export function frigate(): Wire {
  const p: number[][] = [
    [0, 0, 1.3],
    [-0.44, 0.2, 0.35], [0.44, 0.2, 0.35], [-0.44, -0.22, 0.35], [0.44, -0.22, 0.35],
    [-0.54, 0.2, -1.0], [0.54, 0.2, -1.0], [-0.54, -0.22, -1.0], [0.54, -0.22, -1.0],
  ]
  const e: [number, number][] = [
    [0, 1], [0, 2], [0, 3], [0, 4],
    [1, 2], [3, 4], [1, 3], [2, 4],
    [1, 5], [2, 6], [3, 7], [4, 8],
    [5, 6], [7, 8], [5, 7], [6, 8],
  ]
  let n = p.length
  // A gun deck: a raised platform along the spine with three turret rings on it.
  p.push([-0.28, 0.3, 0.2], [0.28, 0.3, 0.2], [0.28, 0.3, -0.8], [-0.28, 0.3, -0.8])
  e.push([n, n + 1], [n + 1, n + 2], [n + 2, n + 3], [n + 3, n], [n, 1], [n + 1, 2], [n + 2, 6], [n + 3, 5])
  n += 4
  for (const z of [0.05, -0.3, -0.65]) {
    const t = ring(p, 5, 0.09, z, 'xy')
    for (let i = 0; i < 5; i += 1) p[t + i][1] += 0.36
    e.push(...loop(t, 5))
    n = p.length
  }
  for (let i = 1; i <= 3; i += 1) {
    const t = i / 4
    const z = 0.35 - t * 1.35
    const w = 0.44 + 0.1 * t
    p.push([-w, 0.2, z], [w, 0.2, z], [w, -0.22, z], [-w, -0.22, z])
    e.push([n, n + 1], [n + 1, n + 2], [n + 2, n + 3], [n + 3, n])
    n += 4
  }
  for (const x of [-0.26, 0.26]) {
    const b = ring(p, 5, 0.14, -1.0, 'xy')
    for (let i = 0; i < 5; i += 1) p[b + i][0] += x
    const f = ring(p, 5, 0.17, -1.3, 'xy')
    for (let i = 0; i < 5; i += 1) p[f + i][0] += x
    e.push(...loop(b, 5), ...loop(f, 5))
    for (let i = 0; i < 5; i += 1) e.push([b + i, f + i])
    n = p.length
  }
  return wire(p, e)
}

/** A destroyer: a long slab, all broadside. */
export function destroyer(): Wire {
  const p: number[][] = [
    [0, 0, 1.7],
    [-0.4, 0.2, 0.7], [0.4, 0.2, 0.7], [-0.4, -0.22, 0.7], [0.4, -0.22, 0.7],
    [-0.5, 0.22, -1.5], [0.5, 0.22, -1.5], [-0.5, -0.24, -1.5], [0.5, -0.24, -1.5],
  ]
  const e: [number, number][] = [
    [0, 1], [0, 2], [0, 3], [0, 4],
    [1, 2], [3, 4], [1, 3], [2, 4],
    [1, 5], [2, 6], [3, 7], [4, 8],
    [5, 6], [7, 8], [5, 7], [6, 8],
  ]
  let n = p.length
  // Broadside casemates: a row of gun ports down each flank, which is the whole of what a
  // destroyer is and the thing a wedge alone does not say.
  for (const s of [-1, 1]) {
    for (const z of [0.4, 0.0, -0.4, -0.8, -1.2]) {
      p.push([s * 0.48, 0.06, z + 0.12], [s * 0.62, 0.06, z], [s * 0.48, 0.06, z - 0.12], [s * 0.48, -0.1, z])
      e.push([n, n + 1], [n + 1, n + 2], [n, n + 3], [n + 2, n + 3], [n + 1, n + 3])
      n += 4
    }
  }
  for (let i = 1; i <= 5; i += 1) {
    const t = i / 6
    const z = 0.7 - t * 2.2
    const w = 0.4 + 0.1 * t
    p.push([-w, 0.21, z], [w, 0.21, z], [w, -0.23, z], [-w, -0.23, z], [0, 0.36, z])
    e.push([n, n + 1], [n + 1, n + 2], [n + 2, n + 3], [n + 3, n], [n + 4, n], [n + 4, n + 1])
    n += 5
  }
  for (const x of [-0.3, 0, 0.3]) {
    const b = ring(p, 5, 0.14, -1.5, 'xy')
    for (let i = 0; i < 5; i += 1) p[b + i][0] += x
    const f = ring(p, 5, 0.17, -1.85, 'xy')
    for (let i = 0; i < 5; i += 1) p[f + i][0] += x
    e.push(...loop(b, 5), ...loop(f, 5))
    for (let i = 0; i < 5; i += 1) e.push([b + i, f + i])
    n = p.length
  }
  return wire(p, e)
}

/** A warfighter: the medium war hull. A broad arrowhead with under-slung pods. */
export function warfighter(): Wire {
  const p: number[][] = [
    [0, 0, 1.5],
    [-0.85, 0.1, -0.5], [0.85, 0.1, -0.5],
    [-0.4, 0.24, -0.1], [0.4, 0.24, -0.1],
    [-0.4, -0.2, -0.1], [0.4, -0.2, -0.1],
    [-0.5, 0.2, -1.15], [0.5, 0.2, -1.15], [-0.5, -0.18, -1.15], [0.5, -0.18, -1.15],
  ]
  const e: [number, number][] = [
    [0, 1], [0, 2], [0, 3], [0, 4], [0, 5], [0, 6],
    [1, 3], [2, 4], [1, 5], [2, 6],
    [3, 4], [5, 6], [3, 5], [4, 6],
    [3, 7], [4, 8], [5, 9], [6, 10],
    [7, 8], [9, 10], [7, 9], [8, 10],
    [1, 7], [2, 8],
  ]
  let n = p.length
  // Under-slung weapon pods on the swept edges, and a dorsal blister.
  for (const s of [-1, 1]) {
    p.push([s * 0.6, -0.14, 0.1], [s * 0.6, -0.14, -0.75], [s * 0.72, -0.06, -0.3])
    e.push([n, n + 1], [n, n + 2], [n + 1, n + 2], [n, s < 0 ? 5 : 6], [n + 1, s < 0 ? 9 : 10])
    n += 3
  }
  p.push([0, 0.38, 0.2], [-0.18, 0.3, -0.3], [0.18, 0.3, -0.3], [0, 0.3, -0.6])
  e.push([n, n + 1], [n, n + 2], [n + 1, n + 3], [n + 2, n + 3], [n, 0])
  n += 4
  for (const x of [-0.24, 0.24]) {
    const b = ring(p, 5, 0.13, -1.15, 'xy')
    for (let i = 0; i < 5; i += 1) p[b + i][0] += x
    const f = ring(p, 5, 0.16, -1.45, 'xy')
    for (let i = 0; i < 5; i += 1) p[f + i][0] += x
    e.push(...loop(b, 5), ...loop(f, 5))
    for (let i = 0; i < 5; i += 1) e.push([b + i, f + i])
    n = p.length
  }
  return wire(p, e)
}

/** A leviathan: twin spines joined by a spine bridge. Bigger than a dreadnought and not it. */
export function leviathan(): Wire {
  const p: number[][] = []
  const e: [number, number][] = []
  for (const s of [-1, 1]) {
    const b = p.length
    p.push(
      [s * 0.4, 0, 2.0],
      [s * 0.24, 0.24, 1.1], [s * 0.58, 0.24, 1.1], [s * 0.58, -0.24, 1.1], [s * 0.24, -0.24, 1.1],
      [s * 0.24, 0.3, -1.6], [s * 0.64, 0.3, -1.6], [s * 0.64, -0.3, -1.6], [s * 0.24, -0.3, -1.6],
    )
    e.push(
      [b, b + 1], [b, b + 2], [b, b + 3], [b, b + 4],
      [b + 1, b + 2], [b + 2, b + 3], [b + 3, b + 4], [b + 4, b + 1],
      [b + 1, b + 5], [b + 2, b + 6], [b + 3, b + 7], [b + 4, b + 8],
      [b + 5, b + 6], [b + 6, b + 7], [b + 7, b + 8], [b + 8, b + 5],
    )
    let n = p.length
    for (let i = 1; i <= 7; i += 1) {
      const t = i / 8
      const z = 1.1 - t * 2.7
      const w = 0.17 + 0.04 * t
      const h = 0.24 + 0.06 * t
      p.push([s * 0.41 - w, h, z], [s * 0.41 + w, h, z], [s * 0.41 + w, -h, z], [s * 0.41 - w, -h, z])
      e.push([n, n + 1], [n + 1, n + 2], [n + 2, n + 3], [n + 3, n])
      n += 4
    }
    for (const x of [-0.14, 0.14]) {
      const bb = ring(p, 5, 0.14, -1.6, 'xy')
      for (let i = 0; i < 5; i += 1) p[bb + i][0] += s * 0.41 + x
      const ff = ring(p, 5, 0.17, -2.0, 'xy')
      for (let i = 0; i < 5; i += 1) p[ff + i][0] += s * 0.41 + x
      e.push(...loop(bb, 5), ...loop(ff, 5))
      for (let i = 0; i < 5; i += 1) e.push([bb + i, ff + i])
    }
  }
  let n = p.length
  // The bridge between the spines, and a spinal weapon slung under it.
  for (const z of [0.9, 0.1, -0.7, -1.4]) {
    p.push([-0.24, 0.2, z], [0.24, 0.2, z], [-0.24, -0.2, z], [0.24, -0.2, z])
    e.push([n, n + 1], [n + 2, n + 3], [n, n + 2], [n + 1, n + 3], [n, n + 3])
    n += 4
  }
  p.push([0, 0.46, 0.4], [0, 0.46, -0.5], [0, 0.2, -0.05])
  e.push([n, n + 1], [n, n + 2], [n + 1, n + 2])
  n += 3
  p.push([0, -0.3, 1.5], [0, -0.3, -1.0])
  e.push([n, n + 1])
  return wire(p, e)
}

/** A titan: a wedge-city. The largest hostile in the sector. */
export function titan(): Wire {
  const p: number[][] = [
    [0, 0, 2.2],
    [-0.75, 0.3, 0.6], [0.75, 0.3, 0.6], [-0.75, -0.34, 0.6], [0.75, -0.34, 0.6],
    [-0.95, 0.34, -1.7], [0.95, 0.34, -1.7], [-0.95, -0.38, -1.7], [0.95, -0.38, -1.7],
  ]
  const e: [number, number][] = [
    [0, 1], [0, 2], [0, 3], [0, 4],
    [1, 2], [3, 4], [1, 3], [2, 4],
    [1, 5], [2, 6], [3, 7], [4, 8],
    [5, 6], [7, 8], [5, 7], [6, 8],
  ]
  let n = p.length
  // The city: five towers of different heights along the spine. It is the one hostile that should
  // read as *inhabited* rather than as a machine, and a skyline is how a silhouette says so.
  for (const [z, h, w] of [[0.4, 0.72, 0.16], [-0.1, 0.95, 0.2], [-0.6, 0.66, 0.14], [-1.05, 0.86, 0.18], [-1.45, 0.5, 0.12]] as [number, number, number][]) {
    p.push([-w, 0.3, z + w], [w, 0.3, z + w], [w, 0.3, z - w], [-w, 0.3, z - w])
    p.push([-w * 0.7, h, z + w * 0.7], [w * 0.7, h, z + w * 0.7], [w * 0.7, h, z - w * 0.7], [-w * 0.7, h, z - w * 0.7])
    e.push(
      [n, n + 1], [n + 1, n + 2], [n + 2, n + 3], [n + 3, n],
      [n + 4, n + 5], [n + 5, n + 6], [n + 6, n + 7], [n + 7, n + 4],
      [n, n + 4], [n + 1, n + 5], [n + 2, n + 6], [n + 3, n + 7],
    )
    n += 8
  }
  for (let i = 1; i <= 7; i += 1) {
    const t = i / 8
    const z = 0.6 - t * 2.3
    const w = 0.75 + 0.2 * t
    p.push([-w, 0.31, z], [w, 0.31, z], [w, -0.35, z], [-w, -0.35, z], [0, -0.52, z])
    e.push([n, n + 1], [n + 1, n + 2], [n + 2, n + 3], [n + 3, n], [n + 4, n + 2], [n + 4, n + 3])
    n += 5
  }
  for (const s of [-1, 1]) {
    for (const z of [0.2, -0.4, -1.0]) {
      p.push([s * 0.88, 0.02, z + 0.16], [s * 1.06, 0.02, z], [s * 0.88, 0.02, z - 0.16], [s * 0.88, -0.16, z])
      e.push([n, n + 1], [n + 1, n + 2], [n, n + 3], [n + 2, n + 3], [n + 1, n + 3])
      n += 4
    }
  }
  for (const x of [-0.55, -0.18, 0.18, 0.55]) {
    const b = ring(p, 6, 0.17, -1.7, 'xy')
    for (let i = 0; i < 6; i += 1) p[b + i][0] += x
    const f = ring(p, 6, 0.21, -2.15, 'xy')
    for (let i = 0; i < 6; i += 1) p[f + i][0] += x
    e.push(...loop(b, 6), ...loop(f, 6))
    for (let i = 0; i < 6; i += 1) e.push([b + i, f + i])
    n = p.length
  }
  return wire(p, e)
}

/** A courier: a pod with a canister slung under it. Visibly carrying, visibly unarmed. */
export function courier(): Wire {
  const p: number[][] = [
    [0, 0.06, 1.0],
    [-0.2, 0.18, 0.2], [0.2, 0.18, 0.2], [-0.2, -0.06, 0.2], [0.2, -0.06, 0.2],
    [-0.2, 0.16, -0.6], [0.2, 0.16, -0.6], [-0.2, -0.06, -0.6], [0.2, -0.06, -0.6],
  ]
  const e: [number, number][] = [
    [0, 1], [0, 2], [0, 3], [0, 4],
    [1, 2], [3, 4], [1, 3], [2, 4],
    [1, 5], [2, 6], [3, 7], [4, 8],
    [5, 6], [7, 8], [5, 7], [6, 8],
  ]
  let n = p.length
  // The canister. A civilian hull has to read as *carrying something* at the range where its
  // colour is two pixels, and a cylinder under the belly is the only thing that does it.
  const a = ring(p, 6, 0.15, 0.45, 'xy')
  for (let i = 0; i < 6; i += 1) p[a + i][1] -= 0.3
  const b = ring(p, 6, 0.15, -0.5, 'xy')
  for (let i = 0; i < 6; i += 1) p[b + i][1] -= 0.3
  e.push(...loop(a, 6), ...loop(b, 6))
  for (let i = 0; i < 6; i += 1) e.push([a + i, b + i])
  n = p.length
  p.push([-0.12, -0.06, 0.1], [-0.12, -0.16, 0.1], [0.12, -0.06, 0.1], [0.12, -0.16, 0.1])
  e.push([n, n + 1], [n + 2, n + 3])
  n += 4
  const bell = ring(p, 5, 0.11, -0.6, 'xy')
  const flare = ring(p, 5, 0.13, -0.85, 'xy')
  e.push(...loop(bell, 5), ...loop(flare, 5))
  for (let i = 0; i < 5; i += 1) e.push([bell + i, flare + i])
  return wire(p, e)
}

/** A freighter: a spine with containers racked along it. */
export function freighter(): Wire {
  const p: number[][] = [
    [0, 0.1, 1.2],
    [-0.22, 0.24, 0.7], [0.22, 0.24, 0.7], [-0.22, -0.02, 0.7], [0.22, -0.02, 0.7],
    [-0.22, 0.22, -1.2], [0.22, 0.22, -1.2], [-0.22, -0.02, -1.2], [0.22, -0.02, -1.2],
  ]
  const e: [number, number][] = [
    [0, 1], [0, 2], [0, 3], [0, 4],
    [1, 2], [3, 4], [1, 3], [2, 4],
    [1, 5], [2, 6], [3, 7], [4, 8],
    [5, 6], [7, 8], [5, 7], [6, 8],
  ]
  let n = p.length
  // Containers: six boxes racked either side of the spine. The cue is *count* — a hauler is a
  // ship you can see the cargo on, and a hauler with none is riding empty.
  for (const s of [-1, 1]) {
    for (const z of [0.4, -0.1, -0.6]) {
      const b = n
      p.push(
        [s * 0.26, 0.2, z + 0.22], [s * 0.62, 0.2, z + 0.22], [s * 0.62, -0.08, z + 0.22], [s * 0.26, -0.08, z + 0.22],
        [s * 0.26, 0.2, z - 0.22], [s * 0.62, 0.2, z - 0.22], [s * 0.62, -0.08, z - 0.22], [s * 0.26, -0.08, z - 0.22],
      )
      e.push(
        [b, b + 1], [b + 1, b + 2], [b + 2, b + 3], [b + 3, b],
        [b + 4, b + 5], [b + 5, b + 6], [b + 6, b + 7], [b + 7, b + 4],
        [b, b + 4], [b + 1, b + 5], [b + 2, b + 6], [b + 3, b + 7],
      )
      n += 8
    }
  }
  p.push([-0.14, 0.36, 0.9], [0.14, 0.36, 0.9], [0.14, 0.36, 0.55], [-0.14, 0.36, 0.55])
  e.push([n, n + 1], [n + 1, n + 2], [n + 2, n + 3], [n + 3, n], [n, 1], [n + 1, 2])
  n += 4
  for (const x of [-0.14, 0.14]) {
    const b = ring(p, 5, 0.12, -1.2, 'xy')
    for (let i = 0; i < 5; i += 1) { p[b + i][0] += x; p[b + i][1] += 0.1 }
    const f = ring(p, 5, 0.15, -1.5, 'xy')
    for (let i = 0; i < 5; i += 1) { p[f + i][0] += x; p[f + i][1] += 0.1 }
    e.push(...loop(b, 5), ...loop(f, 5))
    for (let i = 0; i < 5; i += 1) e.push([b + i, f + i])
    n = p.length
  }
  return wire(p, e)
}

/**
 * The blade fin every patrol hull carries, appended to an existing point list.
 *
 * One shared feature across the three marshal classes, at three scales. A marshal has to be
 * recognisable *as a marshal* before you can tell which marshal — that is what an institution
 * looks like from a distance, and it is the half of the job a per-class silhouette cannot do.
 */
function bladeFin(p: number[][], e: [number, number][], scale: number, z: number): void {
  const n = p.length
  p.push(
    [0, 0.2 * scale, z + 0.1 * scale],
    [0, 0.85 * scale, z - 0.1 * scale],
    [0, 0.85 * scale, z - 0.45 * scale],
    [0, 0.2 * scale, z - 0.55 * scale],
    [-0.1 * scale, 0.5 * scale, z - 0.25 * scale],
    [0.1 * scale, 0.5 * scale, z - 0.25 * scale],
  )
  e.push(
    [n, n + 1], [n + 1, n + 2], [n + 2, n + 3], [n + 3, n],
    [n + 4, n + 1], [n + 4, n + 3], [n + 5, n + 1], [n + 5, n + 3], [n + 4, n + 5],
  )
}

/** A marshal interceptor: institutional, symmetrical, and finned. */
export function marshal(): Wire {
  const p: number[][] = [
    [0, 0, 1.2],
    [-0.24, 0.12, 0.3], [0.24, 0.12, 0.3], [-0.24, -0.12, 0.3], [0.24, -0.12, 0.3],
    [-0.28, 0.12, -0.7], [0.28, 0.12, -0.7], [-0.28, -0.12, -0.7], [0.28, -0.12, -0.7],
    [-0.7, 0, -0.2], [0.7, 0, -0.2], [-0.62, 0, -0.65], [0.62, 0, -0.65],
  ]
  const e: [number, number][] = [
    [0, 1], [0, 2], [0, 3], [0, 4],
    [1, 2], [3, 4], [1, 3], [2, 4],
    [1, 5], [2, 6], [3, 7], [4, 8],
    [5, 6], [7, 8], [5, 7], [6, 8],
    [9, 11], [10, 12], [9, 1], [10, 2], [11, 5], [12, 6], [9, 3], [10, 4],
  ]
  bladeFin(p, e, 0.6, -0.1)
  let n = p.length
  for (const x of [-0.14, 0.14]) {
    const b = ring(p, 5, 0.1, -0.7, 'xy')
    for (let i = 0; i < 5; i += 1) p[b + i][0] += x
    const f = ring(p, 5, 0.12, -0.95, 'xy')
    for (let i = 0; i < 5; i += 1) p[f + i][0] += x
    e.push(...loop(b, 5), ...loop(f, 5))
    for (let i = 0; i < 5; i += 1) e.push([b + i, f + i])
    n = p.length
  }
  return wire(p, e)
}

/** A warden: the patrol's dreadnought. Boxy, symmetrical, and finned. */
export function warden(): Wire {
  const p: number[][] = [
    [0, 0, 1.9],
    [-0.5, 0.28, 0.8], [0.5, 0.28, 0.8], [-0.5, -0.28, 0.8], [0.5, -0.28, 0.8],
    [-0.6, 0.3, -1.5], [0.6, 0.3, -1.5], [-0.6, -0.3, -1.5], [0.6, -0.3, -1.5],
  ]
  const e: [number, number][] = [
    [0, 1], [0, 2], [0, 3], [0, 4],
    [1, 2], [3, 4], [1, 3], [2, 4],
    [1, 5], [2, 6], [3, 7], [4, 8],
    [5, 6], [7, 8], [5, 7], [6, 8],
  ]
  bladeFin(p, e, 1.0, 0.2)
  let n = p.length
  // Boxed frames, evenly spaced. The patrol's hulls are *regular* where a raider's are not, and
  // even spacing is the cheapest way to say built as a class rather than assembled.
  for (let i = 1; i <= 6; i += 1) {
    const t = i / 7
    const z = 0.8 - t * 2.3
    p.push([-0.55, 0.29, z], [0.55, 0.29, z], [0.55, -0.29, z], [-0.55, -0.29, z])
    e.push([n, n + 1], [n + 1, n + 2], [n + 2, n + 3], [n + 3, n])
    n += 4
  }
  for (const s of [-1, 1]) {
    for (const z of [0.3, -0.4, -1.1]) {
      p.push([s * 0.56, 0.06, z + 0.14], [s * 0.72, 0.06, z], [s * 0.56, 0.06, z - 0.14], [s * 0.56, -0.1, z])
      e.push([n, n + 1], [n + 1, n + 2], [n, n + 3], [n + 2, n + 3], [n + 1, n + 3])
      n += 4
    }
  }
  for (const x of [-0.32, 0, 0.32]) {
    const b = ring(p, 6, 0.16, -1.5, 'xy')
    for (let i = 0; i < 6; i += 1) p[b + i][0] += x
    const f = ring(p, 6, 0.19, -1.9, 'xy')
    for (let i = 0; i < 6; i += 1) p[f + i][0] += x
    e.push(...loop(b, 6), ...loop(f, 6))
    for (let i = 0; i < 6; i += 1) e.push([b + i, f + i])
    n = p.length
  }
  return wire(p, e)
}

/** A bastion: the patrol's titan. The warden's geometry, carried much further. */
export function bastion(): Wire {
  const p: number[][] = [
    [0, 0, 2.2],
    [-0.72, 0.36, 0.9], [0.72, 0.36, 0.9], [-0.72, -0.36, 0.9], [0.72, -0.36, 0.9],
    [-0.9, 0.4, -1.7], [0.9, 0.4, -1.7], [-0.9, -0.4, -1.7], [0.9, -0.4, -1.7],
  ]
  const e: [number, number][] = [
    [0, 1], [0, 2], [0, 3], [0, 4],
    [1, 2], [3, 4], [1, 3], [2, 4],
    [1, 5], [2, 6], [3, 7], [4, 8],
    [5, 6], [7, 8], [5, 7], [6, 8],
  ]
  bladeFin(p, e, 1.5, 0.3)
  let n = p.length
  // A second, smaller fin aft: the class marking repeated is what a flagship of a fleet does.
  bladeFin(p, e, 0.8, -1.0)
  n = p.length
  for (let i = 1; i <= 8; i += 1) {
    const t = i / 9
    const z = 0.9 - t * 2.6
    const w = 0.72 + 0.18 * t
    const h = 0.36 + 0.04 * t
    p.push([-w, h, z], [w, h, z], [w, -h, z], [-w, -h, z])
    e.push([n, n + 1], [n + 1, n + 2], [n + 2, n + 3], [n + 3, n])
    n += 4
  }
  for (const s of [-1, 1]) {
    for (const z of [0.4, -0.3, -1.0, -1.5]) {
      p.push([s * 0.84, 0.06, z + 0.16], [s * 1.02, 0.06, z], [s * 0.84, 0.06, z - 0.16], [s * 0.84, -0.12, z])
      e.push([n, n + 1], [n + 1, n + 2], [n, n + 3], [n + 2, n + 3], [n + 1, n + 3])
      n += 4
    }
  }
  for (const x of [-0.5, -0.17, 0.17, 0.5]) {
    const b = ring(p, 6, 0.18, -1.7, 'xy')
    for (let i = 0; i < 6; i += 1) p[b + i][0] += x
    const f = ring(p, 6, 0.22, -2.15, 'xy')
    for (let i = 0; i < 6; i += 1) p[f + i][0] += x
    e.push(...loop(b, 6), ...loop(f, 6))
    for (let i = 0; i < 6; i += 1) e.push([b + i, f + i])
    n = p.length
  }
  return wire(p, e)
}
