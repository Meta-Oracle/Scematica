// Chainlink price feed registry and reader — a port of `alchem_link/feeds.py`.
//
// Every address here was verified against its live aggregator: called for `description()`
// and `decimals()`, and filed under the pair the contract itself reports. That check is
// worth doing — the address widely passed around as Base "BTC/USD" reports `WBTC / USD`,
// and it is registered under the name it actually answers to.
//
// The Python registry stays authoritative. When it changes, change this too — the
// `/api/alchem/verify` route is what catches the two drifting apart, because it asks the
// chain rather than either table.

import { decodeString, scale, toInt, toUint, words } from './abi'
import { DEFAULT_NETWORK, getNetwork } from './networks'
import type { AggregatorRaw } from './rpc'

/** Fallback staleness threshold when a feed has no explicit heartbeat, in seconds. */
export const DEFAULT_HEARTBEAT_SECS = 3600

/**
 * Stablecoin feeds publish on a far longer cadence than volatile pairs — mainnet
 * USDC/USD was observed ~13 hours between updates while ETH/USD moved every few minutes.
 * Flagging those as stale would be noise, not signal.
 */
const STABLE_HEARTBEAT = 86_400

export interface Feed {
  pair: string
  address: string
  decimals: number
  /**
   * Publish interval the feed is expected to beat. A conservative default, not gospel —
   * confirm the official heartbeat for anything you put money behind.
   */
  heartbeatSecs: number
  note?: string
}

function feed(pair: string, address: string, decimals = 8, heartbeatSecs = DEFAULT_HEARTBEAT_SECS, note?: string): Feed {
  return { pair, address, decimals, heartbeatSecs, note }
}

export const FEEDS: Record<string, Record<string, Feed>> = {
  ethereum: {
    'ETH/USD': feed('ETH/USD', '0x5f4eC3Df9cbd43714FE2740f5E3616155c5b8419'),
    'BTC/USD': feed('BTC/USD', '0xF4030086522a5bEEa4988F8cA5B36dbC97BeE88c'),
    'LINK/USD': feed('LINK/USD', '0x2c1d072e956AFFC0D435Cb7AC38EF18d24d9127c'),
    'SOL/USD': feed('SOL/USD', '0x4ffC43a60e009B551865A93d232E33Fce9f01507'),
    'USDC/USD': feed('USDC/USD', '0x8fFfFfd4AfB6115b954Bd326cbe7B4BA576818f6', 8, STABLE_HEARTBEAT),
    'DAI/USD': feed('DAI/USD', '0xAed0c38402a5d19df6E4c03F4E2DceD6e29c1ee9', 8, STABLE_HEARTBEAT),
  },
  sepolia: {
    'ETH/USD': feed('ETH/USD', '0x694AA1769357215DE4FAC081bf1f309aDC325306'),
    'BTC/USD': feed('BTC/USD', '0x1b44F3514812d835EB1BDB0acB33d3fA3351Ee43'),
    'LINK/USD': feed('LINK/USD', '0xc59E3633BAAC79493d908e63626716e204A45EdF'),
  },
  base: {
    'ETH/USD': feed('ETH/USD', '0x71041dddad3595F9CEd3DcCFBe3D1F4b0a16Bb70'),
    // Reports "WBTC / USD" on-chain. Registered under its real name on purpose: WBTC can
    // depeg from BTC, so quoting it as BTC/USD would be a live foot-gun.
    'WBTC/USD': feed(
      'WBTC/USD',
      '0xCCADC697c55bbB68dc5bCdf8d3CBe83CdD4E071E',
      8,
      DEFAULT_HEARTBEAT_SECS,
      'Wrapped BTC, not spot BTC — can depeg.',
    ),
  },
  arbitrum: {
    'ETH/USD': feed('ETH/USD', '0x639Fe6ab55C921f74e7fac1ee960C0B6293ba612'),
    'BTC/USD': feed('BTC/USD', '0x6ce185860a4963106506C203335A2910413708e9'),
    'LINK/USD': feed('LINK/USD', '0x86E53CF1B870786351Da77A57575e79CB55812CB'),
  },
  optimism: {
    'ETH/USD': feed('ETH/USD', '0x13e3Ee699D1909E989722E753853AE30b17e08c5'),
    'BTC/USD': feed('BTC/USD', '0xD702DD976Fb76Fffc2D3963D037dfDae5b04E593'),
  },
  polygon: {
    'ETH/USD': feed('ETH/USD', '0xF9680D99D6C9589e2a93a78A04A279e509205945'),
    'BTC/USD': feed('BTC/USD', '0xc907E116054Ad103354f2D350FD2514433D57F6f'),
    'MATIC/USD': feed('MATIC/USD', '0xAB594600376Ec9fD91F8e885dADF0CE036862dE0'),
  },
}

export function listFeeds(network: string = DEFAULT_NETWORK): Feed[] {
  getNetwork(network) // validates the network name
  return Object.values(FEEDS[network.toLowerCase()] ?? {})
}

export function getFeed(pair: string, network: string = DEFAULT_NETWORK): Feed {
  getNetwork(network)
  const table = FEEDS[network.toLowerCase()] ?? {}
  const key = pair.toUpperCase().replace(/-/g, '/').trim()
  const found = table[key]
  if (!found) {
    const known = Object.keys(table).sort().join(', ') || 'none'
    throw new Error(`no feed '${pair}' on ${network}. Known pairs: ${known}`)
  }
  return found
}

export type FeedStatus = 'FRESH' | 'STALE' | 'INVALID'

export interface FeedReading {
  pair: string
  network: string
  address: string
  /** The contract's own description(), so a mislabelled registry entry is visible. */
  description: string
  price: number
  answerRaw: string
  decimals: number
  roundId: string
  updatedAt: number
  ageSecs: number
  heartbeatSecs: number
  stale: boolean
  status: FeedStatus
  note?: string
}

/** A feed that could not be read. Rendered as a row, never dropped silently. */
export interface FeedFailure {
  pair: string
  address: string
  error: string
}

/**
 * Turn the three raw aggregator responses into a checked reading.
 *
 * Pure: no I/O, so the staleness and decoding logic is testable offline — same contract
 * as `decode_reading` in Python.
 */
export function decodeReading(
  f: Feed,
  network: string,
  raw: AggregatorRaw,
  nowSecs?: number,
): FeedReading {
  const roundWords = words(raw.latestRoundData)
  if (roundWords.length < 5) {
    throw new Error(
      `latestRoundData() returned ${roundWords.length} words, expected 5 — ` +
        `is ${f.address} an AggregatorV3 contract?`,
    )
  }

  const roundId = toUint(roundWords[0])
  const answer = toInt(roundWords[1])
  const updatedAt = Number(toUint(roundWords[3]))

  const decimals = Number(toUint(words(raw.decimals)[0]))
  const description = decodeString(raw.description).trim()

  const current = nowSecs ?? Math.floor(Date.now() / 1000)
  // A feed timestamped in the future is a clock problem, not negative age.
  const ageSecs = Math.max(0, current - updatedAt)
  const stale = ageSecs > f.heartbeatSecs

  return {
    pair: f.pair,
    network: network.toLowerCase(),
    address: f.address,
    description,
    price: scale(answer, decimals),
    answerRaw: answer.toString(),
    decimals,
    roundId: roundId.toString(),
    updatedAt,
    ageSecs,
    heartbeatSecs: f.heartbeatSecs,
    stale,
    status: answer <= 0n ? 'INVALID' : stale ? 'STALE' : 'FRESH',
    note: f.note,
  }
}
