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
from . import __version__

NAV_ITEMS = [
    ("live",       "◉  Live Feeds"),
    ("blueprint",  "⬡  Blueprint"),
    ("alchemy",    "◈  Alchemy"),
    ("chainlink",  "⬢  Chainlink"),
    ("integration","⇄  Integration"),
    ("recipes",    "✦  Recipes"),
]

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


def _kv(name: str, text: str) -> list[Static]:
    return [
        Static(key(name), classes="card-key"),
        Static(f"[{TEXT}]{text}[/]", classes="card-value"),
    ]


def _render_blueprint() -> list[Static | Rule | Label]:
    bp = build_package_blueprint()
    widgets: list = [Label("◈  BLUEPRINT", classes="section-title")]
    for section, data in bp.items():
        widgets.append(Rule())
        widgets.append(Static(title(section.upper()), classes="card-key"))
        if isinstance(data, dict):
            for k, v in data.items():
                if isinstance(v, list):
                    widgets.append(Static(key(k), classes="card-key"))
                    for i, item in enumerate(v, 1):
                        widgets.append(Static(f"  [{AMBER}]{i}.[/] [{TEXT}]{item}[/]"))
                else:
                    widgets += _kv(f"  {k}", str(v))
        else:
            widgets.append(Static(f"[{TEXT}]{data}[/]", classes="card-value"))
    return widgets


def _render_alchemy() -> list:
    data = summarize_alchemy_capabilities()
    widgets: list = [Label("◈  ALCHEMY CAPABILITIES", classes="section-title")]
    for k, v in data.items():
        widgets.append(Rule())
        if isinstance(v, list):
            widgets.append(Static(key(k.upper()), classes="card-key"))
            for item in v:
                widgets.append(Static(f"  [{TEXT}]• {item}[/]"))
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
        widgets.append(Static(title(domain.upper().replace("_", " "))))
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
        widgets.append(Static(title(r["name"])))
        widgets.append(Static(f"[{MUTED}]{r['summary']}[/]", classes="recipe-summary"))
        widgets.append(Static(
            "  " + "  ".join(f"[{GREEN}]#{t}[/]" for t in r["tags"]),
            classes="card-tag",
        ))
        widgets.append(Static(
            hint(f"id: {r['id']}  →  press [bold]r[/] then type id to drill in"),
            classes="dim",
        ))
    return widgets


def _render_recipe_detail(recipe: dict) -> list:
    widgets: list = [
        Label(f"✦  {recipe['name'].upper()}", classes="section-title"),
        Static(f"[{MUTED}]{recipe['summary']}[/]", classes="recipe-summary"),
        Rule(),
        Static(key("STEPS"), classes="card-key"),
    ]
    for i, step in enumerate(recipe["steps"], 1):
        widgets.append(Static(f"  [bold {AMBER}]{i}.[/] [{TEXT}]{step}[/]"))
    widgets.append(Rule())
    widgets.append(Static(
        "  " + "  ".join(f"[{GREEN}]#{t}[/]" for t in recipe["tags"]),
        classes="card-tag",
    ))
    widgets.append(Static(
        "\n  " + hint("← press [bold]Escape[/] to return to recipes"),
        classes="dim",
    ))
    return widgets


def _render_live_loading(network: str) -> list:
    endpoint = resolve_endpoint(network=network)
    return [
        Label(f"◉  LIVE FEEDS — {network.upper()}", classes="section-title"),
        Static(hint(f"{endpoint.redacted()}  ({endpoint.source})"), classes="dim"),
        Rule(),
        Static(f"[{AMBER}]Reading aggregators…[/]"),
    ]


def _render_live(network: str, readings: list, error: str | None = None) -> list:
    endpoint = resolve_endpoint(network=network)
    widgets: list = [
        Label(f"◉  LIVE FEEDS — {network.upper()}", classes="section-title"),
        Static(hint(f"{endpoint.redacted()}  ({endpoint.source})"), classes="dim"),
        Rule(),
    ]
    if error:
        widgets.append(Static(f"[{RED}]{error}[/]"))
        widgets.append(Static(
            hint("press [bold]r[/] to retry, [bold]n[/] to switch network"), classes="dim"
        ))
        return widgets
    if not readings:
        widgets.append(Static(f"[{AMBER}]No feeds could be read on this network.[/]"))
        return widgets

    width = max(len(r.pair) for r in readings)
    for r in readings:
        colour = STATUS_COLOUR[r.status]
        widgets.append(
            Static(
                f"  [bold {BLUE}]{r.pair:<{width}}[/]  "
                f"[{TEXT}]{_fmt_price(r.price):>16}[/]  "
                f"[bold {colour}]{r.status:<7}[/]  "
                f"[{MUTED}]{_fmt_age(r.age_secs)} ago[/]"
            )
        )
        if r.note:
            widgets.append(Static(f"      [{AMBER}]{r.note}[/]", classes="dim"))
    widgets.append(Rule())
    stale = sum(1 for r in readings if r.stale)
    widgets.append(
        Static(
            f"  [{MUTED}]{len(readings)} feeds · "
            f"{len(readings) - stale} fresh · "
            f"{stale} past heartbeat[/]"
        )
    )
    widgets.append(
        Static(
            hint("press [bold]r[/] to refresh · [bold]n[/] to switch network"), classes="dim"
        )
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
                    *[ListItem(Static(label), id=f"nav-{nav}") for nav, label in NAV_ITEMS],
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
