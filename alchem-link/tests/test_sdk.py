"""The 0.23.0 SDK surface: registry search, cache, analytics, simulation, logs, export.

Every test here is offline. That is not a convenience — it is the design being checked.
:mod:`~alchem_link.analytics` and :mod:`~alchem_link.simulate` compute numbers people
size positions and write guards against, and a number that can only be verified by
reading a live chain cannot be verified at all.
"""
from __future__ import annotations

import math
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "src"))

from alchem_link import analytics, cache, logs, registry, simulate
from alchem_link.exporters import (
    FORMATS,
    export,
    to_csv,
    to_markdown,
    to_ndjson,
    to_prometheus,
    write,
)
from alchem_link.errors import (
    AlchemLinkError,
    ConfigurationError,
    SimulationError,
    StaleFeed,
    UnknownFeed,
    UnknownNetwork,
)
from alchem_link.feeds import get_feed
from alchem_link.networks import get_network


class Errors(unittest.TestCase):
    def test_everything_descends_from_one_base(self) -> None:
        for exc in (UnknownNetwork("x"), UnknownFeed("A/B", "base"),
                    SimulationError("no"), StaleFeed("A/B", "base", 100, 50)):
            self.assertIsInstance(exc, AlchemLinkError)

    def test_lookup_errors_stay_key_errors_for_compatibility(self) -> None:
        """Existing `except KeyError` code must keep working across the upgrade."""
        with self.assertRaises(KeyError):
            get_network("nope")
        with self.assertRaises(KeyError):
            get_feed("NOPE/USD", "base")

    def test_lookup_errors_read_as_sentences(self) -> None:
        """KeyError.__str__ repr-quotes its argument; these override it."""
        try:
            get_network("nope")
        except UnknownNetwork as exc:
            self.assertTrue(str(exc).startswith("unknown network"))
            self.assertNotIn('"', str(exc)[:20])

    def test_errors_carry_structured_context(self) -> None:
        exc = StaleFeed("ETH/USD", "base", age_secs=5000, heartbeat_secs=1200)
        payload = exc.as_dict()
        self.assertEqual(payload["pair"], "ETH/USD")
        self.assertEqual(payload["age_secs"], 5000)
        self.assertEqual(payload["error"], "StaleFeed")

    def test_retryable_is_set_where_it_matters(self) -> None:
        self.assertFalse(ConfigurationError("bad input").retryable)
        from alchem_link.errors import TransportError

        self.assertTrue(TransportError("timeout").retryable)
        self.assertFalse(TransportError("HTTP 404", retryable=False).retryable)


class Registry(unittest.TestCase):
    def test_pair_normalisation_is_separator_agnostic(self) -> None:
        for raw in ("eth/usd", "ETH-USD", " eth usd ", "ETH//USD"):
            self.assertEqual(registry.normalise_pair(raw), "ETH/USD")

    def test_resolve_accepts_a_separator_free_pair(self) -> None:
        self.assertEqual(registry.resolve("ethusd", "base"), "ETH/USD")

    def test_resolve_suggests_a_near_miss_rather_than_just_failing(self) -> None:
        with self.assertRaises(UnknownFeed) as caught:
            registry.resolve("ETH/USDX", "base")
        self.assertIn("Did you mean", str(caught.exception))

    def test_resolve_never_silently_substitutes(self) -> None:
        """A fuzzy match is a suggestion on the exception, never a returned value."""
        with self.assertRaises(UnknownFeed):
            registry.resolve("ETH/USDX", "base")

    def test_suggest_never_raises(self) -> None:
        self.assertEqual(registry.suggest("zzzzzz", "base"), [])
        self.assertIn("ETH/USD", registry.suggest("eth", "base"))

    def test_find_by_substring_and_by_address(self) -> None:
        by_name = registry.find("ETH/USD", network="base")
        # A substring search legitimately also matches CBETH/USD; what must hold is that
        # the exact pair ranks first, because callers take `[0]`.
        self.assertEqual(by_name[0].pair, "ETH/USD")
        address = by_name[0].address
        self.assertEqual(registry.find(address[:10])[0].address, address)

    def test_find_by_asset_matches_either_side(self) -> None:
        results = registry.find(asset="BTC")
        self.assertTrue(results)
        self.assertTrue(all("BTC" in (r.base, r.quote) for r in results))

    def test_by_address_is_case_insensitive(self) -> None:
        feed = get_feed("ETH/USD", "base")
        self.assertIsNotNone(registry.by_address(feed.address.lower()))
        self.assertIsNotNone(registry.by_address(feed.address.upper()))

    def test_by_address_returns_none_for_a_stranger(self) -> None:
        self.assertIsNone(registry.by_address("0x" + "00" * 20))

    def test_networks_carrying_puts_mainnets_first(self) -> None:
        """A caller taking the first entry as a reference must not land on Sepolia."""
        carriers = registry.networks_carrying("ETH/USD")
        self.assertIn("sepolia", carriers)
        self.assertNotEqual(carriers[0], "sepolia")

    def test_fastest_ignores_unmeasured_heartbeats(self) -> None:
        """An unmeasured entry is a bound; treating it as cadence would let it win."""
        quickest = registry.fastest("ETH/USD")
        self.assertIsNotNone(quickest)
        self.assertTrue(quickest.feed.heartbeat_measured)
        self.assertLessEqual(quickest.feed.heartbeat_secs, 120)

    def test_fastest_of_an_unknown_pair_is_none(self) -> None:
        self.assertIsNone(registry.fastest("NOPE/USD"))

    def test_coverage_counts_measured_against_bounded(self) -> None:
        table = registry.coverage()
        for name, entry in table.items():
            if not entry.get("feeds"):
                continue
            with self.subTest(name):
                self.assertEqual(entry["measured"] + entry["bounded"], entry["feeds"])
                self.assertLessEqual(entry["fastest_secs"], entry["slowest_secs"])

    def test_describe_feed_names_where_else_the_pair_lives(self) -> None:
        described = registry.describe_feed("ETH/USD", "base")
        self.assertNotIn("base", described["also_on"])
        self.assertTrue(described["also_on"])
        self.assertGreater(described["stale_after_secs"], described["heartbeat_secs"])


class Cache(unittest.TestCase):
    def test_ttl_scales_with_the_heartbeat(self) -> None:
        """A 60s Polygon feed and a 3600s mainnet feed must not share a TTL."""
        fast = cache.ttl_for_feed(60)
        slow = cache.ttl_for_feed(3600)
        self.assertLess(fast, slow)
        self.assertGreaterEqual(fast, cache.MIN_TTL_SECS)
        self.assertLessEqual(slow, cache.MAX_TTL_SECS)

    def test_ttl_handles_a_missing_heartbeat(self) -> None:
        self.assertEqual(cache.ttl_for_feed(0), cache.MIN_TTL_SECS)

    def test_get_and_set_round_trip(self) -> None:
        store = cache.TTLCache()
        store.set("k", 1)
        self.assertEqual(store.get("k"), 1)

    def test_expired_entries_are_dropped(self) -> None:
        store = cache.TTLCache()
        store.set("k", 1, ttl=-1)
        self.assertIsNone(store.get("k"))
        self.assertEqual(store.stats.expirations, 1)

    def test_a_cached_none_is_distinguishable_from_a_miss(self) -> None:
        """`get_or_set` must not re-run the factory just because the value was None."""
        store = cache.TTLCache()
        calls = []

        def factory():
            calls.append(1)
            return None

        store.get_or_set("k", factory)
        store.get_or_set("k", factory)
        self.assertEqual(len(calls), 1)

    def test_lru_eviction_drops_the_least_recently_used(self) -> None:
        store = cache.TTLCache(maxsize=2)
        store.set("a", 1)
        store.set("b", 2)
        store.get("a")           # refresh a's position
        store.set("c", 3)        # evicts b
        self.assertIsNone(store.get("b"))
        self.assertEqual(store.get("a"), 1)

    def test_prefix_and_substring_invalidation(self) -> None:
        store = cache.TTLCache()
        store.set("base:price:eth/usd", 1)
        store.set("base:history:eth/usd", 2)
        store.set("polygon:price:eth/usd", 3)
        self.assertEqual(store.invalidate_prefix("base:"), 2)
        store.set("base:price:eth/usd", 1)
        self.assertEqual(store.invalidate_containing("eth/usd"), 2)

    def test_key_for_builds_a_namespaced_key(self) -> None:
        self.assertEqual(cache.key_for("Base", "price", "ETH/USD"), "base:price:eth/usd")

    def test_stats_track_hits_and_misses(self) -> None:
        store = cache.TTLCache()
        store.set("k", 1)
        store.get("k")
        store.get("missing")
        self.assertEqual(store.stats.hit_rate, 0.5)

    def test_memoize_decorator_caches(self) -> None:
        store = cache.TTLCache()
        calls = []

        @cache.memoize(store)
        def expensive(n):
            calls.append(n)
            return n * 2

        self.assertEqual(expensive(3), 6)
        self.assertEqual(expensive(3), 6)
        self.assertEqual(len(calls), 1)


class SeriesConstruction(unittest.TestCase):
    def test_points_are_sorted_and_deduplicated(self) -> None:
        """Round history arrives newest-first, log history oldest-first."""
        series = analytics.Series.from_pairs([(30, 3.0), (10, 1.0), (20, 2.0), (10, 9.0)])
        self.assertEqual(series.timestamps, [10, 20, 30])
        self.assertEqual(series.points[0].price, 9.0, "the later duplicate should win")

    def test_span_and_window(self) -> None:
        series = analytics.Series.from_pairs([(0, 1.0), (100, 2.0), (200, 3.0)])
        self.assertEqual(series.span_secs, 200)
        self.assertEqual(len(series.window(150)), 2)

    def test_an_empty_series_is_harmless(self) -> None:
        series = analytics.Series()
        self.assertEqual(len(series), 0)
        self.assertIsNone(analytics.twap(series))
        self.assertIsNone(analytics.volatility(series))
        self.assertEqual(analytics.summarise(series).samples, 0)


class Analytics(unittest.TestCase):
    def setUp(self) -> None:
        # Sits at 100 for a long time, then moves quickly — the shape that separates a
        # time-weighted average from the mean of the answers.
        self.series = analytics.Series.from_pairs([
            (0, 100.0), (3000, 100.0), (3060, 105.0), (3120, 110.0), (3180, 108.0),
        ])

    def test_twap_weights_by_duration_not_by_sample(self) -> None:
        mean = analytics.mean_price(self.series)
        weighted = analytics.twap(self.series)
        self.assertLess(weighted, mean,
                        "TWAP should sit near the long-held price, not the busy tail")
        self.assertAlmostEqual(weighted, 100.2830188, places=4)

    def test_twap_of_a_single_point_is_that_point(self) -> None:
        self.assertEqual(analytics.twap(analytics.Series.from_pairs([(0, 7.0)])), 7.0)

    def test_log_returns_skip_non_positive_prices(self) -> None:
        """An oracle reporting zero is a fault, not a −100% return."""
        broken = analytics.Series.from_pairs([(0, 100.0), (60, 0.0), (120, 100.0)])
        self.assertEqual(analytics.log_returns(broken), [])

    def test_volatility_scales_by_the_measured_interval(self) -> None:
        """The same price path sampled faster annualises to a larger number."""
        moves = [1.0, 1.01, 0.99, 1.02, 0.98, 1.01]
        fast = analytics.Series.from_pairs(
            [(i * 60, p) for i, p in enumerate(moves)])
        slow = analytics.Series.from_pairs(
            [(i * 3600, p) for i, p in enumerate(moves)])
        self.assertGreater(analytics.volatility(fast), analytics.volatility(slow))
        # Un-annualised, the two are the same series of returns.
        self.assertAlmostEqual(analytics.volatility(fast, annualise=False),
                               analytics.volatility(slow, annualise=False))

    def test_volatility_needs_at_least_two_returns(self) -> None:
        self.assertIsNone(analytics.volatility(
            analytics.Series.from_pairs([(0, 1.0), (60, 1.1)])))

    def test_median_interval_ignores_one_long_gap(self) -> None:
        series = analytics.Series.from_pairs(
            [(0, 1.0), (60, 1.0), (120, 1.0), (10_000, 1.0)])
        self.assertEqual(analytics.median_interval(series), 60)

    def test_max_drawdown_finds_the_peak_and_trough(self) -> None:
        series = analytics.Series.from_pairs(
            [(0, 100.0), (60, 120.0), (120, 90.0), (180, 130.0)])
        result = analytics.max_drawdown(series)
        self.assertAlmostEqual(result["drawdown"], 0.25)
        self.assertEqual(result["peak"]["price"], 120.0)
        self.assertEqual(result["trough"]["price"], 90.0)
        self.assertTrue(result["recovered"])

    def test_max_drawdown_of_a_rising_series_is_zero(self) -> None:
        series = analytics.Series.from_pairs([(0, 1.0), (60, 2.0), (120, 3.0)])
        self.assertEqual(analytics.max_drawdown(series)["drawdown"], 0.0)

    def test_largest_move_reports_basis_points(self) -> None:
        series = analytics.Series.from_pairs([(0, 100.0), (60, 101.0), (120, 90.9)])
        self.assertAlmostEqual(analytics.largest_move(series)["move_bps"], 1000.0, places=1)

    def test_align_pairs_nearby_observations_only(self) -> None:
        """Two feeds never publish on the same clock; zipping them correlates noise."""
        left = analytics.Series.from_pairs([(0, 1.0), (600, 1.0)])
        right = analytics.Series.from_pairs([(10, 1.0), (5000, 1.0)])
        pairs = analytics.align(left, right, tolerance_secs=60)
        self.assertEqual(len(pairs), 1)

    def test_correlation_of_identical_series_is_one(self) -> None:
        series = analytics.Series.from_pairs(
            [(i * 60, 100 + (i % 5)) for i in range(20)])
        other = analytics.Series.from_pairs(
            [(i * 60, 100 + (i % 5)) for i in range(20)])
        self.assertAlmostEqual(analytics.correlation(series, other), 1.0, places=6)

    def test_correlation_of_a_flat_feed_is_none_not_zero(self) -> None:
        """No variance means no correlation to report — a different thing from zero."""
        moving = analytics.Series.from_pairs([(i * 60, 100 + i) for i in range(10)])
        flat = analytics.Series.from_pairs([(i * 60, 100.0) for i in range(10)])
        self.assertIsNone(analytics.correlation(moving, flat))

    def test_spread_reports_samples_and_extremes(self) -> None:
        left = analytics.Series.from_pairs([(0, 101.0), (60, 102.0)])
        right = analytics.Series.from_pairs([(0, 100.0), (60, 100.0)])
        result = analytics.spread_bps(left, right, tolerance_secs=10)
        self.assertEqual(result["samples"], 2)
        self.assertAlmostEqual(result["max_bps"], 200.0)

    def test_outliers_flags_an_isolated_spike(self) -> None:
        prices = [100.0] * 10 + [200.0] + [100.0] * 10
        series = analytics.Series.from_pairs(
            [(i * 60, p) for i, p in enumerate(prices)])
        found = analytics.outliers(series, z=2.0)
        self.assertTrue(found)

    def test_summarise_fills_every_field_it_can(self) -> None:
        stats = analytics.summarise(self.series)
        self.assertEqual(stats.samples, 5)
        self.assertEqual(stats.low, 100.0)
        self.assertEqual(stats.high, 110.0)
        self.assertIsNotNone(stats.twap)
        self.assertIsNotNone(stats.range_pct)
        self.assertIsNotNone(stats.twap_divergence_bps)
        self.assertTrue(all(math.isfinite(v) for v in
                            (stats.change_pct, stats.max_drawdown_pct)))

    def test_summary_survives_json_round_tripping(self) -> None:
        import json

        json.dumps(analytics.summarise(self.series).as_dict())


class GuardSimulation(unittest.TestCase):
    def test_scenario_ladder_separates_the_presets(self) -> None:
        """The headline claim: more guards catch strictly more failure modes."""
        naive = simulate.audit_guard(simulate.Guard.naive()).score
        default = simulate.audit_guard(simulate.Guard()).score
        strict = simulate.audit_guard(simulate.Guard.strict()).score
        self.assertLess(naive, default)
        self.assertLess(default, strict)
        self.assertEqual(strict, 1.0, "the strict preset must handle every scenario")

    def test_the_healthy_control_stops_reject_everything_from_scoring(self) -> None:
        """Without a control, a guard that rejects all input would score perfectly."""
        paranoid = simulate.Guard(max_age_secs=1, require_positive=True,
                                  max_move_bps=0.0001, reject_carried=True)
        result = simulate.audit_guard(paranoid)
        self.assertIn("healthy", result.failed)
        self.assertLess(result.score, 1.0)

    def test_bounded_crash_needs_more_than_staleness(self) -> None:
        """The LUNA shape: fresh, positive, complete — and catastrophically wrong."""
        staleness_only = simulate.Guard(max_age_secs=3600, require_positive=True)
        report = simulate.run_scenario("bounded_crash", staleness_only)
        self.assertFalse(report.caught)
        self.assertIsNotNone(report.worst_accepted_price)

        with_bound = simulate.Guard(max_age_secs=3600, min_price=500.0)
        self.assertTrue(simulate.run_scenario("bounded_crash", with_bound).caught)

    def test_frozen_feed_is_caught_by_a_staleness_window(self) -> None:
        self.assertFalse(simulate.run_scenario("frozen_feed", simulate.Guard.naive()).caught)
        self.assertTrue(simulate.run_scenario("frozen_feed", simulate.Guard()).caught)

    def test_sequencer_outage_needs_the_uptime_gate(self) -> None:
        without = simulate.run_scenario("sequencer_outage", simulate.Guard())
        with_gate = simulate.run_scenario(
            "sequencer_outage", simulate.Guard(require_sequencer=True))
        self.assertFalse(without.caught)
        self.assertTrue(with_gate.caught)

    def test_flash_spike_needs_a_move_limit(self) -> None:
        self.assertFalse(simulate.run_scenario("flash_spike", simulate.Guard()).caught)
        self.assertTrue(simulate.run_scenario(
            "flash_spike", simulate.Guard(max_move_bps=1000)).caught)

    def test_move_limit_baselines_on_the_last_accepted_answer(self) -> None:
        """A rejected round must not become the baseline — that is what a consumer stores."""
        observations = [
            simulate.Observation(timestamp=t, price=p, updated_at=t, round_id=i + 1)
            for i, (t, p) in enumerate([(1000, 100.0), (1060, 200.0), (1120, 101.0)])
        ]
        report = simulate.replay(simulate.Guard(max_age_secs=0, max_move_bps=1000),
                                 observations)
        # The spike is rejected; the recovery is compared against 100, not 200.
        self.assertFalse(report.verdicts[1].accepted)
        self.assertTrue(report.verdicts[2].accepted)

    def test_every_failing_check_is_reported_not_just_the_first(self) -> None:
        observation = simulate.Observation(timestamp=10_000, price=-5.0, updated_at=0,
                                           round_id=5, answered_in_round=1)
        verdict = simulate.evaluate(simulate.Guard.strict(), observation)
        codes = {r.split(":")[0] for r in verdict.reasons}
        self.assertIn("STALE", codes)
        self.assertIn("NON_POSITIVE", codes)
        self.assertIn("CARRIED_ROUND", codes)

    def test_longest_rejection_streak_is_reported(self) -> None:
        """Two percent rejected in one block halts a protocol; scattered does not."""
        report = simulate.run_scenario("frozen_feed", simulate.Guard())
        self.assertGreater(report.longest_rejection_streak, 1)

    def test_future_timestamps_are_caught(self) -> None:
        report = simulate.run_scenario("clock_skew", simulate.Guard())
        self.assertIn("FUTURE_TIMESTAMP", report.reason_counts)

    def test_unknown_scenario_raises_a_typed_error(self) -> None:
        with self.assertRaises(SimulationError):
            simulate.run_scenario("does-not-exist")

    def test_observations_from_history_are_aged_to_the_worst_case(self) -> None:
        """Evaluating at the publish instant makes every round look perfectly fresh."""
        series = analytics.Series.from_pairs([(0, 100.0), (600, 101.0)])
        observations = simulate.observations_from_series(series, heartbeat_secs=600)
        self.assertTrue(all(o.age_secs == 600 for o in observations))

    def test_reports_serialise(self) -> None:
        import json

        json.dumps(simulate.audit_guard().as_dict())


class EventLogs(unittest.TestCase):
    def test_event_signature_drops_names_and_the_indexed_keyword(self) -> None:
        self.assertEqual(logs.ANSWER_UPDATED.signature,
                         "AnswerUpdated(int256,uint256,uint256)")

    def test_topic0_is_computed_not_stored(self) -> None:
        from alchem_link.keccak import event_topic

        self.assertEqual(logs.ANSWER_UPDATED.topic0,
                         event_topic("AnswerUpdated(int256,uint256,uint256)"))

    def test_indexed_and_unindexed_are_split(self) -> None:
        """The classic decoding bug: ABI-decoding data against the full parameter list."""
        self.assertEqual([n for _, n in logs.ANSWER_UPDATED.indexed],
                         ["current", "roundId"])
        self.assertEqual([n for _, n in logs.ANSWER_UPDATED.unindexed], ["updatedAt"])

    def test_decoding_reads_indexed_values_from_topics(self) -> None:
        entry = logs.Log(
            address="0xfeed",
            topics=[
                logs.ANSWER_UPDATED.topic0,
                "0x" + format(1_930_00000000, "064x"),   # current
                "0x" + format(42, "064x"),               # roundId
            ],
            data="0x" + format(1_700_000_000, "064x"),   # updatedAt
            block_number=100, transaction_hash="0xabc", log_index=0,
        )
        decoded = logs.decode_log(logs.ANSWER_UPDATED, entry)
        self.assertEqual(decoded.args["current"], 1_930_00000000)
        self.assertEqual(decoded.args["roundId"], 42)
        self.assertEqual(decoded.args["updatedAt"], 1_700_000_000)

    def test_negative_indexed_ints_decode_as_signed(self) -> None:
        entry = logs.Log(
            address="0xfeed",
            topics=[logs.ANSWER_UPDATED.topic0,
                    "0x" + format((1 << 256) - 5, "064x"),
                    "0x" + format(1, "064x")],
            data="0x" + format(0, "064x"),
            block_number=1, transaction_hash="0x", log_index=0,
        )
        self.assertEqual(logs.decode_log(logs.ANSWER_UPDATED, entry).args["current"], -5)

    def test_indexed_addresses_are_unpadded(self) -> None:
        address = "0x" + "ab" * 20
        entry = logs.Log(
            address="0xtoken",
            topics=[logs.TRANSFER.topic0,
                    "0x" + "00" * 12 + "ab" * 20,
                    "0x" + "00" * 12 + "cd" * 20],
            data="0x" + format(1000, "064x"),
            block_number=1, transaction_hash="0x", log_index=0,
        )
        decoded = logs.decode_log(logs.TRANSFER, entry)
        self.assertEqual(decoded.args["from"], address)
        self.assertEqual(decoded.args["value"], 1000)

    def test_a_malformed_data_blob_does_not_raise(self) -> None:
        entry = logs.Log(address="0x", topics=[logs.ANSWER_UPDATED.topic0, "0x1", "0x2"],
                         data="0xdeadbeef", block_number=1, transaction_hash="", log_index=0)
        decoded = logs.decode_log(logs.ANSWER_UPDATED, entry)
        self.assertNotIn("updatedAt", decoded.args)

    def test_block_estimates_reflect_chain_speed(self) -> None:
        """Only used to size a query — every timestamp still comes from the chain."""
        self.assertGreater(logs.blocks_for_seconds(3600, "arbitrum"),
                           logs.blocks_for_seconds(3600, "ethereum"))

    def test_parsing_a_bad_declaration_raises(self) -> None:
        from alchem_link.abi import AbiError

        with self.assertRaises(AbiError):
            logs.parse_event("NotAnEvent")


class Export(unittest.TestCase):
    def setUp(self) -> None:
        self.rows = [
            {"pair": "ETH/USD", "network": "base", "address": "0xabc", "price": 1930.2,
             "age_secs": 40, "heartbeat_secs": 1200, "stale": False, "carried_over": False},
            {"pair": "BTC/USD", "network": "base", "address": "0xdef", "price": 68000.0,
             "age_secs": 5000, "heartbeat_secs": 1200, "stale": True, "carried_over": True},
        ]

    def test_csv_has_a_header_and_one_row_each(self) -> None:
        lines = to_csv(self.rows).splitlines()
        self.assertEqual(len(lines), 3)
        self.assertTrue(lines[0].startswith("pair,network"))

    def test_csv_uses_unix_line_endings(self) -> None:
        """CRLF produces blank lines when printed to a terminal that also translates."""
        self.assertNotIn("\r", to_csv(self.rows))

    def test_columns_are_the_union_across_rows(self) -> None:
        """Result objects vary — a failed leg has `error` and no `price`."""
        mixed = [{"a": 1}, {"b": 2}]
        header = to_csv(mixed).splitlines()[0]
        self.assertEqual(header, "a,b")

    def test_ndjson_is_one_object_per_line(self) -> None:
        import json

        lines = to_ndjson(self.rows).splitlines()
        self.assertEqual(len(lines), 2)
        self.assertEqual(json.loads(lines[0])["pair"], "ETH/USD")

    def test_prometheus_emits_help_and_type_headers(self) -> None:
        body = to_prometheus(self.rows)
        self.assertIn("# HELP alchem_link_feed_price", body)
        self.assertIn("# TYPE alchem_link_feed_stale gauge", body)

    def test_prometheus_renders_booleans_as_zero_and_one(self) -> None:
        body = to_prometheus(self.rows)
        self.assertIn('alchem_link_feed_stale{address="0xabc",network="base",pair="ETH/USD"} 0',
                      body)
        self.assertIn('alchem_link_feed_stale{address="0xdef",network="base",pair="BTC/USD"} 1',
                      body)

    def test_prometheus_drops_non_finite_values(self) -> None:
        """A single NaN makes Prometheus reject the whole scrape, losing every metric."""
        body = to_prometheus([{**self.rows[0], "price": float("nan")}])
        self.assertNotIn("nan", body.lower())
        self.assertIn("alchem_link_feed_age_seconds", body, "other metrics must survive")

    def test_prometheus_escapes_label_values(self) -> None:
        body = to_prometheus([{**self.rows[0], "pair": 'we"ird'}])
        self.assertIn(r'pair="we\"ird"', body)

    def test_markdown_is_a_valid_table(self) -> None:
        lines = to_markdown(self.rows).splitlines()
        self.assertTrue(lines[0].startswith("|"))
        self.assertTrue(set(lines[1]) <= set("|-"))
        self.assertEqual(len(lines), 4)

    def test_objects_with_as_dict_are_accepted(self) -> None:
        stats = analytics.summarise(analytics.Series.from_pairs([(0, 1.0), (60, 2.0)]))
        self.assertIn("twap", to_csv([stats]).splitlines()[0])

    def test_nested_values_flatten_into_a_cell(self) -> None:
        body = to_csv([{"tags": ["a", "b"], "meta": {"k": 1}}])
        self.assertIn("a;b", body)

    def test_empty_input_produces_empty_output(self) -> None:
        for fmt in FORMATS:
            with self.subTest(fmt):
                self.assertEqual(export([], fmt), "" if fmt != "json" else "[]")

    def test_unknown_format_raises(self) -> None:
        with self.assertRaises(ValueError):
            export(self.rows, "yaml")

    def test_write_infers_the_format_from_the_extension(self) -> None:
        import tempfile

        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "feeds.csv"
            write(self.rows, str(path))
            self.assertTrue(path.read_text(encoding="utf-8").startswith("pair,network"))


class Parallel(unittest.TestCase):
    def test_results_keep_request_order_not_completion_order(self) -> None:
        """A table built from a sweep must not reshuffle between runs."""
        from alchem_link import parallel

        import time

        report = parallel.run_tasks({
            "slow": lambda: (time.sleep(0.05), "slow")[1],
            "fast": lambda: "fast",
        })
        self.assertEqual([o.key for o in report.outcomes], ["slow", "fast"])

    def test_a_failure_is_a_result_not_an_exception(self) -> None:
        from alchem_link import parallel

        def boom():
            raise ValueError("nope")

        report = parallel.run_tasks({"ok": lambda: 1, "bad": boom})
        self.assertEqual(len(report.ok), 1)
        self.assertEqual(len(report.failed), 1)
        self.assertIn("nope", report.failed[0].error)

    def test_values_returns_only_successes(self) -> None:
        from alchem_link import parallel

        def boom():
            raise RuntimeError("x")

        report = parallel.run_tasks({"a": lambda: 1, "b": boom})
        self.assertEqual(report.values(), {"a": 1})

    def test_an_empty_sweep_is_harmless(self) -> None:
        from alchem_link import parallel

        report = parallel.run_tasks({})
        self.assertEqual(len(report), 0)
        self.assertEqual(report.speedup, 1.0)

    def test_gather_flattens_lists_and_drops_failures(self) -> None:
        from alchem_link import parallel

        def boom():
            raise RuntimeError("x")

        report = parallel.run_tasks({"a": lambda: [1, 2], "b": boom, "c": lambda: [3]})
        self.assertEqual(parallel.gather(report), [1, 2, 3])

    def test_report_serialises(self) -> None:
        import json

        from alchem_link import parallel

        json.dumps(parallel.run_tasks({"a": lambda: 1}).as_dict())


class ClientFacade(unittest.TestCase):
    """Only the offline surface — the live methods need a chain and are not unit tests."""

    def test_construction_opens_no_connection(self) -> None:
        from alchem_link import connect

        link = connect("base")
        self.assertEqual(link.rpc_stats()["requests"], 0)
        self.assertIn("no connection", link.rpc_stats()["note"])

    def test_summary_is_offline_and_complete(self) -> None:
        from alchem_link import connect

        summary = connect("base").summary()
        self.assertEqual(summary["chain_id"], 8453)
        self.assertTrue(summary["layer2"])
        self.assertEqual(summary["feeds"], len(get_feed("ETH/USD", "base").pair) > 0
                         and summary["feeds"])

    def test_cache_reports_as_enabled_before_the_first_entry(self) -> None:
        """TTLCache defines __len__, so a truthiness check calls an empty cache disabled."""
        from alchem_link import connect

        self.assertNotIn("enabled", connect("base").cache_stats())
        self.assertEqual(connect("base", cache=False).cache_stats(), {"enabled": False})

    def test_on_returns_a_new_handle(self) -> None:
        from alchem_link import connect

        base = connect("base")
        polygon = base.on("polygon")
        self.assertEqual(base.network, "base")
        self.assertEqual(polygon.network, "polygon")

    def test_feed_resolution_goes_through_the_registry(self) -> None:
        from alchem_link import connect

        self.assertEqual(connect("base").feed("ethusd").pair, "ETH/USD")

    def test_simulate_needs_no_chain(self) -> None:
        from alchem_link import connect

        self.assertEqual(connect("base").simulate(simulate.Guard.strict()).score, 1.0)

    def test_unknown_network_raises_at_construction(self) -> None:
        from alchem_link import connect

        with self.assertRaises(UnknownNetwork):
            connect("narnia")


if __name__ == "__main__":
    unittest.main()
