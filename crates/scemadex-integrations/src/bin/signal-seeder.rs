//! signal-seeder — distill the sniper's decision history into the pool-score
//! feed the ScemaDEX relay serves.
//!
//! Reads `scematica-pool-decisions.jsonl` (and, if present,
//! `scematica-pool-radar.json`) from a directory and writes
//! `scematica-pool-scores.json` there — the per-mint 0–100 pool-quality map that
//! `FileSignalSource::pool_score` (and thus the relay's `/signal/pool_score/:mint`
//! endpoint) returns. Run it once to give a freshly-deployed relay real day-one
//! data instead of an empty book, or on a schedule to keep the feed fresh.
//!
//! Usage:
//!   signal-seeder [--dir <DIR>] [--out <FILE>]
//!
//!   --dir <DIR>   Directory holding the bot artifacts and where the output is
//!                 written (default: current directory).
//!   --out <FILE>  Output filename within <DIR> (default: scematica-pool-scores.json).

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use scemadex_integrations::pool_scores::{seed_pool_scores, POOL_SCORES_FILE};

const DECISIONS: &str = "scematica-pool-decisions.jsonl";
const RADAR: &str = "scematica-pool-radar.json";

fn main() -> ExitCode {
    let mut dir = PathBuf::from(".");
    let mut out = POOL_SCORES_FILE.to_string();

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--dir" => match args.next() {
                Some(v) => dir = PathBuf::from(v),
                None => return fail("--dir requires a value"),
            },
            "--out" => match args.next() {
                Some(v) => out = v,
                None => return fail("--out requires a value"),
            },
            "-h" | "--help" => {
                eprintln!(
                    "signal-seeder — build {POOL_SCORES_FILE} from {DECISIONS} (+ {RADAR})\n\
                     Usage: signal-seeder [--dir <DIR>] [--out <FILE>]"
                );
                return ExitCode::SUCCESS;
            }
            other => return fail(&format!("unknown argument: {other}")),
        }
    }

    let decisions_path = dir.join(DECISIONS);
    let decisions = match std::fs::read_to_string(&decisions_path) {
        Ok(s) => s,
        Err(e) => return fail(&format!("read {}: {e}", decisions_path.display())),
    };

    // Radar is optional — fold it in if it's there.
    let radar = std::fs::read_to_string(dir.join(RADAR)).ok();

    let file = seed_pool_scores(&decisions, radar.as_deref());
    let json = match serde_json::to_string_pretty(&file) {
        Ok(j) => j,
        Err(e) => return fail(&format!("serialize: {e}")),
    };

    let out_path = dir.join(&out);
    if let Err(e) = atomic_write(&out_path, &json) {
        return fail(&format!("write {}: {e}", out_path.display()));
    }

    println!(
        "seeded {} pool score(s) -> {}",
        file.len(),
        out_path.display()
    );
    ExitCode::SUCCESS
}

/// Write via a temp file + rename so a reader never sees a half-written feed
/// (the same atomic convention the sniper uses for its artifacts).
fn atomic_write(path: &Path, contents: &str) -> std::io::Result<()> {
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, contents)?;
    std::fs::rename(&tmp, path)
}

fn fail(msg: &str) -> ExitCode {
    eprintln!("signal-seeder: {msg}");
    ExitCode::FAILURE
}
