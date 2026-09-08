// The token gate — what it covers, and the larger part it deliberately does not.
//
// ── The decision ─────────────────────────────────────────────────────────────
//
// The sniper and the dashboard gate at startup on a 250k SCEMA balance. Zero splits:
//
//   • **Reading is ungated.** Discovery, scoring, Ψ, the record verifier — all open. The
//     reason is the same one that keeps /escrow and /alchem-link outside the gate: a page
//     whose whole point is public verifiability is defeated by gating it. A track record
//     nobody can check is a screenshot.
//   • **Arming the session key is gated.** Unattended execution is the product, and it is
//     the part with a cost — the RPC load, the support surface, and the fact that an
//     autonomous key trading on somebody's behalf is a thing you want to be able to point
//     at a holder.
//   • **Attended execution is ungated.** A wallet prompt is the user's own wallet doing
//     the user's own swap through Jupiter. Gating that would be gating a link.
//
// ── The honest limit, stated because it is easy to oversell ──────────────────
//
// This gate is a **client-side courtesy, not a security boundary**, and Zero has no
// server that could make it one. Everything Zero does happens in the operator's own
// browser with the operator's own RPC key and the operator's own wallet; there is nothing
// for a gate to withhold. Anyone can edit the check out.
//
// That is fine as long as nobody claims otherwise. What the gate actually buys is a
// default: the shipped build asks for the balance before arming autonomy, which is a real
// answer to "who is this for" and a false answer to "who can use it". The Rust gate is
// different in kind — it protects a process the operator did not write — and this file
// must not be described as the same thing.

export const SCEMA_MINT = 'HcsHqEJ9suf4oHJ8mb52M7AVKjhYhnTaeHgTmde7pump'
/** SCEMA is Token-2022; a legacy-SPL ATA derivation yields an address nobody controls. */
export const TOKEN_2022_PROGRAM_ID = 'TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb'
export const SCEMA_DECIMALS = 6
/** Same threshold as `SCEMATICA_SKIP_GATE`'s counterpart in the Rust startup check. */
export const REQUIRED_SCEMA = 250_000

export type GateVerdict = 'open' | 'insufficient' | 'unknown'

export interface GateState {
  verdict: GateVerdict
  /** Base units held, as a decimal string. `null` when the balance could not be read. */
  balance: string | null
  requiredBaseUnits: string
  reason: string
  /** True when autonomy may be armed. */
  mayArm: boolean
  /** Always true. Reading is never gated. */
  mayRead: boolean
  /** Always true. The user's own wallet signing the user's own swap. */
  mayExecuteAttended: boolean
}

export const REQUIRED_BASE_UNITS = (
  BigInt(REQUIRED_SCEMA) * BigInt(10) ** BigInt(SCEMA_DECIMALS)
).toString()

/**
 * Decide the gate from a balance read.
 *
 * `balance === null` means the read FAILED, and it is `unknown` rather than
 * `insufficient`. The two are different claims and only one is an accusation: telling a
 * holder they do not hold enough, because an RPC timed out, sends them to buy a token
 * they already own. The vault service makes exactly this distinction with 503 rather than
 * 403, and for exactly this reason.
 *
 * It fails CLOSED — `mayArm` is false on `unknown` — while reporting accurately.
 */
export function evaluateGate(balanceBaseUnits: string | null): GateState {
  const base = {
    requiredBaseUnits: REQUIRED_BASE_UNITS,
    mayRead: true,
    mayExecuteAttended: true,
  }

  if (balanceBaseUnits === null) {
    return {
      ...base,
      verdict: 'unknown',
      balance: null,
      mayArm: false,
      reason:
        'Could not read your SCEMA balance. This is not a statement that you hold too little — it is a statement that we could not ask. Reading and attended swaps are unaffected.',
    }
  }

  let held: bigint
  try {
    held = BigInt(balanceBaseUnits)
  } catch {
    return {
      ...base,
      verdict: 'unknown',
      balance: null,
      mayArm: false,
      reason: 'Balance did not decode as an integer.',
    }
  }

  if (held >= BigInt(REQUIRED_BASE_UNITS)) {
    return {
      ...base,
      verdict: 'open',
      balance: balanceBaseUnits,
      mayArm: true,
      reason: `Holding ${format(held)} SCEMA — autonomy may be armed.`,
    }
  }

  return {
    ...base,
    verdict: 'insufficient',
    balance: balanceBaseUnits,
    mayArm: false,
    reason: `Holding ${format(held)} SCEMA; ${REQUIRED_SCEMA.toLocaleString('en-US')} is required to arm the session key. Reading and attended swaps stay open.`,
  }
}

/** Place the decimal point on the string. A u64 past 2^53 loses precision as a number. */
function format(baseUnits: bigint): string {
  const s = baseUnits.toString().padStart(SCEMA_DECIMALS + 1, '0')
  const whole = s.slice(0, -SCEMA_DECIMALS)
  return Number(whole).toLocaleString('en-US')
}

/** The sentence shown next to the gate, so the limit above is never only in a comment. */
export const GATE_NOTE =
  'This check runs in your browser and is a default, not a security boundary — Zero has no server to enforce it with. Everything it does uses your own RPC key and your own wallet.'
