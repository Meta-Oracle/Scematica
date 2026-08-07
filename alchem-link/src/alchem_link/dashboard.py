"""The full-screen console, on the package's own terminal engine.

Twelve panels over the live toolkit: feeds, safety, cross-chain divergence, L2 sequencer
uptime, gas, CCIP lanes, price analytics, guard simulation, registry coverage, and the
reference material. Sidebar on the left, panel on the right, one keystroke to switch
network and one to refresh.

The design decision that shapes this file: **panels render to a list of lines, not to the
screen.** Each renderer is a pure function returning ``List[Line]``, where a line is a
list of ``(text, style)`` segments. The app paints a window onto that list.

Three things fall out of it. Scrolling is free and identical everywhere, because it is one
slice on one list rather than per-panel geometry. Clipping is free, for the same reason.
And every renderer is testable without a terminal — ``tests/test_dashboard.py`` calls them
directly and asserts on the text, including the empty and error states that a happy-path
render never produces and that used to be where a TUI crashed and took the whole screen
with it.

Network work runs on the app's worker pool and is cached per (panel, scope) until ``r``.
Switching panels is instant and does not re-hit the network; a result that lands after you
have navigated away is still cached for its own panel but does not repaint the one you are
looking at.
"""
from __future__ import annotations

from typing import Any, Callable, Dict, List, Optional, Sequence, Tuple

from . import __version__
from .ccip import ROUTERS, summarize_chainlink_capabilities, verify_lanes
from .divergence import compare_all
from .enhanced import summarize_alchemy_capabilities
from .feeds import feed_count, list_feeds, read_all_feeds
from .gas import GAS_SWAP, GAS_TRANSFER, analyse_gas
from .integration import build_integration_map, build_package_blueprint
from .networks import DEFAULT_NETWORK, list_networks, resolve_endpoint
from .recipes import get_recipe_by_id, get_recipes
from .registry import coverage
from .render import fmt_age, fmt_price, fmt_secs
from .rpc import gwei
from .safety import audit_network
from .sequencer import SEQUENCER_FEEDS, read_sequencer
from .simulate import SCENARIOS, Guard, audit_guard
from .term import ansi, boot
from .term.app import App, Job
from .term.screen import Screen
from .term.widgets import Rect, Scroll, panel, scrollbar, sidebar, status_bar
from .theme import BASE, Style, role, severity_style, status_style

#: A styled run of text. Renderers emit these; the app paints them.
Segment = Tuple[str, Style]
Line = List[Segment]


# ── line builders ────────────────────────────────────────────────────────────


def seg(text: str, name: str = "value") -> Segment:
    return (text, role(name))


def line(*segments: Segment) -> Line:
    return list(segments)


def blank() -> Line:
    return []


def title_line(text: str, detail: str = "") -> List[Line]:
    """A panel heading plus its endpoint line and a divider."""
    head = [seg(text, "title")]
    if detail:
        head.append(seg("   " + detail, "hint"))
    return [head, line(seg("─" * 64, "rule"))]


def kv(name: str, value: str, width: int = 14, style: str = "value") -> Line:
    return line(seg(name.ljust(width), "key"), seg(" " + value, style))


def note_line(text: str) -> Line:
    return line(seg(text, "hint"))


def error_lines(message: str) -> List[Line]:
    return [
        line(seg(message, "bad")),
        blank(),
        note_line("press r to retry · n to switch network"),
    ]


def footer_hint() -> List[Line]:
    return [blank(), note_line("r refresh · n network · j/k or ↑/↓ scroll · q quit")]


# ── live panels ──────────────────────────────────────────────────────────────


def render_feeds(network: str, payload: Any, error: Optional[str] = None) -> List[Line]:
    out = title_line(f"LIVE FEEDS — {network.upper()}", _endpoint(network))
    if error:
        return out + error_lines(error)
    readings = payload or []
    if not readings:
        return out + [line(seg("No feeds could be read on this network.", "warn"))]

    width = max(len(r.pair) for r in readings)
    for reading in readings:
        bound = "" if reading.heartbeat_measured else "*"
        out.append(line(
            seg(reading.pair.ljust(width) + "  ", "key"),
            seg(fmt_price(reading.price).rjust(16) + "  ", "number"),
            (reading.status.ljust(8), status_style(reading.status)),
            seg(f"{fmt_age(reading.age_secs):>10} ago · hb {fmt_secs(reading.heartbeat_secs)}{bound}",
                "muted"),
        ))
        if reading.note:
            out.append(line(seg("    " + reading.note, "warn")))

    stale = sum(1 for r in readings if r.stale)
    out.append(line(seg("─" * 64, "rule")))
    out.append(line(seg(
        f"{len(readings)} feeds · {len(readings) - stale} fresh · {stale} past heartbeat",
        "muted",
    )))
    if any(not r.heartbeat_measured for r in readings):
        out.append(note_line(
            "* heartbeat is a conservative bound — no quiet period observed in sampling"
        ))
    return out + footer_hint()


def render_audit(network: str, payload: Any, error: Optional[str] = None) -> List[Line]:
    out = title_line(f"SAFETY AUDIT — {network.upper()}", _endpoint(network))
    if error:
        return out + error_lines(error)
    audits = payload or []
    if not audits:
        return out + [line(seg("No feeds to audit on this network.", "warn"))]

    for audit in audits:
        price = f"  {fmt_price(audit.price)}" if audit.price is not None else ""
        out.append(line(
            seg(audit.pair.ljust(12), "key"),
            seg(price.ljust(16), "number"),
            (audit.worst.upper(), severity_style(audit.worst)),
        ))
        for finding in audit.sorted_findings:
            if finding.severity == "info":
                continue
            out.append(line(
                seg("    "),
                (finding.severity.upper().ljust(9), severity_style(finding.severity)),
                seg(finding.code + " — ", "value"),
                seg(finding.title, "muted"),
            ))
    unsafe = sum(1 for a in audits if not a.safe_to_consume)
    out.append(line(seg("─" * 64, "rule")))
    out.append(line(seg(
        f"{len(audits) - unsafe}/{len(audits)} feeds safe to consume",
        "ok" if unsafe == 0 else "bad",
    )))
    return out + footer_hint()


def render_divergence(network: str, payload: Any, error: Optional[str] = None) -> List[Line]:
    out = title_line("CROSS-CHAIN DIVERGENCE")
    if error:
        return out + error_lines(error)
    reports = payload or []
    if not reports:
        return out + [line(seg("No multi-chain pairs to compare.", "warn"))]

    for report in reports:
        consensus = fmt_price(report.consensus) if report.consensus else "—"
        out.append(line(
            seg(report.pair.ljust(11), "key"),
            seg(report.verdict.upper().ljust(13), "bad" if report.verdict == "diverged" else "ok"),
            seg(f"spread {report.spread_bps:6.1f} bps · consensus {consensus}", "muted"),
        ))
        for leg in sorted(report.legs, key=lambda item: -abs(item.deviation_bps))[:5]:
            if leg.error:
                out.append(line(seg("    " + leg.network.ljust(11), "muted"),
                                seg("unreadable", "bad")))
                continue
            out.append(line(
                seg("    " + leg.network.ljust(11), "muted"),
                seg(fmt_price(leg.price).rjust(14) + "  ", "number"),
                seg(f"{leg.deviation_bps:+7.1f} bps  ", "muted"),
                seg("STALE" if leg.stale else "", "warn"),
            ))
        out.append(blank())
    out.append(note_line("testnets are excluded — their feeds carry unrelated data"))
    return out + footer_hint()


def render_sequencer(network: str, payload: Any, error: Optional[str] = None) -> List[Line]:
    out = title_line("L2 SEQUENCER UPTIME")
    if error:
        return out + error_lines(error)
    statuses = payload or []
    if not statuses:
        return out + [line(seg("No uptime feeds registered.", "warn"))]

    for status in statuses:
        tone = {"UP": "ok", "GRACE": "warn", "DOWN": "bad"}.get(status.state, "muted")
        out.append(line(
            seg(status.state.ljust(8), tone),
            seg(status.network.ljust(11), "key"),
            seg(status.address, "muted"),
        ))
        out.append(line(seg("         " + status.detail, "value")))
    out.append(line(seg("─" * 64, "rule")))
    out.append(note_line(
        "a price feed keeps answering while the sequencer is down — with a frozen price"
    ))
    return out + footer_hint()


def render_gas(network: str, payload: Any, error: Optional[str] = None) -> List[Line]:
    out = title_line(f"GAS — {network.upper()}", _endpoint(network))
    if error:
        return out + error_lines(error)
    if payload is None:
        return out + [line(seg("No fee data.", "warn"))]

    report = payload
    tone = {"rising": "warn", "falling": "ok"}.get(report.trend, "muted")
    out.append(kv("BASE FEE", f"{gwei(report.base_fee_wei):.4f} gwei"))
    out.append(line(
        seg("next block".ljust(14), "muted"),
        seg(f" {gwei(report.next_base_fee_wei):.4f} gwei  ", "value"),
        seg(report.trend, tone),
        seg(f"   · blocks {report.congestion * 100:.0f}% full", "muted"),
    ))
    if report.native_usd:
        out.append(kv(f"{report.native_symbol}/USD", fmt_price(report.native_usd)))
    elif report.price_error:
        out.append(kv(f"{report.native_symbol}/USD", report.price_error, style="warn"))
    out.append(line(seg("─" * 64, "rule")))
    out.append(line(seg(
        f"{'tier':<10}{'tip (gwei)':>13}{'max (gwei)':>13}{'transfer':>12}{'swap':>12}",
        "column",
    )))
    for tier in report.tiers:
        transfer = swap = "—"
        if report.native_usd:
            transfer = f"${tier.cost_wei(GAS_TRANSFER) / 1e18 * report.native_usd:,.4f}"
            swap = f"${tier.cost_wei(GAS_SWAP) / 1e18 * report.native_usd:,.4f}"
        out.append(line(
            seg(tier.label.ljust(10), "key"),
            seg(f"{gwei(tier.priority_fee_wei):>13.4f}{gwei(tier.max_fee_wei):>13.4f}"
                f"{transfer:>12}{swap:>12}", "number"),
        ))
    return out + footer_hint()


def render_ccip(network: str, payload: Any, error: Optional[str] = None) -> List[Line]:
    out = title_line(f"CCIP LANES — {network.upper()}", _endpoint(network))
    if error:
        return out + error_lines(error)
    lanes = payload or []
    if not lanes:
        return out + [
            line(seg(f"No verified CCIP router for {network}.", "warn")),
            note_line(f"routers: {', '.join(sorted(ROUTERS))}"),
        ] + footer_hint()

    out.append(note_line(f"router {ROUTERS.get(network, '')}"))
    for lane in lanes:
        if lane.error:
            state, tone = "error", "bad"
        elif lane.supported:
            state, tone = "open", "ok"
        else:
            state, tone = "closed", "muted"
        out.append(line(
            seg(lane.destination.ljust(13), "key"),
            seg(str(lane.destination_selector).ljust(23), "muted"),
            seg(state, tone),
        ))
    out.append(line(seg("─" * 64, "rule")))
    out.append(note_line("selectors are not chain ids — passing a chain id here reverts"))
    return out + footer_hint()


def render_analytics(network: str, payload: Any, error: Optional[str] = None) -> List[Line]:
    """Per-feed statistics over recent round history, with a sparkline of the window."""
    out = title_line(f"PRICE ANALYTICS — {network.upper()}", _endpoint(network))
    if error:
        return out + error_lines(error)
    entries = payload or []
    if not entries:
        return out + [line(seg("No history could be read on this network.", "warn"))]

    from .term.widgets import sparkline

    for stats, prices in entries:
        if stats.samples < 2:
            out.append(line(seg(stats.pair.ljust(12), "key"),
                            seg("not enough history", "warn")))
            continue
        change_tone = "ok" if (stats.change_pct or 0) >= 0 else "bad"
        out.append(line(
            seg(stats.pair.ljust(12), "key"),
            seg(fmt_price(stats.last or 0).rjust(14) + "  ", "number"),
            seg(f"{stats.change_pct:+6.2f}%  ", change_tone),
            seg(sparkline(prices, 24) + "  ", "spark"),
            seg(f"{stats.samples} rounds over {fmt_age(stats.span_secs)}", "muted"),
        ))
        divergence = stats.twap_divergence_bps
        out.append(line(
            seg("    twap ", "muted"),
            seg(f"{fmt_price(stats.twap or 0)}", "value"),
            seg(f"  ({divergence:+.0f} bps from spot)" if divergence is not None else "",
                "warn" if divergence is not None and abs(divergence) > 100 else "muted"),
            seg(f"   vol {stats.volatility_annual * 100:.1f}%/yr"
                if stats.volatility_annual else "", "muted"),
            seg(f"   max dd {stats.max_drawdown_pct:.2f}%", "muted"),
        ))
    out.append(line(seg("─" * 64, "rule")))
    out.append(note_line(
        "TWAP is time-weighted — an oracle publishes more often when the price moves, so "
        "the mean of the answers over-weights volatile periods"
    ))
    return out + footer_hint()


# ── offline panels ───────────────────────────────────────────────────────────


def render_simulate(network: str = "", payload: Any = None,
                    error: Optional[str] = None) -> List[Line]:
    """Which oracle failure modes each guard preset actually defends against.

    Offline and instant — this is arithmetic over fixed scenarios, not a chain read.
    """
    out = title_line("GUARD SIMULATION", "replayed against known oracle failure modes")
    presets = [("naive", Guard.naive()), ("default", Guard()), ("strict", Guard.strict())]
    results = [(name, audit_guard(guard)) for name, guard in presets]

    out.append(line(
        seg("scenario".ljust(20), "column"),
        *[seg(name.ljust(10), "column") for name, _ in results],
    ))
    for scenario_name, scenario in SCENARIOS.items():
        cells: List[Segment] = [seg(scenario_name.ljust(20), "key")]
        for _, result in results:
            report = next(r for r in result.reports if r.name == scenario_name)
            handled = report.caught == scenario.should_catch
            if scenario.should_catch:
                label = "caught" if handled else "MISSED"
            else:
                label = "clean" if handled else "REJECTS"
            cells.append(seg(label.ljust(10), "ok" if handled else "bad"))
        out.append(line(*cells))

    out.append(line(seg("─" * 64, "rule")))
    for name, result in results:
        tone = "ok" if result.score == 1.0 else ("warn" if result.score >= 0.5 else "bad")
        out.append(line(
            seg(name.ljust(20), "key"),
            seg(f"{result.score * 100:.0f}% of scenarios handled", tone),
            seg(f"   gaps: {', '.join(result.failed)}" if result.failed else "", "muted"),
        ))

    out.append(blank())
    for scenario in SCENARIOS.values():
        out.append(line(seg(scenario.name, "subtitle")))
        out.append(line(seg("    " + scenario.summary, "value")))
        out.append(note_line("    " + scenario.expectation))
    return out + footer_hint()


def render_registry(network: str = "", payload: Any = None,
                    error: Optional[str] = None) -> List[Line]:
    """Registry coverage: how many feeds per chain, and how many heartbeats are measured."""
    out = title_line("REGISTRY COVERAGE", f"{feed_count()} feeds across {len(list_networks())} networks")
    out.append(line(
        seg("network".ljust(12), "column"), seg("feeds".rjust(6), "column"),
        seg("measured".rjust(10), "column"), seg("bounded".rjust(9), "column"),
        seg("fastest".rjust(10), "column"), seg("slowest".rjust(10), "column"),
        seg("  tags", "column"),
    ))
    for name, entry in coverage().items():
        if not entry.get("feeds"):
            out.append(line(seg(name.ljust(12), "key"), seg("0".rjust(6), "muted")))
            continue
        tags = []
        if entry.get("testnet"):
            tags.append("testnet")
        if entry.get("layer2"):
            tags.append("L2")
        bounded = int(entry["bounded"])
        out.append(line(
            seg(name.ljust(12), "key"),
            seg(str(entry["feeds"]).rjust(6), "number"),
            seg(str(entry["measured"]).rjust(10), "ok"),
            seg(str(bounded).rjust(9), "warn" if bounded else "muted"),
            seg(fmt_secs(int(entry["fastest_secs"])).rjust(10), "number"),
            seg(fmt_secs(int(entry["slowest_secs"])).rjust(10), "number"),
            seg("  " + ", ".join(tags), "muted"),
        ))
    out.append(line(seg("─" * 64, "rule")))
    out.append(note_line(
        "a bounded heartbeat is a conservative upper limit, not a measurement — its "
        "staleness verdict fires later than a measured one would"
    ))
    return out + footer_hint()


def render_chainlink(*_args, **_kwargs) -> List[Line]:
    out = title_line("CHAINLINK CAPABILITIES")
    for name, entry in summarize_chainlink_capabilities().items():
        out.append(line(
            seg(name.replace("_", " ").upper().ljust(24), "key"),
            seg("read live" if entry["verified_live"] else "not read",
                "ok" if entry["verified_live"] else "muted"),
        ))
        out.append(line(seg("    " + entry["detail"], "value")))
        if entry["commands"]:
            out.append(note_line("    commands: " + ", ".join(entry["commands"])))
    return out + footer_hint()


def render_alchemy(*_args, **_kwargs) -> List[Line]:
    summary = summarize_alchemy_capabilities()
    out = title_line("ALCHEMY CAPABILITIES")
    out.append(kv("ENDPOINT", summary["endpoint"]))
    out.append(kv("SOURCE", summary["source"]))
    out.append(kv("AUTHENTICATED", "yes" if summary["authenticated"] else "no"))
    out.append(line(seg("─" * 64, "rule")))
    for feature in summary["features"]:
        out.append(line(
            seg(("available" if feature["available"] else "needs key").ljust(11),
                "ok" if feature["available"] else "muted"),
            seg(feature["method"], "key"),
        ))
        out.append(line(seg("    " + feature["capability"], "value")))
    if summary["hint"]:
        out.append(blank())
        out.append(note_line(summary["hint"]))
    return out + footer_hint()


def render_integration(*_args, **_kwargs) -> List[Line]:
    out = title_line("INTEGRATION MAP")
    for domain, entry in build_integration_map().items():
        out.append(line(seg(domain.upper().replace("_", " "), "subtitle")))
        out.append(kv("  Alchemy", entry["alchemy"]))
        out.append(kv("  Chainlink", entry["chainlink"]))
        out.append(kv("  Composed", entry["composed"]))
        out.append(line(seg("    " + "  ".join("$ " + c for c in entry["commands"]), "ok")))
        out.append(blank())
    return out + footer_hint()


def render_blueprint(*_args, **_kwargs) -> List[Line]:
    blueprint = build_package_blueprint()
    out = title_line("BLUEPRINT")
    out.append(line(seg(blueprint["project"], "value")))
    out.append(line(seg("─" * 64, "rule")))
    out.append(line(seg("COVERAGE", "key")))
    for name, count in blueprint["coverage"].items():
        out.append(line(seg(f"{count:>6}  ", "warn"), seg(name.replace("_", " "), "value")))
    out.append(blank())
    out.append(line(seg("GUARANTEES", "key")))
    for index, claim in enumerate(blueprint["guarantees"], 1):
        out.append(line(seg(f"{index:>4}. ", "warn"), seg(claim, "value")))
    out.append(blank())
    out.append(line(seg("DEPENDENCIES", "key")))
    for name, text in blueprint["dependencies"].items():
        out.append(kv(f"  {name}", str(text)))
    return out + footer_hint()


def render_recipes(selected: Optional[str] = None) -> List[Line]:
    if selected:
        recipe = get_recipe_by_id(selected)
        if recipe:
            out = title_line(recipe["name"].upper())
            out.append(line(seg(recipe["summary"], "muted")))
            out.append(blank())
            out.append(line(seg("STEPS", "key")))
            for index, step in enumerate(recipe["steps"], 1):
                out.append(line(seg(f"{index:>4}. ", "warn"), seg(step, "value")))
            out.append(blank())
            out.append(line(seg("  " + "  ".join("#" + t for t in recipe["tags"]), "ok")))
            return out + [blank(), note_line("esc to go back")]

    out = title_line("RECIPES")
    for recipe in get_recipes():
        out.append(line(seg(recipe["name"], "subtitle")))
        out.append(line(seg("  " + recipe["summary"], "muted")))
        out.append(line(seg("  " + "  ".join("#" + t for t in recipe["tags"]), "ok")))
        out.append(blank())
    return out + footer_hint()


def render_about(app: Optional["Dashboard"] = None) -> List[Line]:
    """Terminal capabilities and frame statistics — what the boot layer negotiated."""
    out = title_line(f"ALCHEM-LINK v{__version__}", "terminal and session diagnostics")
    info = boot.describe()
    out.append(line(seg("TERMINAL", "key")))
    for name, value in info.items():
        out.append(kv("  " + name.replace("_", " "), str(value)))
    if app is not None:
        out.append(blank())
        out.append(line(seg("SESSION", "key")))
        for name, value in app.stats().items():
            out.append(kv("  " + name.replace("_", " "), str(value)))
    out.append(blank())
    out.append(line(seg("PALETTE", "key")))
    from .theme import PALETTE

    for name, value in PALETTE.items():
        swatch = Style(fg=value, bg=value)
        out.append(line(seg("  " + name.ljust(12), "muted"), ("████", swatch),
                        seg("  " + value, "hint")))
    out.append(blank())
    out.append(note_line(
        "no third-party packages are involved in any of the above — screen diffing, "
        "colour negotiation and input parsing are all in alchem_link.term"
    ))
    return out + footer_hint()


# ── panel wiring ─────────────────────────────────────────────────────────────


class PanelDef:
    """One sidebar entry: its label, whether it needs a chain, and how to fill it."""

    def __init__(self, key: str, label: str, render: Callable,
                 fetch: Optional[Callable[[str], Any]] = None,
                 loading: str = "", global_scope: bool = False) -> None:
        self.key = key
        self.label = label
        self.render = render
        self.fetch = fetch
        self.loading = loading
        #: True when the result does not depend on the selected network, so switching
        #: chains must not discard it or trigger a refetch.
        self.global_scope = global_scope

    @property
    def live(self) -> bool:
        return self.fetch is not None


def _endpoint(network: str) -> str:
    try:
        endpoint = resolve_endpoint(network=network)
        return f"{endpoint.redacted()}  ({endpoint.source})"
    except Exception:  # pragma: no cover - a bad network key is caught upstream
        return ""


def _fetch_analytics(network: str):
    """Round history for every feed on a network, summarised.

    Sequential on purpose. Each feed's history is already one batched Multicall3 round
    trip, and firing a dozen of those concurrently at a public endpoint is the reliable
    way to earn a 429 — which would render as "no history" and look like a bug in the
    tool rather than politeness toward the provider.
    """
    from .aggregator import round_history
    from .analytics import Series, summarise

    out = []
    for feed in list_feeds(network):
        try:
            rounds = round_history(feed.address, count=24, network=network)
        except Exception:
            continue
        series = Series.from_rounds(rounds, feed.pair, network)
        out.append((summarise(series), series.prices))
    return out


PANELS: List[PanelDef] = [
    PanelDef("feeds", "Live Feeds", render_feeds,
             lambda n: read_all_feeds(network=n), "Reading aggregators…"),
    PanelDef("audit", "Safety Audit", render_audit,
             lambda n: audit_network(network=n),
             "Auditing every feed — proxy resolution and bounds…"),
    PanelDef("analytics", "Analytics", render_analytics, _fetch_analytics,
             "Walking round history for every feed…"),
    PanelDef("divergence", "Cross-chain", render_divergence,
             lambda n: compare_all(), "Reading the same pairs on every chain…",
             global_scope=True),
    PanelDef("sequencer", "L2 Sequencer", render_sequencer,
             lambda n: [
                 status for status in (read_sequencer(net) for net in sorted(SEQUENCER_FEEDS))
                 if status is not None
             ], "Reading uptime feeds…", global_scope=True),
    PanelDef("gas", "Gas", render_gas, lambda n: analyse_gas(network=n),
             "Sampling fee history…"),
    PanelDef("ccip", "CCIP Lanes", render_ccip,
             lambda n: verify_lanes(n) if n in ROUTERS else [],
             "Probing lanes via isChainSupported…"),
    PanelDef("simulate", "Simulation", render_simulate),
    PanelDef("registry", "Registry", render_registry),
    PanelDef("chainlink", "Chainlink", render_chainlink),
    PanelDef("alchemy", "Alchemy", render_alchemy),
    PanelDef("integration", "Integration", render_integration),
    PanelDef("blueprint", "Blueprint", render_blueprint),
    PanelDef("recipes", "Recipes", render_recipes),
    PanelDef("about", "Terminal", render_about),
]

PANELS_BY_KEY = {p.key: p for p in PANELS}

#: Sidebar width. Wide enough for the longest label plus its marker, and no wider — the
#: panel to the right is where the information is.
SIDEBAR_WIDTH = 18


class Dashboard(App):
    """The Alchem-Link developer console."""

    title = f"Alchem-Link v{__version__}"
    tick = 0.12

    def __init__(self, network: str = DEFAULT_NETWORK, **kwargs) -> None:
        super().__init__(**kwargs)
        self.network = network
        self.active = 0
        self.scroll = Scroll()
        self.recipe: Optional[str] = None
        self.focus_sidebar = True

    # ── state ────────────────────────────────────────────────────────────────

    @property
    def panel(self) -> PanelDef:
        return PANELS[self.active]

    @property
    def job_key(self) -> str:
        """Cache key for the current panel. Global panels ignore the network."""
        return f"{self.panel.key}:{'*' if self.panel.global_scope else self.network}"

    def on_start(self) -> None:
        boot.install_signal_handlers(lambda _signum: self.quit())
        self._ensure_loaded()

    def _ensure_loaded(self) -> None:
        if self.panel.live and self.job(self.job_key) is None:
            key, fetch, network = self.job_key, self.panel.fetch, self.network
            self.submit(key, lambda: fetch(network))

    def on_job(self, job: Job) -> None:
        if job.error:
            self.notify(f"{job.key.split(':')[0]}: {job.error[:60]}", 5.0)

    # ── content ──────────────────────────────────────────────────────────────

    def lines(self) -> List[Line]:
        current = self.panel
        if current.key == "recipes":
            return render_recipes(self.recipe)
        if current.key == "about":
            return render_about(self)
        if not current.live:
            return current.render()

        job = self.job(self.job_key)
        if job is None or not job.done:
            heading = current.label.upper()
            if not current.global_scope:
                heading += f" — {self.network.upper()}"
            elapsed = f"  ({job.elapsed:.1f}s)" if job else ""
            return title_line(heading) + [
                line(seg(current.loading + elapsed, "warn")),
            ]
        return current.render(self.network, job.value, job.error)

    # ── painting ─────────────────────────────────────────────────────────────

    def render(self, screen: Screen) -> None:
        root = self.rect
        header, rest = root.split_v(1)
        body, footer = rest.split_v(-1)
        rail, main = body.split_h(SIDEBAR_WIDTH)

        self._paint_header(screen, header)
        sidebar(screen, rail, [p.label for p in PANELS], self.active, title="ALCHEM-LINK")
        self._paint_main(screen, main)
        self._paint_footer(screen, footer)

    def _paint_header(self, screen: Screen, rect: Rect) -> None:
        screen.fill(rect.row, rect.column, rect.width, role("header"))
        screen.put(rect.row, rect.column + 1, f" {self.title} ", role("header"))
        right = f"{self.network}  ·  {feed_count()} feeds  ·  {ansi.Depth.name(self.depth)} "
        screen.put(rect.row, max(0, rect.right - len(right)), right, role("header"))

    def _paint_main(self, screen: Screen, rect: Rect) -> None:
        inner = panel(screen, rect, self.panel.label, focused=not self.focus_sidebar)
        if inner.is_empty:
            return
        content = inner.inset(right=1)  # leave the last column for the scrollbar
        rows = self.lines()
        self.scroll.clamp(len(rows), content.height)

        visible = rows[self.scroll.offset:self.scroll.offset + content.height]
        for offset, row in enumerate(visible):
            x = content.column
            for text, style in row:
                if x >= content.right:
                    break
                x += screen.put(content.row + offset, x, text, style,
                                max_width=content.right - x)
        scrollbar(screen, Rect(content.row, inner.right - 1, 1, content.height),
                  total=len(rows), offset=self.scroll.offset, height=content.height)

    def _paint_footer(self, screen: Screen, rect: Rect) -> None:
        job = self.job(self.job_key)
        if self.notice:
            left = self.notice
        elif job is not None and not job.done:
            left = f"loading… {job.elapsed:.1f}s"
        else:
            left = f"{self.panel.label} · {self.network}"
        status_bar(screen, rect, left,
                   "↑↓/jk scroll · tab pane · n network · r refresh · q quit")

    # ── input ────────────────────────────────────────────────────────────────

    def on_key(self, key) -> bool:
        name = key.name
        rows = len(self.lines())
        height = max(1, self.screen.height - 4)

        if name == "tab":
            self.focus_sidebar = not self.focus_sidebar
            return True
        # Shift-n arrives as the literal "N" — terminals report the shifted character,
        # not a modifier flag, for ordinary letter keys.
        if name == "n":
            self._next_network(1)
            return True
        if name == "N":
            self._next_network(-1)
            return True
        if name == "r":
            self.forget(self.job_key)
            self._ensure_loaded()
            self.notify("refreshing…", 2.0)
            return True

        if self.focus_sidebar:
            if name in ("down", "j"):
                self._select(self.active + 1)
                return True
            if name in ("up", "k"):
                self._select(self.active - 1)
                return True
            if name in ("enter", "right", "l"):
                self.focus_sidebar = False
                return True
        else:
            if name in ("down", "j"):
                self.scroll.move(1, rows, height)
                return True
            if name in ("up", "k"):
                self.scroll.move(-1, rows, height)
                return True
            if name == "pagedown":
                self.scroll.page(1, rows, height)
                return True
            if name == "pageup":
                self.scroll.page(-1, rows, height)
                return True
            if name == "home":
                self.scroll.home(rows, height)
                return True
            if name == "end":
                self.scroll.end(rows, height)
                return True
            if name in ("left", "h"):
                self.focus_sidebar = True
                return True

        if name == "escape" and self.recipe:
            self.recipe = None
            return True
        if name.isdigit() and name != "0":
            index = int(name) - 1
            if index < len(PANELS):
                self._select(index)
                return True
        return False

    def _select(self, index: int) -> None:
        self.active = max(0, min(len(PANELS) - 1, index))
        self.scroll = Scroll()
        self.recipe = None
        self._ensure_loaded()

    def _next_network(self, step: int) -> None:
        keys = [n.key for n in list_networks()]
        self.network = keys[(keys.index(self.network) + step) % len(keys)]
        self.scroll = Scroll()
        self._ensure_loaded()
        self.notify(f"network → {self.network}", 2.0)


def launch(network: str = DEFAULT_NETWORK) -> int:
    """Run the dashboard. The entry point behind ``alchem-link-ui``."""
    return Dashboard(network=network).run()


if __name__ == "__main__":
    raise SystemExit(launch())
