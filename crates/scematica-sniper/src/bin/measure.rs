//! `measure` — read the bot's own decision log and report what it can actually answer.
//!
//! ```text
//! measure                              # the whole log, as one window
//! measure --split 2026-08-05           # before vs from that day, side by side
//! measure --since 2026-08-01           # only recent history
//! measure --decisions path --trades p  # explicit files
//! ```
//!
//! Reads only. It opens two append-only logs the sniper already writes and never touches
//! anything else, so it is safe to run against a live bot — the same guarantee
//! `mesh-dashboard` makes.
//!
//! ## Why `--split` is the point
//!
//! An aggregate over the whole log is a claim about *history*, and reads as a claim about
//! the *bot*. Those come apart the moment anything is fixed. Run without a split on the
//! log this was built against and the momentum gate looks like the single largest cause of
//! rejection ever recorded; split it on the day that veto was removed and it is 28.3% of
//! one window and 0.4% of the next. Both numbers are correct. Only the second is about the
//! bot as it stands.

use std::path::PathBuf;
use std::process::ExitCode;

use scematica_sniper::measure::{
    audit, coverage, funnel, read_jsonl, realised, split_at, CoherenceSample, CoverageReport,
    Decision, Realised, SignalAudit, StageCount, Trade, AUDIT_FIELDS,
};

const VERSION: &str = env!("CARGO_PKG_VERSION");

fn print_help() {
    println!("measure {VERSION} — what the decision log can answer\n");
    println!("USAGE:\n  measure [OPTIONS]\n");
    println!("OPTIONS:");
    println!("  --split <YYYY-MM-DD>   Report two windows either side of this day");
    println!("  --since <YYYY-MM-DD>   Ignore records before this day");
    println!("  --decisions <PATH>     Default: scematica-pool-decisions.jsonl");
    println!("  --trades <PATH>        Default: scematica-trades.jsonl");
    println!("  --coherence <PATH>     Default: scematica-coherence.jsonl");
    println!("  --top <N>              Funnel rows per window (default 8)");
    println!("  -h, --help             Print this help");
    println!("  -V, --version          Print version\n");
    println!("Reads only. Safe against a running bot.");
}

struct Args {
    split: Option<String>,
    since: Option<String>,
    decisions: PathBuf,
    trades: PathBuf,
    coherence: PathBuf,
    top: usize,
}

fn parse_args() -> Result<Option<Args>, String> {
    let mut a = Args {
        split: None,
        since: None,
        decisions: PathBuf::from("scematica-pool-decisions.jsonl"),
        trades: PathBuf::from("scematica-trades.jsonl"),
        coherence: PathBuf::from("scematica-coherence.jsonl"),
        top: 8,
    };
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let mut i = 0;
    while i < argv.len() {
        let need = |i: usize, what: &str| -> Result<String, String> {
            argv.get(i + 1)
                .cloned()
                .ok_or_else(|| format!("{what} needs a value"))
        };
        match argv[i].as_str() {
            "-h" | "--help" => {
                print_help();
                return Ok(None);
            }
            "-V" | "--version" => {
                println!("measure {VERSION}");
                return Ok(None);
            }
            "--split" => {
                a.split = Some(need(i, "--split")?);
                i += 1;
            }
            "--since" => {
                a.since = Some(need(i, "--since")?);
                i += 1;
            }
            "--decisions" => {
                a.decisions = PathBuf::from(need(i, "--decisions")?);
                i += 1;
            }
            "--trades" => {
                a.trades = PathBuf::from(need(i, "--trades")?);
                i += 1;
            }
            "--coherence" => {
                a.coherence = PathBuf::from(need(i, "--coherence")?);
                i += 1;
            }
            "--top" => {
                a.top = need(i, "--top")?
                    .parse()
                    .map_err(|_| "--top needs a number".to_string())?;
                i += 1;
            }
            other => return Err(format!("unrecognized argument `{other}` (try --help)")),
        }
        i += 1;
    }
    Ok(Some(a))
}

fn main() -> ExitCode {
    let args = match parse_args() {
        Ok(Some(a)) => a,
        Ok(None) => return ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("measure: {e}");
            return ExitCode::from(2);
        }
    };

    let (mut decisions, dstats) = read_jsonl::<Decision>(&args.decisions);
    let (mut trades, tstats) = read_jsonl::<Trade>(&args.trades);
    let (mut samples, _) = read_jsonl::<CoherenceSample>(&args.coherence);

    if decisions.is_empty() {
        eprintln!(
            "measure: no decisions in {}.\n\
             The sniper writes this file as it evaluates pools; an empty one means it has \
             not run here, not that it rejected nothing.",
            args.decisions.display()
        );
        return ExitCode::from(1);
    }

    if let Some(since) = &args.since {
        decisions.retain(|d| d.day() >= since.as_str());
        trades.retain(|t| t.day() >= since.as_str());
        samples.retain(|c| c.day() >= since.as_str());
    }

    println!("MEASURE  {VERSION}");
    println!(
        "  decisions  {} parsed{}  {}",
        dstats.parsed,
        skipped(dstats.skipped),
        args.decisions.display()
    );
    println!(
        "  trades     {} parsed{}  {}",
        tstats.parsed,
        skipped(tstats.skipped),
        args.trades.display()
    );
    if let (Some(a), Some(b)) = (decisions.first(), decisions.last()) {
        println!("  span       {} .. {}", a.day(), b.day());
    }

    match &args.split {
        None => {
            let refs: Vec<&Decision> = decisions.iter().collect();
            let trefs: Vec<&Trade> = trades.iter().collect();
            let crefs: Vec<&CoherenceSample> = samples.iter().collect();
            window("WHOLE LOG", &refs, &trefs, &crefs, args.top);
            println!(
                "\n  This is one window over all of history. If anything was changed in this\n  \
                 span, the numbers above average the bot before and after it. Use --split."
            );
        }
        Some(day) => {
            let (before, after) = split_at(&decisions, day, |d| d.day());
            let (tb, ta) = split_at(&trades, day, |t| t.day());
            let (cb, ca) = split_at(&samples, day, |c| c.day());
            window(&format!("BEFORE {day}"), &before, &tb, &cb, args.top);
            window(&format!("FROM   {day}"), &after, &ta, &ca, args.top);
            movement(&before, &after);
        }
    }

    ExitCode::SUCCESS
}

/// A percentage, with negative zero normalised away.
///
/// `-0.0%` in a delta column is not something that happened — it is a float artefact, and
/// this repository already normalises it in `scema_verify::canonical` for the same reason:
/// a reader who sees a minus sign concludes a quantity went down.
fn pct(fraction: f64) -> f64 {
    let v = fraction * 100.0;
    if v == 0.0 {
        0.0
    } else {
        v
    }
}

fn skipped(n: usize) -> String {
    if n == 0 {
        String::new()
    } else {
        format!(", {n} unparseable")
    }
}

fn window(
    title: &str,
    rows: &[&Decision],
    trades: &[&Trade],
    samples: &[&CoherenceSample],
    top: usize,
) {
    println!("\n{}", "─".repeat(76));
    println!("{title}   {} record(s)", rows.len());
    println!("{}", "─".repeat(76));

    if rows.is_empty() {
        println!("  nothing in this window");
        return;
    }

    let owned: Vec<Decision> = rows.iter().map(|r| (*r).clone()).collect();

    println!("\nFUNNEL");
    for s in funnel(&owned).into_iter().take(top) {
        println!(
            "  {:>6} ({:>5.1}%)  {:<9} {}",
            s.count,
            s.share * 100.0,
            s.decision,
            s.stage
        );
    }

    println!("\nSIGNALS   did the field carry information in this window?");
    for a in audit(&owned, AUDIT_FIELDS) {
        println!("  {}", signal_line(&a));
    }
    println!(
        "  {}",
        "— never varied: present in every record and non-zero in none. That is a fact about"
    );
    println!(
        "  {}",
        "  the field, not about the market: measured-and-zero and never-populated are"
    );
    println!(
        "  {}",
        "  different, and this log cannot tell them apart. Go and read the producer."
    );

    let t: Vec<Trade> = trades.iter().map(|t| (*t).clone()).collect();
    let r = realised(&t);
    println!("\nREALISED");
    println!("  {}", realised_line(&r));

    let cs: Vec<CoherenceSample> = samples.iter().map(|c| (*c).clone()).collect();
    println!("\nCOVERAGE");
    print_coverage(&coverage(&cs));
}

/// What the coherence samples say, or an em dash when nobody sampled.
///
/// Two silences are told apart deliberately: no samples at all, versus samples the breaker
/// declined to judge because it had too few observations. Neither is a zero, and a reader
/// fixes them differently — the first by turning the breaker on, the second by waiting.
fn print_coverage(c: &CoverageReport) {
    if !c.measured() {
        let why = if c.samples == 0 {
            "nothing sampled in this window"
        } else {
            "every sample was below the breaker's minimum, so it declined to judge"
        };
        println!("  {:>7}   {why}", "—");
        println!("            Unmeasured, not zero: PnL here cannot be attributed to the");
        println!("            coverage it was earned under. The sniper appends a sample every");
        println!("            30s while the coherence breaker is enabled.");
        return;
    }
    println!(
        "  {:>6.0}%   mean resolution over {} decisive sample(s) of {}",
        c.mean_resolution.unwrap_or(0.0) * 100.0,
        c.decisive,
        c.samples
    );
    if let Some(worst) = c.worst_resolution {
        println!("            worst {:.0}%", worst * 100.0);
    }
    println!(
        "            Sampled on a timer, so this is resolution *around* these decisions,"
    );
    println!("            not *for* them. Per-pool attribution would need a per-evaluation");
    println!("            context the buy path does not carry.");
}

fn signal_line(a: &SignalAudit) -> String {
    let head = format!("{:<26}", a.field);
    match a.nonzero_share() {
        // The field was never written by any build in this window. Absent, not zero.
        None => format!("{head}{:>9}   not recorded by this build", "—"),
        Some(share) => {
            let range = match (a.min, a.max) {
                (Some(lo), Some(hi)) => format!("  range {lo:.4}..{hi:.4}"),
                _ => String::new(),
            };
            let flag = if a.never_varied() { "   <-- NEVER VARIED" } else { "" };
            format!(
                "{head}{:>5} / {:<5} nonzero ({:>5.1}%){range}{flag}",
                a.nonzero,
                a.present,
                share * 100.0
            )
        }
    }
}

fn realised_line(r: &Realised) -> String {
    match (r.mean(), r.win_rate()) {
        (Some(mean), Some(wr)) => format!(
            "{} trade(s)   total {:+.4} SOL   mean {:+.4}   win rate {:.0}%  ({}W / {}L)",
            r.trades,
            r.total_pnl,
            mean,
            wr * 100.0,
            r.wins,
            r.losses
        ),
        // An average over nothing is undefined, not zero — printing 0.00 here invites a
        // comparison against a window that actually traded.
        _ => format!("{:>7}   nothing resolved in this window", "—"),
    }
}

/// What moved between the two windows.
///
/// Only stages present in either window, and only as counts and shares — no attribution.
/// A stage that collapsed between two windows is evidence that *something* changed, not
/// evidence that a particular change caused it, and this report will not pretend otherwise.
fn movement(before: &[&Decision], after: &[&Decision]) {
    if before.is_empty() || after.is_empty() {
        return;
    }
    println!("\n{}", "─".repeat(76));
    println!("MOVEMENT   share of each window, by stage");
    println!("{}", "─".repeat(76));

    let ob: Vec<Decision> = before.iter().map(|r| (*r).clone()).collect();
    let oa: Vec<Decision> = after.iter().map(|r| (*r).clone()).collect();
    let fb = funnel(&ob);
    let fa = funnel(&oa);

    let share = |f: &[StageCount], stage: &str| -> f64 {
        f.iter().filter(|s| s.stage == stage).map(|s| s.share).sum()
    };

    let mut stages: Vec<String> = fb.iter().chain(fa.iter()).map(|s| s.stage.clone()).collect();
    stages.sort();
    stages.dedup();

    let mut rows: Vec<(String, f64, f64)> = stages
        .into_iter()
        .map(|s| {
            let a = share(&fb, &s);
            let b = share(&fa, &s);
            (s, a, b)
        })
        .collect();
    // Largest absolute movement first — that is what a reader is looking for.
    rows.sort_by(|x, y| (y.2 - y.1).abs().total_cmp(&(x.2 - x.1).abs()));

    println!("  {:<20} {:>9} {:>9} {:>9}", "stage", "before", "after", "delta");
    for (stage, a, b) in rows.into_iter().take(10) {
        println!(
            "  {:<20} {:>8.1}% {:>8.1}% {:>+8.1}%",
            stage,
            pct(a),
            pct(b),
            pct(b - a)
        );
    }
    println!(
        "\n  A stage that collapsed between windows shows that something changed. It does not\n  \
         show what changed it — that is a question for the git log, not this report."
    );
}
