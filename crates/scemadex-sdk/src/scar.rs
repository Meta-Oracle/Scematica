//! **The Scar Market** — slash-certified negative knowledge (Primitive F).
//!
//! Everyone selling "alpha" sells successes, and failure data is normally
//! worthless because failure is free to fabricate. A slashed Conviction-Routing
//! bond changes that: the slash is the only **un-fakeable proof that a decision
//! cost real collateral**. The trajectory that led to it is certified pain.
//!
//! A [`ScarRecord`] can therefore only be minted *from a slash* — see
//! [`certify_scar`], which refuses honored settlements and zero-collateral
//! bonds. The record carries the trajectory `(state, action, reward, …)`
//! payload that produced the loss, and the slashed collateral is its
//! credential: the more an agent provably lost, the more its scar is worth as
//! a verified negative example for training veto heads and avoid-policies.
//!
//! In adversarial markets, knowing what kills you is worth more than knowing
//! what works. Pairs naturally with the counter-market
//! ([`crate::counter::CounterMarket`]): challenges generate slashes, slashes
//! generate scar inventory. Run `cargo run -p scemadex-sdk --example
//! scar_market` for the loop.

use std::sync::Mutex;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::bond::{Bond, BondOutcome};
use crate::error::{Result, ScemaDexError};
use crate::primitives::Usdc;

/// A slash-certified failure trajectory offered for sale.
///
/// Only mintable through [`certify_scar`] — the type's existence implies a bond
/// was actually slashed, so the payload cannot be fabricated junk dressed up as
/// a lesson.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ScarRecord {
    /// The agent that took the loss (and is selling the lesson).
    pub peer_id: String,
    /// Digest of the intent whose bond slashed.
    pub intent_digest: String,
    /// Collateral provably lost — the scar's credential (micro-USDC).
    pub slashed_collateral: Usdc,
    /// Number of RL transitions in the trajectory payload.
    pub transitions: u32,
    /// Model-agnostic encoding of the `(state, action, reward, next_state)`
    /// trajectory that led to the slash.
    pub payload: Vec<u8>,
    /// Asking price (micro-USDC).
    pub price: Usdc,
    /// Unix second of the slash (`0` = unset in the scaffold).
    pub slashed_unix: u64,
}

impl ScarRecord {
    /// Certified pain per micro-USDC of asking price — the buyer's value
    /// metric. Higher means more provably-real failure per dollar.
    pub fn collateral_per_price(&self) -> f64 {
        if self.price.0 == 0 {
            f64::INFINITY
        } else {
            self.slashed_collateral.0 as f64 / self.price.0 as f64
        }
    }
}

/// Mint a [`ScarRecord`] from a settled bond. The only constructor: refuses
/// honored outcomes (no loss, no lesson) and zero-collateral bonds (a free
/// failure proves nothing).
pub fn certify_scar(
    bond: &Bond,
    outcome: BondOutcome,
    peer_id: impl Into<String>,
    transitions: u32,
    payload: Vec<u8>,
    price: Usdc,
) -> Result<ScarRecord> {
    if outcome != BondOutcome::Slashed {
        return Err(ScemaDexError::Bond(
            "scar certification requires a slashed bond".into(),
        ));
    }
    if bond.amount.0 == 0 {
        return Err(ScemaDexError::Bond(
            "zero-collateral bonds cannot certify a scar".into(),
        ));
    }
    Ok(ScarRecord {
        peer_id: peer_id.into(),
        intent_digest: bond.intent_digest.clone(),
        slashed_collateral: bond.amount,
        transitions,
        payload,
        price,
        slashed_unix: 0,
    })
}

/// A marketplace for verified failure data, the negative complement of
/// [`crate::mesh::PeerMarket`]'s experience trade.
#[async_trait]
pub trait ScarMarket: Send + Sync {
    /// List a slash-certified failure trajectory for sale.
    async fn sell_scar(&self, scar: ScarRecord) -> Result<()>;
    /// Buy the best scar under a price cap. "Best" is the most certified
    /// collateral per micro-USDC of price — maximum provable pain per dollar.
    async fn buy_scar(&self, max_price: Usdc) -> Result<ScarRecord>;
}

/// In-process [`ScarMarket`]: a local book of scar records, mirroring
/// [`crate::mesh::LocalPeerMarket`] so a networked implementation can slot in
/// behind the same trait.
#[derive(Default)]
pub struct LocalScarMarket {
    scars: Mutex<Vec<ScarRecord>>,
}

impl LocalScarMarket {
    pub fn new() -> Self {
        Self::default()
    }

    /// Number of scars currently listed.
    pub fn scar_count(&self) -> usize {
        self.scars.lock().map(|v| v.len()).unwrap_or(0)
    }
}

#[async_trait]
impl ScarMarket for LocalScarMarket {
    async fn sell_scar(&self, scar: ScarRecord) -> Result<()> {
        self.scars
            .lock()
            .map_err(|_| ScemaDexError::Mesh("scar book lock poisoned".into()))?
            .push(scar);
        Ok(())
    }

    async fn buy_scar(&self, max_price: Usdc) -> Result<ScarRecord> {
        let mut scars = self
            .scars
            .lock()
            .map_err(|_| ScemaDexError::Mesh("scar book lock poisoned".into()))?;
        let idx = scars
            .iter()
            .enumerate()
            .filter(|(_, s)| s.price <= max_price)
            .max_by(|(_, a), (_, b)| {
                a.collateral_per_price()
                    .partial_cmp(&b.collateral_per_price())
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|(i, _)| i);
        match idx {
            Some(i) => Ok(scars.remove(i)),
            None => Err(ScemaDexError::Mesh("no scar under max_price".into())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slashed_bond(digest: &str, amount: u64) -> Bond {
        Bond {
            intent_digest: digest.into(),
            amount: Usdc(amount),
            min_out_raw: 1_000_000,
            deadline_unix: 0,
        }
    }

    #[test]
    fn honored_bonds_cannot_certify() {
        let bond = slashed_bond("d1", 1_000);
        assert!(certify_scar(&bond, BondOutcome::Honored, "p", 10, vec![], Usdc(100)).is_err());
    }

    #[test]
    fn zero_collateral_bonds_cannot_certify() {
        let bond = slashed_bond("d1", 0);
        assert!(certify_scar(&bond, BondOutcome::Slashed, "p", 10, vec![], Usdc(100)).is_err());
    }

    #[test]
    fn slash_certifies_and_carries_collateral() {
        let bond = slashed_bond("d1", 5_000);
        let scar =
            certify_scar(&bond, BondOutcome::Slashed, "p", 10, vec![1, 2], Usdc(100)).unwrap();
        assert_eq!(scar.slashed_collateral, Usdc(5_000));
        assert_eq!(scar.collateral_per_price(), 50.0);
    }

    #[tokio::test]
    async fn buys_most_collateral_per_dollar_under_cap() {
        let market = LocalScarMarket::new();
        let cheap_shallow = certify_scar(
            &slashed_bond("d1", 1_000),
            BondOutcome::Slashed,
            "shallow",
            5,
            vec![],
            Usdc(100), // 10x collateral per price
        )
        .unwrap();
        let cheap_deep = certify_scar(
            &slashed_bond("d2", 50_000),
            BondOutcome::Slashed,
            "deep",
            5,
            vec![],
            Usdc(500), // 100x collateral per price — better lesson per dollar
        )
        .unwrap();
        let over_cap = certify_scar(
            &slashed_bond("d3", 1_000_000),
            BondOutcome::Slashed,
            "pricey",
            5,
            vec![],
            Usdc(10_000),
        )
        .unwrap();
        market.sell_scar(cheap_shallow).await.unwrap();
        market.sell_scar(cheap_deep).await.unwrap();
        market.sell_scar(over_cap).await.unwrap();

        let bought = market.buy_scar(Usdc(1_000)).await.unwrap();
        assert_eq!(bought.peer_id, "deep");
        assert_eq!(market.scar_count(), 2); // consumed

        assert!(market.buy_scar(Usdc(50)).await.is_err());
    }
}
