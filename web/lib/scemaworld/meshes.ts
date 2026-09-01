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
