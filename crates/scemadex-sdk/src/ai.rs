//! Natural-language front-end for ScemaDEX (feature `ai`).
//!
//! Turns plain English ("swap 1.5 SOL to USDC, optimize for price") into a typed
//! [`Intent`], and narrates a solved/settled trade for humans or agent logs.
//! Parsing uses an OpenAI-compatible chat-completions endpoint; narration falls
//! back to a deterministic, offline string when no LLM is configured.

use serde::Deserialize;

use crate::bond::{Bond, BondOutcome};
use crate::error::{Result, ScemaDexError};
use crate::intent::{Constraints, Intent, Objective, Side};
use crate::policy::Solution;
use crate::primitives::{Address, Amount};

/// Known `(symbol, mint, decimals)` triples for offline symbol resolution.
const KNOWN_TOKENS: &[(&str, &str, u8)] = &[
    ("SOL", "So11111111111111111111111111111111111111112", 9),
    ("WSOL", "So11111111111111111111111111111111111111112", 9),
    ("USDC", "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v", 6),
    ("USDT", "Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB", 6),
    ("BONK", "DezXAZ8z7PnrnRJjz3wXBoRgixCa6xjnB7YaB1pPB263", 5),
    ("RAY", "4k3Dyjzvzp8eMZWUXbBCjEvwSkkk59S5iCNLY3QrkX6R", 6),
    ("JUP", "JUPyiwrYJFskUPiHa7hkeR8VUtAeFoSYbKedZNsDvCN", 6),
];

const SYSTEM_PROMPT: &str = "You convert a trading request into JSON with exactly these keys: \
input (token symbol or mint), output (token symbol or mint), amount (number, in input units), \
objective (one of price|speed|stealth), side (one of buy|sell). Reply with ONLY the JSON object.";

/// Resolve a symbol or raw base58 mint to `(address, decimals)`.
fn resolve_token(sym: &str) -> Result<(Address, u8)> {
    let upper = sym.to_uppercase();
    if let Some((_, mint, dec)) = KNOWN_TOKENS.iter().find(|(s, _, _)| *s == upper) {
        return Ok((Address::new(*mint)?, *dec));
    }
    // Unknown symbol but a valid mint: assume 9 decimals (caller can override).
    let addr = Address::new(sym)
        .map_err(|_| ScemaDexError::Other(format!("unknown token symbol: {sym}")))?;
    Ok((addr, 9))
}

#[derive(Deserialize)]
struct IntentJson {
    input: String,
    output: String,
    amount: f64,
    #[serde(default)]
    objective: String,
    #[serde(default)]
    side: String,
}

/// Parse a strict JSON object (as the LLM is asked to emit) into an [`Intent`].
/// Pure and offline — the unit of testable logic behind [`LlmClient::parse_intent`].
pub fn parse_intent_json(json: &str) -> Result<Intent> {
    let j: IntentJson = serde_json::from_str(json.trim())
        .map_err(|e| ScemaDexError::Other(format!("intent json: {e}")))?;
    let (input_mint, in_dec) = resolve_token(&j.input)?;
    let (output_mint, _) = resolve_token(&j.output)?;
    let objective = match j.objective.to_lowercase().as_str() {
        "speed" => Objective::Speed,
        "stealth" => Objective::Stealth,
        _ => Objective::Price,
    };
    let side = match j.side.to_lowercase().as_str() {
        "buy" => Side::Buy,
        _ => Side::Sell,
    };
    let raw = (j.amount * 10f64.powi(in_dec as i32)) as u64;
    Ok(Intent {
        input_mint,
        output_mint,
        amount_in: Amount::new(raw, in_dec),
        side,
        objective,
        constraints: Constraints {
            max_slippage_bps: 150,
            deadline_unix: 0,
            max_legs: Some(3),
        },
    })
}

/// Extract the first balanced-looking `{...}` block from arbitrary LLM text and
/// parse it. Tolerates models that wrap JSON in prose or code fences.
pub fn extract_intent(text: &str) -> Result<Intent> {
    let start = text.find('{');
    let end = text.rfind('}');
    match (start, end) {
        (Some(s), Some(e)) if e > s => parse_intent_json(&text[s..=e]),
        _ => Err(ScemaDexError::Other("no JSON object found in LLM reply".into())),
    }
}

/// A deterministic, offline narration of a (possibly settled) solution.
pub fn narrate_fallback(solution: &Solution, bond: &Bond, outcome: Option<BondOutcome>) -> String {
    let legs = solution.route.legs.len();
    let venues: Vec<String> = solution
        .route
        .legs
        .iter()
        .map(|l| format!("{:?}", l.venue))
        .collect();
    let settled = match outcome {
        Some(BondOutcome::Honored) => "bond honored",
        Some(BondOutcome::Slashed) => "bond slashed",
        None => "settlement pending",
    };
    format!(
        "Routed across {legs} leg(s) [{}] at {:.0}% conviction, backing a {:.3} USDC bond — {settled}.",
        venues.join(", "),
        solution.conviction.0 * 100.0,
        bond.amount.as_usdc(),
    )
}

/// Configuration for an OpenAI-compatible chat-completions endpoint.
#[derive(Clone, Debug)]
pub struct LlmConfig {
    pub endpoint: String,
    pub model: String,
    pub api_key: String,
}

impl LlmConfig {
    /// Build from `SCEMADEX_LLM_ENDPOINT`, `SCEMADEX_LLM_MODEL`, `SCEMADEX_LLM_KEY`.
    /// Returns `None` if the key is unset (so callers can fall back to offline).
    pub fn from_env() -> Option<Self> {
        let api_key = std::env::var("SCEMADEX_LLM_KEY").ok()?;
        Some(Self {
            endpoint: std::env::var("SCEMADEX_LLM_ENDPOINT")
                .unwrap_or_else(|_| "https://api.openai.com/v1/chat/completions".to_string()),
            model: std::env::var("SCEMADEX_LLM_MODEL").unwrap_or_else(|_| "gpt-4o-mini".to_string()),
            api_key,
        })
    }
}

/// An LLM-backed natural-language client.
pub struct LlmClient {
    config: LlmConfig,
    http: reqwest::Client,
}

impl LlmClient {
    pub fn new(config: LlmConfig) -> Self {
        Self {
            config,
            http: reqwest::Client::new(),
        }
    }

    /// Parse a free-text request into an [`Intent`] via the LLM.
    pub async fn parse_intent(&self, nl: &str) -> Result<Intent> {
        let body = serde_json::json!({
            "model": self.config.model,
            "temperature": 0,
            "messages": [
                {"role": "system", "content": SYSTEM_PROMPT},
                {"role": "user", "content": nl},
            ],
        });
        let resp = self
            .http
            .post(&self.config.endpoint)
            .bearer_auth(&self.config.api_key)
            .json(&body)
            .send()
            .await
            .map_err(net_err)?
            .error_for_status()
            .map_err(net_err)?;
        let v: serde_json::Value = resp.json().await.map_err(net_err)?;
        let content = v["choices"][0]["message"]["content"]
            .as_str()
            .ok_or_else(|| ScemaDexError::Other("LLM reply had no content".into()))?;
        extract_intent(content)
    }
}

fn net_err(e: reqwest::Error) -> ScemaDexError {
    ScemaDexError::Other(format!("LLM request failed: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitives::Amount;
    use crate::policy::{Conviction, Solution};
    use crate::route::{Route, RouteLeg, Venue};

    #[test]
    fn parses_strict_intent_json() {
        let intent = parse_intent_json(
            r#"{"input":"SOL","output":"USDC","amount":1.5,"objective":"price","side":"sell"}"#,
        )
        .unwrap();
        assert_eq!(intent.amount_in.decimals, 9);
        assert_eq!(intent.amount_in.raw, 1_500_000_000);
        assert!(matches!(intent.objective, Objective::Price));
    }

    #[test]
    fn extracts_json_from_prose() {
        let reply = "Sure! Here you go:\n```json\n{\"input\":\"SOL\",\"output\":\"BONK\",\"amount\":2}\n```";
        let intent = extract_intent(reply).unwrap();
        assert_eq!(intent.amount_in.raw, 2_000_000_000);
    }

    #[test]
    fn rejects_unknown_symbol() {
        assert!(parse_intent_json(r#"{"input":"NOTATOKEN","output":"USDC","amount":1}"#).is_err());
    }

    #[test]
    fn narration_is_deterministic_offline() {
        let leg = RouteLeg {
            venue: Venue::Raydium,
            input_mint: Address::new("So11111111111111111111111111111111111111112").unwrap(),
            output_mint: Address::new("EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v").unwrap(),
            split_bps: 10_000,
            expected_out: Amount::new(1_000_000, 6),
        };
        let sol = Solution {
            intent_digest: "x".into(),
            route: Route { legs: vec![leg], expected_out: Amount::new(1_000_000, 6) },
            conviction: Conviction::clamped(0.8),
            rationale: "t".into(),
        };
        let bond = Bond {
            intent_digest: "x".into(),
            amount: crate::primitives::Usdc::from_usdc(4.0),
            min_out_raw: 990_000,
            deadline_unix: 0,
        };
        let s = narrate_fallback(&sol, &bond, Some(BondOutcome::Honored));
        assert!(s.contains("80% conviction"));
        assert!(s.contains("bond honored"));
    }
}
