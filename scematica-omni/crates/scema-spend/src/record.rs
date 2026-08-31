//! What the agent actually paid, sealed.
//!
//! The same shape as `scema-effect`, and for the same reason: a runtime that can act must be
//! able to say afterwards what it did, in a form somebody who was not there can check.
//!
//! ## `Settlement::Unknown` is a first-class arm
//!
//! An authorised spend whose settlement could not be observed is **neither paid nor unpaid**.
//! Money and network failures overlap badly: a request that timed out may have settled, and
//! recording it as `Failed` would let the agent retry and pay twice. `scema-effect` reached
//! the same conclusion for effects and exits 3 rather than 0 or 1; this exists so the same
//! honesty is available for money, where the cost of guessing is higher.
//!
//! ## The record does not prove payment
//!
//! It proves what the agent *decided* and what it *observed*. A counterparty's receipt or a
//! chain lookup proves payment, and neither of those is in this process. Stated here because
//! a sealed record is persuasive, and a persuasive artefact that is mistaken for a proof of
//! funds movement is worse than no artefact.

use serde::{Deserialize, Serialize};

use crate::{SpendRequest, Verdict};

/// What became of an authorised spend.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum Settlement {
    /// The counterparty acknowledged. `reference` is theirs — a transaction hash, an x402
    /// receipt id — recorded verbatim and never parsed here.
    Settled { reference: String },
    /// It definitely did not happen, and no money moved.
    Failed { detail: String },
    /// **Nobody knows.** A timeout, a dropped connection, a counterparty that never replied.
    /// Not a failure: retrying could pay twice.
    Unknown { detail: String },
    /// The policy allowed it and `--commit` was not given. Nothing was attempted.
    DryRun,
    /// The policy refused it. Recorded because a refusal is a decision worth keeping — the
    /// pattern of what an agent *wanted* to buy is exactly what a spend policy is for.
    Refused { reason: String },
}

impl Settlement {
    /// Whether this consumed budget. Only a settled spend does.
    ///
    /// `Unknown` deliberately returns `false`: charging the budget for a spend that may not
    /// have happened would let a flaky counterparty exhaust an allowance. The opposite risk —
    /// under-counting a spend that did settle — is real and is why `Unknown` must be
    /// reconciled by hand rather than resolved by a default.
    pub fn consumed_budget(&self) -> bool {
        matches!(self, Settlement::Settled { .. })
    }

    /// Process exit code, matching `scema-effect`'s convention.
    pub fn exit_code(&self) -> u8 {
        match self {
            Settlement::Settled { .. } | Settlement::DryRun => 0,
            Settlement::Failed { .. } | Settlement::Refused { .. } => 1,
            // Its own code, so a sequence cannot continue past an unobserved payment.
            Settlement::Unknown { .. } => 3,
        }
    }

    pub fn headline(&self) -> String {
        match self {
            Settlement::Settled { reference } => format!("settled — {reference}"),
            Settlement::Failed { detail } => format!("failed — {detail}"),
            Settlement::Unknown { detail } => format!(
                "UNKNOWN — {detail}. This is neither paid nor unpaid: retrying may pay twice. \
                 Reconcile with the counterparty before spending again."
            ),
            Settlement::DryRun => "dry run — nothing was attempted".into(),
            Settlement::Refused { reason } => format!("refused — {reason}"),
        }
    }
}

/// A sealed account of one spend.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SpendRecord {
    pub id: String,
    pub at: i64,
    pub runtime: String,
    pub request: SpendRequest,
    pub verdict: Verdict,
    pub settlement: Settlement,
    /// Digest over the fields above, so an edited record is detectable.
    pub commitment: String,
}

impl SpendRecord {
    pub fn seal(
        runtime: impl Into<String>,
        at: i64,
        request: SpendRequest,
        verdict: Verdict,
        settlement: Settlement,
    ) -> Self {
        let runtime = runtime.into();
        let body = serde_json::json!({
            "at": at,
            "runtime": runtime,
            "request": request,
            "verdict": verdict,
            "settlement": settlement,
        });
        let digest = scema_verify::canonical::digest(&body).to_hex();
        SpendRecord {
            id: digest[..16].to_string(),
            at,
            runtime,
            request,
            verdict,
            settlement,
            commitment: digest,
        }
    }

    /// Recompute the commitment and report whether it still matches.
    pub fn verify(&self) -> bool {
        let body = serde_json::json!({
            "at": self.at,
            "runtime": self.runtime,
            "request": self.request,
            "verdict": self.verdict,
            "settlement": self.settlement,
        });
        scema_verify::canonical::digest(&body).to_hex() == self.commitment
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Amount, Refusal, SpendRequest, Verdict};

    fn req() -> SpendRequest {
        SpendRequest {
            capability: "inference.rank".into(),
            payee: "agent-b".into(),
            amount: Amount::new(400, "lamports"),
            intent: Some("07aecde6".into()),
        }
    }

    fn rec(s: Settlement) -> SpendRecord {
        SpendRecord::seal("test/1", 0, req(), Verdict::Allowed { remaining_after: 1 }, s)
    }

    #[test]
    fn a_sealed_record_verifies_and_an_edited_one_does_not() {
        let r = rec(Settlement::Settled { reference: "0xdeadbeef".into() });
        assert!(r.verify());

        let mut tampered = r.clone();
        tampered.request.amount.units = 1;
        assert!(!tampered.verify(), "an edited amount must be detectable");
    }

    #[test]
    fn only_a_settled_spend_consumes_budget() {
        assert!(Settlement::Settled { reference: "x".into() }.consumed_budget());
        assert!(!Settlement::Failed { detail: "x".into() }.consumed_budget());
        assert!(!Settlement::DryRun.consumed_budget());
        assert!(!Settlement::Refused { reason: "x".into() }.consumed_budget());
    }

    #[test]
    fn an_unknown_settlement_does_not_consume_budget_but_is_not_a_failure() {
        // Both halves matter. Charging for a spend that may not have happened lets a flaky
        // counterparty exhaust an allowance; calling it `Failed` invites a retry that pays
        // twice. It is its own arm precisely because neither default is safe.
        let u = Settlement::Unknown { detail: "timeout".into() };
        assert!(!u.consumed_budget());
        assert_eq!(u.exit_code(), 3, "a sequence must not continue past an unobserved payment");
        assert!(u.headline().contains("pay twice"));
    }

    #[test]
    fn a_dry_run_exits_zero_and_a_refusal_does_not() {
        assert_eq!(Settlement::DryRun.exit_code(), 0);
        assert_eq!(Settlement::Refused { reason: "over budget".into() }.exit_code(), 1);
    }

    #[test]
    fn a_refusal_is_recorded_rather_than_discarded() {
        // The pattern of what an agent wanted to buy is exactly what a spend policy is for,
        // and it is invisible if refusals leave no trace.
        let r = SpendRecord::seal(
            "test/1",
            0,
            req(),
            Verdict::Refused { refusal: Refusal::ZeroAmount },
            Settlement::Refused { reason: "zero".into() },
        );
        assert!(r.verify());
        assert!(matches!(r.verdict, Verdict::Refused { .. }));
    }

    #[test]
    fn the_id_is_a_prefix_of_the_commitment_so_neither_can_drift() {
        let r = rec(Settlement::DryRun);
        assert!(r.commitment.starts_with(&r.id));
        assert_eq!(r.id.len(), 16);
    }

    #[test]
    fn two_spends_differing_anywhere_seal_differently() {
        let a = rec(Settlement::DryRun);
        let b = rec(Settlement::Failed { detail: "no".into() });
        assert_ne!(a.commitment, b.commitment);

        let mut other = req();
        other.payee = "agent-c".into();
        let c = SpendRecord::seal(
            "test/1", 0, other, Verdict::Allowed { remaining_after: 1 }, Settlement::DryRun,
        );
        assert_ne!(a.commitment, c.commitment);
    }
}
