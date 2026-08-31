//! The whole spend loop, against a settler that never touches a chain.
//!
//! `authorise → request → settle → receipt → ledger` end to end, so the arrangement is proven
//! to work *before* anybody writes code that can actually spend. Every case here is one a real
//! settler will meet, and the ones that matter most are the ones where nothing came back.

use scema_spend::{
    answers, authorise, settler::SettlementRequest, Amount, Ledger, Receipt, ReceiptOutcome,
    Script, ScriptedSettler, Settler, SpendPolicy, SpendRequest, Verdict,
};

fn policy() -> SpendPolicy {
    SpendPolicy {
        asset: "lamports".into(),
        per_transaction: 1_000,
        total: 2_000,
        capabilities: vec!["inference.rank".into()],
        payees: vec!["agent-b".into()],
    }
}

fn spend(units: u128) -> SpendRequest {
    SpendRequest {
        capability: "inference.rank".into(),
        payee: "agent-b".into(),
        amount: Amount::new(units, "lamports"),
        intent: None,
    }
}

fn request(id: &str, units: u128) -> SettlementRequest {
    SettlementRequest {
        capability: "inference.rank".into(),
        payee: "agent-b".into(),
        amount: Amount::new(units, "lamports"),
        intent: None,
        spend_record: id.into(),
    }
}

/// Apply a receipt the way `scema reconcile` does.
fn apply(ledger: &mut Ledger, req: &SettlementRequest, receipt: &Receipt) -> bool {
    assert_eq!(receipt.validate(), Ok(()), "a settler must emit a valid receipt");
    assert!(answers(req, receipt), "and one about the spend it was asked about");
    match receipt.outcome {
        ReceiptOutcome::Settled { .. } => ledger.settle(&req.spend_record, req.amount.units),
        ReceiptOutcome::Failed { .. } => false,
    }
}

#[test]
fn the_happy_path_charges_the_budget_once() {
    let mut ledger = Ledger::default();
    let req = spend(400);
    assert!(authorise(&policy(), &ledger, &req).permits());

    let s = ScriptedSettler::always_settles("5xQ...sig");
    let sr = request("aaaaaaaaaaaaaaaa", 400);
    let receipt = s.settle(&sr).expect("a receipt");

    assert!(apply(&mut ledger, &sr, &receipt));
    assert_eq!(ledger.spent, 400);
    assert_eq!(ledger.remaining(&policy()), 1_600);
}

#[test]
fn silence_leaves_the_budget_untouched_and_the_spend_unresolved() {
    // The case the whole design is arranged around. Nothing came back, so nothing is known,
    // so nothing is charged — and the spend stays visibly open for a human.
    let mut ledger = Ledger::default();
    let s = ScriptedSettler::new("quiet", vec![Script::Silence]);
    let sr = request("aaaaaaaaaaaaaaaa", 400);

    assert!(s.settle(&sr).is_none());
    assert_eq!(ledger.spent, 0);
    assert!(!ledger.has_settled(&sr.spend_record));
    let _ = &mut ledger;
}

#[test]
fn a_retry_after_silence_cannot_charge_twice_even_if_both_attempts_settled() {
    // The expensive mistake, made concrete. A caller treats silence as failure and retries;
    // the counterparty had in fact settled the first time and settles again. Whatever
    // happened on the wire, the *budget* is charged once, because the ledger keys on the
    // spend record rather than on the attempt.
    let mut ledger = Ledger::default();
    let s = ScriptedSettler::new(
        "flaky",
        vec![Script::Silence, Script::Settle { reference: "sig-2".into() }],
    );
    let sr = request("aaaaaaaaaaaaaaaa", 400);

    assert!(s.settle(&sr).is_none(), "first attempt: nothing came back");
    let receipt = s.settle(&sr).expect("second attempt answers");

    assert!(apply(&mut ledger, &sr, &receipt));
    assert!(!apply(&mut ledger, &sr, &receipt), "applying it again is a no-op");
    assert_eq!(ledger.spent, 400);
    assert_eq!(ledger.count, 1);
}

#[test]
fn a_failed_settlement_leaves_the_budget_for_the_next_attempt() {
    // Otherwise a counterparty that never delivers still exhausts the allowance.
    let mut ledger = Ledger::default();
    let s = ScriptedSettler::new("failing", vec![Script::Fail { detail: "refused".into() }]);
    let sr = request("aaaaaaaaaaaaaaaa", 400);
    let receipt = s.settle(&sr).unwrap();

    assert!(!apply(&mut ledger, &sr, &receipt));
    assert_eq!(ledger.remaining(&policy()), 2_000);
}

#[test]
fn the_budget_actually_binds_across_settled_spends() {
    // The defect this loop was built to close: before reconciliation existed, nothing wrote
    // the ledger and the total cap was inert. Three spends of 800 against a 2000 budget must
    // stop at two.
    let mut ledger = Ledger::default();
    let s = ScriptedSettler::new(
        "steady",
        vec![
            Script::Settle { reference: "s1".into() },
            Script::Settle { reference: "s2".into() },
        ],
    );

    for (i, id) in ["aaaaaaaaaaaaaaaa", "bbbbbbbbbbbbbbbb"].iter().enumerate() {
        let req = spend(800);
        assert!(authorise(&policy(), &ledger, &req).permits(), "spend {i} should be allowed");
        let sr = request(id, 800);
        let receipt = s.settle(&sr).unwrap();
        assert!(apply(&mut ledger, &sr, &receipt));
    }

    assert_eq!(ledger.spent, 1_600);
    let third = authorise(&policy(), &ledger, &spend(800));
    assert!(!third.permits(), "the third must be refused: {}", third.headline());
    assert!(third.headline().contains("400"), "and say what is left: {}", third.headline());
}

#[test]
fn a_receipt_for_the_wrong_spend_is_caught_before_the_ledger_is_touched() {
    // A settler working a queue can reply to the wrong item. The document is well-formed and
    // resolving this spend with it would charge the budget for a payment made elsewhere.
    let s = ScriptedSettler::new(
        "confused",
        vec![Script::WrongRecord { spend_record: "bbbbbbbbbbbbbbbb".into() }],
    );
    let sr = request("aaaaaaaaaaaaaaaa", 400);
    let receipt = s.settle(&sr).unwrap();

    assert_eq!(receipt.validate(), Ok(()), "the document itself is fine");
    assert!(!answers(&sr, &receipt), "but it does not answer this request");
}

#[test]
fn a_settler_is_only_ever_asked_about_an_authorised_spend() {
    // The ordering that keeps the policy meaningful: authorise, then hand off. A settler
    // asked about a refused spend would be a policy bypass with extra steps.
    let ledger = Ledger::default();
    let refused = SpendRequest { payee: "stranger".into(), ..spend(400) };
    let v = authorise(&policy(), &ledger, &refused);
    assert!(matches!(v, Verdict::Refused { .. }));
    // No `SettlementRequest` is constructed from a refused verdict anywhere in the CLI, and
    // there is no constructor that takes one — the type carries no verdict to check.
}
