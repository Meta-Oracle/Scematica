use anyhow::Result;
use solana_sdk::signature::{read_keypair_file, Keypair, Signer};
use std::path::Path;

/// Load a keypair from a file path or base58-encoded private key string
pub fn load_keypair(source: &str) -> Result<Keypair> {
    // 1. Expand ~ to the home directory
    // Resolve home directory: prefer USERPROFILE (Windows), fall back to HOME (Unix)
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_else(|_| ".".to_string());

    // Only expand ~ if the path actually starts with it, to avoid corrupting UNC paths (\\server\...)
    let expanded = if source.starts_with('~') {
        let without_tilde = &source[1..];
        // Strip a leading separator after ~ if present (e.g. ~/foo or ~\foo)
        let rest = without_tilde.trim_start_matches('/').trim_start_matches('\\');
        if rest.is_empty() {
            home.clone()
        } else {
            format!("{}{}{}", home, std::path::MAIN_SEPARATOR, rest)
        }
    } else {
        source.to_string()
    };

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
