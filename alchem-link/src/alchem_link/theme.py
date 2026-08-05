"""The Alchem-Link terminal palette — black surfaces, mid-blue signal.

One module owns every colour the TUI paints, for two reasons.

First, Textual takes colours in two unrelated places: a CSS block, and inline Rich
markup like ``[bold #4d9fff]``. CSS classes cannot reach inside a Rich markup span, so
the render functions must name colours literally. When those literals were scattered
across a dozen f-strings, changing the theme meant a find-and-replace across the file
and missing three of them.

Second, the web build under ``web/app/alchem-link`` mirrors this exact palette. Keeping
the terminal's source of truth in one readable place is what makes "the web version
looks like the terminal" a thing you can check rather than hope for.

Design intent: the background is black — not navy — so the blue reads as *signal*
against it rather than as another shade of the surface. The accent sits in the mid
range (``BLUE`` = #4d9fff); a darker blue muddies into the background on the cheap
LCD panels this actually gets demoed on, and a lighter one drifts into cyan and stops
looking like a deliberate choice.
"""
from __future__ import annotations

# ── surfaces ─────────────────────────────────────────────────────────────────
#: App background. Black with a barely-there blue lift, so it does not read as a
#: dead grey rectangle next to the accents.
BLACK = "#04070c"
#: Cards, sidebar — one step off the background, enough to catch a border.
SURFACE = "#080e17"
#: Hover and selected rows.
SURFACE_HI = "#0e1926"

# ── structure ────────────────────────────────────────────────────────────────
BORDER = "#1b3d63"
BORDER_HI = "#2c6296"

# ── the blue ─────────────────────────────────────────────────────────────────
#: Primary accent — headings, keys, the product mark.
BLUE = "#4d9fff"
#: Highlight for the focused item and section titles.
BLUE_HI = "#7cc0ff"
#: Recessed blue for supporting labels.
BLUE_DIM = "#2f6ba8"

# ── type ─────────────────────────────────────────────────────────────────────
TEXT = "#cddced"
MUTED = "#7f9ec0"
DIM = "#47678c"

# ── status ───────────────────────────────────────────────────────────────────
#: A feed inside its heartbeat.
GREEN = "#2ee6a0"
#: Past heartbeat, or an advisory note on a registry entry.
AMBER = "#ffb340"
#: Unreadable, or an answer <= 0.
RED = "#ff5c78"

#: Status label → colour, shared by the TUI and mirrored by the web badge.
STATUS_COLOUR = {
    "FRESH": GREEN,
    "STALE": AMBER,
    "INVALID": RED,
}

#: Status label → CSS class name, for the widgets that style via CSS instead of markup.
STATUS_CLASS = {
    "FRESH": "status-fresh",
    "STALE": "status-stale",
    "INVALID": "status-invalid",
}


def key(text: str) -> str:
    """A field name — bold accent blue."""
    return f"[bold {BLUE}]{text}[/]"


def value(text: str) -> str:
    """A field value — plain readable type."""
    return f"[{TEXT}]{text}[/]"


def title(text: str) -> str:
    """A section heading — the brightest blue on the screen."""
    return f"[bold {BLUE_HI}]{text}[/]"


def hint(text: str) -> str:
    """A keybinding hint or endpoint line — present but out of the way."""
    return f"[{DIM}]{text}[/]"


def status(label: str) -> str:
    """A FRESH/STALE/INVALID badge in its own colour."""
    return f"[bold {STATUS_COLOUR.get(label, TEXT)}]{label}[/]"


CSS = f"""
Screen {{
    background: {BLACK};
}}

#sidebar {{
    width: 26;
    background: {SURFACE};
    border-right: solid {BORDER};
    padding: 1 0;
}}

#sidebar-title {{
    text-align: center;
    color: {BLUE_HI};
    text-style: bold;
    padding: 0 1 1 1;
}}

ListView {{
    background: {SURFACE};
    border: none;
}}

ListItem {{
    padding: 0 2;
    color: {MUTED};
}}

ListItem:hover {{
    background: {SURFACE_HI};
    color: {BLUE_HI};
}}

ListItem.--highlight {{
    background: {SURFACE_HI};
    color: {BLUE_HI};
    text-style: bold;
}}

#main {{
    background: {BLACK};
    padding: 1 2;
}}

.section-title {{
    color: {BLUE_HI};
    text-style: bold;
    padding: 0 0 1 0;
}}

.card {{
    background: {SURFACE};
    border: solid {BORDER};
    padding: 1 2;
    margin: 0 0 1 0;
}}

.card-key {{
    color: {BLUE};
    text-style: bold;
}}

.card-value {{
    color: {TEXT};
}}

.card-tag {{
    color: {GREEN};
    text-style: italic;
}}

.step-num {{
    color: {AMBER};
    text-style: bold;
}}

.step-text {{
    color: {TEXT};
}}

.dim {{
    color: {DIM};
}}

.status-fresh {{
    color: {GREEN};
    text-style: bold;
}}

.status-stale {{
    color: {AMBER};
    text-style: bold;
}}

.status-invalid {{
    color: {RED};
    text-style: bold;
}}

.recipe-summary {{
    color: {MUTED};
    padding: 0 0 1 0;
}}

Rule {{
    color: {BORDER};
}}

Header {{
    background: {SURFACE};
    color: {BLUE_HI};
    border-bottom: solid {BORDER};
}}

Footer {{
    background: {SURFACE};
    color: {BLUE_DIM};
    border-top: solid {BORDER};
}}
"""
