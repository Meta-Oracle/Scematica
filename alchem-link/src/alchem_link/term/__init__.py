"""Alchem-Link's terminal system — a full-screen UI toolkit in the standard library.

The package claims zero runtime dependencies, and the user interface is the part where
that claim is usually quietly abandoned. It is not abandoned here: everything a terminal
application needs is in this subpackage, in six modules that stack cleanly.

``ansi``     escape sequences, and colour-depth negotiation (truecolor → 256 → 16 → none)
``screen``   a double-buffered cell grid that emits only the runs that changed
``input``    raw mode, plus a pure parser from escape sequences to named keys
``widgets``  panels, tables, sparklines, gauges, tabs, scroll state
``app``      the event loop, background workers, resize handling
``boot``     terminal initialisation — the black-and-blue, and putting it back

The layering is strict: ``ansi`` knows about bytes, ``screen`` knows about cells,
``widgets`` knows about rectangles, ``app`` knows about events, and only ``boot`` knows
about the process. Nothing here imports anything else from ``alchem_link`` except
:mod:`alchem_link.theme`, which is an inert palette — so the terminal system can be read,
tested, and reused on its own.
"""
from __future__ import annotations

from .ansi import (
    Depth,
    detect_depth,
    display_width,
    enable_vt,
    paint,
    pad,
    sgr,
    strip,
    truncate,
)
from .app import App, Job
from .boot import (
    BootState,
    describe,
    enter_fullscreen,
    exit_fullscreen,
    initialize,
    is_frozen,
    restore,
)
from .input import InputReader, Key, Mouse, parse_key, raw_mode
from .screen import Screen, terminal_size
from .widgets import (
    Column,
    Rect,
    Row,
    Scroll,
    badge,
    bar_rows,
    draw_paragraph,
    draw_text,
    gauge,
    key_value,
    panel,
    resolve_widths,
    rule,
    scrollbar,
    sidebar,
    sparkline,
    status_bar,
    table,
    tabs,
    wrap,
)

__all__ = [
    # ansi
    "Depth",
    "detect_depth",
    "display_width",
    "enable_vt",
    "paint",
    "pad",
    "sgr",
    "strip",
    "truncate",
    # screen
    "Screen",
    "terminal_size",
    # input
    "InputReader",
    "Key",
    "Mouse",
    "parse_key",
    "raw_mode",
    # widgets
    "Rect",
    "Column",
    "Row",
    "Scroll",
    "badge",
    "bar_rows",
    "draw_paragraph",
    "draw_text",
    "gauge",
    "key_value",
    "panel",
    "resolve_widths",
    "rule",
    "scrollbar",
    "sidebar",
    "sparkline",
    "status_bar",
    "table",
    "tabs",
    "wrap",
    # app + boot
    "App",
    "Job",
    "BootState",
    "describe",
    "enter_fullscreen",
    "exit_fullscreen",
    "initialize",
    "is_frozen",
    "restore",
]
