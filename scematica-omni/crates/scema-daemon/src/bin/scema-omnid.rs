//! `scema-omnid` — the local agent daemon.
//!
//! ```console
//! $ scema-omnid --allow . --allow ../other-project
//! scema-omnid  scema-omni/0.1.0
//!   listening   http://127.0.0.1:7842   (loopback only, not configurable)
//!   state       .scema
//!   workspace   C:\src\project
//!   decide      OFF  (--allow-decide to enable sealing records over HTTP)
//!
//!   token       .scema\omnid.token
//!               3f9c…  paste this into the extension
//! ```

use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use clap::Parser;
use scema_agent::Agent;
use scema_daemon::{auth, http, routes, State, DEFAULT_PORT};
use scema_tools::Workspace;

#[derive(Parser)]
#[command(
    name = "scema-omnid",
    version,
    about = "Scematica Omni daemon — the cognitive loop over loopback HTTP"
)]
struct Cli {
    /// State directory: decision records, memory, and the pairing token.
    #[arg(long, default_value = ".scema")]
    root: PathBuf,

    /// Port. The interface is always 127.0.0.1 and is deliberately not configurable — see
    /// the note on `scema_daemon::http::loopback`.
    #[arg(long, default_value_t = DEFAULT_PORT)]
    port: u16,

    /// A directory clients may observe. Repeatable. Defaults to the working directory.
    #[arg(long = "allow")]
    allow: Vec<PathBuf>,

    /// Permit `POST /decide` to seal records and append memory.
    ///
    /// Off by default. Reading is one thing; letting a page or a model write into the
    /// operator's decision history without being told it may is another.
    #[arg(long)]
    allow_decide: bool,

    /// Deep Q* checkpoint, for trading worlds.
    #[arg(long)]
    dqstar: Option<String>,

    /// Print the token and exit. For pairing scripts.
    #[arg(long)]
    print_token: bool,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    let token = auth::load_or_create(&cli.root)
        .context("preparing the daemon token")?;
    if cli.print_token {
        println!("{token}");
        return Ok(());
    }

    let roots: Vec<PathBuf> = if cli.allow.is_empty() {
        vec![std::env::current_dir()?]
    } else {
        cli.allow.clone()
    };
    let workspace = Workspace::new(&roots).context("resolving --allow roots")?;

    let agent = Arc::new(Agent::new(cli.root.clone(), cli.dqstar.clone()));
    let addr = http::loopback(cli.port);
    let listener = TcpListener::bind(addr)
        .with_context(|| format!("binding {addr} — is another scema-omnid already running?"))?;

    println!("scema-omnid  {}", scema_agent::RUNTIME);
    println!("  listening   http://{addr}   (loopback only, not configurable)");
    println!("  state       {}", cli.root.display());
    for r in workspace.root_labels() {
        println!("  workspace   {r}");
    }
    println!(
        "  decide      {}",
        if cli.allow_decide {
            "ON   (POST /decide seals records)".to_string()
        } else {
            "OFF  (--allow-decide to enable sealing records over HTTP)".to_string()
        }
    );
    println!();
    println!("  token       {}", auth::token_path(&cli.root).display());
    // A prefix, not the whole secret: enough to confirm the right daemon in a log or a
    // screenshot, useless to anyone reading over a shoulder. The file has the rest.
    println!("              {}…  paste this into the extension", &token[..8]);
    println!();
    println!("  No CORS headers are sent, so a web page cannot read a reply. The extension");
    println!("  fetches from its service worker under host_permissions and is unaffected.");

    let state = State {
        agent,
        workspace,
        token,
        port: cli.port,
        root: cli.root,
        allow_decide: cli.allow_decide,
    };
    let state = Arc::new(state);

    http::serve(listener, move |req| routes::handle(&state, req))?;
    Ok(())
}
