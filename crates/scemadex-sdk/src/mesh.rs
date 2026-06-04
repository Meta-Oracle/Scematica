use std::sync::Mutex;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::error::{Result, ScemaDexError};
use crate::intent::Intent;
use crate::policy::Solution;
use crate::primitives::Usdc;

/// A bonded solution offered by a peer agent, with the price it charges to
/// reveal it. Buying it means paying the fee *and* inheriting the seller's bond:
/// trust is enforced, not assumed.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct InferenceOffer {
    pub solution: Solution,
    pub price: Usdc,
    pub peer_id: String,
}

/// A batch of reinforcement-learning transitions a peer will sell so others can
/// bootstrap their own agents — a market for **experience**, not just answers.
/// The payload is a model-agnostic encoding of `(state, action, reward,
/// next_state)` tuples.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExperienceBatch {
    pub peer_id: String,
    pub transitions: u32,
    pub price: Usdc,
    pub payload: Vec<u8>,
}

/// The headline primitive: a mesh where autonomous agents **trade intelligence**.
///
/// Each node can sell its bonded inferences and its learned experience, and buy
/// better ones from peers — all metered and settled in USDC over x402. An agent
/// earns from what it knows and spends to learn faster, turning ScemaDEX from a
/// swap widget into an *economy of machine intelligence*.
#[async_trait]
pub trait PeerMarket: Send + Sync {
    /// Buy the best available bonded inference for an intent from the mesh.
    async fn buy_inference(&self, intent: &Intent) -> Result<InferenceOffer>;
    /// Publish a bonded inference for sale.
    async fn sell_inference(&self, offer: InferenceOffer) -> Result<()>;
    /// Buy a batch of experience to accelerate local training.
    async fn buy_experience(&self, max_price: Usdc) -> Result<ExperienceBatch>;
    /// Publish a batch of experience for sale.
    async fn sell_experience(&self, batch: ExperienceBatch) -> Result<()>;
}

/// An in-process [`PeerMarket`]: a local order book of inference offers and
/// experience batches. Fully functional and testable without a network; a
/// networked implementation (gossip + x402-settled) slots in later behind the
/// same trait, so callers and the dashboard never change.
#[derive(Default)]
pub struct LocalPeerMarket {
    offers: Mutex<Vec<InferenceOffer>>,
    experience: Mutex<Vec<ExperienceBatch>>,
}

impl LocalPeerMarket {
    pub fn new() -> Self {
        Self::default()
    }

    /// Number of inference offers currently listed.
    pub fn offer_count(&self) -> usize {
        self.offers.lock().map(|v| v.len()).unwrap_or(0)
    }

    /// Number of experience batches currently listed.
    pub fn experience_count(&self) -> usize {
        self.experience.lock().map(|v| v.len()).unwrap_or(0)
    }
}

#[async_trait]
impl PeerMarket for LocalPeerMarket {
    async fn buy_inference(&self, intent: &Intent) -> Result<InferenceOffer> {
        let digest = intent.digest();
        let mut offers = self
            .offers
            .lock()
            .map_err(|_| ScemaDexError::Mesh("offers lock poisoned".into()))?;
        // Cheapest matching offer wins; it is consumed (removed) on purchase.
        let idx = offers
            .iter()
            .enumerate()
            .filter(|(_, o)| o.solution.intent_digest == digest)
            .min_by_key(|(_, o)| o.price.0)
            .map(|(i, _)| i);
        match idx {
            Some(i) => Ok(offers.remove(i)),
            None => Err(ScemaDexError::Mesh(format!(
                "no inference offer for intent {digest}"
            ))),
        }
    }

    async fn sell_inference(&self, offer: InferenceOffer) -> Result<()> {
        self.offers
            .lock()
            .map_err(|_| ScemaDexError::Mesh("offers lock poisoned".into()))?
            .push(offer);
        Ok(())
    }

    async fn buy_experience(&self, max_price: Usdc) -> Result<ExperienceBatch> {
        let mut exp = self
            .experience
            .lock()
            .map_err(|_| ScemaDexError::Mesh("experience lock poisoned".into()))?;
        let idx = exp
            .iter()
            .enumerate()
            .filter(|(_, b)| b.price <= max_price)
            .min_by_key(|(_, b)| b.price.0)
            .map(|(i, _)| i);
        match idx {
            Some(i) => Ok(exp.remove(i)),
            None => Err(ScemaDexError::Mesh(
                "no experience batch under max_price".into(),
            )),
        }
    }

    async fn sell_experience(&self, batch: ExperienceBatch) -> Result<()> {
        self.experience
            .lock()
            .map_err(|_| ScemaDexError::Mesh("experience lock poisoned".into()))?
            .push(batch);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::policy::{Conviction, Solution};
    use crate::primitives::Amount;
    use crate::route::{Route, RouteLeg, Venue};

    fn offer_for(intent: &Intent, price: f64, peer: &str) -> InferenceOffer {
        let leg = RouteLeg {
            venue: Venue::Jupiter,
            input_mint: intent.input_mint.clone(),
            output_mint: intent.output_mint.clone(),
            split_bps: 10_000,
            expected_out: Amount::new(1_000, 6),
        };
        InferenceOffer {
            solution: Solution {
                intent_digest: intent.digest(),
                route: Route {
                    legs: vec![leg],
                    expected_out: Amount::new(1_000, 6),
                },
                conviction: Conviction::clamped(0.7),
                rationale: "peer".into(),
            },
            price: Usdc::from_usdc(price),
            peer_id: peer.into(),
        }
    }

    #[tokio::test]
    async fn buys_cheapest_matching_inference() {
        let market = LocalPeerMarket::new();
        let intent = crate::demo_intent();
        market
            .sell_inference(offer_for(&intent, 0.10, "expensive"))
            .await
            .unwrap();
        market
            .sell_inference(offer_for(&intent, 0.02, "cheap"))
            .await
            .unwrap();
        assert_eq!(market.offer_count(), 2);
        let bought = market.buy_inference(&intent).await.unwrap();
        assert_eq!(bought.peer_id, "cheap");
        assert_eq!(market.offer_count(), 1); // consumed
    }

    #[tokio::test]
    async fn experience_respects_price_cap() {
        let market = LocalPeerMarket::new();
        market
            .sell_experience(ExperienceBatch {
                peer_id: "p".into(),
                transitions: 100,
                price: Usdc::from_usdc(1.0),
                payload: vec![],
            })
            .await
            .unwrap();
        assert!(market
            .buy_experience(Usdc::from_usdc(0.5))
            .await
            .is_err());
        assert!(market
            .buy_experience(Usdc::from_usdc(2.0))
            .await
            .is_ok());
    }
}
