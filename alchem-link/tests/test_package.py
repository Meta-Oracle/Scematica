"""Blueprint, integration map and capability summaries.

These used to assert against hardcoded prose. They now assert that the summaries are
*derived* — that the counts match the real tables and that every claim points at code —
because the failure mode worth catching is a summary drifting away from the package it
describes.
"""
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "src"))

from alchem_link.ccip import ROUTERS, summarize_chainlink_capabilities
from alchem_link.core import build_package_blueprint
from alchem_link.feeds import feed_count
from alchem_link.integration import build_integration_map
from alchem_link.networks import list_networks
from alchem_link.recipes import get_recipe_by_id, get_recipes
from alchem_link.sequencer import SEQUENCER_FEEDS


class PackageBlueprintTests(unittest.TestCase):
    def test_blueprint_coverage_matches_the_real_tables(self):
        """The blueprint must count the registry, not restate a number someone typed."""
        blueprint = build_package_blueprint()
        coverage = blueprint["coverage"]

        self.assertEqual(coverage["feeds"], feed_count())
        self.assertEqual(coverage["networks"], len(list_networks()))
        self.assertEqual(coverage["sequencer_uptime_feeds"], len(SEQUENCER_FEEDS))
        self.assertEqual(coverage["ccip_routers"], len(ROUTERS))

    def test_blueprint_states_the_zero_dependency_position(self):
        blueprint = build_package_blueprint()
        self.assertEqual(blueprint["dependencies"]["runtime"], "none — standard library only")
        self.assertIn("SHA3-256", blueprint["dependencies"]["note"])

    def test_blueprint_embeds_the_integration_map(self):
        self.assertEqual(build_package_blueprint()["integration_map"], build_integration_map())

    def test_integration_map_entries_all_name_code_and_commands(self):
        """Every claim must be runnable — that is the whole point of the rewrite."""
        for domain, entry in build_integration_map().items():
            with self.subTest(domain=domain):
                self.assertIn("alchemy", entry)
                self.assertIn("chainlink", entry)
                self.assertTrue(entry["composed"], f"{domain} has no composed capability")
                self.assertTrue(entry["code"], f"{domain} names no code")
                self.assertTrue(entry["commands"], f"{domain} names no command")

    def test_integration_map_covers_the_expected_domains(self):
        integration = build_integration_map()
        for domain in ("data_ingestion", "valuation", "execution", "safety", "cross_chain"):
            self.assertIn(domain, integration)


class ChainlinkCapabilityTests(unittest.TestCase):
    def test_every_capability_declares_whether_it_is_read_live(self):
        for name, entry in summarize_chainlink_capabilities().items():
            with self.subTest(capability=name):
                self.assertIsInstance(entry["verified_live"], bool)
                self.assertTrue(entry["detail"])

    def test_live_capabilities_name_the_commands_that_exercise_them(self):
        for name, entry in summarize_chainlink_capabilities().items():
            if entry["verified_live"]:
                self.assertTrue(entry["commands"], f"{name} claims live but names no command")

    def test_unread_services_are_marked_rather_than_omitted(self):
        """VRF and Automation are real Chainlink services this package does not read.

        Listing them as capabilities would overstate the toolkit; omitting them would
        leave a reader wondering. They are present and explicitly flagged.
        """
        summary = summarize_chainlink_capabilities()
        self.assertFalse(summary["vrf"]["verified_live"])
        self.assertFalse(summary["automation"]["verified_live"])
        self.assertEqual(summary["vrf"]["commands"], [])

    def test_ccip_capability_counts_the_verified_routers(self):
        detail = summarize_chainlink_capabilities()["ccip"]["detail"]
        self.assertIn(str(len(ROUTERS)), detail)


class RecipeTests(unittest.TestCase):
    def test_recipes_list_and_lookup(self):
        recipes = get_recipes()
        self.assertEqual(len(recipes), 4)
        self.assertIn("ccip-cross-chain-transfer", [r["id"] for r in recipes])

        recipe = get_recipe_by_id("oracle-backed-automation")
        self.assertIsNotNone(recipe)
        self.assertIn("steps", recipe)

    def test_recipe_unknown_id_returns_none(self):
        self.assertIsNone(get_recipe_by_id("nope"))


if __name__ == "__main__":
    unittest.main()
