/**
 * The pilot's account, kept across sessions.
 *
 * ## Why this exists
 *
 * Everything a player earned lived in `GameState.ship`, which `newGame` builds from
 * `newShip()` — `salvage: 0`, `scema: 0`, a stock skiff. So a session's entire earnings ended
 * at the next page load, and dropping a different record wiped them too.
 *
 * That is not a small quality-of-life gap now that SCEMA leaves the game. The withdrawal path
 * (`claim.ts`, `treasury.ts`) is real and works, and the minimum claim is ten SCEMA — but a
 * balance that resets whenever the tab does can only ever be spent in the session that earned
 * it, so for most players the thing the token is *for* was unreachable. **Earning has to
 * accumulate before it can mean anything.**
 *
 * ## What is kept, and what is deliberately not
 *
 * The account is what the player *owns*: the two balances, the hull they bought, and the
 * component levels they paid for. Those are all purchases made with earnings, and losing a hull
 * bought for SCEMA destroys money as surely as losing the SCEMA would.
 *
 * Everything else about the ship is **state, not property**, and is deliberately rebuilt: fuel,
 * hull integrity, shields, jump charges, which node you were docked at, which derelicts you
 * already stripped. Persisting those would mean a player could reload out of a fight, or bank a
 * stripped-derelict list against a world where those node ids mean something different. A
 * session restores what you own and starts you fresh, fuelled, at the origin.
 *
 * ## One account, not one per world
 *
 * Not keyed by the record. Every world pays identically — that is the invariant `economy.ts` and
 * `raiders.ts` exist to protect — so a balance is not a claim about any particular record and
 * there is nothing to keep separate. Keying by world would also punish a player for opening a
 * second record, and would hand them a reason to hoard the one they had earned in, which is the
 * closest thing to "a record is worth money" this design is trying not to build.
 *
 * ## What this does not change
 *
 * A balance in a browser is forgeable, exactly as forgeable as it was before it was written to
 * disk. Persisting it does not make it more trustworthy and nothing here pretends otherwise —
 * the bound on loss is still the capped faucet in `claim.ts`, enforced server-side against a
 * ledger the browser cannot reach. Storage is a convenience for the honest player; it is not a
 * security boundary and must never be treated as one.
 */

import {
  fuelCapacity, hullMax, jumpCapacity, MAX_LEVEL, newShip, shieldMax,
  type Component, type Ship,
} from './ship.ts'
import { HULLS, type HullId } from './hulls.ts'

/**
 * Where the account lives.
 *
 * Versioned in the key itself rather than only in the payload, so a future shape change is a
 * clean miss — the old value is simply not found — instead of a parse that half-succeeds and
 * restores a balance from fields that have since changed meaning.
 */
export const WALLET_KEY = 'scemaworld.account.v1'

/** What is written down. Integers only; see `sanitise`. */
export interface Account {
  salvage: number
  scema: number
  frame: HullId
  levels: Record<Component, number>
}

/** A fresh account, for a player who has never flown or whose storage cannot be read. */
export function newAccount(): Account {
  const s = newShip()
  return { salvage: 0, scema: 0, frame: s.frame, levels: { ...s.levels } }
}

/**
 * Coerce anything at all into a valid account.
 *
 * Every field is checked rather than trusted, and the reason is not tamper-resistance — a player
 * who edits their own storage has simply given themselves salvage, and the faucet caps are what
 * bound that. It is that a **malformed** value must not reach the game as a number: a `NaN`
 * balance propagates through every arithmetic in `economy.ts` and renders as `NaN SCEMA` on a
 * panel that is otherwise making claims about money, and a negative one would let `exchange`
 * mint. Unparseable input yields a fresh account; a partially valid one keeps what parsed.
 *
 * Exported because it is the whole of the risk in this module and deserves to be tested directly.
 */
export function sanitise(raw: unknown): Account {
  const base = newAccount()
  if (!raw || typeof raw !== 'object') return base
  const o = raw as Record<string, unknown>

  const count = (v: unknown): number => {
    const n = typeof v === 'number' ? v : Number(v)
    if (!Number.isFinite(n) || n <= 0) return 0
    // `Number.MAX_SAFE_INTEGER` rather than no ceiling: past it, integer arithmetic stops being
    // exact and a balance would drift under its own addition.
    return Math.min(Math.floor(n), Number.MAX_SAFE_INTEGER)
  }

  const frame =
    typeof o.frame === 'string' && Object.hasOwn(HULLS, o.frame) ? (o.frame as HullId) : base.frame

  const levels = { ...base.levels }
  if (o.levels && typeof o.levels === 'object') {
    const src = o.levels as Record<string, unknown>
    for (const key of Object.keys(levels) as Component[]) {
      levels[key] = Math.min(count(src[key]), MAX_LEVEL)
    }
  }

  return { salvage: count(o.salvage), scema: count(o.scema), frame, levels }
}

/**
 * The storage seam.
 *
 * A two-method interface rather than `localStorage` directly, so the policy above is testable
 * with no browser — the same split as `claim.ts` (pure) against `treasury.ts` (chain). It is the
 * reason `check:scemaworld` can assert that a corrupt value yields a zero balance rather than a
 * crash, which is a thing nobody would otherwise find out until it happened to a player.
 */
export interface Store {
  get(key: string): string | null
  set(key: string, value: string): void
}

/**
 * `localStorage`, when there is one that works.
 *
 * Returns `null` rather than throwing when there is not. Storage is genuinely unavailable in a
 * private window, with site data blocked, and inside some embedded webviews — and in those the
 * *accessor itself* throws, not just the read. A game that fails to start because it could not
 * save is strictly worse than one that runs and cannot remember.
 */
export function browserStore(): Store | null {
  try {
    const ls = globalThis.localStorage
    if (!ls) return null
    // Prove it actually works. Some browsers expose the object and throw on write.
    const probe = `${WALLET_KEY}.probe`
    ls.setItem(probe, '1')
    ls.removeItem(probe)
    return {
      get: (k) => ls.getItem(k),
      set: (k, v) => ls.setItem(k, v),
    }
  } catch {
    return null
  }
}

/** Read the account. Any failure at all is a fresh account, never an exception. */
export function load(store: Store | null): Account {
  if (!store) return newAccount()
  try {
    const raw = store.get(WALLET_KEY)
    if (!raw) return newAccount()
    return sanitise(JSON.parse(raw))
  } catch {
    return newAccount()
  }
}

/**
 * Write the account. Best effort, and a failure is silent by design.
 *
 * There is nothing useful to tell a player when a save fails — they cannot fix a full quota or a
 * blocked-storage setting from inside a space sim — and the alternative, a banner over the game,
 * would fire continuously on exactly the browsers where it is least actionable. What a failed
 * save costs is progress, in the safe direction: the balance in the tab is unaffected.
 */
export function save(store: Store | null, account: Account): void {
  if (!store) return
  try {
    store.set(WALLET_KEY, JSON.stringify(account))
  } catch {
    /* full quota, blocked storage, private mode. The session continues. */
  }
}

/** What a ship holds, as an account. */
export function accountOf(ship: Ship): Account {
  return { salvage: ship.salvage, scema: ship.scema, frame: ship.frame, levels: { ...ship.levels } }
}

/**
 * Restore an account onto a freshly built ship.
 *
 * **A merge into a new ship, never a patch of a live one.** Called exactly once, immediately
 * after `newGame`, before the frame loop starts. Applying a stored balance to a ship that is
 * already flying would let a reload resurrect money that had since been spent — and since the
 * write happens on change, that is a duplication a player could drive deliberately.
 *
 * Consumables are recomputed **from the restored levels and the restored frame**, so a returning
 * pilot arrives with the tanks and hull their upgrades entitle them to. Both arguments matter:
 * every capacity here is a function of the level *and* the hull, so recomputing a marauder's fuel
 * against a skiff would hand a returning player a fraction of the tank they paid for.
 */
export function restore(ship: Ship, account: Account): Ship {
  const levels = { ...account.levels }
  const frame = account.frame
  return {
    ...ship,
    salvage: account.salvage,
    scema: account.scema,
    frame,
    levels,
    fuel: fuelCapacity(levels.tanks, frame),
    hull: hullMax(levels.hull, frame),
    shield: shieldMax(levels.shields, frame),
    jumpFuel: jumpCapacity(levels.drive, frame),
  }
}

/**
 * Whether two accounts differ in anything worth a write.
 *
 * The frame loop runs at 60 Hz and the account is checked against it, so this is what stops a
 * `JSON.stringify` and a synchronous storage write happening sixty times a second forever.
 * `localStorage` writes block the main thread; doing one per frame is a visible stutter.
 */
export function changed(a: Account, b: Account): boolean {
  if (a.salvage !== b.salvage || a.scema !== b.scema || a.frame !== b.frame) return true
  for (const key of Object.keys(a.levels) as Component[]) {
    if (a.levels[key] !== b.levels[key]) return true
  }
  return false
}
