use std::collections::HashMap;
use std::sync::OnceLock;
use anyhow::{anyhow, Result};
use solana_sdk::pubkey::Pubkey;
use std::str::FromStr;
use scematica_core::types::known_tokens;

static TOKEN_CACHE: OnceLock<HashMap<String, Pubkey>> = OnceLock::new();

pub fn resolve_symbol(symbol: &str) -> Result<Pubkey> {
    let symbol_upper = symbol.to_uppercase();

    // 1. Check known tokens
    let known = get_known_tokens();
    if let Some(mint) = known.get(&symbol_upper) {
        return Ok(*mint);
    }

    // 2. Parse as base58 Pubkey
    if let Ok(pubkey) = Pubkey::from_str(symbol) {
        // Validation logic: checking if it's a valid mint could involve RPC,
        // but for now we return the parsed pubkey.
        return Ok(pubkey);
    }

    // 3. TODO: Jupiter token list lookup
    Err(anyhow!("Unknown symbol: {}", symbol))
}

fn get_known_tokens() -> &'static HashMap<String, Pubkey> {
    TOKEN_CACHE.get_or_init(|| {
        let mut map = HashMap::new();
        map.insert("SOL".to_string(), known_tokens::WSOL_MINT);
        map.insert("WSOL".to_string(), known_tokens::WSOL_MINT);
        map.insert("USDC".to_string(), known_tokens::USDC_MINT);
        map.insert("USDT".to_string(), known_tokens::USDT_MINT);
        map.insert("RAY".to_string(), known_tokens::RAY_MINT);
        map.insert("BONK".to_string(), known_tokens::BONK_MINT);
        map
    })
}
