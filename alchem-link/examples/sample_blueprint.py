"""Reference tour: the offline integration material.

For the live half — reading real Chainlink feeds — see `live_feeds.py`.
"""
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "src"))

from alchem_link import (
    build_integration_map,
    build_package_blueprint,
    get_recipes,
    summarize_alchemy_capabilities,
    summarize_chainlink_capabilities,
)


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
