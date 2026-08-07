//! Networked [`PeerMarket`] client (feature `net`).
//!
//! [`RemotePeerMarket`] talks to a ScemaDEX relay over HTTP/JSON. The relay
//! gossips bonded inference offers and experience batches between agents;
//! settlement of the inference fee and the inherited Conviction-Routing bond
//! rides x402 payment headers attached at the application boundary.
//!
//! Endpoint contract (all `POST`, JSON bodies):
//! - `/inference/quote`  `{ "intent_digest": "…" }` → [`InferenceOffer`]
//! - `/inference/offer`  [`InferenceOffer`] → `204`
//! - `/experience/buy`   `{ "max_price": <micro_usdc> }` → [`ExperienceBatch`]
//! - `/experience/offer` [`ExperienceBatch`] → `204`

use async_trait::async_trait;

use crate::error::{Result, ScemaDexError};
use crate::intent::Intent;
use crate::mesh::{ExperienceBatch, InferenceOffer, PeerMarket};
use crate::primitives::Usdc;

/// HTTP client for a remote ScemaDEX peer relay.
pub struct RemotePeerMarket {
    base_url: String,
    http: reqwest::Client,
}

impl RemotePeerMarket {
    /// Connect to a relay rooted at `base_url` (e.g. `https://relay.scemadex.io`).
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into().trim_end_matches('/').to_string(),
            http: reqwest::Client::new(),
        }
    }

    /// The relay root this client targets.
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base_url, path)
    }
}

#[async_trait]
impl PeerMarket for RemotePeerMarket {
    async fn buy_inference(&self, intent: &Intent) -> Result<InferenceOffer> {
        let resp = self
            .http
            .post(self.url("/inference/quote"))
            .json(&serde_json::json!({ "intent_digest": intent.digest() }))
            .send()
            .await
            .map_err(net_err)?
            .error_for_status()
            .map_err(net_err)?;
        resp.json::<InferenceOffer>().await.map_err(net_err)
    }

    async fn sell_inference(&self, offer: InferenceOffer) -> Result<()> {
        self.http
            .post(self.url("/inference/offer"))
            .json(&offer)
            .send()
            .await
            .map_err(net_err)?
            .error_for_status()
            .map_err(net_err)?;
        Ok(())
    }

    async fn buy_experience(&self, max_price: Usdc) -> Result<ExperienceBatch> {
        let resp = self
            .http
            .post(self.url("/experience/buy"))
            .json(&serde_json::json!({ "max_price": max_price.0 }))
            .send()
            .await
            .map_err(net_err)?
            .error_for_status()
            .map_err(net_err)?;
        resp.json::<ExperienceBatch>().await.map_err(net_err)
    }

    async fn sell_experience(&self, batch: ExperienceBatch) -> Result<()> {
        self.http
            .post(self.url("/experience/offer"))
            .json(&batch)
            .send()
            .await
            .map_err(net_err)?
            .error_for_status()
            .map_err(net_err)?;
        Ok(())
    }
}

fn net_err(e: reqwest::Error) -> ScemaDexError {
    ScemaDexError::Mesh(format!("relay request failed: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_base_url() {
        let m = RemotePeerMarket::new("https://relay.example.io/");
        assert_eq!(m.base_url(), "https://relay.example.io");
        assert_eq!(m.url("/inference/quote"), "https://relay.example.io/inference/quote");
    }
}
