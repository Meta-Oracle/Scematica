// Server-side JSON-RPC client — a port of `alchem_link/rpc.py` on `fetch`.
//
// Differences from the Python client, both forced by the setting rather than taste:
//
//   * Calls are **batched** into one HTTP request. The CLI reads six feeds with eighteen
//     sequential round trips and nobody minds; a web panel polling on a timer would burn
//     a public endpoint's rate limit doing the same. A provider that rejects batch gets
//     detected and falls back to individual calls.
//   * `decimals()` and `description()` are cached per aggregator. They are immutable on
//     the contract, so re-reading them every poll buys nothing. Only `latestRoundData()`
//     is actually re-fetched.

import {
  SELECTOR_DECIMALS,
  SELECTOR_DESCRIPTION,
  SELECTOR_LATEST_ROUND_DATA,
} from './abi'
import { redact, type Endpoint } from './endpoint'

export const DEFAULT_TIMEOUT_MS = 12_000
export const DEFAULT_RETRIES = 2

/** The node answered, and the answer was an error. */
export class RpcError extends Error {
  constructor(message: string) {
    super(message)
    this.name = 'RpcError'
  }
}

/** The node could not be reached, or did not answer in time. */
export class RpcTransportError extends Error {
  readonly retryable: boolean
  constructor(message: string, retryable = true) {
    super(message)
    this.name = 'RpcTransportError'
    this.retryable = retryable
  }
}

interface RpcRequest {
  method: string
  params?: unknown[]
}

interface RpcEnvelope {
  id: number
  result?: unknown
  error?: { code?: number; message?: string }
}

/** One call's outcome inside a batch: a result, or the reason that single call failed. */
export type Settled =
  | { ok: true; result: unknown }
  | { ok: false; error: string }

/** What a successful aggregator read yields — three raw hex payloads. */
export interface AggregatorRaw {
  latestRoundData: string
  decimals: string
  description: string
}

const sleep = (ms: number) => new Promise(resolve => setTimeout(resolve, ms))

/**
 * Aggregator constants, cached for the lifetime of the server process.
 *
 * Keyed by `chainId:address` rather than by network name, so two network entries that
 * happen to point at the same chain cannot cross-contaminate each other's cache.
 */
const constantsCache = new Map<string, { decimals: string; description: string }>()

export class RpcClient {
  readonly endpoint: Endpoint
  private readonly timeoutMs: number
  private readonly retries: number
  private requestId = 0

  constructor(endpoint: Endpoint, timeoutMs = DEFAULT_TIMEOUT_MS, retries = DEFAULT_RETRIES) {
    this.endpoint = endpoint
    this.timeoutMs = timeoutMs
    this.retries = Math.max(0, retries)
  }

  // ── transport ──────────────────────────────────────────────────────────────

  /** Exactly one HTTP round trip. Retry policy lives in `post`, not here. */
  private async attempt(body: string): Promise<unknown> {
    const controller = new AbortController()
    const timer = setTimeout(() => controller.abort(), this.timeoutMs)
    try {
      const response = await fetch(this.endpoint.url, {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json',
          Accept: 'application/json',
          // Several public providers 403 an unrecognised agent outright, which reads
          // as "the chain is down" if you have not hit it before.
          'User-Agent': 'alchem-link-web',
        },
        body,
        signal: controller.signal,
        cache: 'no-store',
      })

      if (!response.ok) {
        const detail = (await response.text().catch(() => '')).slice(0, 200)
        // 4xx will not become 2xx on a retry; fail fast and say why. 429 is the
        // exception — backing off is exactly the right response there.
        const fatal = response.status >= 400 && response.status < 500 && response.status !== 429
        throw new RpcTransportError(
          `HTTP ${response.status} from ${redact(this.endpoint.url)}${detail ? `: ${detail}` : ''}`,
          !fatal,
        )
      }

      const text = await response.text()
      try {
        return JSON.parse(text)
      } catch {
        throw new RpcTransportError(`${redact(this.endpoint.url)} returned a non-JSON body`, false)
      }
    } catch (err) {
      if (err instanceof RpcTransportError) throw err
      const reason = err instanceof Error && err.name === 'AbortError'
        ? `timed out after ${this.timeoutMs}ms`
        : err instanceof Error ? err.message : String(err)
      throw new RpcTransportError(`${redact(this.endpoint.url)}: ${reason}`)
    } finally {
      clearTimeout(timer)
    }
  }

  private async post(body: string): Promise<unknown> {
    let lastError: unknown = null
    for (let attempt = 0; attempt <= this.retries; attempt++) {
      try {
        return await this.attempt(body)
      } catch (err) {
        if (err instanceof RpcTransportError && !err.retryable) throw err
        lastError = err
        if (attempt < this.retries) await sleep(250 * (attempt + 1))
      }
    }
    const detail = lastError instanceof Error ? lastError.message : String(lastError)
    throw new RpcTransportError(
      `could not reach ${redact(this.endpoint.url)} after ${this.retries + 1} attempt(s): ${detail}`,
    )
  }

  private static unwrap(envelope: RpcEnvelope, method: string): unknown {
    if (envelope?.error) {
      const { code, message } = envelope.error
      throw new RpcError(
        `${method} failed${code !== undefined ? ` [${code}]` : ''}: ${message ?? 'unknown error'}`,
      )
    }
    if (!envelope || !('result' in envelope)) {
      throw new RpcError(`${method} returned no result field`)
    }
    return envelope.result
  }

  async call(method: string, params: unknown[] = []): Promise<unknown> {
    const id = ++this.requestId
    const body = JSON.stringify({ jsonrpc: '2.0', id, method, params })
    return RpcClient.unwrap((await this.post(body)) as RpcEnvelope, method)
  }

  /**
   * Send several calls as one JSON-RPC batch, preserving request order in the result.
   *
   * Results are **settled, not thrown**. One aggregator reverting must not blank the
   * whole board — the Python reader skips a failing feed and returns the rest, and this
   * keeps that property. A transport failure still throws, because that one really did
   * take out every call in the batch.
   *
   * Responses may come back in any order, so they are re-keyed by id rather than zipped
   * positionally. A provider that reordered would otherwise hand you one feed's price
   * under another feed's label, which is the worst failure available here.
   */
  async callBatch(requests: RpcRequest[]): Promise<Settled[]> {
    if (requests.length === 0) return []

    const withIds = requests.map(req => ({
      jsonrpc: '2.0' as const,
      id: ++this.requestId,
      method: req.method,
      params: req.params ?? [],
    }))

    const body = await this.post(JSON.stringify(withIds))

    if (!Array.isArray(body)) {
      // Provider does not support batch. Fall back to individual calls rather than
      // failing the whole panel over a transport detail.
      return Promise.all(
        requests.map(async (req): Promise<Settled> => {
          try {
            return { ok: true, result: await this.call(req.method, req.params ?? []) }
          } catch (err) {
            if (err instanceof RpcTransportError) throw err
            return { ok: false, error: err instanceof Error ? err.message : String(err) }
          }
        }),
      )
    }

    const byId = new Map<number, RpcEnvelope>()
    for (const envelope of body as RpcEnvelope[]) byId.set(envelope.id, envelope)

    return withIds.map((sent): Settled => {
      const envelope = byId.get(sent.id)
      if (!envelope) return { ok: false, error: `${sent.method} missing from the batch response` }
      try {
        return { ok: true, result: RpcClient.unwrap(envelope, sent.method) }
      } catch (err) {
        return { ok: false, error: err instanceof Error ? err.message : String(err) }
      }
    })
  }

  // ── convenience wrappers ───────────────────────────────────────────────────

  async blockNumber(): Promise<number> {
    return parseInt((await this.call('eth_blockNumber')) as string, 16)
  }

  async chainId(): Promise<number> {
    return parseInt((await this.call('eth_chainId')) as string, 16)
  }

  async gasPriceWei(): Promise<bigint> {
    return BigInt((await this.call('eth_gasPrice')) as string)
  }

  ethCallRequest(to: string, data: string): RpcRequest {
    return { method: 'eth_call', params: [{ to, data }, 'latest'] }
  }

  /**
   * Fetch the three aggregator fields a price read needs, for many addresses at once.
   *
   * `decimals()`/`description()` are only requested for addresses not already cached, so
   * a steady-state poll costs one `latestRoundData()` per feed.
   *
   * Per-address outcome: either the three raw payloads, or the reason that one address
   * failed. The caller renders the failures instead of quietly dropping them.
   */
  async readAggregators(
    addresses: string[],
  ): Promise<Map<string, { ok: true; raw: AggregatorRaw } | { ok: false; error: string }>> {
    const chainId = this.endpoint.network.chainId
    const cacheKey = (address: string) => `${chainId}:${address.toLowerCase()}`

    const requests: RpcRequest[] = []
    const plan: { address: string; needsConstants: boolean }[] = []

    for (const address of addresses) {
      const cached = constantsCache.get(cacheKey(address))
      requests.push(this.ethCallRequest(address, SELECTOR_LATEST_ROUND_DATA))
      if (!cached) {
        requests.push(this.ethCallRequest(address, SELECTOR_DECIMALS))
        requests.push(this.ethCallRequest(address, SELECTOR_DESCRIPTION))
      }
      plan.push({ address, needsConstants: !cached })
    }

    const results = await this.callBatch(requests)

    const out = new Map<string, { ok: true; raw: AggregatorRaw } | { ok: false; error: string }>()
    let cursor = 0
    for (const { address, needsConstants } of plan) {
      const round = results[cursor++]
      const rawDecimals = needsConstants ? results[cursor++] : null
      const rawDescription = needsConstants ? results[cursor++] : null

      // Collect every failure for this address, so the panel can say which call broke.
      const failure = [round, rawDecimals, rawDescription].find(
        (r): r is { ok: false; error: string } => r !== null && !r.ok,
      )
      if (failure) {
        out.set(address, { ok: false, error: failure.error })
        continue
      }

      let constants = constantsCache.get(cacheKey(address))
      if (needsConstants) {
        constants = {
          decimals: (rawDecimals as { ok: true; result: unknown }).result as string,
          description: (rawDescription as { ok: true; result: unknown }).result as string,
        }
        // Only cached after a clean decode-able read, so a transient error never
        // becomes a permanently wrong `decimals` for the process lifetime.
        constantsCache.set(cacheKey(address), constants)
      }

      out.set(address, {
        ok: true,
        raw: {
          latestRoundData: (round as { ok: true; result: unknown }).result as string,
          ...constants!,
        },
      })
    }
    return out
  }
}

export function gwei(wei: bigint): number {
  return Number(wei) / 1e9
}
