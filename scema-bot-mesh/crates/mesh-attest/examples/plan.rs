//! cargo run -p mesh-attest --example plan -- <log> [tail]
use mesh_attest::{attest, parse_log, plan_anchor, DEFAULT_MAX_LAG_SECS};
use mesh_core::commit::Digest;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let path = a.get(1).cloned().unwrap_or_else(|| "scematica-pool-decisions.jsonl".into());
    let tail: usize = a.get(2).and_then(|v| v.parse().ok()).unwrap_or(300);

    let text = std::fs::read_to_string(&path).expect("read log");
    let all = parse_log(&text);
    let records = &all[all.len().saturating_sub(tail)..];
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();

    let att = attest(records, now, DEFAULT_MAX_LAG_SECS).expect("non-empty");
    // Placeholder policy identity until the DQ* weights are exported through mesh-core.
    let weights = Digest([0x11; 32]);
    let plan = plan_anchor(&att, &weights, 3600, "botchain-deployer");

    println!("decisions : {}", att.count);
    println!("root      : {}", att.root.to_hex());
    println!("calldata  : {}...{} ({} bytes)",
        &plan.calldata[..18], &plan.calldata[plan.calldata.len()-8..], (plan.calldata.len()-2)/2);
    if let Some(w) = &plan.warning { println!("\nWARNING: {w}"); }
    println!("\n{}", plan.command);
}
