from __future__ import annotations

import argparse
import json
from typing import Any

from .alchemy import summarize_alchemy_capabilities
from .chainlink import summarize_chainlink_capabilities
from .core import build_package_blueprint
from .integration import build_integration_map
from .recipes import get_recipe_by_id, get_recipes


def _print_json(payload: Any) -> None:
    print(json.dumps(payload, indent=2, sort_keys=True))


def _list_commands() -> list[str]:
    return ["blueprint", "alchemy", "chainlink", "integration", "recipes", "list"]


def main() -> None:
    parser = argparse.ArgumentParser(
        description="Alchemy x Chainlink developer package CLI",
        formatter_class=argparse.ArgumentDefaultsHelpFormatter,
    )
    parser.add_argument(
        "command",
        nargs="?",
        default="blueprint",
        choices=_list_commands(),
        help="Which package view to render",
    )
    parser.add_argument(
        "recipe_id",
        nargs="?",
        help="Optional recipe id to show when using the recipes command",
    )
    args = parser.parse_args()

    if args.command == "blueprint":
        _print_json(build_package_blueprint())
    elif args.command == "alchemy":
        _print_json(summarize_alchemy_capabilities())
    elif args.command == "chainlink":
        _print_json(summarize_chainlink_capabilities())
    elif args.command == "integration":
        _print_json(build_integration_map())
    elif args.command == "recipes":
        if args.recipe_id:
            recipe = get_recipe_by_id(args.recipe_id)
            if recipe is None:
                raise SystemExit(f"Unknown recipe id: {args.recipe_id}")
            _print_json(recipe)
        else:
            _print_json(get_recipes())
    else:
        print("Available commands:")
        for command in _list_commands():
            print(f"- {command}")


if __name__ == "__main__":
    main()
