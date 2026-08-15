//! Print the mesh for a working directory, as text or JSON.
//!
//!   cargo run -p scematica-mesh --example dump             # the repo root
//!   cargo run -p scematica-mesh --example dump -- --json
//!   cargo run -p scematica-mesh --example dump -- /path/to/botdir
//!
//! Exists so the collector can be checked against a real machine without standing up the
//! API and the web app — and so the answer to "why did nothing trade" is one command.

use scematica_mesh::{Collector, Provenance};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let json = args.iter().any(|a| a == "--json");
    let root = args
        .iter()
        .find(|a| !a.starts_with("--"))
        .cloned()
        .unwrap_or_else(|| ".".to_string());

    let mesh = Collector::new(&root).collect();

    if json {
        println!("{}", serde_json::to_string_pretty(&mesh).unwrap());
        return;
    }

    println!("SCEMATICA MESH   {}", mesh.generated_at);
    println!("root             {root}");
    println!(
        "visibility       {:.0}%  ({} live, {} stale, {} unseen of {})",
        mesh.summary.visibility * 100.0,
        mesh.summary.nodes_live,
        mesh.summary.nodes_stale,
        mesh.summary.nodes_absent,
        mesh.summary.nodes_total
    );
    println!("blocking         {}", mesh.summary.blocking);
    println!("diagnosis        {}", mesh.summary.diagnosis);
    println!();

    let c = &mesh.cognition;
    println!("AGENTIC GATE (§32)   Ψ = C · K · (1 − R)");
    println!("  C  confidence  {:.3}", c.confidence);
    println!("  K  coherence   {:.3}   ({} live subsystems, {:.0}% dissent)", c.coherence.value, c.coherence.subsystems, c.coherence.disagreement * 100.0);
    println!("  R  risk        {:.3}", c.risk.value);
    println!("  Ψ              {:.3}   {:?}", c.psi, c.verdict);
    println!("  Ω  (§33)       {}", c.omega.map(|o| format!("{o:.3}")).unwrap_or_else(|| "unavailable — none of its five subsystems exist".into()));
    println!("  measured       {:.0}% of terms", c.measured_fraction * 100.0);
    println!("  reading        {}", c.reading);
    println!();
    println!("  terms:");
    for t in c.confidence_terms.iter().chain(c.risk.components.iter()).chain(c.omega_terms.iter()) {
        println!(
            "    [{}] {:8} {:6.3}  {:28} {}",
            if t.measured { "measured" } else { "  ——    " },
            t.symbol,
            t.value,
            t.name,
            t.note
        );
    }
    println!();

    for n in &mesh.nodes {
        let mark = match n.provenance {
            Provenance::Live { .. } => "live",
            Provenance::Stale { .. } => "STALE",
            Provenance::Absent => "unseen",
            Provenance::Simulated => "sim",
        };
        println!("[{:6}] {:22} {:?}", mark, n.label, n.verdict);
        if let Some(r) = &n.reason {
            println!("           {r}");
        }
    }

    println!();
    for e in &mesh.edges {
        if e.is_blocking() {
            println!("BLOCKING  {} -> {}", e.from, e.to);
        }
    }

    let problems = mesh.validate();
    if !problems.is_empty() {
        println!("\nSTRUCTURAL PROBLEMS:");
        for p in problems {
            println!("  {p}");
        }
    }
}
