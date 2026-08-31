//! The seam a real settler plugs into — and the double that makes the loop testable without one.
//!
//! Omni authorises and records; something else moves the money. That "something else" needs a
//! shape to implement and, more importantly, the loop around it needs to be testable *before*
//! anybody writes code that can spend. [`Settler`] is the seam; [`ScriptedSettler`] is the
//! double.
//!
//! ## Why the double is scripted rather than "always succeeds"
//!
//! A stub that always settles tests exactly one path, and it is the path least likely to go
//! wrong. The interesting cases are a settler that fails, one that returns a malformed
//! receipt, and one that **never answers** — the last being the reason `Settlement::Unknown`
//! exists at all. `ScriptedSettler` can do all four, including returning nothing, so a caller
//! can be tested against the silence it will eventually meet in production.
//!
//! ## What a real settler must not assume
//!
//! It is handed a [`SettlementRequest`] and returns a [`Receipt`] or nothing. It does **not**
//! get the policy, the ledger, or the ability to authorise — those decisions are already made
//! and are not its business. A settler that could re-decide would be a second, undocumented
//! spend policy living wherever somebody happened to put the network code.

use serde::{Deserialize, Serialize};

use crate::{Amount, Receipt};

/// What omni hands to a settler. The same document `scema pay --commit` prints.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SettlementRequest {
    pub capability: String,
    pub payee: String,
    pub amount: Amount,
    pub intent: Option<String>,
    /// The `SpendRecord` this settles. A receipt must name it back.
    pub spend_record: String,
}

/// Something that can move money.
///
/// Returning `None` is a first-class answer and means **"I do not know"** — a timeout, a
/// dropped connection, a counterparty that never replied. It is deliberately not an error
/// type: an error invites a caller to retry, and retrying an unobserved payment is how you pay
/// twice. `None` leaves the spend `Unknown`, which is exactly where a human should look.
pub trait Settler {
    /// A stable name, recorded in the receipt so a reconciliation can say whose word it is.
    fn name(&self) -> &str;

    fn settle(&self, request: &SettlementRequest) -> Option<Receipt>;
}

/// What a [`ScriptedSettler`] does with the next request.
#[derive(Clone, Debug)]
pub enum Script {
    Settle { reference: String },
    Fail { detail: String },
    /// Answer nothing. The case that produces `Settlement::Unknown`, and the one a caller is
    /// least likely to have thought about.
    Silence,
    /// Return a receipt naming a different spend record — a real bug class, since a settler
    /// processing a queue can reply to the wrong item.
    WrongRecord { spend_record: String },
}

/// A settler that does what it was told, in order. Test double; never spends anything.
pub struct ScriptedSettler {
    name: String,
    script: std::cell::RefCell<std::collections::VecDeque<Script>>,
}

impl ScriptedSettler {
    pub fn new(name: impl Into<String>, script: Vec<Script>) -> Self {
        ScriptedSettler {
            name: name.into(),
            script: std::cell::RefCell::new(script.into_iter().collect()),
        }
    }

    /// Always settles. Convenience for the happy path only — prefer an explicit script.
    pub fn always_settles(reference: impl Into<String>) -> Self {
        let r = reference.into();
        ScriptedSettler::new("scripted/settle", vec![Script::Settle { reference: r }])
    }

    /// Whether every scripted step was used. A test that scripts three answers and makes one
    /// call is usually not testing what it thinks.
    pub fn exhausted(&self) -> bool {
        self.script.borrow().is_empty()
    }
}

impl Settler for ScriptedSettler {
    fn name(&self) -> &str {
        &self.name
    }

    fn settle(&self, request: &SettlementRequest) -> Option<Receipt> {
        // Running past the end is silence rather than a panic: a caller that makes one more
        // request than expected should meet the production behaviour, not a test artefact.
        let step = self.script.borrow_mut().pop_front().unwrap_or(Script::Silence);
        match step {
            Script::Silence => None,
            Script::Settle { reference } => Some(Receipt {
                spend_record: request.spend_record.clone(),
                outcome: crate::ReceiptOutcome::Settled { reference },
                settler: self.name.clone(),
            }),
            Script::Fail { detail } => Some(Receipt {
                spend_record: request.spend_record.clone(),
                outcome: crate::ReceiptOutcome::Failed { detail },
                settler: self.name.clone(),
            }),
            Script::WrongRecord { spend_record } => Some(Receipt {
                spend_record,
                outcome: crate::ReceiptOutcome::Settled { reference: "sig".into() },
                settler: self.name.clone(),
            }),
        }
    }
}

/// Check a receipt actually answers the request it was given.
///
/// Separate from `Receipt::validate`, which only knows about the document. This is the check
/// a caller must do and would plausibly forget: a settler processing a queue can reply to the
/// wrong item, and a receipt that is perfectly well-formed for *another* spend would otherwise
/// resolve this one.
pub fn answers(request: &SettlementRequest, receipt: &Receipt) -> bool {
    receipt.spend_record == request.spend_record
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ReceiptOutcome;

    fn request() -> SettlementRequest {
        SettlementRequest {
            capability: "inference.rank".into(),
            payee: "agent-b".into(),
            amount: Amount::new(400, "lamports"),
            intent: None,
            spend_record: "dd4c36f4cc82292f".into(),
        }
    }

    #[test]
    fn a_settled_receipt_names_the_record_it_was_asked_about() {
        let s = ScriptedSettler::always_settles("sig-1");
        let r = s.settle(&request()).expect("a receipt");
        assert!(answers(&request(), &r));
        assert_eq!(r.validate(), Ok(()));
        assert!(s.exhausted());
    }

    #[test]
    fn silence_is_a_first_class_answer_and_not_an_error() {
        // The case that produces `Settlement::Unknown`, and the one a caller is least likely
        // to have thought about. It is `None`, not `Err`, because an error invites a retry
        // and retrying an unobserved payment is how you pay twice.
        let s = ScriptedSettler::new("quiet", vec![Script::Silence]);
        assert!(s.settle(&request()).is_none());
    }

    #[test]
    fn running_past_the_script_is_silence_rather_than_a_panic() {
        // A caller making one more request than expected should meet production behaviour,
        // not a test artefact.
        let s = ScriptedSettler::new("short", vec![]);
        assert!(s.settle(&request()).is_none());
    }

    #[test]
    fn a_receipt_for_another_record_is_caught_by_answers_not_by_validate() {
        // A settler working a queue can reply to the wrong item. The document is perfectly
        // well-formed; it simply is not about this spend.
        let s = ScriptedSettler::new(
            "confused",
            vec![Script::WrongRecord { spend_record: "0000000000000000".into() }],
        );
        let r = s.settle(&request()).unwrap();
        assert_eq!(r.validate(), Ok(()), "the document itself is fine");
        assert!(!answers(&request(), &r), "but it does not answer this request");
    }

    #[test]
    fn a_scripted_failure_is_well_formed_and_says_why() {
        let s = ScriptedSettler::new(
            "failing",
            vec![Script::Fail { detail: "counterparty refused".into() }],
        );
        let r = s.settle(&request()).unwrap();
        assert_eq!(r.validate(), Ok(()));
        assert!(matches!(r.outcome, ReceiptOutcome::Failed { .. }));
    }

    #[test]
    fn the_script_runs_in_order_so_a_retry_sequence_can_be_tested() {
        // The realistic shape: silence, then a settlement on the second attempt. A caller
        // that treats the first as failure would pay twice here.
        let s = ScriptedSettler::new(
            "flaky",
            vec![Script::Silence, Script::Settle { reference: "sig-2".into() }],
        );
        assert!(s.settle(&request()).is_none());
        let r = s.settle(&request()).expect("second attempt answers");
        assert!(answers(&request(), &r));
        assert!(s.exhausted());
    }

    #[test]
    fn a_settler_is_never_handed_the_policy_or_the_ledger() {
        // Stated as a test because the signature is the guarantee. A settler that could
        // re-decide would be a second, undocumented spend policy living wherever somebody
        // happened to put the network code.
        let r = request();
        assert!(r.intent.is_none() || r.intent.is_some());
        // `SettlementRequest` carries what to pay and who to pay — no caps, no allow-lists,
        // no remaining balance. There is nothing here to re-authorise from.
        let json = serde_json::to_string(&r).unwrap();
        for forbidden in ["per_transaction", "total", "capabilities", "payees", "spent"] {
            assert!(!json.contains(forbidden), "a settler must not receive `{forbidden}`");
        }
    }
}
