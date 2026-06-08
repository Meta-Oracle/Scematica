//! `scematica` — the unified launcher shipped with the `scematica-suite` crate.
//!
//! It dispatches subcommands to the component binaries, resolving each one next
//! to this launcher's own executable first (so when the component crates are
//! installed alongside the suite via `cargo install`, they're found
//! automatically) and then on `PATH`. All arguments after the subcommand are
//! forwarded verbatim.
//!
//!   scematica dashboard --demo
//!   scematica sniper
//!   scematica ddqn
//!   scematica scemadex

use std::path::PathBuf;
use std::process::Command;

const VERSION: &str = env!("CARGO_PKG_VERSION");

/// (subcommand, binary stem, crate that provides it, one-line description).
const COMMANDS: &[(&str, &str, &str, &str)] = &[
    ("dashboard", "dashboard", "scematica-dashboard", "Terminal monitoring dashboard (try: dashboard --demo)"),
    ("sniper", "sniper", "scematica-sniper", "New-pool sniper engine (needs keypair + RPC)"),
    ("backtest", "backtest", "scematica-sniper", "Replay pool history through the filter + TP/SL sim"),
    ("protocol", "protocol", "scematica-protocol", "x402 HTTP 402 payment facilitator server"),
    ("ddqn", "scema-ddqn", "scematica-nn", "Deep Q* agent live training viewer"),
    ("scemadex", "scemadex", "scemadex-sdk", "ScemaDEX agentic-liquidity live viewer"),
];

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let sub = args.get(1).map(|s| s.as_str());

    match sub {
        None | Some("-h") | Some("--help") | Some("help") => {
            print_help();
        }
        Some("-V") | Some("--version") | Some("version") => {
            println!("scematica {VERSION}");
        }
        Some(name) => match COMMANDS.iter().find(|(cmd, ..)| *cmd == name) {
            Some((_, stem, krate, _)) => {
                let code = dispatch(stem, krate, &args[2..]);
                std::process::exit(code);
            }
            None => {
                eprintln!("scematica: unknown command `{name}`\n");
                print_help();
                std::process::exit(2);
            }
        },
    }
}

/// Resolve and run a component binary, forwarding `rest` and propagating its
/// exit code. Returns the code to exit with.
fn dispatch(stem: &str, krate: &str, rest: &[String]) -> i32 {
    let bin = format!("{stem}{}", std::env::consts::EXE_SUFFIX);
    let Some(path) = resolve(&bin) else {
        eprintln!(
            "scematica: `{stem}` is not installed.\n\
             Install it with:\n    cargo install {krate}\n\
             (the launcher finds component binaries next to itself or on PATH)."
        );
        return 127;
    };
    match Command::new(&path).args(rest).status() {
        Ok(status) => status.code().unwrap_or(1),
        Err(e) => {
            eprintln!("scematica: failed to run {}: {e}", path.display());
            1
        }
    }
}

/// Find `bin` next to the current executable (the `cargo install` bin dir),
/// then on `PATH`.
fn resolve(bin: &str) -> Option<PathBuf> {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(sibling) = exe.parent().map(|d| d.join(bin)) {
            if sibling.is_file() {
                return Some(sibling);
            }
        }
    }
    let path_var = std::env::var_os("PATH")?;
    std::env::split_paths(&path_var)
        .map(|dir| dir.join(bin))
        .find(|p| p.is_file())
}

fn print_help() {
    println!("scematica {VERSION} — unified launcher for the Scematica stack\n");
    println!("USAGE:\n  scematica <command> [args...]\n");
    println!("COMMANDS:");
    for (cmd, _, _, desc) in COMMANDS {
        println!("  {cmd:<10} {desc}");
    }
    println!("  help       Show this help");
    println!("  version    Show the suite version\n");
    println!("Each command runs the corresponding installed binary. Install the");
    println!("whole stack alongside the launcher with, e.g.:");
    println!("  cargo install scematica-suite scematica-dashboard scematica-sniper \\");
    println!("                scematica-protocol scematica-nn scemadex-sdk");
}
