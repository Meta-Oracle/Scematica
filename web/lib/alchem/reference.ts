// The offline half of alchem-link — the integration reference the TUI shows under
// Blueprint / Alchemy / Chainlink / Integration / Recipes.
//
// Mirrors `core.py`, `alchemy.py`, `chainlink.py`, `integration.py` and `recipes.py`.
// This is static prose, not chain data: it has no freshness, no source badge, and
// nothing here is ever presented as a live reading.

export interface Recipe {
  id: string
  name: string
  summary: string
  steps: string[]
  tags: string[]
}

export const BLUEPRINT: Record<string, Record<string, string>> = {
  alchemy: {
    focus: 'Developer workflows, RPC abstractions, and API orchestration',
    goal: 'Surface reliable onchain integrations for builders',
  },
  chainlink: {
    focus: 'Oracle patterns, data feeds, and hybrid smart contract logic',
    goal: 'Enable secure cross-chain and off-chain composability',
  },
}

export const BLUEPRINT_NEXT_STEPS: string[] = [
  'Map Alchemy APIs to Chainlink primitives',
  'Document integration patterns for smart contract developers',
  'Create starter examples and reference implementations',
]

export const ALCHEMY_CAPABILITIES: { key: string; value: string }[] = [
  { key: 'RPC', value: 'High-throughput node access for transaction submission and chain state reads' },
  { key: 'WEBSOCKET', value: 'Real-time event subscriptions for application and monitoring workflows' },
  { key: 'APIS', value: 'Enhanced APIs for token, NFT, and transaction intelligence' },
  { key: 'APIS', value: 'Debug and trace endpoints for smarter development and incident handling' },
  { key: 'DEVELOPER VALUE', value: 'Makes blockchain integration feel practical, observable, and fast to iterate on' },
]

export const CHAINLINK_CAPABILITIES: { key: string; value: string }[] = [
  { key: 'PRICE FEEDS', value: 'Secure off-chain market data for smart contract and application logic' },
  { key: 'VRF', value: 'Verifiable randomness for games, lotteries, and dynamic systems' },
  { key: 'AUTOMATION', value: 'Condition-based execution for maintenance and operational workflows' },
  { key: 'CCIP', value: 'Cross-Chain Interoperability Protocol for secure token and message transfers across chains' },
  { key: 'DEVELOPER VALUE', value: 'Adds trust-minimized, verifiable execution patterns to the stack' },
]

export const INTEGRATION_MAP: { domain: string; alchemy: string; chainlink: string }[] = [
  {
    domain: 'DATA INGESTION',
    alchemy: 'Pull chain state and event streams into application services',
    chainlink: 'Validate and enrich state with trusted external data',
  },
  {
    domain: 'EXECUTION',
    alchemy: 'Submit and monitor transactions with low-friction developer tooling',
    chainlink: 'Trigger secure automation and oracle-driven actions',
  },
  {
    domain: 'MONITORING',
    alchemy: 'Observe transaction status and debugging signals',
    chainlink: 'Observe external conditions and operational triggers',
  },
  {
    domain: 'CROSS CHAIN',
    alchemy: 'Track and verify transactions across source and destination chains via multi-network RPC',
    chainlink: 'Route messages and tokens securely across chains using CCIP',
  },
]

export const RECIPES: Recipe[] = [
  {
    id: 'oracle-backed-automation',
    name: 'Oracle-backed automation',
    summary:
      'Use Chainlink automation plus Alchemy transaction monitoring to trigger and verify onchain actions.',
    steps: [
      'Watch for a trigger condition with Chainlink automation',
      'Use Alchemy to submit and monitor the transaction',
      'Record execution state back to your application layer',
    ],
    tags: ['automation', 'monitoring', 'execution'],
  },
  {
    id: 'real-time-data-pipeline',
    name: 'Real-time data pipeline',
    summary: 'Stream blockchain events through Alchemy and enrich them with Chainlink data feeds.',
    steps: [
      'Subscribe to event streams via Alchemy websockets',
      'Normalize event payloads into a shared application model',
      'Cross-check values with Chainlink feeds before acting on them',
    ],
    tags: ['data', 'websocket', 'feeds'],
  },
  {
    id: 'secure-bridge-experiment',
    name: 'Secure bridge experiment',
    summary:
      'Prototype a bridge-like workflow that depends on Alchemy for transaction flow and Chainlink for trust-minimized signals.',
    steps: [
      'Prepare the source and destination chain context',
      'Use Alchemy to submit and inspect transaction state',
      'Use Chainlink oracles to verify the external state before finalizing',
    ],
    tags: ['bridge', 'cross-chain', 'security'],
  },
  {
    id: 'ccip-cross-chain-transfer',
    name: 'CCIP cross-chain transfer',
    summary:
      'Use Chainlink CCIP to send tokens or messages cross-chain while Alchemy tracks state on both ends.',
    steps: [
      'Initiate a CCIP send transaction on the source chain via Alchemy RPC',
      'Monitor the source chain tx confirmation with Alchemy websockets',
      'Poll the destination chain via Alchemy until the CCIP message is executed',
    ],
    tags: ['ccip', 'cross-chain', 'monitoring'],
  },
]
