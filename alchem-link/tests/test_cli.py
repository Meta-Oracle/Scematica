import json
import os
import subprocess
import sys
import unittest
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[1]
ENV = {**dict(os.environ), "PYTHONPATH": str(REPO_ROOT / "src")}


def _run(*args: str) -> subprocess.CompletedProcess:
    return subprocess.run(
        [sys.executable, "-m", "alchem_link.cli", *args],
        cwd=REPO_ROOT,
        capture_output=True,
        text=True,
        env=ENV,
    )


class CLITests(unittest.TestCase):
    def test_blueprint_command(self):
        result = _run("blueprint")
        self.assertEqual(result.returncode, 0)
        self.assertIn('"project"', result.stdout)
        self.assertIn("Alchemy x Chainlink Developer Package", result.stdout)

    def test_list_command(self):
        result = _run("list")
        self.assertEqual(result.returncode, 0)
        self.assertIn("blueprint", result.stdout)
        self.assertIn("integration", result.stdout)

    def test_recipes_command_all(self):
        result = _run("recipes")
        self.assertEqual(result.returncode, 0)
        self.assertIn("oracle-backed-automation", result.stdout)
        self.assertIn("ccip-cross-chain-transfer", result.stdout)

    def test_recipes_command_by_id(self):
        result = _run("recipes", "real-time-data-pipeline")
        self.assertEqual(result.returncode, 0)
        self.assertIn("real-time-data-pipeline", result.stdout)

    def test_recipes_unknown_id_exits(self):
        result = _run("recipes", "does-not-exist")
        self.assertNotEqual(result.returncode, 0)


class OfflineLiveCommandTests(unittest.TestCase):
    """Verbs that belong to the live half but need no network to answer."""

    def test_networks_lists_every_supported_chain(self):
        result = _run("networks")
        self.assertEqual(result.returncode, 0)
        for key in ("ethereum", "sepolia", "base", "arbitrum", "optimism", "polygon"):
            self.assertIn(key, result.stdout)

    def test_networks_json_carries_chain_ids(self):
        result = _run("networks", "--json")
        self.assertEqual(result.returncode, 0)
        payload = json.loads(result.stdout)
        by_key = {n["key"]: n for n in payload}
        self.assertEqual(by_key["ethereum"]["chain_id"], 1)
        self.assertEqual(by_key["polygon"]["chain_id"], 137)

    def test_feeds_registry_listing_is_offline(self):
        result = _run("feeds", "--json")
        self.assertEqual(result.returncode, 0)
        pairs = {f["pair"] for f in json.loads(result.stdout)}
        self.assertIn("ETH/USD", pairs)

    def test_feeds_on_base_expose_the_wbtc_label(self):
        result = _run("feeds", "-n", "base", "--json")
        self.assertEqual(result.returncode, 0)
        pairs = {f["pair"] for f in json.loads(result.stdout)}
        self.assertIn("WBTC/USD", pairs)
        self.assertNotIn("BTC/USD", pairs)

    def test_unknown_network_is_a_usage_error(self):
        result = _run("feeds", "-n", "not-a-chain")
        self.assertEqual(result.returncode, 2)

    def test_price_without_pair_is_a_usage_error(self):
        result = _run("price")
        self.assertEqual(result.returncode, 2)

    def test_list_separates_live_from_reference(self):
        result = _run("list")
        self.assertEqual(result.returncode, 0)
        self.assertIn("price", result.stdout)
        self.assertIn("doctor", result.stdout)
        self.assertIn("ALCHEMY_API_KEY", result.stdout)

    def test_version_flag(self):
        result = _run("--version")
        self.assertEqual(result.returncode, 0)
        self.assertIn("alchem-link", result.stdout)


if __name__ == "__main__":
    unittest.main()
