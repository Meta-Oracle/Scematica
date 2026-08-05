// Supported networks — a port of the table in `alchem_link/networks.py`.
//
// Static data only, so the client network picker can import it. Endpoint *resolution*
// lives in `endpoint.ts` and is server-only: it reads ALCHEMY_API_KEY, which must never
// reach the browser bundle. (Reading these endpoints from the page would fail anyway —
// most public RPC hosts send no CORS headers.)

export interface Network {
  key: string
  label: string
  chainId: number
  nativeSymbol: string
  /** Endpoint is https://<subdomain>.g.alchemy.com/v2/<key> */
  alchemySubdomain: string
  /** Keyless fallback, so the page works with zero setup. */
  publicRpc: string
  explorer: string
}

export const NETWORKS: Record<string, Network> = {
  ethereum: {
    key: 'ethereum',
    label: 'Ethereum Mainnet',
    chainId: 1,
    nativeSymbol: 'ETH',
    alchemySubdomain: 'eth-mainnet',
    publicRpc: 'https://ethereum-rpc.publicnode.com',
    explorer: 'https://etherscan.io',
  },
  sepolia: {
    key: 'sepolia',
    label: 'Ethereum Sepolia',
    chainId: 11155111,
    nativeSymbol: 'ETH',
    alchemySubdomain: 'eth-sepolia',
    publicRpc: 'https://ethereum-sepolia-rpc.publicnode.com',
    explorer: 'https://sepolia.etherscan.io',
  },
  base: {
    key: 'base',
    label: 'Base Mainnet',
    chainId: 8453,
    nativeSymbol: 'ETH',
    alchemySubdomain: 'base-mainnet',
    publicRpc: 'https://base-rpc.publicnode.com',
    explorer: 'https://basescan.org',
  },
  arbitrum: {
    key: 'arbitrum',
    label: 'Arbitrum One',
    chainId: 42161,
    nativeSymbol: 'ETH',
    alchemySubdomain: 'arb-mainnet',
    publicRpc: 'https://arbitrum-one-rpc.publicnode.com',
    explorer: 'https://arbiscan.io',
  },
  optimism: {
    key: 'optimism',
    label: 'OP Mainnet',
    chainId: 10,
    nativeSymbol: 'ETH',
    alchemySubdomain: 'opt-mainnet',
    publicRpc: 'https://optimism-rpc.publicnode.com',
    explorer: 'https://optimistic.etherscan.io',
  },
  polygon: {
    key: 'polygon',
    label: 'Polygon PoS',
    chainId: 137,
    nativeSymbol: 'POL',
    alchemySubdomain: 'polygon-mainnet',
    publicRpc: 'https://polygon-bor-rpc.publicnode.com',
    explorer: 'https://polygonscan.com',
  },
}

export const DEFAULT_NETWORK = 'ethereum'

export const NETWORK_KEYS = Object.keys(NETWORKS)

export function listNetworks(): Network[] {
  return Object.values(NETWORKS)
}

export function getNetwork(key: string): Network {
  const net = NETWORKS[key.toLowerCase()]
  if (!net) {
    throw new Error(`unknown network '${key}'. Known networks: ${NETWORK_KEYS.sort().join(', ')}`)
  }
  return net
}

// Endpoint resolution — `resolveEndpoint`, `redact`, and the `Endpoint` type — lives in
// `endpoint.ts`, which is server-only. Keeping it out of this file is what lets the
// client import the network table without dragging `process.env` along with it.
