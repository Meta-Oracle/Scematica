"""The Alchem-Link palette — black surfaces, mid-blue signal.

One module owns every colour the product paints: the full-screen dashboard, the
interactive shell, plain command output, and the boot sequence that repaints the host
terminal itself. It is deliberately inert — a table of hex strings and semantic
:class:`Style` records, no escape sequences and no I/O. :mod:`alchem_link.term.ansi`
turns a :class:`Style` into bytes for whatever colour depth the terminal admits to, and
:mod:`alchem_link.render` uses the same roles for line-oriented output. Keeping the
decision ("a heading is the brightest blue on screen") apart from the encoding ("which
of 256 indices is nearest that blue") is what lets one palette drive a truecolor
terminal, a 16-colour Windows console, and a `NO_COLOR` pipe without three palettes.

The web build under ``web/lib/alchem`` mirrors these exact values. Keeping the
terminal's source of truth in one readable place is what makes "the web version looks
like the terminal" a thing you can check rather than hope for.

Design intent: the background is black — not navy — so the blue reads as *signal*
against it rather than as another shade of the surface. The accent sits in the mid range
(``BLUE`` = #4d9fff); a darker blue muddies into the background on the cheap LCD panels
this actually gets demoed on, and a lighter one drifts into cyan and stops looking like a
deliberate choice.
"""
from __future__ import annotations

from dataclasses import dataclass
from typing import Dict, Optional

# ── surfaces ─────────────────────────────────────────────────────────────────
#: App background. Black with a barely-there blue lift, so it does not read as a
#: dead grey rectangle next to the accents. This is also the colour the boot
#: sequence pushes to the host terminal via OSC 11.
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

#: Every named colour, for the boot banner, the web mirror, and `alchem-link theme`.
PALETTE: Dict[str, str] = {
    "black": BLACK,
    "surface": SURFACE,
    "surface_hi": SURFACE_HI,
    "border": BORDER,
    "border_hi": BORDER_HI,
    "blue": BLUE,
    "blue_hi": BLUE_HI,
    "blue_dim": BLUE_DIM,
    "text": TEXT,
    "muted": MUTED,
    "dim": DIM,
    "green": GREEN,
    "amber": AMBER,
    "red": RED,
}

#: Status label → colour, shared by the dashboard and mirrored by the web badge.
STATUS_COLOUR = {
    "FRESH": GREEN,
    "STALE": AMBER,
    "INVALID": RED,
}

#: Status label → CSS class name, for the web mirror.
STATUS_CLASS = {
    "FRESH": "status-fresh",
    "STALE": "status-stale",
    "INVALID": "status-invalid",
}

#: Audit severity → colour. `ok` is not a severity the auditor emits, but it is the
#: verdict shown when a feed produced no findings, so it lives in the same table.
SEVERITY_COLOUR = {
    "critical": RED,
    "high": RED,
    "medium": AMBER,
    "low": MUTED,
    "info": MUTED,
    "ok": GREEN,
}


@dataclass(frozen=True)
class Style:
    """A semantic appearance: colours as hex, attributes as flags.

    Frozen and hashable so :mod:`alchem_link.term.ansi` can memoise the encoded escape
    sequence per (style, depth). A full-screen repaint asks for the same dozen styles
    thousands of times; encoding each one once matters more than it looks.
    """

    fg: Optional[str] = None
    bg: Optional[str] = None
    bold: bool = False
    dim: bool = False
    italic: bool = False
    underline: bool = False
    reverse: bool = False

    def with_(self, **overrides) -> "Style":
        """A copy with some fields changed — `ROLES['key'].with_(bg=SURFACE_HI)`."""
        fields = {
            "fg": self.fg, "bg": self.bg, "bold": self.bold, "dim": self.dim,
            "italic": self.italic, "underline": self.underline, "reverse": self.reverse,
        }
        fields.update(overrides)
        return Style(**fields)

    @property
    def is_plain(self) -> bool:
        """True when this style asks for nothing — no escape needs emitting at all."""
        return (
            self.fg is None and self.bg is None
            and not (self.bold or self.dim or self.italic or self.underline or self.reverse)
        )


#: The default appearance of everything: readable type on the app background. Every
#: other role is a deviation from this, which is why the screen buffer fills with it.
BASE = Style(fg=TEXT, bg=BLACK)

#: Semantic roles. Render code names a role; it never names a colour. The rule this
#: enforces is testable — see ``tests/test_theme.py``, which fails the build if a render
#: module hardcodes a hex value instead of asking for a role.
ROLES: Dict[str, Style] = {
    # structure
    "base": BASE,
    "title": Style(fg=BLUE_HI, bg=BLACK, bold=True),
    "subtitle": Style(fg=BLUE, bg=BLACK, bold=True),
    "key": Style(fg=BLUE, bg=BLACK, bold=True),
    "value": Style(fg=TEXT, bg=BLACK),
    "muted": Style(fg=MUTED, bg=BLACK),
    "hint": Style(fg=DIM, bg=BLACK),
    "accent": Style(fg=BLUE, bg=BLACK),
    "border": Style(fg=BORDER, bg=BLACK),
    "border_focus": Style(fg=BORDER_HI, bg=BLACK, bold=True),
    "rule": Style(fg=BORDER, bg=BLACK),
    # chrome
    "header": Style(fg=BLUE_HI, bg=SURFACE, bold=True),
    "footer": Style(fg=BLUE_DIM, bg=SURFACE),
    "sidebar": Style(fg=MUTED, bg=SURFACE),
    "sidebar_title": Style(fg=BLUE_HI, bg=SURFACE, bold=True),
    "selected": Style(fg=BLUE_HI, bg=SURFACE_HI, bold=True),
    "cursor": Style(fg=BLACK, bg=BLUE, bold=True),
    "card": Style(fg=TEXT, bg=SURFACE),
    # data
    "number": Style(fg=TEXT, bg=BLACK),
    "column": Style(fg=BLUE_DIM, bg=BLACK),
    "ok": Style(fg=GREEN, bg=BLACK, bold=True),
    "warn": Style(fg=AMBER, bg=BLACK, bold=True),
    "bad": Style(fg=RED, bg=BLACK, bold=True),
    "spark": Style(fg=BLUE, bg=BLACK),
    "gauge_full": Style(fg=BLUE, bg=BLACK),
    "gauge_empty": Style(fg=BORDER, bg=BLACK),
    # prompt
    "prompt": Style(fg=BLUE, bg=BLACK, bold=True),
    "prompt_net": Style(fg=DIM, bg=BLACK),
    "banner": Style(fg=BLUE, bg=BLACK, bold=True),
}


def role(name: str) -> Style:
    """Look up a role, falling back to :data:`BASE`.

    Deliberately total. A render pass that asks for a role someone renamed should paint
    plain text, not raise mid-frame and leave the alternate screen buffer occupied.
    """
    return ROLES.get(name, BASE)


def status_style(label: str) -> Style:
    """FRESH / STALE / INVALID, in the colour the whole product agrees on."""
    return Style(fg=STATUS_COLOUR.get(label, TEXT), bg=BLACK, bold=True)


def severity_style(severity: str) -> Style:
    """An audit finding's severity, in its colour."""
    return Style(fg=SEVERITY_COLOUR.get(severity, MUTED), bg=BLACK, bold=severity in ("critical", "high"))


def rgb(value: str) -> tuple:
    """``"#4d9fff"`` → ``(77, 159, 255)``. Accepts a leading ``#`` or not."""
    raw = value.lstrip("#")
    if len(raw) != 6:
        raise ValueError(f"not a 6-digit hex colour: {value!r}")
    return int(raw[0:2], 16), int(raw[2:4], 16), int(raw[4:6], 16)
