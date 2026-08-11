// BOT Chain network table — **pure data, safe in a client bundle**.
//
// This mirrors `scema-botchain/crates/botchain-core/src/chain.rs`, and the Rust side is
// authoritative. Same rule as `lib/alchem/` mirroring the Python package: when one
// changes, change the other, and let a live check catch the drift rather than trusting
// either table. `/api/botchain/status` verifies the chain id against the endpoint on
// every call, so a stale value here surfaces as a mismatch instead of as wrong data.
//
// Split from `rpc.ts` deliberately — that module reads env vars and must never travel
// into the browser. This one is a table and can.

export type EndpointKind = 'node' | 'explorer-proxy'

export interface Endpoint {
  url: string
  kind: EndpointKind
  /** Why it sits at this position in the list. */
  note: string
}

export interface Venue {
  name: string
  router: string
  /** Resolved on-chain via `factory()`, not copied from documentation. */
  factory: string
}

export interface TokenInfo {
  symbol: string
  name: string
  address: string
  decimals: number
}

export interface Network {
  key: 'mainnet' | 'testnet'
  name: string
  chainId: number
  symbol: string
  decimals: number
  explorer: string
  endpoints: Endpoint[]
  venues: Venue[]
  tokens: TokenInfo[]
  /** Surfaced in the UI when the chain id is not a reliable identifier. */
  chainIdWarning?: string
}

export const MAINNET: Network = {
  key: 'mainnet',
  name: 'BOT Chain Mainnet',
  chainId: 677,
  symbol: 'BOT',
  decimals: 18,
  explorer: 'https://scan.botchain.ai',
  endpoints: [
    {
      url: 'https://rpc.botchain.ai',
      kind: 'node',
      note: 'Official node RPC — the only endpoint that can broadcast transactions.',
    },
    {
      // Kept as a second entry because the node RPC was briefly unreachable during
      // testing while this Cloudflare-fronted host answered every time. Reads only.
      url: 'https://scan.botchain.ai/api/eth-rpc',
      kind: 'explorer-proxy',
      note: 'Explorer JSON-RPC proxy — reads only, but reachable when the node is not.',
    },
  ],
  venues: [
    {
      name: 'SwapRouter (V3-style)',
      router: '0x07032d47A1b9f8460cBeE9dC17c1d3E438693929',
      factory: '0x1c51c173323ec11bb4e3c4fd2314c225dc4b5419',
    },
    {
      name: 'CASwapRouter',
      router: '0x5b90611D4eB8FC82Fc2E3d1F0501Dd6F434441AD',
      factory: '0x9c937ebc3748825026677e20b13b5e306494a38d',
    },
  ],
  tokens: [
    { symbol: 'WBOT', name: 'Wrapped BOT', address: '0xD5452816194a3784dBa983426cCe7c122F4abd30', decimals: 18 },
    { symbol: 'USDT', name: 'Tether USD', address: '0xaBabc7Ddc03e501d190C676BF3d92ef0e6e87a3C', decimals: 18 },
    { symbol: 'CA', name: 'CaryPact', address: '0x546307af427902A75771434Df831d88219784E19', decimals: 18 },
  ],
}

export const TESTNET: Network = {
  key: 'testnet',
  name: 'BOT Chain Testnet',
  chainId: 968,
  symbol: 'BOT',
  decimals: 18,
  explorer: 'https://scan.bohr.life',
  endpoints: [
    { url: 'https://rpc.bohr.life', kind: 'node', note: 'Official testnet RPC.' },
  ],
  venues: [],
  tokens: [],
  chainIdWarning:
    'Chain ID 968 is not unique: ChainList registers it as Datagram, and it is also ' +
    "BSC's Rialto config. Never identify this network by chain id alone — pin the " +
    'endpoint first, then verify the id against it.',
}

export const NETWORKS: Network[] = [MAINNET, TESTNET]

export function getNetwork(key: string): Network {
  return NETWORKS.find((n) => n.key === key) ?? MAINNET
}

/** Parlia's validator-set contract. Its per-block deposit dominates chain activity. */
export const VALIDATOR_SET = '0x0000000000000000000000000000000000001000'

/** Loose address check — enough to avoid sending obvious junk to an RPC. */
export function isAddress(v: string): boolean {
  return /^0x[0-9a-fA-F]{40}$/.test(v.trim())
}

/** Format a wei-scale bigint for display without floating-point drift. */
export function formatUnits(value: bigint, decimals: number, places = 4): string {
  const base = 10n ** BigInt(decimals)
  const whole = value / base
  const frac = value % base
  if (places === 0) return whole.toString()
  const fracStr = frac.toString().padStart(decimals, '0').slice(0, places)
  return `${whole.toString()}.${fracStr}`
}

export function shortAddress(a: string): string {
  return a.length > 12 ? `${a.slice(0, 6)}…${a.slice(-4)}` : a
}
