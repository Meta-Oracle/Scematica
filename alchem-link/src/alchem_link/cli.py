"""Command line interface.

Every command takes ``-n/--network``, ``--rpc-url`` and ``--json``, and returns an exit
code that means something (see the table in the README) so these compose into CI gates
rather than needing their output parsed.

One Windows detail is handled up front: this package prints em-dashes and box glyphs,
and a Windows console defaulting to cp1252 raises ``UnicodeEncodeError`` on them mid-line,
turning a diagnostic into a crash. :func:`_configure_output` reconfigures the streams to
UTF-8 with replacement. Given the toolkit's whole purpose is telling you when something
is wrong, it must not fall over while doing so.
"""
from __future__ import annotations

import argparse
import json
import sys
from typing import Any, List, Optional

from . import __version__
from .aggregator import describe_aggregator, round_history, split_round_id
from .cadence import profile_feed
from .ccip import ROUTERS, summarize_chainlink_capabilities, verify_lanes
from .codegen import LANGUAGES, generate_consumer
from .divergence import common_pairs, compare_all, compare_pair
from .enhanced import (
    NeedsAlchemyKey,
    get_asset_transfers,
    summarize_alchemy_capabilities,
    value_holdings,
)
from .feeds import feed_count, list_feeds, read_all_feeds, read_feed, verify_registry
from .gas import GAS_SWAP, GAS_TRANSFER, analyse_gas
from .health import diagnose
from .integration import build_integration_map, build_package_blueprint
from .networks import ALCHEMY_KEY_ENV, DEFAULT_NETWORK, get_network, list_networks
from .recipes import get_recipe_by_id, get_recipes
from .rpc import RpcError, RpcTransportError, client_for, gwei
from .safety import audit_feed, audit_network
from .sequencer import SEQUENCER_FEEDS, is_l2, read_sequencer
from .watch import watch_feed

EXIT_OK = 0
EXIT_UNUSABLE = 1
EXIT_USAGE = 2
EXIT_NETWORK = 3
EXIT_RPC = 4

_SEVERITY_MARK = {
    "critical": "CRIT",
    "high": "HIGH",
    "medium": "MED ",
    "low": "LOW ",
    "info": "info",
}


def _configure_output() -> None:
    """Make stdout/stderr survive non-ASCII on a legacy Windows codepage."""
    for stream in (sys.stdout, sys.stderr):
        reconfigure = getattr(stream, "reconfigure", None)
        if reconfigure is not None:
            try:
                reconfigure(encoding="utf-8", errors="replace")
            except (ValueError, OSError):  # pragma: no cover - stream already closed//piped oddly
                pass


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
    if seconds < 86400:
        return f"{seconds // 3600}h {(seconds % 3600) // 60}m"
    return f"{seconds // 86400}d {(seconds % 86400) // 3600}h"


def _fmt_secs(seconds: int) -> str:
    if not seconds:
        return "?"
    if seconds % 86400 == 0:
        return f"{seconds // 86400}d"
    if seconds % 3600 == 0:
        return f"{seconds // 3600}h"
    if seconds % 60 == 0:
        return f"{seconds // 60}m"
    return f"{seconds}s"


def _client(args: argparse.Namespace):
    return client_for(network=args.network, rpc_url=args.rpc_url)


# ── live commands ────────────────────────────────────────────────────────────────


def _cmd_price(args: argparse.Namespace) -> int:
    reading = read_feed(args.pair, network=args.network, rpc_url=args.rpc_url)
    if args.json:
        _print_json(reading.as_dict())
        return EXIT_UNUSABLE if reading.stale else EXIT_OK

    print(f"{reading.description}  ({reading.network})")
    print(f"  price      {_fmt_price(reading.price)}")
    print(f"  status     {reading.status}")
    bound = "" if reading.heartbeat_measured else " (bound, not measured)"
    print(
        f"  updated    {_fmt_age(reading.age_secs)} ago "
        f"(heartbeat {_fmt_secs(reading.heartbeat_secs)}{bound})"
    )
    print(f"  round      {reading.round_id}  (phase {split_round_id(reading.round_id)[0]})")
    print(f"  aggregator {reading.address}")
    if reading.note:
        print(f"  note       {reading.note}")
    if reading.carried_over:
        print("  WARNING    this round carried an older answer forward (answeredInRound < roundId)")
    if reading.stale:
        print("  WARNING    last update is older than the heartbeat — do not trade on this value")
        return EXIT_UNUSABLE
    return EXIT_OK


def _cmd_feeds(args: argparse.Namespace) -> int:
    if args.live:
        readings = read_all_feeds(network=args.network, rpc_url=args.rpc_url)
        if args.json:
            _print_json([r.as_dict() for r in readings])
            return EXIT_OK
        if not readings:
            print(f"no feeds could be read on {args.network}")
            return EXIT_UNUSABLE
        width = max(len(r.pair) for r in readings)
        for r in readings:
            print(
                f"  {r.pair:<{width}}  {_fmt_price(r.price):>16}  {r.status:<6}  "
                f"{_fmt_age(r.age_secs)} ago"
            )
        stale = sum(1 for r in readings if r.stale)
        print(f"\n  {len(readings)} feeds · {len(readings) - stale} fresh · {stale} past heartbeat")
        return EXIT_UNUSABLE if stale else EXIT_OK

    feeds = list_feeds(args.network)
    if args.json:
        _print_json([
            {
                "pair": f.pair,
                "address": f.address,
                "decimals": f.decimals,
                "heartbeat_secs": f.heartbeat_secs,
                "heartbeat_measured": f.heartbeat_measured,
                "note": f.note,
            }
            for f in feeds
        ])
        return EXIT_OK
    if not feeds:
        print(f"no feeds registered for {args.network}")
        return EXIT_UNUSABLE
    width = max(len(f.pair) for f in feeds)
    for f in feeds:
        suffix = f"  — {f.note}" if f.note else ""
        mark = "" if f.heartbeat_measured else "*"
        print(f"  {f.pair:<{width}}  {f.address}  {_fmt_secs(f.heartbeat_secs)}{mark}{suffix}")
    if any(not f.heartbeat_measured for f in feeds):
        print("\n  * heartbeat is a conservative bound — no quiet period was observed in sampling")
    return EXIT_OK


def _cmd_block(args: argparse.Namespace) -> int:
    rpc = _client(args)
    result = rpc.call("eth_blockNumber")
    block = int(result.result, 16)
    if args.json:
        _print_json({
            "network": args.network,
            "block": block,
            "elapsed_ms": round(result.elapsed_ms, 1),
        })
    else:
        print(f"{block:,}  ({result.elapsed_ms:.0f} ms via {rpc.endpoint.source})")
    return EXIT_OK


def _cmd_gas(args: argparse.Namespace) -> int:
    report = analyse_gas(network=args.network, rpc_url=args.rpc_url, blocks=args.blocks)
    if args.json:
        _print_json(report.as_dict())
        return EXIT_OK

    print(f"{report.network}  ({report.blocks_sampled} blocks sampled)")
    print(f"  base fee   {gwei(report.base_fee_wei):.4f} gwei  →  next block "
          f"{gwei(report.next_base_fee_wei):.4f} gwei [{report.trend}]")
    print(f"  congestion {report.congestion * 100:.0f}% of target")
    if report.native_usd:
        stale = " (STALE)" if report.native_price_stale else ""
        print(f"  {report.native_symbol}/USD    {_fmt_price(report.native_usd)}{stale}")
    elif report.price_error:
        print(f"  {report.native_symbol}/USD    unavailable — {report.price_error}")
    print("")
    header = f"  {'tier':<10} {'tip (gwei)':>12} {'max (gwei)':>12}"
    if report.native_usd:
        header += f" {'transfer':>11} {'swap':>11}"
    print(header)
    for tier in report.tiers:
        line = (
            f"  {tier.label:<10} {gwei(tier.priority_fee_wei):>12.4f} "
            f"{gwei(tier.max_fee_wei):>12.4f}"
        )
        if report.native_usd:
            line += (
                f" {'$' + format(tier.cost_wei(GAS_TRANSFER) / 1e18 * report.native_usd, '.4f'):>11}"
                f" {'$' + format(tier.cost_wei(GAS_SWAP) / 1e18 * report.native_usd, '.4f'):>11}"
            )
        print(line)
    return EXIT_OK


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
                "testnet": n.testnet,
                "layer2": n.layer2,
                "sequencer_feed": SEQUENCER_FEEDS.get(n.key, ""),
                "ccip_router": ROUTERS.get(n.key, ""),
                "feeds": len(list_feeds(n.key)),
            }
            for n in nets
        ])
        return EXIT_OK
    width = max(len(n.key) for n in nets)
    for n in nets:
        tags = []
        if n.testnet:
            tags.append("testnet")
        if n.layer2:
            tags.append("L2")
        if n.key in SEQUENCER_FEEDS:
            tags.append("uptime-feed")
        if n.key in ROUTERS:
            tags.append("ccip")
        suffix = f"  [{', '.join(tags)}]" if tags else ""
        print(
            f"  {n.key:<{width}}  chain {n.chain_id:<9} {len(list_feeds(n.key)):>2} feeds  "
            f"{n.label}{suffix}"
        )
    print(f"\n  {feed_count()} feeds across {len(nets)} networks")
    return EXIT_OK


def _cmd_doctor(args: argparse.Namespace) -> int:
    result = diagnose(network=args.network, rpc_url=args.rpc_url)
    if args.json:
        _print_json(result.as_dict())
        return EXIT_OK if result.ok else EXIT_UNUSABLE
    print(f"network   {result.network}")
    print(f"endpoint  {result.endpoint}  ({result.endpoint_source})")
    print("")
    for check in result.checks:
        mark = "ok  " if check.ok else "FAIL"
        print(f"  [{mark}] {check.name:<18} {check.detail}")
        if check.hint:
            print(f"           {check.hint}")
    return EXIT_OK if result.ok else EXIT_UNUSABLE


def _cmd_verify(args: argparse.Namespace) -> int:
    results = verify_registry(network=args.network, rpc_url=args.rpc_url)
    if args.json:
        _print_json(results)
        return EXIT_OK if all(r.get("ok") for r in results) else EXIT_UNUSABLE
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
    return EXIT_OK if failures == 0 else EXIT_UNUSABLE


def _cmd_audit(args: argparse.Namespace) -> int:
    rpc = _client(args)
    if args.pair:
        audits = [audit_feed(args.pair, network=args.network, client=rpc, address=args.address)]
    else:
        audits = audit_network(network=args.network, client=rpc)

    if args.json:
        _print_json([a.as_dict() for a in audits])
        return EXIT_OK if all(a.safe_to_consume for a in audits) else EXIT_UNUSABLE

    unsafe = 0
    for audit in audits:
        if not audit.safe_to_consume:
            unsafe += 1
        headline = audit.description or audit.pair
        price = f"  {_fmt_price(audit.price)}" if audit.price is not None else ""
        print(f"\n{headline}  ({audit.network}){price}   [{audit.worst}]")
        print(f"  {audit.address}")
        if not audit.findings:
            print("  no findings")
        for finding in audit.sorted_findings:
            print(f"  [{_SEVERITY_MARK[finding.severity]}] {finding.code}: {finding.title}")
            print(f"         {finding.detail}")
            if finding.remedy:
                print(f"         fix: {finding.remedy}")
    print(f"\n{len(audits) - unsafe}/{len(audits)} feed(s) safe to consume")
    return EXIT_OK if unsafe == 0 else EXIT_UNUSABLE


def _cmd_inspect(args: argparse.Namespace) -> int:
    rpc = _client(args)
    address = args.address
    if not address:
        if not args.pair:
            print("usage: alchem-link inspect <PAIR> | --address 0x...", file=sys.stderr)
            return EXIT_USAGE
        from .feeds import get_feed
        address = get_feed(args.pair, args.network).address

    info = describe_aggregator(address, network=args.network, client=rpc)
    if args.json:
        _print_json(info.as_dict())
        return EXIT_OK

    print(f"{info.description or '(no description)'}  ({info.network})")
    print(f"  proxy          {info.address}")
    print(f"  implementation {info.implementation or '(this address is the aggregator)'}")
    print(f"  type           {info.type_and_version or 'unknown'}")
    print(f"  version        {info.version}   phase {info.phase_id}")
    print(f"  decimals       {info.decimals}")
    print(f"  owner          {info.owner or 'n/a'}")
    if info.latest:
        latest = info.latest
        carried = "  (carried an older answer forward)" if latest.carried_over else ""
        print(f"  latest         {_fmt_price(latest.price)}  round {latest.aggregator_round} "
              f"of phase {latest.phase_id}{carried}")
    if info.min_answer is not None or info.max_answer is not None:
        print(f"  bounds         min {info.min_price:,.8g}  max {info.max_price:,.6g}")
        floor = info.floor_headroom
        ceiling = info.ceiling_headroom
        if floor is not None and ceiling is not None:
            print(f"  headroom       {floor:.3g}x to the floor, {ceiling:.3g}x to the ceiling"
                  f"   [{'BINDING' if info.bounds_are_binding else 'not binding'}]")
    for note in info.notes:
        print(f"  note           {note}")
    return EXIT_OK


def _cmd_history(args: argparse.Namespace) -> int:
    from .feeds import get_feed

    rpc = _client(args)
    address = args.address or get_feed(args.pair, args.network).address
    rounds = round_history(address, count=args.rounds, network=args.network, client=rpc)
    if args.json:
        _print_json([r.as_dict() for r in rounds])
        return EXIT_OK
    if not rounds:
        print("no rounds could be read")
        return EXIT_UNUSABLE
    print(f"  {'round':>10}  {'price':>16}  {'interval':>10}  {'change':>9}")
    ordered = sorted(rounds, key=lambda r: -r.updated_at)
    for index, entry in enumerate(ordered):
        if index + 1 < len(ordered):
            previous = ordered[index + 1]
            gap = f"{entry.updated_at - previous.updated_at}s"
            change = (
                f"{(entry.price - previous.price) / previous.price * 100:+.3f}%"
                if previous.price else "—"
            )
        else:
            gap, change = "—", "—"
        flag = "  carried" if entry.carried_over else ""
        print(f"  {entry.aggregator_round:>10}  {_fmt_price(entry.price):>16}  {gap:>10}  {change:>9}{flag}")
    return EXIT_OK


def _cmd_cadence(args: argparse.Namespace) -> int:
    rpc = _client(args)
    if args.pair:
        profiles = [profile_feed(args.pair, args.network, rounds=args.rounds, client=rpc)]
    else:
        profiles = [
            profile_feed(feed.pair, args.network, rounds=args.rounds, client=rpc)
            for feed in list_feeds(args.network)
        ]

    if args.json:
        _print_json([p.as_dict() for p in profiles])
        return EXIT_OK

    mismatched = 0
    for profile in profiles:
        verdict = profile.heartbeat_verdict
        if verdict in ("declared too tight", "declared too loose"):
            mismatched += 1
        observed = _fmt_secs(profile.observed_heartbeat) if profile.observed_heartbeat else "—"
        print(f"\n{profile.pair}  ({profile.network})   [{verdict}]")
        print(f"  declared   {_fmt_secs(profile.declared_heartbeat)}")
        print(f"  observed   {observed}   (ceiling {profile.observed_ceiling_secs}s, "
              f"median {profile.median_interval}s over {profile.samples} intervals)")
        print(f"  triggers   {profile.heartbeat_triggered} by heartbeat, "
              f"{profile.deviation_triggered} by deviation")
        if profile.inferred_deviation_pct is not None:
            print(f"  deviation  threshold is at most {profile.inferred_deviation_pct}% "
                  f"(largest move seen {profile.largest_move_pct:.3f}%)")
        print(f"  {profile.verdict_detail}")
    if len(profiles) > 1:
        print(f"\n{mismatched}/{len(profiles)} declared heartbeat(s) disagree with measurement")
    return EXIT_UNUSABLE if mismatched else EXIT_OK


def _cmd_divergence(args: argparse.Namespace) -> int:
    if args.pair:
        reports = [compare_pair(args.pair, outlier_bps=args.threshold)]
    else:
        reports = compare_all(outlier_bps=args.threshold)

    if args.json:
        _print_json([r.as_dict() for r in reports])
        return EXIT_OK if all(r.verdict != "diverged" for r in reports) else EXIT_UNUSABLE

    diverged = 0
    for report in reports:
        if report.verdict == "diverged":
            diverged += 1
        consensus = f"consensus {_fmt_price(report.consensus)}" if report.consensus else "no consensus"
        print(f"\n{report.pair}   [{report.verdict}]   {consensus}")
        for leg in sorted(report.legs, key=lambda leg: -abs(leg.deviation_bps)):
            if leg.error:
                print(f"  {leg.network:<10}  unreadable — {leg.error[:60]}")
                continue
            mark = "STALE" if leg.stale else "     "
            print(f"  {leg.network:<10}  {_fmt_price(leg.price):>16}  "
                  f"{leg.deviation_bps:+8.1f} bps  {mark}  {_fmt_age(leg.age_secs)} old")
        print(f"  {report.detail}")
    if len(reports) > 1:
        print(f"\n{diverged}/{len(reports)} pair(s) diverged beyond {args.threshold:.0f} bps")
    return EXIT_UNUSABLE if diverged else EXIT_OK


def _cmd_sequencer(args: argparse.Namespace) -> int:
    targets = [args.network] if args.network != DEFAULT_NETWORK else sorted(SEQUENCER_FEEDS)
    if args.network in SEQUENCER_FEEDS:
        targets = [args.network]

    statuses = []
    for network in targets:
        status = read_sequencer(network)
        if status is not None:
            statuses.append(status)

    if args.json:
        _print_json([s.as_dict() for s in statuses])
        return EXIT_OK if all(s.ok for s in statuses) else EXIT_UNUSABLE

    if not statuses:
        net = get_network(args.network)
        if is_l2(net.key):
            print(f"{net.key} is a rollup but no uptime feed is registered — "
                  "price reads there are unguarded")
            return EXIT_UNUSABLE
        print(f"{net.key} is not a rollup; no sequencer uptime feed applies")
        return EXIT_OK

    bad = 0
    for status in statuses:
        if not status.ok:
            bad += 1
        print(f"  [{status.state:<7}] {status.network:<10} {status.address}")
        print(f"             {status.detail}")
    return EXIT_UNUSABLE if bad else EXIT_OK


def _cmd_generate(args: argparse.Namespace) -> int:
    result = generate_consumer(args.pair, network=args.network, language=args.lang)
    if args.json:
        _print_json(result.as_dict())
        return EXIT_OK
    print(result.code)
    return EXIT_OK


def _cmd_ccip(args: argparse.Namespace) -> int:
    if args.network not in ROUTERS:
        print(f"no verified CCIP router for {args.network}. "
              f"Known: {', '.join(sorted(ROUTERS))}", file=sys.stderr)
        return EXIT_USAGE
    lanes = verify_lanes(args.network, rpc_url=args.rpc_url)
    if args.json:
        _print_json([lane.as_dict() for lane in lanes])
        return EXIT_OK
    print(f"router {ROUTERS[args.network]}  ({args.network})")
    for lane in lanes:
        if lane.error:
            state = f"error — {lane.error[:50]}"
        else:
            state = "open" if lane.supported else "closed"
        print(f"  {lane.destination:<12} selector {lane.destination_selector:<22} {state}")
    return EXIT_OK


def _cmd_holdings(args: argparse.Namespace) -> int:
    if not args.address:
        print("usage: alchem-link holdings --address 0x...", file=sys.stderr)
        return EXIT_USAGE
    holdings = value_holdings(
        args.address,
        tokens=args.tokens.split(",") if args.tokens else None,
        network=args.network,
        rpc_url=args.rpc_url,
    )
    if args.json:
        _print_json(holdings.as_dict())
        return EXIT_OK

    print(f"{holdings.address}  ({holdings.network})   via {holdings.discovered_via}")
    if holdings.native_value_usd is not None:
        print(f"  {holdings.native_symbol:<8} {holdings.native_balance:>18,.6f}  "
              f"${holdings.native_value_usd:,.2f}")
    for token in holdings.tokens:
        if not token.raw_balance:
            continue
        value = f"${token.value_usd:,.2f}" if token.value_usd is not None else "unpriced"
        print(f"  {token.symbol:<8} {token.balance:>18,.6f}  {value}")
    print(f"\n  {holdings.coverage}")
    print(f"  total (priced positions only): ${holdings.total_usd:,.2f}")
    for note in holdings.notes:
        print(f"  note: {note}")
    return EXIT_OK


def _cmd_transfers(args: argparse.Namespace) -> int:
    if not args.address:
        print("usage: alchem-link transfers --address 0x...", file=sys.stderr)
        return EXIT_USAGE
    transfers = get_asset_transfers(
        network=args.network,
        from_address=args.address if args.direction == "out" else None,
        to_address=args.address if args.direction == "in" else None,
        max_count=args.limit,
        rpc_url=args.rpc_url,
    )
    if args.json:
        _print_json(transfers)
        return EXIT_OK
    if not transfers:
        print("no transfers found")
        return EXIT_OK
    for entry in transfers:
        asset = entry.get("asset") or "?"
        value = entry.get("value")
        stamp = (entry.get("metadata") or {}).get("blockTimestamp", "")
        print(f"  {stamp:<22} {str(value):>18} {asset:<8} "
              f"{entry.get('from', '')[:10]}… → {entry.get('to', '') or ''}"[:100])
    return EXIT_OK


def _cmd_watch(args: argparse.Namespace) -> int:
    try:
        for event in watch_feed(
            args.pair,
            network=args.network,
            rpc_url=args.rpc_url,
            interval=args.interval,
            max_events=args.limit,
            duration=args.duration,
        ):
            print(event.to_json(), flush=True)
    except KeyboardInterrupt:
        return 130
    return EXIT_OK


def _cmd_alchemy(args: argparse.Namespace) -> int:
    summary = summarize_alchemy_capabilities(network=args.network)
    if args.json:
        _print_json(summary)
        return EXIT_OK
    print(f"endpoint  {summary['endpoint']}  ({summary['source']})")
    print(f"auth      {'yes' if summary['authenticated'] else 'no'}")
    print("")
    for feature in summary["features"]:
        mark = "ok  " if feature["available"] else "n/a "
        print(f"  [{mark}] {feature['method']:<32} {feature['capability']}")
    if summary["hint"]:
        print(f"\n  {summary['hint']}")
    return EXIT_OK


def _cmd_chainlink(args: argparse.Namespace) -> int:
    summary = summarize_chainlink_capabilities()
    if args.json:
        _print_json(summary)
        return EXIT_OK
    for name, entry in summary.items():
        mark = "live" if entry["verified_live"] else "  — "
        print(f"  [{mark}] {name}")
        print(f"         {entry['detail']}")
        if entry["commands"]:
            print(f"         commands: {', '.join(entry['commands'])}")
    return EXIT_OK


def _cmd_integration(args: argparse.Namespace) -> int:
    _print_json(build_integration_map())
    return EXIT_OK


def _cmd_blueprint(args: argparse.Namespace) -> int:
    _print_json(build_package_blueprint())
    return EXIT_OK


def _cmd_recipes(args: argparse.Namespace) -> int:
    if args.pair:
        recipe = get_recipe_by_id(args.pair)
        if recipe is None:
            print(f"Unknown recipe id: {args.pair}", file=sys.stderr)
            return EXIT_USAGE
        _print_json(recipe)
        return EXIT_OK
    _print_json(get_recipes())
    return EXIT_OK


LIVE_COMMANDS = [
    ("price", "Read one feed, with a staleness verdict"),
    ("feeds", "List registered feeds (--live to read them)"),
    ("audit", "Run every oracle-consumer safety check"),
    ("inspect", "Resolve the proxy and read its bounds and type"),
    ("history", "Walk a feed's round history"),
    ("cadence", "Measure the real heartbeat and deviation threshold"),
    ("divergence", "Compare one pair across every chain that carries it"),
    ("sequencer", "L2 sequencer uptime, with the grace period applied"),
    ("watch", "Stream new rounds as JSON Lines"),
    ("gas", "EIP-1559 fee tiers, priced in USD"),
    ("holdings", "Value an address's tokens through Chainlink"),
    ("transfers", "Transfer history (needs an Alchemy key)"),
    ("block", "Current block height and latency"),
    ("networks", "Supported networks and what each carries"),
    ("doctor", "End-to-end readiness check"),
    ("verify", "Confirm each address reports the pair it is filed under"),
    ("ccip", "CCIP routers, chain selectors and live lane status"),
]

REFERENCE_COMMANDS = [
    ("generate", "Emit a consumer contract with all the checks wired in"),
    ("alchemy", "What the current endpoint can actually do"),
    ("chainlink", "Chainlink services, and which this toolkit reads live"),
    ("integration", "Cross-system integration map"),
    ("blueprint", "Package blueprint"),
    ("recipes", "Developer recipes"),
]

HANDLERS = {
    "price": _cmd_price,
    "feeds": _cmd_feeds,
    "audit": _cmd_audit,
    "inspect": _cmd_inspect,
    "history": _cmd_history,
    "cadence": _cmd_cadence,
    "divergence": _cmd_divergence,
    "sequencer": _cmd_sequencer,
    "watch": _cmd_watch,
    "gas": _cmd_gas,
    "holdings": _cmd_holdings,
    "transfers": _cmd_transfers,
    "block": _cmd_block,
    "networks": _cmd_networks,
    "doctor": _cmd_doctor,
    "verify": _cmd_verify,
    "ccip": _cmd_ccip,
    "generate": _cmd_generate,
    "alchemy": _cmd_alchemy,
    "chainlink": _cmd_chainlink,
    "integration": _cmd_integration,
    "blueprint": _cmd_blueprint,
    "recipes": _cmd_recipes,
}


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        prog="alchem-link",
        description=(
            "Alchemy x Chainlink developer toolkit — live oracle reads, consumer-safety "
            "auditing, and the integration reference"
        ),
    )
    parser.add_argument("--version", action="version", version=f"alchem-link {__version__}")

    subparsers = parser.add_subparsers(dest="command", metavar="<command>")

    def add(name: str, help_text: str) -> argparse.ArgumentParser:
        sub = subparsers.add_parser(name, help=help_text, description=help_text)
        sub.add_argument(
            "-n", "--network",
            default=DEFAULT_NETWORK,
            help=f"Network key (default: {DEFAULT_NETWORK}). See `alchem-link networks`.",
        )
        sub.add_argument("--rpc-url", help="Override the RPC endpoint entirely")
        sub.add_argument("--json", action="store_true", help="Emit JSON instead of text")
        return sub

    def add_pair(sub: argparse.ArgumentParser, required: bool = True, help_text: str = "") -> None:
        sub.add_argument(
            "pair",
            nargs=None if required else "?",
            help=help_text or "Feed pair, e.g. ETH/USD",
        )

    for name, help_text in LIVE_COMMANDS + REFERENCE_COMMANDS:
        sub = add(name, help_text)

        if name in ("price", "watch"):
            add_pair(sub)
        elif name in ("audit", "inspect", "history", "cadence", "divergence", "recipes"):
            add_pair(sub, required=False, help_text=(
                "Recipe id" if name == "recipes" else "Feed pair (default: every feed)"
            ))

        if name in ("audit", "inspect", "history"):
            sub.add_argument("--address", help="Audit an arbitrary aggregator address")
        if name in ("holdings", "transfers"):
            sub.add_argument("--address", help="Account address to inspect")
        if name == "holdings":
            sub.add_argument("--tokens", help="Comma-separated ERC-20 addresses (no key needed)")
        if name == "transfers":
            sub.add_argument("--direction", choices=["in", "out"], default="out")
            sub.add_argument("--limit", type=int, default=25)
        if name in ("history", "cadence"):
            sub.add_argument("--rounds", type=int, default=30, help="Rounds of history to walk")
        if name == "divergence":
            sub.add_argument("--threshold", type=float, default=50.0,
                             help="Outlier threshold in basis points (default: 50)")
        if name == "feeds":
            sub.add_argument("--live", action="store_true", help="Read live prices too")
        if name == "gas":
            sub.add_argument("--blocks", type=int, default=20, help="Blocks of fee history")
        if name == "watch":
            sub.add_argument("--interval", type=float, help="Poll interval (default: from heartbeat)")
            sub.add_argument("--limit", type=int, help="Stop after this many events")
            sub.add_argument("--duration", type=float, help="Stop after this many seconds")
        if name == "generate":
            add_pair(sub)
            sub.add_argument("--lang", choices=LANGUAGES, default="solidity")

    return parser


def _print_overview() -> int:
    print("Live commands (talk to a chain):")
    for name, help_text in LIVE_COMMANDS:
        print(f"  {name:<12} {help_text}")
    print("\nReference and codegen:")
    for name, help_text in REFERENCE_COMMANDS:
        print(f"  {name:<12} {help_text}")
    print(f"\n{feed_count()} feeds across {len(list_networks())} networks.")
    print(f"Set {ALCHEMY_KEY_ENV} to use Alchemy; otherwise a keyless public endpoint is used.")
    print("\nRun `alchem-link <command> --help` for options.")
    return EXIT_OK


def main(argv: Optional[List[str]] = None) -> int:
    _configure_output()
    parser = build_parser()
    args = parser.parse_args(argv)

    if not args.command:
        return _print_overview()

    handler = HANDLERS[args.command]
    try:
        return int(handler(args) or EXIT_OK)
    except NeedsAlchemyKey as exc:
        print(f"error: {exc}", file=sys.stderr)
        return EXIT_USAGE
    except (KeyError, ValueError) as exc:
        print(f"error: {exc}", file=sys.stderr)
        return EXIT_USAGE
    except RpcTransportError as exc:
        print(f"network error: {exc}", file=sys.stderr)
        return EXIT_NETWORK
    except RpcError as exc:
        print(f"rpc error: {exc}", file=sys.stderr)
        return EXIT_RPC
    except BrokenPipeError:  # pragma: no cover - `| head` closes the pipe
        return EXIT_OK
    except KeyboardInterrupt:
        return 130


if __name__ == "__main__":
    raise SystemExit(main())
