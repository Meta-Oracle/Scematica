//! The settlement contract — what a settler hands back, and what omni will accept.
//!
//! `scema pay` authorises and stops. Something else moves the money, because omni cannot link
//! `scematica-protocol` (solana-sdk). The boundary between them is this JSON shape, chosen
//! the same way the `WorldState` producer contract was: **a format, not a trait**, because the
//! two sides cannot link and a compiler will never stand between them.
//!
//! That has a consequence worth stating. Nothing here can verify that money moved — a settler
//! could fabricate a reference and this would accept it. What the contract *does* buy is that
//! a settler cannot be vague: a receipt names one spend record, one outcome, and for a
//! settlement a reference somebody can go and check on a chain. The verification is a human or
//! a chain lookup, and the receipt is the thing they check against.
//!
//! ## Why `Unknown` is not an accepted outcome
//!
//! A settler reporting "I don't know" is reporting the state the record is already in, so
//! accepting it would let reconciliation appear to make progress while changing nothing. If a
//! settler genuinely cannot tell, it should emit no receipt — the spend stays `Unknown` and
//! stays visible as needing a human. Silence is the honest signal there.

use serde::{Deserialize, Serialize};

/// What a settler says became of one authorised spend.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum ReceiptOutcome {
    /// Money moved. `reference` is the counterparty's — a transaction signature, an x402
    /// receipt id — recorded verbatim and never parsed here. It exists so a human or a chain
    /// lookup can check the claim; this crate cannot.
    Settled { reference: String },
    /// It definitely did not happen and no money moved. Safe to retry.
    Failed { detail: String },
}

/// One settler's report.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Receipt {
    /// The `SpendRecord` id this is about. Exactly one.
    pub spend_record: String,
    #[serde(flatten)]
    pub outcome: ReceiptOutcome,
    /// Who produced this receipt. Recorded so a reconciliation can say whose word it is —
    /// the same reason `ImportObserver` rewrites `observer` to `imported:<name>`.
    #[serde(default)]
    pub settler: String,
}

/// Why a receipt was rejected.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReceiptError {
    /// No `spend_record`, or one that is not a plausible record id.
    NoSpendRecord,
    /// A settlement with no reference. Refused: the reference is the entire difference
    /// between a claim somebody can check and one they must take on trust.
    NoReference,
    /// A failure with no explanation. Refused for the same reason every refusal in this
    /// workspace explains itself — an operator has to know whether to retry.
    NoDetail,
}

impl ReceiptError {
    pub fn explain(&self) -> String {
        match self {
            ReceiptError::NoSpendRecord =>
                "a receipt must name exactly one spend record".into(),
            ReceiptError::NoReference =>
                "a settled receipt must carry a reference — without one the claim that money \
                 moved cannot be checked by anybody, which is the only thing this contract buys"
                    .into(),
            ReceiptError::NoDetail =>
                "a failed receipt must say why, or nobody can tell whether to retry".into(),
        }
    }
}

impl Receipt {
    /// Check the shape. Says nothing about whether the claim is true.
    pub fn validate(&self) -> Result<(), ReceiptError> {
        if self.spend_record.trim().is_empty() || self.spend_record.len() < 8 {
            return Err(ReceiptError::NoSpendRecord);
        }
        match &self.outcome {
            ReceiptOutcome::Settled { reference } if reference.trim().is_empty() => {
                Err(ReceiptError::NoReference)
            }
            ReceiptOutcome::Failed { detail } if detail.trim().is_empty() => {
                Err(ReceiptError::NoDetail)
            }
            _ => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settled() -> Receipt {
        Receipt {
            spend_record: "dd4c36f4cc82292f".into(),
            outcome: ReceiptOutcome::Settled { reference: "5xQ...sig".into() },
            settler: "scematica-protocol/1.28.0".into(),
        }
    }

    #[test]
    fn a_settlement_round_trips_through_json() {
        let text = serde_json::to_string(&settled()).unwrap();
        assert_eq!(serde_json::from_str::<Receipt>(&text).unwrap(), settled());
        assert!(text.contains("\"outcome\":\"settled\""), "{text}");
    }

    #[test]
    fn a_settlement_without_a_reference_is_refused() {
        // The reference is the entire difference between a claim somebody can check and one
        // they must take on trust. A settler that cannot produce one has not settled.
        let mut r = settled();
        r.outcome = ReceiptOutcome::Settled { reference: "  ".into() };
        assert_eq!(r.validate(), Err(ReceiptError::NoReference));
    }

    #[test]
    fn a_failure_without_a_reason_is_refused() {
        let r = Receipt {
            spend_record: "dd4c36f4cc82292f".into(),
            outcome: ReceiptOutcome::Failed { detail: String::new() },
            settler: "x".into(),
        };
        assert_eq!(r.validate(), Err(ReceiptError::NoDetail));
    }

    #[test]
    fn a_receipt_must_name_one_record() {
        let mut r = settled();
        r.spend_record = String::new();
        assert_eq!(r.validate(), Err(ReceiptError::NoSpendRecord));
        r.spend_record = "short".into();
        assert_eq!(r.validate(), Err(ReceiptError::NoSpendRecord));
    }

    #[test]
    fn unknown_is_not_an_accepted_outcome() {
        // A settler reporting "I don't know" is reporting the state the record is already in.
        // Accepting it would let reconciliation appear to progress while changing nothing; a
        // settler that cannot tell should emit no receipt at all.
        let text = r#"{"spend_record":"dd4c36f4cc82292f","outcome":"unknown"}"#;
        assert!(serde_json::from_str::<Receipt>(text).is_err());
    }

    #[test]
    fn a_valid_receipt_says_nothing_about_whether_money_moved() {
        // Stated as a test because `validate` invites the opposite reading. Nothing in this
        // process can check a chain; the reference exists so somebody else can.
        assert_eq!(settled().validate(), Ok(()));
        let fabricated = Receipt {
            spend_record: "dd4c36f4cc82292f".into(),
            outcome: ReceiptOutcome::Settled { reference: "entirely-made-up".into() },
            settler: "liar".into(),
        };
        assert_eq!(fabricated.validate(), Ok(()), "shape is all this checks");
    }
}
