use crate::bond::{Bond, BondEngine, BondOutcome};
use crate::error::Result;
use crate::intent::Intent;
use crate::policy::{RoutePolicy, Solution};
use crate::route::Fill;
use crate::venue::VenueExecutor;

/// The top-level entry point.
///
/// Generic over the injected [`RoutePolicy`], [`BondEngine`], and
/// [`VenueExecutor`] so the lean core stays fully decoupled from the bot. The
/// `scematica` feature provides concrete wirings (Deep Q* policy, x402 bond
/// engine, multi-DEX executor); the [`crate::reference_client`] helper wires the
/// lean reference impls for tests and examples.
pub struct ScemaDex<P, B, V> {
    pub policy: P,
    pub bond: B,
    pub venue: V,
}

impl<P, B, V> ScemaDex<P, B, V>
where
    P: RoutePolicy,
    B: BondEngine,
    V: VenueExecutor,
{
    pub fn new(policy: P, bond: B, venue: V) -> Self {
        Self { policy, bond, venue }
    }

    /// Solve an intent into a bonded solution (no execution yet). This is the
    /// unit other agents pay for in the marketplace: a route plus a slashable
    /// guarantee.
    pub async fn quote(&self, intent: &Intent) -> Result<(Solution, Bond)> {
        let solution = self.policy.solve(intent).await?;
        let bond = self.bond.escrow(&solution).await?;
        Ok((solution, bond))
    }

    /// Full path: solve → bond → execute → settle. Returns the realized fill and
    /// the bond outcome (honored or slashed).
    pub async fn execute(&self, intent: &Intent) -> Result<(Fill, BondOutcome)> {
        let (solution, bond) = self.quote(intent).await?;
        let fill = self.venue.execute(&solution.route).await?;
        let outcome = self.bond.settle(&bond, &fill).await?;
        Ok((fill, outcome))
    }
}
