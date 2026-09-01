/**
 * Fetching a world you hold the token for.
 *
 * The PNG names a world; `scema-vault` serves it to the address holding the token that
 * commits to it. This is the client half, and it is the only place in Scema-World that talks
 * to a network at all — everything else is a pure function of a file the player already has.
 *
 * ## Three answers, carried all the way to the player
 *
 * `scema-entitlement` distinguishes denied from undetermined, and `scema-vault` turns that
 * into 403 versus 503. Collapsing them here would undo the whole point: told "you do not own
 * this", somebody goes and buys a token they already have. So [`VaultResult`] keeps the
 * distinction, and the one that means "the chain could not be read" says *retry*.
 *
 * ## What is not trusted
 *
 * The vault serves bytes; it does not certify them. The record is verified in the browser
 * exactly as a dropped file is, with the same `verifyRecordText`, and the returned digest is
 * checked against the one that was asked for. A vault that returned a different world — by
 * bug or by design — is caught here rather than silently flown.
 */

import type { Verification } from '../omni/verify.ts'

export interface VaultRecord {
  /** The record text, verbatim. Verified by the caller; never parsed here. */
  text: string
  commitment: string
}

export type VaultResult =
  | { kind: 'ok'; record: VaultRecord }
  /** A fact about the holder. Acting on it means acquiring the token, or asking for another world. */
  | { kind: 'denied'; detail: string }
  /** A fact about the infrastructure. **Not** a denial — retry. */
  | { kind: 'undetermined'; detail: string }
  /** The vault does not have it, which is a gap in the vault rather than in the entitlement. */
  | { kind: 'absent'; detail: string }
  /** The vault could not be reached or answered with something unusable. */
  | { kind: 'unreachable'; detail: string }
  /** It answered, and the bytes are not the world that was asked for. */
  | { kind: 'mismatch'; detail: string }

/** Trim a trailing slash so `base + path` never doubles it. */
function normalise(base: string): string {
  return base.replace(/\/+$/, '')
}

/**
 * Ask a vault for the record behind a commitment.
 *
 * `holder` is the address claiming to hold the token; the vault decides whether it does.
 * Nothing here signs anything — proving control of an address is `scema-entitlement`'s
 * challenge flow, and a browser game is not where a key should be.
 */
export async function fetchWorld(
  base: string,
  commitment: string,
  holder: string,
  fetchImpl: typeof fetch = fetch,
): Promise<VaultResult> {
  const url = `${normalise(base)}/world/${commitment}`
  let res: Response
  try {
    res = await fetchImpl(url, { headers: { 'X-Scema-Holder': holder } })
  } catch (e) {
    return {
      kind: 'unreachable',
      detail: `${url} could not be reached: ${e instanceof Error ? e.message : String(e)}`,
    }
  }

  const body = await res.text()
  if (res.status === 200) {
    return { kind: 'ok', record: { text: body, commitment } }
  }

  let detail = body
  try {
    const parsed = JSON.parse(body) as { detail?: string; error?: string }
    detail = parsed.detail ?? parsed.error ?? body
  } catch {
    // A vault behind a login page answers HTML. Keep the raw body rather than inventing a
    // reason for it — "unreachable" with the text is more useful than a guessed error.
  }

  switch (res.status) {
    case 403:
      return { kind: 'denied', detail }
    // Never folded into `denied`. An RPC timeout is not a fact about the holder.
    case 503:
      return { kind: 'undetermined', detail }
    case 404:
      return { kind: 'absent', detail }
    case 401:
      return { kind: 'denied', detail: detail || 'no holder address was sent' }
    default:
      return { kind: 'unreachable', detail: `${res.status}: ${detail}` }
  }
}

/**
 * What the player is told, per outcome.
 *
 * One place, so a wording change cannot accidentally turn "retry" into "you do not own this".
 */
export function explain(r: VaultResult): string {
  switch (r.kind) {
    case 'ok':
      return 'fetched'
    case 'denied':
      return `Denied — ${r.detail}`
    case 'undetermined':
      return `Undetermined — ${r.detail} This is not a denial. Try again shortly.`
    case 'absent':
      return `Not stored — ${r.detail} That is a gap in the vault, not in your entitlement.`
    case 'mismatch':
      return `Wrong world — ${r.detail}`
    case 'unreachable':
      return `Unreachable — ${r.detail}`
  }
}

/** Whether the player should be invited to retry rather than to go and buy something. */
export function retryable(r: VaultResult): boolean {
  return r.kind === 'undetermined' || r.kind === 'unreachable'
}

/**
 * Confirm a fetched record is the world that was asked for.
 *
 * The vault is not trusted to return the right thing. `verify` is the caller's existing
 * browser-side check; this adds the binding between *what was requested* and *what arrived*,
 * which no signature on the record itself can provide.
 */
export function matchesRequest(
  requested: string,
  actual: string,
  verification: Verification,
): VaultResult | null {
  if (actual !== requested) {
    return {
      kind: 'mismatch',
      detail: `asked for ${requested.slice(0, 16)}… and received ${actual.slice(0, 16)}…`,
    }
  }
  if (!verification.valid) {
    return {
      kind: 'mismatch',
      detail: 'the record does not match its own commitment — it was edited after sealing',
    }
  }
  return null
}
