from __future__ import annotations

import argparse
import json
import sys
from typing import Any, List, Optional

from . import __version__
from .alchemy import summarize_alchemy_capabilities
from .chainlink import summarize_chainlink_capabilities
from .core import build_package_blueprint
from .feeds import list_feeds, read_all_feeds, read_feed, verify_registry
from .health import diagnose
from .integration import build_integration_map
from .networks import ALCHEMY_KEY_ENV, DEFAULT_NETWORK, list_networks
from .recipes import get_recipe_by_id, get_recipes
from .rpc import RpcError, RpcTransportError, client_for, gwei

LIVE_COMMANDS = ["price", "feeds", "block", "gas", "networks", "doctor", "verify"]
REFERENCE_COMMANDS = ["blueprint", "alchemy", "chainlink", "integration", "recipes"]
ALL_COMMANDS = LIVE_COMMANDS + REFERENCE_COMMANDS + ["list"]


def _print_json(payload: Any) -> None:
    print(json.dumps(payload, indent=2, sort_keys=True, default=str))


def _fmt_price(value: float) -> str:
    """Pick a sensible precision for the magnitude.

    Naive trailing-zero stripping is wrong here: "1,900.00000000".rstrip("0") eats the
    integer digits too and yields "1,9".
    """
    magnitude = abs(value)
    if magnitude >= 1000:
        return f"{value:,.2f}"
    if magnitude >= 1:
        return f"{value:,.4f}"
    return f"{value:,.8f}"


def _fmt_age(seconds: int) -> str:
    if seconds < 60:
        return f"{seconds}s"
    if seconds < 3600:
        return f"{seconds // 60}m {seconds % 60}s"
    return f"{seconds // 3600}h {(seconds % 3600) // 60}m"


def _cmd_price(args: argparse.Namespace) -> int:
    if not args.pair:
        print("usage: alchem-link price <PAIR>   e.g. alchem-link price ETH/USD", file=sys.stderr)
        return 2
    reading = read_feed(args.pair, network=args.network, rpc_url=args.rpc_url)
    if args.json:
        _print_json(reading.as_dict())
        return 0 if not reading.stale else 1

    print(f"{reading.description}  ({reading.network})")
    print(f"  price      {_fmt_price(reading.price)}")
    print(f"  status     {reading.status}")
    print(f"  updated    {_fmt_age(reading.age_secs)} ago (heartbeat {reading.heartbeat_secs}s)")
    print(f"  round      {reading.round_id}")
    print(f"  aggregator {reading.address}")
    if reading.note:
        print(f"  note       {reading.note}")
    if reading.stale:
        print("  WARNING    last update is older than the heartbeat — do not trade on this value")
        return 1
    return 0


def _cmd_feeds(args: argparse.Namespace) -> int:
    if args.live:
        readings = read_all_feeds(network=args.network, rpc_url=args.rpc_url)
        if args.json:
            _print_json([r.as_dict() for r in readings])
            return 0
        if not readings:
            print(f"no feeds could be read on {args.network}")
            return 1
        width = max(len(r.pair) for r in readings)
        for r in readings:
            print(f"  {r.pair:<{width}}  {_fmt_price(r.price):>16}  {r.status:<6}  {_fmt_age(r.age_secs)} ago")
        return 0

    feeds = list_feeds(args.network)
    if args.json:
        _print_json([
            {
                "pair": f.pair,
                "address": f.address,
                "decimals": f.decimals,
                "heartbeat_secs": f.heartbeat_secs,
                "note": f.note,
            }
            for f in feeds
        ])
        return 0
    if not feeds:
        print(f"no feeds registered for {args.network}")
        return 1
    width = max(len(f.pair) for f in feeds)
    for f in feeds:
        suffix = f"  — {f.note}" if f.note else ""
        print(f"  {f.pair:<{width}}  {f.address}  {f.heartbeat_secs}s{suffix}")
    return 0


def _cmd_block(args: argparse.Namespace) -> int:
    rpc = client_for(network=args.network, rpc_url=args.rpc_url)
    result = rpc.call("eth_blockNumber")
    block = int(result.result, 16)
    if args.json:
        _print_json({"network": args.network, "block": block, "elapsed_ms": round(result.elapsed_ms, 1)})
    else:
        print(f"{block:,}  ({result.elapsed_ms:.0f} ms via {rpc.endpoint.source})")
    return 0


def _cmd_gas(args: argparse.Namespace) -> int:
    rpc = client_for(network=args.network, rpc_url=args.rpc_url)
    wei = rpc.gas_price_wei()
    if args.json:
        _print_json({"network": args.network, "wei": wei, "gwei": gwei(wei)})
    else:
        print(f"{gwei(wei):.3f} gwei")
    return 0


def _cmd_networks(args: argparse.Namespace) -> int:
    nets = list_networks()
    if args.json:
        _print_json([
            {
                "key": n.key,
                "label": n.label,
                "chain_id": n.chain_id,
                "native": n.native_symbol,
                "alchemy_subdomain": n.alchemy_subdomain,
                "explorer": n.explorer,
                "feeds": len(list_feeds(n.key)),
            }
            for n in nets
        ])
        return 0
    width = max(len(n.key) for n in nets)
    for n in nets:
        print(f"  {n.key:<{width}}  chain {n.chain_id:<9} {len(list_feeds(n.key))} feeds  {n.label}")
    return 0


def _cmd_doctor(args: argparse.Namespace) -> int:
    result = diagnose(network=args.network, rpc_url=args.rpc_url)
    if args.json:
        _print_json(result.as_dict())
        return 0 if result.ok else 1
    print(f"network   {result.network}")
    print(f"endpoint  {result.endpoint}  ({result.endpoint_source})")
    print("")
    for check in result.checks:
        mark = "ok  " if check.ok else "FAIL"
        print(f"  [{mark}] {check.name:<16} {check.detail}")
        if check.hint:
            print(f"           {check.hint}")
    return 0 if result.ok else 1


def _cmd_verify(args: argparse.Namespace) -> int:
    results = verify_registry(network=args.network, rpc_url=args.rpc_url)
    if args.json:
        _print_json(results)
        return 0 if all(r.get("ok") for r in results) else 1
    failures = 0
    for entry in results:
        ok = bool(entry.get("ok"))
        if not ok:
            failures += 1
        mark = "ok  " if ok else "FAIL"
        detail = entry.get("description") or entry.get("error", "")
        print(f"  [{mark}] {str(entry['pair']):<10} {entry['address']}  {detail}")
    print("")
    print(f"{len(results) - failures}/{len(results)} registry entries match their on-chain description")
    return 0 if failures == 0 else 1


def _cmd_recipes(args: argparse.Namespace) -> int:
    if args.pair:  # positional doubles as the recipe id
        recipe = get_recipe_by_id(args.pair)
        if recipe is None:
            print(f"Unknown recipe id: {args.pair}", file=sys.stderr)
            return 2
        _print_json(recipe)
        return 0
    _print_json(get_recipes())
    return 0


def _cmd_list(_args: argparse.Namespace) -> int:
    print("Live commands (talk to a chain):")
    for command in LIVE_COMMANDS:
        print(f"  {command}")
    print("")
    print("Reference commands (offline):")
    for command in REFERENCE_COMMANDS:
        print(f"  {command}")
    print("")
    print(f"Set {ALCHEMY_KEY_ENV} to use Alchemy; otherwise a keyless public endpoint is used.")
    return 0


def _cmd_blueprint(_args: argparse.Namespace) -> int:
    _print_json(build_package_blueprint())
    return 0


def _cmd_alchemy(_args: argparse.Namespace) -> int:
    _print_json(summarize_alchemy_capabilities())
    return 0


def _cmd_chainlink(_args: argparse.Namespace) -> int:
    _print_json(summarize_chainlink_capabilities())
    return 0


def _cmd_integration(_args: argparse.Namespace) -> int:
    _print_json(build_integration_map())
    return 0


HANDLERS = {
    "price": _cmd_price,
    "feeds": _cmd_feeds,
    "block": _cmd_block,
    "gas": _cmd_gas,
    "networks": _cmd_networks,
    "doctor": _cmd_doctor,
    "verify": _cmd_verify,
    "recipes": _cmd_recipes,
    "list": _cmd_list,
    "blueprint": _cmd_blueprint,
    "alchemy": _cmd_alchemy,
    "chainlink": _cmd_chainlink,
    "integration": _cmd_integration,
}


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        prog="alchem-link",
        description="Alchemy x Chainlink developer toolkit — live feed reads and integration reference",
    )
    parser.add_argument("--version", action="version", version=f"alchem-link {__version__}")
    parser.add_argument(
        "command",
        nargs="?",
        default="list",
        choices=ALL_COMMANDS,
        help="What to run (default: list)",
    )
    parser.add_argument(
        "pair",
        nargs="?",
        help="Feed pair for `price` (e.g. ETH/USD), or recipe id for `recipes`",
    )
    parser.add_argument(
        "-n", "--network",
        default=DEFAULT_NETWORK,
        help=f"Network key (default: {DEFAULT_NETWORK}). See `alchem-link networks`.",
    )
    parser.add_argument("--rpc-url", help="Override the RPC endpoint entirely")
    parser.add_argument("--json", action="store_true", help="Emit JSON instead of text")
    parser.add_argument("--live", action="store_true", help="For `feeds`: read live prices too")
    return parser


def main(argv: Optional[List[str]] = None) -> int:
    args = build_parser().parse_args(argv)
    handler = HANDLERS[args.command]
    try:
        return int(handler(args) or 0)
    except (KeyError, ValueError) as exc:
        print(f"error: {exc}", file=sys.stderr)
        return 2
    except RpcTransportError as exc:
        print(f"network error: {exc}", file=sys.stderr)
        return 3
    except RpcError as exc:
        print(f"rpc error: {exc}", file=sys.stderr)
        return 4
    except KeyboardInterrupt:
        return 130


if __name__ == "__main__":
    raise SystemExit(main())
