use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::intent::Intent;
use crate::route::{Route, RouteLeg, Venue};

/// The policy's self-assessed confidence in a solution, in `[0, 1]`.
///
/// In ScemaDEX this value is *priced*: higher conviction backs a larger bond
/// (Primitive D) and commands a larger inference fee (Primitive A). The Deep Q*
/// value head produces it directly.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Conviction(pub f64);

impl Conviction {
    pub fn clamped(v: f64) -> Self {
        Self(v.clamp(0.0, 1.0))
    }
}

/// A solved route plus the policy's conviction and a human/agent-readable
/// rationale (the latter is what the AI layer narrates in the dashboard).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Solution {
    pub intent_digest: String,
    pub route: Route,
    pub conviction: Conviction,
    pub rationale: String,
}

/// The brain. Turns an [`Intent`] into a [`Solution`].
///
/// The default crate ships [`ReferenceRoutePolicy`]; the `scematica` feature
/// injects the Deep Q* agent, whose action space is `(venue, split,
/// wait/execute)` and whose reward is realized fill vs. oracle mid minus any
/// slashed bond — so the agent learns to only bond what it can deliver.
#[async_trait]
pub trait RoutePolicy: Send + Sync {
    async fn solve(&self, intent: &Intent) -> Result<Solution>;
}

/// A naive single-venue reference policy. It does no real pricing — it exists so
/// the lean core compiles, tests, and documents end-to-end without the bot.
pub struct ReferenceRoutePolicy;

#[async_trait]
impl RoutePolicy for ReferenceRoutePolicy {
    async fn solve(&self, intent: &Intent) -> Result<Solution> {
        // Stub: route the full order through Jupiter and echo the input amount as
        // the expected output. Real venue math arrives with the `scematica`
        // feature / a live quoting `VenueExecutor`.
        let leg = RouteLeg {
            venue: Venue::Jupiter,
            input_mint: intent.input_mint.clone(),
            output_mint: intent.output_mint.clone(),
            split_bps: 10_000,
            expected_out: intent.amount_in,
        };
        let route = Route {
            legs: vec![leg],
            expected_out: intent.amount_in,
        };
        Ok(Solution {
            intent_digest: intent.digest(),
            route,
            conviction: Conviction::clamped(0.5),
            rationale: "reference single-venue stub (no live pricing)".to_string(),
        })
    }
}
