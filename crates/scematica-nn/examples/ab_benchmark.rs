//! A/B benchmark: does the distributional + world-model + CVaR machinery
//! actually beat the scalar agent? This trains each variant on the adversarial
//! pool simulator and evaluates greedily per archetype, so you can see — in
//! numbers — whether the new RL machinery reduces losses on rugs/honeypots and
//! keeps the upside on legit/pump pools.
//!
//! Run:
//!   cargo run --release -p scematica-nn --example ab_benchmark            # defaults
//!   cargo run --release -p scematica-nn --example ab_benchmark -- 300 100 # train eval
//!
//! Notes: the simulator uses a live RNG, so absolute numbers vary run-to-run;
//! the *ranking* across variants is the signal. More training episodes = more
//! signal (and more time — QR-DQN backprop over 51 quantiles is not cheap).

use scematica_nn::{AdversarialPoolSim, DQNAgent, PoolArchetype, ScarProfile, TradeAction};

const ARCHETYPES: &[(PoolArchetype, &str)] = &[
    (PoolArchetype::Legit, "legit"),
    (PoolArchetype::PumpDump, "pump-dump"),
    (PoolArchetype::Rug, "rug"),
    (PoolArchetype::Honeypot, "honeypot"),
    (PoolArchetype::SlowBleed, "slow-bleed"),
];

/// Train an agent on profile-sampled episodes.
fn train(agent: &mut DQNAgent, episodes: usize) {
    let mut sim = AdversarialPoolSim::new(ScarProfile::default());
    agent.pretrain_on_simulator(&mut sim, episodes);
}

/// Greedy evaluation: mean episode reward per archetype and overall.
#[allow(unused_assignments)]
fn evaluate(agent: &DQNAgent, episodes_per_archetype: usize) -> (Vec<f64>, f64) {
    let mut sim = AdversarialPoolSim::new(ScarProfile::default());
    let mut per_arch = Vec::with_capacity(ARCHETYPES.len());
    let mut grand_total = 0.0;
    let mut grand_n = 0.0;
    for (arch, _) in ARCHETYPES {
        let mut total = 0.0;
        for _ in 0..episodes_per_archetype {
            let mut state = sim.reset_with_archetype(*arch);
            loop {
                let (action, _) = agent.greedy_action(&state);
                let step = sim.step(action);
                total += step.reward;
                state = step.state;
                if step.done {
                    break;
                }
            }
        }
        let avg = total / episodes_per_archetype as f64;
        per_arch.push(avg);
        grand_total += total;
        grand_n += episodes_per_archetype as f64;
    }
    (per_arch, grand_total / grand_n)
}

/// A never-buy baseline (0 reward everywhere) — the "do nothing" floor. Any
/// variant below this is actively destroying capital.
fn hold_floor(episodes_per_archetype: usize) -> (Vec<f64>, f64) {
    let mut sim = AdversarialPoolSim::new(ScarProfile::default());
    let mut per_arch = Vec::new();
    let mut gt = 0.0;
    let mut gn = 0.0;
    for (arch, _) in ARCHETYPES {
        let mut total = 0.0;
        for _ in 0..episodes_per_archetype {
            sim.reset_with_archetype(*arch);
            loop {
                let step = sim.step(TradeAction::Hold); // Hold ignores state
                total += step.reward;
                if step.done {
                    break;
                }
            }
        }
        per_arch.push(total / episodes_per_archetype as f64);
        gt += total;
        gn += episodes_per_archetype as f64;
    }
    (per_arch, gt / gn)
}

fn print_row(name: &str, per_arch: &[f64], overall: f64) {
    print!("{name:<28}");
    for v in per_arch {
        print!("{v:>11.3}");
    }
    println!("{overall:>11.3}");
}

/// Build a fresh agent for a named variant.
fn build_variant(name: &str) -> DQNAgent {
    match name {
        "scalar Double-DQN" => DQNAgent::new(),
        "distributional (mean)" => DQNAgent::new_distributional(),
        "distributional + worldmodel" => {
            let mut a = DQNAgent::new_distributional();
            a.enable_world_model();
            a
        }
        "distributional + CVaR0.50" => {
            let mut a = DQNAgent::new_distributional();
            a.set_risk_alpha(0.50);
            a
        }
        "distributional + CVaR0.25" => {
            let mut a = DQNAgent::new_distributional();
            a.set_risk_alpha(0.25);
            a
        }
        _ => DQNAgent::new(),
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let train_eps: usize = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(150);
    let eval_eps: usize = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(40);
    let repeats: usize = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(3).max(1);

    println!(
        "\nA/B benchmark — adversarial pool simulator\n\
         train episodes/variant: {train_eps} · eval episodes/archetype: {eval_eps} · \
         seeds averaged: {repeats}\n\
         (higher = better; negative = losing capital; single-seed RL is noisy — averaging helps)\n"
    );

    // Header.
    print!("{:<28}", "variant");
    for (_, label) in ARCHETYPES {
        print!("{label:>11}");
    }
    println!("{:>11}", "OVERALL");
    println!("{}", "-".repeat(28 + 11 * (ARCHETYPES.len() + 1)));

    // Floor (deterministic-ish; a single pass over eval episodes).
    let (floor, floor_all) = hold_floor(eval_eps);
    print_row("never-buy (floor)", &floor, floor_all);

    let variants = [
        "scalar Double-DQN",
        "distributional (mean)",
        "distributional + worldmodel",
        "distributional + CVaR0.50",
        "distributional + CVaR0.25",
    ];

    for name in variants {
        // Average over `repeats` independently-seeded training runs to cut the
        // large single-seed variance of RL.
        let mut acc = vec![0.0; ARCHETYPES.len()];
        let mut acc_all = 0.0;
        for _ in 0..repeats {
            let mut agent = build_variant(name);
            train(&mut agent, train_eps);
            let (per, all) = evaluate(&agent, eval_eps);
            for (a, v) in acc.iter_mut().zip(&per) {
                *a += v;
            }
            acc_all += all;
        }
        let per: Vec<f64> = acc.iter().map(|v| v / repeats as f64).collect();
        print_row(name, &per, acc_all / repeats as f64);
    }

    println!(
        "\nReading it: watch `rug`/`honeypot` (loss avoidance) vs `legit`/`pump-dump`\n\
         (upside capture). A capital-preserving policy (CVaR) can beat a mean-based\n\
         one on OVERALL simply by not buying rugs; the goal is a variant that clears\n\
         the never-buy floor AND stays positive on the tail columns. Scale up\n\
         train-episodes for a firmer verdict: `--example ab_benchmark -- 500 60 3`.\n"
    );
}
