use anyhow::Result;
use solana_sdk::signature::{read_keypair_file, Keypair, Signer};
use std::path::Path;

/// Load a keypair from a file path or base58-encoded private key string
pub fn load_keypair(source: &str) -> Result<Keypair> {
    // 1. Always attempt to read as a file if it looks like a path or exists
    // Resolve home directory: prefer USERPROFILE (Windows), fall back to HOME (Unix)
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_else(|_| ".".to_string());
    let expanded = source.replace("~", &home);
    // Normalize any double separators introduced by the substitution (e.g. "~\\.config" → "C:\Users\x\\.config")
    // but preserve leading double backslashes for UNC paths on Windows.
    let expanded = if expanded.starts_with("\\\\") {
        "\\\\".to_string() + &expanded[2..].replace("\\\\", "\\")
    } else {
        expanded.replace("\\\\", "\\")
    };
    let expanded = expanded.replace("//", "/");
    let path = Path::new(&expanded);
    
    if path.exists() {
        return read_keypair_file(path)
            .map_err(|e| anyhow::anyhow!("Failed to read existing keypair file {}: {}", expanded, e));
    }

    // 2. Explicitly handle paths that don't exist but are clearly paths
    if source.starts_with('/') || source.starts_with('~') || source.starts_with("./") || source.contains(std::path::MAIN_SEPARATOR) {
        return Err(anyhow::anyhow!("Keypair file path specified but not found: {}", expanded));
    }

    // 3. Try as base58-encoded private key
    let bytes = bs58::decode(source)
        .into_vec()
        .map_err(|e| anyhow::anyhow!("Failed to decode base58 key (input: '{}'): {}", source, e))?;
    
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
