//! Re-verify every record in the compatibility corpus.
//!
//! This is the mechanism behind the 1.0 promise: **a record sealed today still verifies
//! tomorrow.** `docs/COMPATIBILITY.md` states it; this test is what makes it checkable, and
//! it runs on every commit through the `omni` CI job.
//!
//! The corpus holds real records sealed by builds that no longer exist — including two from
//! before `WorldState::schema` was added. Those two are the sharp end. The field is
//! `Option` with `skip_serializing_if`, and both halves are load-bearing: make it required
//! and the old records fail to parse; serialise it as `null` and their canonical encoding
//! changes and the digest moves. Either way a verifier starts reporting untouched history as
//! tampered, which is the single failure that teaches a reader to stop believing it.
//!
//! ## Why the test lives in this crate
//!
//! It needs to verify both record types, and `scema-effect` is the only crate that can see
//! both — it depends on `scema-verify` for decisions and defines effects itself. Putting it
//! here is a dependency fact rather than a statement that effects are the important half.
//!
//! ## What a failure means
//!
//! Almost always that the change under test is wrong, not that the record is. The corpus is
//! not regenerated: re-sealing a file with today's build makes it agree with today's build
//! by construction, and the test stops detecting anything at all.

use std::path::PathBuf;

fn corpus_dir() -> Option<PathBuf> {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../corpus");
    p.is_dir().then_some(p)
}

/// Which kind of record a file holds, by shape.
///
/// By shape rather than by filename: a corpus file renamed for tidiness must not silently
/// change which verifier checks it.
fn kind(v: &serde_json::Value) -> Option<&'static str> {
    if v.get("world").is_some() && v.get("decision").is_some() {
        return Some("decision");
    }
    if v.get("effect").is_some() && v.get("outcome").is_some() {
        return Some("effect");
    }
    None
}

#[test]
fn every_record_in_the_corpus_still_verifies() {
    let Some(dir) = corpus_dir() else {
        eprintln!("SKIP: no corpus (published crate, or a partial checkout)");
        return;
    };

    let mut checked = 0usize;
    let mut failures = Vec::new();
    let mut kinds = std::collections::BTreeSet::new();

    for entry in std::fs::read_dir(&dir).expect("read corpus").flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        let text = std::fs::read_to_string(&path).expect("read record");
        let value: serde_json::Value = match serde_json::from_str(&text) {
            Ok(v) => v,
            Err(e) => {
                failures.push(format!("{name}: does not parse as JSON: {e}"));
                continue;
            }
        };

        match kind(&value) {
            Some("decision") => {
                kinds.insert("decision");
                match serde_json::from_value::<scema_verify::DecisionRecord>(value) {
                    Ok(record) => {
                        let v = scema_verify::verify(&record);
                        if !v.valid {
                            failures.push(format!(
                                "{name}: commitment no longer verifies — {:?}",
                                v.mismatches
                            ));
                        }
                        checked += 1;
                    }
                    Err(e) => failures.push(format!(
                        "{name}: no longer deserialises as a DecisionRecord: {e}"
                    )),
                }
            }
            Some("effect") => {
                kinds.insert("effect");
                match serde_json::from_value::<scema_effect::EffectRecord>(value) {
                    Ok(record) => {
                        let v = scema_effect::verify(&record);
                        if !v.valid {
                            failures.push(format!(
                                "{name}: commitment no longer verifies — {:?}",
                                v.mismatches
                            ));
                        }
                        checked += 1;
                    }
                    Err(e) => failures.push(format!(
                        "{name}: no longer deserialises as an EffectRecord: {e}"
                    )),
                }
            }
            _ => failures.push(format!("{name}: is neither a decision nor an effect record")),
        }
    }

    assert!(
        failures.is_empty(),
        "\nThe compatibility corpus no longer verifies. This is almost certainly the change \
         under test, not the records — they were sealed by builds that no longer exist. Do \
         not regenerate them; a re-sealed record agrees with today's build by construction \
         and detects nothing.\n\n{}",
        failures.join("\n")
    );

    // A corpus that quietly checks nothing is worse than none: it reports success forever.
    assert!(checked >= 4, "expected at least four corpus records, checked {checked}");
    assert!(
        kinds.contains("decision") && kinds.contains("effect"),
        "the corpus must cover both record types; it covered {kinds:?}"
    );
}

#[test]
fn the_corpus_still_contains_a_record_from_before_the_schema_field() {
    // The sharpest case, asserted separately so that losing it is a red test rather than a
    // silently weaker suite. `WorldState::schema` is `Option` + `skip_serializing_if`, and
    // both halves matter: required breaks parsing, and serialising `null` changes the
    // canonical encoding and moves the digest.
    let Some(dir) = corpus_dir() else {
        eprintln!("SKIP: no corpus");
        return;
    };
    let mut found = 0;
    for entry in std::fs::read_dir(&dir).expect("read corpus").flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let text = std::fs::read_to_string(&path).expect("read");
        let v: serde_json::Value = match serde_json::from_str(&text) {
            Ok(v) => v,
            Err(_) => continue,
        };
        if let Some(world) = v.get("world") {
            if world.get("schema").is_none() {
                found += 1;
            }
        }
    }
    assert!(
        found >= 2,
        "the corpus must keep at least two pre-schema records; found {found}. \
         One could pass by accident."
    );
}
