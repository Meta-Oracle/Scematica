import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "src"))

from alchem_link.alchemy import summarize_alchemy_capabilities
from alchem_link.chainlink import summarize_chainlink_capabilities
from alchem_link.core import build_package_blueprint
from alchem_link.integration import build_integration_map
from alchem_link.recipes import get_recipes


if __name__ == "__main__":
    print("Blueprint:")
    print(build_package_blueprint())
    print("\nAlchemy:")
    print(summarize_alchemy_capabilities())
    print("\nChainlink:")
    print(summarize_chainlink_capabilities())
    print("\nIntegration map:")
    print(build_integration_map())
    print("\nRecipes:")
    for r in get_recipes():
        print(f"  [{r['id']}] {r['name']} — {r['summary']}")
