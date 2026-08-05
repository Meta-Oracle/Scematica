// Display helpers, ported from the TUI so a price reads identically in both.

import type { FeedStatus } from './feeds'

/**
 * Significant digits scale with magnitude: an index price does not need eight decimals,
 * and a stablecoin depeg is invisible without them.
 */
export function fmtPrice(value: number): string {
  const magnitude = Math.abs(value)
  if (magnitude >= 1000) return value.toLocaleString('en-US', { minimumFractionDigits: 2, maximumFractionDigits: 2 })
  if (magnitude >= 1) return value.toLocaleString('en-US', { minimumFractionDigits: 4, maximumFractionDigits: 4 })
  return value.toLocaleString('en-US', { minimumFractionDigits: 8, maximumFractionDigits: 8 })
}

export function fmtAge(seconds: number): string {
  if (seconds < 60) return `${seconds}s`
  if (seconds < 3600) return `${Math.floor(seconds / 60)}m ${seconds % 60}s`
  return `${Math.floor(seconds / 3600)}h ${Math.floor((seconds % 3600) / 60)}m`
}

/** Status → text colour. Mirrors STATUS_COLOUR in `alchem_link/theme.py`. */
export const STATUS_TEXT: Record<FeedStatus, string> = {
  FRESH: 'text-alchem-green',
  STALE: 'text-alchem-amber',
  INVALID: 'text-alchem-red',
}

/** Status → badge chrome, for the pill next to a reading. */
export const STATUS_BADGE: Record<FeedStatus, string> = {
  FRESH: 'text-alchem-green border-alchem-green/40 bg-alchem-green/10',
  STALE: 'text-alchem-amber border-alchem-amber/40 bg-alchem-amber/10',
  INVALID: 'text-alchem-red border-alchem-red/40 bg-alchem-red/10',
}

export function shortAddress(address: string): string {
  return `${address.slice(0, 6)}…${address.slice(-4)}`
}

/**
 * How far through its heartbeat a feed is, clamped to [0, 1].
 *
 * Deliberately linear against the *declared* heartbeat rather than some smoothed
 * "confidence" number: the bar has to mean one checkable thing, which is how close this
 * reading is to the interval it is contractually expected to beat.
 */
export function heartbeatProgress(ageSecs: number, heartbeatSecs: number): number {
  if (heartbeatSecs <= 0) return 0
  return Math.min(1, Math.max(0, ageSecs / heartbeatSecs))
}
