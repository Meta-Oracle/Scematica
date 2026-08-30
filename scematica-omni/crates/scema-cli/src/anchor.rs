//! `scema anchor` — batch sealed records under one root, and pin it somewhere else.
//!
//! ```text
//! scema anchor                              # build a batch over every sealed record
//! scema anchor --proof 8f92a1c4             # the inclusion proof for one record
//! scema anchor --list                       # batches, and where each is published
//! scema anchor --record base=0xabc… --batch 4f21…   # note a submission that happened
//! scema anchor --check proof.json --root 4f21…      # verify a proof, offline
//! ```
//!
//! ## What this closes
//!
//! `scema verify` proves a record was not edited after sealing, and says plainly that it
//! does **not** prove the record is the original — somebody holding the only copy can seal a
//! different one and the commitment will be perfectly valid. Every statement of that limit
//! in this repository ends the same way: *until the root is anchored somewhere the author
//! does not control.* This is the half that batches and proves; publishing is the half that
//! needs a chain and a key.
//!
//! ## Recording an anchor is an assertion, not a verification
//!
//! `--record` writes down that a root was published somewhere. **Nothing here checks that.**
//! It cannot: reaching a chain is a network act, and this command runs offline by design.
//! So an anchor entry is the operator's claim, in exactly the sense `--ground` is — and, as
//! with grounding, the honest thing is to say so rather than to let the presence of a field
//! imply a verification that did not happen.
//!
//! A reader who cares follows `reference` and checks for themselves. That is not a
//! deficiency of the design; it is the design. An anchor whose truth you take on the
//! author's word is not an anchor.

use std::path::Path;
use std::process::ExitCode;

use anyhow::{bail, Context, Result};
use scema_anchor::{verify_inclusion, Anchor, Batch, InclusionProof};

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or_default()
}

/// Every sealed root under `root`, with the file it came from.
///
/// Decisions and effects together: an effect record is as much a thing somebody might later
/// want to prove was committed at a particular time as the decision that ordered it — more
/// so, since it is the one that claims something happened in the world.
fn collect(root: &Path) -> Result<Vec<(String, String)>> {
    let mut out: Vec<(String, String)> = Vec::new();
    for (dir, field) in [("decisions", "decision"), ("effects", "effect")] {
        let d = root.join(dir);
        let Ok(entries) = std::fs::read_dir(&d) else { continue };
        for e in entries.flatten() {
            let p = e.path();
            if p.extension().and_then(|x| x.to_str()) != Some("json") {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&p) else { continue };
            let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) else { continue };
            if let Some(r) = v["commitment"]["root"].as_str() {
                out.push((r.to_string(), format!("{field} {}", p.display())));
            }
        }
    }
    // Sorted by root, so the same set of records always produces the same batch. An
    // unstable order would mean two people batching the same directory get different roots
    // and neither can check the other's proofs.
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out.dedup_by(|a, b| a.0 == b.0);
    Ok(out)
}

fn batches_dir(root: &Path) -> std::path::PathBuf {
    root.join("anchors")
}

fn load_batch(root: &Path, prefix: &str) -> Result<(std::path::PathBuf, Batch)> {
    let dir = batches_dir(root);
    let entries = std::fs::read_dir(&dir)
        .with_context(|| format!("no batches under {}", dir.display()))?;
    let mut hits = Vec::new();
    for e in entries.flatten() {
        let p = e.path();
        if p.extension().and_then(|x| x.to_str()) != Some("json") {
            continue;
        }
        let text = std::fs::read_to_string(&p)?;
        let b: Batch = serde_json::from_str(&text)?;
        if b.root.starts_with(prefix) {
            hits.push((p, b));
        }
    }
    match hits.len() {
        1 => Ok(hits.remove(0)),
        0 => bail!("no batch whose root starts with `{prefix}`"),
        n => bail!("`{prefix}` matches {n} batches — use more of the root"),
    }
}

#[allow(clippy::too_many_arguments)]
pub fn run(
    root: &Path,
    list: bool,
    proof_for: Option<&str>,
    record: Option<&str>,
    batch_ref: Option<&str>,
    check: Option<&Path>,
    check_root: Option<&str>,
) -> Result<ExitCode> {
    // ── verify a proof, offline, against a root somebody gave you ──────────────
    if let Some(path) = check {
        let Some(expected) = check_root else {
            bail!("--check needs --root <merkle-root> to check against");
        };
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("reading {}", path.display()))?;
        let proof: InclusionProof = serde_json::from_str(&text)?;
        let ok = verify_inclusion(&proof, expected);
        println!("PROOF    {}", if ok { "INCLUDED" } else { "NOT INCLUDED" });
        println!("  record  {}", proof.leaf);
        println!("  root    {expected}");
        println!("  steps   {}  ({})", proof.steps.len(), proof.algorithm);
        if ok {
            println!(
                "\n  This proves the record's commitment was in the batch under that root.\n  \
                 It does not prove the root was published anywhere — follow the anchor\n  \
                 reference and check the chain yourself."
            );
        }
        return Ok(if ok { ExitCode::SUCCESS } else { ExitCode::from(1) });
    }

    // ── list ──────────────────────────────────────────────────────────────────
    if list {
        let dir = batches_dir(root);
        let Ok(entries) = std::fs::read_dir(&dir) else {
            println!("No batches under {}.", dir.display());
            println!("  scema anchor    # build one over every sealed record");
            return Ok(ExitCode::SUCCESS);
        };
        let mut any = false;
        for e in entries.flatten() {
            let p = e.path();
            if p.extension().and_then(|x| x.to_str()) != Some("json") {
                continue;
            }
            let Ok(b) = serde_json::from_str::<Batch>(&std::fs::read_to_string(&p)?) else {
                continue;
            };
            any = true;
            println!("{}  {} record(s)", &b.root[..16.min(b.root.len())], b.leaves.len());
            if b.anchors.is_empty() {
                // Said plainly. A batch built and never published proves only what
                // `scema verify` already proved.
                println!("  NOT ANCHORED — built, never published");
            }
            for a in &b.anchors {
                println!("  anchored  {:<12} {}", a.chain, a.reference);
            }
            if !b.root_matches_leaves() {
                println!("  ROOT DOES NOT MATCH ITS LEAVES — this file has been edited");
            }
        }
        if !any {
            println!("No batches under {}.", dir.display());
        }
        return Ok(ExitCode::SUCCESS);
    }

    // ── record an anchor that happened elsewhere ──────────────────────────────
    if let Some(spec) = record {
        let Some((chain, reference)) = spec.split_once('=') else {
            bail!("--record wants <chain>=<reference>, e.g. base=0xabc…");
        };
        let Some(prefix) = batch_ref else {
            bail!("--record needs --batch <root-prefix> to say which batch was published");
        };
        let (path, mut b) = load_batch(root, prefix)?;
        if !b.root_matches_leaves() {
            bail!(
                "that batch's root does not match its leaves — it has been edited, and \
                 recording an anchor against it would attest to something else"
            );
        }
        b.anchors.push(Anchor {
            chain: chain.trim().to_string(),
            reference: reference.trim().to_string(),
            at: now(),
        });
        std::fs::write(&path, serde_json::to_string_pretty(&b)?)?;
        println!("RECORDED {} on {}", &b.root[..16.min(b.root.len())], chain.trim());
        println!("  {}", path.display());
        println!(
            "\n  Written down, not verified. Nothing here reached a chain — this is your\n  \
             assertion that the root was published, in the same sense `--ground` is.\n  \
             A reader follows the reference and checks for themselves."
        );
        return Ok(ExitCode::SUCCESS);
    }

    // ── issue a proof ─────────────────────────────────────────────────────────
    if let Some(id) = proof_for {
        let dir = batches_dir(root);
        let Ok(entries) = std::fs::read_dir(&dir) else {
            bail!("no batches under {} — run `scema anchor` first", dir.display());
        };
        // A record's own root is what a leaf is, but an operator types the short id. Look
        // it up from the records first.
        let target = resolve_root(root, id)?;
        for e in entries.flatten() {
            let p = e.path();
            if p.extension().and_then(|x| x.to_str()) != Some("json") {
                continue;
            }
            let Ok(b) = serde_json::from_str::<Batch>(&std::fs::read_to_string(&p)?) else {
                continue;
            };
            let Some(tree) = b.tree() else { continue };
            if let Some(proof) = tree.proof_for(&target) {
                println!("{}", serde_json::to_string_pretty(&proof)?);
                eprintln!("\nbatch root  {}", b.root);
                if b.anchors.is_empty() {
                    eprintln!(
                        "NOT ANCHORED — this proves membership in a batch nobody published."
                    );
                } else {
                    for a in &b.anchors {
                        eprintln!("anchored    {:<12} {}", a.chain, a.reference);
                    }
                }
                return Ok(ExitCode::SUCCESS);
            }
        }
        bail!("`{id}` is not in any batch — run `scema anchor` to include it");
    }

    // ── build a batch ─────────────────────────────────────────────────────────
    let found = collect(root)?;
    if found.is_empty() {
        println!("Nothing sealed under {} to batch.", root.display());
        println!("  scema decide \"<goal>\" --ground <signal-id>");
        return Ok(ExitCode::SUCCESS);
    }
    let leaves: Vec<String> = found.iter().map(|(r, _)| r.clone()).collect();
    let Some(batch) = Batch::build(&leaves, now()) else {
        bail!("could not build a batch — a record root was not a 32-byte hex digest");
    };

    let dir = batches_dir(root);
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{}.json", &batch.root[..16.min(batch.root.len())]));
    std::fs::write(&path, serde_json::to_string_pretty(&batch)?)?;

    println!("BATCH    {}", batch.root);
    println!("  {} record(s)   {}", batch.leaves.len(), batch.algorithm);
    println!("  {}", path.display());
    for (_, src) in found.iter().take(5) {
        println!("    {src}");
    }
    if found.len() > 5 {
        println!("    … {} more", found.len() - 5);
    }
    println!(
        "\n  NOT ANCHORED. Publishing this root is a network act with a key behind it, and\n  \
         nothing here does it — recording an anchor that was never submitted would be the\n  \
         fabrication the rest of this runtime exists to refuse.\n\n  \
         Publish the root, then:  scema anchor --record <chain>=<ref> --batch {}",
        &batch.root[..16.min(batch.root.len())]
    );
    Ok(ExitCode::SUCCESS)
}

/// Map a record id (or prefix) to its commitment root.
fn resolve_root(root: &Path, id: &str) -> Result<String> {
    for dir in ["decisions", "effects"] {
        let d = root.join(dir);
        let Ok(entries) = std::fs::read_dir(&d) else { continue };
        for e in entries.flatten() {
            let p = e.path();
            let stem = p.file_stem().and_then(|s| s.to_str()).unwrap_or_default();
            if !stem.starts_with(id) {
                continue;
            }
            let text = std::fs::read_to_string(&p)?;
            let v: serde_json::Value = serde_json::from_str(&text)?;
            if let Some(r) = v["commitment"]["root"].as_str() {
                return Ok(r.to_string());
            }
        }
    }
    // A 64-character hex string is already a root; accept it so a third party who was given
    // one but holds no records can still ask for a proof.
    if id.len() == 64 && id.chars().all(|c| c.is_ascii_hexdigit()) {
        return Ok(id.to_string());
    }
    bail!("no sealed record under {} whose id starts with `{id}`", root.display())
}
