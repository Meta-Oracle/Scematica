use solana_sdk::pubkey::Pubkey;
use spl_associated_token_account::get_associated_token_address;
use crate::types::known_tokens;

/// Derive the associated token account address for a wallet + mint
pub fn get_ata(wallet: &Pubkey, mint: &Pubkey) -> Pubkey {
    get_associated_token_address(wallet, mint)
}

/// Resolve a token symbol to its mint pubkey
pub fn resolve_mint(symbol: &str) -> Option<Pubkey> {
    match symbol.to_uppercase().as_str() {
        "WSOL" | "SOL" => Some(known_tokens::WSOL_MINT),
        "USDC" => Some(known_tokens::USDC_MINT),
        "USDT" => Some(known_tokens::USDT_MINT),
        "RAY" => Some(known_tokens::RAY_MINT),
        "BONK" => Some(known_tokens::BONK_MINT),
        _ => None,
    }
}

/// Resolve a token symbol to its decimals
pub fn resolve_decimals(symbol: &str) -> Option<u8> {
    match symbol.to_uppercase().as_str() {
        "WSOL" | "SOL" => Some(9),
        "USDC" => Some(6),
        "USDT" => Some(6),
        "RAY" => Some(6),
        "BONK" => Some(5),
        _ => None,
    }
}

/// Convert UI amount (e.g. 1.5 SOL) to raw lamports/tokens
pub fn ui_to_raw(ui_amount: f64, decimals: u8) -> u64 {
    (ui_amount * 10f64.powi(decimals as i32)) as u64
}

/// Convert raw amount to UI amount
pub fn raw_to_ui(raw: u64, decimals: u8) -> f64 {
    raw as f64 / 10f64.powi(decimals as i32)
}

/// Apply slippage to get minimum output amount
pub fn apply_slippage(amount: u64, slippage_pct: f64) -> u64 {
    let factor = 1.0 - (slippage_pct / 100.0);
    (amount as f64 * factor) as u64
}
