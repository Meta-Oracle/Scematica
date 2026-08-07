// Supported networks — a port of the table in `alchem_link/networks.py`.
//
// Static data only, so the client network picker can import it. Endpoint *resolution*
// lives in `endpoint.ts` and is server-only: it reads ALCHEMY_API_KEY, which must never
// reach the browser bundle. (Reading these endpoints from the page would fail anyway —
// most public RPC hosts send no CORS headers.)
//
// Every chain id below was read back from the live endpoint, not copied from a table.

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
  /**
   * Testnet feeds carry test data from a separate node set. They must never be mixed
   * into a cross-chain price comparison with mainnets — the numbers are unrelated.
   */
  testnet?: boolean
  /**
   * Rollups need a sequencer-uptime check alongside any price read: the feed keeps
   * answering while the sequencer is down, with a price frozen at the moment it stopped.
   */
  layer2?: boolean
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
    testnet: true,
  },
  base: {
    key: 'base',
    label: 'Base Mainnet',
    chainId: 8453,
    nativeSymbol: 'ETH',
    alchemySubdomain: 'base-mainnet',
    publicRpc: 'https://base-rpc.publicnode.com',
    explorer: 'https://basescan.org',
    layer2: true,
  },
  arbitrum: {
    key: 'arbitrum',
    label: 'Arbitrum One',
    chainId: 42161,
    nativeSymbol: 'ETH',
    alchemySubdomain: 'arb-mainnet',
    publicRpc: 'https://arbitrum-one-rpc.publicnode.com',
    explorer: 'https://arbiscan.io',
    layer2: true,
  },
  optimism: {
    key: 'optimism',
    label: 'OP Mainnet',
    chainId: 10,
    nativeSymbol: 'ETH',
    alchemySubdomain: 'opt-mainnet',
    publicRpc: 'https://optimism-rpc.publicnode.com',
    explorer: 'https://optimistic.etherscan.io',
    layer2: true,
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
  avalanche: {
    key: 'avalanche',
    label: 'Avalanche C-Chain',
    chainId: 43114,
    nativeSymbol: 'AVAX',
    alchemySubdomain: 'avax-mainnet',
    publicRpc: 'https://avalanche-c-chain-rpc.publicnode.com',
    explorer: 'https://snowtrace.io',
  },
  bnb: {
    key: 'bnb',
    label: 'BNB Smart Chain',
    chainId: 56,
    nativeSymbol: 'BNB',
    alchemySubdomain: 'bnb-mainnet',
    publicRpc: 'https://bsc-rpc.publicnode.com',
    explorer: 'https://bscscan.com',
  },
  gnosis: {
    key: 'gnosis',
    label: 'Gnosis Chain',
    chainId: 100,
    nativeSymbol: 'xDAI',
    alchemySubdomain: 'gnosis-mainnet',
    publicRpc: 'https://gnosis-rpc.publicnode.com',
    explorer: 'https://gnosisscan.io',
  },
  scroll: {
    key: 'scroll',
    label: 'Scroll',
    chainId: 534352,
    nativeSymbol: 'ETH',
    alchemySubdomain: 'scroll-mainnet',
    publicRpc: 'https://scroll-rpc.publicnode.com',
    explorer: 'https://scrollscan.com',
    layer2: true,
  },
  linea: {
    key: 'linea',
    label: 'Linea',
    chainId: 59144,
    nativeSymbol: 'ETH',
    alchemySubdomain: 'linea-mainnet',
    publicRpc: 'https://linea-rpc.publicnode.com',
    explorer: 'https://lineascan.build',
    layer2: true,
  },
}

export const DEFAULT_NETWORK = 'ethereum'

export const NETWORK_KEYS = Object.keys(NETWORKS)

/**
 * Chainlink L2 sequencer uptime feeds — a port of `alchem_link/sequencer.py`.
 *
 * Each was verified live: all three report `L2 Sequencer Uptime Status Feed` as their
 * own `description()`.
 */
export const SEQUENCER_FEEDS: Record<string, string> = {
  arbitrum: '0xFdB631F5EE196F0ed6FAa767959853A9F217697D',
  optimism: '0x371EAD81c9102C9BF4874A9075FFFf170F2Ee389',
  base: '0xBCF85224fc0756B9Fa45aA7892530B47e10b6433',
}

/** Chainlink's documented grace period after a sequencer restart, in seconds. */
export const SEQUENCER_GRACE_PERIOD_SECS = 3600

export function listNetworks(): Network[] {
  return Object.values(NETWORKS)
}

export function getNetwork(key: string): Network {
  const net = NETWORKS[key.toLowerCase()]
  if (!net) {
    throw new Error(`unknown network '${key}'. Known networks: ${NETWORK_KEYS.slice().sort().join(', ')}`)
  }
  return net
}

// Endpoint resolution — `resolveEndpoint`, `redact`, and the `Endpoint` type — lives in
// `endpoint.ts`, which is server-only. Keeping it out of this file is what lets the
// client import the network table without dragging `process.env` along with it.
