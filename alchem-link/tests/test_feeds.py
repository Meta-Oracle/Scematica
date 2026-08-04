"""Feed registry, decoding and staleness tests. All offline — no network required."""
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "src"))

from alchem_link.feeds import (
    DEFAULT_HEARTBEAT_SECS,
    FEEDS,
    Feed,
    decode_reading,
    get_feed,
    list_feeds,
)
from alchem_link.networks import NETWORKS

RAW_ETH_USD = {
    "latest_round_data": (
        "0x0000000000000000000000000000000000000000000000070000000000007dd0"
        "0000000000000000000000000000000000000000000000000000002bb96bfc45"
        "000000000000000000000000000000000000000000000000000000006a724e85"
        "000000000000000000000000000000000000000000000000000000006a724e97"
        "0000000000000000000000000000000000000000000000070000000000007dd0"
    ),
    "decimals": "0x0000000000000000000000000000000000000000000000000000000000000008",
    "description": (
        "0x0000000000000000000000000000000000000000000000000000000000000020"
        "0000000000000000000000000000000000000000000000000000000000000009"
        "455448202f205553440000000000000000000000000000000000000000000000"
    ),
}
UPDATED_AT = 0x6A724E97


class RegistryTests(unittest.TestCase):
    def test_every_registry_network_is_a_known_network(self):
        for network in FEEDS:
            self.assertIn(network, NETWORKS, f"{network} has feeds but no Network entry")

    def test_addresses_are_well_formed_and_unique_per_network(self):
        for network, table in FEEDS.items():
            seen = set()
            for pair, feed in table.items():
                self.assertTrue(feed.address.startswith("0x"), f"{network}:{pair}")
                self.assertEqual(len(feed.address), 42, f"{network}:{pair}")
                int(feed.address, 16)  # raises if not hex
                self.assertNotIn(feed.address.lower(), seen, f"duplicate address in {network}")
                seen.add(feed.address.lower())

    def test_registry_key_matches_feed_pair(self):
        for network, table in FEEDS.items():
            for key, feed in table.items():
                self.assertEqual(key, feed.pair, f"{network}: key {key} != pair {feed.pair}")

    def test_base_wbtc_is_not_labelled_btc(self):
        # The commonly-shared "Base BTC/USD" address reports WBTC / USD on-chain.
        # Registering it as BTC/USD would quote a wrapper that can depeg.
        self.assertNotIn("BTC/USD", FEEDS["base"])
        self.assertIn("WBTC/USD", FEEDS["base"])
        self.assertTrue(FEEDS["base"]["WBTC/USD"].note)

    def test_lookup_is_case_and_separator_insensitive(self):
        self.assertEqual(get_feed("eth/usd").pair, "ETH/USD")
        self.assertEqual(get_feed("ETH-USD").pair, "ETH/USD")

    def test_unknown_pair_lists_known_ones(self):
        with self.assertRaises(KeyError) as ctx:
            get_feed("DOGE/USD")
        self.assertIn("ETH/USD", str(ctx.exception))

    def test_unknown_network_rejected(self):
        with self.assertRaises(KeyError):
            list_feeds("not-a-network")

    def test_stablecoins_get_a_longer_heartbeat(self):
        self.assertGreater(
            FEEDS["ethereum"]["USDC/USD"].heartbeat_secs,
            FEEDS["ethereum"]["ETH/USD"].heartbeat_secs,
        )


class DecodeReadingTests(unittest.TestCase):
    def setUp(self):
        self.feed = get_feed("ETH/USD", "ethereum")

    def test_decodes_price_and_metadata(self):
        reading = decode_reading(self.feed, "ethereum", RAW_ETH_USD, now=UPDATED_AT)
        self.assertEqual(reading.description, "ETH / USD")
        self.assertEqual(reading.decimals, 8)
        self.assertAlmostEqual(reading.price, 1877.94455621, places=8)
        self.assertEqual(reading.updated_at, UPDATED_AT)

    def test_fresh_when_within_heartbeat(self):
        reading = decode_reading(self.feed, "ethereum", RAW_ETH_USD, now=UPDATED_AT + 60)
        self.assertEqual(reading.age_secs, 60)
        self.assertFalse(reading.stale)
        self.assertEqual(reading.status, "FRESH")

    def test_stale_once_past_heartbeat(self):
        beyond = UPDATED_AT + DEFAULT_HEARTBEAT_SECS + 1
        reading = decode_reading(self.feed, "ethereum", RAW_ETH_USD, now=beyond)
        self.assertTrue(reading.stale)
        self.assertEqual(reading.status, "STALE")

    def test_exactly_at_heartbeat_is_not_yet_stale(self):
        edge = UPDATED_AT + DEFAULT_HEARTBEAT_SECS
        reading = decode_reading(self.feed, "ethereum", RAW_ETH_USD, now=edge)
        self.assertFalse(reading.stale)

    def test_future_timestamp_clamps_age_to_zero(self):
        reading = decode_reading(self.feed, "ethereum", RAW_ETH_USD, now=UPDATED_AT - 500)
        self.assertEqual(reading.age_secs, 0)
        self.assertFalse(reading.stale)

    def test_non_positive_answer_is_invalid(self):
        raw = dict(RAW_ETH_USD)
        raw["latest_round_data"] = (
            "0x" + "00" * 32 + "00" * 32 + "00" * 32 + f"{UPDATED_AT:064x}" + "00" * 32
        )
        reading = decode_reading(self.feed, "ethereum", raw, now=UPDATED_AT)
        self.assertEqual(reading.answer_raw, 0)
        self.assertEqual(reading.status, "INVALID")

    def test_wrong_shape_raises_with_a_useful_message(self):
        raw = dict(RAW_ETH_USD)
        raw["latest_round_data"] = "0x" + "00" * 32
        with self.assertRaises(ValueError) as ctx:
            decode_reading(self.feed, "ethereum", raw, now=UPDATED_AT)
        self.assertIn("expected 5", str(ctx.exception))

    def test_as_dict_round_trips_the_status(self):
        payload = decode_reading(self.feed, "ethereum", RAW_ETH_USD, now=UPDATED_AT).as_dict()
        self.assertEqual(payload["status"], "FRESH")
        self.assertEqual(payload["pair"], "ETH/USD")


class FeedDataclassTests(unittest.TestCase):
    def test_default_heartbeat_applied(self):
        self.assertEqual(Feed("X/Y", "0x" + "11" * 20, 8).heartbeat_secs, DEFAULT_HEARTBEAT_SECS)


if __name__ == "__main__":
    unittest.main()
