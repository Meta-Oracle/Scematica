use anyhow::{Context, Result};
use solana_sdk::signature::{read_keypair_file, Keypair, Signer};
use std::path::Path;

/// Load a keypair from a file path or base58-encoded private key string
pub fn load_keypair(source: &str) -> Result<Keypair> {
    // Try as a file path first
    let expanded = shellexpand::tilde(source).to_string();
    let path = Path::new(&expanded);
    if path.exists() {
        return read_keypair_file(path)
            .map_err(|e| anyhow::anyhow!("Failed to read keypair file {}: {}", source, e));
    }

    // Try as base58-encoded private key
    let bytes = bs58::decode(source)
        .into_vec()
        .context("Failed to decode base58 private key")?;
    Keypair::from_bytes(&bytes)
        .map_err(|e| anyhow::anyhow!("Invalid keypair bytes: {}", e))
}

/// Wallet wrapper with convenience methods
pub struct Wallet {
    pub keypair: Keypair,
}

impl Wallet {
    pub fn new(keypair: Keypair) -> Self {
        Self { keypair }
    }

    pub fn from_source(source: &str) -> Result<Self> {
        Ok(Self::new(load_keypair(source)?))
    }

    pub fn pubkey(&self) -> solana_sdk::pubkey::Pubkey {
        self.keypair.pubkey()
    }
}

impl std::fmt::Debug for Wallet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Wallet({})", self.pubkey())
    }
}
