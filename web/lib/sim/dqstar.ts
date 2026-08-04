// Deep Q*™ — Dueling Double-DQN in pure TypeScript, no ML framework.
//
// Mirrors the reference implementation in `crates/scematica-nn`:
//   STATE_DIM(24) → 128 → 64 → { V(s), A(s,a) },  He init, ReLU,
//   Q(s,a) = V(s) + A(s,a) − mean(A)
// with an online/target net pair (Double DQN: online selects, target evaluates),
// experience replay, and epsilon-greedy exploration decaying 1.0 → 0.05.
//
// This is a genuine network — forward passes and gradient steps actually run. It
// backs the self-contained web API so the dashboard demonstrates the real
// architecture without needing the Rust bot process.

export const STATE_DIM = 24
const H1 = 128
const H2 = 64

export const ACTIONS = [
  'Hold',
  'BuyStandard',
  'BuyAggressive',
  'SellPartial',
  'SellAll',
] as const
export type ActionName = (typeof ACTIONS)[number]
export const N_ACTIONS = ACTIONS.length

// ── deterministic RNG ─────────────────────────────────────────────────────────
// The whole engine is seeded so a given (seed, step) always yields the same
// result — required because serverless invocations recompute state from scratch
// rather than sharing memory.

export function mulberry32(seed: number): () => number {
  let a = seed >>> 0
  return () => {
    a = (a + 0x6d2b79f5) >>> 0
    let t = Math.imul(a ^ (a >>> 15), 1 | a)
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296
  }
}

function gaussian(rnd: () => number): number {
  let u = 0
  let v = 0
  while (u === 0) u = rnd()
  while (v === 0) v = rnd()
  return Math.sqrt(-2 * Math.log(u)) * Math.cos(2 * Math.PI * v)
}

// ── dense layer ───────────────────────────────────────────────────────────────

class Dense {
  readonly nIn: number
  readonly nOut: number
  w: Float64Array
  b: Float64Array

  constructor(nIn: number, nOut: number, rnd: () => number) {
    this.nIn = nIn
    this.nOut = nOut
    this.w = new Float64Array(nIn * nOut)
    this.b = new Float64Array(nOut)
    const std = Math.sqrt(2 / nIn) // He initialisation for ReLU
    for (let i = 0; i < this.w.length; i++) this.w[i] = gaussian(rnd) * std
  }

  forward(x: Float64Array, out: Float64Array, relu: boolean): void {
    for (let o = 0; o < this.nOut; o++) {
      const base = o * this.nIn
      let sum = this.b[o]
      for (let i = 0; i < this.nIn; i++) sum += this.w[base + i] * x[i]
      out[o] = relu && sum < 0 ? 0 : sum
    }
  }

  /** Accumulate grads and backpropagate dOut → dIn. `preAct` gates the ReLU. */
  backward(
    x: Float64Array,
    dOut: Float64Array,
    dIn: Float64Array | null,
    preAct: Float64Array | null,
    lr: number,
  ): void {
    if (dIn) dIn.fill(0)
    for (let o = 0; o < this.nOut; o++) {
      let g = dOut[o]
      if (preAct && preAct[o] <= 0) g = 0 // ReLU derivative
      if (g === 0) continue
      const base = o * this.nIn
      if (dIn) for (let i = 0; i < this.nIn; i++) dIn[i] += this.w[base + i] * g
      const step = lr * g
      for (let i = 0; i < this.nIn; i++) this.w[base + i] -= step * x[i]
      this.b[o] -= step
    }
  }

  copyFrom(other: Dense): void {
    this.w.set(other.w)
    this.b.set(other.b)
  }
}

// ── dueling network ───────────────────────────────────────────────────────────

export class DuelingNet {
  l1: Dense
  l2: Dense
  vHead: Dense
  aHead: Dense

  // scratch buffers reused across passes (single-threaded, no aliasing risk)
  private h1 = new Float64Array(H1)
  private h2 = new Float64Array(H2)
  private vOut = new Float64Array(1)
  private aOut = new Float64Array(N_ACTIONS)
  private q = new Float64Array(N_ACTIONS)

  constructor(rnd: () => number) {
    this.l1 = new Dense(STATE_DIM, H1, rnd)
    this.l2 = new Dense(H1, H2, rnd)
    this.vHead = new Dense(H2, 1, rnd)
    this.aHead = new Dense(H2, N_ACTIONS, rnd)
  }

  /** Q(s,·) = V(s) + A(s,·) − mean(A). Returns a buffer valid until the next call. */
  forward(state: Float64Array): Float64Array {
    this.l1.forward(state, this.h1, true)
    this.l2.forward(this.h1, this.h2, true)
    this.vHead.forward(this.h2, this.vOut, false)
    this.aHead.forward(this.h2, this.aOut, false)

    let meanA = 0
    for (let i = 0; i < N_ACTIONS; i++) meanA += this.aOut[i]
    meanA /= N_ACTIONS

    for (let i = 0; i < N_ACTIONS; i++) this.q[i] = this.vOut[0] + this.aOut[i] - meanA
    return this.q
  }

  /** One MSE gradient step on Q(s, action) toward `target`. Returns the loss. */
  trainStep(state: Float64Array, action: number, target: number, lr: number): number {
    const q = this.forward(state)
    const err = q[action] - target
    const loss = err * err

    // Huber-style gradient clip keeps a single outlier reward from blowing up the net.
    const dQ = Math.max(-1, Math.min(1, err))

    // Q = V + A_a − mean(A)  ⇒  dQ/dV = 1,  dQ/dA_j = [j==a] − 1/N
    const dV = new Float64Array(1)
    dV[0] = dQ
    const dA = new Float64Array(N_ACTIONS)
    for (let j = 0; j < N_ACTIONS; j++) dA[j] = ((j === action ? 1 : 0) - 1 / N_ACTIONS) * dQ

    const dH2FromV = new Float64Array(H2)
    const dH2FromA = new Float64Array(H2)
    this.vHead.backward(this.h2, dV, dH2FromV, null, lr)
    this.aHead.backward(this.h2, dA, dH2FromA, null, lr)

    const dH2 = new Float64Array(H2)
    for (let i = 0; i < H2; i++) dH2[i] = dH2FromV[i] + dH2FromA[i]

    const dH1 = new Float64Array(H1)
    // h2/h1 hold post-ReLU activations; because ReLU(x)>0 ⇔ x>0, they gate correctly.
    this.l2.backward(this.h1, dH2, dH1, this.h2, lr)
    this.l1.backward(state, dH1, null, this.h1, lr)

    return loss
  }

  copyFrom(other: DuelingNet): void {
    this.l1.copyFrom(other.l1)
    this.l2.copyFrom(other.l2)
    this.vHead.copyFrom(other.vHead)
    this.aHead.copyFrom(other.aHead)
  }
}

// ── agent ─────────────────────────────────────────────────────────────────────

export interface Transition {
  state: Float64Array
  action: number
  reward: number
  next: Float64Array
  done: boolean
}

export interface AgentHyperParams {
  epsilonDecay: number
  lr: number
  gamma: number
}

const REPLAY_CAP = 10_000
/** Small batch: a cold serverless start replays the whole session in one request. */
const BATCH = 8
const TARGET_SYNC = 200
const EPS_MIN = 0.05

/** A single Double-DQN agent: online + target nets, replay buffer, ε-greedy policy. */
export class DQStarAgent {
  online: DuelingNet
  target: DuelingNet
  epsilon = 1.0
  stepCount = 0
  trainSteps = 0
  targetUpdates = 0
  totalReward = 0
  lossSum = 0
  lossCount = 0
  lastQ: number[] = new Array(N_ACTIONS).fill(0)

  private replay: Transition[] = []
  private rnd: () => number
  private hp: AgentHyperParams

  constructor(seed: number, hp: AgentHyperParams) {
    this.rnd = mulberry32(seed)
    this.hp = hp
    this.online = new DuelingNet(this.rnd)
    this.target = new DuelingNet(mulberry32(seed))
    this.target.copyFrom(this.online)
  }

  /** ε-greedy action. Records the greedy Q-vector for dashboard display. */
  selectAction(state: Float64Array): number {
    const q = this.online.forward(state)
    this.lastQ = Array.from(q)
    if (this.rnd() < this.epsilon) return Math.floor(this.rnd() * N_ACTIONS) % N_ACTIONS
    let best = 0
    for (let i = 1; i < N_ACTIONS; i++) if (q[i] > q[best]) best = i
    return best
  }

  greedyQ(state: Float64Array): Float64Array {
    return this.online.forward(state)
  }

  /**
   * Record that the agent made a live decision. Every state it evaluates is a
   * step of the ε schedule — rewards only arrive later, when a position closes,
   * so decay cannot be tied to `observe` or it would barely move.
   */
  noteDecision(): void {
    this.stepCount++
    if (this.epsilon > EPS_MIN) this.epsilon *= this.hp.epsilonDecay
  }

  /** Record a completed transition (reward known) into the replay buffer. */
  observe(t: Transition): void {
    this.replay.push(t)
    if (this.replay.length > REPLAY_CAP) this.replay.shift()
    this.totalReward += t.reward
  }

  /** One minibatch of Double-DQN updates (online selects the arg-max, target evaluates). */
  train(): void {
    if (this.replay.length < BATCH) return
    for (let n = 0; n < BATCH; n++) {
      const t = this.replay[Math.floor(this.rnd() * this.replay.length) % this.replay.length]
      let targetQ = t.reward
      if (!t.done) {
        const onlineNext = this.online.forward(t.next)
        let argmax = 0
        for (let i = 1; i < N_ACTIONS; i++) if (onlineNext[i] > onlineNext[argmax]) argmax = i
        const targetNext = this.target.forward(t.next)
        targetQ += this.hp.gamma * targetNext[argmax]
      }
      this.lossSum += this.online.trainStep(t.state, t.action, targetQ, this.hp.lr)
      this.lossCount++
      this.trainSteps++

      if (this.trainSteps % TARGET_SYNC === 0) {
        this.target.copyFrom(this.online)
        this.targetUpdates++
      }
    }
  }

  get avgLoss(): number {
    return this.lossCount > 0 ? this.lossSum / this.lossCount : 0
  }

  get replaySize(): number {
    return this.replay.length
  }

  /** Matches `ready_to_advise` in the Rust agent: enough training + a real signal. */
  get readyToAdvise(): boolean {
    return this.trainSteps >= 10_000 && this.lastQ.some((v) => v !== 0)
  }
}
