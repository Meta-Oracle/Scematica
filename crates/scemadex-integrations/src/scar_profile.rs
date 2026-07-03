//! Bridge: **Scar-Market statistics → adversarial-simulator [`ScarProfile`]**.
//!
//! This is the load-bearing link of the self-improving flywheel. The Scar Market
//! (`scemadex_sdk::scar`) accumulates *verified* failures — a [`ScarRecord`] can
//! only be minted from a slashed Conviction bond, so its `slashed_collateral` is
//! un-fakeable proof of how badly a decision cost. The bond ledger records how
//! *often* bonds slash. Together they tell us the current rug meta: how frequent
//! and how severe failures are right now.
//!
//! We fold that into a [`ScarProfile`], which parameterises the
//! [`scematica_nn::AdversarialPoolSim`] the agent pre-trains against
//! (`DQNAgent::pretrain_on_simulator`). The bot writes the profile to
//! `scematica-scar-profile.json`; the sniper reads it at boot. Verified failures
//! literally shape what the next agent learns to avoid.
//!
//! **Honest limitation:** scars record *that* and *how badly* a decision failed,
//! not *which archetype* it was (rug vs honeypot vs pump-dump). So we derive the
//! overall **failure rate** and **severity** from real data and keep the relative
//! archetype mix as a prior — scaling, not inventing, the distribution.

use scemadex_sdk::{BondLedger, ScarRecord};
use scematica_nn::ScarProfile;

/// Reference slashed collateral (micro-USDC) that maps to "typical" severity.
/// Scars far above this push the simulated peak-before-rug and volatility up.
const REFERENCE_COLLATERAL: f64 = 5_000_000.0; // 5 USDC

/// Build a [`ScarProfile`] from live Scar-Market data.
///
/// - **Failure rate** comes from the bond ledger: `slashed / total`. The default
///   profile's archetype rates (which sum to the default failure rate) are scaled
///   so their total matches the observed rate — preserving the rug/honeypot/
///   pump-dump/bleed *mix* while tracking the observed *frequency*.
/// - **Severity** comes from the mean slashed collateral of the scars, relative
///   to [`REFERENCE_COLLATERAL`], nudging the simulated peak-before-rug and
///   volatility.
///
/// With no history, returns [`ScarProfile::default`].
pub fn scar_profile_from_market(ledger: &BondLedger, scars: &[ScarRecord]) -> ScarProfile {
    let base = ScarProfile::default();
    if ledger.total() == 0 {
        return base;
    }

    let observed_fail = (ledger.slashed as f64 / ledger.total() as f64).clamp(0.0, 0.98);
    let base_fail = base.rug_rate + base.honeypot_rate + base.pump_dump_rate + base.slow_bleed_rate;
    // Scale factor to retarget the total failure rate onto the observed one,
    // keeping the archetype proportions from the prior.
    let scale = if base_fail > 1e-9 {
        observed_fail / base_fail
    } else {
        1.0
    };

    // Severity multiplier from mean slashed collateral.
    let severity = if scars.is_empty() {
        1.0
    } else {
        let mean_collateral: f64 = scars
            .iter()
            .map(|s| s.slashed_collateral.0 as f64)
            .sum::<f64>()
            / scars.len() as f64;
        (mean_collateral / REFERENCE_COLLATERAL).clamp(0.5, 2.5)
    };

    ScarProfile {
        rug_rate: (base.rug_rate * scale).clamp(0.0, 1.0),
        honeypot_rate: (base.honeypot_rate * scale).clamp(0.0, 1.0),
        pump_dump_rate: (base.pump_dump_rate * scale).clamp(0.0, 1.0),
        slow_bleed_rate: (base.slow_bleed_rate * scale).clamp(0.0, 1.0),
        mean_time_to_rug_secs: base.mean_time_to_rug_secs,
        // Higher-severity scars → deeper peaks used to lure buyers before the rug,
        // and more chop along the way.
        mean_peak_before_rug_pct: base.mean_peak_before_rug_pct * severity,
        mean_legit_return_pct: base.mean_legit_return_pct,
        volatility: (base.volatility * severity).clamp(0.0, 1.0),
    }
}

/// Convenience: build the profile and write it to `path` (where the sniper reads
/// it at boot). Atomic via [`ScarProfile::save`].
pub fn write_scar_profile(
    path: &str,
    ledger: &BondLedger,
    scars: &[ScarRecord],
) -> std::io::Result<ScarProfile> {
    let profile = scar_profile_from_market(ledger, scars);
    profile.save(path)?;
    Ok(profile)
}

#[cfg(test)]
mod tests {
    use super::*;
    use scemadex_sdk::Usdc;

    fn scar(collateral: u64) -> ScarRecord {
        ScarRecord {
            peer_id: "p".into(),
            intent_digest: "d".into(),
            slashed_collateral: Usdc(collateral),
            transitions: 5,
            payload: vec![],
            price: Usdc(1000),
            slashed_unix: 0,
        }
    }

    #[test]
    fn empty_history_is_default() {
        let p = scar_profile_from_market(&BondLedger::default(), &[]);
        let d = ScarProfile::default();
        assert!((p.rug_rate - d.rug_rate).abs() < 1e-9);
    }

    #[test]
    fn higher_slash_rate_raises_failure_rates() {
        // 8 slashed of 10 → observed fail 0.8; the profile's archetype rates are
        // rescaled so their total tracks 0.8 while preserving their proportions.
        let ledger = BondLedger {
            honored: 2,
            slashed: 8,
        };
        let p = scar_profile_from_market(&ledger, &[scar(5_000_000)]);
        let total = p.rug_rate + p.honeypot_rate + p.pump_dump_rate + p.slow_bleed_rate;
        assert!(
            (total - 0.8).abs() < 0.02,
            "total failure rate should track observed 0.8, got {total}"
        );
        // Proportions preserved: rug is still the largest failure mode.
        assert!(p.rug_rate >= p.honeypot_rate);
    }

    #[test]
    fn severe_scars_deepen_peaks_and_vol() {
        let ledger = BondLedger {
            honored: 5,
            slashed: 5,
        };
        let mild = scar_profile_from_market(&ledger, &[scar(2_500_000)]);
        let harsh = scar_profile_from_market(&ledger, &[scar(12_500_000)]);
        assert!(harsh.mean_peak_before_rug_pct > mild.mean_peak_before_rug_pct);
        assert!(harsh.volatility >= mild.volatility);
    }
}
