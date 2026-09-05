// Scema-World — the $SCEMA treasury. **Server-only.**
//
// Reads `SCEMAWORLD_RPC_ENDPOINT` then `RPC_ENDPOINT` (either carries a provider key) and
// `SCEMAWORLD_TREASURY_SECRET` (a signing key), so this must never be imported from a client
// component. Same guard and same reasoning as `lib/escrow/rpc.ts` and `lib/alchem/endpoint.ts`:
// Next only inlines `NEXT_PUBLIC_*` into client bundles, so a stray import would not leak the
// secret — it would silently fall back to nothing, which is the more confusing failure. The throw
// makes it loud.
//
// **There is no simulation branch in this file, by design**, and the reason is stronger here than
// on `/escrow`. A fabricated reserve figure misleads a reader; a fabricated *payout* tells
// somebody they have been sent money that was never sent. Every read is a chain read or an error;
// every write is a signed transaction or a refusal. A build with no signer answers 501 and says
// exactly that.
//
// WHAT THIS FILE CANNOT DO, restated from `claim.ts` because it is the thing an operator most
// needs to have understood before setting the secret: the balance a claim is made against lives
// in the player's browser and is trivially forgeable. The caps are what bound the loss. Setting
// `SCEMAWORLD_TREASURY_SECRET` is the decision to run a capped public faucet, and it should be
// made deliberately, with a key that holds only what the operator is willing to distribute.

import { readFile, writeFile, rename, mkdir, open, unlink, stat } from 'node:fs/promises'
import { dirname, join } from 'node:path'

import { Connection, Keypair, PublicKey, Transaction } from '@solana/web3.js'
import {
  ASSOCIATED_TOKEN_PROGRAM_ID,
  TOKEN_2022_PROGRAM_ID,
  TOKEN_PROGRAM_ID,
  createAssociatedTokenAccountInstruction,
  createTransferCheckedInstruction,
  getAssociatedTokenAddressSync,
} from '@solana/spl-token'
import bs58 from 'bs58'

import { decodeMint, programKind, type TokenProgramKind } from '../escrow/mintinfo.ts'
import {
  DEFAULT_POLICY,
  NO_RECORD,
  SCEMA_MINT,
  TREASURY,
  entitlement,
  toBaseUnits,
  toWholeTokens,
  type Entitlement,
  type Policy,
  type WalletRecord,
} from './claim.ts'

if (typeof window !== 'undefined') {
  throw new Error(
    'lib/scemaworld/treasury.ts is server-only — import lib/scemaworld/claim.ts from components',
  )
}

/** Public mainnet fallback, used only when no keyed endpoint is configured. */
const PUBLIC_FALLBACK = 'https://solana-rpc.publicnode.com'

function connection(): { conn: Connection; host: string; authenticated: boolean } {
  // Scema-World first, shared second. Precedence rather than replacement, so a deployment that
  // only sets `RPC_ENDPOINT` keeps working untouched — and one that wants the game pointed at a
  // different cluster from the sniper API can say so without dragging everything else along.
  const raw =
    process.env.SCEMAWORLD_RPC_ENDPOINT?.trim() || process.env.RPC_ENDPOINT?.trim()
  const url = raw || PUBLIC_FALLBACK
  let host = 'unknown'
  try {
    host = new URL(url).host
  } catch {
    /* keep 'unknown' rather than echoing a malformed string that may carry a key */
  }
  return { conn: new Connection(url, 'confirmed'), host, authenticated: Boolean(raw) }
}

/**
 * The policy in force, with every limit overridable from the environment.
 *
 * Overridable because these are an operator's risk appetite, not a design constant — and a
 * deployment that has to edit a source file to lower a cap is a deployment that will not lower it.
 * A malformed or negative value falls back to the default rather than being coerced: `Number('')`
 * is 0, and a budget silently read as zero would take the faucet offline in a way that looks like
 * a bug in the chain read.
 */
export function policy(): Policy {
  const num = (name: string, fallback: number): number => {
    const raw = process.env[name]?.trim()
    if (!raw) return fallback
    const v = Number(raw)
    return Number.isFinite(v) && v >= 0 ? v : fallback
  }
  return {
    rate: num('SCEMAWORLD_CLAIM_RATE', DEFAULT_POLICY.rate),
    perClaim: num('SCEMAWORLD_CLAIM_MAX', DEFAULT_POLICY.perClaim),
    perWallet: num('SCEMAWORLD_WALLET_MAX', DEFAULT_POLICY.perWallet),
    budget: num('SCEMAWORLD_TREASURY_BUDGET', DEFAULT_POLICY.budget),
    cooldownMs: num('SCEMAWORLD_CLAIM_COOLDOWN_MS', DEFAULT_POLICY.cooldownMs),
    minimum: num('SCEMAWORLD_CLAIM_MIN', DEFAULT_POLICY.minimum),
  }
}

export const mint = () => (process.env.SCEMAWORLD_MINT?.trim() || SCEMA_MINT)
export const treasuryOwner = () => (process.env.SCEMAWORLD_TREASURY?.trim() || TREASURY)

// ── the signer ───────────────────────────────────────────────────────────────

/**
 * The treasury's keypair, or `null` when this deployment has none.
 *
 * `null` is a first-class answer and the default one. A build with no secret is a build that can
 * read the treasury, quote a claim exactly, and refuse to settle it — which is a genuinely useful
 * state and the one every checkout is in. It must never be papered over.
 *
 * Two encodings accepted, because both are what people actually have: base58 (what a wallet
 * exports) and a JSON byte array (what `solana-keygen` writes). A `SCEMAWORLD_TREASURY_KEYFILE`
 * path is preferred over either, so the secret can live in a file with its own permissions rather
 * than in an environment block that gets printed by half the tools that read it.
 */
export async function signer(): Promise<Keypair | null> {
  const file = process.env.SCEMAWORLD_TREASURY_KEYFILE?.trim()
  let raw = process.env.SCEMAWORLD_TREASURY_SECRET?.trim() ?? ''
  if (file) {
    try {
      raw = (await readFile(file, 'utf8')).trim()
    } catch {
      // Deliberately not fatal and deliberately not detailed: a missing key file leaves the
      // deployment in the unconfigured state, which is already handled and already reported. The
      // path is not echoed anywhere a player can see.
      return null
    }
  }
  if (!raw) return null
  try {
    const bytes = raw.startsWith('[')
      ? Uint8Array.from(JSON.parse(raw) as number[])
      : bs58.decode(raw)
    return Keypair.fromSecretKey(bytes)
  } catch {
    return null
  }
}

// ── reading the treasury ─────────────────────────────────────────────────────

export interface TreasuryReading {
  mint: string
  owner: string
  /** The treasury's associated token account for this mint. */
  account: string
  program: TokenProgramKind
  decimals: number
  /** Raw u64 as a decimal string — never a JS number. */
  balanceBase: string
  /** The same figure floored to whole tokens, which is the unit every cap is written in. */
  balance: number
  /**
   * The treasury's SOL, for the rent on a first-time claimant's token account.
   *
   * `null` when it could not be read — never 0, which would read as "the treasury is broke" on
   * the strength of a failed RPC call.
   */
  sol: number | null
  /** The slot the read was answered at. What makes the figure checkable. */
  slot: number
  host: string
  authenticated: boolean
  /** False when this deployment can quote a claim and cannot settle one. */
  configured: boolean
}

export type TreasuryFailure = 'mint_unreadable' | 'not_a_mint' | 'account_unreadable' | 'rpc_failed'

export type TreasuryResult =
  | { ok: true; reading: TreasuryReading }
  | { ok: false; reason: TreasuryFailure; detail: string; host: string }

/**
 * Read the mint and the treasury's balance. **No fallbacks and no defaults.**
 *
 * `decimals` in particular comes from the mint account and nowhere else. The same rule `/escrow`
 * states as a money rule rather than a display one: a wrong `decimals` is a wrong *quantity*, and
 * a token list that says 6 against a real 9 pays out a thousandth of what was intended — or, in
 * the other direction, a thousand times it. The token program is decoded from the account's owner
 * for the same reason: this repo's own notes disagree with themselves about whether $SCEMA is
 * Token-2022, and the chain does not.
 */
export async function readTreasury(): Promise<TreasuryResult> {
  const { conn, host, authenticated } = connection()
  const configured = (await signer()) !== null
  let mintKey: PublicKey
  let ownerKey: PublicKey
  try {
    mintKey = new PublicKey(mint())
    ownerKey = new PublicKey(treasuryOwner())
  } catch (e) {
    return { ok: false, reason: 'mint_unreadable', detail: String(e), host }
  }

  try {
    const info = await conn.getAccountInfoAndContext(mintKey)
    if (!info.value) {
      return { ok: false, reason: 'mint_unreadable', detail: 'the mint account does not exist', host }
    }
    const kind = programKind(info.value.owner.toBase58())
    if (!kind) {
      return {
        ok: false,
        reason: 'not_a_mint',
        detail: `owned by ${info.value.owner.toBase58()}, which is not a token program`,
        host,
      }
    }
    const decoded = decodeMint(new Uint8Array(info.value.data))
    if (!decoded || !decoded.initialized) {
      return { ok: false, reason: 'not_a_mint', detail: 'the account is not an initialised mint', host }
    }

    const programId = kind === 'token-2022' ? TOKEN_2022_PROGRAM_ID : TOKEN_PROGRAM_ID
    // The ATA seeds include the token program, so it must be derived with the mint's *own*
    // program. Deriving with the wrong one yields a real, valid address that nobody controls —
    // the failure reads as "the treasury is empty", which is exactly the wrong diagnosis.
    const account = getAssociatedTokenAddressSync(mintKey, ownerKey, true, programId)

    // ## Decoded from the account, not fetched with `getTokenAccountBalance`
    //
    // The convenience method is classed as an indexed request by several providers, and the
    // public fallback this file falls back to answers it with a 403 telling you to buy a token.
    // A plain `getAccountInfo` is available everywhere, is one round trip, and — the reason that
    // matters — makes the treasury readable on an unconfigured checkout. A balance panel that only
    // works with a paid endpoint is a balance panel most people will meet as an error.
    //
    // The `amount` field is at byte 64 of the 165-byte base `Account` layout, which Token-2022
    // shares with legacy SPL and extends rather than rearranges (this treasury's own account is
    // 170 bytes). Anything shorter than the base layout is refused rather than decoded.
    const acct = await conn.getAccountInfo(account).catch(() => null)
    if (!acct || acct.data.length < 72) {
      // A treasury whose token account does not exist yet is a real and distinct state, and it is
      // NOT a balance of zero — "we could not read the reserve" and "the reserve is zero" are
      // different claims and only one of them is an accusation. `/escrow` learned this the hard
      // way; the same rule applies to a wallet.
      return {
        ok: false,
        reason: 'account_unreadable',
        detail: `no readable token account at ${account.toBase58()} for this mint`,
        host,
      }
    }
    const bytes = new Uint8Array(acct.data)
    const balanceBase = new DataView(
      bytes.buffer,
      bytes.byteOffset,
      bytes.byteLength,
    ).getBigUint64(64, true)

    // The treasury's SOL, because a first-time claimant's token account has to be created and the
    // treasury pays that rent. A treasury full of $SCEMA and empty of SOL settles claims for
    // existing holders and fails for everybody new — a failure that looks like the faucet being
    // broken rather than like a balance being low, so the figure is surfaced rather than left to
    // be discovered in a transaction error.
    const lamports = await conn.getBalance(ownerKey).catch(() => null)

    return {
      ok: true,
      reading: {
        mint: mintKey.toBase58(),
        owner: ownerKey.toBase58(),
        account: account.toBase58(),
        program: kind,
        decimals: decoded.decimals,
        balanceBase: balanceBase.toString(),
        balance: toWholeTokens(balanceBase, decoded.decimals),
        // `null`, never 0, when it could not be read. Same rule as the token balance.
        sol: lamports === null ? null : lamports / 1e9,
        slot: info.context.slot,
        host,
        authenticated,
        configured,
      },
    }
  } catch (e) {
    return { ok: false, reason: 'rpc_failed', detail: String(e), host }
  }
}

// ── the ledger ───────────────────────────────────────────────────────────────

export interface Ledger {
  /**
   * Bumped on every write. The stale-write detector.
   *
   * A reservation is decided against a snapshot and written a moment later. If anything at all
   * has written in between, the decision was made against figures that no longer hold and the
   * write must not land — see `writeLedger`. Absent in ledgers written before this existed, which
   * read as 0 and start counting from there.
   */
  version: number
  /** Whole tokens paid out by this deployment, all wallets. */
  dispensed: number
  wallets: Record<string, WalletRecord>
  /** Every settled claim, newest last. Kept whole: this is the audit trail. */
  claims: {
    wallet: string
    tokens: number
    /** The world commitment the claim was made against, when the client named one. */
    world: string | null
    at: number
    signature: string
    /**
     * Whether the transfer was seen to land.
     *
     * `unconfirmed` rows are the ones an operator has to go and look at: the allowance was
     * consumed and nobody watched the transaction settle. Recording them is the difference
     * between a reservation an operator can investigate by signature and one that just sits
     * there looking like a payment.
     */
    status: 'confirmed' | 'unconfirmed'
  }[]
}

const EMPTY: Ledger = { version: 0, dispensed: 0, wallets: {}, claims: [] }

function ledgerPath(): string {
  return process.env.SCEMAWORLD_LEDGER?.trim() || join(process.cwd(), '.scemaworld-claims.json')
}

/**
 * Read the ledger. A missing file is an empty ledger; an **unreadable** one is an error.
 *
 * The distinction is the whole point. A corrupt or unparseable ledger read as empty would reset
 * every cap in the policy at once — the budget, every wallet's lifetime total, every cooldown —
 * and the first symptom would be the treasury emptying. So a parse failure throws, and the route
 * turns it into a refusal.
 */
export async function readLedger(): Promise<Ledger> {
  let text: string
  try {
    text = await readFile(ledgerPath(), 'utf8')
  } catch (e) {
    if ((e as NodeJS.ErrnoException).code === 'ENOENT') return { ...EMPTY }
    throw e
  }
  const parsed = JSON.parse(text) as Ledger
  if (typeof parsed?.dispensed !== 'number' || typeof parsed?.wallets !== 'object') {
    throw new Error('the claim ledger is present but not in the expected shape')
  }
  return {
    // A ledger written before versioning reads as 0 rather than being rejected. Refusing it would
    // strand a live deployment's history on an upgrade, and 0 is a true statement about a file
    // that has never been written by a versioned writer.
    version: typeof parsed.version === 'number' ? parsed.version : 0,
    dispensed: parsed.dispensed,
    wallets: parsed.wallets ?? {},
    claims: parsed.claims ?? [],
  }
}

/**
 * Write the ledger: temp file, then rename.
 *
 * The same convention as every state file the bot writes (`FilterStats::write_to_file`), and for
 * the same reason: a reader must see the old file or the new one, never a half-written one. Here
 * the half-written one would be a ledger with a truncated wallet table, which is a set of caps
 * that have silently reset.
 */
async function writeLedger(next: Ledger, expectedVersion: number): Promise<void> {
  // Re-read and compare before writing. Inside `withLedger` this can only fail if the mutex and
  // the lock have both been defeated, so it is an assertion rather than the mechanism — but it is
  // the assertion that turns a silently lost reservation into a refused claim, and a reservation
  // is the only thing standing between a forged balance and the treasury.
  let current = 0
  try {
    current = (await readLedger()).version
  } catch {
    // Unreadable here means unreadable *now*, after we read it successfully a moment ago. Writing
    // over it would destroy a ledger somebody may still be able to repair.
    throw new LedgerConflict('the claim ledger became unreadable between the read and the write')
  }
  if (current !== expectedVersion) {
    throw new LedgerConflict(
      `the claim ledger changed underneath this claim (expected version ${expectedVersion}, found ${current})`,
    )
  }
  const path = ledgerPath()
  await mkdir(dirname(path), { recursive: true }).catch(() => {})
  const tmp = `${path}.tmp`
  await writeFile(tmp, JSON.stringify({ ...next, version: expectedVersion + 1 }, null, 2), 'utf8')
  await rename(tmp, path)
}

/** A write that was refused because the ledger moved. Never a reason to retry automatically. */
export class LedgerConflict extends Error {}

// ── serialising read-decide-write ────────────────────────────────────────────
//
// **The finding this closes.** A cap is checked against a snapshot and enforced by a write that
// happens later. Between the two there is at least one `await` — the read itself yields the event
// loop — so two claims arriving together each decided against the same untouched figures and the
// later write overwrote the earlier. Both were paid; the ledger recorded one. Every limit in the
// policy falls to this at once: the deployment budget, the per-wallet lifetime cap and the
// cooldown are all a comparison against a number a second request has already invalidated.
//
// It is not a narrow window and it does not need luck to win. It is the entire span from reading
// the file to renaming the new one over it, and a faucet is exactly the sort of endpoint people
// point concurrency at.
//
// Three layers, because they close different things and the cheapest one is not the strongest:
//
// 1. **An in-process mutex.** Node is single-threaded, so the shipped race is entirely between
//    interleaved awaits in one process. A promise chain closes it completely, and for the
//    deployment this ledger is designed for — one server, one local file — it is sufficient on
//    its own.
// 2. **An exclusive lock file.** `open(..., 'wx')` fails if the file exists, and that check and
//    create are atomic in the operating system. This is what makes the guarantee hold across two
//    processes on one filesystem rather than only within one.
// 3. **The version check in `writeLedger`.** A backstop for the case where both of the above have
//    somehow failed. It cannot double-pay, because a refused write happens *before* the transfer
//    is sent — the claim is simply refused.
//
// What none of them buys: a deployment whose instances do not share a filesystem. There the
// ledger file is not shared either, so each instance has its own caps and the budget is multiplied
// by the instance count. That is a property of keeping the ledger in a file and cannot be fixed
// here; `SCEMAWORLD_LEDGER` pointing at shared storage, or a real database, is the answer.

const LOCK_STALE_MS = 60_000
const LOCK_POLL_MS = 25
const LOCK_TIMEOUT_MS = 15_000

/** Serialises within this process. The fast path, and the one that matters in practice. */
let chain: Promise<unknown> = Promise.resolve()

async function acquireFileLock(): Promise<() => Promise<void>> {
  const path = `${ledgerPath()}.lock`
  await mkdir(dirname(path), { recursive: true }).catch(() => {})
  const deadline = Date.now() + LOCK_TIMEOUT_MS

  for (;;) {
    try {
      const handle = await open(path, 'wx')
      await handle.writeFile(`${process.pid} ${new Date().toISOString()}
`, 'utf8').catch(() => {})
      await handle.close().catch(() => {})
      return async () => {
        await unlink(path).catch(() => {})
      }
    } catch (e) {
      if ((e as NodeJS.ErrnoException).code !== 'EEXIST') throw e
    }

    // Break a lock left by a process that died holding it. Bounded by age, because the alternative
    // is a faucet that stays down until somebody notices a file — and a stale lock is far more
    // likely than the two-process contention this layer exists for.
    const age = await stat(path).then((st) => Date.now() - st.mtimeMs).catch(() => 0)
    if (age > LOCK_STALE_MS) {
      await unlink(path).catch(() => {})
      continue
    }
    if (Date.now() > deadline) {
      throw new LedgerConflict('the claim ledger is locked by another writer')
    }
    await new Promise((r) => setTimeout(r, LOCK_POLL_MS))
  }
}

/**
 * Run `fn` with exclusive access to the ledger, handing it a freshly read copy.
 *
 * Everything that reads the ledger and then writes a decision based on it must be inside this, and
 * **no RPC may happen within it** — a network round trip under a lock turns a faucet into a queue
 * with a fifteen-second timeout. The transfer therefore sits between two separate critical
 * sections: one that reserves, one that records the outcome.
 */
async function withLedger<T>(fn: (ledger: Ledger) => Promise<T> | T): Promise<T> {
  const run = async (): Promise<T> => {
    const release = await acquireFileLock()
    try {
      return await fn(await readLedger())
    } finally {
      await release()
    }
  }
  // Queue behind whatever is already running, and keep the chain alive if this one throws.
  const result = chain.then(run, run)
  chain = result.then(
    () => undefined,
    () => undefined,
  )
  return result
}

export type Reservation =
  | { ok: true; ent: Entitlement; prev: WalletRecord }
  | { ok: false; reason: string; detail: string; entitlement?: Entitlement }

/**
 * Decide a claim and reserve it against the ledger, **as one indivisible step**.
 *
 * Split out of `settle` for exactly the reason `transferPlan` is: it is the part with no safe
 * failure mode, and settling for real needs a funded treasury key — so without this the arithmetic
 * that bounds every payout this deployment can make would only ever be exercised by moving money.
 * `check:scemaworld` drives it concurrently against a temporary ledger, which is the only way the
 * property below can be tested at all.
 *
 * **The property**: for any number of claims arriving in any interleaving, the sum paid never
 * exceeds the deployment budget, no wallet exceeds its lifetime cap, and no wallet's cooldown is
 * bypassed. Deciding and reserving used to be separated by the write that enforced them, so two
 * claims arriving together each measured themselves against a figure the other had already spent.
 *
 * `treasury` is passed in rather than read here, deliberately: an RPC call inside the lock would
 * hold every other claim behind a network round trip. It is therefore a moment stale, which is
 * bounded by the budget — a cap that *is* serialised and is an order of magnitude below the
 * balance.
 */
export async function reserve(args: {
  scema: number
  wallet: string
  nowMs: number
  /** The treasury's balance in whole tokens, read before the lock. */
  treasury: number
}): Promise<Reservation> {
  try {
    return await withLedger(async (ledger): Promise<Reservation> => {
      const record = ledger.wallets[args.wallet] ?? NO_RECORD
      const ent = entitlement(
        {
          scema: args.scema,
          wallet: args.wallet,
          record,
          dispensed: ledger.dispensed,
          treasury: args.treasury,
          nowMs: args.nowMs,
        },
        policy(),
      )
      if (ent.refusal) {
        return { ok: false, reason: ent.refusal, detail: ent.message, entitlement: ent }
      }
      await writeLedger(
        {
          ...ledger,
          dispensed: ledger.dispensed + ent.tokens,
          wallets: {
            ...ledger.wallets,
            [args.wallet]: {
              paid: record.paid + ent.tokens,
              lastMs: args.nowMs,
              claims: record.claims + 1,
            },
          },
        },
        ledger.version,
      )
      return { ok: true, ent, prev: record }
    })
  } catch (e) {
    if (e instanceof LedgerConflict) {
      // Refused, and nothing was sent. Safe to retry precisely because the refusal happened
      // before the transfer rather than after it.
      return { ok: false, reason: 'ledger_busy', detail: String(e) }
    }
    // An unreadable ledger must never be treated as an empty one — see `readLedger`.
    return { ok: false, reason: 'ledger_unreadable', detail: String(e) }
  }
}

// ── settling a claim ─────────────────────────────────────────────────────────

/**
 * Where a payout goes and what it carries. **Pure** — addresses and arithmetic, no RPC, no key.
 *
 * Split out of `settle` because it is the part with no safe failure mode and the part that cannot
 * otherwise be tested: settling for real needs a funded treasury key, so without this the riskiest
 * arithmetic in the file would only ever be exercised by moving money. Everything that can be
 * wrong here is wrong *silently* —
 *
 * - the wrong token program derives a **valid** associated address that nobody controls, and the
 *   tokens land there. $SCEMA is Token-2022 (verified against the chain, not against this repo's
 *   own notes, which disagree with themselves about it), and the ATA seeds include the program.
 * - the wrong `decimals` moves the wrong quantity by a factor of a thousand. It is passed to
 *   `transferChecked`, so the chain refuses a mismatch rather than executing it — but only if the
 *   number here is the one that came off the mint account.
 */
export interface TransferPlan {
  programId: PublicKey
  source: PublicKey
  destination: PublicKey
  mint: PublicKey
  owner: PublicKey
  /** Base units. `bigint`, never a JS number — a u64 outruns `Number.MAX_SAFE_INTEGER`. */
  amount: bigint
  decimals: number
}

export function transferPlan(args: {
  reading: Pick<TreasuryReading, 'mint' | 'account' | 'owner' | 'program' | 'decimals'>
  holder: PublicKey
  tokens: number
}): TransferPlan {
  const programId =
    args.reading.program === 'token-2022' ? TOKEN_2022_PROGRAM_ID : TOKEN_PROGRAM_ID
  const mintKey = new PublicKey(args.reading.mint)
  return {
    programId,
    source: new PublicKey(args.reading.account),
    // Derived with the mint's own program, exactly as the treasury's own account is.
    destination: getAssociatedTokenAddressSync(mintKey, args.holder, true, programId),
    mint: mintKey,
    owner: new PublicKey(args.reading.owner),
    amount: toBaseUnits(args.tokens, args.reading.decimals),
    decimals: args.reading.decimals,
  }
}

export type SettleResult =
  | { ok: true; tokens: number; spend: number; signature: string; created: boolean }
  /**
   * Sent, and the outcome could not be observed. **Neither success nor failure.**
   *
   * A third arm rather than a flavour of failure, and it is not hypothetical — the very first
   * mainnet transaction this code sent landed and finalized while the confirmation wait hung
   * (see `confirm` below). Calling that `transfer_failed` would have released the reservation on
   * a claim that had actually paid, and the next request would have paid again.
   *
   * The same shape as `Outcome::Unknown` in `scema-effect`, for the same reason: an effect
   * attempted whose result nobody could observe is its own answer, and collapsing it into either
   * neighbour causes a specific, expensive wrong action.
   */
  | { ok: false; reason: 'unconfirmed'; detail: string; signature: string }
  | { ok: false; reason: string; detail: string; entitlement?: Entitlement; signature?: string }

/**
 * How long to wait for a signature to confirm before answering `unconfirmed`.
 *
 * Bounded, because the caller is an HTTP request. A wait with no ceiling is how a route ends up
 * holding a connection open on a transaction that finalized two minutes ago.
 */
const CONFIRM_TIMEOUT_MS = 45_000
const CONFIRM_POLL_MS = 1_500

/**
 * Send a signed transaction and poll for its status over HTTP.
 *
 * ## Why not `sendAndConfirmTransaction`
 *
 * Because it does not work here, and the way it fails is the worst available. It waits on a
 * **WebSocket** `signatureSubscribe`, and the `ws` package's `bufferutil` binding does not
 * survive Next's bundler — the subscription throws `TypeError: t.mask is not a function`, the
 * promise never settles, and the route hangs until the client gives up. Meanwhile the
 * transaction has been broadcast and confirmed perfectly normally. So the failure presents as
 * "the faucet is broken" on a payout that succeeded, which is the single most misleading pair of
 * facts this feature could produce.
 *
 * Polling `getSignatureStatuses` is plain HTTP, opens no socket, and is the right tool in a
 * request handler regardless of the bundler: a subscription would leak a WebSocket per claim.
 *
 * Returns `null` when the wait ran out — the caller turns that into `unconfirmed`, never into a
 * failure. `getSignatureStatuses` without `searchTransactionHistory` only sees recent signatures,
 * which is exactly the window this poll lives in.
 */
async function confirm(conn: Connection, signature: string, deadline: number): Promise<boolean | null> {
  while (Date.now() < deadline) {
    const st = await conn.getSignatureStatuses([signature]).catch(() => null)
    const v = st?.value?.[0]
    if (v) {
      // An on-chain error is a definite failure and is reported as one — the transaction was
      // observed, it just did not do anything.
      if (v.err) return false
      if (v.confirmationStatus === 'confirmed' || v.confirmationStatus === 'finalized') return true
    }
    await new Promise((r) => setTimeout(r, CONFIRM_POLL_MS))
  }
  return null
}

/**
 * Quote a claim without settling it. Same policy, same ledger, no transaction.
 *
 * Exists so the panel can show a real number before anyone presses anything, and so the number it
 * shows is computed by the code that will pay it rather than by a second implementation that
 * drifts. A preview that disagrees with the payer is worse than no preview.
 */
export type QuoteResult =
  | { ok: true; entitlement: Entitlement; treasury: TreasuryResult; dispensed: number }
  /**
   * The ledger could not be read, so there is no honest quote to give.
   *
   * The same refusal `settle` returns for the same condition, and that is the whole point. This
   * used to be `readLedger().catch(() => EMPTY)`: an unreadable ledger was quoted against as
   * though nothing had ever been paid — every cap reset to full — while the payer refused the
   * identical request. The preview promised 250 and the button returned a 500.
   *
   * Swallowing it was worse than a disagreement. A ledger read as empty is a ledger whose budget,
   * lifetime caps and cooldowns have all silently reset, which is exactly the failure `readLedger`
   * throws to prevent; catching the throw here reintroduced it on the one path that tells a player
   * what they are owed.
   */
  | { ok: false; reason: 'ledger_unreadable'; detail: string }

export async function quote(scema: number, wallet: string, nowMs: number): Promise<QuoteResult> {
  const treasury = await readTreasury()
  let ledger: Ledger
  try {
    ledger = await readLedger()
  } catch (e) {
    return { ok: false, reason: 'ledger_unreadable', detail: String(e) }
  }
  const ent = entitlement(
    {
      scema,
      wallet,
      record: ledger.wallets[wallet] ?? NO_RECORD,
      dispensed: ledger.dispensed,
      treasury: treasury.ok ? treasury.reading.balance : 0,
      nowMs,
    },
    policy(),
  )
  return { ok: true, entitlement: ent, treasury, dispensed: ledger.dispensed }
}

/**
 * Pay a claim.
 *
 * ## Order of operations, and the residual risk it leaves
 *
 * The ledger is written **before** the transfer is sent, and released if the send fails. Reserving
 * first is what makes two concurrent requests from one wallet unable to draw the cap twice; the
 * cost is that a process killed between the write and the confirmation leaves a reservation
 * standing against a payment that never happened — the player is short and the ledger says they
 * were paid.
 *
 * That is the correct direction for the error to fall. The alternative — pay, then record — fails
 * the other way: a crash after a confirmed transfer leaves a *paid* wallet with no record of it,
 * and the next request pays again. One of these loses a player their allowance and can be fixed by
 * an operator editing a file; the other drains a treasury and cannot be undone. Neither is
 * hypothetical, and pretending a lock removes the choice would be the real mistake.
 */
export async function settle(args: {
  scema: number
  wallet: string
  world: string | null
  nowMs: number
}): Promise<SettleResult> {
  const key = await signer()
  if (!key) {
    // Not an error the player caused, and not a failure of the chain. This deployment can quote a
    // claim and cannot settle one — reported as exactly that, and never as a success.
    return {
      ok: false,
      reason: 'not_configured',
      detail:
        'this deployment has no treasury signer, so it can quote a withdrawal but cannot pay it',
    }
  }

  const treasury = await readTreasury()
  if (!treasury.ok) {
    return { ok: false, reason: treasury.reason, detail: treasury.detail }
  }
  if (treasury.reading.owner !== key.publicKey.toBase58()) {
    // A signer that is not the treasury would build a transfer from an account it does not own and
    // fail on chain with a signature error, which reads as an RPC problem. Naming it here is the
    // difference between "the faucet is broken" and "you configured the wrong key".
    return {
      ok: false,
      reason: 'signer_mismatch',
      detail: `the configured signer is ${key.publicKey.toBase58()}, not the treasury ${treasury.reading.owner}`,
    }
  }

  const { conn } = connection()
  let holder: PublicKey
  try {
    holder = new PublicKey(args.wallet)
  } catch (e) {
    return { ok: false, reason: 'bad_wallet', detail: String(e) }
  }

  const reservation = await reserve({
    scema: args.scema,
    wallet: args.wallet,
    nowMs: args.nowMs,
    treasury: treasury.reading.balance,
  })
  if (!reservation.ok) {
    return {
      ok: false,
      reason: reservation.reason,
      detail: reservation.detail,
      entitlement: reservation.entitlement,
    }
  }
  const { ent, prev } = reservation
  const plan = transferPlan({ reading: treasury.reading, holder, tokens: ent.tokens })

  // ── critical section 2: record the outcome, as a delta ─────────────────────
  //
  // Both of these re-read under the lock and apply a *change*, rather than writing back the
  // snapshot this claim was decided against. Writing that snapshot back is how releasing one
  // reservation silently cancels somebody else's: the old code restored the whole pre-claim
  // ledger, so a claim that failed on chain erased every reservation taken while it was in flight
  // — and those claims had already been paid.

  const appendClaim = async (signature: string, status: 'confirmed' | 'unconfirmed') => {
    await withLedger(async (cur) => {
      await writeLedger(
        {
          ...cur,
          claims: [
            ...cur.claims,
            { wallet: args.wallet, tokens: ent.tokens, world: args.world, at: args.nowMs, signature, status },
          ],
        },
        cur.version,
      )
    }).catch(() => {})
  }

  const releaseReservation = async () => {
    await withLedger(async (cur) => {
      const now = cur.wallets[args.wallet] ?? NO_RECORD
      await writeLedger(
        {
          ...cur,
          dispensed: cur.dispensed - ent.tokens,
          wallets: {
            ...cur.wallets,
            [args.wallet]: {
              paid: now.paid - ent.tokens,
              // Back to whatever it was before this claim, so a refused attempt does not leave a
              // player serving a six-hour cooldown for a withdrawal they never received.
              lastMs: prev.lastMs,
              claims: Math.max(0, now.claims - 1),
            },
          },
        },
        cur.version,
      )
    }).catch(() => {})
  }

  try {
    const tx = new Transaction()
    let created = false
    const existing = await conn.getAccountInfo(plan.destination)
    if (!existing) {
      // The treasury pays the rent for a holder's first claim. It costs the treasury SOL, and a
      // treasury with no SOL fails here — which surfaces as a send error naming the account rather
      // than as a silent no-op, because a claim that quietly did nothing is the worst answer
      // available.
      created = true
      tx.add(
        createAssociatedTokenAccountInstruction(
          key.publicKey,
          plan.destination,
          holder,
          plan.mint,
          plan.programId,
          ASSOCIATED_TOKEN_PROGRAM_ID,
        ),
      )
    }
    tx.add(
      // `transferChecked`, not `transfer`. It carries the decimals and the mint, so a transfer
      // built against a wrong `decimals` is rejected by the chain instead of moving a thousand
      // times the intended amount. The whole decimals argument in `readTreasury` gets a second
      // enforcement here, on the one side that cannot be talked out of it.
      createTransferCheckedInstruction(
        plan.source,
        plan.mint,
        plan.destination,
        key.publicKey,
        plan.amount,
        plan.decimals,
        [],
        plan.programId,
      ),
    )
    // Blockhash and fee payer are set explicitly because the transaction is signed and sent by
    // hand below rather than by `sendAndConfirmTransaction` — see `confirm` for why.
    const { blockhash } = await conn.getLatestBlockhash('confirmed')
    tx.recentBlockhash = blockhash
    tx.feePayer = key.publicKey
    tx.sign(key)

    // Preflight left on. It costs a round trip and it turns "the treasury has no SOL for rent"
    // from a confirmed-failed transaction into a refusal before anything is broadcast.
    const signature = await conn.sendRawTransaction(tx.serialize(), { skipPreflight: false })
    const landed = await confirm(conn, signature, Date.now() + CONFIRM_TIMEOUT_MS)

    if (landed === null) {
      // Sent, unobserved. The reservation **stays**, because the transfer may well have gone
      // through — releasing it here is how a paid claim gets paid a second time.
      await appendClaim(signature, 'unconfirmed')
      return {
        ok: false,
        reason: 'unconfirmed',
        detail:
          'the transfer was broadcast but did not confirm within the wait. It may still have ' +
          'landed — check the signature before retrying, and note the withdrawal has been ' +
          'recorded against your allowance so it cannot be paid twice.',
        signature,
      }
    }
    if (landed === false) {
      // Observed, and it failed on chain. A definite outcome, so the reservation is released.
      await releaseReservation()
      return { ok: false, reason: 'transfer_failed', detail: `the transaction failed on chain: ${signature}`, signature }
    }

    await appendClaim(signature, 'confirmed')
    return { ok: true, tokens: ent.tokens, spend: ent.spend, signature, created }
  } catch (e) {
    // Release the reservation. Reaching here means the send itself threw — preflight rejected it,
    // or the endpoint refused — so nothing was broadcast and the caps must not act as though it
    // was. A failed send that permanently consumed a player's lifetime allowance would be
    // indistinguishable, from their side, from theft.
    //
    // Note the asymmetry with `unconfirmed` above, which is the whole point of separating them: a
    // throw means we know nothing happened, a timeout means we do not know.
    await releaseReservation()
    return { ok: false, reason: 'transfer_failed', detail: String(e) }
  }
}
