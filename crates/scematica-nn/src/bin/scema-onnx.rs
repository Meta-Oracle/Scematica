//! `scema-onnx` — export the live-trained Deep Q\* policy to a portable `.onnx` model.
//!
//! ```text
//! scema-onnx                                    # checkpoint → scematica-dqn.onnx
//! scema-onnx --checkpoint path.json --out m.onnx
//! scema-onnx --reference ref.json               # also dump validation vectors
//! scema-onnx --target                           # export the lagged target net instead
//! ```
//!
//! The weights come from the agent's own checkpoint, so what lands in the file is the
//! policy that actually traded — not a fresh initialisation with the right shape.
//!
//! `--reference` writes the inputs and the Q-values this crate computes for them.
//! `scripts/validate_onnx.py` runs the exported graph under onnxruntime and asserts the
//! two agree. That pairing is the point: an exported model that loads cleanly but
//! computes something subtly different is worse than no export at all, because it fails
//! silently and confidently.
//!
//! Reference inputs come from two places. States rebuilt from `scematica-trades.jsonl`
//! keep the validation anchored to the distribution the network actually sees, and
//! deterministic pseudo-random states cover the corners of the input domain that live
//! trading happens not to have visited. Equivalence has to hold on both.

use std::collections::HashMap;
use std::path::Path;

use scematica_nn::agent::DQNAgent;
use scematica_nn::network::QNetwork;
use scematica_nn::onnx::{self, ExportOptions};
use scematica_nn::state::{TradeState, STATE_DIM, STATE_FEATURES};
use scematica_nn::action::{TradeAction, ACTION_DIM};

const DEFAULT_CHECKPOINT: &str = "scematica-nn-agent.json";
const DEFAULT_TRADES: &str = "scematica-trades.jsonl";
const DEFAULT_OUT: &str = "scematica-dqn.onnx";

struct Args {
    checkpoint: String,
    out: String,
    reference: Option<String>,
    trades: String,
    samples: usize,
    use_target: bool,
}

impl Args {
    fn parse() -> Result<Self, String> {
        let mut args = Args {
            checkpoint: DEFAULT_CHECKPOINT.to_string(),
            out: DEFAULT_OUT.to_string(),
            reference: None,
            trades: DEFAULT_TRADES.to_string(),
            samples: 64,
            use_target: false,
        };
        let mut argv = std::env::args().skip(1);
        while let Some(flag) = argv.next() {
            match flag.as_str() {
                "--checkpoint" | "-c" => {
                    args.checkpoint = argv.next().ok_or("--checkpoint needs a path")?
                }
                "--out" | "-o" => args.out = argv.next().ok_or("--out needs a path")?,
                "--reference" | "-r" => {
                    args.reference = Some(argv.next().ok_or("--reference needs a path")?)
                }
                "--trades" => args.trades = argv.next().ok_or("--trades needs a path")?,
                "--samples" => {
                    args.samples = argv
                        .next()
                        .ok_or("--samples needs a number")?
                        .parse()
                        .map_err(|_| "--samples must be a number")?
                }
                "--target" => args.use_target = true,
                "--help" | "-h" => {
                    print_help();
                    std::process::exit(0);
                }
                other => return Err(format!("unknown argument: {other}")),
            }
        }
        Ok(args)
    }
}

fn print_help() {
    println!(
        "scema-onnx — export the trained Deep Q* policy network to ONNX\n\n\
         USAGE:\n  scema-onnx [OPTIONS]\n\n\
         OPTIONS:\n\
         \x20 -c, --checkpoint <PATH>  agent checkpoint (default: {DEFAULT_CHECKPOINT})\n\
         \x20 -o, --out <PATH>         output model (default: {DEFAULT_OUT})\n\
         \x20 -r, --reference <PATH>   also write validation vectors as JSON\n\
         \x20     --trades <PATH>      trade log for realistic states (default: {DEFAULT_TRADES})\n\
         \x20     --samples <N>        reference vectors to emit (default: 64)\n\
         \x20     --target             export the lagged target net instead of the online one\n\
         \x20 -h, --help               this text"
    );
}

/// A tiny deterministic LCG. Reproducible reference vectors matter more here than
/// statistical quality — the same command must produce the same validation set on any
/// machine, and pulling in a seeded RNG for uniform noise would be overkill.
struct Lcg(u64);

impl Lcg {
    fn next_unit(&mut self) -> f64 {
        // Numerical Recipes constants.
        self.0 = self.0.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        ((self.0 >> 11) as f64) / ((1u64 << 53) as f64)
    }
}

/// Rebuild plausible states from the live trade log.
fn states_from_trades(path: &str, limit: usize) -> Vec<Vec<f64>> {
    let Ok(raw) = std::fs::read_to_string(path) else {
        return Vec::new();
    };

    let mut states = Vec::new();
    let mut daily_pnl = 0.0f64;
    let mut wins = 0i32;
    let mut losses = 0i32;

    for line in raw.lines().take(limit * 4) {
        if states.len() >= limit {
            break;
        }
        let Ok(record) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        let pnl = record.get("pnl").and_then(|v| v.as_f64()).unwrap_or(0.0);
        let amount = record.get("amount").and_then(|v| v.as_f64()).unwrap_or(0.0);
        let kind = record.get("kind").and_then(|v| v.as_str()).unwrap_or("");

        // Only closed positions carry a realised PnL worth turning into a state.
        if !kind.eq_ignore_ascii_case("SELL") {
            continue;
        }
        daily_pnl += pnl;
        if pnl > 0.0 {
            wins += 1;
            losses = 0;
        } else if pnl < 0.0 {
            losses += 1;
            wins = 0;
        }

        let pnl_pct = if amount > 0.0 { pnl / amount } else { 0.0 };
        let state = TradeState::from_trade_fields(
            pnl_pct,
            (states.len() as f64) * 13.0 % 3_600.0,
            daily_pnl,
            wins,
            losses,
            1.0 + daily_pnl.max(-0.9),
            (states.len() % 5) as i32,
        );
        states.push(state.to_vec());
    }
    states
}

/// Deterministic states spanning the whole normalised input domain.
fn synthetic_states(count: usize, seed: u64) -> Vec<Vec<f64>> {
    let mut rng = Lcg(seed);
    (0..count)
        .map(|index| {
            // Include the corners: an all-zeros and an all-ones state are exactly where
            // a ReLU boundary or a broadcast bug is most likely to show up.
            match index {
                0 => vec![0.0; STATE_DIM],
                1 => vec![1.0; STATE_DIM],
                2 => vec![0.5; STATE_DIM],
                _ => (0..STATE_DIM).map(|_| rng.next_unit()).collect(),
            }
        })
        .collect()
}

fn main() {
    let args = match Args::parse() {
        Ok(args) => args,
        Err(message) => {
            eprintln!("error: {message}\n");
            print_help();
            std::process::exit(2);
        }
    };

    if !Path::new(&args.checkpoint).exists() {
        eprintln!(
            "error: no checkpoint at {}\n\n\
             The point of this export is the *trained* weights, so it refuses to run\n\
             rather than emit a freshly initialised network that looks identical from\n\
             the outside. Train the agent first, or pass --checkpoint.",
            args.checkpoint
        );
        std::process::exit(2);
    }

    let agent = match DQNAgent::load(&args.checkpoint) {
        Ok(agent) => agent,
        Err(err) => {
            eprintln!("error: could not load {}: {err}", args.checkpoint);
            std::process::exit(1);
        }
    };

    let stats = agent.stats();
    let net: &QNetwork = if args.use_target {
        agent.target_net()
    } else {
        agent.online_net()
    };

    let is_dueling = net.value_head.is_some() && net.advantage_head.is_some();
    let params: usize = net
        .layers
        .iter()
        .map(|l| l.weights.len() * l.weights[0].len() + l.biases.len())
        .sum::<usize>()
        + net
            .value_head
            .as_ref()
            .map(|h| h.weights.len() * h.weights[0].len() + h.biases.len())
            .unwrap_or(0)
        + net
            .advantage_head
            .as_ref()
            .map(|h| h.weights.len() * h.weights[0].len() + h.biases.len())
            .unwrap_or(0);

    println!("checkpoint    {}", args.checkpoint);
    println!(
        "network       {} ({})",
        net.layer_sizes
            .iter()
            .map(|s| s.to_string())
            .collect::<Vec<_>>()
            .join(" → "),
        if is_dueling { "dueling" } else { "standard" }
    );
    println!("parameters    {params}");
    println!(
        "training      {} steps · epsilon {:.4} · {} target copies",
        stats.train_steps, stats.epsilon, stats.target_updates
    );
    println!(
        "exporting     {} net",
        if args.use_target { "target" } else { "online" }
    );

    let export = if args.use_target {
        let mut options = ExportOptions {
            doc_string: format!(
                "Scematica Deep Q* TARGET network (lagged copy). Trained {} steps.",
                stats.train_steps
            ),
            ..Default::default()
        };
        options.metadata = onnx::describe(net);
        options.metadata.push(("network_role".into(), "target".into()));
        options
            .metadata
            .push(("train_steps".into(), stats.train_steps.to_string()));
        onnx::export_qnetwork(net, &args.out, &options)
    } else {
        onnx::export_agent(&agent, &args.out)
    };

    if let Err(err) = export {
        eprintln!("error: could not write {}: {err}", args.out);
        std::process::exit(1);
    }

    let size = std::fs::metadata(&args.out).map(|m| m.len()).unwrap_or(0);
    println!("wrote         {} ({size} bytes)", args.out);

    if let Some(reference_path) = args.reference {
        let mut states = states_from_trades(&args.trades, args.samples / 2);
        let live = states.len();
        states.extend(synthetic_states(args.samples.saturating_sub(live), 0x5CE_A71C));

        let q_values: Vec<Vec<f64>> = states.iter().map(|s| net.forward(s)).collect();

        let mut payload = HashMap::new();
        payload.insert(
            "note".to_string(),
            serde_json::json!(
                "Q-values computed by scematica-nn's own forward pass. The exported \
                 ONNX graph must reproduce these elementwise."
            ),
        );
        payload.insert("model".to_string(), serde_json::json!(args.out));
        payload.insert("state_dim".to_string(), serde_json::json!(STATE_DIM));
        payload.insert("action_dim".to_string(), serde_json::json!(ACTION_DIM));
        payload.insert(
            "action_labels".to_string(),
            serde_json::json!((0..ACTION_DIM)
                .map(|i| TradeAction::from_index(i).label())
                .collect::<Vec<_>>()),
        );
        payload.insert(
            "state_features".to_string(),
            serde_json::json!(STATE_FEATURES),
        );
        payload.insert("live_states".to_string(), serde_json::json!(live));
        payload.insert(
            "synthetic_states".to_string(),
            serde_json::json!(states.len() - live),
        );
        payload.insert("inputs".to_string(), serde_json::json!(states));
        payload.insert("q_values".to_string(), serde_json::json!(q_values));

        match serde_json::to_string_pretty(&payload) {
            Ok(json) => {
                if let Err(err) = std::fs::write(&reference_path, json) {
                    eprintln!("error: could not write {reference_path}: {err}");
                    std::process::exit(1);
                }
                println!(
                    "reference     {reference_path} ({} vectors: {live} from live trades, {} synthetic)",
                    states.len(),
                    states.len() - live
                );
            }
            Err(err) => {
                eprintln!("error: could not serialise reference vectors: {err}");
                std::process::exit(1);
            }
        }
    }

    println!("\nValidate with:  python scripts/validate_onnx.py");
}
