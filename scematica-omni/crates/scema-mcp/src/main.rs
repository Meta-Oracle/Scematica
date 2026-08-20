//! `scema-mcp` — the omni loop as MCP tools.
//!
//! ```jsonc
//! // claude_desktop_config.json / .mcp.json
//! {
//!   "mcpServers": {
//!     "scema-omni": {
//!       "command": "scema-mcp",
//!       "args": ["--allow", "/path/to/a/project"]
//!     }
//!   }
//! }
//! ```
//!
//! Reads newline-delimited JSON-RPC on stdin and writes it on stdout. **stdout is the
//! transport** — every diagnostic goes to stderr, and this binary installs no logger that
//! could accidentally write to it.

mod mcp;
mod tools;

use std::io::{BufRead, Write};
use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Parser;
use scema_agent::Agent;
use scema_tools::Workspace;

use crate::mcp::McpServer;
use crate::tools::Tools;

#[derive(Parser)]
#[command(
    name = "scema-mcp",
    version,
    about = "Scematica Omni over the Model Context Protocol (stdio)"
)]
struct Cli {
    /// State directory: decision records and memory.
    #[arg(long, default_value = ".scema")]
    root: PathBuf,

    /// A directory the caller may observe. Repeatable. Defaults to the working directory.
    ///
    /// Point this at a project, not a home directory. A cooperative model asked to audit a
    /// codebase will reason its way to `~/.ssh` because that is genuinely relevant to an
    /// audit, and confinement is what stops the observer going there.
    #[arg(long = "allow")]
    allow: Vec<PathBuf>,

    /// Advertise and permit `omni_decide`, which seals records and appends to memory.
    ///
    /// Off by default. Without it the tool is not listed at all, because a tool that is
    /// advertised and always fails teaches a model to retry it.
    #[arg(long)]
    allow_decide: bool,

    /// Deep Q* checkpoint, for trading worlds.
    #[arg(long)]
    dqstar: Option<String>,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    let roots: Vec<PathBuf> = if cli.allow.is_empty() {
        vec![std::env::current_dir()?]
    } else {
        cli.allow.clone()
    };
    let workspace = Workspace::new(&roots).context("resolving --allow roots")?;

    // stderr, always. See the module note.
    eprintln!("scema-mcp {} — {}", env!("CARGO_PKG_VERSION"), scema_agent::RUNTIME);
    for r in workspace.root_labels() {
        eprintln!("  allow        {r}");
    }
    eprintln!("  state        {}", cli.root.display());
    eprintln!(
        "  omni_decide  {}",
        if cli.allow_decide { "advertised" } else { "not advertised (--allow-decide)" }
    );

    let server = McpServer::new(Tools {
        agent: Agent::new(cli.root.clone(), cli.dqstar.clone()),
        workspace,
        root: cli.root,
        allow_decide: cli.allow_decide,
    });

    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    for line in stdin.lock().lines() {
        let line = line.context("reading stdin")?;
        if let Some(reply) = server.handle_line(&line) {
            stdout.write_all(reply.as_bytes())?;
            stdout.write_all(b"\n")?;
            // Flushed per message. A buffered reply is a client that hangs waiting for a
            // response that has already been computed.
            stdout.flush()?;
        }
    }
    Ok(())
}
