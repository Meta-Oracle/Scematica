"""Cadence, divergence, sequencer, safety grading, gas and codegen — all offline.

Each module's judgement lives in pure functions and dataclass properties, so the
interesting behaviour is testable without a network. The cases below are the ones that
were wrong at some point during development, which is why they are pinned:

* a cadence window where every publish was deviation-triggered reported a heartbeat it
  had never observed
* a divergence report said "agree within 58 bps (threshold 50)"
* a testnet leg was averaged into a mainnet consensus
"""
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "src"))

from alchem_link.aggregator import AggregatorInfo, Round, join_round_id, split_round_id
from alchem_link.cadence import profile_rounds
from alchem_link.codegen import generate_consumer
from alchem_link.divergence import DivergenceReport, Leg, common_pairs, networks_carrying
from alchem_link.gas import GAS_TRANSFER, FeeEstimate, GasReport
from alchem_link.safety import Audit, Finding
from alchem_link.sequencer import GRACE_PERIOD_SECS, SequencerStatus, is_l2
from alchem_link.watch import WatchEvent, poll_interval_for

BASE_TIME = 1_786_000_000


def make_round(offset: int, price: float, agg_round: int, phase: int = 7) -> Round:
    raw = int(price * 1e8)
    return Round(
        round_id=join_round_id(phase, agg_round),
        phase_id=phase,
        aggregator_round=agg_round,
        answer=raw,
        price=price,
        started_at=BASE_TIME + offset,
        updated_at=BASE_TIME + offset,
        answered_in_round=join_round_id(phase, agg_round),
    )


class RoundIdTests(unittest.TestCase):
    def test_round_ids_pack_a_phase_into_the_high_bits(self):
        """129127208515966893596 is not a counter — it is phase 7, round 32284.

        Read from mainnet ETH/USD; the twenty-digit id is what makes people think
        the round counter has overflowed.
        """
        self.assertEqual(split_round_id(129127208515966893596), (7, 32284))
        self.assertEqual(split_round_id(55340232221128655964), (3, 1116))

    def test_join_is_the_inverse_of_split(self):
        packed = join_round_id(3, 1116)
        self.assertEqual(split_round_id(packed), (3, 1116))

    def test_carried_over_detects_a_stale_round_answer(self):
        entry = make_round(0, 1900.0, 50)
        self.assertFalse(entry.carried_over)
        stale = Round(
            round_id=join_round_id(7, 50),
            phase_id=7,
            aggregator_round=50,
            answer=1,
            price=1.0,
            started_at=BASE_TIME,
            updated_at=BASE_TIME,
            answered_in_round=join_round_id(7, 49),
        )
        self.assertTrue(stale.carried_over)


class CadenceTests(unittest.TestCase):
    def test_recovers_a_clean_hourly_heartbeat(self):
        rounds = [make_round(i * 3600, 1900.0 + i, 100 + i) for i in range(10)]
        profile = profile_rounds(rounds, declared_heartbeat=3600, now=BASE_TIME + 9 * 3600)
        self.assertEqual(profile.observed_heartbeat, 3600)
        self.assertTrue(profile.heartbeat_observed)
        self.assertEqual(profile.heartbeat_verdict, "matches")

    def test_snaps_jittery_intervals_to_a_configured_value(self):
        """Block timestamps make real intervals 3624s or 3648s, never exactly 3600."""
        offsets = [0, 3624, 7248, 10884, 14520, 18168]
        rounds = [make_round(o, 1900.0, 100 + i) for i, o in enumerate(offsets)]
        profile = profile_rounds(rounds, declared_heartbeat=3600, now=BASE_TIME + 18168)
        self.assertEqual(profile.observed_heartbeat, 3600)

    def test_deviation_dominated_window_refuses_to_claim_a_heartbeat(self):
        """The bug this guards: a 49-minute Arbitrum window with 28 of 29 rounds
        deviation-triggered reported a 600s heartbeat it had never seen exercised.

        Modelled the same way — many fast deviation publishes and a single longer gap
        that merely ends the window. One interval at the ceiling is not a measurement.
        """
        offsets = [i * 60 for i in range(19)] + [19 * 60 + 451]
        rounds = [make_round(o, 1900.0 + i * 5, 100 + i) for i, o in enumerate(offsets)]
        profile = profile_rounds(rounds, declared_heartbeat=3600, now=BASE_TIME + 1591)
        self.assertFalse(profile.heartbeat_observed)
        self.assertEqual(profile.heartbeat_verdict, "not observed")
        self.assertEqual(profile.observed_heartbeat, 0)
        self.assertIn("deviation-triggered", profile.verdict_detail)

    def test_flags_a_declared_heartbeat_that_is_too_loose(self):
        rounds = [make_round(i * 60, 1900.0, 100 + i) for i in range(12)]
        profile = profile_rounds(rounds, declared_heartbeat=3600, now=BASE_TIME + 660)
        self.assertEqual(profile.heartbeat_verdict, "declared too loose")
        self.assertIn("miss a genuinely stalled feed", profile.verdict_detail)

    def test_flags_a_declared_heartbeat_that_is_too_tight(self):
        rounds = [make_round(i * 7200, 1900.0, 100 + i) for i in range(8)]
        profile = profile_rounds(rounds, declared_heartbeat=3600, now=BASE_TIME + 50400)
        self.assertEqual(profile.heartbeat_verdict, "declared too tight")

    def test_infers_the_deviation_threshold_from_early_publishes(self):
        # Nine hourly rounds, then one arriving early after a 0.5% move.
        rounds = [make_round(i * 3600, 1000.0, 100 + i) for i in range(9)]
        # Arrives 600s after the previous round — far inside the 3600s ceiling, so it
        # can only have been triggered by the 0.5% move.
        rounds.append(make_round(8 * 3600 + 600, 1005.0, 109))
        profile = profile_rounds(rounds, declared_heartbeat=3600, now=BASE_TIME + 29400)
        self.assertIsNotNone(profile.inferred_deviation_pct)
        self.assertAlmostEqual(profile.inferred_deviation_pct, 0.5, places=2)

    def test_too_little_history_is_reported_as_unknown(self):
        profile = profile_rounds([make_round(0, 1900.0, 100)], declared_heartbeat=3600)
        self.assertEqual(profile.samples, 0)
        self.assertEqual(profile.heartbeat_verdict, "unknown")
        self.assertFalse(profile.confident)

    def test_non_monotonic_timestamps_are_skipped_not_counted(self):
        rounds = [make_round(0, 1900.0, 100), make_round(0, 1901.0, 101)]
        profile = profile_rounds(rounds, declared_heartbeat=3600)
        self.assertEqual(profile.samples, 0)

    def test_stall_detected_when_current_age_exceeds_the_ceiling(self):
        rounds = [make_round(i * 3600, 1900.0, 100 + i) for i in range(6)]
        profile = profile_rounds(rounds, declared_heartbeat=3600, now=BASE_TIME + 18000 + 99999)
        self.assertTrue(profile.stalled)


def leg(network, price, age=10, stale=False, heartbeat=3600):
    return Leg(
        network=network,
        address="0x" + "11" * 20,
        description="ETH / USD",
        price=price,
        age_secs=age,
        heartbeat_secs=heartbeat,
        stale=stale,
    )


def build_report(legs, threshold=50.0):
    import statistics

    report = DivergenceReport(pair="ETH/USD", legs=legs, outlier_bps=threshold)
    fresh = [l.price for l in report.fresh if l.price > 0]
    if fresh:
        report.consensus = statistics.median(fresh)
        for entry in report.readable:
            entry.deviation_bps = (entry.price - report.consensus) / report.consensus * 10_000
    return report


class DivergenceTests(unittest.TestCase):
    def test_agreeing_chains(self):
        report = build_report([leg("ethereum", 2000.0), leg("base", 2000.5), leg("polygon", 2000.2)])
        self.assertEqual(report.verdict, "agree")
        self.assertEqual(report.outliers, [])

    def test_detects_an_outlier_leg(self):
        report = build_report([leg("ethereum", 2000.0), leg("base", 2000.0), leg("gnosis", 2100.0)])
        self.assertEqual(report.verdict, "diverged")
        self.assertEqual([o.network for o in report.outliers], ["gnosis"])

    def test_stale_legs_are_excluded_from_consensus(self):
        """A feed past its heartbeat is not evidence about the current price."""
        report = build_report([
            leg("ethereum", 2000.0),
            leg("base", 2000.0),
            leg("gnosis", 1000.0, age=99999, stale=True),
        ])
        self.assertEqual(report.consensus, 2000.0)
        self.assertEqual(len(report.fresh), 2)

    def test_stale_outlier_is_attributed_to_staleness(self):
        report = build_report([
            leg("ethereum", 2000.0),
            leg("base", 2000.0),
            leg("gnosis", 1000.0, age=99999, stale=True),
        ])
        self.assertEqual(report.verdict, "diverged")
        self.assertIn("stale by", report.detail)

    def test_fresh_outlier_is_explicitly_not_explained_by_staleness(self):
        report = build_report([leg("ethereum", 2000.0), leg("base", 2000.0), leg("bnb", 2100.0)])
        self.assertIn("not explained by staleness", report.detail)

    def test_agree_message_is_never_self_contradictory(self):
        """It once read "agree within 58.0 bps (threshold 50)"."""
        report = build_report([leg("a", 1000.0), leg("b", 1003.0), leg("c", 1000.0)])
        self.assertEqual(report.verdict, "agree")
        self.assertIn("worst leg", report.detail)
        self.assertIn("widest pairwise spread", report.detail)

    def test_spread_measures_the_widest_pair(self):
        report = build_report([leg("a", 1000.0), leg("b", 1010.0), leg("c", 1005.0)])
        self.assertAlmostEqual(report.spread_bps, 100.0, places=1)

    def test_single_fresh_leg_is_insufficient(self):
        report = build_report([leg("a", 1000.0), leg("b", 900.0, age=99999, stale=True)])
        self.assertEqual(report.verdict, "insufficient")

    def test_unreadable_legs_do_not_count_as_agreement(self):
        broken = leg("gnosis", 0.0)
        broken.error = "endpoint down"
        report = build_report([leg("a", 1000.0), leg("b", 1000.1), broken])
        self.assertEqual(len(report.readable), 2)

    def test_testnets_are_excluded_from_comparison_by_default(self):
        """Sepolia's feed is a test deployment; averaging it in describes nothing."""
        self.assertNotIn("sepolia", networks_carrying("ETH/USD"))
        self.assertIn("sepolia", networks_carrying("ETH/USD", include_testnets=True))

    def test_common_pairs_excludes_testnet_only_coverage(self):
        self.assertNotIn("sepolia", common_pairs())


class SequencerTests(unittest.TestCase):
    def _status(self, up=True, since=7200, error=""):
        return SequencerStatus(
            network="base",
            address="0x" + "22" * 20,
            up=up,
            started_at=BASE_TIME,
            since_secs=since,
            error=error,
        )

    def test_up_and_past_grace_is_ok(self):
        status = self._status(up=True, since=GRACE_PERIOD_SECS + 1)
        self.assertTrue(status.ok)
        self.assertEqual(status.state, "UP")

    def test_recently_restarted_is_not_ok(self):
        """The moment after a restart is the dangerous one, not the safe one."""
        status = self._status(up=True, since=60)
        self.assertTrue(status.in_grace_period)
        self.assertFalse(status.ok)
        self.assertEqual(status.state, "GRACE")
        self.assertIn("grace period remain", status.detail)

    def test_exactly_at_the_grace_boundary_is_still_gated(self):
        self.assertFalse(self._status(up=True, since=GRACE_PERIOD_SECS).ok)

    def test_down_is_reported(self):
        status = self._status(up=False, since=300)
        self.assertEqual(status.state, "DOWN")
        self.assertIn("DOWN", status.detail)

    def test_unreadable_feed_is_unknown_not_up(self):
        status = self._status(error="feed unreachable")
        self.assertEqual(status.state, "UNKNOWN")
        self.assertFalse(status.ok)

    def test_l2_classification(self):
        self.assertTrue(is_l2("base"))
        self.assertTrue(is_l2("arbitrum"))
        self.assertFalse(is_l2("ethereum"))
        self.assertFalse(is_l2("polygon"))


class SafetyGradingTests(unittest.TestCase):
    def _audit(self, *severities):
        audit = Audit(pair="ETH/USD", network="ethereum", address="0x" + "11" * 20)
        for index, severity in enumerate(severities):
            audit.add(Finding(f"CODE_{index}", severity, "t", "d"))
        return audit

    def test_worst_severity_ignores_info(self):
        self.assertEqual(self._audit("info", "info").worst, "ok")

    def test_worst_severity_picks_the_most_severe(self):
        self.assertEqual(self._audit("low", "critical", "medium").worst, "critical")

    def test_findings_sort_worst_first(self):
        audit = self._audit("low", "critical", "medium")
        self.assertEqual([f.severity for f in audit.sorted_findings], ["critical", "medium", "low"])

    def test_medium_blocks_consumption(self):
        self.assertFalse(self._audit("medium").safe_to_consume)

    def test_low_and_info_do_not_block(self):
        self.assertTrue(self._audit("low", "info").safe_to_consume)

    def test_no_findings_is_safe(self):
        self.assertTrue(self._audit().safe_to_consume)


class BoundsTests(unittest.TestCase):
    def _info(self, price, min_answer, max_answer):
        latest = make_round(0, price, 100)
        return AggregatorInfo(
            address="0x" + "11" * 20,
            network="ethereum",
            decimals=8,
            min_answer=min_answer,
            max_answer=max_answer,
            latest=latest,
        )

    def test_modern_ocr2_bounds_are_not_binding(self):
        """minAnswer=1, maxAnswer=int192 extreme — a circuit breaker in name only."""
        info = self._info(1900.0, 1, 95780971304118053647396689196894323976171195136475135)
        self.assertFalse(info.bounds_are_binding)
        self.assertGreater(info.floor_headroom, 1e10)

    def test_a_tight_floor_is_binding(self):
        """The LUNA case: the floor sits within reach of the market price."""
        info = self._info(1900.0, int(1000 * 1e8), int(1e12 * 1e8))
        self.assertTrue(info.bounds_are_binding)
        self.assertAlmostEqual(info.floor_headroom, 1.9, places=1)

    def test_a_tight_ceiling_is_binding(self):
        info = self._info(1900.0, 1, int(5000 * 1e8))
        self.assertTrue(info.bounds_are_binding)

    def test_missing_bounds_yield_no_headroom(self):
        info = self._info(1900.0, None, None)
        self.assertIsNone(info.floor_headroom)
        self.assertFalse(info.bounds_are_binding)


class GasTests(unittest.TestCase):
    def test_max_fee_leaves_headroom_for_base_fee_growth(self):
        estimate = FeeEstimate("standard", priority_fee_wei=1_000_000_000, base_fee_wei=2_000_000_000)
        self.assertEqual(estimate.max_fee_wei, 5_000_000_000)
        self.assertEqual(estimate.total_wei, 3_000_000_000)

    def test_transfer_cost(self):
        estimate = FeeEstimate("standard", 0, 1_000_000_000)
        self.assertEqual(estimate.cost_wei(GAS_TRANSFER), 21_000_000_000_000)

    def test_trend_from_the_next_base_fee(self):
        rising = GasReport("ethereum", "ETH", 1_000_000_000, 1_100_000_000, 20)
        falling = GasReport("ethereum", "ETH", 1_000_000_000, 900_000_000, 20)
        flat = GasReport("ethereum", "ETH", 1_000_000_000, 1_001_000_000, 20)
        self.assertEqual(rising.trend, "rising")
        self.assertEqual(falling.trend, "falling")
        self.assertEqual(flat.trend, "flat")

    def test_congestion_is_the_mean_fullness(self):
        report = GasReport("ethereum", "ETH", 1, 1, 3, gas_used_ratios=[0.4, 0.5, 0.6])
        self.assertAlmostEqual(report.congestion, 0.5)

    def test_usd_pricing_appears_only_when_a_price_is_known(self):
        estimate = FeeEstimate("standard", 0, 1_000_000_000)
        self.assertNotIn("transfer_cost_usd", estimate.as_dict())
        self.assertIn("transfer_cost_usd", estimate.as_dict(native_usd=2000.0))


class CodegenTests(unittest.TestCase):
    def test_l1_consumer_has_the_four_core_guards(self):
        result = generate_consumer("ETH/USD", "ethereum", "solidity")
        for guard in ("StalePrice", "InvalidPrice", "IncompleteRound", "StaleRoundAnswer"):
            self.assertIn(guard, result.code)

    def test_l1_consumer_omits_the_sequencer_gate(self):
        """Those lines are noise on an L1 and would not compile against nothing."""
        self.assertNotIn("SequencerDown", generate_consumer("ETH/USD", "ethereum").code)

    def test_l2_consumer_includes_the_sequencer_gate_and_grace_period(self):
        result = generate_consumer("ETH/USD", "base", "solidity")
        self.assertIn("SequencerDown", result.code)
        self.assertIn("GracePeriodNotOver", result.code)
        self.assertIn("0xBCF85224fc0756B9Fa45aA7892530B47e10b6433", result.code)
        self.assertIn("L2 sequencer uptime + grace period", result.guards)

    def test_max_age_uses_the_measured_per_chain_heartbeat(self):
        """Base is 1200s and Polygon 60s — a shared 3600 constant is the bug."""
        base = generate_consumer("ETH/USD", "base")
        polygon = generate_consumer("ETH/USD", "polygon")
        self.assertIn("MAX_AGE = 1380", base.code)
        self.assertIn("MAX_AGE = 69", polygon.code)

    def test_contract_name_is_derived_from_the_pair(self):
        self.assertIn("contract EthUsdConsumer", generate_consumer("ETH/USD").code)
        self.assertIn("contract LinkUsdConsumer", generate_consumer("LINK/USD").code)

    def test_address_is_checksummed_in_the_output(self):
        self.assertIn("0x5f4eC3Df9cbd43714FE2740f5E3616155c5b8419", generate_consumer("ETH/USD").code)

    def test_typescript_target(self):
        result = generate_consumer("ETH/USD", "ethereum", "typescript")
        self.assertIn("export async function readEthUsd", result.code)
        self.assertIn("MAX_AGE_SECONDS", result.code)

    def test_python_target(self):
        result = generate_consumer("ETH/USD", "ethereum", "python")
        self.assertIn("def latest_eth_usd", result.code)

    def test_unknown_language_rejected(self):
        with self.assertRaises(ValueError):
            generate_consumer("ETH/USD", "ethereum", "cobol")


class WatchTests(unittest.TestCase):
    def test_poll_interval_scales_with_the_heartbeat(self):
        self.assertEqual(poll_interval_for(60), 15.0)
        self.assertEqual(poll_interval_for(1200), 300.0)

    def test_poll_interval_is_clamped_at_both_ends(self):
        self.assertEqual(poll_interval_for(4), 5.0)
        self.assertEqual(poll_interval_for(86400), 300.0)

    def test_event_serialises_as_one_json_object(self):
        import json

        event = WatchEvent(
            kind="round", pair="ETH/USD", network="ethereum", timestamp=BASE_TIME,
            price=1900.0, round_id=5, age_secs=12, change_pct=0.5, interval_secs=3600,
        )
        payload = json.loads(event.to_json())
        self.assertEqual(payload["kind"], "round")
        self.assertEqual(payload["change_pct"], 0.5)

    def test_optional_fields_are_omitted_when_absent(self):
        import json

        payload = json.loads(WatchEvent("round", "ETH/USD", "ethereum", BASE_TIME).to_json())
        self.assertNotIn("change_pct", payload)
        self.assertNotIn("interval_secs", payload)


if __name__ == "__main__":
    unittest.main()
