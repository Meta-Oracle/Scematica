// Chainlink price feed registry and reader — a port of `alchem_link/feeds.py`.
//
// Two promises hold this table together, and both are checkable rather than asserted.
//
// **Every address was called before it was written down.** Each was queried for
// `description()` and `decimals()`, and is filed under the pair the contract itself
// reports. That check keeps catching things: the address widely shared as Base "BTC/USD"
// reports `WBTC / USD`, and the Gnosis address commonly labelled "xDAI/USD" reports
// `DAI / USD`. Both are registered under the names they answer to.
//
// **Every heartbeat was measured, not copied.** This registry used to declare 3600s for
// everything, inherited from Ethereum mainnet, and that is wrong almost everywhere:
// Polygon's feeds publish every ~60 seconds, Optimism and Base every ~1200. A 3600s
// staleness check on a Polygon feed will not fire until the feed has been dead for an
// hour. The values here come from `alchem-link cadence` walking each feed's round history.
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
 * Slack allowed on top of the heartbeat before calling a feed stale.
 *
 * A "1 hour" heartbeat does not mean 3600.000 seconds. Measured ceilings run a percent
 * or two over — mainnet ETH/USD was observed at 3684s against a 3600s configuration —
 * because publishes are triggered by block timestamps, not a wall clock. Without slack
 * every feed flickers STALE at the top of its cycle, which trains people to ignore the
 * flag exactly when it starts meaning something.
 */
export const STALENESS_TOLERANCE = 0.15

export interface Feed {
  pair: string
  address: string
  decimals: number
  /** Publish interval measured from round history, in seconds. */
  heartbeatSecs: number
  /**
   * True when a heartbeat-triggered publish was actually observed. False means the value
   * is a conservative upper bound: the sampling window never contained a quiet period
   * long enough for the clock rather than a price move to trigger the publish.
   */
  heartbeatMeasured: boolean
  note?: string
}

function feed(
  pair: string,
  address: string,
  decimals = 8,
  heartbeatSecs = DEFAULT_HEARTBEAT_SECS,
  heartbeatMeasured = true,
  note?: string,
): Feed {
  return { pair, address, decimals, heartbeatSecs, heartbeatMeasured, note }
}

/** Age at which a feed is treated as stale, tolerance included. */
export function staleAfterSecs(f: Feed): number {
  return Math.floor(f.heartbeatSecs * (1 + STALENESS_TOLERANCE))
}

export const FEEDS: Record<string, Record<string, Feed>> = {
  ethereum: {
    'ETH/USD': feed('ETH/USD', '0x5f4eC3Df9cbd43714FE2740f5E3616155c5b8419', 8, 3600),
    'BTC/USD': feed('BTC/USD', '0xF4030086522a5bEEa4988F8cA5B36dbC97BeE88c', 8, 3600),
    'LINK/USD': feed('LINK/USD', '0x2c1d072e956AFFC0D435Cb7AC38EF18d24d9127c', 8, 3600),
    'DAI/USD': feed('DAI/USD', '0xAed0c38402a5d19df6E4c03F4E2DceD6e29c1ee9', 8, 3600),
    'AAVE/USD': feed('AAVE/USD', '0x547a514d5e3769680Ce22B2361c10Ea13619e8a9', 8, 3600),
    'UNI/USD': feed('UNI/USD', '0x553303d460EE0afB37EdFf9bE42922D8FF63220e', 8, 3600),
    'ETH/BTC': feed('ETH/BTC', '0xAc559F25B1619171CbC396a50854A3240b6A4e99', 8, 3600),
    'STETH/USD': feed('STETH/USD', '0xCfE54B5cD566aB89272946F602D76Ea879CAb4a8', 8, 3600, true, 'Liquid-staked ETH, not spot ETH — trades at its own price and can discount.'),
    'XAU/USD': feed('XAU/USD', '0x214eD9Da11D2fbe465a6fc601a91E62EbEc1a0D6', 8, 14400),
    'SOL/USD': feed('SOL/USD', '0x4ffC43a60e009B551865A93d232E33Fce9f01507', 8, 86400),
    'AVAX/USD': feed('AVAX/USD', '0xFF3EEb22B5E3dE6e705b44749C2559d704923FD7', 8, 86400),
    'BNB/USD': feed('BNB/USD', '0x14e613AC84a31f709eadbdF89C6CC390fDc9540A', 8, 86400),
    'MATIC/USD': feed('MATIC/USD', '0x7bAC85A8a13A4BcD8abb3eB7d6b4d632c5a57676', 8, 86400),
    'EUR/USD': feed('EUR/USD', '0xb49f677943BC038e9857d61E7d053CaA2C1734C1', 8, 86400),
    'USDC/USD': feed('USDC/USD', '0x8fFfFfd4AfB6115b954Bd326cbe7B4BA576818f6', 8, 86400),
    'USDT/USD': feed('USDT/USD', '0x3E7d1eAB13ad0104d2750B8863b489D65364e32D', 8, 86400),
  },
  sepolia: {
    'ETH/USD': feed('ETH/USD', '0x694AA1769357215DE4FAC081bf1f309aDC325306', 8, 3600),
    'BTC/USD': feed('BTC/USD', '0x1b44F3514812d835EB1BDB0acB33d3fA3351Ee43', 8, 3600),
    'LINK/USD': feed('LINK/USD', '0xc59E3633BAAC79493d908e63626716e204A45EdF', 8, 3600),
  },
  base: {
    'ETH/USD': feed('ETH/USD', '0x71041dddad3595F9CEd3DcCFBe3D1F4b0a16Bb70', 8, 1200),
    'WBTC/USD': feed('WBTC/USD', '0xCCADC697c55bbB68dc5bCdf8d3CBe83CdD4E071E', 8, 1200, true, 'Wrapped BTC, not spot BTC — can depeg.'),
    'CBETH/USD': feed('CBETH/USD', '0xd7818272B9e248357d13057AAb0B417aF31E817d', 8, 1200, true, 'Coinbase staked ETH — priced against its own market, not ETH spot.'),
    'LINK/USD': feed('LINK/USD', '0x17CAb8FE31E32f08326e5E27412894e49B0f9D65', 8, 43200, false),
    'USDC/USD': feed('USDC/USD', '0x7e860098F58bBFC8648a4311b374B1D669a2bc6B', 8, 86400),
    'DAI/USD': feed('DAI/USD', '0x591e79239a7d679378eC8c847e5038150364C78F', 8, 86400),
  },
  arbitrum: {
    'ARB/USD': feed('ARB/USD', '0xb2A824043730FE05F3DA2efaFa1CBbe83fa548D6', 8, 300),
    'USDC/USD': feed('USDC/USD', '0x50834F3163758fcC1Df9973b6e91f0F0F0434aD3', 8, 300),
    'USDT/USD': feed('USDT/USD', '0x3f3f5dF88dC9F13eac63DF89EC16ef6e7E25DdE7', 8, 300),
    'ETH/USD': feed('ETH/USD', '0x639Fe6ab55C921f74e7fac1ee960C0B6293ba612', 8, 600),
    'BTC/USD': feed('BTC/USD', '0x6ce185860a4963106506C203335A2910413708e9', 8, 900, false),
    'LINK/USD': feed('LINK/USD', '0x86E53CF1B870786351Da77A57575e79CB55812CB', 8, 1800),
    'SOL/USD': feed('SOL/USD', '0x24ceA4b8ce57cdA5058b924B9B9987992450590c', 8, 3600, false),
    'DAI/USD': feed('DAI/USD', '0xc5C8E77B397E531B8EC06BFb0048328B30E9eCfB', 8, 86400),
  },
  optimism: {
    'ETH/USD': feed('ETH/USD', '0x13e3Ee699D1909E989722E753853AE30b17e08c5', 8, 1200),
    'BTC/USD': feed('BTC/USD', '0xD702DD976Fb76Fffc2D3963D037dfDae5b04E593', 8, 1200),
    'LINK/USD': feed('LINK/USD', '0xCc232dcFAAE6354cE191Bd574108c1aD03f86450', 8, 1200),
    'OP/USD': feed('OP/USD', '0x0D276FC14719f9292D5C1eA2198673d1f4269246', 8, 1200),
    'USDC/USD': feed('USDC/USD', '0x16a9FA2FDa030272Ce99B29CF780dFA30361E0f3', 8, 86400),
    'DAI/USD': feed('DAI/USD', '0x8dBa75e83DA73cc766A7e5a0ee71F656BAb470d6', 8, 86400),
  },
  polygon: {
    'ETH/USD': feed('ETH/USD', '0xF9680D99D6C9589e2a93a78A04A279e509205945', 8, 60),
    'BTC/USD': feed('BTC/USD', '0xc907E116054Ad103354f2D350FD2514433D57F6f', 8, 60),
    'MATIC/USD': feed('MATIC/USD', '0xAB594600376Ec9fD91F8e885dADF0CE036862dE0', 8, 60),
    'LINK/USD': feed('LINK/USD', '0xd9FFdb71EbE7496cC440152d43986Aae0AB76665', 8, 60),
    'SOL/USD': feed('SOL/USD', '0x10C8264C0935b3B9870013e057f330Ff3e9C56dC', 8, 60),
    'USDC/USD': feed('USDC/USD', '0xfE4A8cc5b5B2366C1B58Bea3858e81843581b2F7', 8, 60),
    'DAI/USD': feed('DAI/USD', '0x4746DeC9e833A82EC7C2C1356372CcF2cfcD2F3D', 8, 60),
  },
  avalanche: {
    'AVAX/USD': feed('AVAX/USD', '0x0A77230d17318075983913bC2145DB16C7366156', 8, 120),
    'ETH/USD': feed('ETH/USD', '0x976B3D034E162d8bD72D6b9C989d545b839003b0', 8, 7200, false),
    'BTC/USD': feed('BTC/USD', '0x2779D32d5166BAaa2B2b658333bA7e6Ec0C65743', 8, 7200, false),
    'LINK/USD': feed('LINK/USD', '0x49ccd9ca821EfEab2b98c60dC60F518E765EDe9a', 8, 14400),
    'USDC/USD': feed('USDC/USD', '0xF096872672F44d6EBA71458D74fe67F9a77a23B9', 8, 86400),
  },
  bnb: {
    'BNB/USD': feed('BNB/USD', '0x0567F2323251f0Aab15c8dFb1967E4e8A7D42aeE', 8, 60),
    'ETH/USD': feed('ETH/USD', '0x9ef1B8c0E4F7dc8bF5719Ea496883DC6401d5b2e', 8, 60),
    'BTC/USD': feed('BTC/USD', '0x264990fbd0A4796A3E3d8E37C4d5F87a3aCa5Ebf', 8, 60),
    'LINK/USD': feed('LINK/USD', '0xca236E327F629f9Fc2c30A4E95775EbF0B89fac8', 8, 600),
    'USDC/USD': feed('USDC/USD', '0x51597f405303C4377E36123cBc172b13269EA163', 8, 900),
  },
  gnosis: {
    'DAI/USD': feed('DAI/USD', '0x678df3415fc31947dA4324eC63212874be5a82f8', 8, 86400, true, 'Often shared as xDAI/USD — the contract reports DAI / USD.'),
    'ETH/USD': feed('ETH/USD', '0xa767f745331D267c7751297D982b050c93985627', 8, 86400, false),
    'BTC/USD': feed('BTC/USD', '0x6C1d7e76EF7304a40e8456ce883BC56d3dEA3F7d', 8, 86400, false),
    'LINK/USD': feed('LINK/USD', '0xed322A5ac55BAE091190dFf9066760b86751947B', 8, 43200, false),
  },
  scroll: {
    'ETH/USD': feed('ETH/USD', '0x6bF14CB0A831078629D993FDeBcB182b21A8774C', 8, 86400, false),
    'BTC/USD': feed('BTC/USD', '0xCaca6BFdeDA537236Ee406437D2F8a400026C589', 8, 86400, false),
    'USDC/USD': feed('USDC/USD', '0x43d12Fb3AfCAd5347fA764EeAB105478337b7200', 8, 86400),
  },
  linea: {
    'ETH/USD': feed('ETH/USD', '0x3c6Cd9Cc7c7a4c2Cf5a82734CD249D7D593354dA', 8, 86400, false),
    'BTC/USD': feed('BTC/USD', '0x7A99092816C8BD5ec8ba229e3a6E6Da1E628E1F9', 8, 86400, false),
    'USDC/USD': feed('USDC/USD', '0xAADAa473C1bDF7317ec07c915680Af29DeBfdCb5', 8, 86400),
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

export function feedCount(): number {
  return Object.values(FEEDS).reduce((total, table) => total + Object.keys(table).length, 0)
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
  heartbeatMeasured: boolean
  stale: boolean
  /** answeredInRound < roundId — this round carried an older answer forward. */
  carriedOver: boolean
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
  const answeredInRound = toUint(roundWords[4])

  const decimals = Number(toUint(words(raw.decimals)[0]))
  const description = decodeString(raw.description).trim()

  const current = nowSecs ?? Math.floor(Date.now() / 1000)
  // A feed timestamped in the future is a clock problem, not negative age.
  const ageSecs = Math.max(0, current - updatedAt)
  const stale = ageSecs > staleAfterSecs(f)

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
    heartbeatMeasured: f.heartbeatMeasured,
    stale,
    carriedOver: answeredInRound < roundId,
    status: answer <= 0n ? 'INVALID' : stale ? 'STALE' : 'FRESH',
    note: f.note,
  }
}
