//! Writes the fixture the TypeScript port is checked against, and fails if it drifted.
//!
//! The arrangement is the same one `web/lib/omni/fixtures/record.json` uses, and it is the
//! only one that actually proves anything: **the fixture carries Rust's answer**, so
//! `check:omni` asks whether the port agrees with this crate rather than whether the port
//! agrees with a snapshot of itself. A test that regenerates its own expectation passes
//! forever and detects nothing.
//!
//! Running `cargo test -p scema-nft` refreshes the files when they are missing and
//! **fails** when they exist and differ. That order matters: a contributor who changes the
//! plate gets a red test naming the file, not a silently rewritten fixture that makes the
//! browser check pass against new bytes nobody reviewed.
//!
//! Set `SCEMA_NFT_BLESS=1` to accept a deliberate change.

use std::fs;
use std::path::PathBuf;

use scema_nft::{fixtures::parity_world, render_metadata, render_svg, world_digest};

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures")
}

fn check_or_write(name: &str, actual: &str) {
    let dir = fixtures_dir();
    fs::create_dir_all(&dir).expect("create fixtures dir");
    let path = dir.join(name);

    let bless = std::env::var_os("SCEMA_NFT_BLESS").is_some();
    match fs::read_to_string(&path) {
        Ok(existing) if existing.replace("\r\n", "\n") == actual => {}
        Ok(_) if bless => fs::write(&path, actual).expect("bless fixture"),
        Ok(_) => panic!(
            "fixture {} is out of date.\n\
             The plate changed. If that was deliberate, re-run with SCEMA_NFT_BLESS=1 and \
             review the diff — `web/scripts/check-omni.mjs` compares the TypeScript port \
             against this file, so blessing it without also updating the port turns a real \
             parity failure into a green build.",
            path.display()
        ),
        Err(_) => fs::write(&path, actual).expect("write fixture"),
    }
}

#[test]
fn the_parity_fixture_is_current() {
    let w = parity_world();
    let d = world_digest(&w);
    let svg = render_svg(&w, &d);
    let meta = render_metadata(&w, &svg, &d, None);

    // The world itself, so the port renders from the same input rather than from a
    // hand-copied approximation of it.
    let world_json = serde_json::to_string_pretty(&w).expect("serialise world");
    check_or_write("parity-world.json", &format!("{world_json}\n"));
    check_or_write("parity-plate.svg", &format!("{svg}\n"));
    check_or_write(
        "parity-metadata.json",
        &format!("{}\n", serde_json::to_string_pretty(&meta).expect("serialise metadata")),
    );
    check_or_write("parity-digest.txt", &format!("{d}\n"));
}

#[test]
fn the_fixture_world_exercises_every_branch_the_plate_can_draw() {
    // A fixture that only covers the happy path proves the two implementations agree about
    // nothing in particular. Asserted rather than trusted, because it is easy to simplify
    // this world later without noticing what the simplification stopped testing.
    let w = parity_world();

    assert!(w.signals.iter().any(|s| s.measured), "no counted signal");
    assert!(w.signals.iter().any(|s| !s.measured), "no estimated signal");
    assert!(
        w.signals.iter().any(|s| s.polarity == scema_world::Polarity::Risk),
        "no risk signal"
    );
    assert!(
        w.signals.iter().any(|s| s.polarity == scema_world::Polarity::Opportunity),
        "no opportunity signal"
    );
    assert!(w.signals.iter().any(|s| s.magnitude == 0.0), "no measured-zero magnitude");
    assert!(w.signals.iter().any(|s| s.magnitude == 1.0), "no full magnitude");
    assert!(!w.blind_spots.is_empty(), "no blind spots");
    assert!(w.extent.total.is_some(), "extent must be bounded here");

    let kinds: Vec<_> = w.objects.iter().map(|o| &o.provenance).collect();
    use scema_world::Provenance::*;
    assert!(kinds.iter().any(|p| matches!(p, Live { .. })), "no live object");
    assert!(kinds.iter().any(|p| matches!(p, Stale { .. })), "no stale object");
    assert!(kinds.iter().any(|p| matches!(p, Absent)), "no absent object");
    assert!(kinds.iter().any(|p| matches!(p, Simulated)), "no simulated object");

    assert!(w.entity.label.contains('<'), "label must force XML escaping");
    assert!(!w.observer.is_ascii(), "observer must force non-ASCII encoding");
}
