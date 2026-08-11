//! Attest a real decision log:  cargo run -p mesh-attest --example attest_log -- <path> [tail]
use mesh_attest::{attest, parse_log, Freshness, DEFAULT_MAX_LAG_SECS};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let path = args.get(1).cloned().unwrap_or_else(|| "scematica-pool-decisions.jsonl".into());
    let tail: usize = args.get(2).and_then(|v| v.parse().ok()).unwrap_or(500);

    let contents = std::fs::read_to_string(&path).expect("read decision log");
    let all = parse_log(&contents);
    let start = all.len().saturating_sub(tail);
    let records = &all[start..];

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();

    println!("log            : {path}");
    println!("parsed         : {} of {} lines", all.len(), contents.lines().count());
    match attest(records, now, DEFAULT_MAX_LAG_SECS) {
        None => println!("nothing to attest"),
        Some(a) => {
            println!("attesting      : {} decisions", a.count);
            println!("window         : {} .. {} (unix)", a.window.0, a.window.1);
            println!("max lag        : {}s", a.max_lag_secs);
            println!("freshness      : {:?}", a.freshness);
            println!("root           : {}", a.root.to_hex());
            if a.freshness == Freshness::Retrospective {
                println!();
                println!("NOTE: these decisions are older than the {DEFAULT_MAX_LAG_SECS}s bound, so their");
                println!("outcomes are already knowable. Anchoring them is a weaker claim than a live");
                println!("attestation and must be labelled as such wherever it is published.");
            }
        }
    }
}
