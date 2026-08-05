// Endpoint resolution — server-only.
//
// Split out of `networks.ts` deliberately. That module is a static table and gets
// imported by the client `AlchemConsole` to build the network picker; this one reads
// `ALCHEMY_API_KEY` and must never travel with it. Next.js only inlines `NEXT_PUBLIC_*`
// into client bundles, so a stray import would not actually leak the key — it would
// silently resolve to the public fallback instead, which is the more likely and more
// confusing bug. The guard below turns that into a loud one.
//
// (The `server-only` package would make this a build error rather than a runtime one.
// It is not a dependency here, and this file is small enough not to warrant adding one.)

import { DEFAULT_NETWORK, getNetwork, type Network } from './networks'

if (typeof window !== 'undefined') {
  throw new Error(
    'lib/alchem/endpoint.ts is server-only — import lib/alchem/networks.ts from client components',
  )
}

export interface Endpoint {
  url: string
  /** Where the URL came from — surfaced by the doctor panel so nobody debugs the wrong one. */
  source: string
  authenticated: boolean
  network: Network
}

/** URL safe to display: an Alchemy key in the path is replaced with a placeholder. */
export function redact(url: string): string {
  const marker = '/v2/'
  const idx = url.indexOf(marker)
  return idx === -1 ? url : url.slice(0, idx + marker.length) + '<key>'
}

/**
 * Pick the RPC endpoint to use, most explicit source first:
 *
 *   1. `ALCHEMY_URL`     — a full endpoint, for a non-default host
 *   2. `ALCHEMY_API_KEY` — combined with the network's Alchemy subdomain
 *   3. the network's keyless public endpoint
 *
 * The fallback is what makes the page work on a fresh clone. It is rate limited and
 * unauthenticated; the doctor panel says so rather than letting you find out under load.
 *
 * Unlike the CLI there is no `--rpc-url` equivalent: accepting a caller-supplied URL here
 * would turn these routes into an open proxy that any visitor could point at an internal
 * host. The environment is the only input.
 */
export function resolveEndpoint(network: string = DEFAULT_NETWORK): Endpoint {
  const net = getNetwork(network)

  const explicitUrl = (process.env.ALCHEMY_URL || '').trim()
  if (explicitUrl) {
    return { url: explicitUrl, source: 'ALCHEMY_URL', authenticated: true, network: net }
  }

  const apiKey = (process.env.ALCHEMY_API_KEY || '').trim()
  if (apiKey) {
    return {
      url: `https://${net.alchemySubdomain}.g.alchemy.com/v2/${apiKey}`,
      source: 'ALCHEMY_API_KEY',
      authenticated: true,
      network: net,
    }
  }

  return { url: net.publicRpc, source: 'public fallback', authenticated: false, network: net }
}
