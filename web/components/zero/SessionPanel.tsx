'use client'

// Funding, sweeping, and the numbers that bound the loss.
//
// The caps are on screen whenever the key is armed, because a cap the operator cannot see
// reads as a broken button — the same rule `claim.ts` states for the faucet. And the
// warning is not a tooltip: a session key is a hot key in a browser tab, the caps buy
// bounded loss rather than secrecy, and the one sentence that makes that survivable has
// to be somewhere the operator cannot avoid reading it.

import { useCallback, useEffect, useState } from 'react'
import { useWallet } from '@solana/wallet-adapter-react'
import { Keypair } from '@solana/web3.js'

import { LAMPORTS_PER_SOL, SESSION_WARNING, type SessionCaps } from '@/lib/zero/session'
import {
  MIN_FUND_LAMPORTS,
  SWEEP_RESERVE_LAMPORTS,
  fundingPlan,
  sweepBlockedBy,
  sweepPlan,
  type FundingPlan,
} from '@/lib/zero/funding'
import { ZeroRpc, type RpcConfig } from '@/lib/zero/host/rpc'
import { fundSession, loadFunder, sweepSession, type Movement } from '@/lib/zero/host/treasury'
import { walletSigner } from '@/lib/zero/host/signer'

const sol = (lamports: number) => (lamports / LAMPORTS_PER_SOL).toFixed(4)

export function SessionPanel({
  rpc,
  session,
  caps,
  openPositions,
  onFunded,
}: {
  rpc: RpcConfig | null
  session: Keypair | null
  caps: SessionCaps
  openPositions: number
  onFunded: () => void
}) {
  const { publicKey, signTransaction, connected } = useWallet()
  const [balance, setBalance] = useState<number | null>(null)
  const [walletBalance, setWalletBalance] = useState<number | null>(null)
  const [amount, setAmount] = useState('0.05')
  const [busy, setBusy] = useState(false)
  const [result, setResult] = useState<Movement | null>(null)
  const funder = loadFunder()

  const address = session?.publicKey.toBase58() ?? null

  const refresh = useCallback(async () => {
    if (!rpc || !address) return
    const client = new ZeroRpc(rpc)
    try {
      setBalance(await client.lamports(address))
    } catch {
      // A balance that could not be read stays null. It is not zero, and rendering it as
      // zero would show a funded key as empty and invite a second funding.
      setBalance(null)
    }
    // The funder's balance is read too, so the preview can apply the SAME
    // `insufficient-funder` rule the transfer will. Guessing it here would make the
    // button promise something the payer then refuses.
    if (!publicKey) return setWalletBalance(null)
    try {
      setWalletBalance(await client.lamports(publicKey.toBase58()))
    } catch {
      setWalletBalance(null)
    }
  }, [rpc, address, publicKey])

  useEffect(() => {
    void refresh()
  }, [refresh, result])

  const lamports = Math.round((Number(amount) || 0) * LAMPORTS_PER_SOL)

  // The preview refuses exactly where the payer refuses.
  //
  // `fundSession` will not fund against a balance it could not read — that is how a cap
  // gets walked past — so previewing with `balance ?? 0` would show a green button for a
  // request the transfer then rejects. The escrow path already paid for this once, where
  // `quote` caught the ledger read's throw and priced a claim against an empty ledger
  // while `settle` refused the identical request. Same shape, same fix: the unreadable
  // case is surfaced here rather than defaulted away.
  const plan: FundingPlan =
    balance === null
      ? {
          ok: false,
          refusal: 'no-funder',
          detail:
            'the session key balance could not be read, and funding against an unknown total is how a cap gets walked past',
        }
      : fundingPlan(balance, lamports, caps.maxBalanceLamports, connected ? walletBalance : null)
  const sweepBlocked = sweepBlockedBy(openPositions)
  const sweepable = balance === null ? null : sweepPlan(balance)

  const fund = useCallback(async () => {
    if (!rpc || !address || !publicKey || !signTransaction) return
    setBusy(true)
    setResult(null)
    try {
      const r = await fundSession(
        new ZeroRpc(rpc),
        walletSigner(publicKey.toBase58(), signTransaction),
        address,
        lamports,
        caps.maxBalanceLamports,
      )
      setResult(r)
      if (r.ok) onFunded()
    } catch (e) {
      setResult({ ok: false, detail: e instanceof Error ? e.message : String(e) })
    } finally {
      setBusy(false)
    }
  }, [rpc, address, publicKey, signTransaction, lamports, caps.maxBalanceLamports, onFunded])

  const sweep = useCallback(async () => {
    if (!rpc || !session) return
    setBusy(true)
    setResult(null)
    try {
      setResult(await sweepSession(new ZeroRpc(rpc), session, funder))
    } catch (e) {
      setResult({ ok: false, detail: e instanceof Error ? e.message : String(e) })
    } finally {
      setBusy(false)
    }
  }, [rpc, session, funder])

  if (!session) {
    return (
      <section className="border border-zero-border bg-zero-surface px-4 py-3">
        <div className="text-xs text-zero-accent uppercase tracking-wider">session key</div>
        <p className="text-xs text-zero-dim mt-2">
          No key yet. Arming creates one in this browser; funding it is a separate, deliberate
          step you approve in your own wallet.
        </p>
      </section>
    )
  }

  return (
    <section className="border border-zero-border bg-zero-surface">
      <div className="px-4 py-2 border-b border-zero-border text-xs text-zero-accent uppercase tracking-wider">
        session key
      </div>

      <div className="px-4 py-3 space-y-2 text-xs">
        <div className="text-zero-warn">{SESSION_WARNING}</div>

        <div className="text-zero-dim break-all">
          {address}
          <span className="text-zero-muted">
            {' · '}
            {/* A balance that could not be read is an em dash, never 0.0000 — a funded key
                shown as empty invites a second funding. */}
            {balance === null ? '— SOL (not read)' : `${sol(balance)} SOL`}
            {' of '}
            {sol(caps.maxBalanceLamports)} cap
          </span>
        </div>

        <div className="flex gap-2 items-center flex-wrap">
          <input
            value={amount}
            onChange={e => setAmount(e.target.value)}
            inputMode="decimal"
            className="w-24 bg-zero-black border border-zero-border px-2 py-1.5 outline-none focus:border-zero-accent"
          />
          <span className="text-zero-dim">SOL</span>
          <button
            onClick={fund}
            disabled={busy || !connected || !plan.ok}
            className="px-3 py-1.5 border border-zero-border text-zero-accent hover:bg-zero-hi disabled:opacity-30"
          >
            {busy ? 'working…' : 'fund'}
          </button>
          <button
            onClick={sweep}
            disabled={busy || sweepBlocked !== null || !sweepable?.ok}
            title={sweepBlocked ?? (sweepable && !sweepable.ok ? sweepable.detail : '')}
            className="px-3 py-1.5 border border-zero-border text-zero-muted hover:text-zero-text disabled:opacity-30"
          >
            sweep back
          </button>
        </div>

        {/* Why a disabled button is disabled. A control that is simply grey is
            indistinguishable from a broken one, and the refusals here are the product. */}
        {!connected && <div className="text-zero-dim">Connect a wallet to fund from.</div>}
        {connected && walletBalance === null && (
          <div className="text-zero-dim">Reading your wallet balance…</div>
        )}
        {connected && !plan.ok && <div className="text-zero-warn">{plan.detail}</div>}
        {sweepBlocked && <div className="text-zero-alarm">{sweepBlocked}</div>}
        {!sweepBlocked && sweepable && !sweepable.ok && (
          <div className="text-zero-dim">{sweepable.detail}</div>
        )}

        <div className="text-zero-dim leading-relaxed">
          Minimum top-up {sol(MIN_FUND_LAMPORTS)} SOL. A sweep leaves{' '}
          {SWEEP_RESERVE_LAMPORTS} lamports behind to pay for itself and returns everything else
          to{' '}
          {funder ? (
            <span className="text-zero-muted break-all">{funder}</span>
          ) : (
            <span className="text-zero-warn">the wallet that funds it — nothing is on record yet</span>
          )}
          . The destination is read from that record, never typed: a sweep that took an address
          would be a one-click drain of this key to anywhere.
        </div>

        {result && (
          <div className={result.ok ? 'text-zero-ok' : 'text-zero-alarm'}>
            {result.detail}
            {result.signature && <span className="text-zero-dim"> — {result.signature.slice(0, 16)}…</span>}
          </div>
        )}
      </div>
    </section>
  )
}
