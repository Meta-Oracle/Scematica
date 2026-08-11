//! Shared core for the BOT Chain port of Scematica.
//!
//! The Solana bot is untouched and stays where the measured edge is. This tree is the
//! EVM side, kept in its own cargo workspace because the two dependency trees cannot
//! share a lockfile — see `scema-botchain/Cargo.toml` for the specific conflict.
//!
//! Nothing here may depend on a crate that pulls `solana-sdk`.

pub mod chain;
pub mod rpc;

pub use chain::{Endpoint, EndpointKind, Network, MAINNET, TESTNET};
pub use rpc::Client;

/// Resolve a network and an optional operator endpoint from the environment.
///
/// `BOTCHAIN_NETWORK` = `mainnet` | `testnet` (default `mainnet`).
/// `BOTCHAIN_RPC_URL` = a node URL tried before the built-in list.
pub fn client_from_env() -> anyhow::Result<Client> {
    let name = std::env::var("BOTCHAIN_NETWORK").unwrap_or_else(|_| "mainnet".into());
    let network = chain::by_name(&name)
        .ok_or_else(|| anyhow::anyhow!("unknown BOTCHAIN_NETWORK '{name}' (mainnet|testnet)"))?;

    let mut client = Client::new(network)?;
    if let Ok(url) = std::env::var("BOTCHAIN_RPC_URL") {
        let url = url.trim();
        if !url.is_empty() {
            // Leaked for the process lifetime on purpose: `Endpoint` holds `&'static str`
            // so the built-in table can be a `const`. One deliberate leak of one URL at
            // startup is a fair trade for that.
            client = client.with_endpoint(Box::leak(url.to_string().into_boxed_str()));
        }
    }
    Ok(client)
}
