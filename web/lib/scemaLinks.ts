import { SCEMA_MINT as CA } from './ScemaGateContext'

// Where "buy $SCEMA" sends people.
//
// One module, because these URLs were previously copy-pasted across CABanner, Links and
// GatedControls, and a venue that stops working has to be fixed in one place.
//
// **Jupiter is the primary buy link, not pump.fun.** Both reach the same liquidity — a
// SOL→SCEMA quote routes through Pump.fun either way while the token is still on its
// bonding curve — but pump.fun is unreachable for a meaningful slice of visitors:
//
//   * it geo-blocks several jurisdictions outright (the UK among them),
//   * and its domain is on common adblock/DNS filter lists,
//
// both of which surface to the user as a 404 or a dead page rather than an honest
// "unavailable in your region". Jupiter is neither geo-blocked nor filtered, aggregates
// across venues, and keeps working unchanged after the token graduates off the curve —
// at which point a pump.fun coin link genuinely does become the wrong destination.
//
// pump.fun stays as a secondary link for anyone who wants the bonding-curve UI.

/** Primary buy destination — pre-fills a SOL → SCEMA swap. */
export const BUY_URL = `https://jup.ag/swap/SOL-${CA}`

/** Jupiter's token page: price, chart, and holders, server-rendered. */
export const TOKEN_URL = `https://jup.ag/tokens/${CA}`

/** The bonding-curve UI. Secondary: see the geo-blocking note above. */
export const PUMPFUN_URL = `https://pump.fun/coin/${CA}`

/** Chart. Dexscreener resolves a mint to its top pair, so this survives graduation. */
export const DEXSCREENER_URL = `https://dexscreener.com/solana/${CA}`

export { CA as SCEMA_MINT }
