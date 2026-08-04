"""Alchem-Link TUI — futuristic developer dashboard."""
from __future__ import annotations

from textual import work
from textual.app import App, ComposeResult
from textual.binding import Binding
from textual.containers import Horizontal, Vertical, ScrollableContainer
from textual.widgets import (
    Footer,
    Header,
    Label,
    ListItem,
    ListView,
    Rule,
    Static,
)

from .alchemy import summarize_alchemy_capabilities
from .chainlink import summarize_chainlink_capabilities
from .core import build_package_blueprint
from .feeds import read_all_feeds
from .integration import build_integration_map
from .networks import DEFAULT_NETWORK, list_networks, resolve_endpoint
from .recipes import get_recipe_by_id, get_recipes
from . import __version__

# ── colour palette (CSS vars) ────────────────────────────────────────────────
CSS = """
Screen {
    background: #0a0e1a;
}

#sidebar {
    width: 26;
    background: #0d1220;
    border-right: solid #1e3a5f;
    padding: 1 0;
}

#sidebar-title {
    text-align: center;
    color: #00d4ff;
    text-style: bold;
    padding: 0 1 1 1;
}

ListView {
    background: #0d1220;
    border: none;
}

ListItem {
    padding: 0 2;
    color: #7a9cc4;
}

ListItem:hover {
    background: #1a2a40;
    color: #00d4ff;
}

ListItem.--highlight {
    background: #0f2a45;
    color: #00d4ff;
    text-style: bold;
}

#main {
    background: #0a0e1a;
    padding: 1 2;
}

.section-title {
    color: #00d4ff;
    text-style: bold;
    padding: 0 0 1 0;
}

.card {
    background: #0d1220;
    border: solid #1e3a5f;
    padding: 1 2;
    margin: 0 0 1 0;
}

.card-key {
    color: #4a9eff;
    text-style: bold;
}

.card-value {
    color: #c8d8e8;
}

.card-tag {
    color: #00ff9f;
    text-style: italic;
}

.step-num {
    color: #f0a500;
    text-style: bold;
}

.step-text {
    color: #c8d8e8;
}

.dim {
    color: #3a5070;
}

.status-fresh {
    color: #00ff9f;
    text-style: bold;
}

.status-stale {
    color: #f0a500;
    text-style: bold;
}

.status-invalid {
    color: #ff4d6d;
    text-style: bold;
}

.recipe-summary {
    color: #a0c4e8;
    padding: 0 0 1 0;
}

Rule {
    color: #1e3a5f;
}

Header {
    background: #0d1220;
    color: #00d4ff;
    border-bottom: solid #1e3a5f;
}

Footer {
    background: #0d1220;
    color: #3a6090;
    border-top: solid #1e3a5f;
}
"""

NAV_ITEMS = [
    ("live",       "◉  Live Feeds"),
    ("blueprint",  "⬡  Blueprint"),
    ("alchemy",    "◈  Alchemy"),
    ("chainlink",  "⬢  Chainlink"),
    ("integration","⇄  Integration"),
    ("recipes",    "✦  Recipes"),
]

_STATUS_CLASS = {
    "FRESH": "status-fresh",
    "STALE": "status-stale",
    "INVALID": "status-invalid",
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
    return f"{seconds // 3600}h {(seconds % 3600) // 60}m"


def _kv(key: str, value: str) -> list[Static]:
    return [
        Static(f"[bold #4a9eff]{key}[/]", classes="card-key"),
        Static(f"[#c8d8e8]{value}[/]", classes="card-value"),
    ]


def _render_blueprint() -> list[Static | Rule | Label]:
    bp = build_package_blueprint()
    widgets: list = [Label("◈  BLUEPRINT", classes="section-title")]
    for section, data in bp.items():
        widgets.append(Rule())
        widgets.append(Static(f"[bold #00d4ff]{section.upper()}[/]", classes="card-key"))
        if isinstance(data, dict):
            for k, v in data.items():
                if isinstance(v, list):
                    widgets.append(Static(f"[bold #4a9eff]{k}[/]", classes="card-key"))
                    for i, item in enumerate(v, 1):
                        widgets.append(Static(f"  [#f0a500]{i}.[/] [#c8d8e8]{item}[/]"))
                else:
                    widgets += _kv(f"  {k}", str(v))
        else:
            widgets.append(Static(f"[#c8d8e8]{data}[/]", classes="card-value"))
    return widgets


def _render_alchemy() -> list:
    data = summarize_alchemy_capabilities()
    widgets: list = [Label("◈  ALCHEMY CAPABILITIES", classes="section-title")]
    for k, v in data.items():
        widgets.append(Rule())
        if isinstance(v, list):
            widgets.append(Static(f"[bold #4a9eff]{k.upper()}[/]", classes="card-key"))
            for item in v:
                widgets.append(Static(f"  [#c8d8e8]• {item}[/]"))
        else:
            widgets += _kv(k.upper(), str(v))
    return widgets


def _render_chainlink() -> list:
    data = summarize_chainlink_capabilities()
    widgets: list = [Label("⬢  CHAINLINK CAPABILITIES", classes="section-title")]
    for k, v in data.items():
        widgets.append(Rule())
        widgets += _kv(k.upper(), str(v))
    return widgets


def _render_integration() -> list:
    data = build_integration_map()
    widgets: list = [Label("⇄  INTEGRATION MAP", classes="section-title")]
    for domain, sides in data.items():
        widgets.append(Rule())
        widgets.append(Static(f"[bold #00d4ff]{domain.upper().replace('_', ' ')}[/]"))
        for side, desc in sides.items():
            widgets += _kv(f"  {side.capitalize()}", desc)
    return widgets


def _render_recipes(selected_id: str | None = None) -> list:
    recipes = get_recipes()
    if selected_id:
        recipe = get_recipe_by_id(selected_id)
        if recipe:
            return _render_recipe_detail(recipe)
    widgets: list = [Label("✦  RECIPES", classes="section-title")]
    for r in recipes:
        widgets.append(Rule())
        widgets.append(Static(f"[bold #00d4ff]{r['name']}[/]"))
        widgets.append(Static(f"[#a0c4e8]{r['summary']}[/]", classes="recipe-summary"))
        widgets.append(Static(
            "  " + "  ".join(f"[#00ff9f]#{t}[/]" for t in r["tags"]),
            classes="card-tag",
        ))
        widgets.append(Static(
            f"  [dim #3a6090]id: {r['id']}  →  press [bold]r[/] then type id to drill in[/]",
            classes="dim",
        ))
    return widgets


def _render_recipe_detail(recipe: dict) -> list:
    widgets: list = [
        Label(f"✦  {recipe['name'].upper()}", classes="section-title"),
        Static(f"[#a0c4e8]{recipe['summary']}[/]", classes="recipe-summary"),
        Rule(),
        Static("[bold #4a9eff]STEPS[/]", classes="card-key"),
    ]
    for i, step in enumerate(recipe["steps"], 1):
        widgets.append(Static(f"  [bold #f0a500]{i}.[/] [#c8d8e8]{step}[/]"))
    widgets.append(Rule())
    widgets.append(Static(
        "  " + "  ".join(f"[#00ff9f]#{t}[/]" for t in recipe["tags"]),
        classes="card-tag",
    ))
    widgets.append(Static(
        f"\n  [dim #3a6090]← press [bold]Escape[/] to return to recipes[/]",
        classes="dim",
    ))
    return widgets


def _render_live_loading(network: str) -> list:
    endpoint = resolve_endpoint(network=network)
    return [
        Label(f"◉  LIVE FEEDS — {network.upper()}", classes="section-title"),
        Static(f"[#3a6090]{endpoint.redacted()}  ({endpoint.source})[/]", classes="dim"),
        Rule(),
        Static("[#f0a500]Reading aggregators…[/]"),
    ]


def _render_live(network: str, readings: list, error: str | None = None) -> list:
    endpoint = resolve_endpoint(network=network)
    widgets: list = [
        Label(f"◉  LIVE FEEDS — {network.upper()}", classes="section-title"),
        Static(f"[#3a6090]{endpoint.redacted()}  ({endpoint.source})[/]", classes="dim"),
        Rule(),
    ]
    if error:
        widgets.append(Static(f"[#ff4d6d]{error}[/]"))
        widgets.append(Static("[#3a6090]press [bold]r[/] to retry, [bold]n[/] to switch network[/]", classes="dim"))
        return widgets
    if not readings:
        widgets.append(Static("[#f0a500]No feeds could be read on this network.[/]"))
        return widgets

    width = max(len(r.pair) for r in readings)
    for r in readings:
        colour = {"FRESH": "#00ff9f", "STALE": "#f0a500", "INVALID": "#ff4d6d"}[r.status]
        widgets.append(
            Static(
                f"  [bold #4a9eff]{r.pair:<{width}}[/]  "
                f"[#c8d8e8]{_fmt_price(r.price):>16}[/]  "
                f"[bold {colour}]{r.status:<7}[/]  "
                f"[#7a9cc4]{_fmt_age(r.age_secs)} ago[/]"
            )
        )
        if r.note:
            widgets.append(Static(f"      [#f0a500]{r.note}[/]", classes="dim"))
    widgets.append(Rule())
    stale = sum(1 for r in readings if r.stale)
    widgets.append(
        Static(
            f"  [#7a9cc4]{len(readings)} feeds · "
            f"{len(readings) - stale} fresh · "
            f"{stale} past heartbeat[/]"
        )
    )
    widgets.append(
        Static("  [#3a6090]press [bold]r[/] to refresh · [bold]n[/] to switch network[/]", classes="dim")
    )
    return widgets


RENDERERS = {
    "blueprint":   _render_blueprint,
    "alchemy":     _render_alchemy,
    "chainlink":   _render_chainlink,
    "integration": _render_integration,
    "recipes":     _render_recipes,
}


class AlchemLinkApp(App):
    """Alchem-Link developer dashboard."""

    TITLE = f"Alchem-Link  v{__version__}"
    CSS = CSS
    BINDINGS = [
        Binding("q", "quit", "Quit"),
        Binding("r", "refresh_live", "Refresh"),
        Binding("n", "next_network", "Network"),
        Binding("escape", "back", "Back", show=False),
        Binding("j,down", "cursor_down", "Down", show=False),
        Binding("k,up", "cursor_up", "Up", show=False),
    ]

    def __init__(self) -> None:
        super().__init__()
        self._active: str = "live"
        self._recipe_detail: str | None = None
        self._network: str = DEFAULT_NETWORK
        self._readings: list = []
        self._live_error: str | None = None
        self._live_loaded: bool = False

    def compose(self) -> ComposeResult:
        yield Header()
        with Horizontal():
            with Vertical(id="sidebar"):
                yield Static("ALCHEM-LINK", id="sidebar-title")
                yield ListView(
                    *[ListItem(Static(label), id=f"nav-{key}") for key, label in NAV_ITEMS],
                    id="nav",
                )
            with ScrollableContainer(id="main"):
                yield Static("")  # placeholder, replaced on mount
        yield Footer()

    def on_mount(self) -> None:
        self._refresh_main()
        self.query_one("#nav", ListView).index = 0
        self._load_feeds()

    def on_list_view_selected(self, event: ListView.Selected) -> None:
        item_id = event.item.id or ""
        if item_id.startswith("nav-"):
            self._active = item_id[4:]
            self._recipe_detail = None
            self._refresh_main()
            # First visit to Live fetches; later visits reuse the last read until `r`.
            if self._active == "live" and not self._live_loaded:
                self._load_feeds()

    def action_back(self) -> None:
        if self._recipe_detail:
            self._recipe_detail = None
            self._refresh_main()

    def action_refresh_live(self) -> None:
        if self._active != "live":
            return
        self._live_loaded = False
        self._refresh_main()
        self._load_feeds()

    def action_next_network(self) -> None:
        if self._active != "live":
            return
        keys = [n.key for n in list_networks()]
        self._network = keys[(keys.index(self._network) + 1) % len(keys)]
        self._live_loaded = False
        self._refresh_main()
        self._load_feeds()

    @work(thread=True, exclusive=True)
    def _load_feeds(self) -> None:
        """Read feeds off the UI thread — an RPC round trip must not freeze the app."""
        network = self._network
        try:
            readings = read_all_feeds(network=network)
            error = None if readings else "No feeds could be read. Check connectivity."
        except Exception as exc:  # surfaced in the panel rather than crashing the TUI
            readings, error = [], str(exc)
        # A network switch may have landed while this was in flight; drop stale results.
        if network != self._network:
            return
        self._readings = readings
        self._live_error = error
        self._live_loaded = True
        self.call_from_thread(self._refresh_main)

    def action_cursor_down(self) -> None:
        self.query_one("#nav", ListView).action_cursor_down()

    def action_cursor_up(self) -> None:
        self.query_one("#nav", ListView).action_cursor_up()

    def _refresh_main(self) -> None:
        container = self.query_one("#main", ScrollableContainer)
        container.remove_children()
        if self._active == "live":
            widgets = (
                _render_live(self._network, self._readings, self._live_error)
                if self._live_loaded
                else _render_live_loading(self._network)
            )
        elif self._active == "recipes":
            widgets = RENDERERS["recipes"](self._recipe_detail)
        else:
            widgets = RENDERERS[self._active]()
        for w in widgets:
            container.mount(w)
        container.scroll_home(animate=False)


def launch() -> None:
    AlchemLinkApp().run()


if __name__ == "__main__":
    launch()
