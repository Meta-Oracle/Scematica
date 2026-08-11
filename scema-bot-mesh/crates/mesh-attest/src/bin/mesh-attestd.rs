//! `mesh-attestd` — the flush loop that makes the record *live*.
//!
//! ```text
//! mesh-attestd --log <decisions.jsonl> --weights 0x… [--interval 60] [--broadcast]
//! ```
//!
//! Reads the sniper's decision log on an interval, anchors anything new, and records
//! progress. Without `--broadcast` it plans and spools only — the default is safe.
//!
//! # Signing
//!
//! This never touches a private key. With `--broadcast` it shells out to `cast send
//! --account <name>`, and Foundry's keystore handles the key exactly as it did for the
//! deployments. A daemon holding a key that can rewrite the public record it exists to
//! protect would defeat its own purpose, so the key stays where the operator put it.

use std::process::Command;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use mesh_attest::{
    attest,
    daemon::{advance, load_checkpoint, next_batch, save_checkpoint, Resync},
    parse_log, plan_anchor, DEFAULT_MAX_LAG_SECS, MESH_MAINNET,
};
use mesh_core::commit::Digest;

/// Default flush cadence.
///
/// Must stay well under how fast positions resolve, or every batch lands retrospective and
/// the loop achieves nothing beyond a slower archive. The sniper's positions turn over in
/// minutes, so a minute is the right order of magnitude.
const DEFAULT_INTERVAL_SECS: u64 = 60;

/// Challenge window submitted with each anchor. Must clear the contract's 5-minute floor.
const CHALLENGE_WINDOW_SECS: u64 = 3600;

fn arg(name: &str) -> Option<String> {
    let args: Vec<String> = std::env::args().collect();
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1).cloned())
}

fn flag(name: &str) -> bool {
    std::env::args().any(|a| a == name)
}

fn parse_digest(hex: &str) -> Option<Digest> {
    let clean = hex.trim_start_matches("0x");
    if clean.len() != 64 {
        return None;
    }
    let mut out = [0u8; 32];
    for (i, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(clean.get(i * 2..i * 2 + 2)?, 16).ok()?;
    }
    Some(Digest(out))
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn main() {
    let log_path = arg("--log").unwrap_or_else(|| "scematica-pool-decisions.jsonl".into());
    let checkpoint_path = arg("--checkpoint").unwrap_or_else(|| "mesh-attest-checkpoint.json".into());
    let account = arg("--account").unwrap_or_else(|| "botchain-deployer".into());
    let rpc = arg("--rpc").unwrap_or_else(|| "https://rpc.botchain.ai".into());
    let interval = arg("--interval").and_then(|v| v.parse().ok()).unwrap_or(DEFAULT_INTERVAL_SECS);
    let broadcast = flag("--broadcast");
    let once = flag("--once");

    let weights = match arg("--weights").as_deref().and_then(parse_digest) {
        Some(w) => w,
        None => {
            eprintln!("--weights <0x…32 bytes> is required.");
            eprintln!("Compute it from the live checkpoint:");
            eprintln!("  cargo run -p mesh-attest --example weights_hash -- scematica-nn-agent.json");
            std::process::exit(2);
        }
    };

    println!("mesh-attestd");
    println!("  log        : {log_path}");
    println!("  checkpoint : {checkpoint_path}");
    println!("  weights    : {}", weights.to_hex());
    println!("  contract   : {MESH_MAINNET}");
    println!("  interval   : {interval}s");
    println!("  mode       : {}", if broadcast { "BROADCAST" } else { "plan only (add --broadcast to send)" });
    println!();

    loop {
        match flush(&log_path, &checkpoint_path, &weights, &account, &rpc, broadcast) {
            Ok(()) => {}
            Err(e) => eprintln!("[{}] flush failed: {e}", now()),
        }
        if once {
            break;
        }
        std::thread::sleep(Duration::from_secs(interval));
    }
}

fn flush(
    log_path: &str,
    checkpoint_path: &str,
    weights: &Digest,
    account: &str,
    rpc: &str,
    broadcast: bool,
) -> Result<(), String> {
    let contents = std::fs::read_to_string(log_path).map_err(|e| format!("read {log_path}: {e}"))?;
    let all = parse_log(&contents);
    let mut checkpoint = load_checkpoint(checkpoint_path);

    let batch = next_batch(&all, &checkpoint);
    match batch.resync {
        Resync::None => {}
        // Reported, never silent: a rotated log may mean decisions were never anchored,
        // and a gap in the record is the accusation this system exists to refute.
        Resync::LogShrank => eprintln!(
            "[{}] WARNING: log is shorter than recorded progress — rotated or truncated. \
             Re-covering from the start; earlier decisions may never have been anchored.",
            now()
        ),
        Resync::DigestMismatch => eprintln!(
            "[{}] WARNING: log content changed under a previously anchored offset. \
             Re-covering from the start.",
            now()
        ),
    }

    if batch.records.is_empty() {
        // A daemon that prints nothing while idle is indistinguishable from a hung one.
        // Say so, and say how far it has got, so silence never has to be interpreted.
        println!(
            "[{}] idle — {} decisions anchored, nothing new (waiting on the sniper)",
            now(),
            checkpoint.processed
        );
        return Ok(());
    }

    let attestation = match attest(batch.records, now(), DEFAULT_MAX_LAG_SECS) {
        Some(a) => a,
        None => return Ok(()),
    };
    let plan = plan_anchor(&attestation, weights, CHALLENGE_WINDOW_SECS, account);

    println!(
        "[{}] {} decisions | {:?} | lag {}s | root {}",
        now(),
        attestation.count,
        attestation.freshness,
        attestation.max_lag_secs,
        attestation.root.to_hex()
    );
    if let Some(w) = &plan.warning {
        println!("       {w}");
    }

    if !broadcast {
        println!("       (plan only) {}", plan.command.replace('\n', " "));
        return Ok(());
    }

    let output = Command::new("cast")
        .args(["send", MESH_MAINNET, &plan.calldata, "--rpc-url", rpc, "--account", account, "--legacy"])
        .output()
        .map_err(|e| format!("cast not runnable: {e}"))?;

    if !output.status.success() {
        // Progress is NOT advanced here. Advancing on a failed send would skip these
        // decisions permanently — the gap this daemon is built to prevent.
        return Err(format!(
            "cast send failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }

    // Surface the transaction so the anchor can be checked on the explorer rather than
    // taken on trust from this process's own say-so.
    let stdout = String::from_utf8_lossy(&output.stdout);
    if let Some(line) = stdout.lines().find(|l| l.contains("transactionHash")) {
        println!("       {}", line.trim());
    }

    let consumed = batch.start_index + batch.records.len();
    advance(&mut checkpoint, &all, consumed, &attestation.root);
    save_checkpoint(checkpoint_path, &checkpoint).map_err(|e| format!("save checkpoint: {e}"))?;
    println!("       anchored, progress now {consumed}");
    Ok(())
}
