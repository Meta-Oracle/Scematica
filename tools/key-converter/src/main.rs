use anyhow::{anyhow, Result};
use bip39::{Language, Mnemonic};
use solana_sdk::signature::{Keypair, Signer, SeedDerivable};
use std::fs::File;
use std::io::{self, Write, BufRead};

fn main() -> Result<()> {
    println!("--- Scematica Keypair Converter ---");
    println!("Enter your 12 or 24-word seed phrase:");
    
    let stdin = io::stdin();
    let phrase = stdin.lock().lines().next()
        .ok_or_else(|| anyhow!("Failed to read input"))??;

    // 2. Derive mnemonic
    let mnemonic = Mnemonic::parse_in(Language::English, phrase)
        .map_err(|e| anyhow!("Invalid seed phrase: {}", e))?;
    let seed_bytes = mnemonic.to_seed("");
    
    // 3. Derive keypair using Solana SDK
    let keypair = Keypair::from_seed(&seed_bytes[..32])
        .map_err(|e| anyhow!("Failed to derive keypair: {}", e))?;
    
    // 5. Save to file
    let path = std::env::current_dir()?.join("id.json");
    let mut file = File::create(&path)?;
    // Convert [u8; 64] to Vec<u8> so it can be serialized as JSON array
    let bytes = keypair.to_bytes().to_vec();
    file.write_all(serde_json::to_string(&bytes)?.as_bytes())?;

    println!("\n✅ Successfully created file at: {}", path.display());
    println!("Public Key: {}", keypair.pubkey());
    println!("Move this file to: /home/deadsg/.config/solana/id.json");

    Ok(())
    }
