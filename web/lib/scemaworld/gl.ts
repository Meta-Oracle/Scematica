/**
 * The WebGL2 layer. It places geometry and nothing else.
 *
 * No engine dependency, by the same reasoning as the hand-written rasteriser, HTTP server and
 * PNG encoder elsewhere in this project — though the argument is weaker here and worth being
 * honest about. The raster *must* be hand-written because its bytes are a derivative of the
 * record and a library would antialias differently. A frame on screen is not committed to
 * anything, so this could have used a library; it does not, because a 600 KB dependency for
 * lines and spheres is a poor trade and because keeping the surface small keeps the rules in
 * `view.ts` where they can be tested.
 *
 * **Every colour and every role decision lives in `view.ts`.** This file looks up a palette
 * entry and draws. If a `if (ghost)` ever appears here, the rule has leaked out of the one
 * place that tests it.
 */

import type { Mat4 } from './camera.ts'
import { PALETTE, type Body, type DrawList, type Segment } from './view.ts'

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

const VERT_LINE = `#version 300 es
in vec3 aPos;
in vec3 aColor;
uniform mat4 uViewProj;
out vec3 vColor;
void main() {
  vColor = aColor;
  gl_Position = uViewProj * vec4(aPos, 1.0);
}`

const FRAG_LINE = `#version 300 es
precision highp float;
in vec3 vColor;
out vec4 outColor;
void main() { outColor = vec4(vColor, 0.55); }`

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

/** A unit icosphere, subdivided once. Cheap, and round enough at the sizes drawn here. */
function icosphere(): Float32Array {
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

export interface Renderer {
  draw(viewProj: Mat4, width: number, height: number): void
  upload(list: DrawList): void
  dispose(): void
}

/** Build a renderer on a canvas. Throws when WebGL2 is unavailable, rather than degrading. */
export function createRenderer(canvas: HTMLCanvasElement): Renderer {
  const ctx = canvas.getContext('webgl2', { antialias: true, alpha: false })
  if (!ctx) throw new Error('WebGL2 is not available in this browser')
  // Re-bound with a declared type. Narrowing from the null check does not survive into the
  // closures below, and `gl!` at every call site would be forty assertions instead of one.
  const gl: WebGL2RenderingContext = ctx

  const bodyProg = link(gl, VERT_BODY, FRAG_BODY)
  const lineProg = link(gl, VERT_LINE, FRAG_LINE)

  const sphere = icosphere()
  const bodyVao = gl.createVertexArray()!
  gl.bindVertexArray(bodyVao)

  const meshBuf = gl.createBuffer()!
  gl.bindBuffer(gl.ARRAY_BUFFER, meshBuf)
  gl.bufferData(gl.ARRAY_BUFFER, sphere, gl.STATIC_DRAW)
  const aPos = gl.getAttribLocation(bodyProg, 'aPos')
  gl.enableVertexAttribArray(aPos)
  gl.vertexAttribPointer(aPos, 3, gl.FLOAT, false, 0, 0)

  // Instances: offset, scale, colour, solid.
  const instBuf = gl.createBuffer()!
  gl.bindBuffer(gl.ARRAY_BUFFER, instBuf)
  const stride = 8 * 4
  const attrs: [string, number, number][] = [
    ['aOffset', 3, 0],
    ['aScale', 1, 12],
    ['aColor', 3, 16],
    ['aSolid', 1, 28],
  ]
  for (const [name, size, offset] of attrs) {
    const loc = gl.getAttribLocation(bodyProg, name)
    gl.enableVertexAttribArray(loc)
    gl.vertexAttribPointer(loc, size, gl.FLOAT, false, stride, offset)
    gl.vertexAttribDivisor(loc, 1)
  }
  gl.bindVertexArray(null)

  const lineVao = gl.createVertexArray()!
  gl.bindVertexArray(lineVao)
  const lineBuf = gl.createBuffer()!
  gl.bindBuffer(gl.ARRAY_BUFFER, lineBuf)
  const lPos = gl.getAttribLocation(lineProg, 'aPos')
  const lCol = gl.getAttribLocation(lineProg, 'aColor')
  gl.enableVertexAttribArray(lPos)
  gl.vertexAttribPointer(lPos, 3, gl.FLOAT, false, 24, 0)
  gl.enableVertexAttribArray(lCol)
  gl.vertexAttribPointer(lCol, 3, gl.FLOAT, false, 24, 12)
  gl.bindVertexArray(null)

  let instanceCount = 0
  let lineVertices = 0

  function upload(list: DrawList) {
    const solid: Body[] = []
    const hollow: Body[] = []
    for (const b of list.bodies) (b.solid ? solid : hollow).push(b)
    // Hollow last so the transparent shells blend over the solids behind them.
    const ordered = [...solid, ...hollow]

    const data = new Float32Array(ordered.length * 8)
    ordered.forEach((b, i) => {
      const c = PALETTE[b.role]
      const o = i * 8
      data[o] = b.at.x
      data[o + 1] = b.at.y
      data[o + 2] = b.at.z
      data[o + 3] = b.radius
      data[o + 4] = c[0]
      data[o + 5] = c[1]
      data[o + 6] = c[2]
      data[o + 7] = b.solid ? 1 : 0
    })
    gl.bindBuffer(gl.ARRAY_BUFFER, instBuf)
    gl.bufferData(gl.ARRAY_BUFFER, data, gl.DYNAMIC_DRAW)
    instanceCount = ordered.length

    const lines = new Float32Array(list.segments.length * 12)
    list.segments.forEach((s: Segment, i) => {
      const c = PALETTE[s.role]
      const o = i * 12
      lines.set([s.from.x, s.from.y, s.from.z, c[0], c[1], c[2]], o)
      lines.set([s.to.x, s.to.y, s.to.z, c[0], c[1], c[2]], o + 6)
    })
    gl.bindBuffer(gl.ARRAY_BUFFER, lineBuf)
    gl.bufferData(gl.ARRAY_BUFFER, lines, gl.DYNAMIC_DRAW)
    lineVertices = list.segments.length * 2
  }

  function draw(viewProj: Mat4, width: number, height: number) {
    gl.viewport(0, 0, width, height)
    gl.clearColor(0.02, 0.015, 0.04, 1)
    gl.clear(gl.COLOR_BUFFER_BIT | gl.DEPTH_BUFFER_BIT)
    gl.enable(gl.DEPTH_TEST)
    gl.enable(gl.BLEND)
    gl.blendFunc(gl.SRC_ALPHA, gl.ONE_MINUS_SRC_ALPHA)

    gl.useProgram(lineProg)
    gl.uniformMatrix4fv(gl.getUniformLocation(lineProg, 'uViewProj'), false, viewProj)
    gl.bindVertexArray(lineVao)
    gl.drawArrays(gl.LINES, 0, lineVertices)

    gl.useProgram(bodyProg)
    gl.uniformMatrix4fv(gl.getUniformLocation(bodyProg, 'uViewProj'), false, viewProj)
    gl.bindVertexArray(bodyVao)
    gl.drawArraysInstanced(gl.TRIANGLES, 0, sphere.length / 3, instanceCount)
    gl.bindVertexArray(null)
  }

  function dispose() {
    gl.deleteProgram(bodyProg)
    gl.deleteProgram(lineProg)
    gl.deleteBuffer(meshBuf)
    gl.deleteBuffer(instBuf)
    gl.deleteBuffer(lineBuf)
  }

  return { draw, upload, dispose }
}
