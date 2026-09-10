'use client'

// The runtime — where the pure core meets a browser.
//
// Everything impure lives here: the socket, the wallet, storage, the swap. The reducer
// in `engine.ts` decides; this executes and feeds the results back in as events. Keeping
// the split absolute is what makes phase 3's extension a new host for the same core
// rather than a second implementation of the loop.
//
// ── The one timer in Zero, and why it is allowed ─────────────────────────────
//
// `heartbeat` emits `tick`. Z-1 permits it precisely because `step()` ignores it: the
// reducer produces no `swap` on a tick in any state, and `check:zero` proves that with
// 2000 of them. Its job is to re-render the readout so a stale feed becomes visible —
// which is the opposite of a timer deciding money. It is the mechanism by which the
// operator finds out that the timers CANNOT be trusted.

import { type Effect, type ZeroConfig, type ZeroEvent } from '../types.ts'
import { type SessionCaps } from '../session.ts'
import { type ZeroState, initialState, step } from '../engine.ts'

/**
 * The host's clock, in one place.
 *
 * The pure core never reads a clock — every time value arrives on an event — so this is
 * the seam where a real one enters. Seconds, not millis: every `atUnix` in `ZeroEvent` is
 * unix seconds, and a millisecond value in one of them makes the coherence window roll a
 * thousand times too eagerly and the no-pump timeout unreachable.
 */
const nowUnix = () => Math.floor(Date.now() / 1000)
import { acquireLease, type Lease } from '../lease.ts'
import { ZeroRpc, ZeroSocket, type RpcConfig, type Subscription, redact, wsUrl } from './rpc.ts'
import { interpret, pollPlan, BLOCKHASH_VALID_SECS, unknownNotice } from '../observe.ts'
import { type Signer } from './signer.ts'
import { fillAmount, type AccountKey, type TxMeta } from './fills.ts'

const STATE_STORAGE = 'scematica-zero-state'
/** Bump when the persisted shape changes. An old blob is DISCARDED, never coerced. */
export const STATE_VERSION = 1

export interface RuntimeHooks {
  /** Called after every event with the new state, so React can render it. */
  onState: (state: ZeroState) => void
  /** Build and submit a swap; resolves to a signature. Supplied by the page. */
  submitSwap: (
    e: Extract<Effect, { kind: 'swap' }>,
    signer: Signer,
  ) => Promise<{ signature: string; outAmount: number | null }>
  /** Where a mint's two vaults live. `null` means Zero cannot watch it. */
  resolveVaults: (mint: string) => Promise<Subscription | null>
  signerFor: (kind: 'wallet' | 'session') => Signer | null
}

export class ZeroRuntime {
  state: ZeroState
  private socket: ZeroSocket | null = null
  private lease: Lease | null = null
  private heartbeat: ReturnType<typeof setInterval> | null = null
  private rpc: ZeroRpc | null = null

  constructor(
    config: ZeroConfig,
    caps: SessionCaps,
    private hooks: RuntimeHooks,
    private now: () => number = () => Date.now() / 1000,
  ) {
    this.state = restore(config, caps)
  }

  start(rpcConfig: RpcConfig | null): void {
    // The election first: a follower must never open a second write path.
    this.lease = acquireLease(
      () => this.dispatch({ kind: 'lease.acquired', atUnix: this.now() }),
      () => this.dispatch({ kind: 'lease.lost', atUnix: this.now() }),
    )
    if (this.lease.state === 'unsupported') {
      this.dispatch({ kind: 'disarm', atUnix: this.now(), reason: 'no Web Locks API — cannot guarantee a single writer' })
    }

    if (rpcConfig) {
      this.rpc = new ZeroRpc(rpcConfig)
      this.socket = new ZeroSocket(wsUrl(rpcConfig), e => this.dispatch(e), this.now)
      this.socket.connect()
      // Re-watch everything already held, or a reload leaves open positions unwatched —
      // which is the silent-failure case this whole design exists to prevent.
      for (const mint of Object.keys(this.state.positions)) void this.watch(mint)
    }

    this.heartbeat = setInterval(() => this.dispatch({ kind: 'tick', atUnix: this.now() }), 5000)
  }

  stop(): void {
    this.socket?.close()
    this.lease?.release()
    if (this.heartbeat) clearInterval(this.heartbeat)
  }

  dispatch(event: ZeroEvent): void {
    const { state, effects } = step(this.state, event)
    this.state = state
    this.hooks.onState(state)
    for (const effect of effects) void this.perform(effect)
  }

  private async perform(effect: Effect): Promise<void> {
    switch (effect.kind) {
      case 'subscribe':
        await this.watch(effect.mint)
        return

      case 'unsubscribe':
        this.socket?.unsubscribe(effect.mint)
        return

      case 'persist':
        persist(this.state)
        return

      case 'seal':
        // Records are held in state and persisted with it. Sealing is async and its
        // commitment is computed lazily by the panel that displays it, so a slow digest
        // can never delay a decision.
        return

      case 'notify':
        return

      case 'swap':
        await this.swap(effect)
        return
    }
  }

  private async watch(mint: string): Promise<void> {
    if (!this.socket) return
    try {
      const sub = await this.hooks.resolveVaults(mint)
      if (sub) {
        this.socket.subscribe(sub)
        this.dispatch({ kind: 'read.resolved', label: `vaults:${mint}`, atUnix: nowUnix() })
      } else {
        // Zero cannot price a position it cannot watch. That is a coherence failure, and
        // saying so is what stops it being counted as a healthy read.
        this.dispatch({ kind: 'read.failed', label: `vaults:${mint}`, reason: 'vaults not found', atUnix: nowUnix() })
      }
    } catch (e) {
      this.dispatch({
        kind: 'read.failed',
        label: `vaults:${mint}`,
        reason: redact(e instanceof Error ? e.message : String(e)),
        atUnix: nowUnix(),
      })
    }
  }

  private async swap(effect: Extract<Effect, { kind: 'swap' }>): Promise<void> {
    const signer = this.hooks.signerFor(effect.signer)
    if (!signer) {
      this.dispatch({ kind: 'fill.failed', mint: effect.mint, signature: '', reason: 'no signer available' })
      return
    }

    let signature: string
    try {
      const sent = await this.hooks.submitSwap(effect, signer)
      signature = sent.signature
    } catch (e) {
      // A throw from the SEND means nothing was submitted, so the reservation is released.
      // This is the one case where we have positive evidence that no money moved.
      this.dispatch({
        kind: 'fill.failed',
        mint: effect.mint,
        signature: '',
        reason: redact(e instanceof Error ? e.message : String(e)),
      })
      return
    }

    await this.observe(effect.mint, signature, effect.side, signer.publicKey)
  }

  /**
   * Poll the signature to its conclusion — landed, failed, or unknown.
   *
   * NEVER `sendAndConfirmTransaction`: it waits on a `signatureSubscribe`, and the
   * treasury path already paid for a promise that never settles on a transaction that
   * already finalized. A successful trade presenting as a dead bot is the worst pair of
   * facts available, and the obvious retry pays twice.
   */
  private async observe(
    mint: string,
    signature: string,
    side: 'buy' | 'sell',
    owner: string,
  ): Promise<void> {
    const started = this.now()
    for (let attempt = 0; ; attempt++) {
      const elapsed = this.now() - started
      const plan = pollPlan(attempt, elapsed)
      if (!plan.keepPolling) break
      await sleep(plan.delayMs)

      let status = null
      try {
        status = (await this.rpc?.signatureStatus(signature)) ?? null
      } catch {
        // A failed status READ is not a failed transaction. Keep asking.
        continue
      }

      const obs = interpret(signature, status as never, this.now() - started, BLOCKHASH_VALID_SECS)
      if (obs.outcome === 'landed') {
        const outAmount = await this.readFill(signature, mint, side, owner)
        this.dispatch({ kind: 'fill.observed', mint, signature, outAmount, side, atUnix: this.now() })
        return
      }
      if (obs.outcome === 'failed') {
        this.dispatch({ kind: 'fill.failed', mint, signature, reason: obs.reason })
        return
      }
    }

    // Fell out of the loop: submitted, never observed. A third outcome, not an error.
    this.dispatch({ kind: 'fill.unknown', mint, signature, atUnix: this.now() })
    this.dispatch({
      kind: 'disarm',
      atUnix: this.now(),
      reason: unknownNotice(signature, 'status never resolved'),
    })
  }

  /**
   * What the landed swap actually produced.
   *
   * Read from the transaction rather than from the quote, and rather than from a balance
   * delta: a delta taken around one signature absorbs anything else that landed in the
   * meantime, and Zero can hold several positions at once. `preTokenBalances` /
   * `postTokenBalances` attribute the movement to THIS signature.
   *
   * `null` on any failure and it stays `null` — the engine turns that into an unpriceable
   * position rather than an entry price of zero. Retrying the read is safe (it moves
   * nothing), so it is retried once: the transaction may not have propagated to this
   * endpoint yet, and giving up early strands a position that was perfectly readable a
   * second later.
   */
  private async readFill(
    signature: string,
    mint: string,
    side: 'buy' | 'sell',
    owner: string,
  ): Promise<number | null> {
    for (let attempt = 0; attempt < 3; attempt++) {
      if (attempt > 0) await sleep(1200)
      try {
        const tx = await this.rpc?.transaction(signature)
        if (!tx?.meta) continue
        const amount = fillAmount(
          tx.meta as unknown as TxMeta,
          tx.transaction.message.accountKeys as AccountKey[],
          owner,
          mint,
          side,
        )
        if (amount !== null) return amount
      } catch {
        // A failed READ is not a failed fill. Keep trying, then admit ignorance.
      }
    }
    return null
  }
}

const sleep = (ms: number) => new Promise<void>(r => setTimeout(r, ms))

// ── persistence ──────────────────────────────────────────────────────────────

interface Persisted {
  version: number
  positions: ZeroState['positions']
  ledger: ZeroState['ledger']
  outcomes: ZeroState['outcomes']
  holds: ZeroState['holds']
  records: ZeroState['records']
}

function persist(state: ZeroState): void {
  try {
    const blob: Persisted = {
      version: STATE_VERSION,
      positions: state.positions,
      ledger: state.ledger,
      outcomes: state.outcomes,
      holds: state.holds,
      records: state.records.slice(-200),
    }
    localStorage.setItem(STATE_STORAGE, JSON.stringify(blob))
  } catch {
    /* storage full or disabled; the session continues in memory */
  }
}

/**
 * Restore what is OWNED, rebuild what is STATE.
 *
 * Positions, the spend ledger and the settled outcomes are restored: they are claims
 * about money and about what has already been authorised, and losing them would let a
 * reload hand back budget that has been spent.
 *
 * `armed` is deliberately NOT restored. A page that comes back trading because it was
 * trading when the tab closed is a bot nobody chose to start — arming is a decision, and
 * a decision made an hour ago in a tab that has since been closed is not consent now.
 */
function restore(config: ZeroConfig, caps: SessionCaps): ZeroState {
  const fresh = initialState(config, caps, Math.floor(Date.now() / 1000))
  try {
    const raw = localStorage.getItem(STATE_STORAGE)
    if (!raw) return fresh
    const blob = JSON.parse(raw) as Persisted
    // A blob from another version is DISCARDED, never coerced. Coercing a ledger whose
    // shape has changed is how a spend cap silently reads the wrong field.
    if (blob.version !== STATE_VERSION) return fresh
    return {
      ...fresh,
      positions: blob.positions ?? {},
      ledger: blob.ledger ?? fresh.ledger,
      outcomes: blob.outcomes ?? [],
      holds: blob.holds ?? {},
      records: blob.records ?? [],
    }
  } catch {
    return fresh
  }
}
