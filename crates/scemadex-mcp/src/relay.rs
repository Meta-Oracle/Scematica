//! Thin HTTP client for a running `scemadex-relay` (peer-mesh + signal oracle).
//!
//! Deliberately dependency-light: it speaks the relay's REST contract with raw
//! JSON bodies rather than importing the SDK types, so this crate carries no
//! solana-sdk / reqwest-0.12 conflicts and stays publishable on its own.

use anyhow::Result;

/// Outcome of a relay call, preserving the HTTP status so the MCP layer can turn
/// a `402 Payment Required` into an informative (non-error) tool result carrying
/// the x402 payment requirements.
pub struct RelayResponse {
    pub status: u16,
    pub body: String,
}

#[derive(Clone)]
pub struct RelayClient {
    base: String,
    http: reqwest::Client,
    /// Optional x402 payer (feature `pay`): when set, a `402` triggers an
    /// automatic signed payment + retry instead of being surfaced to the caller.
    #[cfg(feature = "pay")]
    payer: Option<std::sync::Arc<solana_sdk::signature::Keypair>>,
}

/// USDC has 6 decimals; the relay's x402 gate prices in micro-USDC.
#[cfg(feature = "pay")]
const USDC_DECIMALS: u8 = 6;

impl RelayClient {
    pub fn new(base_url: impl Into<String>) -> Self {
        let base = base_url.into();
        let base = base.trim_end_matches('/').to_string();
        Self {
            base,
            http: reqwest::Client::new(),
            #[cfg(feature = "pay")]
            payer: None,
        }
    }

    /// Attach an x402 payer so 402s are auto-settled (feature `pay`).
    #[cfg(feature = "pay")]
    pub fn with_payer(mut self, payer: std::sync::Arc<solana_sdk::signature::Keypair>) -> Self {
        self.payer = Some(payer);
        self
    }

    pub fn base_url(&self) -> &str {
        &self.base
    }

    async fn finish(resp: reqwest::Response) -> Result<RelayResponse> {
        let status = resp.status().as_u16();
        let body = resp.text().await.unwrap_or_default();
        Ok(RelayResponse { status, body })
    }

    /// Send a request. With feature `pay` and a configured payer, a `402`
    /// triggers building an x402 payment from the returned requirements and one
    /// retry with the `X-Payment` header. Otherwise the response is returned
    /// verbatim (the MCP layer surfaces the 402 requirements to the agent).
    async fn send(&self, req: reqwest::RequestBuilder) -> Result<RelayResponse> {
        #[cfg(feature = "pay")]
        if self.payer.is_some() {
            if let Some(retry) = req.try_clone() {
                let first = req.send().await?;
                if first.status().as_u16() == 402 {
                    let body = first.text().await.unwrap_or_default();
                    if let Some(header) = self.payment_header(&body) {
                        let paid = retry.header("X-Payment", header).send().await?;
                        return Self::finish(paid).await;
                    }
                    return Ok(RelayResponse { status: 402, body });
                }
                return Self::finish(first).await;
            }
        }
        Self::finish(req.send().await?).await
    }

    /// Build the `X-Payment` header value from a `402` PaymentRequired body.
    #[cfg(feature = "pay")]
    fn payment_header(&self, body_402: &str) -> Option<String> {
        let required: scematica_protocol::PaymentRequired = serde_json::from_str(body_402).ok()?;
        let requirements = required.accepts.first()?;
        let payer = self.payer.as_ref()?;
        let payload =
            scematica_protocol::client::build_payment_payload(payer, requirements, USDC_DECIMALS)
                .ok()?;
        scematica_protocol::client::encode_payment_header(&payload).ok()
    }

    /// `GET /signal/<kind>/<mint>` — reputation | pool_score | advice.
    pub async fn get_signal(&self, kind: &str, mint: &str) -> Result<RelayResponse> {
        let url = format!("{}/signal/{}/{}", self.base, kind, mint);
        self.send(self.http.get(url)).await
    }

    /// `POST /inference/quote` with `{ "intent_digest": "…" }`.
    pub async fn inference_quote(&self, intent_digest: &str) -> Result<RelayResponse> {
        let url = format!("{}/inference/quote", self.base);
        let body = serde_json::json!({ "intent_digest": intent_digest });
        self.send(self.http.post(url).json(&body)).await
    }

    /// `POST /experience/buy` with `{ "max_price": <micro-USDC> }`.
    pub async fn experience_buy(&self, max_price: u64) -> Result<RelayResponse> {
        let url = format!("{}/experience/buy", self.base);
        let body = serde_json::json!({ "max_price": max_price });
        self.send(self.http.post(url).json(&body)).await
    }

    /// `GET /health` — liveness probe.
    pub async fn health(&self) -> Result<RelayResponse> {
        let url = format!("{}/health", self.base);
        self.send(self.http.get(url)).await
    }
}
