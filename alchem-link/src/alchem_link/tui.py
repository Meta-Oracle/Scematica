"""Alchem-Link TUI — a terminal console over the live toolkit.

Every panel that talks to a chain does so on a worker thread. An RPC round trip is
hundreds of milliseconds on a public endpoint and a cross-chain divergence sweep is
several seconds; doing either on the UI thread freezes the app mid-render, which reads
as a crash. Panels render a loading state first and repaint when the worker lands.

Results are cached per panel until ``r``, so switching tabs is instant and does not
re-hit the network. A network switch mid-flight discards the in-flight result rather
than painting one chain's data under another chain's heading.
"""
from __future__ import annotations

from textual import work
from textual.app import App, ComposeResult
from textual.binding import Binding
from textual.containers import Horizontal, ScrollableContainer, Vertical
from textual.widgets import Footer, Header, Label, ListItem, ListView, Rule, Static

from . import __version__
from .ccip import ROUTERS, summarize_chainlink_capabilities, verify_lanes
from .divergence import compare_all
from .enhanced import summarize_alchemy_capabilities
from .feeds import feed_count, read_all_feeds
from .gas import GAS_SWAP, GAS_TRANSFER, analyse_gas
from .integration import build_integration_map, build_package_blueprint
from .networks import DEFAULT_NETWORK, get_network, list_networks, resolve_endpoint
from .recipes import get_recipe_by_id, get_recipes
from .rpc import gwei
from .safety import audit_network
from .sequencer import SEQUENCER_FEEDS, read_sequencer
from .theme import (
    AMBER,
    BLUE,
    CSS,
    GREEN,
    MUTED,
    RED,
    STATUS_COLOUR,
    TEXT,
    hint,
    key,
    title,
)

NAV_ITEMS = [
    ("live",        "◉  Live Feeds"),
    ("audit",       "⚠  Safety Audit"),
    ("divergence",  "⇄  Cross-chain"),
    ("sequencer",   "⬢  L2 Sequencer"),
    ("gas",         "⛽ Gas"),
    ("ccip",        "⬡  CCIP Lanes"),
    ("chainlink",   "◈  Chainlink"),
    ("alchemy",     "◈  Alchemy"),
    ("integration", "⇢  Integration"),
    ("blueprint",   "⬡  Blueprint"),
    ("recipes",     "✦  Recipes"),
]

#: Panels that need a chain. Everything else renders instantly from local tables.
LIVE_PANELS = {"live", "audit", "divergence", "sequencer", "gas", "ccip"}

SEVERITY_COLOUR = {
    "critical": RED,
    "high": RED,
    "medium": AMBER,
    "low": MUTED,
    "info": MUTED,
    "ok": GREEN,
}


def _fmt_price(value: float) -> str:
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
    for unit, size in (("d", 86400), ("h", 3600), ("m", 60)):
        if seconds % size == 0:
            return f"{seconds // size}{unit}"
    return f"{seconds}s"


def _kv(name: str, text: str) -> list:
    return [
        Static(key(name), classes="card-key"),
        Static(f"[{TEXT}]{text}[/]", classes="card-value"),
    ]


def _header(icon: str, text: str, network: str | None = None) -> list:
    widgets = [Label(f"{icon}  {text}", classes="section-title")]
    if network:
        endpoint = resolve_endpoint(network=network)
        widgets.append(
            Static(hint(f"{endpoint.redacted()}  ({endpoint.source})"), classes="dim")
        )
    widgets.append(Rule())
    return widgets


def _loading(icon: str, text: str, network: str | None, note: str) -> list:
    return _header(icon, text, network) + [Static(f"[{AMBER}]{note}[/]")]


def _error(message: str) -> list:
    return [
        Static(f"[{RED}]{message}[/]"),
        Static(hint("press [bold]r[/] to retry · [bold]n[/] to switch network"), classes="dim"),
    ]


def _footer_hint() -> Static:
    return Static(
        hint("press [bold]r[/] to refresh · [bold]n[/] to switch network"), classes="dim"
    )


# ── live panels ──────────────────────────────────────────────────────────────────


def _render_live(network: str, payload, error: str | None = None) -> list:
    widgets = _header("◉", f"LIVE FEEDS — {network.upper()}", network)
    if error:
        return widgets + _error(error)
    readings = payload or []
    if not readings:
        return widgets + [Static(f"[{AMBER}]No feeds could be read on this network.[/]")]

    width = max(len(r.pair) for r in readings)
    for r in readings:
        colour = STATUS_COLOUR[r.status]
        bound = "" if r.heartbeat_measured else "*"
        widgets.append(Static(
            f"  [bold {BLUE}]{r.pair:<{width}}[/]  "
            f"[{TEXT}]{_fmt_price(r.price):>16}[/]  "
            f"[bold {colour}]{r.status:<7}[/]  "
            f"[{MUTED}]{_fmt_age(r.age_secs):>10} ago · hb {_fmt_secs(r.heartbeat_secs)}{bound}[/]"
        ))
        if r.note:
            widgets.append(Static(f"      [{AMBER}]{r.note}[/]", classes="dim"))
    widgets.append(Rule())
    stale = sum(1 for r in readings if r.stale)
    widgets.append(Static(
        f"  [{MUTED}]{len(readings)} feeds · {len(readings) - stale} fresh · "
        f"{stale} past heartbeat[/]"
    ))
    if any(not r.heartbeat_measured for r in readings):
        widgets.append(Static(
            hint("* heartbeat is a conservative bound — no quiet period observed in sampling"),
            classes="dim",
        ))
    widgets.append(_footer_hint())
    return widgets


def _render_audit(network: str, payload, error: str | None = None) -> list:
    widgets = _header("⚠", f"SAFETY AUDIT — {network.upper()}", network)
    if error:
        return widgets + _error(error)
    audits = payload or []
    if not audits:
        return widgets + [Static(f"[{AMBER}]No feeds to audit on this network.[/]")]

    for audit in audits:
        colour = SEVERITY_COLOUR.get(audit.worst, MUTED)
        price = f"  [{TEXT}]{_fmt_price(audit.price)}[/]" if audit.price is not None else ""
        widgets.append(Static(
            f"  [bold {BLUE}]{audit.pair:<11}[/]{price}  [bold {colour}]{audit.worst.upper()}[/]"
        ))
        for finding in audit.sorted_findings:
            if finding.severity == "info":
                continue
            tone = SEVERITY_COLOUR[finding.severity]
            widgets.append(Static(
                f"      [{tone}]{finding.severity.upper():<8}[/] [{TEXT}]{finding.code}[/] — "
                f"[{MUTED}]{finding.title}[/]"
            ))
    widgets.append(Rule())
    unsafe = sum(1 for a in audits if not a.safe_to_consume)
    tone = GREEN if unsafe == 0 else RED
    widgets.append(Static(
        f"  [bold {tone}]{len(audits) - unsafe}/{len(audits)} feeds safe to consume[/]"
    ))
    widgets.append(_footer_hint())
    return widgets


def _render_divergence(network: str, payload, error: str | None = None) -> list:
    widgets = _header("⇄", "CROSS-CHAIN DIVERGENCE")
    if error:
        return widgets + _error(error)
    reports = payload or []
    if not reports:
        return widgets + [Static(f"[{AMBER}]No multi-chain pairs to compare.[/]")]

    for report in reports:
        tone = RED if report.verdict == "diverged" else GREEN
        consensus = _fmt_price(report.consensus) if report.consensus else "—"
        widgets.append(Static(
            f"  [bold {BLUE}]{report.pair:<10}[/] [bold {tone}]{report.verdict.upper():<12}[/] "
            f"[{MUTED}]spread {report.spread_bps:6.1f} bps · consensus {consensus}[/]"
        ))
        for entry in sorted(report.legs, key=lambda leg: -abs(leg.deviation_bps))[:4]:
            if entry.error:
                widgets.append(Static(f"      [{RED}]{entry.network:<10} unreadable[/]", classes="dim"))
                continue
            mark = f"[{AMBER}]STALE[/]" if entry.stale else "     "
            widgets.append(Static(
                f"      [{MUTED}]{entry.network:<10}[/] [{TEXT}]{_fmt_price(entry.price):>14}[/]  "
                f"[{MUTED}]{entry.deviation_bps:+7.1f} bps[/]  {mark}",
                classes="dim",
            ))
    widgets.append(Rule())
    widgets.append(Static(hint("testnets are excluded — their feeds carry unrelated data"), classes="dim"))
    widgets.append(_footer_hint())
    return widgets


def _render_sequencer(network: str, payload, error: str | None = None) -> list:
    widgets = _header("⬢", "L2 SEQUENCER UPTIME")
    if error:
        return widgets + _error(error)
    statuses = payload or []
    if not statuses:
        return widgets + [Static(f"[{AMBER}]No uptime feeds registered.[/]")]

    for status in statuses:
        tone = {"UP": GREEN, "GRACE": AMBER, "DOWN": RED}.get(status.state, MUTED)
        widgets.append(Static(
            f"  [bold {tone}]{status.state:<7}[/] [bold {BLUE}]{status.network:<10}[/] "
            f"[{MUTED}]{status.address}[/]"
        ))
        widgets.append(Static(f"          [{TEXT}]{status.detail}[/]", classes="dim"))
    widgets.append(Rule())
    widgets.append(Static(
        hint("a price feed keeps answering while the sequencer is down — with a frozen price"),
        classes="dim",
    ))
    widgets.append(_footer_hint())
    return widgets


def _render_gas(network: str, payload, error: str | None = None) -> list:
    widgets = _header("⛽", f"GAS — {network.upper()}", network)
    if error:
        return widgets + _error(error)
    if payload is None:
        return widgets + [Static(f"[{AMBER}]No fee data.[/]")]

    report = payload
    tone = {"rising": AMBER, "falling": GREEN}.get(report.trend, MUTED)
    widgets += _kv("BASE FEE", f"{gwei(report.base_fee_wei):.4f} gwei")
    widgets.append(Static(
        f"  [{MUTED}]next block[/] [{TEXT}]{gwei(report.next_base_fee_wei):.4f} gwei[/] "
        f"[bold {tone}]{report.trend}[/]  [{MUTED}]· blocks {report.congestion * 100:.0f}% full[/]"
    ))
    if report.native_usd:
        widgets += _kv(f"{report.native_symbol}/USD", _fmt_price(report.native_usd))
    widgets.append(Rule())
    widgets.append(Static(
        f"  [{MUTED}]{'tier':<10}{'tip (gwei)':>13}{'max (gwei)':>13}"
        f"{'transfer':>12}{'swap':>12}[/]"
    ))
    for tier in report.tiers:
        transfer = swap = "—"
        if report.native_usd:
            transfer = f"${tier.cost_wei(GAS_TRANSFER) / 1e18 * report.native_usd:,.4f}"
            swap = f"${tier.cost_wei(GAS_SWAP) / 1e18 * report.native_usd:,.4f}"
        widgets.append(Static(
            f"  [bold {BLUE}]{tier.label:<10}[/][{TEXT}]{gwei(tier.priority_fee_wei):>13.4f}"
            f"{gwei(tier.max_fee_wei):>13.4f}{transfer:>12}{swap:>12}[/]"
        ))
    widgets.append(_footer_hint())
    return widgets


def _render_ccip(network: str, payload, error: str | None = None) -> list:
    widgets = _header("⬡", f"CCIP LANES — {network.upper()}", network)
    if error:
        return widgets + _error(error)
    lanes = payload or []
    if not lanes:
        return widgets + [
            Static(f"[{AMBER}]No verified CCIP router for {network}.[/]"),
            Static(hint(f"routers: {', '.join(sorted(ROUTERS))}"), classes="dim"),
            _footer_hint(),
        ]

    widgets.append(Static(f"  [{MUTED}]router {ROUTERS.get(network, '')}[/]", classes="dim"))
    for lane in lanes:
        if lane.error:
            state, tone = "error", RED
        elif lane.supported:
            state, tone = "open", GREEN
        else:
            state, tone = "closed", MUTED
        widgets.append(Static(
            f"  [bold {BLUE}]{lane.destination:<12}[/] "
            f"[{MUTED}]{lane.destination_selector:<22}[/] [bold {tone}]{state}[/]"
        ))
    widgets.append(Rule())
    widgets.append(Static(
        hint("selectors are not chain ids — passing a chain id here reverts"), classes="dim"
    ))
    widgets.append(_footer_hint())
    return widgets


# ── reference panels (offline, instant) ──────────────────────────────────────────


def _render_chainlink() -> list:
    widgets = _header("◈", "CHAINLINK CAPABILITIES")
    for name, entry in summarize_chainlink_capabilities().items():
        tone = GREEN if entry["verified_live"] else MUTED
        mark = "read live" if entry["verified_live"] else "not read"
        widgets.append(Static(
            f"  [bold {BLUE}]{name.replace('_', ' ').upper()}[/]  [bold {tone}]{mark}[/]"
        ))
        widgets.append(Static(f"      [{TEXT}]{entry['detail']}[/]", classes="dim"))
        if entry["commands"]:
            widgets.append(Static(
                f"      [{MUTED}]commands: {', '.join(entry['commands'])}[/]", classes="dim"
            ))
    return widgets


def _render_alchemy() -> list:
    summary = summarize_alchemy_capabilities()
    widgets = _header("◈", "ALCHEMY CAPABILITIES")
    widgets += _kv("ENDPOINT", summary["endpoint"])
    widgets += _kv("SOURCE", summary["source"])
    widgets += _kv("AUTHENTICATED", "yes" if summary["authenticated"] else "no")
    widgets.append(Rule())
    for feature in summary["features"]:
        tone = GREEN if feature["available"] else MUTED
        mark = "available" if feature["available"] else "needs key"
        widgets.append(Static(
            f"  [bold {tone}]{mark:<10}[/] [bold {BLUE}]{feature['method']}[/]"
        ))
        widgets.append(Static(f"      [{TEXT}]{feature['capability']}[/]", classes="dim"))
    if summary["hint"]:
        widgets.append(Rule())
        widgets.append(Static(hint(summary["hint"]), classes="dim"))
    return widgets


def _render_integration() -> list:
    widgets = _header("⇢", "INTEGRATION MAP")
    for domain, entry in build_integration_map().items():
        widgets.append(Static(title(domain.upper().replace("_", " "))))
        widgets += _kv("  Alchemy", entry["alchemy"])
        widgets += _kv("  Chainlink", entry["chainlink"])
        widgets += _kv("  Composed", entry["composed"])
        widgets.append(Static(
            f"      [{GREEN}]{'  '.join('$ ' + c for c in entry['commands'])}[/]", classes="dim"
        ))
        widgets.append(Rule())
    return widgets


def _render_blueprint() -> list:
    blueprint = build_package_blueprint()
    widgets = _header("⬡", "BLUEPRINT")
    widgets.append(Static(f"[{TEXT}]{blueprint['project']}[/]", classes="card-value"))
    widgets.append(Rule())
    widgets.append(Static(key("COVERAGE"), classes="card-key"))
    for name, count in blueprint["coverage"].items():
        widgets.append(Static(
            f"  [{AMBER}]{count:>4}[/]  [{TEXT}]{name.replace('_', ' ')}[/]"
        ))
    widgets.append(Rule())
    widgets.append(Static(key("GUARANTEES"), classes="card-key"))
    for index, claim in enumerate(blueprint["guarantees"], 1):
        widgets.append(Static(f"  [{AMBER}]{index}.[/] [{TEXT}]{claim}[/]"))
    widgets.append(Rule())
    widgets.append(Static(key("DEPENDENCIES"), classes="card-key"))
    for name, text in blueprint["dependencies"].items():
        widgets += _kv(f"  {name}", str(text))
    return widgets


def _render_recipes(selected_id: str | None = None) -> list:
    if selected_id:
        recipe = get_recipe_by_id(selected_id)
        if recipe:
            return _render_recipe_detail(recipe)
    widgets = _header("✦", "RECIPES")
    for recipe in get_recipes():
        widgets.append(Static(title(recipe["name"])))
        widgets.append(Static(f"[{MUTED}]{recipe['summary']}[/]", classes="recipe-summary"))
        widgets.append(Static(
            "  " + "  ".join(f"[{GREEN}]#{tag}[/]" for tag in recipe["tags"]), classes="card-tag"
        ))
        widgets.append(Rule())
    return widgets


def _render_recipe_detail(recipe: dict) -> list:
    widgets = [
        Label(f"✦  {recipe['name'].upper()}", classes="section-title"),
        Static(f"[{MUTED}]{recipe['summary']}[/]", classes="recipe-summary"),
        Rule(),
        Static(key("STEPS"), classes="card-key"),
    ]
    for index, step in enumerate(recipe["steps"], 1):
        widgets.append(Static(f"  [bold {AMBER}]{index}.[/] [{TEXT}]{step}[/]"))
    widgets.append(Rule())
    widgets.append(Static(
        "  " + "  ".join(f"[{GREEN}]#{tag}[/]" for tag in recipe["tags"]), classes="card-tag"
    ))
    return widgets


LIVE_RENDERERS = {
    "live": _render_live,
    "audit": _render_audit,
    "divergence": _render_divergence,
    "sequencer": _render_sequencer,
    "gas": _render_gas,
    "ccip": _render_ccip,
}

REFERENCE_RENDERERS = {
    "chainlink": _render_chainlink,
    "alchemy": _render_alchemy,
    "integration": _render_integration,
    "blueprint": _render_blueprint,
}

LOADING_NOTES = {
    "live": ("◉", "LIVE FEEDS", "Reading aggregators…"),
    "audit": ("⚠", "SAFETY AUDIT", "Auditing every feed — proxy resolution and bounds…"),
    "divergence": ("⇄", "CROSS-CHAIN DIVERGENCE", "Reading the same pairs on every chain…"),
    "sequencer": ("⬢", "L2 SEQUENCER UPTIME", "Reading uptime feeds…"),
    "gas": ("⛽", "GAS", "Sampling fee history…"),
    "ccip": ("⬡", "CCIP LANES", "Probing lanes via isChainSupported…"),
}


def _fetch(panel: str, network: str):
    """Run one panel's network work. Called on a worker thread."""
    if panel == "live":
        return read_all_feeds(network=network)
    if panel == "audit":
        return audit_network(network=network)
    if panel == "divergence":
        return compare_all()
    if panel == "sequencer":
        return [
            status
            for status in (read_sequencer(net) for net in sorted(SEQUENCER_FEEDS))
            if status is not None
        ]
    if panel == "gas":
        return analyse_gas(network=network)
    if panel == "ccip":
        return verify_lanes(network) if network in ROUTERS else []
    return None


class AlchemLinkApp(App):
    """Alchem-Link developer console."""

    TITLE = f"Alchem-Link  v{__version__}"
    CSS = CSS
    BINDINGS = [
        Binding("q", "quit", "Quit"),
        Binding("r", "refresh_panel", "Refresh"),
        Binding("n", "next_network", "Network"),
        Binding("escape", "back", "Back", show=False),
        Binding("j,down", "cursor_down", "Down", show=False),
        Binding("k,up", "cursor_up", "Up", show=False),
    ]

    def __init__(self) -> None:
        super().__init__()
        self._active = "live"
        self._recipe_detail: str | None = None
        self._network = DEFAULT_NETWORK
        # Per panel: (payload, error). Absent means "not fetched yet".
        self._cache: dict[tuple[str, str], tuple] = {}

    # ── layout ───────────────────────────────────────────────────────────────

    def compose(self) -> ComposeResult:
        yield Header()
        with Horizontal():
            with Vertical(id="sidebar"):
                yield Static("ALCHEM-LINK", id="sidebar-title")
                yield ListView(
                    *[ListItem(Static(label), id=f"nav-{nav}") for nav, label in NAV_ITEMS],
                    id="nav",
                )
            with ScrollableContainer(id="main"):
                yield Static("")
        yield Footer()

    def on_mount(self) -> None:
        self._refresh_main()
        self.query_one("#nav", ListView).index = 0
        self._ensure_loaded()

    # ── navigation ───────────────────────────────────────────────────────────

    def on_list_view_selected(self, event: ListView.Selected) -> None:
        item_id = event.item.id or ""
        if item_id.startswith("nav-"):
            self._active = item_id[4:]
            self._recipe_detail = None
            self._refresh_main()
            self._ensure_loaded()

    def action_back(self) -> None:
        if self._recipe_detail:
            self._recipe_detail = None
            self._refresh_main()

    def action_refresh_panel(self) -> None:
        if self._active not in LIVE_PANELS:
            return
        self._cache.pop(self._cache_key, None)
        self._refresh_main()
        self._load_panel()

    def action_next_network(self) -> None:
        keys = [n.key for n in list_networks()]
        self._network = keys[(keys.index(self._network) + 1) % len(keys)]
        self._refresh_main()
        self._ensure_loaded()

    def action_cursor_down(self) -> None:
        self.query_one("#nav", ListView).action_cursor_down()

    def action_cursor_up(self) -> None:
        self.query_one("#nav", ListView).action_cursor_up()

    # ── data loading ─────────────────────────────────────────────────────────

    @property
    def _cache_key(self) -> tuple[str, str]:
        # Divergence and sequencer span every chain, so their result does not depend on
        # the selected network and should not be refetched when it changes.
        scope = "*" if self._active in ("divergence", "sequencer") else self._network
        return (self._active, scope)

    def _ensure_loaded(self) -> None:
        if self._active in LIVE_PANELS and self._cache_key not in self._cache:
            self._load_panel()

    @work(thread=True, exclusive=True)
    def _load_panel(self) -> None:
        """Fetch on a worker thread — an RPC round trip must not freeze the app."""
        panel, network = self._active, self._network
        wanted = self._cache_key
        try:
            payload, error = _fetch(panel, network), None
        except Exception as exc:  # surfaced in the panel rather than crashing the TUI
            payload, error = None, str(exc)

        # The user may have navigated while this was in flight. Cache the result either
        # way — it is still valid for its own panel — but only repaint if it is current.
        self._cache[wanted] = (payload, error)
        if (panel, network) == (self._active, self._network):
            self.call_from_thread(self._refresh_main)

    # ── rendering ────────────────────────────────────────────────────────────

    def _widgets(self) -> list:
        if self._active in LIVE_PANELS:
            cached = self._cache.get(self._cache_key)
            if cached is None:
                icon, label, note = LOADING_NOTES[self._active]
                network = None if self._active in ("divergence", "sequencer") else self._network
                heading = label if network is None else f"{label} — {self._network.upper()}"
                return _loading(icon, heading, network, note)
            payload, error = cached
            return LIVE_RENDERERS[self._active](self._network, payload, error)

        if self._active == "recipes":
            return _render_recipes(self._recipe_detail)
        return REFERENCE_RENDERERS[self._active]()

    def _refresh_main(self) -> None:
        container = self.query_one("#main", ScrollableContainer)
        container.remove_children()
        for widget in self._widgets():
            container.mount(widget)
        container.scroll_home(animate=False)


def launch() -> None:
    AlchemLinkApp().run()


if __name__ == "__main__":
    launch()
