'use client'

import type { FeedFailure, FeedReading } from './feeds'
import { usePoll, type Snapshot } from '../store'

// Polling hooks for the alchem-link console.
//
// These go through the shared store in `lib/store.ts` for the same reason every other
// panel does: one timer per key, refcounted so a panel that is not on screen stops
// fetching. The keys are network-scoped (`alchem:feeds:base`) so switching networks
// starts a distinct poll instead of overwriting the previous network's data — and so the
// old network's timer is torn down when its last subscriber unmounts.

export const ALCHEM_POLL_MS = {
  /** Aggregators update on the order of minutes; a faster poll only burns rate limit. */
  feeds: 20_000,
  /** Endpoint health moves slowly and every check costs a round trip. */
  doctor: 30_000,
  /** The registry is static — this re-reads the chain, so it is the most expensive call. */
  verify: 120_000,
} as const

export interface FeedsPayload {
  ok: boolean
  network: string
  networkLabel: string
  chainId: number
  explorer: string
  endpoint: string
  endpointSource: string
  authenticated: boolean
  readAt: number
  readings: FeedReading[]
  failures: FeedFailure[]
  error?: string
}

export interface Check {
  name: string
  ok: boolean
  detail: string
  hint?: string
}

export interface DoctorPayload {
  ok: boolean
  network: string
  networkLabel: string
  endpoint: string
  endpointSource: string
  authenticated: boolean
  checks: Check[]
  error?: string
}

export interface VerifyEntry {
  pair: string
  address: string
  ok: boolean
  description?: string
  decimals?: number
  declaredDecimals: number
  price?: number
  status?: string
  error?: string
}

export interface VerifyPayload {
  ok: boolean
  network: string
  networkLabel: string
  endpoint: string
  endpointSource: string
  entries: VerifyEntry[]
  error?: string
}

/**
 * A 502 from these routes still carries a useful body — the endpoint that failed and
 * why. Returning the parsed payload instead of null lets the panel print the actual
 * error rather than a generic "offline", which is the whole point of the doctor view.
 */
async function readJson<T>(url: string): Promise<T | null> {
  try {
    const response = await fetch(url, { cache: 'no-store' })
    const body = (await response.json()) as T
    return body ?? null
  } catch {
    return null
  }
}

export const useAlchemFeeds = (network: string): Snapshot<FeedsPayload> =>
  usePoll(
    `alchem:feeds:${network}`,
    () => readJson<FeedsPayload>(`/api/alchem/feeds?network=${encodeURIComponent(network)}`),
    ALCHEM_POLL_MS.feeds,
  )

export const useAlchemDoctor = (network: string): Snapshot<DoctorPayload> =>
  usePoll(
    `alchem:doctor:${network}`,
    () => readJson<DoctorPayload>(`/api/alchem/doctor?network=${encodeURIComponent(network)}`),
    ALCHEM_POLL_MS.doctor,
  )

export const useAlchemVerify = (network: string): Snapshot<VerifyPayload> =>
  usePoll(
    `alchem:verify:${network}`,
    () => readJson<VerifyPayload>(`/api/alchem/verify?network=${encodeURIComponent(network)}`),
    ALCHEM_POLL_MS.verify,
  )
