// Multi-tab: exactly one writer, elected.
//
// ── The problem, which is the `reserve()` race with a new front door ─────────
//
// Two tabs open on /zero share one session key, one budget and one set of positions —
// because all three live in the same origin's storage. Without an election they also
// share nothing else: each tab reads the ledger, decides against it, and writes back.
// Two entries authorised in the same second each measure themselves against a budget the
// other has already taken, and the ledger afterwards reads exactly at the cap, so nothing
// downstream ever sees a figure that looks wrong.
//
// `session.ts::authorise` closes that *within* a tab (single-threaded, no await). It
// cannot close it *across* tabs, because two tabs are two threads with one storage.
//
// ── Why Web Locks and not a storage flag ─────────────────────────────────────
//
// A `localStorage` "I am the leader" flag is not a lock: writing it is not atomic with
// reading it, and a tab that crashes leaves it set forever, which is worse than no lock
// at all — Zero would then refuse to trade in every tab until somebody cleared storage.
//
// `navigator.locks.request` with `mode: 'exclusive'` is held for the lifetime of the
// callback and is released BY THE BROWSER when the tab dies. That is the property that
// matters: the failure mode of a crashed leader is automatic re-election, not a wedged
// deployment.
//
// ── What this does NOT buy ───────────────────────────────────────────────────
//
// Two browsers, two profiles, or a browser and a phone do not share storage, so they do
// not share a ledger either — and the budget is multiplied by the number of them. That is
// a property of keeping the ledger in browser storage, exactly as the treasury's file
// ledger is multiplied by instances that do not share a filesystem. It is stated rather
// than mitigated, because the mitigation is a server and Zero does not have one.

export const ZERO_LOCK = 'scematica-zero-writer'

export type LeaseState = 'writer' | 'follower' | 'unsupported'

export interface Lease {
  state: LeaseState
  /** Release the lock and stop being the writer. Idempotent. */
  release: () => void
}

interface LockManagerLike {
  request(
    name: string,
    options: { mode: 'exclusive'; signal?: AbortSignal },
    fn: () => Promise<void>,
  ): Promise<void>
}

function lockManager(): LockManagerLike | null {
  const nav = globalThis.navigator as unknown as { locks?: LockManagerLike } | undefined
  return nav?.locks ?? null
}

/**
 * Try to become the single writer.
 *
 * `onAcquire` fires when this tab holds the lock; `onLost` when it is given up. The lock
 * is held until the returned `release()` is called or the tab dies.
 *
 * **When the Web Locks API is missing, this returns `unsupported` and the caller must
 * treat that as NOT being the writer.** Assuming leadership on an unknown platform is the
 * unsafe default: it lets every tab on that browser trade simultaneously against one
 * budget. Refusing costs an operator on an old browser the ability to trade, which is
 * recoverable; the other error is not.
 */
export function acquireLease(onAcquire: () => void, onLost: () => void): Lease {
  const locks = lockManager()
  if (!locks) {
    return { state: 'unsupported', release: () => {} }
  }

  const controller = new AbortController()
  let released = false
  const lease: Lease = {
    state: 'follower',
    release: () => {
      if (released) return
      released = true
      controller.abort()
    },
  }

  // The promise inside the callback is what holds the lock: it resolves only when
  // `release()` aborts, so the lock is held for as long as this tab wants to be writer.
  void locks
    .request(ZERO_LOCK, { mode: 'exclusive', signal: controller.signal }, async () => {
      lease.state = 'writer'
      onAcquire()
      await new Promise<void>(resolve => {
        if (released) return resolve()
        controller.signal.addEventListener('abort', () => resolve(), { once: true })
      })
    })
    .catch(() => {
      // AbortError on release, or the request was rejected. Either way: not the writer.
    })
    .finally(() => {
      if (lease.state === 'writer') {
        lease.state = 'follower'
        onLost()
      }
    })

  return lease
}

/**
 * What a follower tab may do.
 *
 * Read everything, decide nothing. A follower still renders positions, Ψ, the price feed
 * and the readout — a second tab showing "another tab is trading" and nothing else is a
 * broken-looking page, and an operator will close the *writer* to fix it.
 *
 * The rule is narrow and total: a follower emits no `swap` effect. `engine.ts` enforces
 * it centrally rather than at each call site, because "check the lease" repeated at four
 * sites is three chances to forget.
 */
export function mayTrade(state: LeaseState): boolean {
  return state === 'writer'
}

export function leaseNote(state: LeaseState): string {
  switch (state) {
    case 'writer':
      return 'This tab is the writer.'
    case 'follower':
      return 'Another tab is trading. This one is read-only — positions and prices are live.'
    case 'unsupported':
      return 'This browser has no Web Locks API, so Zero cannot guarantee a single writer across tabs. Trading is disabled; reading is not.'
  }
}
