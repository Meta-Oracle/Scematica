"""Command line interface.

Every command takes ``-n/--network``, ``--rpc-url`` and ``--format``, and returns an exit
code that means something (see the table in the README) so these compose into CI gates
rather than needing their output parsed.

Two structural notes.

**Output goes through :class:`~alchem_link.render.Console`, not ``print``.** That is what
makes plain command output the same black-and-blue as the dashboard, and it is why
:func:`main` calls :func:`alchem_link.term.boot.initialize` before doing anything else —
including inside a frozen binary, where a fresh console reports no ``TERM`` at all and
would otherwise be the one place the product does not look like itself. Colour is always
a decoration: ``NO_COLOR``, a pipe, or a terminal that admits to no colour each reduce
this to plain text with the layout intact.

**``--format`` is one lookup, not a branch per command.** Anything with a list of result
objects can be emitted as JSON, NDJSON, CSV, Markdown or a Prometheus scrape body,
because every result object in the package exposes ``as_dict()``. ``--json`` remains as
shorthand for ``--format json``.
"""
from __future__ import annotations

import argparse
import sys
from pathlib import Path
from typing import Any, List, Optional, Sequence

from . import __version__
from .aggregator import describe_aggregator, round_history, split_round_id
from .approvals import AutoApprover, TrustPolicy, default_approver
from .analytics import Series, summarise
from .cadence import profile_feed
from .ccip import ROUTERS, summarize_chainlink_capabilities, verify_lanes
from .codegen import (
    FRAMEWORKS,
    LANGUAGES,
    basket_pairs,
    generate_basket,
    generate_consumer,
    generate_project,
)
from .divergence import compare_all, compare_pair
from .enhanced import (
    NeedsAlchemyKey,
    get_asset_transfers,
    summarize_alchemy_capabilities,
    value_holdings,
)
from .errors import AlchemLinkError
from .exporters import FORMATS, export
from .feeds import feed_count, list_feeds, read_all_feeds, read_feed
from .gas import GAS_SWAP, GAS_TRANSFER, analyse_gas
from .health import diagnose
from .integration import build_integration_map, build_package_blueprint
from .logs import answer_updates
from .networks import ALCHEMY_KEY_ENV, DEFAULT_NETWORK, get_network, list_networks
from .recipes import get_recipe_by_id, get_recipes
from .registry import coverage, describe_feed, find, resolve, suggest
from .render import Console, console, fmt_age, fmt_price, fmt_secs, reset_console
from .rpc import RpcError, RpcTransportError, client_for, gwei
from .safety import audit_feed, audit_network
from .sequencer import SEQUENCER_FEEDS, is_l2, read_sequencer
from .simulate import SCENARIOS, Guard, audit_guard, run_scenario
from .term import boot
from .theme import PALETTE, Style, role
from .watch import watch_feed

EXIT_OK = 0
EXIT_UNUSABLE = 1
EXIT_USAGE = 2
EXIT_NETWORK = 3
EXIT_RPC = 4


def _out(args: argparse.Namespace) -> Console:
    return console()


def _fmt(args: argparse.Namespace) -> str:
    """The requested output format. ``--json`` is shorthand for ``--format json``."""
    if getattr(args, "json", False):
        return "json"
    return getattr(args, "format", "text") or "text"


def _structured(args: argparse.Namespace, items: Sequence[Any]) -> bool:
    """Emit ``items`` in the requested machine format. True when it handled the output.

    Returning a bool rather than raising keeps every command's shape the same: ask this
    first, and fall through to the human rendering when it says no.
    """
    fmt = _fmt(args)
    if fmt == "text":
        return False
    _out(args).write(export(list(items), fmt))
    return True


def _client(args: argparse.Namespace):
    return client_for(network=args.network, rpc_url=args.rpc_url)


# ── live commands ────────────────────────────────────────────────────────────────


def _cmd_price(args: argparse.Namespace) -> int:
    reading = read_feed(resolve(args.pair, args.network), network=args.network,
                        rpc_url=args.rpc_url)
    if _structured(args, [reading]):
        return EXIT_UNUSABLE if reading.stale else EXIT_OK

    out = _out(args)
    out.heading(f"{reading.description}", f"({reading.network})")
    bound = "" if reading.heartbeat_measured else " (bound, not measured)"
    out.kv("price", fmt_price(reading.price))
    out.kv("status", reading.status,
           value_style={"FRESH": "ok", "STALE": "warn", "INVALID": "bad"}[reading.status])
    out.kv("updated", f"{fmt_age(reading.age_secs)} ago "
                      f"(heartbeat {fmt_secs(reading.heartbeat_secs)}{bound})")
    out.kv("round", f"{reading.round_id}  (phase {split_round_id(reading.round_id)[0]})")
    out.kv("aggregator", reading.address)
    if reading.note:
        out.kv("note", reading.note, value_style="warn")
    if reading.carried_over:
        out.warn("this round carried an older answer forward (answeredInRound < roundId)")
    if reading.stale:
        out.error("last update is older than the heartbeat — do not trade on this value")
        return EXIT_UNUSABLE
    return EXIT_OK


def _cmd_feeds(args: argparse.Namespace) -> int:
    out = _out(args)
    if args.live:
        readings = read_all_feeds(network=args.network, rpc_url=args.rpc_url)
        if _structured(args, readings):
            return EXIT_OK
        if not readings:
            out.warn(f"no feeds could be read on {args.network}")
            return EXIT_UNUSABLE
        out.table(
            ["pair", "price", "status", "age"],
            [[r.pair, fmt_price(r.price), r.status, f"{fmt_age(r.age_secs)} ago"]
             for r in readings],
            aligns=["left", "right", "left", "right"],
            styles=["key", "number", None, "muted"],
            row_styles=[None if r.status == "FRESH" else "warn" for r in readings],
        )
        stale = sum(1 for r in readings if r.stale)
        out.blank()
        out.line(f"{len(readings)} feeds · {len(readings) - stale} fresh · "
                 f"{stale} past heartbeat", "muted")
        return EXIT_UNUSABLE if stale else EXIT_OK

    feeds = list_feeds(args.network)
    if _structured(args, [
        {
            "pair": f.pair, "address": f.address, "decimals": f.decimals,
            "heartbeat_secs": f.heartbeat_secs, "heartbeat_measured": f.heartbeat_measured,
            "note": f.note,
        }
        for f in feeds
    ]):
        return EXIT_OK
    if not feeds:
        out.warn(f"no feeds registered for {args.network}")
        return EXIT_UNUSABLE
    out.table(
        ["pair", "address", "heartbeat", "note"],
        [[f.pair, f.address, fmt_secs(f.heartbeat_secs) + ("" if f.heartbeat_measured else "*"),
          f.note] for f in feeds],
        styles=["key", "muted", "number", "hint"],
    )
    if any(not f.heartbeat_measured for f in feeds):
        out.blank()
        out.note("* heartbeat is a conservative bound — no quiet period was observed "
                 "in sampling")
    return EXIT_OK


def _cmd_search(args: argparse.Namespace) -> int:
    """Search the registry across every chain. Offline — a table lookup, not a read."""
    out = _out(args)
    everywhere = args.all or not args.pair
    results = find(args.pair or "", network=None if everywhere else args.network,
                   asset=args.asset)
    if _structured(args, [r.as_dict() for r in results]):
        return EXIT_OK
    if not results:
        hint = suggest(args.pair or "")
        out.warn(f"nothing matches {args.pair or args.asset!r}")
        if hint:
            out.note("did you mean: " + ", ".join(hint))
        return EXIT_UNUSABLE
    out.table(
        ["pair", "network", "address", "heartbeat"],
        [[r.pair, r.network, r.address,
          fmt_secs(r.feed.heartbeat_secs) + ("" if r.feed.heartbeat_measured else "*")]
         for r in results],
        styles=["key", "accent", "muted", "number"],
    )
    out.blank()
    out.line(f"{len(results)} feed(s)", "muted")
    return EXIT_OK


def _cmd_coverage(args: argparse.Namespace) -> int:
    out = _out(args)
    table = coverage()
    if _structured(args, [{"network": k, **v} for k, v in table.items()]):
        return EXIT_OK
    rows = []
    for name, entry in table.items():
        if not entry.get("feeds"):
            rows.append([name, "0", "", "", "", ""])
            continue
        tags = [t for t, on in (("testnet", entry.get("testnet")),
                                ("L2", entry.get("layer2"))) if on]
        rows.append([
            name, str(entry["feeds"]), str(entry["measured"]), str(entry["bounded"]),
            f"{fmt_secs(int(entry['fastest_secs']))}–{fmt_secs(int(entry['slowest_secs']))}",
            ", ".join(tags),
        ])
    out.table(["network", "feeds", "measured", "bounded", "cadence", "tags"], rows,
              aligns=["left", "right", "right", "right", "right", "left"],
              styles=["key", "number", "ok", "warn", "number", "muted"])
    out.blank()
    out.note("a bounded heartbeat is a conservative upper limit, not a measurement — "
             "its staleness verdict fires later than a measured one would")
    return EXIT_OK


def _cmd_block(args: argparse.Namespace) -> int:
    rpc = _client(args)
    result = rpc.call("eth_blockNumber")
    block = int(result.result, 16)
    payload = {"network": args.network, "block": block,
               "elapsed_ms": round(result.elapsed_ms, 1)}
    if _structured(args, [payload]):
        return EXIT_OK
    _out(args).line(f"{block:,}  ({result.elapsed_ms:.0f} ms via {rpc.endpoint.source})")
    return EXIT_OK


def _cmd_gas(args: argparse.Namespace) -> int:
    report = analyse_gas(network=args.network, rpc_url=args.rpc_url, blocks=args.blocks)
    if _structured(args, [report]):
        return EXIT_OK

    out = _out(args)
    out.heading(report.network, f"({report.blocks_sampled} blocks sampled)")
    out.kv("base fee", f"{gwei(report.base_fee_wei):.4f} gwei  →  next block "
                       f"{gwei(report.next_base_fee_wei):.4f} gwei [{report.trend}]")
    out.kv("congestion", f"{report.congestion * 100:.0f}% of target")
    if report.native_usd:
        stale = " (STALE)" if report.native_price_stale else ""
        out.kv(f"{report.native_symbol}/USD", f"{fmt_price(report.native_usd)}{stale}")
    elif report.price_error:
        out.kv(f"{report.native_symbol}/USD", f"unavailable — {report.price_error}",
               value_style="warn")
    out.blank()

    columns = ["tier", "tip (gwei)", "max (gwei)"]
    aligns = ["left", "right", "right"]
    if report.native_usd:
        columns += ["transfer", "swap"]
        aligns += ["right", "right"]
    rows = []
    for tier in report.tiers:
        row = [tier.label, f"{gwei(tier.priority_fee_wei):.4f}",
               f"{gwei(tier.max_fee_wei):.4f}"]
        if report.native_usd:
            row += [
                f"${tier.cost_wei(GAS_TRANSFER) / 1e18 * report.native_usd:.4f}",
                f"${tier.cost_wei(GAS_SWAP) / 1e18 * report.native_usd:.4f}",
            ]
        rows.append(row)
    out.table(columns, rows, aligns=aligns, styles=["key", "number", "number",
                                                    "number", "number"])
    return EXIT_OK


def _cmd_networks(args: argparse.Namespace) -> int:
    nets = list_networks()
    if _structured(args, [
        {
            "key": n.key, "label": n.label, "chain_id": n.chain_id,
            "native": n.native_symbol, "alchemy_subdomain": n.alchemy_subdomain,
            "explorer": n.explorer, "testnet": n.testnet, "layer2": n.layer2,
            "sequencer_feed": SEQUENCER_FEEDS.get(n.key, ""),
            "ccip_router": ROUTERS.get(n.key, ""), "feeds": len(list_feeds(n.key)),
        }
        for n in nets
    ]):
        return EXIT_OK

    out = _out(args)
    rows = []
    for net in nets:
        tags = [t for t, on in (
            ("testnet", net.testnet), ("L2", net.layer2),
            ("uptime-feed", net.key in SEQUENCER_FEEDS), ("ccip", net.key in ROUTERS),
        ) if on]
        rows.append([net.key, str(net.chain_id), str(len(list_feeds(net.key))),
                     net.label, ", ".join(tags)])
    out.table(["network", "chain", "feeds", "label", "tags"], rows,
              aligns=["left", "right", "right", "left", "left"],
              styles=["key", "number", "number", "value", "muted"])
    out.blank()
    out.line(f"{feed_count()} feeds across {len(nets)} networks", "muted")
    return EXIT_OK


def _cmd_doctor(args: argparse.Namespace) -> int:
    result = diagnose(network=args.network, rpc_url=args.rpc_url)
    if _structured(args, [{**result.as_dict(), "terminal": boot.describe()}]):
        return EXIT_OK if result.ok else EXIT_UNUSABLE

    out = _out(args)
    out.heading("READINESS", f"{result.network}")
    out.kv("endpoint", f"{result.endpoint}  ({result.endpoint_source})")
    out.blank()
    for check in result.checks:
        out.check(check.ok, check.name, check.detail, check.hint)
    out.blank()
    out.heading("TERMINAL")
    for name, value in boot.describe().items():
        out.kv(name.replace("_", " "), str(value), width=16)
    return EXIT_OK if result.ok else EXIT_UNUSABLE


def _cmd_omni(args: argparse.Namespace) -> int:
    """Emit this network's oracle feeds as a Scematica Omni ``WorldState``.

    Always JSON, and deliberately not routed through ``_structured``/``_out``. The other
    commands have a text form for a human and a JSON form for a pipe; this one has a single
    consumer — an agent runtime that reads one exact shape — and a "text form" of a
    ``WorldState`` would be a second, prettier thing nobody parses. ``--format`` is
    therefore ignored here rather than silently producing something the consumer rejects.

        $ alchem-link omni -n base | scema simulate "is this safe to price against" --path -

    Every signal it emits is a count. An unreadable aggregator becomes a blind spot, never
    a price of zero. See :mod:`alchem_link.omni`.
    """
    import json

    from .omni import perceive, perceive_window

    if getattr(args, "window", False):
        # A different question, not a prettier answer. The snapshot says what the feeds say
        # now; the window says how they have behaved, and a feed can be perfectly fresh at
        # the instant you look and have been absent for the four hours before. Both are
        # worlds, both carry the same entity locator, and an agent can hold both.
        state = perceive_window(
            network=args.network, hours=args.hours, rpc_url=args.rpc_url
        )
    else:
        state = perceive(network=args.network, rpc_url=args.rpc_url)
    print(json.dumps(state, indent=2, sort_keys=True))

    # Exit code reflects what was found, so a shell pipeline can branch on it: a world with
    # nothing counted against it is a clean read, and one carrying a risk signal is not a
    # failure of this command either — the *agent* decides what to do about it. Only an
    # unreadable set is a problem this command can report.
    blind = {"unreadable-feeds", "feeds-without-history"}
    unreadable = any(s["id"] in blind for s in state["signals"])
    return EXIT_UNUSABLE if unreadable else EXIT_OK


def _cmd_verify(args: argparse.Namespace) -> int:
    from .feeds import verify_registry

    results = verify_registry(network=args.network, rpc_url=args.rpc_url)
    if _structured(args, results):
        return EXIT_OK if all(r.get("ok") for r in results) else EXIT_UNUSABLE

    out = _out(args)
    failures = 0
    for entry in results:
        ok = bool(entry.get("ok"))
        failures += 0 if ok else 1
        detail = entry.get("description") or entry.get("error", "")
        out.check(ok, str(entry["pair"]), f"{entry['address']}  {detail}")
    out.blank()
    out.line(f"{len(results) - failures}/{len(results)} registry entries match their "
             "on-chain description", "ok" if failures == 0 else "bad")
    return EXIT_OK if failures == 0 else EXIT_UNUSABLE


def _cmd_audit(args: argparse.Namespace) -> int:
    rpc = _client(args)
    if args.pair:
        audits = [audit_feed(resolve(args.pair, args.network), network=args.network,
                             client=rpc, address=args.address)]
    else:
        audits = audit_network(network=args.network, client=rpc)

    if _structured(args, audits):
        return EXIT_OK if all(a.safe_to_consume for a in audits) else EXIT_UNUSABLE

    out = _out(args)
    unsafe = 0
    for audit in audits:
        unsafe += 0 if audit.safe_to_consume else 1
        price = f"  {fmt_price(audit.price)}" if audit.price is not None else ""
        out.blank()
        out.heading(f"{audit.description or audit.pair}",
                    f"({audit.network}){price}   [{audit.worst}]")
        out.note(audit.address)
        if not audit.findings:
            out.ok("no findings")
        for finding in audit.sorted_findings:
            out.finding(finding.severity, finding.code, finding.title,
                        finding.detail, finding.remedy)
    out.blank()
    out.line(f"{len(audits) - unsafe}/{len(audits)} feed(s) safe to consume",
             "ok" if unsafe == 0 else "bad")
    return EXIT_OK if unsafe == 0 else EXIT_UNUSABLE


def _cmd_inspect(args: argparse.Namespace) -> int:
    rpc = _client(args)
    address = args.address
    if not address:
        if not args.pair:
            _out(args).error("usage: alchem-link inspect <PAIR> | --address 0x...")
            return EXIT_USAGE
        from .feeds import get_feed

        address = get_feed(resolve(args.pair, args.network), args.network).address

    info = describe_aggregator(address, network=args.network, client=rpc)
    if _structured(args, [info]):
        return EXIT_OK

    out = _out(args)
    out.heading(info.description or "(no description)", f"({info.network})")
    out.kv("proxy", info.address, width=15)
    out.kv("implementation", info.implementation or "(this address is the aggregator)",
           width=15)
    out.kv("type", info.type_and_version or "unknown", width=15)
    out.kv("version", f"{info.version}   phase {info.phase_id}", width=15)
    out.kv("decimals", str(info.decimals), width=15)
    out.kv("owner", info.owner or "n/a", width=15)
    if info.latest:
        carried = "  (carried an older answer forward)" if info.latest.carried_over else ""
        out.kv("latest", f"{fmt_price(info.latest.price)}  round "
                         f"{info.latest.aggregator_round} of phase "
                         f"{info.latest.phase_id}{carried}", width=15)
    if info.min_answer is not None or info.max_answer is not None:
        out.kv("bounds", f"min {info.min_price:,.8g}  max {info.max_price:,.6g}", width=15)
        floor, ceiling = info.floor_headroom, info.ceiling_headroom
        if floor is not None and ceiling is not None:
            out.kv("headroom",
                   f"{floor:.3g}x to the floor, {ceiling:.3g}x to the ceiling   "
                   f"[{'BINDING' if info.bounds_are_binding else 'not binding'}]",
                   width=15,
                   value_style="bad" if info.bounds_are_binding else "value")
    for note in info.notes:
        out.kv("note", note, width=15, value_style="warn")
    return EXIT_OK


def _cmd_history(args: argparse.Namespace) -> int:
    from .feeds import get_feed

    rpc = _client(args)
    address = args.address or get_feed(resolve(args.pair, args.network), args.network).address
    rounds = round_history(address, count=args.rounds, network=args.network, client=rpc)
    if _structured(args, rounds):
        return EXIT_OK

    out = _out(args)
    if not rounds:
        out.warn("no rounds could be read")
        return EXIT_UNUSABLE
    ordered = sorted(rounds, key=lambda r: -r.updated_at)
    rows = []
    for index, entry in enumerate(ordered):
        if index + 1 < len(ordered):
            previous = ordered[index + 1]
            gap = f"{entry.updated_at - previous.updated_at}s"
            change = (f"{(entry.price - previous.price) / previous.price * 100:+.3f}%"
                      if previous.price else "—")
        else:
            gap, change = "—", "—"
        rows.append([str(entry.aggregator_round), fmt_price(entry.price), gap, change,
                     "carried" if entry.carried_over else ""])
    out.table(["round", "price", "interval", "change", ""], rows,
              aligns=["right", "right", "right", "right", "left"],
              styles=["key", "number", "muted", "value", "warn"])
    return EXIT_OK


def _cmd_stats(args: argparse.Namespace) -> int:
    """TWAP, volatility, drawdown and outliers over a feed's recent history."""
    from .feeds import get_feed

    rpc = _client(args)
    pairs = [resolve(args.pair, args.network)] if args.pair else [
        f.pair for f in list_feeds(args.network)
    ]
    out = _out(args)
    summaries = []
    for pair in pairs:
        address = get_feed(pair, args.network).address
        rounds = round_history(address, count=args.rounds, network=args.network, client=rpc)
        summaries.append(summarise(Series.from_rounds(rounds, pair, args.network)))

    if _structured(args, summaries):
        return EXIT_OK

    from .term.widgets import sparkline

    for pair, stats in zip(pairs, summaries):
        out.blank()
        if stats.samples < 2:
            out.heading(pair, f"({args.network})")
            out.warn("not enough history to summarise")
            continue
        out.heading(pair, f"({args.network})  {stats.samples} rounds over "
                          f"{fmt_age(stats.span_secs)}")
        out.kv("last", fmt_price(stats.last or 0), width=13)
        out.kv("change", f"{stats.change_pct:+.3f}%", width=13,
               value_style="ok" if (stats.change_pct or 0) >= 0 else "bad")
        out.kv("range", f"{fmt_price(stats.low or 0)} – {fmt_price(stats.high or 0)}"
                        f"   ({stats.range_pct:.2f}%)" if stats.range_pct is not None
                        else "—", width=13)
        out.kv("twap", fmt_price(stats.twap or 0), width=13)
        if stats.twap_divergence_bps is not None:
            out.kv("vs twap", f"{stats.twap_divergence_bps:+.0f} bps", width=13,
                   value_style="warn" if abs(stats.twap_divergence_bps) > 100 else "value")
        if stats.volatility_annual:
            out.kv("volatility", f"{stats.volatility_annual * 100:.1f}% annualised", width=13)
        out.kv("max drawdown",
               f"{stats.max_drawdown_pct:.3f}%" if stats.max_drawdown_pct is not None
               else "—", width=13)
        out.kv("largest move",
               f"{stats.largest_move_bps:.1f} bps" if stats.largest_move_bps is not None
               else "—", width=13)
        if stats.median_interval_secs:
            out.kv("interval", f"median {fmt_secs(int(stats.median_interval_secs))}", width=13)
    return EXIT_OK


def _cmd_updates(args: argparse.Namespace) -> int:
    """Publishes read from event logs rather than by walking rounds."""
    from .feeds import get_feed

    pair = resolve(args.pair, args.network)
    address = get_feed(pair, args.network).address
    updates = answer_updates(address, hours=args.hours, network=args.network,
                             rpc_url=args.rpc_url)
    if _structured(args, updates):
        return EXIT_OK

    out = _out(args)
    if not updates:
        out.warn(f"no AnswerUpdated logs for {pair} in the last {args.hours:g}h")
        out.note("public endpoints prune logs aggressively — try a shorter window, "
                 "or `alchem-link history` which walks rounds instead")
        return EXIT_UNUSABLE
    rows = []
    for index, update in enumerate(updates):
        gap = f"{update.updated_at - updates[index - 1].updated_at}s" if index else "—"
        rows.append([str(update.aggregator_round), fmt_price(update.price), gap,
                     str(update.block_number), f"{fmt_age(update.age_secs)} ago"])
    out.table(["round", "price", "interval", "block", "age"], rows,
              aligns=["right", "right", "right", "right", "right"],
              styles=["key", "number", "muted", "muted", "muted"])
    out.blank()
    out.line(f"{len(updates)} publishes in {args.hours:g}h", "muted")
    return EXIT_OK


def _cmd_cadence(args: argparse.Namespace) -> int:
    rpc = _client(args)
    if args.pair:
        profiles = [profile_feed(resolve(args.pair, args.network), args.network,
                                 rounds=args.rounds, client=rpc)]
    else:
        profiles = [
            profile_feed(feed.pair, args.network, rounds=args.rounds, client=rpc)
            for feed in list_feeds(args.network)
        ]
    if _structured(args, profiles):
        return EXIT_OK

    out = _out(args)
    mismatched = 0
    for profile in profiles:
        verdict = profile.heartbeat_verdict
        mismatched += 1 if verdict in ("declared too tight", "declared too loose") else 0
        observed = fmt_secs(profile.observed_heartbeat) if profile.observed_heartbeat else "—"
        out.blank()
        out.heading(profile.pair, f"({profile.network})   [{verdict}]")
        out.kv("declared", fmt_secs(profile.declared_heartbeat))
        out.kv("observed", f"{observed}   (ceiling {profile.observed_ceiling_secs}s, "
                           f"median {profile.median_interval}s over {profile.samples} intervals)")
        out.kv("triggers", f"{profile.heartbeat_triggered} by heartbeat, "
                           f"{profile.deviation_triggered} by deviation")
        if profile.inferred_deviation_pct is not None:
            out.kv("deviation", f"threshold is at most {profile.inferred_deviation_pct}% "
                                f"(largest move seen {profile.largest_move_pct:.3f}%)")
        out.note(profile.verdict_detail)
    if len(profiles) > 1:
        out.blank()
        out.line(f"{mismatched}/{len(profiles)} declared heartbeat(s) disagree with "
                 "measurement", "warn" if mismatched else "ok")
    return EXIT_UNUSABLE if mismatched else EXIT_OK


def _cmd_divergence(args: argparse.Namespace) -> int:
    if args.pair:
        reports = [compare_pair(resolve(args.pair), outlier_bps=args.threshold)]
    else:
        reports = compare_all(outlier_bps=args.threshold)
    if _structured(args, reports):
        return EXIT_OK if all(r.verdict != "diverged" for r in reports) else EXIT_UNUSABLE

    out = _out(args)
    diverged = 0
    for report in reports:
        diverged += 1 if report.verdict == "diverged" else 0
        consensus = (f"consensus {fmt_price(report.consensus)}" if report.consensus
                     else "no consensus")
        out.blank()
        out.heading(report.pair, f"[{report.verdict}]   {consensus}")
        rows = []
        for leg in sorted(report.legs, key=lambda item: -abs(item.deviation_bps)):
            if leg.error:
                rows.append([leg.network, "unreadable", leg.error[:40], "", ""])
                continue
            rows.append([leg.network, fmt_price(leg.price), f"{leg.deviation_bps:+.1f} bps",
                         "STALE" if leg.stale else "", f"{fmt_age(leg.age_secs)} old"])
        out.table(["network", "price", "deviation", "", "age"], rows,
                  aligns=["left", "right", "right", "left", "right"],
                  styles=["key", "number", "value", "warn", "muted"])
        out.note(report.detail)
    if len(reports) > 1:
        out.blank()
        out.line(f"{diverged}/{len(reports)} pair(s) diverged beyond "
                 f"{args.threshold:.0f} bps", "bad" if diverged else "ok")
    return EXIT_UNUSABLE if diverged else EXIT_OK


def _cmd_sequencer(args: argparse.Namespace) -> int:
    targets = [args.network] if args.network in SEQUENCER_FEEDS else (
        [args.network] if args.network != DEFAULT_NETWORK else sorted(SEQUENCER_FEEDS)
    )
    statuses = [s for s in (read_sequencer(net) for net in targets) if s is not None]
    if _structured(args, statuses):
        return EXIT_OK if all(s.ok for s in statuses) else EXIT_UNUSABLE

    out = _out(args)
    if not statuses:
        net = get_network(args.network)
        if is_l2(net.key):
            out.error(f"{net.key} is a rollup but no uptime feed is registered — "
                      "price reads there are unguarded")
            return EXIT_UNUSABLE
        out.line(f"{net.key} is not a rollup; no sequencer uptime feed applies", "muted")
        return EXIT_OK

    bad = 0
    for status in statuses:
        bad += 0 if status.ok else 1
        out.check(status.ok, f"{status.state:<7} {status.network}", status.address)
        out.note("           " + status.detail)
    return EXIT_UNUSABLE if bad else EXIT_OK


def _cmd_simulate(args: argparse.Namespace) -> int:
    """Replay a consumer guard against the known oracle failure modes."""
    guard = _guard_from_args(args)
    out = _out(args)

    if args.pair:
        report = run_scenario(args.pair, guard)
        if _structured(args, [report]):
            return EXIT_OK if report.caught == SCENARIOS[report.name].should_catch else EXIT_UNUSABLE
        scenario = SCENARIOS[report.name]
        out.heading(scenario.name.upper(), scenario.summary)
        out.note(scenario.expectation)
        out.blank()
        for verdict in report.verdicts:
            observation = verdict.observation
            label = "accept" if verdict.accepted else "REJECT"
            out.write("  " + out.paint(label.ljust(8), role("ok" if verdict.accepted else "bad"))
                      + out.paint(f"{fmt_price(observation.price):>14}   "
                                  f"age {observation.age_secs:>6}s   "
                                  f"round {observation.round_id}", role("value")))
            for reason in verdict.reasons:
                out.note("           " + reason)
        out.blank()
        handled = report.caught == scenario.should_catch
        out.line(f"{len(report.accepted)}/{len(report.verdicts)} observations accepted"
                 + (f" · worst accepted price {fmt_price(report.worst_accepted_price)}"
                    if report.worst_accepted_price is not None else ""), "muted")
        out.line("guard handles this scenario" if handled
                 else "guard does NOT handle this scenario", "ok" if handled else "bad")
        return EXIT_OK if handled else EXIT_UNUSABLE

    result = audit_guard(guard)
    if _structured(args, [result.as_dict()]):
        return EXIT_OK if not result.failed else EXIT_UNUSABLE

    out.heading("GUARD SIMULATION", "replayed against known oracle failure modes")
    out.blank()
    rows = []
    for report in result.reports:
        scenario = SCENARIOS[report.name]
        handled = report.caught == scenario.should_catch
        rows.append([
            report.name,
            ("caught" if report.caught else "MISSED") if scenario.should_catch
            else ("clean" if not report.caught else "REJECTS"),
            f"{len(report.accepted)}/{len(report.verdicts)}",
            scenario.expectation,
        ])
    out.table(["scenario", "result", "accepted", "what it takes to catch"], rows,
              styles=["key", None, "number", "hint"],
              row_styles=[
                  "ok" if SCENARIOS[r.name].should_catch == r.caught else "bad"
                  for r in result.reports
              ])
    out.blank()
    out.line(f"{len(result.passed)}/{len(result.reports)} scenarios handled "
             f"({result.score * 100:.0f}%)",
             "ok" if result.score == 1.0 else ("warn" if result.score >= 0.5 else "bad"))
    if result.failed:
        out.error("gaps: " + ", ".join(result.failed))
        out.note("try --strict, or turn on the specific guard each gap names")
    return EXIT_OK if not result.failed else EXIT_UNUSABLE


def _cmd_backtest(args: argparse.Namespace) -> int:
    """Replay a guard against a feed's real round history.

    The complement to ``simulate``: rejections here are false positives, not catches.
    """
    from .client import AlchemLink

    if not args.pair:
        _out(args).error("usage: alchem-link backtest <PAIR>")
        return EXIT_USAGE

    link = AlchemLink(args.network, rpc_url=args.rpc_url)
    report = link.backtest(args.pair, _guard_from_args(args), rounds=args.rounds)
    if _structured(args, [report]):
        return EXIT_OK if not report.rejected else EXIT_UNUSABLE

    out = _out(args)
    out.heading(report.name.upper(), "guard replayed against real history")
    out.kv("observations", str(len(report.verdicts)))
    out.kv("accepted", f"{len(report.accepted)}  ({report.acceptance_rate * 100:.1f}%)",
           value_style="ok" if report.acceptance_rate == 1.0 else "warn")
    out.kv("rejected", str(len(report.rejected)),
           value_style="value" if not report.rejected else "bad")
    if report.rejected:
        out.kv("longest run", f"{report.longest_rejection_streak} consecutive")
        out.blank()
        for code, count in report.reason_counts.items():
            out.bullet(f"{code} × {count}", style="warn")
        out.blank()
        out.note("every rejection here is a round the feed legitimately produced — "
                 "a guard that rejects real history will halt your protocol")
    else:
        out.blank()
        out.ok("no false positives over this window")
    return EXIT_OK if not report.rejected else EXIT_UNUSABLE


def _guard_from_args(args: argparse.Namespace) -> Guard:
    if getattr(args, "strict", False):
        guard = Guard.strict()
    elif getattr(args, "naive", False):
        guard = Guard.naive()
    else:
        guard = Guard()
    if getattr(args, "max_age", None) is not None:
        guard.max_age_secs = args.max_age
    if getattr(args, "max_move_bps", None) is not None:
        guard.max_move_bps = args.max_move_bps
    return guard


def _cmd_generate(args: argparse.Namespace) -> int:
    out = _out(args)
    if args.basket is not None:
        pairs = [p.strip() for p in args.basket.split(",") if p.strip()] or basket_pairs(args.network)
        result = generate_basket(pairs, network=args.network)
        if _fmt(args) == "json":
            out.json(result.as_dict())
        else:
            out.write(result.code)
        return EXIT_OK

    if args.project:
        project = generate_project(args.pair, network=args.network, framework=args.framework)
        if not args.out:
            if _fmt(args) == "json":
                out.json(project.as_dict())
                return EXIT_OK
            # Without --out there is nowhere to put a dozen files, so show the plan and
            # the command that would write it rather than dumping them all to stdout.
            out.heading(project.name, f"{project.framework} project for "
                                      f"{', '.join(project.pairs)} on {project.network}")
            out.blank()
            out.table(["path", "purpose"],
                      [[a.path, a.description] for a in project.artifacts],
                      styles=["key", "muted"])
            out.blank()
            out.kv("guards", ", ".join(project.guards))
            out.blank()
            out.note(f"write it with:  alchem-link generate {args.pair} -n {args.network} "
                     "--project --out <dir>")
            return EXIT_OK

        try:
            written = project.write(args.out, overwrite=args.force)
        except FileExistsError as exc:
            out.error(str(exc))
            return EXIT_USAGE
        if _fmt(args) == "json":
            out.json({**project.as_dict(), "written": written})
            return EXIT_OK
        for path in written:
            out.bullet(f"wrote {path}")
        out.blank()
        out.note(f"{len(written)} files. Next:\n"
                 f"    cd {args.out} && forge install foundry-rs/forge-std && forge test -vv")
        return EXIT_OK

    result = generate_consumer(args.pair, network=args.network, language=args.lang)
    if _fmt(args) == "json":
        out.json(result.as_dict())
        return EXIT_OK
    if args.out:
        suffix = {"solidity": "sol", "typescript": "ts", "python": "py", "rust": "rs"}[args.lang]
        target = Path(args.out)
        if target.is_dir() or not target.suffix:
            target = target / f"{_identifier_for(result.pair)}Consumer.{suffix}"
        target.parent.mkdir(parents=True, exist_ok=True)
        if target.exists() and not args.force:
            out.error(f"{target} exists — pass --force to replace it")
            return EXIT_USAGE
        target.write_text(result.code, encoding="utf-8")
        out.bullet(f"wrote {target}")
        return EXIT_OK
    out.write(result.code)
    return EXIT_OK


def _identifier_for(pair: str) -> str:
    from .codegen import _identifier

    return _identifier(pair)


def _cmd_ccip(args: argparse.Namespace) -> int:
    out = _out(args)
    if args.network not in ROUTERS:
        out.error(f"no verified CCIP router for {args.network}. "
                  f"Known: {', '.join(sorted(ROUTERS))}")
        return EXIT_USAGE
    lanes = verify_lanes(args.network, rpc_url=args.rpc_url)
    if _structured(args, lanes):
        return EXIT_OK
    out.heading(f"CCIP LANES — {args.network.upper()}", f"router {ROUTERS[args.network]}")
    out.table(
        ["destination", "selector", "state"],
        [[lane.destination, str(lane.destination_selector),
          f"error — {lane.error[:40]}" if lane.error else ("open" if lane.supported else "closed")]
         for lane in lanes],
        styles=["key", "muted", None],
        row_styles=["bad" if l.error else ("ok" if l.supported else "muted") for l in lanes],
    )
    out.blank()
    out.note("selectors are not chain ids — passing a chain id here reverts")
    return EXIT_OK


def _cmd_holdings(args: argparse.Namespace) -> int:
    out = _out(args)
    if not args.address:
        out.error("usage: alchem-link holdings --address 0x...")
        return EXIT_USAGE
    holdings = value_holdings(
        args.address,
        tokens=args.tokens.split(",") if args.tokens else None,
        network=args.network,
        rpc_url=args.rpc_url,
    )
    if _structured(args, [holdings]):
        return EXIT_OK

    out.heading(holdings.address, f"({holdings.network})   via {holdings.discovered_via}")
    rows = []
    if holdings.native_value_usd is not None:
        rows.append([holdings.native_symbol, f"{holdings.native_balance:,.6f}",
                     f"${holdings.native_value_usd:,.2f}"])
    for token in holdings.tokens:
        if not token.raw_balance:
            continue
        rows.append([token.symbol, f"{token.balance:,.6f}",
                     f"${token.value_usd:,.2f}" if token.value_usd is not None else "unpriced"])
    out.table(["asset", "balance", "value"], rows, aligns=["left", "right", "right"],
              styles=["key", "number", "number"])
    out.blank()
    out.note(holdings.coverage)
    out.kv("total", f"${holdings.total_usd:,.2f} (priced positions only)")
    for note in holdings.notes:
        out.note(note)
    return EXIT_OK


def _cmd_transfers(args: argparse.Namespace) -> int:
    out = _out(args)
    if not args.address:
        out.error("usage: alchem-link transfers --address 0x...")
        return EXIT_USAGE
    transfers = get_asset_transfers(
        network=args.network,
        from_address=args.address if args.direction == "out" else None,
        to_address=args.address if args.direction == "in" else None,
        max_count=args.limit,
        rpc_url=args.rpc_url,
    )
    if _structured(args, transfers):
        return EXIT_OK
    if not transfers:
        out.line("no transfers found", "muted")
        return EXIT_OK
    out.table(
        ["timestamp", "value", "asset", "from", "to"],
        [[(e.get("metadata") or {}).get("blockTimestamp", ""), str(e.get("value")),
          e.get("asset") or "?", (e.get("from") or "")[:12] + "…",
          (e.get("to") or "")[:12] + "…"] for e in transfers],
        aligns=["left", "right", "left", "left", "left"],
        styles=["muted", "number", "key", "muted", "muted"],
    )
    return EXIT_OK


def _cmd_watch(args: argparse.Namespace) -> int:
    try:
        for event in watch_feed(
            resolve(args.pair, args.network),
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
    if _fmt(args) != "text":
        _out(args).json(summary)
        return EXIT_OK
    out = _out(args)
    out.kv("endpoint", f"{summary['endpoint']}  ({summary['source']})", width=10)
    out.kv("auth", "yes" if summary["authenticated"] else "no", width=10)
    out.blank()
    for feature in summary["features"]:
        out.check(feature["available"], feature["method"], feature["capability"])
    if summary["hint"]:
        out.blank()
        out.note(summary["hint"])
    return EXIT_OK


def _cmd_chainlink(args: argparse.Namespace) -> int:
    summary = summarize_chainlink_capabilities()
    if _fmt(args) != "text":
        _out(args).json(summary)
        return EXIT_OK
    out = _out(args)
    for name, entry in summary.items():
        out.check(entry["verified_live"], name, "read live" if entry["verified_live"]
                  else "not read by this toolkit")
        out.note("         " + entry["detail"])
        if entry["commands"]:
            out.note("         commands: " + ", ".join(entry["commands"]))
    return EXIT_OK


def _cmd_integration(args: argparse.Namespace) -> int:
    _out(args).json(build_integration_map())
    return EXIT_OK


def _cmd_blueprint(args: argparse.Namespace) -> int:
    _out(args).json(build_package_blueprint())
    return EXIT_OK


def _cmd_recipes(args: argparse.Namespace) -> int:
    out = _out(args)
    if args.pair:
        recipe = get_recipe_by_id(args.pair)
        if recipe is None:
            out.error(f"unknown recipe id: {args.pair}")
            return EXIT_USAGE
        out.json(recipe)
        return EXIT_OK
    out.json(get_recipes())
    return EXIT_OK


def _cmd_theme(args: argparse.Namespace) -> int:
    """Show the palette and what the terminal negotiated. The visual smoke test."""
    from .term import ansi

    out = _out(args)
    info = boot.describe()
    if _fmt(args) != "text":
        out.json({"palette": PALETTE, "terminal": info})
        return EXIT_OK

    out.heading("ALCHEM-LINK PALETTE", "black surfaces, mid-blue signal")
    out.blank()
    for name, value in PALETTE.items():
        swatch = out.paint("  ████  ", Style(fg=value, bg=value))
        out.write("  " + out.paint(name.ljust(12), role("value")) + swatch
                  + out.paint("  " + value, _role_style(out, True)
                              if False else role("value")))
    out.blank()
    out.heading("TERMINAL")
    for name, value in info.items():
        out.kv(name.replace("_", " "), str(value), width=16)
    out.blank()
    out.note(f"colour is negotiated per stream: {ansi.Depth.name(out.depth)} here. "
             "NO_COLOR, a pipe, or ALCHEM_COLOR=16 each change this line.")
    return EXIT_OK


def _cmd_ui(args: argparse.Namespace) -> int:
    from .dashboard import launch

    return launch(network=args.network)


def _cmd_shell(args: argparse.Namespace) -> int:
    from .shell import Shell

    return Shell(
        network=args.network,
        mode=args.mode,
        workspace=getattr(args, "workspace", None),
        policy=_policy_from_args(args),
    ).run()


def _policy_from_args(args: argparse.Namespace) -> TrustPolicy:
    """Turn the trust flags into a policy.

    ``--read-only`` wins over the permissive flags rather than combining with them. A
    command line that says both is a mistake, and the safe reading of a mistake is the
    restrictive one.
    """
    if getattr(args, "read_only", False):
        return TrustPolicy.read_only_policy()
    return TrustPolicy(
        allow_writes=getattr(args, "yes", False),
        allow_execute=getattr(args, "allow_exec", False),
    )


def _cmd_chat(args: argparse.Namespace) -> int:
    """One-shot chat: ask a question, print the answer, exit.

    The same agent the shell uses, so this composes into scripts and pipes without
    holding a session open.
    """
    from .agent import build_agent
    from .llm import NoProviderConfigured

    out = _out(args)
    if not args.pair:
        out.error('usage: alchem-link chat "is the ETH/USD feed on base safe?"')
        return EXIT_USAGE

    policy = _policy_from_args(args)
    # One-shot chat is frequently piped or scripted, where nobody can answer a prompt.
    # `default_approver` refuses in that case; `--yes` is the explicit opt-out.
    approver = AutoApprover() if getattr(args, "yes", False) else default_approver()

    try:
        agent = build_agent(
            network=args.network,
            workspace=getattr(args, "workspace", None),
            policy=policy,
            approver=approver,
        )
    except NoProviderConfigured as exc:
        out.error(str(exc))
        return EXIT_USAGE
    except Exception as exc:
        out.error(str(exc))
        return EXIT_USAGE

    calls: List[Any] = []
    turn = agent.ask(args.pair, on_tool=calls.append)

    if _fmt(args) != "text":
        out.json({
            "question": args.pair, "reply": turn.reply, "model": turn.model,
            "rounds": turn.rounds,
            "tool_calls": [
                {"name": c.name, "arguments": c.arguments, "ok": c.ok, "error": c.error}
                for c in calls
            ],
            "error": turn.error,
        })
        return EXIT_OK if turn.ok else EXIT_UNUSABLE

    # Tool calls go to stderr so `alchem-link chat ... > answer.txt` captures only the
    # answer while the reads stay visible in the terminal.
    errors = Console(sys.stderr)
    for call in calls:
        errors.bullet(call.summary + ("" if call.ok else f"  {call.error[:70]}"),
                      marker="·" if call.ok else "×",
                      style="muted" if call.ok else "bad")
    if calls:
        errors.blank()
    out.write(turn.reply)
    # Written files are a fact and are reported to stderr regardless of what the model
    # remembered to say, so `chat ... > answer.txt` still shows them.
    if turn.changed_paths:
        errors.blank()
        errors.ok("changed: " + ", ".join(turn.changed_paths))
    for refusal in turn.refusals:
        errors.warn(f"refused: {refusal.name}"
                    + (f" {refusal.path}" if refusal.path else ""))
    return EXIT_OK if turn.ok else EXIT_UNUSABLE


def _cmd_providers(args: argparse.Namespace) -> int:
    from .llm import PROVIDER_ENV, available_providers

    entries = available_providers()
    if _structured(args, entries):
        return EXIT_OK if any(e["ready"] for e in entries) else EXIT_UNUSABLE

    out = _out(args)
    for entry in entries:
        out.check(entry["ready"], entry["label"],
                  f"{'free' if entry['free'] else 'paid'}  {entry['detail']}")
        out.note("         " + entry["model"])
    if not any(e["ready"] for e in entries):
        out.blank()
        out.warn("no provider configured — chat is unavailable. Everything else works.")
        return EXIT_UNUSABLE
    out.blank()
    out.note(f"override with {PROVIDER_ENV}=<provider>.")
    return EXIT_OK


LIVE_COMMANDS = [
    ("price", "Read one feed, with a staleness verdict"),
    ("feeds", "List registered feeds (--live to read them)"),
    ("audit", "Run every oracle-consumer safety check"),
    ("inspect", "Resolve the proxy and read its bounds and type"),
    ("history", "Walk a feed's round history"),
    ("updates", "Publishes from event logs — cheaper than walking rounds"),
    ("stats", "TWAP, volatility, drawdown over recent history"),
    ("cadence", "Measure the real heartbeat and deviation threshold"),
    ("divergence", "Compare one pair across every chain that carries it"),
    ("sequencer", "L2 sequencer uptime, with the grace period applied"),
    ("watch", "Stream new rounds as JSON Lines"),
    ("gas", "EIP-1559 fee tiers, priced in USD"),
    ("holdings", "Value an address's tokens through Chainlink"),
    ("transfers", "Transfer history (needs an Alchemy key)"),
    ("block", "Current block height and latency"),
    ("doctor", "End-to-end readiness check"),
    ("verify", "Confirm each address reports the pair it is filed under"),
    ("ccip", "CCIP routers, chain selectors and live lane status"),
    ("omni", "Emit these feeds as a Scematica Omni WorldState (JSON, for `scema`)"),
]

OFFLINE_COMMANDS = [
    ("search", "Find feeds by pair, asset or address — no chain read"),
    ("networks", "Supported networks and what each carries"),
    ("coverage", "Per-chain feed counts and how much cadence is measured"),
    ("simulate", "Replay your consumer guards against known failure modes"),
    ("backtest", "Replay your guards against a feed's real history"),
    ("theme", "The palette, and what this terminal negotiated"),
]

INTERACTIVE_COMMANDS = [
    ("ui", "Full-screen dashboard over the whole toolkit"),
    ("shell", "Interactive console — commands and chat in one prompt"),
    ("chat", "Ask one question; the agent answers by reading chains"),
    ("providers", "LLM providers and which are usable right now"),
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
    "omni": _cmd_omni,
    "ui": _cmd_ui,
    "shell": _cmd_shell,
    "chat": _cmd_chat,
    "providers": _cmd_providers,
    "price": _cmd_price,
    "feeds": _cmd_feeds,
    "search": _cmd_search,
    "coverage": _cmd_coverage,
    "audit": _cmd_audit,
    "inspect": _cmd_inspect,
    "history": _cmd_history,
    "updates": _cmd_updates,
    "stats": _cmd_stats,
    "cadence": _cmd_cadence,
    "divergence": _cmd_divergence,
    "sequencer": _cmd_sequencer,
    "simulate": _cmd_simulate,
    "backtest": _cmd_backtest,
    "theme": _cmd_theme,
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

ALL_COMMANDS = LIVE_COMMANDS + OFFLINE_COMMANDS + INTERACTIVE_COMMANDS + REFERENCE_COMMANDS


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        prog="alchem-link",
        description=(
            "Alchemy x Chainlink developer toolkit — live oracle reads, consumer-safety "
            "auditing, guard simulation, and the integration reference"
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
        sub.add_argument("--json", action="store_true", help="Shorthand for --format json")
        sub.add_argument("--format", choices=("text",) + FORMATS, default="text",
                         help="Output format (default: text)")
        sub.add_argument("--no-color", action="store_true",
                         help="Plain text output, same as NO_COLOR=1")
        return sub

    def add_pair(sub: argparse.ArgumentParser, required: bool = True,
                 help_text: str = "") -> None:
        sub.add_argument("pair", nargs=None if required else "?",
                         help=help_text or "Feed pair, e.g. ETH/USD")

    for name, help_text in ALL_COMMANDS:
        sub = add(name, help_text)

        if name in ("price", "watch", "updates"):
            add_pair(sub)
        elif name == "chat":
            sub.add_argument("pair", nargs="?", metavar="QUESTION",
                             help='Your question, e.g. "is ETH/USD on base safe?"')
        elif name == "search":
            add_pair(sub, required=False, help_text="Pair, partial name or address")
        elif name == "simulate":
            add_pair(sub, required=False,
                     help_text=f"Scenario to run in detail ({', '.join(SCENARIOS)})")
        elif name in ("audit", "inspect", "history", "cadence", "divergence", "recipes",
                      "stats", "backtest"):
            add_pair(sub, required=False, help_text=(
                "Recipe id" if name == "recipes" else "Feed pair (default: every feed)"
            ))

        if name == "shell":
            sub.add_argument("--mode", choices=["auto", "cmd", "chat"], default="auto",
                             help="Start pinned to a mode (default: infer per line)")
        if name in ("shell", "chat"):
            sub.add_argument("--workspace", metavar="DIR",
                             help="Directory the agent may read and write "
                                  "(default: the current one)")
            sub.add_argument("--yes", "-y", action="store_true",
                             help="Do not prompt before writing files")
            sub.add_argument("--allow-exec", action="store_true",
                             help="Let the agent run commands (still prompts for each)")
            sub.add_argument("--read-only", action="store_true",
                             help="Refuse every write and command")
        if name == "search":
            sub.add_argument("--asset", help="Filter to feeds involving this asset")
            sub.add_argument("--all", action="store_true",
                             help="Search every network, not just -n")
        if name in ("simulate", "backtest"):
            sub.add_argument("--strict", action="store_true",
                             help="Every guard on — what `generate` emits")
            sub.add_argument("--naive", action="store_true",
                             help="latestRoundData() and nothing else")
            sub.add_argument("--max-age", type=int, help="Staleness window in seconds")
            sub.add_argument("--max-move-bps", type=float,
                             help="Reject a move larger than this many bps")
        if name in ("audit", "inspect", "history"):
            sub.add_argument("--address", help="Audit an arbitrary aggregator address")
        if name in ("holdings", "transfers"):
            sub.add_argument("--address", help="Account address to inspect")
        if name == "holdings":
            sub.add_argument("--tokens", help="Comma-separated ERC-20 addresses (no key needed)")
        if name == "transfers":
            sub.add_argument("--direction", choices=["in", "out"], default="out")
            sub.add_argument("--limit", type=int, default=25)
        if name in ("history", "cadence", "stats", "backtest"):
            sub.add_argument("--rounds", type=int, default=30, help="Rounds of history to walk")
        if name in ("updates", "omni"):
            sub.add_argument("--hours", type=float, default=6.0,
                             help="Window to search for publishes (default: 6)")
        if name == "omni":
            sub.add_argument("--window", action="store_true",
                             help="Describe the last --hours of history rather than the "
                                  "present instant")
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
            add_pair(sub, required=False, help_text="Feed pair, e.g. ETH/USD")
            sub.add_argument("--lang", choices=LANGUAGES, default="solidity",
                             help="Single-file target language (default: solidity)")
            sub.add_argument("--project", action="store_true",
                             help="Emit a full project: consumer, mocks, tests, deploy script")
            sub.add_argument("--framework", choices=FRAMEWORKS, default="foundry")
            sub.add_argument("--basket", metavar="PAIRS",
                             help="Comma-separated pairs for one multi-feed contract "
                                  "(empty string = every feed on the network)")
            sub.add_argument("--out", help="Write to this directory (or file, for --lang)")
            sub.add_argument("--force", action="store_true", help="Overwrite existing files")

    return parser


def command_names() -> List[str]:
    """Every dispatchable command name. Used by the shell for completion and routing."""
    return sorted(HANDLERS)


def _print_overview() -> int:
    out = console()
    out.heading(f"ALCHEM-LINK v{__version__}",
                f"{feed_count()} feeds across {len(list_networks())} networks")
    for label, group in (
        ("Live — talk to a chain", LIVE_COMMANDS),
        ("Offline — table lookups and simulation", OFFLINE_COMMANDS),
        ("Interactive", INTERACTIVE_COMMANDS),
        ("Reference and codegen", REFERENCE_COMMANDS),
    ):
        out.blank()
        out.subheading(label)
        for name, help_text in group:
            out.kv(name, help_text, width=12, value_style="muted")
    out.blank()
    out.note(f"set {ALCHEMY_KEY_ENV} to use Alchemy; otherwise a keyless public endpoint is used.")
    out.note("run `alchem-link ui` for the dashboard, `alchem-link shell` for a console, "
             "or `alchem-link <command> --help` for options.")
    return EXIT_OK


def run_argv(argv: List[str]) -> int:
    """Parse and dispatch one command line.

    Split out of :func:`main` so the interactive shell dispatches through exactly the
    same parser and handlers rather than growing its own copy of every command.
    """
    parser = build_parser()
    args = parser.parse_args(argv)
    if not args.command:
        return _print_overview()
    if getattr(args, "no_color", False):
        import os

        os.environ["NO_COLOR"] = "1"
        reset_console()
    return _dispatch(args)


def _dispatch(args: argparse.Namespace) -> int:
    handler = HANDLERS[args.command]
    errors = Console(sys.stderr)
    try:
        return int(handler(args) or EXIT_OK)
    except NeedsAlchemyKey as exc:
        errors.error(str(exc))
        return EXIT_USAGE
    except RpcTransportError as exc:
        errors.error(f"network error: {exc}")
        return EXIT_NETWORK
    except RpcError as exc:
        errors.error(f"rpc error: {exc}")
        return EXIT_RPC
    except AlchemLinkError as exc:
        # The structured hierarchy carries a next step for a human; use it when there is
        # one rather than making people guess from the message.
        errors.error(str(exc))
        if exc.hint:
            errors.note(exc.hint)
        return EXIT_USAGE
    except (KeyError, ValueError) as exc:
        errors.error(f"error: {exc}")
        return EXIT_USAGE
    except BrokenPipeError:  # pragma: no cover - `| head` closes the pipe
        return EXIT_OK
    except KeyboardInterrupt:
        return 130


def main(argv: Optional[List[str]] = None) -> int:
    """Entry point. Themes the terminal before producing any output.

    :func:`~alchem_link.term.boot.initialize` is what makes plain command output land on
    the product's own black background rather than whatever the console happened to be —
    and it is deliberately unconditional, because the frozen binary is launched into a
    fresh console with no ``TERM`` and is exactly where the theme matters most.
    """
    boot.initialize(title=boot.banner_title(__version__))
    reset_console()
    return run_argv(list(argv) if argv is not None else sys.argv[1:])


if __name__ == "__main__":
    raise SystemExit(main())
