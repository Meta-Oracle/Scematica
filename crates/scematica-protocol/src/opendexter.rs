use anyhow::{anyhow, bail, Context, Result};
use reqwest::{Client, Method, StatusCode, Url};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use solana_sdk::signature::Keypair;
use std::{env, time::Duration};

use crate::{
    client::{build_payment_payload, encode_payment_header},
    middleware::{PAYMENT_HEADER, PAYMENT_RESPONSE_HEADER},
    types::PaymentRequirements,
};

pub const SCEMATICA_X402_MARKETPLACE_URL_ENV: &str = "SCEMATICA_X402_MARKETPLACE_URL";
pub const SCEMATICA_X402_MAX_USDC_ENV: &str = "SCEMATICA_X402_MAX_USDC";
pub const SCEMATICA_X402_ALLOW_UNKNOWN_PRICE_ENV: &str = "SCEMATICA_X402_ALLOW_UNKNOWN_PRICE";
pub const SVM_PRIVATE_KEY_ENV: &str = "SVM_PRIVATE_KEY";
pub const EVM_PRIVATE_KEY_ENV: &str = "EVM_PRIVATE_KEY";
pub const DEFAULT_X402_MAX_USDC: f64 = 0.25;
pub const DEFAULT_MIN_QUALITY_SCORE: f64 = 75.0;
pub const DEFAULT_SEARCH_LIMIT: usize = 8;

const SOLANA_USDC_MINT: &str = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v";
const SOLANA_WSOL_MINT: &str = "So11111111111111111111111111111111111111112";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketplaceSearchRequest {
    pub query: String,
    pub verified_only: bool,
    pub min_quality_score: f64,
    pub limit: usize,
}

impl MarketplaceSearchRequest {
    pub fn new(query: impl Into<String>) -> Self {
        Self {
            query: query.into(),
            verified_only: false,
            min_quality_score: DEFAULT_MIN_QUALITY_SCORE,
            limit: DEFAULT_SEARCH_LIMIT,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketplaceApi {
    pub id: Option<String>,
    pub name: String,
    pub description: Option<String>,
    pub endpoint: Option<String>,
    pub seller: Option<String>,
    pub network: Option<String>,
    pub price_usdc: Option<f64>,
    pub quality_score: Option<f64>,
    pub verified: bool,
    pub call_count: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaymentOption {
    pub scheme: String,
    pub network: String,
    pub asset: String,
    pub amount_raw: u64,
    pub pay_to: String,
    pub max_timeout_seconds: u64,
    pub amount_usdc: Option<f64>,
    pub requirements: PaymentRequirements,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PriceCheck {
    pub url: String,
    pub method: String,
    pub status: u16,
    pub requires_payment: bool,
    pub description: Option<String>,
    pub payment_options: Vec<PaymentOption>,
    pub max_payment_usdc: f64,
    pub raw: Value,
}

impl PriceCheck {
    pub fn cheapest_option(&self) -> Option<&PaymentOption> {
        self.payment_options.iter().min_by(|a, b| {
            let a_price = a.amount_usdc.unwrap_or(f64::INFINITY);
            let b_price = b.amount_usdc.unwrap_or(f64::INFINITY);
            a_price.total_cmp(&b_price)
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WalletStatus {
    pub svm_configured: bool,
    pub evm_configured: bool,
    pub active_networks: Vec<String>,
    pub max_payment_usdc: f64,
    pub marketplace_url_configured: bool,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaidFetchResponse {
    pub status: u16,
    pub paid_usdc: Option<f64>,
    pub network: Option<String>,
    pub payment_response: Option<String>,
    pub data: Value,
}

#[derive(Debug, Clone)]
pub struct OpenDexterClient {
    http: Client,
    marketplace_url: Option<String>,
    max_payment_usdc: f64,
    allow_unknown_price: bool,
}

impl OpenDexterClient {
    pub fn from_env() -> Result<Self> {
        let max_payment_usdc = env::var(SCEMATICA_X402_MAX_USDC_ENV)
            .ok()
            .and_then(|v| v.parse::<f64>().ok())
            .unwrap_or(DEFAULT_X402_MAX_USDC);

        let allow_unknown_price = env_flag(SCEMATICA_X402_ALLOW_UNKNOWN_PRICE_ENV);
        let marketplace_url = env::var(SCEMATICA_X402_MARKETPLACE_URL_ENV)
            .ok()
            .filter(|v| !v.trim().is_empty());

        let http = Client::builder()
            .timeout(Duration::from_secs(20))
            .user_agent(format!("scematica/{}", env!("CARGO_PKG_VERSION")))
            .build()
            .context("failed to build x402 HTTP client")?;

        Ok(Self {
            http,
            marketplace_url,
            max_payment_usdc,
            allow_unknown_price,
        })
    }

    pub fn wallet_status(&self) -> WalletStatus {
        let svm_configured = env::var(SVM_PRIVATE_KEY_ENV)
            .map(|v| !v.trim().is_empty())
            .unwrap_or(false);
        let evm_configured = env::var(EVM_PRIVATE_KEY_ENV)
            .map(|v| !v.trim().is_empty())
            .unwrap_or(false);

        let mut active_networks = Vec::new();
        if svm_configured {
            active_networks.push("solana".to_string());
        }
        if evm_configured {
            active_networks.extend(
                ["base", "polygon", "arbitrum", "optimism", "avalanche"]
                    .into_iter()
                    .map(str::to_string),
            );
        }

        let mut warnings = Vec::new();
        if !svm_configured && !evm_configured {
            warnings.push(format!(
                "No x402 payment wallet configured. Set {} for Solana payments or {} for EVM payments.",
                SVM_PRIVATE_KEY_ENV, EVM_PRIVATE_KEY_ENV
            ));
        }
        if self.marketplace_url.is_none() {
            warnings.push(format!(
                "Marketplace search is disabled until {} points at a marketplace search API.",
                SCEMATICA_X402_MARKETPLACE_URL_ENV
            ));
        }
        if self.max_payment_usdc <= 0.0 {
            warnings.push(format!(
                "{} is zero or negative; paid x402 fetches will be blocked.",
                SCEMATICA_X402_MAX_USDC_ENV
            ));
        }

        WalletStatus {
            svm_configured,
            evm_configured,
            active_networks,
            max_payment_usdc: self.max_payment_usdc,
            marketplace_url_configured: self.marketplace_url.is_some(),
            warnings,
        }
    }

    pub async fn search(&self, request: MarketplaceSearchRequest) -> Result<Vec<MarketplaceApi>> {
        let marketplace_url = self.marketplace_url.as_ref().ok_or_else(|| {
            anyhow!(
                "{} is not set. Configure it with a marketplace search endpoint before using x402_search.",
                SCEMATICA_X402_MARKETPLACE_URL_ENV
            )
        })?;

        let mut url = Url::parse(marketplace_url)
            .with_context(|| format!("invalid {}", SCEMATICA_X402_MARKETPLACE_URL_ENV))?;
        url.query_pairs_mut()
            .append_pair("q", &request.query)
            .append_pair("query", &request.query)
            .append_pair("limit", &request.limit.to_string())
            .append_pair("verified", &request.verified_only.to_string())
            .append_pair("minQualityScore", &request.min_quality_score.to_string());

        let response = self.http.get(url).send().await?;
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        if !status.is_success() {
            bail!(
                "x402 marketplace search failed with HTTP {}: {}",
                status.as_u16(),
                truncate(&body, 400)
            );
        }

        let value: Value =
            serde_json::from_str(&body).context("marketplace response was not JSON")?;
        let mut apis = parse_marketplace_results(&value);
        if request.verified_only {
            apis.retain(|api| api.verified);
        }
        apis.retain(|api| api.quality_score.unwrap_or(0.0) >= request.min_quality_score);
        apis.truncate(request.limit);
        Ok(apis)
    }

    pub async fn check(&self, url: &str, method: Option<&str>) -> Result<PriceCheck> {
        let method = parse_method(method.unwrap_or("GET"))?;
        let response = self.http.request(method.clone(), url).send().await?;
        let status = response.status();
        let body = response.text().await.unwrap_or_default();

        if status.as_u16() == StatusCode::PAYMENT_REQUIRED.as_u16() {
            let raw: Value = serde_json::from_str(&body).unwrap_or(Value::String(body));
            let description = raw
                .pointer("/resource/description")
                .and_then(Value::as_str)
                .map(str::to_string);
            let payment_options = parse_payment_options(&raw);

            return Ok(PriceCheck {
                url: url.to_string(),
                method: method.as_str().to_string(),
                status: status.as_u16(),
                requires_payment: true,
                description,
                payment_options,
                max_payment_usdc: self.max_payment_usdc,
                raw,
            });
        }

        if status.is_success() {
            return Ok(PriceCheck {
                url: url.to_string(),
                method: method.as_str().to_string(),
                status: status.as_u16(),
                requires_payment: false,
                description: Some("Endpoint did not require x402 payment.".to_string()),
                payment_options: Vec::new(),
                max_payment_usdc: self.max_payment_usdc,
                raw: serde_json::json!({ "status": status.as_u16(), "bodyPreview": truncate(&body, 800) }),
            });
        }

        bail!(
            "x402 price check failed with HTTP {}: {}",
            status.as_u16(),
            truncate(&body, 400)
        );
    }

    pub async fn fetch(&self, url: &str, method: Option<&str>) -> Result<PaidFetchResponse> {
        let method = parse_method(method.unwrap_or("GET"))?;
        let check = self.check(url, Some(method.as_str())).await?;
        if !check.requires_payment {
            let response = self.http.request(method, url).send().await?;
            return response_to_fetch(None, None, response).await;
        }

        let option = check.cheapest_option().ok_or_else(|| {
            anyhow!("endpoint requires payment but did not return usable payment options")
        })?;
        self.guard_payment(option)?;

        if !option.network.to_ascii_lowercase().contains("solana") {
            bail!(
                "x402 endpoint requires {} payment. Scematica currently has native Rust signing for Solana x402 only.",
                option.network
            );
        }

        let keypair = read_svm_keypair()?;
        let decimals = token_decimals(option)?;
        let payload = build_payment_payload(&keypair, &option.requirements, decimals)?;
        let payment_header = encode_payment_header(&payload)?;

        let response = self
            .http
            .request(method, url)
            .header(PAYMENT_HEADER, payment_header)
            .send()
            .await?;

        response_to_fetch(option.amount_usdc, Some(option.network.clone()), response).await
    }

    fn guard_payment(&self, option: &PaymentOption) -> Result<()> {
        match option.amount_usdc {
            Some(amount) if amount <= self.max_payment_usdc => Ok(()),
            Some(amount) => bail!(
                "x402 payment {:.6} USDC exceeds {}={:.6}",
                amount,
                SCEMATICA_X402_MAX_USDC_ENV,
                self.max_payment_usdc
            ),
            None if self.allow_unknown_price => Ok(()),
            None => bail!(
                "x402 payment option has no USDC price. Set {}=1 to allow non-USDC or unknown-priced payments.",
                SCEMATICA_X402_ALLOW_UNKNOWN_PRICE_ENV
            ),
        }
    }
}

async fn response_to_fetch(
    paid_usdc: Option<f64>,
    network: Option<String>,
    response: reqwest::Response,
) -> Result<PaidFetchResponse> {
    let status = response.status();
    let payment_response = response
        .headers()
        .get(PAYMENT_RESPONSE_HEADER)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    let body = response.text().await.unwrap_or_default();
    let data = serde_json::from_str(&body).unwrap_or(Value::String(body));

    if !status.is_success() {
        bail!(
            "x402 fetch failed with HTTP {}: {}",
            status.as_u16(),
            truncate(&data.to_string(), 400)
        );
    }

    Ok(PaidFetchResponse {
        status: status.as_u16(),
        paid_usdc,
        network,
        payment_response,
        data,
    })
}

fn parse_method(method: &str) -> Result<Method> {
    method
        .parse::<Method>()
        .with_context(|| format!("invalid HTTP method '{method}'"))
}

fn parse_marketplace_results(value: &Value) -> Vec<MarketplaceApi> {
    let candidates = value
        .as_array()
        .or_else(|| value.get("results").and_then(Value::as_array))
        .or_else(|| value.get("apis").and_then(Value::as_array))
        .or_else(|| value.get("items").and_then(Value::as_array))
        .or_else(|| value.get("data").and_then(Value::as_array));

    candidates
        .into_iter()
        .flatten()
        .filter_map(parse_marketplace_api)
        .collect()
}

fn parse_marketplace_api(value: &Value) -> Option<MarketplaceApi> {
    let name = string_at(value, &["name", "title"])?;
    Some(MarketplaceApi {
        id: string_at(value, &["id", "slug"]),
        name,
        description: string_at(value, &["description", "summary"]),
        endpoint: string_at(value, &["endpoint", "url", "apiUrl", "api_url"]),
        seller: string_at(value, &["seller", "provider", "owner"]),
        network: string_at(value, &["network", "chain"]),
        price_usdc: number_at(
            value,
            &["priceUsdc", "price_usdc", "paidUsdc", "paid_usdc", "price"],
        ),
        quality_score: number_at(value, &["qualityScore", "quality_score", "score"]),
        verified: bool_at(value, &["verified", "isVerified"]).unwrap_or(false),
        call_count: u64_at(value, &["callCount", "call_count", "calls"]),
    })
}

fn parse_payment_options(raw: &Value) -> Vec<PaymentOption> {
    raw.get("accepts")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(parse_payment_option)
        .collect()
}

fn parse_payment_option(value: &Value) -> Option<PaymentOption> {
    let scheme = string_at(value, &["scheme"])?;
    let network = string_at(value, &["network"])?;
    let asset = string_at(value, &["asset"])?;
    let amount_raw = u64_at(value, &["amount", "maxAmountRequired"])?;
    let pay_to = string_at(value, &["pay_to", "payTo"])?;
    let max_timeout_seconds =
        u64_at(value, &["max_timeout_seconds", "maxTimeoutSeconds"]).unwrap_or(300);
    let extra = value.get("extra").cloned().unwrap_or(Value::Null);

    let amount_usdc = number_at(
        value,
        &["amount_usdc", "amountUsdc", "price_usdc", "priceUsdc"],
    )
    .or_else(|| {
        number_at(
            &extra,
            &["amount_usdc", "amountUsdc", "price_usdc", "priceUsdc"],
        )
    })
    .or_else(|| infer_usdc_amount(&asset, amount_raw, &extra));

    let requirements = PaymentRequirements {
        scheme: scheme.clone(),
        network: network.clone(),
        asset: asset.clone(),
        amount: amount_raw,
        pay_to: pay_to.clone(),
        max_timeout_seconds,
        extra,
    };

    Some(PaymentOption {
        scheme,
        network,
        asset,
        amount_raw,
        pay_to,
        max_timeout_seconds,
        amount_usdc,
        requirements,
    })
}

fn read_svm_keypair() -> Result<Keypair> {
    let raw = env::var(SVM_PRIVATE_KEY_ENV)
        .with_context(|| format!("{} is not set", SVM_PRIVATE_KEY_ENV))?;
    let trimmed = raw.trim();

    if trimmed.starts_with('[') {
        let bytes: Vec<u8> =
            serde_json::from_str(trimmed).context("SVM_PRIVATE_KEY JSON array is invalid")?;
        return Keypair::from_bytes(&bytes)
            .context("SVM_PRIVATE_KEY JSON array is not a Solana keypair");
    }

    let bytes = bs58::decode(trimmed)
        .into_vec()
        .context("SVM_PRIVATE_KEY is not valid base58")?;
    Keypair::from_bytes(&bytes).context("SVM_PRIVATE_KEY base58 is not a Solana keypair")
}

fn token_decimals(option: &PaymentOption) -> Result<u8> {
    if let Some(decimals) = u64_at(&option.requirements.extra, &["decimals"]) {
        return u8::try_from(decimals).context("token decimals exceed u8");
    }

    if option.asset == SOLANA_USDC_MINT {
        return Ok(6);
    }
    if option.asset == SOLANA_WSOL_MINT {
        return Ok(9);
    }

    bail!(
        "payment token decimals are unknown for asset {}; include extra.decimals in the 402 response",
        option.asset
    )
}

fn infer_usdc_amount(asset: &str, amount_raw: u64, extra: &Value) -> Option<f64> {
    if asset == SOLANA_USDC_MINT {
        let decimals = u64_at(extra, &["decimals"]).unwrap_or(6) as i32;
        return Some(amount_raw as f64 / 10_f64.powi(decimals));
    }
    None
}

fn env_flag(name: &str) -> bool {
    env::var(name)
        .map(|v| matches!(v.to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on"))
        .unwrap_or(false)
}

fn string_at(value: &Value, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|key| value.get(*key))
        .and_then(Value::as_str)
        .map(str::to_string)
}

fn number_at(value: &Value, keys: &[&str]) -> Option<f64> {
    keys.iter().find_map(|key| {
        let value = value.get(*key)?;
        value
            .as_f64()
            .or_else(|| value.as_str().and_then(|s| s.parse::<f64>().ok()))
    })
}

fn u64_at(value: &Value, keys: &[&str]) -> Option<u64> {
    keys.iter().find_map(|key| {
        let value = value.get(*key)?;
        value
            .as_u64()
            .or_else(|| value.as_str().and_then(|s| s.parse::<u64>().ok()))
    })
}

fn bool_at(value: &Value, keys: &[&str]) -> Option<bool> {
    keys.iter().find_map(|key| {
        let value = value.get(*key)?;
        value.as_bool().or_else(|| {
            value
                .as_str()
                .map(|s| matches!(s.to_ascii_lowercase().as_str(), "true" | "1" | "yes"))
        })
    })
}

fn truncate(input: &str, max_chars: usize) -> String {
    let mut out = String::new();
    for (idx, ch) in input.chars().enumerate() {
        if idx >= max_chars {
            out.push_str("...");
            return out;
        }
        out.push(ch);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_camel_case_payment_requirements() {
        let raw = serde_json::json!({
            "accepts": [{
                "scheme": "exact",
                "network": "solana-mainnet",
                "asset": SOLANA_USDC_MINT,
                "maxAmountRequired": "25000",
                "payTo": "11111111111111111111111111111111",
                "maxTimeoutSeconds": 120,
                "extra": { "decimals": 6 }
            }]
        });

        let options = parse_payment_options(&raw);
        assert_eq!(options.len(), 1);
        assert_eq!(options[0].amount_raw, 25_000);
        assert_eq!(options[0].amount_usdc, Some(0.025));
        assert_eq!(
            options[0].requirements.pay_to,
            "11111111111111111111111111111111"
        );
    }

    #[test]
    fn parses_marketplace_shapes() {
        let raw = serde_json::json!({
            "results": [{
                "name": "Weather API",
                "apiUrl": "https://example.com/weather",
                "priceUsdc": "0.01",
                "qualityScore": 91,
                "verified": true,
                "callCount": 42
            }]
        });

        let results = parse_marketplace_results(&raw);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].price_usdc, Some(0.01));
        assert_eq!(results[0].quality_score, Some(91.0));
        assert!(results[0].verified);
    }
}
