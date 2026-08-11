// BOT Chain JSON-RPC with ordered endpoint failover. **Server-only.**
//
// Mirrors `botchain-core::rpc` on the Rust side; that stays authoritative. Kept out of
// the browser because it reads `BOTCHAIN_RPC_URL` — an operator's private node URL is
// often credentialed, and the same guard style as `lib/alchem/endpoint.ts` makes a stray
// client import loud instead of silently degrading to the public list.
//
// **No simulation branch**, exactly like the alchem-link routes: these read a chain or
// report the error. A fabricated balance would be worse here than a missing one.

if (typeof window !== 'undefined') {
  throw new Error(
    'lib/botchain/rpc.ts is server-only — import lib/botchain/networks.ts from client components',
  )
}

import { getNetwork, type Endpoint, type Network } from './networks'

/** Per-endpoint timeout. The point of a list is to move on, not to wait. */
const TIMEOUT_MS = 8_000

export interface RpcResult<T> {
  result: T
  /** Which endpoint answered — a proxy read and a node read are not interchangeable. */
  endpoint: string
  kind: Endpoint['kind']
  elapsedMs: number
}

class RpcError extends Error {
  constructor(message: string, readonly code: number) {
    super(message)
  }
}

function endpointsFor(network: Network): Endpoint[] {
  const override = (process.env.BOTCHAIN_RPC_URL || '').trim()
  // An operator node goes first; the built-ins stay as fallback, so a private node going
  // down degrades to public reads rather than to nothing.
  return override
    ? [{ url: override, kind: 'node', note: 'operator override (BOTCHAIN_RPC_URL)' }, ...network.endpoints]
    : network.endpoints
}

/**
 * Call a method, walking the endpoint list until one answers.
 *
 * A transport failure moves to the next endpoint. A well-formed JSON-RPC **error** does
 * not: if one node says "execution reverted", every node will, and retrying turns one
 * honest error into N slow ones.
 */
export async function rpc<T = unknown>(
  networkKey: string,
  method: string,
  params: unknown[] = [],
): Promise<RpcResult<T>> {
  const network = getNetwork(networkKey)
  const errors: string[] = []

  for (const ep of endpointsFor(network)) {
    const started = Date.now()
    try {
      const res = await fetch(ep.url, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ jsonrpc: '2.0', id: 1, method, params }),
        cache: 'no-store',
        signal: AbortSignal.timeout(TIMEOUT_MS),
      })
      if (!res.ok) {
        errors.push(`${ep.url}: HTTP ${res.status}`)
        continue
      }

      const body = (await res.json()) as { result?: T; error?: { code: number; message: string } }
      if (body.error) {
        throw new RpcError(`${method} rejected: ${body.error.message}`, body.error.code)
      }
      if (body.result === undefined) {
        errors.push(`${ep.url}: reply had neither result nor error`)
        continue
      }

      return { result: body.result, endpoint: ep.url, kind: ep.kind, elapsedMs: Date.now() - started }
    } catch (err) {
      if (err instanceof RpcError) throw err
      errors.push(`${ep.url}: ${err instanceof Error ? err.message : String(err)}`)
    }
  }

  throw new Error(`No BOT Chain endpoint answered ${method}. Tried — ${errors.join('; ')}`)
}

/**
 * Verify the endpoint reports the chain id we expect, and fail loudly if not.
 *
 * Always ahead of trusting a read. Chain ids are not unique — 968 answers for BOT Chain
 * testnet, for Datagram in the public registry, and for BSC's Rialto — so the meaningful
 * check is "this pinned endpoint reports the id I expect", never "a registry says 968 is
 * BOT Chain".
 */
export async function verifiedChainId(networkKey: string): Promise<RpcResult<number>> {
  const network = getNetwork(networkKey)
  const r = await rpc<string>(networkKey, 'eth_chainId')
  const got = Number.parseInt(r.result, 16)
  if (got !== network.chainId) {
    throw new Error(
      `${r.endpoint} reported chain id ${got} but ${network.chainId} was expected — wrong ` +
        'network, or a chain-id collision. Refusing to report its data as BOT Chain.',
    )
  }
  return { ...r, result: got }
}

export const hexToBigInt = (hex: string): bigint => BigInt(hex)

// ── minimal ABI encoding ──────────────────────────────────────────────────────
//
// Hand-rolled rather than pulling viem/ethers for three selectors. Same reasoning as
// `lib/scylar/markdown.ts`: the subset actually needed is small and spelling it out
// avoids a dependency whose surface dwarfs the use. Add a library when signing arrives —
// that is where writing it yourself stops being sensible.

const pad32 = (hexNo0x: string) => hexNo0x.padStart(64, '0')

/** `balanceOf(address)` — selector 0x70a08231. */
export function encodeBalanceOf(owner: string): string {
  return '0x70a08231' + pad32(owner.toLowerCase().replace(/^0x/, ''))
}

/** `totalSupply()` — selector 0x18160ddd. */
export const TOTAL_SUPPLY_DATA = '0x18160ddd'

/** Decode a single uint256 return value. Empty data means the call returned nothing. */
export function decodeUint(hex: string): bigint | null {
  if (!hex || hex === '0x') return null
  try {
    return BigInt(hex)
  } catch {
    return null
  }
}

export async function ethCall(networkKey: string, to: string, data: string): Promise<string> {
  const r = await rpc<string>(networkKey, 'eth_call', [{ to, data }, 'latest'])
  return r.result
}
