/**
 * Withdrawing in-game SCEMA as real $SCEMA. The pure half.
 *
 * Client-safe: arithmetic, address shapes and policy only. No RPC, no keys, no filesystem. The
 * server half is `lib/scemaworld/treasury.ts`, split for the same reason `lib/escrow/rpc.ts` is
 * split from `lib/escrow/program.ts`.
 *
 * ## What changed, and what it costs
 *
 * `economy.ts` used to open with a warning: SCEMA is a name and an exchange rate and nothing else,
 * and connecting it to an actual token would give the game a property it did not have — **the
 * things you do in it become worth money outside it.** That has now happened. The warning named
 * two rules that would stop being design preferences and start being load-bearing, and both were
 * re-verified rather than inherited:
 *
 * 1. *No quantity in the record may translate into a reward.* Still true, and now checked under a
 *    stronger reading: salvage comes from acts (`ship.ts`), raider and traffic density are
 *    constants (`raiders.ts`, `factions.ts`), reinforcement floors are constants (`respawn.ts`),
 *    and the withdrawal rate below reads nothing but a balance. `check:scemaworld` asserts the
 *    reward path and this file read no record field. A world with more blind spots is worth
 *    exactly the same as one with fewer, which is the property that stops a forged record paying.
 * 2. *Sealed records are the map.* A record that paid better than another would be a record worth
 *    forging. Every world pays identically, so forging one gains nothing — and a claim still
 *    names the world it came from (`world`), so the ledger is auditable per record even though
 *    nothing about the record moves the amount.
 *
 * ## The threat this does NOT solve, stated plainly because it is the important one
 *
 * **A balance is client-side.** Scema-World runs entirely in the browser: `ship.scema` lives in a
 * tab, and anyone with a developer console can set it to any number they like. Nothing in this
 * file, or anywhere else in this codebase, can tell a balance that was earned from one that was
 * typed. Making it unforgeable would mean running the simulation server-side, which is a different
 * product.
 *
 * So this is **a capped faucet, and it is described as one on screen.** What the caps buy is not
 * secrecy, it is *bounded loss*: the treasury cannot be drained by one claim, one wallet, or one
 * day, and every payout is written to a ledger before it is attempted. What they do not buy is
 * resistance to somebody making many wallets, which on Solana is cheap. If that matters more than
 * the faucet does, the honest fix is to stop paying out, not to add a harder-looking number here.
 *
 * Every limit is a constant, overridable by the operator through the environment, and all of them
 * are reported by `GET /api/scemaworld/treasury` — a cap the player cannot see is a cap that reads
 * as the button being broken.
 */

/** Base58 at pubkey length, no 0/O/I/l. A shape test, not a validity claim. */
export const ADDRESS_RE = /^[1-9A-HJ-NP-Za-km-z]{32,44}$/

export function looksLikeAddress(s: string): boolean {
  return ADDRESS_RE.test(s.trim())
}

/** The $SCEMA mint. The one address in this file that is not a policy number. */
export const SCEMA_MINT = 'HcsHqEJ9suf4oHJ8mb52M7AVKjhYhnTaeHgTmde7pump'

/** The treasury that pays claims. Public by nature — it is a wallet address, not a secret. */
export const TREASURY = 'FCm6Yn1Xmv8XqVx9jnC2pMnff2ZkMC6gShpfRS9ETJ1C'

/**
 * The policy. Whole tokens, never base units — see `Entitlement` for why the two are kept apart.
 *
 * Deliberately conservative against a treasury of roughly ninety thousand. The per-wallet lifetime
 * cap is the number that decides how much a single forged balance is worth, and the budget is the
 * number that decides how much *every* forged balance together is worth. Neither is a guess about
 * how honest people are; they are the two figures that bound the loss when somebody is not.
 */
export interface Policy {
  /** Whole $SCEMA per unit of in-game SCEMA. */
  rate: number
  /** Most a single claim may pay. */
  perClaim: number
  /** Most one wallet may ever be paid, across every claim it makes. */
  perWallet: number
  /** Most this deployment will ever pay out in total, across every wallet. */
  budget: number
  /** Milliseconds a wallet must wait between claims. */
  cooldownMs: number
  /** Smallest claim worth a transaction — below this the network fee dominates. */
  minimum: number
}

export const DEFAULT_POLICY: Policy = {
  // One for one. A rate that is not one invites the question "why that number", and there is no
  // answer to it that is not arbitrary — the *caps* are where the conservatism lives, and putting
  // it in the rate as well would be the same conservatism applied twice and legible nowhere.
  rate: 1,
  perClaim: 250,
  perWallet: 1_000,
  // A tenth of a ninety-thousand treasury. The remainder is not reserved for anything in
  // particular; it is simply not exposed to a bug or a raid in this path.
  budget: 9_000,
  cooldownMs: 6 * 60 * 60 * 1_000,
  minimum: 10,
}

/** What a wallet has already been paid, and when. The ledger's per-wallet row. */
export interface WalletRecord {
  /** Whole tokens paid, cumulative. */
  paid: number
  /** Milliseconds since the epoch of the last settled claim. */
  lastMs: number
  claims: number
}

export const NO_RECORD: WalletRecord = { paid: 0, lastMs: -1, claims: 0 }

/**
 * Why a claim was refused. Each is a different instruction to the player, and that is the whole
 * reason this is an enumeration rather than a boolean.
 *
 * `not_configured` is the one that is about the deployment rather than the player: this build can
 * read the treasury but has no signer, so it can say what it *would* pay and cannot pay it. It is
 * reported as exactly that and never as a failure the player caused — and never, ever as a
 * success, which is the one answer that would matter.
 */
export type Refusal =
  | 'bad_wallet'
  | 'nothing_to_claim'
  | 'below_minimum'
  | 'wallet_limit'
  | 'cooling_down'
  | 'budget_exhausted'
  | 'treasury_short'

export interface Entitlement {
  /** Whole tokens this claim would pay. Zero whenever `refusal` is set. */
  tokens: number
  /** In-game SCEMA the claim consumes. Always `tokens / rate`, and never more than asked for. */
  spend: number
  refusal: Refusal | null
  /** A sentence for the player, always present — a refusal with no reason reads as a dead button. */
  message: string
  /** Milliseconds until the next claim is allowed, when `cooling_down`. */
  waitMs: number
}

function plural(n: number, one: string): string {
  return `${n} ${one}${n === 1 ? '' : 's'}`
}

/**
 * What a claim would pay, given everything that bounds it.
 *
 * Pure and total: every path returns an `Entitlement` with a message, including the ones nobody
 * expects to hit. The server calls this and then *calls it again* against the ledger it just
 * reserved against — one implementation of the policy, consulted at both ends, because a preview
 * that computes the limits differently from the payer is a preview that lies exactly when it
 * matters.
 *
 * `treasury` is the treasury's actual on-chain balance in whole tokens, floored. It is checked
 * last and separately from the budget because "this deployment has paid out its allowance" and
 * "the treasury is empty" are different facts about different things, and only one of them is
 * something the operator can fix by editing a config.
 */
export function entitlement(
  args: {
    /** In-game SCEMA the player is offering. */
    scema: number
    wallet: string
    /** What this wallet has already been paid. */
    record: WalletRecord
    /** Total paid out by this deployment, all wallets. */
    dispensed: number
    /** The treasury's real balance, in whole tokens. */
    treasury: number
    nowMs: number
  },
  policy: Policy = DEFAULT_POLICY,
): Entitlement {
  const none = (refusal: Refusal, message: string, waitMs = 0): Entitlement => ({
    tokens: 0,
    spend: 0,
    refusal,
    message,
    waitMs,
  })

  if (!looksLikeAddress(args.wallet)) {
    return none('bad_wallet', 'that does not look like a Solana address')
  }

  const offered = Math.floor(Math.max(0, args.scema))
  if (offered <= 0) return none('nothing_to_claim', 'no SCEMA to withdraw')

  const sinceLast = args.nowMs - args.record.lastMs
  if (args.record.lastMs > 0 && sinceLast < policy.cooldownMs) {
    const waitMs = policy.cooldownMs - sinceLast
    const hours = Math.ceil(waitMs / 3_600_000)
    return none('cooling_down', `next withdrawal in ${plural(hours, 'hour')}`, waitMs)
  }

  const walletLeft = policy.perWallet - args.record.paid
  if (walletLeft <= 0) {
    return none(
      'wallet_limit',
      `this wallet has drawn its lifetime limit of ${policy.perWallet} $SCEMA`,
    )
  }

  const budgetLeft = policy.budget - args.dispensed
  if (budgetLeft <= 0) {
    return none('budget_exhausted', 'this deployment has paid out its whole allowance')
  }

  // Every bound applied at once, then floored. Applying them one at a time and returning early on
  // each would report the *first* limit reached rather than the one that actually binds, and a
  // player told "the per-claim cap is 250" while a lifetime cap of 40 is what stopped them has
  // been told something true and useless.
  const tokens = Math.floor(
    Math.min(offered * policy.rate, policy.perClaim, walletLeft, budgetLeft, args.treasury),
  )

  if (tokens <= 0) {
    // Reachable only through the treasury, since the other four are checked above.
    return none('treasury_short', 'the treasury cannot cover a withdrawal right now')
  }
  if (tokens < policy.minimum) {
    return none(
      'below_minimum',
      `${policy.minimum} $SCEMA is the smallest withdrawal — a smaller one costs more in ` +
        'network fees than it is worth',
    )
  }

  const binding =
    tokens >= offered * policy.rate
      ? null
      : tokens === Math.floor(policy.perClaim)
        ? `capped at ${policy.perClaim} per withdrawal`
        : tokens === Math.floor(walletLeft)
          ? `capped at this wallet's remaining ${walletLeft}`
          : tokens === Math.floor(budgetLeft)
            ? "capped at this deployment's remaining allowance"
            : 'capped by the treasury balance'

  return {
    tokens,
    // Only what actually converted is spent. Charging the remainder would take SCEMA and give
    // nothing for it — the same bug `economy.ts::exchange` documents, in a place where the thing
    // taken has a market price.
    spend: Math.ceil(tokens / policy.rate),
    refusal: null,
    message: binding ? `${tokens} $SCEMA — ${binding}` : `${tokens} $SCEMA`,
    waitMs: 0,
  }
}

/**
 * Base units for a whole-token amount, as a **decimal string**.
 *
 * `bigint` throughout and a string on the wire, never a JS number. A u64 reaches about 1.8e19
 * against `Number.MAX_SAFE_INTEGER`'s 9e15, so a nine-decimal mint loses precision at ten million
 * tokens and a six-decimal one at nine billion — and the loss is silent, in a quantity of money.
 * Same rule as `/escrow`, which is where it was learned.
 */
export function toBaseUnits(tokens: number, decimals: number): bigint {
  if (!Number.isInteger(tokens) || tokens < 0) {
    throw new Error('a token amount must be a whole non-negative number')
  }
  return BigInt(tokens) * 10n ** BigInt(decimals)
}

/**
 * Whole tokens from base units, **floored**.
 *
 * Floored, always, and in both directions of use: a treasury reported as 90,000.9 must never buy a
 * 90,000-token claim it cannot settle, and a player must never be shown a balance the treasury
 * does not hold. Rounding a balance up is how a page ends up promising money that is not there.
 */
export function toWholeTokens(base: bigint, decimals: number): number {
  return Number(base / 10n ** BigInt(decimals))
}

/**
 * The line the market prints about what SCEMA is now.
 *
 * On screen, not only in a comment — the same rule the old placeholder note followed, and for a
 * sharper reason: the previous version told the player the currency was worth nothing outside the
 * session, and that sentence is now false. A player who is told a currency is real has been told;
 * one who infers it later has been misled, and one still reading the *old* line has been misled by
 * us specifically.
 */
export const SCEMA_NOTE =
  'SCEMA is redeemable for the $SCEMA token from a fixed treasury, at a published rate and under ' +
  'published caps. Your balance lives in this browser tab, so withdrawals are capped per wallet ' +
  'and per deployment — see the treasury panel for the exact limits and what is left.'
