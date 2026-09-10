// Ψ — the slice of `scematica-sentience` that `coherence.rs` actually calls.
//
// ⚠️  PORT. Rust is authoritative: `crates/scematica-sentience/src/{master_equation,
// sentience,perception,rationality,logic,ethics,agency,meta_cognition,types}.rs`.
// `check:zero` pins this against `fixtures/sniper-parity.json`, which is emitted by
// `cargo test -p scematica-sniper zero_parity` — i.e. by the bot itself.
//
// ── Why this file exists at all ──────────────────────────────────────────────
//
// Zero's coherence gate used to report Ψ as the resolution rate: `resolved / total`,
// a number in [0,1] that moves the right way and is not this quantity. The real Ψ is a
// product of six bounded terms, and at a PERFECT resolution rate on a fresh feed it is
// **0.2055**, not 1.0 — because the four terms nobody instruments here (meta-cognition,
// agency, feedback, and the sentience index's own factors) contribute their defaults,
// which are below one.
//
// Two consequences, and both are the reason a "close enough" port is not close enough:
//
//   • A threshold means something different on each scale. The sniper halts at
//     Ψ < 0.02 (`CAUTION_THRESHOLD`); on the rate scale that is a resolution rate
//     somewhere near a tenth. Zero's own `minPsi` of 0.55 — read against a rate — was
//     roughly five times stricter than the bot it claimed to copy.
//   • The curve is not linear in the rate. Ψ multiplies the rate in four places, so it
//     falls as roughly the fourth power. Halving the rate does not halve Ψ.
//
// ── Bounded clamps at every step ─────────────────────────────────────────────
//
// `Bounded::new` is `clamp(0.0, 1.0)` and Rust's `.into()` calls it on every intermediate.
// Reproducing the formula without the intermediate clamps gives the same answer on
// healthy inputs and a different one wherever a term overflows — which is exactly the
// degraded case the gate exists for. `bounded()` below is applied at the same places.

/** `Bounded::new` — clamp into [0,1]. Applied at each intermediate, as in Rust. */
export const bounded = (v: number): number => (v < 0 ? 0 : v > 1 ? 1 : v)

/** `rationality.rs::EPSILON`. Guards a zero bias from dividing by zero. */
export const EPSILON = 1e-6

/** `overlay.rs::GO_THRESHOLD`. */
export const GO_THRESHOLD = 0.1
/** `overlay.rs::CAUTION_THRESHOLD`. Below this is HOLD, and HOLD is what halts buys. */
export const CAUTION_THRESHOLD = 0.02

export type Gate = 'GO' | 'CAUTION' | 'HOLD'

// ── the defaults the coherence breaker does not overwrite ────────────────────
//
// `coherence::assess` builds a `CognitiveState::initial()` and replaces five fields.
// Everything else keeps its `Default`, and those defaults are multiplied into Ψ — which
// is why Ψ maxes out below a quarter. Spelled out as named constants rather than folded
// into one number, because a reader has to be able to see that Ψ = 0.2055 is a healthy
// reading and not a fault.

/** `MetaCognitionInputs::default()` = (0.8, 0.75, 0.85, 0.9); `MC = Rc × Ec × Uc × Sc`. */
export const META_COGNITION = bounded(0.8 * 0.75 * 0.85 * 0.9)

/** `AgencyInputs::default()` = (0.9, 0.85, 0.85, 0.9, 0.85); `Ag = P × Mo × Ev × Dc × Fb`. */
export const AGENCY = bounded(0.9 * 0.85 * 0.85 * 0.9 * 0.85)

/** The feedback term `overlay.rs::current` passes as a literal `Bounded::new(0.9)`. */
export const FEEDBACK = 0.9

export interface PsiTerms {
  /** `SentienceIndex::compute` — `S = R × L × M × D`. */
  sentience: number
  /** `I` — the perception data ratio, which is also one of S's factors. */
  information: number
  /** `K` — knowledge density. */
  knowledge: number
  metaCognition: number
  agency: number
  feedback: number
  psi: number
}

/**
 * `MasterEquation::compute` for the inputs `coherence::assess` supplies.
 *
 * Not a general port of the crate — only the arm the breaker uses, with the terms it
 * leaves at their defaults folded in above. A general port would be four more files and
 * an invitation to drift on code Zero never executes.
 *
 * Multiplication order matches Rust's left-to-right `s * i * k * mc * ag * f`. Float
 * multiplication is not associative, so reordering these six terms changes the last bits
 * of the result and the fixture comparison stops being exact — which would leave nothing
 * to distinguish a rounding difference from a wrong equation.
 */
export function masterEquation(
  /** `Perception::new(1, 1, feedHealth, integrity)` → the sensory term. */
  feedHealth: number,
  /** The share of RPC-bound checks that resolved. Threaded through four terms. */
  resolutionRate: number,
): PsiTerms {
  const rr = resolutionRate

  // Perception: audio and visual are 1.0 — there is no such instrument here and an
  // unmeasured dimension takes the neutral element, never a "modest" 0.9. Ψ is a product,
  // so anything below 1.0 on an uninstrumented dimension is a standing tax that drags a
  // healthy pipeline toward the threshold. (`coherence.rs` says exactly this, having made
  // the mistake once in the API's gate.)
  const dataRatio = bounded(1.0 * 1.0 * bounded(feedHealth) * bounded(rr))

  // `RationalityInputs::new(rr, rr, 1.0, 0.0)` → `R = (E × Co × U) / (B + ε)`.
  // Bias is 0.0, so this saturates to 1.0 for any rr above about a thousandth — and is
  // exactly 0.0 at rr = 0, which is the arm that actually matters.
  const rationality = bounded((bounded(rr) * bounded(rr) * 1.0) / (0.0 + EPSILON))

  // `LogicInputs::new(1.0, rr, 1.0, 1.0)` — a pipeline reporting "passed" for checks it
  // never completed is internally inconsistent, which is what the consistency slot is.
  const logic = bounded(1.0 * bounded(rr) * 1.0 * 1.0)

  // `EthicsInputs::new(1, 1, 1, 1)` — not instrumented, so neutral.
  const moral = bounded(1.0 * 1.0 * 1.0 * 1.0)

  const sentience = bounded(rationality * logic * moral * dataRatio)
  const information = dataRatio
  const knowledge = bounded(Math.max(bounded(rr), 0.1))

  const psi = bounded(
    sentience * information * knowledge * META_COGNITION * AGENCY * FEEDBACK,
  )

  return {
    sentience,
    information,
    knowledge,
    metaCognition: META_COGNITION,
    agency: AGENCY,
    feedback: FEEDBACK,
    psi,
  }
}

/** `Overlay::gate`. */
export function gateOf(psi: number): Gate {
  if (psi >= GO_THRESHOLD) return 'GO'
  if (psi >= CAUTION_THRESHOLD) return 'CAUTION'
  return 'HOLD'
}

/**
 * The highest Ψ this equation can produce.
 *
 * Exported because it has to be on screen. A gauge whose maximum is 0.2055 renders as a
 * permanent quarter-full bar, and an operator reading it against an implied ceiling of
 * 1.0 concludes the bot is three-quarters broken when it is perfectly healthy. Same
 * reasoning as `measured_fraction` never being separated from Ψ on `/mesh`.
 */
export const PSI_MAX = masterEquation(1, 1).psi
