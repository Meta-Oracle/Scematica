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
  const p = [
    [0, 0, 1.35], // 0 nose
    [-0.85, -0.12, -0.7], // 1 port wingtip
    [0.85, -0.12, -0.7], // 2 starboard wingtip
    [0, 0.16, -0.55], // 3 spine
    [0, -0.1, -0.45], // 4 belly
    [0, 0.62, -0.85], // 5 fin
    [-0.3, -0.05, -0.8], // 6 port engine
    [0.3, -0.05, -0.8], // 7 starboard engine
  ]
  return wire(p, [
    [0, 1], [0, 2], [0, 3], [0, 4],
    [1, 3], [2, 3], [1, 4], [2, 4],
    [1, 6], [2, 7], [6, 7],
    [3, 5], [5, 6], [5, 7],
  ])
}

/**
 * A gunship: blunter, wider, with a visible weapon boom on each flank.
 *
 * Reads as *heavy* because it is short and broad where the interceptor is long and thin, which
 * is a silhouette difference rather than a size difference — the two are distinguishable even
 * when one is far away and the other is close.
 */
export function gunship(): Wire {
  const p = [
    [0, 0, 1.0], // 0 prow
    [-0.55, 0.3, 0.2], // 1
    [0.55, 0.3, 0.2], // 2
    [-0.55, -0.3, 0.2], // 3
    [0.55, -0.3, 0.2], // 4
    [-0.5, 0.25, -0.9], // 5
    [0.5, 0.25, -0.9], // 6
    [-0.5, -0.25, -0.9], // 7
    [0.5, -0.25, -0.9], // 8
    [-1.0, 0, 0.55], // 9 port boom
    [1.0, 0, 0.55], // 10 starboard boom
    [-1.0, 0, -0.5], // 11
    [1.0, 0, -0.5], // 12
  ]
  return wire(p, [
    [0, 1], [0, 2], [0, 3], [0, 4],
    [1, 2], [3, 4], [1, 3], [2, 4],
    [1, 5], [2, 6], [3, 7], [4, 8],
    [5, 6], [7, 8], [5, 7], [6, 8],
    [9, 10], [9, 11], [10, 12], [9, 1], [10, 2], [11, 5], [12, 6],
  ])
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
