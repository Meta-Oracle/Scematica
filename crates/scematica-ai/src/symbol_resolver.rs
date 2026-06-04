use anyhow::{anyhow, Result};
use scematica_core::types::known_tokens;
use solana_sdk::pubkey::Pubkey;
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::OnceLock;
use tokio::sync::OnceCell;

static TOKEN_CACHE: OnceLock<HashMap<String, Pubkey>> = OnceLock::new();

/// Jupiter verified token list, fetched once and cached for the process lifetime.
/// Powers both `symbol → mint` (chat/AI swap commands) and `mint → symbol`
/// (trade labeling) resolution for listed tokens. Fetch failures fail open to
/// an empty map so resolution degrades to the built-in known tokens rather than
/// erroring the whole AI layer.
static JUPITER_TOKENS: OnceCell<JupiterMaps> = OnceCell::const_new();

// Verified tag keeps the list small (~hundreds of tokens) and excludes scam
// look-alikes. Brand-new pump.fun mints are not listed here — that is expected;
// reverse lookup simply returns None for them.
const JUPITER_TOKEN_LIST_URL: &str = "https://tokens.jup.ag/tokens?tags=verified";

#[derive(Default)]
struct JupiterMaps {
    symbol_to_mint: HashMap<String, Pubkey>,
    mint_to_symbol: HashMap<Pubkey, String>,
}

#[derive(serde::Deserialize)]
struct JupToken {
    address: String,
    symbol: String,
}

async fn jupiter_tokens() -> &'static JupiterMaps {
    JUPITER_TOKENS
        .get_or_init(|| async {
            match fetch_jupiter_tokens().await {
                Ok(maps) => {
                    tracing::info!(
                        "Jupiter token list loaded: {} symbols",
                        maps.symbol_to_mint.len()
                    );
                    maps
                }
                Err(e) => {
                    tracing::warn!(
                        "Jupiter token list fetch failed: {} — symbol resolution limited to known tokens",
                        e
                    );
                    JupiterMaps::default()
                }
            }
        })
        .await
}

async fn fetch_jupiter_tokens() -> Result<JupiterMaps> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()?;
    let tokens: Vec<JupToken> = client
        .get(JUPITER_TOKEN_LIST_URL)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    let mut maps = JupiterMaps::default();
    for t in tokens {
        if let Ok(mint) = Pubkey::from_str(&t.address) {
            // First listing wins on collision (verified list is deduped, but be safe).
            maps.symbol_to_mint
                .entry(t.symbol.to_uppercase())
                .or_insert(mint);
            maps.mint_to_symbol.entry(mint).or_insert(t.symbol);
        }
    }
    Ok(maps)
}

/// Resolve a user-supplied symbol or base58 mint string to a `Pubkey`.
///
/// Resolution order: built-in known tokens → literal base58 pubkey → Jupiter
/// verified token list.
pub async fn resolve_symbol(symbol: &str) -> Result<Pubkey> {
    let symbol_upper = symbol.to_uppercase();

    // 1. Check known tokens (no network).
    if let Some(mint) = get_known_tokens().get(&symbol_upper) {
        return Ok(*mint);
    }

    // 2. Parse as base58 Pubkey.
    if let Ok(pubkey) = Pubkey::from_str(symbol) {
        return Ok(pubkey);
    }

    // 3. Jupiter verified token list (fetched + cached on first use).
    if let Some(mint) = jupiter_tokens().await.symbol_to_mint.get(&symbol_upper) {
        return Ok(*mint);
    }

    Err(anyhow!("Unknown symbol: {}", symbol))
}

/// Reverse lookup: resolve a mint to its listed ticker symbol.
///
/// Checks built-in known tokens first, then the Jupiter verified list. Returns
/// `None` for unlisted tokens (e.g. freshly-launched pump.fun mints), which the
/// caller should treat as "no symbol available" rather than an error.
pub async fn resolve_mint_symbol(mint: &Pubkey) -> Option<String> {
    for (sym, known) in get_known_tokens() {
        if known == mint {
            return Some(sym.clone());
        }
    }
    jupiter_tokens().await.mint_to_symbol.get(mint).cloned()
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
