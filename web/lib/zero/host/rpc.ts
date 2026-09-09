'use client'

// The chain connection — BYO key, browser-only.
//
// ── Why the user brings the key ──────────────────────────────────────────────
//
// `NEXT_PUBLIC_RPC_ENDPOINT` is deliberately unset in this repo, and `lib/escrow/rpc.ts`
// throws if it is ever imported into a browser bundle, because anything with that prefix
// is served to every visitor. So there is no keyed endpoint Zero can ship with, and the
// public cluster endpoint is rate-limited to the point of being unusable for a bot.
//
// The user's key is stored in `localStorage` and **never leaves the browser**: it is not
// sent to our server, not written into a decision record, and not included in an error
// string. `redact()` below is applied to every message that can reach a log or a record,
// because an endpoint URL with `?api-key=` in it is the single most likely way for a
// secret to escape a page that is otherwise careful.

import { type ZeroEvent } from '../types.ts'
import { decodeTokenAmount, priceFromVaults } from './parse.ts'

const KEY_STORAGE = 'scematica-zero-rpc'

export interface RpcConfig {
  /** Full HTTPS endpoint, key included. */
  http: string
  /** Derived from `http` unless given. */
  ws?: string
}

/**
 * Why the endpoint is validated rather than trusted.
 *
 * A field labelled "paste your key here" receives whatever is on the clipboard, and the
 * failure mode of accepting it is worse than it looks. `wsUrl` only rewrites a leading
 * `http`, so a bare string passes through unchanged — and `new WebSocket('npm_abc…')` is
 * a RELATIVE url, which the browser resolves against the page origin. The result is an
 * endless reconnect loop against `wss://your-own-site/npm_abc…`, with the pasted value
 * printed into the console on every attempt.
 *
 * That is exactly what happened in the wild, with an npm access token. Nothing left the
 * browser — that part of the design held — but the value ended up in `localStorage` and
 * in the console, and the error gave no hint that the endpoint was the problem.
 *
 * Returns the reason it is unusable, or `null` when it is fine.
 */
export function endpointProblem(raw: string): string | null {
  const value = raw.trim()
  if (!value) return 'Paste your RPC endpoint.'

  // Named before the URL check, because the generic "that is not a URL" is unhelpful next
  // to the mistake people actually make. These are prefixes for credentials that are
  // routinely one clipboard entry away from an RPC endpoint.
  if (/^(npm_|ghp_|github_pat_|gh[pousr]_|sk-|xox[baprs]-|AKIA|glpat-)/.test(value)) {
    return 'That looks like an access token, not an endpoint. Nothing was saved or sent — but rotate it, since it has been on your clipboard.'
  }

  let url: URL
  try {
    url = new URL(value)
  } catch {
    return 'That is not a URL. It needs the scheme too — https://your-provider/…'
  }
  if (url.protocol !== 'https:' && url.protocol !== 'http:') {
    return `Endpoints are http or https, not ${url.protocol.replace(':', '')}.`
  }
  if (!url.host) return 'That URL has no host.'
  return null
}

/**
 * The stored endpoint, or nothing.
 *
 * Validated on the way OUT as well as in. A value saved before this check existed — or
 * written by hand into storage — must not be able to resurrect the reconnect loop, and
 * an unusable one is dropped rather than returned.
 */
export function loadRpc(): RpcConfig | null {
  try {
    const raw = localStorage.getItem(KEY_STORAGE)
    if (!raw) return null
    const parsed = JSON.parse(raw) as RpcConfig
    if (typeof parsed?.http !== 'string' || endpointProblem(parsed.http)) {
      localStorage.removeItem(KEY_STORAGE)
      return null
    }
    return parsed
  } catch {
    return null
  }
}

/** Saves only a usable endpoint. Returns the reason it refused, or `null` on success. */
export function saveRpc(config: RpcConfig | null): string | null {
  try {
    if (config === null) {
      localStorage.removeItem(KEY_STORAGE)
      return null
    }
    const problem = endpointProblem(config.http)
    if (problem) return problem
    localStorage.setItem(KEY_STORAGE, JSON.stringify({ ...config, http: config.http.trim() }))
    return null
  } catch {
    /* private mode, or storage disabled — Zero degrades to read-only rather than throwing */
    return null
  }
}

/** `https://…` → `wss://…`. Providers serve both on the same host and key. */
export function wsUrl(config: RpcConfig): string {
  if (config.ws) return config.ws
  return config.http.replace(/^http/, 'ws')
}

/**
 * Strip anything that looks like a credential.
 *
 * Applied to every string that can reach a notice, a record or the console. The patterns
 * cover the two shapes providers actually use — a query parameter and a path segment —
 * and the host is kept, because "which endpoint failed" is the diagnostic and the key is
 * not.
 */
export function redact(text: string): string {
  return text
    .replace(/([?&](?:api[-_]?key|token|key)=)[^&\s"']+/gi, '$1REDACTED')
    .replace(/(https?:\/\/[^/\s]+\/)[A-Za-z0-9_-]{16,}/g, '$1REDACTED')
}

/** Everything the page can ask the chain for. One place, so `redact` cannot be skipped. */
export class ZeroRpc {
  private nextId = 1
  private config: RpcConfig

  // Fields assigned explicitly rather than declared as constructor parameter properties.
  // That syntax is a TYPE-DIRECTED emit, so Node's strip-only loader refuses the whole
  // module — which would make everything in this file untestable, `endpointProblem`
  // included. `lib/swap.ts` has the same construct and is why the Raydium layout had to
  // move into parse.ts; one landmine of that kind in the codebase is enough.
  constructor(config: RpcConfig) {
    this.config = config
  }

  get host(): string {
    try {
      return new URL(this.config.http).host
    } catch {
      return 'invalid endpoint'
    }
  }

  async call<T>(method: string, params: unknown[]): Promise<T> {
    const res = await fetch(this.config.http, {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ jsonrpc: '2.0', id: this.nextId++, method, params }),
    })
    if (!res.ok) throw new Error(redact(`${method} → HTTP ${res.status}`))
    const json = (await res.json()) as { result?: T; error?: { message: string } }
    if (json.error) throw new Error(redact(`${method} → ${json.error.message}`))
    return json.result as T
  }

  /** Base64 account data, or `null` when the account does not exist. */
  async accountData(address: string): Promise<Uint8Array | null> {
    const r = await this.call<{ value: { data: [string, string] } | null }>('getAccountInfo', [
      address,
      { encoding: 'base64', commitment: 'confirmed' },
    ])
    if (!r.value) return null
    return base64ToBytes(r.value.data[0])
  }

  async signatureStatus(signature: string) {
    const r = await this.call<{ value: Array<{ slot: number; confirmationStatus?: string; err: unknown } | null> }>(
      'getSignatureStatuses',
      [[signature], { searchTransactionHistory: true }],
    )
    return r.value[0] ?? null
  }

  /**
   * The transaction, for reading what a landed swap actually produced.
   *
   * `maxSupportedTransactionVersion: 0` is not optional: Jupiter routes are versioned
   * transactions, and omitting it makes the node refuse the very transactions Zero
   * needs to read — with an error about the version rather than about the parameter.
   */
  async transaction(signature: string) {
    return this.call<{
      meta: {
        fee: number
        err: unknown | null
        preBalances: number[]
        postBalances: number[]
        preTokenBalances?: unknown[]
        postTokenBalances?: unknown[]
      } | null
      transaction: { message: { accountKeys: Array<string | { pubkey: string }> } }
    } | null>('getTransaction', [
      signature,
      { encoding: 'jsonParsed', commitment: 'confirmed', maxSupportedTransactionVersion: 0 },
    ])
  }

  async lamports(address: string): Promise<number | null> {
    const r = await this.call<{ value: number }>('getBalance', [address, { commitment: 'confirmed' }])
    return typeof r?.value === 'number' ? r.value : null
  }

  async blockhash(): Promise<string> {
    const r = await this.call<{ value: { blockhash: string } }>('getLatestBlockhash', [
      { commitment: 'confirmed' },
    ])
    return r.value.blockhash
  }

  async sendRaw(base64Tx: string): Promise<string> {
    // Preflight ON. Skipping it lands failing transactions and charges for them.
    return this.call<string>('sendTransaction', [
      base64Tx,
      { encoding: 'base64', skipPreflight: false, preflightCommitment: 'confirmed', maxRetries: 3 },
    ])
  }
}

export function base64ToBytes(b64: string): Uint8Array {
  const bin = atob(b64)
  const out = new Uint8Array(bin.length)
  for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i)
  return out
}

// ── the socket ───────────────────────────────────────────────────────────────

export interface Subscription {
  /** Vault pair for one mint, so a price can be computed from an arrival. */
  mint: string
  quoteVault: string
  baseVault: string
}

/**
 * The chain feed.
 *
 * **This is the object Z-1 rests on.** Its `onmessage` handler runs in a hidden tab
 * where a `setInterval` is throttled to roughly once a minute, which is why every exit
 * predicate is evaluated from here rather than from a poll.
 *
 * Reconnection is timer-driven and that is allowed: a reconnect does not decide
 * anything, and while the socket is down `gate.ts::liveness` reports the positions as
 * NOT being evaluated rather than letting silence look like a hold.
 */
export class ZeroSocket {
  private ws: WebSocket | null = null
  private subs = new Map<string, Subscription>()
  /** subscription id → the vault address it belongs to. */
  private idToVault = new Map<number, string>()
  /** vault address → its mint and which leg it is. */
  private vaultToLeg = new Map<string, { mint: string; leg: 'quote' | 'base' }>()
  /** Last decoded balance per vault, so one arrival can be paired with its sibling. */
  private balances = new Map<string, Uint8Array>()
  private nextId = 1
  private closed = false
  private retry = 0

  private url: string
  private emit: (e: ZeroEvent) => void
  private now: () => number

  constructor(
    url: string,
    emit: (e: ZeroEvent) => void,
    now: () => number = () => Date.now() / 1000,
  ) {
    this.url = url
    this.emit = emit
    this.now = now
  }

  connect(): void {
    if (this.closed) return
    const ws = new WebSocket(this.url)
    this.ws = ws

    ws.onopen = () => {
      this.retry = 0
      this.emit({ kind: 'socket.opened', atUnix: this.now() })
      for (const s of this.subs.values()) this.sendSubscribe(s)
    }

    ws.onmessage = ev => this.onMessage(String(ev.data))

    ws.onclose = () => {
      this.emit({ kind: 'socket.closed', atUnix: this.now(), reason: 'connection closed' })
      if (this.closed) return
      // Exponential backoff, capped. A tight reconnect loop against a rate-limited
      // endpoint is how a key gets suspended.
      const delay = Math.min(30_000, 1000 * 2 ** Math.min(this.retry++, 5))
      setTimeout(() => this.connect(), delay)
    }

    ws.onerror = () => {
      /* onclose always follows; reporting both would double every notice */
    }
  }

  close(): void {
    this.closed = true
    this.ws?.close()
  }

  subscribe(sub: Subscription): void {
    this.subs.set(sub.mint, sub)
    this.vaultToLeg.set(sub.quoteVault, { mint: sub.mint, leg: 'quote' })
    this.vaultToLeg.set(sub.baseVault, { mint: sub.mint, leg: 'base' })
    if (this.ws?.readyState === WebSocket.OPEN) this.sendSubscribe(sub)
  }

  unsubscribe(mint: string): void {
    const sub = this.subs.get(mint)
    if (!sub) return
    this.subs.delete(mint)
    this.vaultToLeg.delete(sub.quoteVault)
    this.vaultToLeg.delete(sub.baseVault)
    this.balances.delete(sub.quoteVault)
    this.balances.delete(sub.baseVault)
  }

  private sendSubscribe(sub: Subscription): void {
    for (const vault of [sub.quoteVault, sub.baseVault]) {
      const id = this.nextId++
      this.idToVault.set(id, vault)
      this.ws?.send(
        JSON.stringify({
          jsonrpc: '2.0',
          id,
          method: 'accountSubscribe',
          params: [vault, { encoding: 'base64', commitment: 'confirmed' }],
        }),
      )
    }
  }

  private onMessage(raw: string): void {
    let msg: {
      id?: number
      result?: number
      method?: string
      params?: { subscription: number; result: { context: { slot: number }; value: { data: [string, string] } } }
    }
    try {
      msg = JSON.parse(raw)
    } catch {
      return
    }

    // Subscription confirmations map our request id to the server's subscription id.
    if (msg.id !== undefined && typeof msg.result === 'number') {
      const vault = this.idToVault.get(msg.id)
      if (vault) this.idToVault.set(msg.result, vault)
      return
    }

    if (msg.method !== 'accountNotification' || !msg.params) return
    const vault = this.idToVault.get(msg.params.subscription)
    if (!vault) return
    const leg = this.vaultToLeg.get(vault)
    if (!leg) return

    this.balances.set(vault, base64ToBytes(msg.params.result.value.data[0]))

    const sub = this.subs.get(leg.mint)
    if (!sub) return
    const price = priceFromVaults(
      this.decode(sub.quoteVault),
      this.decode(sub.baseVault),
    )
    // A single leg is not a price. Waiting for the sibling is correct: emitting an event
    // with a made-up denominator would put a fabricated number in front of an exit rule.
    if (price === null) return

    this.emit({
      kind: 'vault.changed',
      mint: leg.mint,
      priceSol: price,
      slot: msg.params.result.context.slot,
      atUnix: this.now(),
    })
  }

  private decode(vault: string): bigint | null {
    const data = this.balances.get(vault)
    return data ? decodeTokenAmount(data) : null
  }
}
