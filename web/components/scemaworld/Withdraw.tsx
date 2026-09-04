'use client'

/**
 * The treasury panel: withdrawing in-game SCEMA as real $SCEMA.
 *
 * ## What this component is not allowed to do
 *
 * **Invent a number, or imply a success.** It renders what `GET /api/scemaworld/treasury` and
 * `POST /api/scemaworld/claim?quote=1` say and nothing else — there is no local estimate of the
 * balance, no optimistic debit, and no "sent!" that is not backed by a signature the route got
 * back from the chain. A page that showed a plausible figure while the read was failing would be
 * making a promise about money on the strength of nothing, which is the exact failure `/escrow`
 * has a whole page of rules about.
 *
 * So four states are drawn distinctly and none of them is a zero:
 *
 * - **unread** — the treasury has not answered yet. `—`, never `0`.
 * - **unreadable** — it answered with an error, which is named. "The mint could not be read", "the
 *   treasury has no token account for this mint" and "the RPC failed" send an operator to three
 *   different places, and a balance of zero is an accusation none of them deserve.
 * - **quotable but unpayable** — the deployment can price a withdrawal and has no signer. Said in
 *   the button's own label, because a player who presses it and meets a 501 has been misled by the
 *   interface rather than informed by it.
 * - **payable** — the amount, the cap that binds it, and what is left.
 *
 * ## Why the caps are on screen rather than only enforced
 *
 * A cap the player cannot see is a cap that reads as the button being broken. This is the same
 * lesson as the station panel that states *why* a service is unavailable instead of refusing into
 * a notice that fades in three seconds — refuelling, jumping and the market were all reported
 * broken and all three were refusing correctly.
 *
 * And the honest sentence about forgeability is here too, in the panel that pays. A player is
 * entitled to know that the balance is client-side, that the caps exist because of it, and that
 * they are not a judgement about them personally.
 */

import { useCallback, useEffect, useState } from 'react'

import { looksLikeAddress, type Entitlement, type Policy } from '@/lib/scemaworld/claim'

interface TreasuryReading {
  mint: string
  owner: string
  account: string
  program: string
  decimals: number
  balance: number
  sol: number | null
  slot: number
  host: string
  configured: boolean
}

type Treasury =
  | { state: 'unread' }
  | { state: 'ok'; reading: TreasuryReading; policy: Policy }
  | { state: 'error'; reason: string; detail: string; policy: Policy | null }

const REASON: Record<string, string> = {
  mint_unreadable: 'the $SCEMA mint could not be read',
  not_a_mint: 'that address is not an initialised token mint',
  account_unreadable: 'the treasury holds no token account for this mint',
  rpc_failed: 'the Solana endpoint did not answer',
}

/** Whole tokens with thousands separators. Never a currency symbol — there is no price here. */
function amount(n: number): string {
  return n.toLocaleString('en-US')
}

export function Withdraw({
  scema,
  world,
  onWithdrawn,
}: {
  /** The ship's in-game SCEMA balance. */
  scema: number
  /** The world commitment this session is flying, recorded with the claim. Never priced by it. */
  world: string | null
  /** Called with what the *server* said was spent, never with what was offered. */
  onWithdrawn: (spend: number, tokens: number) => void
}) {
  const [treasury, setTreasury] = useState<Treasury>({ state: 'unread' })
  const [wallet, setWallet] = useState('')
  const [quote, setQuote] = useState<Entitlement | null>(null)
  const [busy, setBusy] = useState(false)
  /**
   * `sent` is a third state, not a flavour of failure.
   *
   * A broadcast transaction whose confirmation was not observed is neither — and it is not
   * hypothetical: the first mainnet transaction this feature ever sent landed and finalized while
   * the confirmation wait hung. Painting that red tells the player nothing happened, which is
   * false and invites the retry that pays twice; painting it green claims a settlement nobody
   * watched. The same `Outcome::Unknown` split `scema-effect` uses.
   */
  const [result, setResult] = useState<
    { state: 'sent' | 'ok' | 'failed'; text: string; signature?: string | null } | null
  >(null)

  useEffect(() => {
    let cancelled = false
    fetch('/api/scemaworld/treasury')
      .then(async (r) => ({ status: r.status, body: await r.json() }))
      .then(({ body }) => {
        if (cancelled) return
        setTreasury(
          body.ok
            ? { state: 'ok', reading: body.treasury, policy: body.policy }
            : { state: 'error', reason: body.reason, detail: body.detail, policy: body.policy ?? null },
        )
      })
      .catch((e) => {
        if (cancelled) return
        // A fetch that never landed is not a treasury of zero either.
        setTreasury({ state: 'error', reason: 'rpc_failed', detail: String(e), policy: null })
      })
    return () => {
      cancelled = true
    }
  }, [])

  // The quote comes from the server, computed by the same function that will pay it. A local
  // estimate would be a second implementation of the policy and would drift from the payer
  // exactly when a cap started binding — which is the only moment the number matters.
  useEffect(() => {
    if (!looksLikeAddress(wallet) || scema <= 0) {
      setQuote(null)
      return
    }
    let cancelled = false
    const id = setTimeout(() => {
      fetch('/api/scemaworld/claim?quote=1', {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({ wallet, scema, world }),
      })
        .then((r) => r.json())
        .then((body) => {
          if (!cancelled) setQuote(body.ok ? body.quote : null)
        })
        .catch(() => {
          if (!cancelled) setQuote(null)
        })
    }, 300)
    return () => {
      cancelled = true
      clearTimeout(id)
    }
  }, [wallet, scema, world])

  const withdraw = useCallback(async () => {
    setBusy(true)
    setResult(null)
    try {
      const r = await fetch('/api/scemaworld/claim', {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({ wallet, scema, world }),
      })
      const body = await r.json()
      if (!body.ok) {
        // Unobserved, not failed. The balance is deliberately **not** debited — the caps already
        // hold the reservation, so nothing can be claimed twice, and debiting on an outcome
        // nobody watched would take the player's SCEMA on a transfer that may not have landed.
        setResult({
          state: body.reason === 'unconfirmed' ? 'sent' : 'failed',
          text: body.detail || body.reason,
          signature: body.signature ?? null,
        })
        return
      }
      // Debited only now, and only by what the server said it spent. A capped claim pays less
      // than was offered, and debiting the offer would burn the difference.
      onWithdrawn(body.spend, body.tokens)
      setResult({
        state: 'ok',
        text: `${amount(body.tokens)} $SCEMA sent${body.createdAccount ? ' — token account created' : ''}`,
        signature: body.signature,
      })
    } catch (e) {
      setResult({ state: 'failed', text: String(e) })
    } finally {
      setBusy(false)
    }
  }, [wallet, scema, world, onWithdrawn])

  const policy = treasury.state === 'unread' ? null : treasury.policy
  const configured = treasury.state === 'ok' && treasury.reading.configured
  const canWithdraw =
    !busy && configured && quote !== null && quote.refusal === null && quote.tokens > 0

  return (
    <div className="space-y-2">
      <div className="flex flex-wrap items-baseline gap-x-4 gap-y-1">
        <b className="text-omni-accent">TREASURY</b>
        <span className="text-omni-dim">
          holds{' '}
          <span className="text-omni-text">
            {/* Never a zero for an unread or unreadable treasury. The em-dash rule, applied to
                money: "we have not read it" and "it is empty" are different claims. */}
            {treasury.state === 'ok' ? `${amount(treasury.reading.balance)} $SCEMA` : '—'}
          </span>
        </span>
        {treasury.state === 'ok' && (
          <span className="text-omni-dim">
            slot {treasury.reading.slot} · {treasury.reading.host} · {treasury.reading.program}
            {/* `—` when unread, never 0 — a failed RPC call must not read as a broke treasury. */}
            {' · '}
            {treasury.reading.sol === null ? '— SOL' : `${treasury.reading.sol.toFixed(3)} SOL`}
          </span>
        )}
        {/*
          A first claim has to create the claimant's token account, and the treasury pays that
          rent (about 0.002 SOL). A treasury full of $SCEMA and empty of SOL settles for existing
          holders and fails for everyone new — which looks like the faucet being broken rather
          than like a balance being low, so it is said before it happens rather than after.
        */}
        {treasury.state === 'ok' && treasury.reading.sol !== null && treasury.reading.sol < 0.01 && (
          <span className="text-omni-warn">low SOL — a first-time claim may fail on rent</span>
        )}
      </div>

      {treasury.state === 'error' && (
        <div className="text-omni-invalid">
          {REASON[treasury.reason] ?? treasury.reason} — {treasury.detail}
        </div>
      )}

      {policy && (
        <div className="text-omni-dim">
          {policy.rate} $SCEMA per SCEMA · at most {amount(policy.perClaim)} per withdrawal ·{' '}
          {amount(policy.perWallet)} per wallet, ever · minimum {amount(policy.minimum)} ·{' '}
          {Math.round(policy.cooldownMs / 3_600_000)}h between withdrawals
        </div>
      )}

      <div className="flex flex-wrap items-center gap-2">
        <input
          value={wallet}
          onChange={(e) => setWallet(e.target.value)}
          spellCheck={false}
          placeholder="your Solana wallet address"
          className="min-w-0 flex-1 rounded border border-omni-border bg-black/60 px-2 py-1 font-mono text-omni-text outline-none focus:border-omni-accent"
        />
        <button
          type="button"
          disabled={!canWithdraw}
          onClick={withdraw}
          className="rounded border border-omni-border px-2 py-1 text-omni-text hover:border-omni-accent disabled:opacity-40"
        >
          {busy
            ? 'sending…'
            : quote && quote.tokens > 0
              ? `withdraw ${amount(quote.tokens)} $SCEMA`
              : 'withdraw'}
        </button>
      </div>

      {/* Why the button is off, always said. A disabled control with no reason is a dead key. */}
      {!configured && treasury.state !== 'unread' && (
        <div className="text-omni-warn">
          This deployment can price a withdrawal but has no treasury signer, so it cannot pay one.
          The figures above are real; the button is not available here.
        </div>
      )}
      {wallet !== '' && !looksLikeAddress(wallet) && (
        <div className="text-omni-invalid">that does not look like a Solana address</div>
      )}
      {quote?.refusal && <div className="text-omni-warn">{quote.message}</div>}
      {quote && !quote.refusal && (
        <div className="text-omni-valid">
          {quote.message} — costs {amount(quote.spend)} of your {amount(scema)} SCEMA
        </div>
      )}

      {result && (
        <div
          className={
            result.state === 'ok'
              ? 'text-omni-valid'
              : result.state === 'sent'
                ? 'text-omni-warn'
                : 'text-omni-invalid'
          }
        >
          {result.text}
          {result.signature && (
            <>
              {' '}
              <a
                href={`https://solscan.io/tx/${result.signature}`}
                target="_blank"
                rel="noreferrer"
                className="underline hover:text-omni-accent"
              >
                {result.signature.slice(0, 8)}…
              </a>
            </>
          )}
        </div>
      )}

      <div className="text-omni-muted">
        Your SCEMA balance lives in this browser tab, and nothing here can tell a balance that was
        earned from one that was typed — making it unforgeable would mean running the whole
        simulation on a server. The caps above exist because of that, not because of you. They
        bound what this treasury can lose; they are not a judgement about how you played.
      </div>
    </div>
  )
}
