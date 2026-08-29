//! The protected-path half of the shared trust vectors, run against this implementation.
//!
//! `PROTECTED_PATTERNS` exists twice — here and in `alchem_link.workspace` — and two copies
//! of a secrets list drift in the worst possible direction: silently, and only for the
//! pattern nobody thought to add. `alchem-link/vectors/trust-model.json` carries the paths
//! both sides must refuse and the paths both sides must leave alone, and Python already
//! runs them.
//!
//! The second list matters as much as the first. A runtime that refuses to read
//! `docs/env-setup.md` because the name contains "env" is one whose confinement gets
//! switched off by an irritated operator, and then nothing is protected.
//!
//! Skips when the vectors are absent — a published `scema-tools` does not carry a sibling
//! tree — and says so, because a conformance suite that quietly runs zero cases is worse
//! than one that fails.

use std::path::{Path, PathBuf};

use scema_tools::workspace::is_protected;

fn vectors_path() -> Option<PathBuf> {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../alchem-link/vectors/trust-model.json");
    p.exists().then_some(p)
}

#[test]
fn the_protected_list_agrees_with_python() {
    let Some(path) = vectors_path() else {
        eprintln!("SKIP: vectors not present (published crate, or a partial checkout)");
        return;
    };
    let text = std::fs::read_to_string(&path).expect("read vectors");
    let doc: serde_json::Value = serde_json::from_str(&text).expect("parse vectors");
    let block = &doc["protected_paths"];

    let list = |key: &str| -> Vec<String> {
        block[key]
            .as_array()
            .unwrap_or_else(|| panic!("vectors are missing protected_paths.{key}"))
            .iter()
            .filter_map(|v| v.as_str().map(str::to_string))
            .collect()
    };

    let refused = list("refused");
    let allowed = list("allowed");
    assert!(!refused.is_empty() && !allowed.is_empty(), "both lists must be exercised");

    let mut failures = Vec::new();
    for p in &refused {
        if !is_protected(Path::new(p)) {
            failures.push(format!("{p}: Python refuses this, Rust does not"));
        }
    }
    for p in &allowed {
        if is_protected(Path::new(p)) {
            failures.push(format!("{p}: Rust refuses this, Python does not"));
        }
    }

    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
    eprintln!(
        "{} refused and {} allowed path(s) agreed with Python",
        refused.len(),
        allowed.len()
    );
}
