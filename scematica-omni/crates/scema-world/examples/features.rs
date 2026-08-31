//! Print `WorldFeatures` for every captured producer fixture.
//!
//! The claim `WorldFeatures` makes is that one vector shape fits a repository, a running bot,
//! a DOM and a set of oracle feeds. That is cheap to assert and worth demonstrating, so this
//! runs it over the real captures in `scema-tools/fixtures/` and prints the coverage each
//! world supports — which is the number a consumer must read beside the vector.
use scema_world::{WorldFeatures, WorldState};

fn main() {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../scema-tools/fixtures");
    let mut files: Vec<_> = std::fs::read_dir(dir)
        .expect("fixtures")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .collect();
    files.sort();

    for path in files {
        let text = std::fs::read_to_string(&path).expect("read");
        let w: WorldState = match serde_json::from_str(&text) {
            Ok(w) => w,
            Err(e) => {
                println!("{}: not a world ({e})", path.file_name().unwrap().to_string_lossy());
                continue;
            }
        };
        let f = WorldFeatures::of(&w);
        println!("\n{}", path.file_name().unwrap().to_string_lossy());
        println!("  domain {:<14} coverage {}", w.domain.as_str(), f.coverage().label());
        for (name, t) in WorldFeatures::names().iter().zip(f.terms()) {
            let shown = if t.measured { format!("{:.3}", t.value) } else { "—".into() };
            println!("    {name:<20} {shown:>6}");
        }
    }
}
