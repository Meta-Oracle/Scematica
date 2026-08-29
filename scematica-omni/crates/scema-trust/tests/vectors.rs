//! The shared conformance vectors, run against this implementation.
//!
//! `alchem-link/vectors/trust-model.json` is the contract in
//! `alchem-link/docs/TRUST-MODEL.md`. Python is the reference implementation and already
//! passes these; this asks whether the Rust port agrees. Same arrangement as
//! `canonical.rs` / `canonical.ts`: one stated rule, one file both sides run, and whichever
//! side fails is the wrong one.
//!
//! ## Why it skips rather than fails when the file is missing
//!
//! The vectors live in a sibling tree, not in this crate. A published `scema-trust` does not
//! carry them, so a consumer running `cargo test` would otherwise see a failure that says
//! nothing about the code. Skipping is the same call `plugins/scema-web` makes for its wire
//! tests, and the skip is announced rather than silent — a conformance suite that quietly
//! runs zero cases is worse than one that fails.

use std::path::PathBuf;

use scema_trust::{Decision, Request, Risk, Rule, TrustPolicy};

/// Locate the vectors relative to this crate, or `None` outside the repository.
fn vectors_path() -> Option<PathBuf> {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../alchem-link/vectors/trust-model.json");
    p.exists().then_some(p)
}

/// A minimal reader for the vector file.
///
/// Hand-parsed because this crate has no dependencies and adding `serde_json` for a test
/// would mean the thing under test drags a parser into every consumer's build. The file is
/// ours and its shape is fixed by the spec.
fn field<'a>(block: &'a str, key: &str) -> Option<&'a str> {
    let needle = format!("\"{key}\"");
    let at = block.find(&needle)?;
    let rest = &block[at + needle.len()..];
    let colon = rest.find(':')?;
    Some(rest[colon + 1..].trim_start())
}

fn string_field(block: &str, key: &str) -> Option<String> {
    let v = field(block, key)?;
    let v = v.strip_prefix('"')?;
    let end = v.find('"')?;
    Some(v[..end].to_string())
}

fn bool_field(block: &str, key: &str) -> bool {
    field(block, key).is_some_and(|v| v.starts_with("true"))
}

/// Split the `cases` array into per-case blocks by brace depth.
fn cases(text: &str) -> Vec<String> {
    let Some(start) = text.find("\"cases\"") else { return vec![] };
    let bytes: Vec<char> = text[start..].chars().collect();
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut buf = String::new();
    let mut in_array = false;
    for c in bytes {
        if !in_array {
            if c == '[' {
                in_array = true;
            }
            continue;
        }
        match c {
            '{' => {
                depth += 1;
                buf.push(c);
            }
            '}' => {
                depth -= 1;
                buf.push(c);
                if depth == 0 {
                    out.push(std::mem::take(&mut buf));
                }
            }
            ']' if depth == 0 => break,
            _ if depth > 0 => buf.push(c),
            _ => {}
        }
    }
    out
}

/// Everything between `"policy": {` and its matching brace.
fn subobject(block: &str, key: &str) -> String {
    let needle = format!("\"{key}\"");
    let Some(at) = block.find(&needle) else { return String::new() };
    let rest = &block[at..];
    let Some(open) = rest.find('{') else { return String::new() };
    let mut depth = 0i32;
    let mut out = String::new();
    for c in rest[open..].chars() {
        match c {
            '{' => {
                depth += 1;
                if depth > 1 {
                    out.push(c);
                }
            }
            '}' => {
                depth -= 1;
                if depth == 0 {
                    break;
                }
                out.push(c);
            }
            _ => out.push(c),
        }
    }
    out
}

fn build_policy(spec: &str) -> TrustPolicy {
    // Built through the public surface rather than a struct literal: `grants` is private,
    // which is the point — a grant may only arrive through `grant` or `remember`, so no
    // caller can install one without going past the settling rule.
    let mut p = TrustPolicy::new();
    p.read_only = bool_field(spec, "read_only");
    p.allow_writes = bool_field(spec, "allow_writes");
    p.allow_execute = bool_field(spec, "allow_execute");

    // Rules: an array of flat objects, in order. Order is part of the policy.
    if let Some(at) = spec.find("\"rules\"") {
        for rule in cases(&format!("\"cases\"{}", &spec[at + 7..])) {
            let (Some(tool), Some(decision)) =
                (string_field(&rule, "tool"), string_field(&rule, "decision"))
            else {
                continue;
            };
            let d = match decision.as_str() {
                "allow" => Decision::Allow,
                "deny" => Decision::Deny,
                other => panic!("unknown decision in vectors: {other}"),
            };
            let path = string_field(&rule, "path").unwrap_or_else(|| "*".into());
            p.rules.push(Rule::new(tool, d).at(path));
        }
    }

    // Session grants: a flat object of key -> "allow" | "deny".
    let grants = subobject(spec, "session_grants");
    for part in grants.split(',') {
        // `rsplit_once`, not `split_once`: a grant key is `tool:dirname` and therefore
        // contains a colon itself. Splitting on the first one yields the key `"write_file`
        // and the value `docs": "allow"`, which silently installs no grant at all — the
        // case then falls through to the standing configuration and the vector reports the
        // library as wrong when the harness is.
        let Some((k, v)) = part.rsplit_once(':') else { continue };
        let key = k.trim().trim_matches('"');
        let val = v.trim().trim_matches(|c| c == '"' || c == ' ');
        if key.is_empty() {
            continue;
        }
        match val {
            "allow" => p.grant(key, Decision::Allow),
            "deny" => p.grant(key, Decision::Deny),
            _ => continue,
        }
    }
    p
}

#[test]
fn the_rust_port_agrees_with_the_shared_vectors() {
    let Some(path) = vectors_path() else {
        eprintln!("SKIP: vectors not present (published crate, or a partial checkout)");
        return;
    };
    let text = std::fs::read_to_string(&path).expect("read vectors");
    let cases = cases(&text);
    assert!(cases.len() >= 20, "expected the full vector set, got {}", cases.len());

    let mut failures = Vec::new();
    let mut seen_ask = false;

    for case in &cases {
        let name = string_field(case, "name").unwrap_or_default();
        let expected = string_field(case, "expected").unwrap_or_default();
        if expected == "ask" {
            seen_ask = true;
        }

        let policy = build_policy(&subobject(case, "policy"));
        let req_block = subobject(case, "request");
        let tool = string_field(&req_block, "tool").expect("tool");
        let risk = Risk::parse(&string_field(&req_block, "risk").expect("risk"))
            .expect("vectors must use a known risk");
        let path = string_field(&req_block, "path").unwrap_or_default();

        let request = Request::new(tool, risk).at(path);
        let actual = match policy.preflight(&request) {
            None => "ask".to_string(),
            Some(d) => d.as_str().to_string(),
        };
        if actual != expected {
            failures.push(format!("{name}: expected {expected}, got {actual}"));
        }
    }

    // A vector file where nothing asks would pass against an implementation that never
    // prompts, which is the most dangerous way for this to be wrong.
    assert!(seen_ask, "the vectors must exercise `ask`");
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
    eprintln!("{} vector(s) agreed with Python", cases.len());
}
