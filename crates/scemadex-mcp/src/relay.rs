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
}

impl RelayClient {
    pub fn new(base_url: impl Into<String>) -> Self {
        let base = base_url.into();
        let base = base.trim_end_matches('/').to_string();
        Self {
            base,
            http: reqwest::Client::new(),
        }
    }

    pub fn base_url(&self) -> &str {
        &self.base
    }

    async fn finish(resp: reqwest::Response) -> Result<RelayResponse> {
        let status = resp.status().as_u16();
        let body = resp.text().await.unwrap_or_default();
        Ok(RelayResponse { status, body })
    }

    /// `GET /signal/<kind>/<mint>` — reputation | pool_score | advice.
    pub async fn get_signal(&self, kind: &str, mint: &str) -> Result<RelayResponse> {
        let url = format!("{}/signal/{}/{}", self.base, kind, mint);
        Self::finish(self.http.get(url).send().await?).await
    }

    /// `POST /inference/quote` with `{ "intent_digest": "…" }`.
    pub async fn inference_quote(&self, intent_digest: &str) -> Result<RelayResponse> {
        let url = format!("{}/inference/quote", self.base);
        let body = serde_json::json!({ "intent_digest": intent_digest });
        Self::finish(self.http.post(url).json(&body).send().await?).await
    }

    /// `POST /experience/buy` with `{ "max_price": <micro-USDC> }`.
    pub async fn experience_buy(&self, max_price: u64) -> Result<RelayResponse> {
        let url = format!("{}/experience/buy", self.base);
        let body = serde_json::json!({ "max_price": max_price });
        Self::finish(self.http.post(url).json(&body).send().await?).await
    }

    /// `GET /health` — liveness probe.
    pub async fn health(&self) -> Result<RelayResponse> {
        let url = format!("{}/health", self.base);
        Self::finish(self.http.get(url).send().await?).await
    }
}
