"""Alchem-Link TUI — futuristic developer dashboard."""
from __future__ import annotations

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
from .integration import build_integration_map
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
    ("blueprint",  "⬡  Blueprint"),
    ("alchemy",    "◈  Alchemy"),
    ("chainlink",  "⬢  Chainlink"),
    ("integration","⇄  Integration"),
    ("recipes",    "✦  Recipes"),
]


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
        Binding("escape", "back", "Back", show=False),
        Binding("j,down", "cursor_down", "Down", show=False),
        Binding("k,up", "cursor_up", "Up", show=False),
    ]

    def __init__(self) -> None:
        super().__init__()
        self._active: str = "blueprint"
        self._recipe_detail: str | None = None

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

    def on_list_view_selected(self, event: ListView.Selected) -> None:
        item_id = event.item.id or ""
        if item_id.startswith("nav-"):
            self._active = item_id[4:]
            self._recipe_detail = None
            self._refresh_main()

    def action_back(self) -> None:
        if self._recipe_detail:
            self._recipe_detail = None
            self._refresh_main()

    def action_cursor_down(self) -> None:
        self.query_one("#nav", ListView).action_cursor_down()

    def action_cursor_up(self) -> None:
        self.query_one("#nav", ListView).action_cursor_up()

    def _refresh_main(self) -> None:
        container = self.query_one("#main", ScrollableContainer)
        container.remove_children()
        renderer = RENDERERS[self._active]
        if self._active == "recipes":
            widgets = renderer(self._recipe_detail)
        else:
            widgets = renderer()
        for w in widgets:
            container.mount(w)
        container.scroll_home(animate=False)


def launch() -> None:
    AlchemLinkApp().run()


if __name__ == "__main__":
    launch()
