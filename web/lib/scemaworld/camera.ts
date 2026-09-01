/**
 * Six-degrees-of-freedom flight, as pure arithmetic.
 *
 * Separate from the WebGL layer on purpose, and the reason is the same one that put
 * `lib/mesh/view.ts::toneFor` in its own file: the part with a rule in it should be testable
 * without the part that needs a GPU. Everything here runs in Node, and `check:scemaworld`
 * pins it. `gl.ts` consumes matrices and never computes one.
 *
 * ## Quaternions, not Euler angles
 *
 * A space game has no up. Euler angles give you gimbal lock the first time a player pitches
 * through vertical, and the failure is a ship that suddenly cannot roll — reported as "the
 * controls broke" and very hard to find afterwards. Orientation is a unit quaternion and
 * every rotation is applied in the ship's own frame, so pitch after a roll does what the
 * player's hands expect.
 *
 * Floats are fine here, unlike in the generator. The camera is not committed to, not hashed
 * and not shared: two machines rendering the same space from slightly different viewpoints
 * are still in the same space. The *map* must be exact; the eye looking at it need not be.
 */

export type Vec3 = readonly [number, number, number]
export type Quat = readonly [number, number, number, number] // x, y, z, w
/** Column-major 4x4, the layout WebGL expects. */
export type Mat4 = Float32Array

export interface Camera {
  position: Vec3
  orientation: Quat
}

export const IDENTITY: Quat = [0, 0, 0, 1]

export function camera(position: Vec3 = [0, 0, 0], orientation: Quat = IDENTITY): Camera {
  return { position, orientation }
}

// ── quaternion ────────────────────────────────────────────────────────────────

export function qMul(a: Quat, b: Quat): Quat {
  const [ax, ay, az, aw] = a
  const [bx, by, bz, bw] = b
  return [
    aw * bx + ax * bw + ay * bz - az * by,
    aw * by - ax * bz + ay * bw + az * bx,
    aw * bz + ax * by - ay * bx + az * bw,
    aw * bw - ax * bx - ay * by - az * bz,
  ]
}

/**
 * Renormalise.
 *
 * Called after every rotation rather than occasionally. Quaternion drift is slow and then
 * catastrophic: a few thousand frames of accumulated error turns rotation into a shear, and
 * the symptom is geometry that stretches rather than anything obviously wrong with the maths.
 */
export function qNorm(q: Quat): Quat {
  const [x, y, z, w] = q
  const l = Math.hypot(x, y, z, w)
  if (l === 0) return IDENTITY
  return [x / l, y / l, z / l, w / l]
}

/** A rotation of `angle` radians about a unit axis. */
export function qAxis(axis: Vec3, angle: number): Quat {
  const h = angle / 2
  const s = Math.sin(h)
  return qNorm([axis[0] * s, axis[1] * s, axis[2] * s, Math.cos(h)])
}

/** Rotate a vector by a quaternion. */
export function qRotate(q: Quat, v: Vec3): Vec3 {
  const [x, y, z, w] = q
  const [vx, vy, vz] = v
  // t = 2 * (q.xyz × v)
  const tx = 2 * (y * vz - z * vy)
  const ty = 2 * (z * vx - x * vz)
  const tz = 2 * (x * vy - y * vx)
  return [
    vx + w * tx + (y * tz - z * ty),
    vy + w * ty + (z * tx - x * tz),
    vz + w * tz + (x * ty - y * tx),
  ]
}

/** The ship's own axes, in world space. */
export function forward(c: Camera): Vec3 {
  return qRotate(c.orientation, [0, 0, -1])
}
export function right(c: Camera): Vec3 {
  return qRotate(c.orientation, [1, 0, 0])
}
export function up(c: Camera): Vec3 {
  return qRotate(c.orientation, [0, 1, 0])
}

// ── control ───────────────────────────────────────────────────────────────────

/**
 * Rotate in the ship's frame: pitch about its right, yaw about its up, roll about its nose.
 *
 * Composed as `orientation * delta` rather than `delta * orientation`, which is what makes it
 * a local rotation. The other order rotates about world axes and produces the "my controls
 * are inverted when upside down" complaint.
 */
export function rotate(c: Camera, pitch: number, yaw: number, roll: number): Camera {
  let q = c.orientation
  if (pitch) q = qMul(q, qAxis([1, 0, 0], pitch))
  if (yaw) q = qMul(q, qAxis([0, 1, 0], yaw))
  if (roll) q = qMul(q, qAxis([0, 0, 1], roll))
  return { position: c.position, orientation: qNorm(q) }
}

/** Translate along the ship's own axes. `[strafe, lift, thrust]`, thrust negative-Z forward. */
export function translate(c: Camera, local: Vec3): Camera {
  const d = qRotate(c.orientation, local)
  return {
    position: [c.position[0] + d[0], c.position[1] + d[1], c.position[2] + d[2]],
    orientation: c.orientation,
  }
}

// ── matrices ──────────────────────────────────────────────────────────────────

export function perspective(fovY: number, aspect: number, near: number, far: number): Mat4 {
  const f = 1 / Math.tan(fovY / 2)
  const nf = 1 / (near - far)
  const m = new Float32Array(16)
  m[0] = f / aspect
  m[5] = f
  m[10] = (far + near) * nf
  m[11] = -1
  m[14] = 2 * far * near * nf
  return m
}

/** View matrix: the inverse of the camera's transform. */
export function view(c: Camera): Mat4 {
  const [x, y, z, w] = c.orientation
  // Conjugate — the inverse rotation, since the quaternion is unit length.
  const [cx, cy, cz, cw] = [-x, -y, -z, w]
  const m = new Float32Array(16)

  const x2 = cx + cx
  const y2 = cy + cy
  const z2 = cz + cz
  const xx = cx * x2
  const xy = cx * y2
  const xz = cx * z2
  const yy = cy * y2
  const yz = cy * z2
  const zz = cz * z2
  const wx = cw * x2
  const wy = cw * y2
  const wz = cw * z2

  m[0] = 1 - (yy + zz)
  m[1] = xy + wz
  m[2] = xz - wy
  m[4] = xy - wz
  m[5] = 1 - (xx + zz)
  m[6] = yz + wx
  m[8] = xz + wy
  m[9] = yz - wx
  m[10] = 1 - (xx + yy)
  m[15] = 1

  // Then translate by the inverted position, in the rotated frame.
  const p = c.position
  m[12] = -(m[0] * p[0] + m[4] * p[1] + m[8] * p[2])
  m[13] = -(m[1] * p[0] + m[5] * p[1] + m[9] * p[2])
  m[14] = -(m[2] * p[0] + m[6] * p[1] + m[10] * p[2])
  return m
}

/**
 * The view matrix with the translation stripped: rotation only.
 *
 * For the starfield. Stars are drawn on a unit sphere around the eye and must never parallax —
 * a star you could fly toward would be an *object*, and the record makes no claim about one.
 * Reusing `view` and zeroing the translation afterwards would be a second place that has to
 * agree with the first about which three entries carry it.
 */
export function viewRotation(c: Camera): Mat4 {
  const m = view(c)
  const r = new Float32Array(m)
  r[12] = 0
  r[13] = 0
  r[14] = 0
  return r
}

/** `a * b`, column-major. */
export function mul(a: Mat4, b: Mat4): Mat4 {
  const o = new Float32Array(16)
  for (let c = 0; c < 4; c += 1) {
    for (let r = 0; r < 4; r += 1) {
      o[c * 4 + r] =
        a[r] * b[c * 4] +
        a[4 + r] * b[c * 4 + 1] +
        a[8 + r] * b[c * 4 + 2] +
        a[12 + r] * b[c * 4 + 3]
    }
  }
  return o
}
