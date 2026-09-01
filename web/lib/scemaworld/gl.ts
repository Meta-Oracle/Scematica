/**
 * The WebGL2 layer. It places geometry and nothing else.
 *
 * No engine dependency, by the same reasoning as the hand-written rasteriser, HTTP server and
 * PNG encoder elsewhere in this project — though the argument is weaker here and worth being
 * honest about. The raster *must* be hand-written because its bytes are a derivative of the
 * record and a library would antialias differently. A frame on screen is not committed to
 * anything, so this could have used a library; it does not, because a 600 KB dependency for
 * lines and hulls is a poor trade and because keeping the surface small keeps the rules in
 * `view.ts` where they can be tested.
 *
 * **Every colour and every role decision lives in `view.ts`; every silhouette lives in
 * `meshes.ts`.** This file looks up a palette entry, picks the mesh the role names, and draws.
 * If an `if (ghost)` ever appears here, the rule has leaked out of the one place that tests it.
 *
 * ## Four passes, in this order, and the order is load-bearing
 *
 * 1. **Stars** — depth writes off, drawn first, camera translation stripped. They are the
 *    backdrop; anything else drawn before them would be painted over.
 * 2. **Lanes** — faint lines. Barely visible on purpose: they are structure, not traffic.
 * 3. **Bodies and hulls** — the solids, the shells, and the instanced wireframes.
 * 4. **Bolts** — additive, depth-test on but depth-*write* off, so two overlapping tracers sum
 *    into a brighter core instead of one occluding the other. That summing is the glow.
 */

import type { Mat4 } from './camera.ts'
import { BOLT_GLOW, BOLT_LENGTH } from './scale.ts'
import * as Mesh from './meshes.ts'
import { PALETTE, shapeOf, type Body, type DrawList, type Segment } from './view.ts'
import type { Shape } from './classes.ts'

// ── shaders ───────────────────────────────────────────────────────────────────

/**
 * Build a rotation from a facing vector, in the vertex shader.
 *
 * Shared by every instanced program. An up-vector of +Y degenerates when the facing is vertical,
 * so it falls back to +X — without the fallback a ship pointing straight up collapses to a line,
 * which happens rarely enough to ship and often enough to be reported.
 */
const BASIS = `
mat3 basisFrom(vec3 f) {
  vec3 fwd = normalize(f);
  vec3 ref = abs(fwd.y) > 0.98 ? vec3(1.0, 0.0, 0.0) : vec3(0.0, 1.0, 0.0);
  vec3 rgt = normalize(cross(ref, fwd));
  vec3 up  = cross(fwd, rgt);
  return mat3(rgt, up, fwd);
}`

const VERT_BODY = `#version 300 es
in vec3 aPos;
in vec3 aOffset;
in float aScale;
in vec3 aColor;
in float aSolid;
uniform mat4 uViewProj;
out vec3 vColor;
out float vSolid;
out vec3 vNormal;
void main() {
  vColor = aColor;
  vSolid = aSolid;
  vNormal = normalize(aPos);
  gl_Position = uViewProj * vec4(aPos * aScale + aOffset, 1.0);
}`

const FRAG_BODY = `#version 300 es
precision highp float;
in vec3 vColor;
in float vSolid;
in vec3 vNormal;
out vec4 outColor;
void main() {
  // A fixed key light. Not physical — enough shading to read a sphere as a sphere.
  float lambert = 0.35 + 0.65 * max(dot(vNormal, normalize(vec3(0.4, 0.8, 0.45))), 0.0);
  if (vSolid > 0.5) {
    outColor = vec4(vColor * lambert, 1.0);
  } else {
    // A hollow body: rim-lit shell, transparent through the middle. A ghost has to read as
    // "maybe not there" at a glance, from any angle, without relying on hue.
    float rim = 1.0 - abs(dot(vNormal, vec3(0.0, 0.0, 1.0)));
    outColor = vec4(vColor, pow(rim, 2.0) * 0.9);
  }
}`

/** Instanced wireframe hulls: one mesh, many ships, each with its own facing. */
const VERT_WIRE = `#version 300 es
in vec3 aPos;
in vec3 aOffset;
in float aScale;
in vec3 aColor;
in vec3 aFacing;
in float aFlash;
uniform mat4 uViewProj;
out vec3 vColor;
out float vFlash;
${BASIS}
void main() {
  vColor = aColor;
  vFlash = aFlash;
  vec3 p = basisFrom(aFacing) * (aPos * aScale) + aOffset;
  gl_Position = uViewProj * vec4(p, 1.0);
}`

const FRAG_WIRE = `#version 300 es
precision highp float;
in vec3 vColor;
in float vFlash;
out vec4 outColor;
void main() {
  // aFlash is a hit flash, raised toward white for a few frames after damage lands. It is the
  // whole of the game's feedback that a shot connected, so it is deliberately loud.
  vec3 c = mix(vColor, vec3(1.0), vFlash * 0.85);
  outColor = vec4(c, 0.55 + 0.45 * vFlash);
}`

/** Bolts: instanced cylinders oriented along travel, drawn additively. */
const VERT_BOLT = `#version 300 es
in vec3 aPos;
in vec3 aOffset;
in float aScale;
in vec3 aColor;
in vec3 aFacing;
in float aGlow;
uniform mat4 uViewProj;
out vec3 vColor;
out float vGlow;
out float vAlong;
${BASIS}
void main() {
  vColor = aColor;
  vGlow = aGlow;
  // 0 at the tail, 1 at the head, for the falloff below.
  vAlong = -aPos.z;
  vec3 local = vec3(aPos.xy * aScale, aPos.z * aScale * ${BOLT_LENGTH.toFixed(1)});
  gl_Position = uViewProj * vec4(basisFrom(aFacing) * local + aOffset, 1.0);
}`

const FRAG_BOLT = `#version 300 es
precision highp float;
in vec3 vColor;
in float vGlow;
in float vAlong;
out vec4 outColor;
void main() {
  // Brightest at the head, fading down the tail: a tracer, not a rod. The additive blend does
  // the rest — where the core and its halo overlap the sum clips to white, which is what reads
  // as heat without a bloom pass.
  float taper = pow(clamp(vAlong, 0.0, 1.0), 0.6);
  float a = vGlow > 0.5 ? 0.10 * taper : 0.95 * taper;
  outColor = vec4(vColor * (vGlow > 0.5 ? 0.8 : 1.6), a);
}`

const VERT_LINE = `#version 300 es
in vec3 aPos;
in vec3 aColor;
in float aAlpha;
uniform mat4 uViewProj;
out vec3 vColor;
out float vAlpha;
void main() {
  vColor = aColor;
  vAlpha = aAlpha;
  gl_Position = uViewProj * vec4(aPos, 1.0);
}`

const FRAG_LINE = `#version 300 es
precision highp float;
in vec3 vColor;
in float vAlpha;
out vec4 outColor;
void main() { outColor = vec4(vColor, vAlpha); }`

/**
 * Stars. Drawn on a unit sphere with the camera's translation removed, so they rotate with the
 * ship and never approach — a star you could fly to would be an object, and the record makes no
 * claim about one.
 */
const VERT_STAR = `#version 300 es
in vec3 aPos;
in float aMag;
uniform mat4 uViewRot;
out float vMag;
void main() {
  vMag = aMag;
  vec4 p = uViewRot * vec4(aPos, 1.0);
  // Pinned to just inside the far plane in clip space, so no depth precision is spent on them.
  gl_Position = vec4(p.xy, p.w * 0.9999, p.w);
  gl_PointSize = 1.0 + aMag * 1.6;
}`

const FRAG_STAR = `#version 300 es
precision highp float;
in float vMag;
out vec4 outColor;
void main() {
  // Round rather than square. A field of tiny squares reads as dead pixels.
  vec2 d = gl_PointCoord - vec2(0.5);
  if (dot(d, d) > 0.25) discard;
  outColor = vec4(vec3(0.72, 0.76, 0.95) * vMag, vMag);
}`

// ── plumbing ──────────────────────────────────────────────────────────────────

function compile(gl: WebGL2RenderingContext, type: number, src: string): WebGLShader {
  const s = gl.createShader(type)
  if (!s) throw new Error('could not create a shader')
  gl.shaderSource(s, src)
  gl.compileShader(s)
  if (!gl.getShaderParameter(s, gl.COMPILE_STATUS)) {
    // Surfaced rather than swallowed: a silently failed shader is a black screen, which is
    // indistinguishable from an unreadable world and would be diagnosed as one.
    throw new Error(`shader: ${gl.getShaderInfoLog(s) ?? 'unknown'}`)
  }
  return s
}

function link(gl: WebGL2RenderingContext, vs: string, fs: string): WebGLProgram {
  const p = gl.createProgram()
  if (!p) throw new Error('could not create a program')
  gl.attachShader(p, compile(gl, gl.VERTEX_SHADER, vs))
  gl.attachShader(p, compile(gl, gl.FRAGMENT_SHADER, fs))
  gl.linkProgram(p)
  if (!gl.getProgramParameter(p, gl.LINK_STATUS)) {
    throw new Error(`link: ${gl.getProgramInfoLog(p) ?? 'unknown'}`)
  }
  return p
}

export interface Renderer {
  draw(viewProj: Mat4, viewRot: Mat4, width: number, height: number): void
  upload(list: DrawList): void
  /** Build the star sphere. Once per world — it is a function of the commitment. */
  sky(seed: string): void
  dispose(): void
}

/** Instance stride in floats: offset(3) scale(1) colour(3) facing(3) extra(1). */
const INSTANCE = 11

/** Build a renderer on a canvas. Throws when WebGL2 is unavailable, rather than degrading. */
export function createRenderer(canvas: HTMLCanvasElement): Renderer {
  const ctx = canvas.getContext('webgl2', { antialias: true, alpha: false })
  if (!ctx) throw new Error('WebGL2 is not available in this browser')
  // Re-bound with a declared type. Narrowing from the null check does not survive into the
  // closures below, and `gl!` at every call site would be forty assertions instead of one.
  const gl: WebGL2RenderingContext = ctx

  const bodyProg = link(gl, VERT_BODY, FRAG_BODY)
  const wireProg = link(gl, VERT_WIRE, FRAG_WIRE)
  const boltProg = link(gl, VERT_BOLT, FRAG_BOLT)
  const lineProg = link(gl, VERT_LINE, FRAG_LINE)
  const starProg = link(gl, VERT_STAR, FRAG_STAR)

  /** One instanced draw group: a static mesh plus a per-frame instance buffer. */
  interface Group {
    vao: WebGLVertexArrayObject
    mesh: WebGLBuffer
    inst: WebGLBuffer
    verts: number
    count: number
    mode: number
  }

  function group(prog: WebGLProgram, mesh: Float32Array, mode: number, extra: string): Group {
    const vao = gl.createVertexArray()!
    gl.bindVertexArray(vao)

    const meshBuf = gl.createBuffer()!
    gl.bindBuffer(gl.ARRAY_BUFFER, meshBuf)
    gl.bufferData(gl.ARRAY_BUFFER, mesh, gl.STATIC_DRAW)
    const aPos = gl.getAttribLocation(prog, 'aPos')
    gl.enableVertexAttribArray(aPos)
    gl.vertexAttribPointer(aPos, 3, gl.FLOAT, false, 0, 0)

    const inst = gl.createBuffer()!
    gl.bindBuffer(gl.ARRAY_BUFFER, inst)
    const stride = INSTANCE * 4
    const attrs: [string, number, number][] = [
      ['aOffset', 3, 0],
      ['aScale', 1, 12],
      ['aColor', 3, 16],
      ['aFacing', 3, 28],
      [extra, 1, 40],
    ]
    for (const [name, size, offset] of attrs) {
      const loc = gl.getAttribLocation(prog, name)
      // A program that does not use an attribute reports -1. Skipping rather than throwing lets
      // the body program share this layout without carrying a facing it ignores.
      if (loc < 0) continue
      gl.enableVertexAttribArray(loc)
      gl.vertexAttribPointer(loc, size, gl.FLOAT, false, stride, offset)
      gl.vertexAttribDivisor(loc, 1)
    }
    gl.bindVertexArray(null)
    return { vao, mesh: meshBuf, inst, verts: mesh.length / 3, count: 0, mode }
  }

  const sphere = Mesh.icosphere()
  const groups: Record<Shape, Group> = {
    sphere: group(bodyProg, sphere, gl.TRIANGLES, 'aSolid'),
    shell: group(bodyProg, sphere, gl.TRIANGLES, 'aSolid'),
    interceptor: group(wireProg, Mesh.interceptor(), gl.LINES, 'aFlash'),
    gunship: group(wireProg, Mesh.gunship(), gl.LINES, 'aFlash'),
    capital: group(wireProg, Mesh.capital(), gl.LINES, 'aFlash'),
    bolt: group(boltProg, Mesh.bolt(), gl.TRIANGLES, 'aGlow'),
  }
  // The halo: the same cylinder, drawn larger and dimmer under the same additive blend.
  const glowGroup = group(boltProg, Mesh.bolt(), gl.TRIANGLES, 'aGlow')

  // Lanes.
  const lineVao = gl.createVertexArray()!
  gl.bindVertexArray(lineVao)
  const lineBuf = gl.createBuffer()!
  gl.bindBuffer(gl.ARRAY_BUFFER, lineBuf)
  for (const [name, size, off] of [['aPos', 3, 0], ['aColor', 3, 12], ['aAlpha', 1, 24]] as const) {
    const loc = gl.getAttribLocation(lineProg, name)
    gl.enableVertexAttribArray(loc)
    gl.vertexAttribPointer(loc, size, gl.FLOAT, false, 28, off)
  }
  gl.bindVertexArray(null)
  let lineVertices = 0

  // Stars.
  const starVao = gl.createVertexArray()!
  gl.bindVertexArray(starVao)
  const starBuf = gl.createBuffer()!
  gl.bindBuffer(gl.ARRAY_BUFFER, starBuf)
  for (const [name, size, off] of [['aPos', 3, 0], ['aMag', 1, 12]] as const) {
    const loc = gl.getAttribLocation(starProg, name)
    gl.enableVertexAttribArray(loc)
    gl.vertexAttribPointer(loc, size, gl.FLOAT, false, 16, off)
  }
  gl.bindVertexArray(null)
  let starCount = 0

  function sky(seed: string) {
    const stars = Mesh.starfield(seed)
    gl.bindBuffer(gl.ARRAY_BUFFER, starBuf)
    gl.bufferData(gl.ARRAY_BUFFER, stars, gl.STATIC_DRAW)
    starCount = stars.length / 4
  }

  function pack(bodies: Body[]): Float32Array {
    const data = new Float32Array(bodies.length * INSTANCE)
    bodies.forEach((b, i) => {
      const c = PALETTE[b.role]
      const o = i * INSTANCE
      data[o] = b.at.x
      data[o + 1] = b.at.y
      data[o + 2] = b.at.z
      data[o + 3] = b.radius
      data[o + 4] = c[0]
      data[o + 5] = c[1]
      data[o + 6] = c[2]
      const f = b.facing ?? { x: 0, y: 0, z: 1 }
      data[o + 7] = f.x
      data[o + 8] = f.y
      data[o + 9] = f.z
      // The last slot is `aSolid` for spheres, `aFlash` for hulls and `aGlow` for bolts. Three
      // meanings on one lane is a little tight, and it keeps the instance layout identical
      // across every group, which is what lets one `pack` serve all of them.
      data[o + 10] = b.solid ? 1 : b.flash ?? 0
      if (b.flash !== undefined) data[o + 10] = b.flash
    })
    return data
  }

  function upload(list: DrawList) {
    const buckets: Record<Shape, Body[]> = {
      sphere: [], shell: [], interceptor: [], gunship: [], capital: [], bolt: [],
    }
    for (const b of list.bodies) buckets[shapeOf(b)].push(b)

    for (const key of Object.keys(buckets) as Shape[]) {
      const g = groups[key]
      const data = pack(buckets[key])
      gl.bindBuffer(gl.ARRAY_BUFFER, g.inst)
      gl.bufferData(gl.ARRAY_BUFFER, data, gl.DYNAMIC_DRAW)
      g.count = buckets[key].length
    }

    // The halo is the bolt list again at a larger radius and flagged as glow.
    const halo = buckets.bolt.map((b) => ({ ...b, radius: b.radius * BOLT_GLOW, flash: 1 }))
    gl.bindBuffer(gl.ARRAY_BUFFER, glowGroup.inst)
    gl.bufferData(gl.ARRAY_BUFFER, pack(halo), gl.DYNAMIC_DRAW)
    glowGroup.count = halo.length

    const lines = new Float32Array(list.segments.length * 14)
    list.segments.forEach((s: Segment, i) => {
      const c = PALETTE[s.role]
      const o = i * 14
      lines.set([s.from.x, s.from.y, s.from.z, c[0], c[1], c[2], s.alpha], o)
      lines.set([s.to.x, s.to.y, s.to.z, c[0], c[1], c[2], s.alpha], o + 7)
    })
    gl.bindBuffer(gl.ARRAY_BUFFER, lineBuf)
    gl.bufferData(gl.ARRAY_BUFFER, lines, gl.DYNAMIC_DRAW)
    lineVertices = list.segments.length * 2
  }

  function drawGroup(prog: WebGLProgram, g: Group, viewProj: Mat4) {
    if (g.count === 0) return
    gl.useProgram(prog)
    gl.uniformMatrix4fv(gl.getUniformLocation(prog, 'uViewProj'), false, viewProj)
    gl.bindVertexArray(g.vao)
    gl.drawArraysInstanced(g.mode, 0, g.verts, g.count)
    gl.bindVertexArray(null)
  }

  function draw(viewProj: Mat4, viewRot: Mat4, width: number, height: number) {
    gl.viewport(0, 0, width, height)
    // Very slightly blue-black rather than pure black: a pure-black ground makes the faintest
    // stars vanish into it, and the faint ones are most of the sky.
    gl.clearColor(0.008, 0.007, 0.016, 1)
    gl.clear(gl.COLOR_BUFFER_BIT | gl.DEPTH_BUFFER_BIT)
    gl.enable(gl.BLEND)
    gl.blendFunc(gl.SRC_ALPHA, gl.ONE_MINUS_SRC_ALPHA)

    // 1. Stars, behind everything and writing no depth.
    if (starCount > 0) {
      gl.disable(gl.DEPTH_TEST)
      gl.useProgram(starProg)
      gl.uniformMatrix4fv(gl.getUniformLocation(starProg, 'uViewRot'), false, viewRot)
      gl.bindVertexArray(starVao)
      gl.drawArrays(gl.POINTS, 0, starCount)
      gl.bindVertexArray(null)
    }

    gl.enable(gl.DEPTH_TEST)
    gl.depthMask(true)

    // 2. Lanes.
    gl.useProgram(lineProg)
    gl.uniformMatrix4fv(gl.getUniformLocation(lineProg, 'uViewProj'), false, viewProj)
    gl.bindVertexArray(lineVao)
    gl.drawArrays(gl.LINES, 0, lineVertices)
    gl.bindVertexArray(null)

    // 3. Bodies, then hulls.
    drawGroup(bodyProg, groups.sphere, viewProj)
    drawGroup(wireProg, groups.interceptor, viewProj)
    drawGroup(wireProg, groups.gunship, viewProj)
    drawGroup(wireProg, groups.capital, viewProj)
    // Shells last of the depth-writing pass so their transparency blends over the solids.
    drawGroup(bodyProg, groups.shell, viewProj)

    // 4. Bolts, additive. Depth *test* on so a tracer behind a station is hidden; depth *write*
    // off so two overlapping tracers sum instead of one occluding the other — that sum is the
    // glow, and it is the whole reason this pass is separate.
    gl.blendFunc(gl.SRC_ALPHA, gl.ONE)
    gl.depthMask(false)
    drawGroup(boltProg, glowGroup, viewProj)
    drawGroup(boltProg, groups.bolt, viewProj)
    gl.depthMask(true)
    gl.blendFunc(gl.SRC_ALPHA, gl.ONE_MINUS_SRC_ALPHA)
  }

  function dispose() {
    for (const p of [bodyProg, wireProg, boltProg, lineProg, starProg]) gl.deleteProgram(p)
    for (const g of [...Object.values(groups), glowGroup]) {
      gl.deleteBuffer(g.mesh)
      gl.deleteBuffer(g.inst)
    }
    gl.deleteBuffer(lineBuf)
    gl.deleteBuffer(starBuf)
  }

  return { draw, upload, sky, dispose }
}
