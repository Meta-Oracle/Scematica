"""The oracle set, as a Scematica Omni ``WorldState``.

Every test here is offline. :func:`alchem_link.omni.world` is a pure transform from
readings to a dict on purpose — everything that decides what a reading *means* is testable
without an RPC endpoint, and the meaning is the part worth testing. Only
:func:`alchem_link.omni.perceive` touches a chain, and it does nothing but read and
delegate.

The recurring subject is the same one the rest of this package is about: **an unreadable
feed is not a zero, and a stale feed is not a fresh one.** A producer that blurred either
would hand an agent a confident world it had no right to.
"""
from __future__ import annotations

import json
import unittest

from alchem_link import omni
from alchem_link.feeds import FeedReading


def reading(
    pair: str = "ETH/USD",
    *,
    price: float = 3000.0,
    answer_raw: int = 300_000_000_000,
    age_secs: int = 30,
    heartbeat_secs: int = 1200,
    stale: bool = False,
    heartbeat_measured: bool = True,
    round_id: int = 42,
    answered_in_round: int = 42,
    network: str = "base",
) -> FeedReading:
    return FeedReading(
        pair=pair,
        network=network,
        address="0x" + "11" * 20,
        description=pair,
        price=price,
        answer_raw=answer_raw,
        decimals=8,
        round_id=round_id,
        updated_at=1_700_000_000,
        age_secs=age_secs,
        heartbeat_secs=heartbeat_secs,
        stale=stale,
        answered_in_round=answered_in_round,
        heartbeat_measured=heartbeat_measured,
    )


class WorldShape(unittest.TestCase):
    """The wire contract omni's ``ImportObserver`` enforces on the far side."""

    def test_a_clean_read_produces_a_valid_world_with_no_signals(self):
        w = omni.world([reading()], network="base", now=1_700_000_100)
        self.assertEqual(w["observer"], "alchem-link")
        self.assertEqual(w["entity"]["locator"], "chainlink:base")
        self.assertEqual(w["signals"], [])
        self.assertEqual(w["blind_spots"], [])
        self.assertEqual(w["observed_at"], 1_700_000_100)

    def test_the_world_is_json_serialisable_without_special_casing(self):
        # It goes down a pipe as JSON and nothing else. A float that ``json.dumps`` emits as
        # a bare ``NaN`` is valid Python and invalid JSON, and the consumer would report a
        # parse failure that reads like a broken producer.
        w = omni.world([reading()], network="base")
        text = json.dumps(w)
        self.assertNotIn("NaN", text)
        self.assertNotIn("Infinity", text)
        json.loads(text)

    def test_the_domain_is_named_so_a_specialist_can_decline(self):
        # ``Domain`` exists so a domain-specific evaluator can decline rather than pretend.
        # The bot's Deep Q* net reads pool and position data; asked about an oracle set it
        # would still emit five finite Q-values, correctly shaped and entirely meaningless.
        #
        # This asserted ``unknown`` while the vocabulary was closed, which was the weaker
        # claim: a web page and a set of price feeds both reported ``unknown``, so a
        # specialist could tell it was not being handed a market but not what it *was*
        # being handed. What matters is that the domain is named and is not ``trading``.
        w = omni.world([reading()], network="base")
        self.assertEqual(w["domain"], "data")
        self.assertNotEqual(w["domain"], "trading")

    def test_scalars_use_the_externally_tagged_form(self):
        w = omni.world([reading()], network="base")
        attrs = w["objects"][0]["attrs"]
        for key, value in attrs.items():
            self.assertIn(value["t"], ("int", "num", "text", "bool"), key)
            self.assertIn("v", value)


class ProvenanceBeforeValue(unittest.TestCase):
    """Can this be believed, asked before the number is reported."""

    def test_a_fresh_feed_is_live_and_a_stale_one_is_stale(self):
        fresh = omni.world([reading(stale=False, age_secs=30)], network="base")
        self.assertEqual(fresh["objects"][0]["provenance"]["kind"], "live")

        old = omni.world(
            [reading(stale=True, age_secs=9_000, heartbeat_secs=1_200)], network="base"
        )
        prov = old["objects"][0]["provenance"]
        self.assertEqual(prov["kind"], "stale")
        self.assertEqual(prov["age_secs"], 9_000)
        self.assertEqual(prov["budget_secs"], 1_200)

    def test_a_stale_feed_still_carries_its_price(self):
        # Stale means "was true once", not "unknown". Dropping the value would be as wrong
        # as presenting it as current — the reader needs both the number and the warning.
        w = omni.world([reading(stale=True, price=2999.5)], network="base")
        self.assertAlmostEqual(w["objects"][0]["attrs"]["price"]["v"], 2999.5)

    def test_an_unreadable_feed_is_a_blind_spot_and_not_an_object(self):
        # The rule the whole toolkit is built on. A feed that did not answer has no price,
        # no age and no verdict; rendering it as a price of zero is the same error as
        # rendering an unreadable vault balance as zero.
        w = omni.world([reading()], network="base", unreadable=["BTC/USD"])
        ids = [o["id"] for o in w["objects"]]
        self.assertNotIn("feed:BTC/USD", ids)
        self.assertTrue(any("BTC/USD" in b for b in w["blind_spots"]), w["blind_spots"])

    def test_an_unmeasured_heartbeat_survives_into_the_object(self):
        # ``heartbeat_measured: False`` marks a conservative bound rather than a
        # measurement. An agent told a heartbeat is 3600 when nobody measured it will call a
        # feed fresh that its publisher considers late.
        w = omni.world([reading(heartbeat_measured=False)], network="base")
        self.assertIs(w["objects"][0]["attrs"]["heartbeat_measured"]["v"], False)


class SignalsAreCounts(unittest.TestCase):
    """``measured: true`` is a claim that somebody counted something."""

    def _by_id(self, w, ident):
        for s in w["signals"]:
            if s["id"] == ident:
                return s
        return None

    def test_every_signal_is_measured_and_cites_its_count(self):
        # The property ``scema-sim`` depends on to score a real expected gain. A signal
        # claiming to be counted with nothing to cite is a guess wearing a measurement's
        # clothes, and nothing downstream could tell.
        w = omni.world(
            [
                reading("ETH/USD", stale=True, age_secs=9_000),
                reading("BTC/USD", answer_raw=0, price=0.0),
                reading("LINK/USD", round_id=9, answered_in_round=7),
                reading("DAI/USD", heartbeat_measured=False),
            ],
            network="base",
            unreadable=["USDC/USD"],
        )
        self.assertGreaterEqual(len(w["signals"]), 5)
        for s in w["signals"]:
            self.assertTrue(s["measured"], s["id"])
            self.assertTrue(s["evidence"], s["id"])
            self.assertIn("counted", s["evidence"][0], s["id"])

    def test_a_stale_feed_is_counted_against_its_own_heartbeat(self):
        w = omni.world(
            [reading("ETH/USD", stale=True, age_secs=9_000, heartbeat_secs=1_200)],
            network="base",
        )
        s = self._by_id(w, "stale-feeds")
        self.assertIsNotNone(s)
        self.assertIn("9000s > 1200s", s["evidence"][0])

    def test_a_non_positive_answer_is_the_loudest_thing_here(self):
        # A Chainlink price feed never legitimately reports zero or below. Treating it as a
        # low price is how a liquidation cascade starts, so it takes the top of the scale
        # regardless of how many feeds are involved.
        w = omni.world([reading(answer_raw=0, price=0.0)], network="base")
        s = self._by_id(w, "non-positive-answers")
        self.assertIsNotNone(s)
        self.assertEqual(s["magnitude"], 1.0)

    def test_an_unmeasured_heartbeat_is_counted_apart_from_staleness(self):
        # Two different problems with two different fixes: one is a feed to stop using, the
        # other is a table entry to go and measure. Collapsing them would send the reader
        # to the wrong one.
        w = omni.world([reading(heartbeat_measured=False, stale=False)], network="base")
        self.assertIsNotNone(self._by_id(w, "unmeasured-heartbeats"))
        self.assertIsNone(self._by_id(w, "stale-feeds"))

    def test_a_carried_over_answer_is_counted(self):
        w = omni.world([reading(round_id=9, answered_in_round=7)], network="base")
        s = self._by_id(w, "carried-over-answers")
        self.assertIsNotNone(s)
        self.assertIn("answeredInRound < roundId", s["evidence"][0])

    def test_no_signal_estimates_a_severity(self):
        # Nothing here invents an "oracle health score". Every magnitude is a share of a
        # counted population or a flat 1.0 for a single definite fact, and the evidence line
        # is what makes that checkable.
        w = omni.world(
            [reading("ETH/USD", stale=True), reading("BTC/USD")],
            network="base",
        )
        s = self._by_id(w, "stale-feeds")
        self.assertGreater(s["magnitude"], 0.0)
        self.assertLessEqual(s["magnitude"], 1.0)

    def test_magnitudes_stay_inside_the_unit_interval(self):
        # An out-of-range magnitude would dominate a ranking through arithmetic rather than
        # through importance, and omni's importer refuses one outright. Two hundred stale
        # readings is more than any registry holds, which also exercises the unbounded-extent
        # path below.
        many = [reading(f"P{i}/USD", stale=True) for i in range(200)]
        w = omni.world(many, network="base")
        for s in w["signals"]:
            self.assertGreaterEqual(s["magnitude"], 0.0, s["id"])
            self.assertLessEqual(s["magnitude"], 1.0, s["id"])

    def test_signal_ids_are_unique(self):
        # Two signals with one id would rank as two independent supports for one thing, and
        # ``--ground`` could not name either unambiguously.
        w = omni.world(
            [
                reading("ETH/USD", stale=True),
                reading("BTC/USD", stale=True, heartbeat_measured=False),
                reading("LINK/USD", answer_raw=-1, price=-1.0),
            ],
            network="base",
            unreadable=["USDC/USD"],
        )
        ids = [s["id"] for s in w["signals"]]
        self.assertEqual(len(ids), len(set(ids)), ids)


class Extent(unittest.TestCase):
    def test_the_denominator_is_the_registry_because_the_registry_is_a_fixed_table(self):
        # One of the few observers in this project that can honestly claim a bounded extent.
        # An unnecessary ``null`` manufactures uncertainty the same way a missing one
        # manufactures confidence.
        w = omni.world([reading()], network="base")
        self.assertIsNotNone(w["extent"]["total"])
        self.assertGreaterEqual(w["extent"]["total"], w["extent"]["observed"])

    def test_unread_feeds_lower_the_numerator_and_not_the_denominator(self):
        w = omni.world([reading()], network="base", unreadable=["BTC/USD", "LINK/USD"])
        self.assertEqual(w["extent"]["observed"], 1)
        self.assertGreater(w["extent"]["total"], 1)

    def test_a_read_beyond_the_registry_reports_an_unknown_denominator(self):
        # An ad-hoc set or an `--address` override describes something the registry does not
        # bound. Reporting `observed` over a smaller `total` would claim more than 100%
        # coverage; `None` is what "the denominator is unknown" means, and `scema-sim` turns
        # it into measured uncertainty rather than confidence.
        many = [reading(f"P{i}/USD") for i in range(500)]
        w = omni.world(many, network="base")
        self.assertIsNone(w["extent"]["total"])
        self.assertIn("beyond the registry", w["extent"]["note"])


class Sequencer(unittest.TestCase):
    """An L2's uptime feed: down, in grace, or unreadable — three states, not two."""

    def test_a_down_sequencer_is_the_loudest_signal(self):
        w = omni.world(
            [reading()], network="base", sequencer={"readable": True, "up": False}
        )
        s = [x for x in w["signals"] if x["id"] == "sequencer-down"]
        self.assertTrue(s)
        self.assertEqual(s[0]["magnitude"], 1.0)

    def test_a_grace_period_is_a_signal_rather_than_a_pass(self):
        # Inside the grace window a feed can read fresh while the value behind it was
        # produced before the outage. "Up" is not the same as "safe to consume".
        w = omni.world(
            [reading()],
            network="base",
            sequencer={"readable": True, "up": True, "grace_remaining_secs": 900},
        )
        self.assertTrue(any(x["id"] == "sequencer-grace-period" for x in w["signals"]))

    def test_an_unreadable_sequencer_is_a_blind_spot_and_never_a_pass(self):
        # Whether L2 prices are safe is *unknown*, which is a different claim from "fine".
        w = omni.world(
            [reading()], network="base", sequencer={"readable": False, "detail": "rpc down"}
        )
        self.assertTrue(
            any("sequencer" in b for b in w["blind_spots"]), w["blind_spots"]
        )
        self.assertFalse(any(x["id"].startswith("sequencer") for x in w["signals"]))

    def test_a_healthy_sequencer_produces_nothing(self):
        w = omni.world(
            [reading()],
            network="base",
            sequencer={"readable": True, "up": True, "grace_remaining_secs": 0},
        )
        self.assertFalse(any(x["id"].startswith("sequencer") for x in w["signals"]))

    def test_no_sequencer_facts_from_a_status_of_none(self):
        # ``None`` in, ``None`` out — and that is not the same as "up". On an L1 there is no
        # sequencer; on an L2 with no registered feed the risk was never checked.
        self.assertIsNone(omni.sequencer_facts(None))


class SequencerFacts(unittest.TestCase):
    class _Status:
        def __init__(self, up=True, since=10_000, grace=3_600, error=""):
            self.up = up
            self.since_secs = since
            self.grace_period_secs = grace
            self.error = error

        @property
        def in_grace_period(self):
            return self.up and self.since_secs <= self.grace_period_secs

    def test_an_errored_status_is_unreadable_rather_than_down(self):
        facts = omni.sequencer_facts(self._Status(error="rpc refused"))
        self.assertIs(facts["readable"], False)
        self.assertNotIn("up", facts)

    def test_grace_remaining_is_computed_from_the_window(self):
        facts = omni.sequencer_facts(self._Status(up=True, since=600, grace=3_600))
        self.assertEqual(facts["grace_remaining_secs"], 3_000)

    def test_a_long_uptime_reports_no_grace_remaining(self):
        facts = omni.sequencer_facts(self._Status(up=True, since=90_000, grace=3_600))
        self.assertEqual(facts["grace_remaining_secs"], 0)


class SelfValidation(unittest.TestCase):
    """The checks omni's ``ImportObserver`` applies, restated on this side.

    This package fails its own tests rather than producing something the consumer rejects
    at run time — which is the only thing keeping two hand-written implementations of one
    wire format honest.
    """

    def test_a_counted_signal_with_no_evidence_is_refused(self):
        bad = {
            # Declared, because `_check` refuses an undeclared world before it looks at
            # anything else. Without this the test passes for the wrong reason: it sees a
            # ValueError, which is all `assertRaises` asks for, and never reaches the
            # evidence rule it is named after.
            "schema": omni.WORLD_SCHEMA,
            "observer": "x",
            "entity": {"kind": "service", "locator": "l", "label": "x"},
            "domain": "unknown",
            "observed_at": 0,
            "objects": [],
            "facts": [],
            "signals": [
                {
                    "id": "a",
                    "polarity": "risk",
                    "label": "x",
                    "detail": "",
                    "magnitude": 0.5,
                    "measured": True,
                    "targets": [],
                    "evidence": [],
                }
            ],
            "extent": {"observed": 0, "total": 0, "note": ""},
            "blind_spots": [],
        }
        with self.assertRaises(ValueError) as ctx:
            omni._check(bad)
        self.assertIn("cites nothing", str(ctx.exception))

    def test_an_empty_locator_is_refused(self):
        bad = {
            "observer": "x",
            "entity": {"kind": "service", "locator": "   ", "label": "x"},
            "domain": "unknown",
            "observed_at": 0,
            "objects": [],
            "facts": [],
            "signals": [],
            "extent": {"observed": 0, "total": 0, "note": ""},
            "blind_spots": [],
        }
        with self.assertRaises(ValueError):
            omni._check(bad)

    def test_an_extent_numerator_over_its_denominator_is_refused(self):
        bad = {
            "observer": "x",
            "entity": {"kind": "service", "locator": "l", "label": "x"},
            "domain": "unknown",
            "observed_at": 0,
            "objects": [],
            "facts": [],
            "signals": [],
            "extent": {"observed": 9, "total": 3, "note": ""},
            "blind_spots": [],
        }
        with self.assertRaises(ValueError):
            omni._check(bad)

    def test_world_validates_itself_on_the_way_out(self):
        # Not an optional extra pass: :func:`world` calls ``_check`` before returning, so a
        # producer bug becomes a traceback here rather than a rejection three processes away.
        self.assertIsNotNone(omni.world([reading()], network="base"))


class Truncation(unittest.TestCase):
    def test_a_truncated_blind_spot_list_says_how_many_it_dropped(self):
        # A silently truncated list is a wrong count, and the count is the whole point.
        many = [f"P{i}/USD" for i in range(omni.MAX_BLIND_SPOTS + 5)]
        w = omni.world([reading()], network="base", unreadable=many)
        self.assertTrue(
            any("further unreadable" in b for b in w["blind_spots"]), w["blind_spots"]
        )


if __name__ == "__main__":
    unittest.main()
