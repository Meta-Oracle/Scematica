//! `scema-vault` — serve sealed world trees to token holders.
//!
//! See the crate note for why this is not a flag on `scema-omnid`.

use std::net::{SocketAddr, TcpListener};
use std::path::PathBuf;

use anyhow::Result;
use clap::Parser;
use scema_entitlement::{Holder, Ownership, OwnershipOracle, TokenRef};
use scema_vault::{load_entitlements, Vault};

#[derive(Parser, Debug)]
#[command(name = "scema-vault", version, about = "Serve sealed world trees to token holders")]
struct Cli {
    /// Directory of records, one `<commitment>.json` per world.
    #[arg(long, default_value = ".scema/decisions")]
    records: PathBuf,

    /// JSON array of `{ chain, contract, token_id, world_commitment }`.
    ///
    /// Written by the operator. This process never mints and never guesses which token
    /// commits to which world — the mapping is an input, so it can be reviewed.
    #[arg(long)]
    entitlements: PathBuf,

    /// Address to bind.
    ///
    /// Unlike `scema-omnid` this is configurable, because a distribution service is
    /// *supposed* to be reachable. It still defaults to loopback so that starting it by
    /// accident exposes nothing, and TLS is a reverse proxy's job.
    #[arg(long, default_value = "127.0.0.1:7843")]
    bind: SocketAddr,

    /// Trust every request. **Development only.**
    ///
    /// Prints a warning on every start that cannot be silenced, because the failure mode is
    /// leaving it on — and a service that quietly grants everything looks exactly like one
    /// that is working.
    #[arg(long)]
    insecure_grant_all: bool,
}

/// The oracle used with `--insecure-grant-all`.
struct GrantAll;
impl OwnershipOracle for GrantAll {
    fn holds(&self, _t: &TokenRef, _h: &Holder) -> Ownership {
        Ownership::Held
    }
}

/// The default when no chain is wired up.
///
/// Returns `Unknown`, never `NotHeld`. This process genuinely cannot tell whether anybody
/// holds anything, and saying "you do not own this" would be a claim it has no basis for —
/// the holder would go and buy a token they may already have. `Unknown` fails closed and
/// reports accurately, which are different things and both required.
struct NoChain;
impl OwnershipOracle for NoChain {
    fn holds(&self, _t: &TokenRef, _h: &Holder) -> Ownership {
        Ownership::Unknown {
            why: "no ownership oracle is configured — this vault cannot read any chain".into(),
        }
    }
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    let entitlements = load_entitlements(&cli.entitlements)?;
    let oracle: Box<dyn OwnershipOracle + Send + Sync> = if cli.insecure_grant_all {
        eprintln!("!! --insecure-grant-all: every request is served to anyone who asks.");
        eprintln!("!! No ownership is checked. Do not run this where it can be reached.");
        Box::new(GrantAll)
    } else {
        Box::new(NoChain)
    };

    let vault = Vault::new(&cli.records, oracle, entitlements);
    let listener = TcpListener::bind(cli.bind)?;

    eprintln!("scema-vault on http://{}", cli.bind);
    eprintln!("  records      {}", cli.records.display());
    eprintln!("  entitlements {}", vault.entitlement_count());
    if !cli.bind.ip().is_loopback() {
        eprintln!("  bound off loopback — put a TLS-terminating proxy in front of this");
    }
    if !cli.insecure_grant_all {
        eprintln!("  no ownership oracle: every request answers 503 undetermined, not 403");
    }
    eprintln!();
    eprintln!("A record served here verifies offline afterwards — `scema verify --file`, or");
    eprintln!("the /omni page. This gates distribution, never truth.");

    scema_daemon::http::serve(listener, move |req| vault.handle(req))?;
    Ok(())
}
