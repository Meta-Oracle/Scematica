import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "src"))

from alchem_link.alchemy import summarize_alchemy_capabilities
from alchem_link.chainlink import summarize_chainlink_capabilities
from alchem_link.core import build_package_blueprint
from alchem_link.integration import build_integration_map
from alchem_link.recipes import get_recipe_by_id, get_recipes


class PackageBlueprintTests(unittest.TestCase):
    def test_blueprint_contains_core_domain_sections(self):
        blueprint = build_package_blueprint()

        self.assertEqual(blueprint["project"], "Alchemy x Chainlink Developer Package")
        self.assertIn("alchemy", blueprint)
        self.assertIn("chainlink", blueprint)
        self.assertIn("synergy", blueprint)
        self.assertIn("developer_package", blueprint["synergy"])

    def test_alchemy_and_chainlink_capabilities_are_structured(self):
        alchemy = summarize_alchemy_capabilities()
        chainlink = summarize_chainlink_capabilities()

        self.assertIn("rpc", alchemy)
        self.assertIn("websocket", alchemy)
        self.assertIn("price_feeds", chainlink)
        self.assertIn("vrf", chainlink)

    def test_alchemy_capabilities_include_websocket(self):
        alchemy = summarize_alchemy_capabilities()
        self.assertIn("websocket", alchemy)

    def test_chainlink_capabilities_include_ccip(self):
        chainlink = summarize_chainlink_capabilities()
        self.assertIn("ccip", chainlink)

    def test_integration_map_links_complementary_patterns(self):
        integration = build_integration_map()

        self.assertIn("data_ingestion", integration)
        self.assertIn("execution", integration)
        self.assertIn("monitoring", integration)
        self.assertIn("cross_chain", integration)
        self.assertIn("alchemy", integration["data_ingestion"])

    def test_recipes_list_and_lookup(self):
        recipes = get_recipes()
        self.assertEqual(len(recipes), 4)
        ids = [r["id"] for r in recipes]
        self.assertIn("ccip-cross-chain-transfer", ids)

        recipe = get_recipe_by_id("oracle-backed-automation")
        self.assertIsNotNone(recipe)
        self.assertIn("steps", recipe)

    def test_recipe_unknown_id_returns_none(self):
        self.assertIsNone(get_recipe_by_id("nonexistent"))


if __name__ == "__main__":
    unittest.main()
