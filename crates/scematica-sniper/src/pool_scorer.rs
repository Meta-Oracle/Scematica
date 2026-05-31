use crate::cache::CachedPool;

/// Predictive pool score in the range 0.0–100.0.
///
/// # Mathematical model
///
/// Uses an empirical Bayesian multiplicative model calibrated against 834 live
/// trades.  Each signal provides a likelihood ratio (LR) that updates a prior
/// probability of profitability.
///
/// Prior P(win) ≈ 0.10  (10% from live data; 30% stated win-rate overstates
///                        because most "wins" are sub-1% returns)
///
/// The raw posterior is mapped onto 0–100 via a logistic sigmoid:
///   score = 100 / (1 + exp(−k × (P_posterior − threshold)))
///   k = 18, threshold = 0.08
///
/// Key empirical findings:
///  • Pools < 3 SOL: ~0% actual profit rate  → hard reject (score 0–5)
///  • Pools 6.5–28 SOL: highest win-rate     → LR 4.5
///  • No velocity signal (stale pool): dead-on-arrival → LR 0.35
///  • No velocity signal (fresh, open_time=0): neutral  → LR 1.0 (estimated)
///  • Strong velocity (>2 SOL/s): runner     → LR 3.0
///  • Buy pressure (high quote/base skew): confirms momentum → LR 2.0
///  • PumpFun score ≥90: exceptional pre-grad momentum → LR 3.5
///  • Live inflow ≥1.5 SOL/s during evaluation window → +12 pts (score_full)
pub struct PoolScorer;

impl PoolScorer {
    pub fn score(
        pool: &CachedPool,
        pool_size_lamports: u64,
        base_vault_lamports: u64,
        detected_at_secs: u64,
    ) -> f64 {
        const SOL: f64 = 1_000_000_000.0;

        // ── Hard rejects: pools empirically proven to never be profitable ─────
        if pool_size_lamports == 0 {
            return 0.0; // ghost pool
        }
        let size_sol = pool_size_lamports as f64 / SOL;
        if size_sol < 1.0 {
            return 1.0; // sub-1 SOL: ~0% win rate, always rug
        }
        if size_sol < 3.0 {
            return 8.0; // 1-3 SOL: empirically unprofitable
        }

        // ── Age calculation ───────────────────────────────────────────────────
        let now_secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let effective_open_time = if pool.open_time > 0 {
            pool.open_time
        } else if detected_at_secs > 0 && detected_at_secs >= now_secs.saturating_sub(60) {
            detected_at_secs
        } else {
            0
        };

        let age_secs = if effective_open_time > 0 && effective_open_time <= now_secs {
            now_secs.saturating_sub(effective_open_time)
        } else if effective_open_time > now_secs + 30 {
            // Future timestamp = clock skew / fake; treat as very old
            9999
        } else {
            0 // unknown age
        };

        // ── Bayesian likelihood ratios for each signal ────────────────────────
        //
        // P(win | signals) ∝ P(win) × ∏ LR_i
        // Prior P(win) = 0.10 from empirical data
        let prior = 0.10_f64;
        let mut p = prior;

        // Signal 1: Pool size (most discriminating signal)
        // Log-normal distribution fit: winning pools cluster at 8–20 SOL.
        // Strong penalty for 3-6.5 SOL because these are typically micro-caps
        // with no sustained buying after rug-pump pattern.
        let size_lr = if size_sol < 3.0 {
            0.05 // hard reject (already handled above but keep consistent)
        } else if size_sol < 5.0 {
            0.15 // very risky
        } else if size_sol < 10.0 {
            0.35 // thin micro-cap
        } else if size_sol < 20.0 {
            0.85 // can run, but recent live data underperformed
        } else if size_sol <= 45.0 {
            4.2 // elite sweet spot: depth plus early momentum
        } else if size_sol <= 85.0 {
            3.2 // strong migration / launch-liquidity band
        } else if size_sol <= 120.0 {
            1.5 // viable but often later in the first move
        } else if size_sol <= 150.0 {
            0.70 // established pool, initial pump likely over
        } else {
            0.30 // whale pool, no entry edge
        };
        p *= size_lr;

        // Signal 2: Pool age (exponential decay of opportunity)
        // Ultra-fresh pools captured within 7s have 2x baseline win rate.
        // Pools > 2 minutes old: opportunity window has closed.
        let age_lr = if age_secs == 0 {
            0.60 // unknown age — suspicious
        } else if age_secs <= 7 {
            2.80 // ultra-fresh: maximum first-mover advantage
        } else if age_secs <= 20 {
            1.90 // fresh: still in pump window
        } else if age_secs <= 40 {
            1.10 // marginal
        } else if age_secs <= 90 {
            0.55 // late: pump likely peaked
        } else if age_secs <= 210 {
            0.20 // very late: almost no chance
        } else {
            0.05 // dead pool
        };
        p *= age_lr;

        // Signal 3: SOL inflow velocity (SOL per second)
        // Prefer pool.open_time when available. When open_time == 0 (most pump.fun
        // Raydium migrations open immediately, leaving this field at zero), fall back
        // to detected_at_secs for velocity estimation. If the detection timestamp is
        // within the last 60 s the pool is fresh and we can compute a rough velocity.
        // Using 0.35 ("no data → dead-pool suspect") for ALL open_time=0 pools was
        // wrong — essentially every modern pump.fun pool has open_time=0.
        let (velocity_lr, velocity_sol_per_sec) =
            if pool_size_lamports > 0 && pool.open_time > 0 && pool.open_time <= now_secs {
                let age_s = now_secs.saturating_sub(pool.open_time).max(1) as f64;
                let v = size_sol / age_s;
                let lr = if v >= 5.0 {
                    3.50 // exceptional: 5+ SOL/s — crowd stampede
                } else if v >= 2.0 {
                    2.80 // strong: 2–5 SOL/s
                } else if v >= 0.8 {
                    1.80 // moderate: buyers are active
                } else if v >= 0.2 {
                    1.20 // mild inflow
                } else {
                    0.65 // slow — little buying interest
                };
                (lr, v)
            } else if pool_size_lamports > 0
                && detected_at_secs > 0
                && detected_at_secs >= now_secs.saturating_sub(60)
            {
                // Pool was freshly detected; open_time=0 means it opens immediately
                // (standard for pump.fun Raydium migrations). We cannot derive velocity
                // without knowing when the pool was created — detected_at_secs is the
                // current wall-clock, not the pool birth time. Use a size-proportional
                // heuristic: larger pools accumulated more SOL and are more likely to be
                // genuine runners than micro-caps. Avoids the 0.35 dead-pool penalty
                // that wrongly tagged EVERY open_time=0 pool as suspicious.
                let size_lr = if size_sol >= 20.0 {
                    1.80 // large graduation — strong pre-existing demand
                } else if size_sol >= 10.0 {
                    1.40 // mid-tier graduation
                } else if size_sol >= 5.0 {
                    1.10 // small but plausible
                } else {
                    0.80 // tiny pool — likely micro/test launch
                };
                (size_lr, 0.0)
            } else {
                (0.35, 0.0) // stale or undetectable pool — dead-pool risk
            };
        p *= velocity_lr;

        // Signal 4: AMM buy-pressure ratio (quote_vault / base_vault)
        // Raydium x*y=k: as buyers purchase tokens the SOL side grows and token
        // side shrinks → ratio rises above the launch equilibrium.
        //
        // Thresholds re-calibrated for meme token reality (live data confirms all
        // ratios land in 0.00003–0.0002 range). Old thresholds (>=1.0, >=0.2, >=0.02)
        // were from DeFi blue-chip pools and were NEVER triggered — all meme pools
        // were getting LR=0.90 (slight negative) regardless of actual buy pressure.
        // A typical pump.fun graduation: 35 SOL / ~800M tokens × 10^6 raw = 4.4e-5.
        // Post-buying: ratio rises as SOL increases and tokens decrease.
        if base_vault_lamports > 0 {
            let ratio = pool_size_lamports as f64 / base_vault_lamports as f64;
            let pressure_lr = if ratio >= 0.0002 {
                2.20 // very heavily bought — 4–5× launch ratio, strong momentum
            } else if ratio >= 0.0001 {
                1.60 // clearly bought — 2–3× launch ratio
            } else if ratio >= 0.00005 {
                1.20 // mild buy skew — near/above typical launch equilibrium
            } else if ratio >= 0.00002 {
                1.00 // near launch balance — neutral
            } else {
                0.85 // token-heavy / sell-skewed — no bullish signal
            };
            p *= pressure_lr;
        } else {
            p *= 0.80; // no base vault read means no buy-pressure confirmation
        }

        // Signal 5: Expected-value from AMM constant-product formula
        // For a 0.01 SOL buy into a pool of S SOL:
        //   tokens_out ≈ base_reserve × 0.01 / (S + 0.01)  [ignoring fee]
        //   For a 2× exit, other buyers must add S + 0.01 more SOL in `t` seconds.
        //   P(success) ≈ min(1, velocity_sol_per_sec × no_pump_timeout / S)
        // Only applied when velocity data is available.
        if velocity_sol_per_sec > 0.0 && size_sol > 0.0 {
            let no_pump_secs = 15.0_f64; // matches config no_pump_timeout_secs
            let expected_inflow = velocity_sol_per_sec * no_pump_secs;
            let p_2x = (expected_inflow / size_sol).min(1.0);
            // Convert to LR: p_2x=0.1 → LR=1.0 (neutral), p_2x=1.0 → LR=2.5
            let ev_lr = 1.0 + 1.5 * p_2x;
            p *= ev_lr;
        }

        // Signal 6: PumpFun pre-graduation momentum score.
        // A non-zero pumpfun_score means the trending monitor tracked sustained buy
        // pressure on this token before it graduated to Raydium. High score = community
        // was already piling in, confirming organic demand rather than a cold launch.
        if pool.pumpfun_score > 0.0 {
            let pf_lr = if pool.pumpfun_score >= 90.0 {
                3.5 // exceptional: stampede-level pre-graduation buying
            } else if pool.pumpfun_score >= 80.0 {
                2.8 // very strong: sustained community momentum
            } else if pool.pumpfun_score >= 70.0 {
                2.0 // strong: meets our trending threshold
            } else {
                1.3 // mild: some momentum, didn't hit threshold
            };
            p *= pf_lr;
        }

        // Signal 7: Social presence (community backing = lower rug probability)
        // social_count is set by the caller when SocialLinksFilter metadata is available.
        // LR values calibrated: no socials = more likely anon dev rug.
        // Pass 0 to indicate "unknown/unenriched" — neutral.

        // ── Map posterior to 0–100 via logistic sigmoid ───────────────────────
        // sigmoid(x) = 100 / (1 + exp(−k(x − x0)))
        // Calibrated so that p=0.08 → score≈50, p=0.25 → score≈90
        let k = 28.0_f64;
        let x0 = 0.09_f64;
        let score = 100.0 / (1.0 + (-k * (p - x0)).exp());

        score.max(0.0).min(100.0)
    }

    /// Full scoring with all available signals, including live inflow rate measured during
    /// the filter evaluation window.
    ///
    /// `inflow_rate_now_sol_per_sec`: SOL/s flowing INTO the pool vault measured between
    /// pool detection and buy evaluation (~600ms window). 0.0 = unknown/bypassed.
    /// This is a *spot* rate — independent of historical velocity. It measures what's
    /// happening RIGHT NOW and is the strongest runner confirmation signal.
    pub fn score_full(
        pool: &CachedPool,
        pool_size_lamports: u64,
        base_vault_lamports: u64,
        detected_at_secs: u64,
        social_count: u8,
        inflow_rate_now_sol_per_sec: f64,
    ) -> f64 {
        let base = Self::score_with_socials(
            pool,
            pool_size_lamports,
            base_vault_lamports,
            detected_at_secs,
            social_count,
        );
        // Instantaneous inflow rate: additive post-sigmoid boost.
        // Calibrated so a 1+ SOL/s spot inflow rescues borderline pools (score 58 → 68).
        let inflow_boost = if inflow_rate_now_sol_per_sec >= 3.0 {
            18.0 // stampede: 3+ SOL/s pouring in — confirmed runner
        } else if inflow_rate_now_sol_per_sec >= 1.5 {
            12.0 // very strong: likely parabolic pump
        } else if inflow_rate_now_sol_per_sec >= 0.5 {
            7.0 // solid: active buying during evaluation window
        } else if inflow_rate_now_sol_per_sec >= 0.2 {
            3.0 // mild: some buying visible
        } else {
            0.0 // unknown or zero — no boost
        };
        (base + inflow_boost).max(0.0).min(100.0)
    }

    /// Variant that additionally applies a social-presence likelihood ratio.
    /// social_count: 0 = no socials found / unknown, 1–4 = twitter/telegram/website/discord.
    pub fn score_with_socials(
        pool: &CachedPool,
        pool_size_lamports: u64,
        base_vault_lamports: u64,
        detected_at_secs: u64,
        social_count: u8,
    ) -> f64 {
        let base = Self::score(
            pool,
            pool_size_lamports,
            base_vault_lamports,
            detected_at_secs,
        );
        // Social LR adjustment: applied AFTER logistic sigmoid so the boost is additive
        // on the score, not the posterior (prevents over-weighting on already-high scores).
        let social_boost = match social_count {
            0 => -4.0, // no socials: anonymous — slight penalty (may be fine for tiny projects)
            1 => 2.0,  // one link: a little community signal
            2 => 5.0,  // two links: credible project
            3 => 8.0,  // three links: well-connected
            _ => 10.0, // four links: full social stack — strongest signal
        };
        (base + social_boost).max(0.0).min(100.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use solana_sdk::pubkey::Pubkey;

    fn make_pool(open_time: u64) -> CachedPool {
        CachedPool {
            id: Pubkey::new_unique(),
            base_mint: Pubkey::new_unique(),
            quote_mint: Pubkey::new_unique(),
            base_vault: Pubkey::new_unique(),
            quote_vault: Pubkey::new_unique(),
            market_id: Pubkey::new_unique(),
            open_time,
            base_decimals: 9,
            quote_decimals: 9,
            pumpfun_score: 0.0,
        }
    }

    #[test]
    fn ghost_pool_scores_zero() {
        let pool = make_pool(0);
        assert_eq!(PoolScorer::score(&pool, 0, 0, 0), 0.0);
    }

    #[test]
    fn sub_1_sol_hard_reject() {
        let pool = make_pool(0);
        let score = PoolScorer::score(&pool, 500_000_000, 0, 0); // 0.5 SOL
        assert!(score < 5.0, "sub-1 SOL pool scored too high: {}", score);
    }

    #[test]
    fn tiny_pool_3sol_low_score() {
        let pool = make_pool(0);
        let score = PoolScorer::score(&pool, 2_000_000_000, 0, 0); // 2 SOL
        assert!(score < 20.0, "2 SOL pool scored too high: {}", score);
    }

    #[test]
    fn perfect_pool_scores_high() {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        // 30 SOL sweet spot, 5s old (ultra-fresh), with buy pressure
        let pool = make_pool(now - 5);
        let score = PoolScorer::score(&pool, 30_000_000_000, 1_000_000_000, 0);
        // Should be high: good size + ultra-fresh + strong velocity + buy pressure
        assert!(score >= 85.0, "perfect pool scored too low: {}", score);
    }

    #[test]
    fn stale_pool_scores_low() {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let pool = make_pool(now - 300); // 5 minutes old
        let score = PoolScorer::score(&pool, 15_000_000_000, 0, 0);
        assert!(score < 30.0, "stale pool scored too high: {}", score);
    }

    #[test]
    fn pumpfun_graduation_fresh_high_score() {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        // 30 SOL pool, detected now, with buy pressure
        let pool = make_pool(0); // open_time=0 (pump.fun style)
        let score = PoolScorer::score(&pool, 30_000_000_000, 500_000_000, now);
        assert!(
            score >= 70.0,
            "fresh pump.fun graduation scored too low: {}",
            score
        );
    }

    #[test]
    fn unknown_velocity_without_pressure_scores_low() {
        // Pool with open_time=0 AND detected_at=0 (truly unknown / not recently detected).
        // After the velocity-fallback fix, only pools with detected_at_secs=0 fall through
        // to the 0.35 dead-pool LR. A recently-detected open_time=0 pool legitimately
        // scores higher because we know it's fresh.
        let pool = make_pool(0);
        let score = PoolScorer::score(&pool, 18_000_000_000, 0, 0); // detected_at=0 → stale
        assert!(
            score < 60.0,
            "stale unknown-velocity pool without buy pressure scored too high: {}",
            score
        );
    }

    #[test]
    fn realistic_pumpfun_migration_scores_well() {
        // Realistic pump.fun graduation: 35 SOL, ~800M tokens with 6 decimals.
        // buy_pressure_ratio = 35e9 / 800e12 ≈ 4.4e-5.
        // With open_time=0 and fresh detection, this should score well.
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let pool = make_pool(0);
        let score = PoolScorer::score(&pool, 35_000_000_000, 800_000_000_000_000, now);
        assert!(
            score >= 80.0,
            "realistic pump.fun migration scored too low: {}",
            score
        );
    }

    #[test]
    fn old_established_pool_low_score() {
        let pool = make_pool(0);
        // 200 SOL pool, unknown age (not ultra-fresh)
        let score = PoolScorer::score(&pool, 200_000_000_000, 0, 0);
        assert!(score < 25.0, "large old pool scored too high: {}", score);
    }
}
