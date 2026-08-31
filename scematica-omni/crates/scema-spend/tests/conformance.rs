//! The settler contract, checked against the vectors a settler author also runs.
//!
//! `vectors/receipts.json` is the shared artefact. Omni checks its own `Receipt::validate`
//! against it here; a settler written in the bot workspace — or in another language, which is
//! the likely case for anything x402 — runs the same file against its emitter. Both sides are
//! then checked against **one document** rather than against each other's prose, which is the
//! arrangement `alchem-link/vectors/trust-model.json` already uses for the trust model.
//!
//! The alternative is a settler author reading `receipt.rs` and reimplementing what they think
//! it means. That has failed here before, in a smaller way: three producers hand-written
//! against a JSON shape drifted until `scema-tools/fixtures/` pinned them.
//!
//! ## What a passing run does and does not say
//!
//! It says the emitter and the consumer agree on **shape**. It says nothing about whether any
//! money moved — `fabricated-but-well-formed` is an accepted vector precisely so that reading
//! cannot survive contact with the suite.

use scema_spend::{Receipt, ReceiptError};
use serde_json::Value;

fn vectors() -> Value {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/vectors/receipts.json");
    let text = std::fs::read_to_string(path).expect("vectors/receipts.json");
    serde_json::from_str(&text).expect("vectors are valid JSON")
}

/// Map a vector's `error` name onto the variant, so the file names reasons rather than
/// positions. A settler author reads `no_reference`, not "the second error".
fn error_name(e: &ReceiptError) -> &'static str {
    match e {
        ReceiptError::NoSpendRecord => "no_spend_record",
        ReceiptError::NoReference => "no_reference",
        ReceiptError::NoDetail => "no_detail",
    }
}

#[test]
fn every_vector_agrees_with_this_implementation() {
    let v = vectors();
    let cases = v["vectors"].as_array().expect("vectors array");
    assert!(cases.len() >= 8, "the suite should not quietly shrink");

    for case in cases {
        let name = case["name"].as_str().unwrap();
        let expected = case["accepted"].as_bool().unwrap();

        let receipt: Receipt = serde_json::from_value(case["receipt"].clone())
            .unwrap_or_else(|e| panic!("{name}: a listed vector must parse: {e}"));

        match (receipt.validate(), expected) {
            (Ok(()), true) => {}
            (Err(e), false) => {
                let want = case["error"].as_str().unwrap_or_else(|| {
                    panic!("{name}: a rejected vector must name the error it expects")
                });
                assert_eq!(error_name(&e), want, "{name}: wrong reason");
            }
            (Ok(()), false) => panic!("{name}: accepted a receipt the vectors reject"),
            (Err(e), true) => panic!("{name}: rejected an accepted vector — {}", e.explain()),
        }
    }
}

#[test]
fn every_rejected_outcome_fails_to_parse_rather_than_failing_validation() {
    // The distinction matters. A value that parses and then fails validation is a settler
    // that got a detail wrong; one that does not parse at all is a settler that misunderstood
    // the contract, and that should be loud at the boundary rather than a quiet no-op.
    let v = vectors();
    for case in v["rejected_outcomes"]["cases"].as_array().unwrap() {
        let name = case["name"].as_str().unwrap();
        let parsed = serde_json::from_value::<Receipt>(case["receipt"].clone());
        assert!(parsed.is_err(), "{name}: `{}` must not parse as an outcome", name);
    }
}

#[test]
fn every_vector_says_why_it_exists() {
    // A vector without a reason gets deleted by the next person who finds it inconvenient.
    let v = vectors();
    let all = v["vectors"]
        .as_array()
        .unwrap()
        .iter()
        .chain(v["rejected_outcomes"]["cases"].as_array().unwrap().iter());
    for case in all {
        let name = case["name"].as_str().unwrap();
        let why = case["why"].as_str().unwrap_or("");
        assert!(why.len() > 40, "{name}: `why` must explain, not label");
    }
}

#[test]
fn the_suite_covers_acceptance_as_well_as_refusal() {
    // A conformance suite that only lists refusals teaches a settler author what not to do
    // and leaves them guessing at what a correct receipt looks like.
    let v = vectors();
    let cases = v["vectors"].as_array().unwrap();
    let accepted = cases.iter().filter(|c| c["accepted"].as_bool().unwrap()).count();
    let rejected = cases.len() - accepted;
    assert!(accepted >= 3, "only {accepted} accepted vector(s)");
    assert!(rejected >= 4, "only {rejected} rejected vector(s)");
}

#[test]
fn a_fabricated_reference_is_accepted_and_the_suite_says_so_out_loud() {
    // The single most important thing a settler author can misread. Pinned as its own test so
    // it cannot be lost in a bulk edit of the vectors file.
    let v = vectors();
    let case = v["vectors"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["name"] == "fabricated-but-well-formed")
        .expect("the suite must keep this case");
    assert_eq!(case["accepted"], true);
    assert!(
        case["why"].as_str().unwrap().contains("never truth"),
        "the reason must say the contract checks shape and not truth"
    );

    let receipt: Receipt = serde_json::from_value(case["receipt"].clone()).unwrap();
    assert_eq!(receipt.validate(), Ok(()));
}

#[test]
fn the_contract_is_versioned_so_a_settler_can_tell_which_one_it_read() {
    // The world contract learned this the expensive way: a format implemented in four
    // languages with no version is a silent misread waiting for the first change.
    let v = vectors();
    assert_eq!(v["contract"], "scema.receipt/1");
}
