"""The windowed oracle world.

Every case here asserts something does *not* happen. A window of price history is the most
fabricable thing this package produces — the statistics are all real numbers of a plausible
shape, and a volatility invented from three points is indistinguishable from one measured
over three hundred unless something refuses to emit it.

Offline by construction: :func:`windowed_world` takes histories and returns a dict. The
reading lives in ``perceive_window`` and is not exercised here.
"""
from __future__ import annotations

import unittest

from alchem_link.analytics import Point, Series
from alchem_link.feeds import list_feeds
from alchem_link.omni import (
    DRAWDOWN_PCT,
    GAP_FACTOR,
    MAX_BLIND_SPOTS,
    MIN_SAMPLES,
    TWAP_DIVERGENCE_BPS,
    WORLD_SCHEMA,
    windowed_world,
)

NETWORK = "base"
NOW = 1_800_000_000


def _feed_pairs(n: int = 3):
    return [f.pair for f in list_feeds(NETWORK)][:n]


def series(pair: str, prices, step: int = 60, end: int = NOW) -> Series:
    """A history ending at ``end``, one point every ``step`` seconds."""
    points = [
        Point(timestamp=end - step * (len(prices) - 1 - i), price=p)
        for i, p in enumerate(prices)
    ]
    return Series(pair=pair, network=NETWORK, points=points)


def signals(state):
    return {s["id"]: s for s in state["signals"]}


class WindowedWorldShape(unittest.TestCase):
    def test_it_declares_the_contract_version_and_passes_its_own_check(self):
        pair = _feed_pairs(1)[0]
        state = windowed_world([series(pair, [1.0] * 20)], network=NETWORK, now=NOW)
        self.assertEqual(state["schema"], WORLD_SCHEMA)
        self.assertEqual(state["domain"], "data")
        self.assertEqual(state["entity"]["kind"], "service")

    def test_it_shares_the_snapshot_worlds_locator(self):
        # One subject observed two ways. A separate locator would split a network's memory
        # in half, and an agent would never connect "this feed was stale then" with "this
        # feed gaps".
        state = windowed_world([], network=NETWORK, now=NOW)
        self.assertEqual(state["entity"]["locator"], f"chainlink:{NETWORK}")

    def test_object_ids_match_the_snapshot_worlds(self):
        pair = _feed_pairs(1)[0]
        state = windowed_world([series(pair, [1.0] * 20)], network=NETWORK, now=NOW)
        self.assertEqual(state["objects"][0]["id"], f"feed:{pair}")


class UnmeasuredStatisticsAreAbsent(unittest.TestCase):
    def test_a_single_point_has_no_volatility_attribute_rather_than_zero(self):
        # A volatility of 0.0 is a claim the price did not move. One print has no
        # volatility at all, and the two must not read alike.
        pair = _feed_pairs(1)[0]
        state = windowed_world([series(pair, [100.0])], network=NETWORK, now=NOW)
        attrs = state["objects"][0]["attrs"]
        self.assertNotIn("volatility_annual", attrs)
        self.assertNotIn("volatility_period", attrs)
        self.assertNotIn("median_interval_secs", attrs)

    def test_a_moving_series_does_carry_one(self):
        pair = _feed_pairs(1)[0]
        state = windowed_world(
            [series(pair, [100.0, 101.0, 99.0, 102.0, 98.0])], network=NETWORK, now=NOW
        )
        attrs = state["objects"][0]["attrs"]
        self.assertIn("volatility_period", attrs)
        self.assertIn("median_interval_secs", attrs)

    def test_a_flat_series_reports_a_measured_zero(self):
        # The other half of the rule. A price that genuinely did not move has a volatility,
        # and it is zero — that is an observation and it must be present.
        pair = _feed_pairs(1)[0]
        state = windowed_world([series(pair, [100.0] * 10)], network=NETWORK, now=NOW)
        attrs = state["objects"][0]["attrs"]
        self.assertIn("volatility_period", attrs)
        self.assertEqual(attrs["volatility_period"]["v"], 0.0)


class EmptyHistoryIsIgnoranceNotAnObservation(unittest.TestCase):
    def test_a_feed_with_no_points_is_a_blind_spot_not_an_object(self):
        pair = _feed_pairs(1)[0]
        state = windowed_world([series(pair, [])], network=NETWORK, now=NOW)
        self.assertEqual(state["objects"], [])
        self.assertIn(pair, " ".join(state["blind_spots"]))

    def test_a_feed_nobody_mentioned_is_still_missing(self):
        # Computed by difference against the registry, never taken from the caller. An
        # observer that reported only what it was told about would silently shrink the
        # world every time a caller passed a short list — the same trap `perceive` names.
        registered = [f.pair for f in list_feeds(NETWORK)]
        self.assertGreater(len(registered), 1, "this test needs a multi-feed network")
        one = registered[0]
        state = windowed_world([series(one, [100.0] * 12)], network=NETWORK, now=NOW)
        # Parsed rather than substring-matched: `ETH/USD` occurs inside `CBETH/USD`, so a
        # naive `assertIn` passes for a feed that was never named.
        named = {s.split(" on ")[0] for s in state["blind_spots"]}
        self.assertEqual(named, set(registered[1:]))
        self.assertNotIn(one, named, "the feed that did answer is not a blind spot")

    def test_the_two_reasons_a_feed_is_missing_are_named_separately(self):
        # "The node would not answer" and "the feed did not publish" are different claims,
        # and only the second is a fact about the oracle.
        a, b = _feed_pairs(2)
        state = windowed_world(
            [series(a, [])], network=NETWORK, unreadable=[b], now=NOW
        )
        spots = {s.split(" on ")[0]: s for s in state["blind_spots"]}
        self.assertIn("published nothing", spots[a])
        self.assertIn("node did not answer", spots[b])

    def test_it_does_not_pad_the_extent_numerator(self):
        pair = _feed_pairs(1)[0]
        state = windowed_world([series(pair, [])], network=NETWORK, now=NOW)
        self.assertEqual(state["extent"]["observed"], 0)
        self.assertGreater(state["extent"]["total"], 0)

    def test_the_blind_spot_list_states_its_own_truncation(self):
        # A silently shortened list is a wrong count, and the count is what sim reads.
        many = [f"FAKE{i}/USD" for i in range(MAX_BLIND_SPOTS + 5)]
        state = windowed_world([], network=NETWORK, unreadable=many, now=NOW)
        self.assertLessEqual(len(state["blind_spots"]), MAX_BLIND_SPOTS + 1)
        self.assertIn("further feed(s) without history not listed", state["blind_spots"][-1])

    def test_unreadable_and_empty_are_both_counted_but_named_differently(self):
        # "The node would not answer" and "the feed did not publish" are different claims.
        # Both are ignorance and both belong in blind spots; neither is a zero.
        a, b = _feed_pairs(2)
        state = windowed_world(
            [series(a, [])], network=NETWORK, unreadable=[b], now=NOW
        )
        self.assertEqual(signals(state)["feeds-without-history"]["magnitude"] > 0, True)
        joined = " ".join(state["blind_spots"])
        self.assertIn(a, joined)
        self.assertIn(b, joined)


class SignalsAreCounts(unittest.TestCase):
    def test_every_signal_claiming_measured_cites_evidence(self):
        # `_check` enforces it, but the point of the rule is that it is checked on real
        # output rather than trusted.
        a, b = _feed_pairs(2)
        state = windowed_world(
            [series(a, [100.0, 130.0, 88.0, 95.0] ), series(b, [])],
            network=NETWORK,
            now=NOW,
        )
        for s in state["signals"]:
            self.assertTrue(s["measured"])
            self.assertTrue(s["evidence"], f"{s['id']} claims measured and cites nothing")

    def test_a_thin_history_is_flagged_and_its_statistics_are_still_emitted(self):
        # Hiding them would be this module deciding on the agent's behalf. Flagging them
        # lets the agent discount numbers it can still see.
        pair = _feed_pairs(1)[0]
        state = windowed_world(
            [series(pair, [100.0, 101.0, 102.0])], network=NETWORK, now=NOW
        )
        self.assertIn("thin-history", signals(state))
        self.assertIn("median_interval_secs", state["objects"][0]["attrs"])

    def test_a_full_history_is_not_flagged_as_thin(self):
        pair = _feed_pairs(1)[0]
        prices = [100.0 + i for i in range(MIN_SAMPLES + 2)]
        state = windowed_world([series(pair, prices)], network=NETWORK, now=NOW)
        self.assertNotIn("thin-history", signals(state))

    def test_a_feed_publishing_slower_than_its_heartbeat_is_counted(self):
        feed = list_feeds(NETWORK)[0]
        step = int(feed.heartbeat_secs * GAP_FACTOR) + 60
        state = windowed_world(
            [series(feed.pair, [100.0] * 12, step=step)], network=NETWORK, now=NOW
        )
        self.assertIn("publish-gaps", signals(state))

    def test_a_feed_publishing_on_cadence_is_not(self):
        feed = list_feeds(NETWORK)[0]
        step = max(1, int(feed.heartbeat_secs / 4))
        state = windowed_world(
            [series(feed.pair, [100.0] * 12, step=step)], network=NETWORK, now=NOW
        )
        self.assertNotIn("publish-gaps", signals(state))

    def test_a_spot_answer_far_from_its_own_twap_is_counted(self):
        pair = _feed_pairs(1)[0]
        prices = [100.0] * 20 + [100.0 * (1 + TWAP_DIVERGENCE_BPS / 10_000 * 5)]
        state = windowed_world([series(pair, prices)], network=NETWORK, now=NOW)
        self.assertIn("twap-divergence", signals(state))

    def test_a_drawdown_is_counted_and_named_as_a_price_move(self):
        # It is a risk to a position, not a fault in the oracle, and conflating the two
        # would have an agent distrust a feed that is working perfectly.
        pair = _feed_pairs(1)[0]
        prices = [100.0, 100.0, 100.0 * (1 - DRAWDOWN_PCT / 100 - 0.05), 90.0]
        state = windowed_world([series(pair, prices)], network=NETWORK, now=NOW)
        s = signals(state)["window-drawdown"]
        self.assertIn("not to the oracle", s["detail"])

    def test_steady_is_an_opportunity_that_claims_behaviour_not_correctness(self):
        feed = list_feeds(NETWORK)[0]
        step = max(1, int(feed.heartbeat_secs / 4))
        state = windowed_world(
            [series(feed.pair, [100.0] * 20, step=step)], network=NETWORK, now=NOW
        )
        s = signals(state)["steady-feeds"]
        self.assertEqual(s["polarity"], "opportunity")
        self.assertIn("not that they are correct", s["detail"])

    def test_a_troubled_feed_is_never_also_steady(self):
        feed = list_feeds(NETWORK)[0]
        state = windowed_world(
            [series(feed.pair, [100.0, 101.0, 102.0])], network=NETWORK, now=NOW
        )
        got = signals(state)
        self.assertIn("thin-history", got)
        self.assertNotIn("steady-feeds", got)


class ProvenanceAtTheWindowEnd(unittest.TestCase):
    def test_a_history_ending_long_ago_is_stale_and_keeps_its_value(self):
        # The history is still perfectly good evidence about the span it covers. Dropping
        # it would lose a measurement; presenting it as current would be a lie.
        feed = list_feeds(NETWORK)[0]
        end = NOW - feed.heartbeat_secs * 10
        state = windowed_world(
            [series(feed.pair, [100.0] * 10, end=end)], network=NETWORK, now=NOW
        )
        prov = state["objects"][0]["provenance"]
        self.assertEqual(prov["kind"], "stale")
        self.assertGreater(prov["age_secs"], feed.heartbeat_secs)
        self.assertIn("last", state["objects"][0]["attrs"])

    def test_a_history_ending_now_is_live(self):
        feed = list_feeds(NETWORK)[0]
        state = windowed_world(
            [series(feed.pair, [100.0] * 10, step=5)], network=NETWORK, now=NOW
        )
        self.assertEqual(state["objects"][0]["provenance"]["kind"], "live")


class TruncationIsStated(unittest.TestCase):
    def test_a_capped_scan_is_a_signal_not_a_silent_short_window(self):
        # The statistics are real and describe less time than the rest. Comparing them like
        # for like overstates the shorter window's stability.
        pair = _feed_pairs(1)[0]
        state = windowed_world(
            [series(pair, [100.0] * 12)], network=NETWORK, truncated=[pair], now=NOW
        )
        self.assertIn("truncated-window", signals(state))

    def test_an_untruncated_scan_says_nothing(self):
        pair = _feed_pairs(1)[0]
        state = windowed_world([series(pair, [100.0] * 12)], network=NETWORK, now=NOW)
        self.assertNotIn("truncated-window", signals(state))


class Determinism(unittest.TestCase):
    def test_the_same_histories_produce_the_same_world(self):
        # `now` is a parameter for exactly this reason: a world that read the clock could
        # not be compared with itself, and every downstream commitment would differ.
        pair = _feed_pairs(1)[0]
        args = ([series(pair, [100.0, 103.0, 99.0])],)
        a = windowed_world(*args, network=NETWORK, now=NOW)
        b = windowed_world(*args, network=NETWORK, now=NOW)
        self.assertEqual(a, b)

    def test_an_empty_world_is_valid_and_says_nothing_happened(self):
        state = windowed_world([], network=NETWORK, now=NOW)
        self.assertEqual(state["objects"], [])
        self.assertEqual(state["extent"]["observed"], 0)
        # Every registered feed is missing, so that is the one thing it does report.
        self.assertIn("feeds-without-history", signals(state))


if __name__ == "__main__":
    unittest.main()
