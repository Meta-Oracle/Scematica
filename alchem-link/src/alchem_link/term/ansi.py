"""Escape sequences, and the colour-depth negotiation that decides which to emit.

This is the only module in the package that knows what a terminal control byte looks
like. Everything above it names a :class:`~alchem_link.theme.Style`; everything below it
is a stream of bytes.

Three things here are easy to get wrong and expensive to debug:

* **Depth is negotiated, not assumed.** Emitting ``38;2;r;g;b`` at a terminal that only
  speaks 16 colours does not degrade — it prints the digits as literal text across the
  frame. So a truecolor style is *converted*: to the nearest xterm-256 index, or to the
  nearest of the sixteen, or dropped entirely. One palette, four rendering targets.
* **Windows needs to be asked.** A Windows console ignores escape sequences until
  ``ENABLE_VIRTUAL_TERMINAL_PROCESSING`` is set on its output handle. Without that call
  the entire UI renders as visible ``←[38;2;...m`` garbage. :func:`enable_vt` performs
  it and reports whether it took, so callers can fall back rather than paint noise.
* **Escapes are not width.** ``len()`` over a styled string counts the escape bytes, so
  any column arithmetic done on styled text is wrong by however much colour it carries.
  :func:`display_width` measures what a terminal will actually show, wide CJK cells
  included, and every layout helper in this package uses it instead of ``len``.
"""
from __future__ import annotations

import os
import re
import sys
import unicodedata
from functools import lru_cache
from typing import Iterable, Optional, Tuple

from ..theme import BLACK, BLUE, Style, rgb

ESC = "\x1b"
CSI = f"{ESC}["
OSC = f"{ESC}]"
BEL = "\x07"
ST = f"{ESC}\\"

#: Matches every escape sequence this package emits, so styled text can be measured or
#: stripped back to what the user would copy out of the terminal.
ANSI_RE = re.compile(r"\x1b(?:\[[0-9;?]*[ -/]*[@-~]|\][^\x07\x1b]*(?:\x07|\x1b\\)|[@-Z\\-_])")


# ── colour depth ─────────────────────────────────────────────────────────────


class Depth:
    """How much colour the output stream will accept. Ordered, so `>=` works."""

    NONE = 0
    ANSI16 = 4
    ANSI256 = 8
    TRUECOLOR = 24

    NAMES = {0: "none", 4: "16-colour", 8: "256-colour", 24: "truecolor"}

    @classmethod
    def name(cls, depth: int) -> str:
        return cls.NAMES.get(depth, str(depth))


def _env_depth() -> Optional[int]:
    """A depth explicitly demanded by the environment, or None to keep probing.

    ``NO_COLOR`` wins over everything — it is a user saying "not on this machine", and a
    tool that overrides it is a tool people stop trusting. ``FORCE_COLOR`` is the inverse
    and matters in CI, where nothing is a tty but the log viewer renders colour fine.
    """
    if os.environ.get("NO_COLOR"):
        return Depth.NONE
    if os.environ.get("ALCHEM_COLOR"):
        wanted = os.environ["ALCHEM_COLOR"].strip().lower()
        return {
            "0": Depth.NONE, "none": Depth.NONE, "off": Depth.NONE,
            "16": Depth.ANSI16, "ansi": Depth.ANSI16,
            "256": Depth.ANSI256,
            "24": Depth.TRUECOLOR, "true": Depth.TRUECOLOR, "truecolor": Depth.TRUECOLOR,
        }.get(wanted, Depth.TRUECOLOR)
    force = os.environ.get("FORCE_COLOR")
    if force is not None:
        if force in ("0", "false"):
            return Depth.NONE
        return Depth.TRUECOLOR if force in ("3", "") else Depth.ANSI256
    return None


def detect_depth(stream=None) -> int:
    """Best-effort colour depth for ``stream`` (default: stdout).

    The probe order is: explicit environment override, then tty-ness, then the terminal's
    own advertisement. ``COLORTERM=truecolor`` is the reliable 24-bit signal; Windows
    Terminal and modern VS Code do not always set it, so they are recognised by name.
    """
    explicit = _env_depth()
    if explicit is not None:
        return explicit

    stream = stream if stream is not None else sys.stdout
    if not _isatty(stream):
        return Depth.NONE

    if os.environ.get("COLORTERM", "").lower() in ("truecolor", "24bit"):
        return Depth.TRUECOLOR
    if os.environ.get("WT_SESSION") or os.environ.get("TERM_PROGRAM") in (
        "vscode", "iTerm.app", "WezTerm", "ghostty", "Hyper", "Apple_Terminal"
    ):
        # Apple Terminal is 256-only; the rest of this group do 24-bit.
        return (
            Depth.ANSI256
            if os.environ.get("TERM_PROGRAM") == "Apple_Terminal"
            else Depth.TRUECOLOR
        )

    term = os.environ.get("TERM", "")
    if "256" in term:
        return Depth.ANSI256
    if term in ("dumb", ""):
        # A bare Windows console reports no TERM at all but does 24-bit once VT is on.
        return Depth.TRUECOLOR if os.name == "nt" and enable_vt() else Depth.NONE
    return Depth.ANSI16


def _isatty(stream) -> bool:
    try:
        return bool(stream.isatty())
    except (AttributeError, ValueError):  # closed, or a non-file object
        return False


# ── Windows virtual terminal ─────────────────────────────────────────────────

_VT_ENABLED: Optional[bool] = None


def enable_vt() -> bool:
    """Turn on VT processing for the Windows console. True when escapes will work.

    Idempotent and cached: this is called from the boot path, from depth detection, and
    from the screen driver, and each would otherwise re-enter the Win32 API. On anything
    that is not Windows the answer is trivially yes.
    """
    global _VT_ENABLED
    if _VT_ENABLED is not None:
        return _VT_ENABLED
    if os.name != "nt":
        _VT_ENABLED = True
        return True
    try:  # pragma: no cover - exercised only on Windows
        import ctypes

        kernel32 = ctypes.windll.kernel32
        enable_processing = 0x0004  # ENABLE_VIRTUAL_TERMINAL_PROCESSING
        ok = True
        for handle_id in (-11, -12):  # STD_OUTPUT_HANDLE, STD_ERROR_HANDLE
            handle = kernel32.GetStdHandle(handle_id)
            if handle in (0, -1):
                continue
            mode = ctypes.c_uint32()
            if not kernel32.GetConsoleMode(handle, ctypes.byref(mode)):
                continue
            if not kernel32.SetConsoleMode(handle, mode.value | enable_processing):
                ok = False
        _VT_ENABLED = ok
    except Exception:  # pragma: no cover - no console, or a stubbed ctypes
        _VT_ENABLED = False
    return _VT_ENABLED


# ── colour conversion ────────────────────────────────────────────────────────

#: xterm's 6-level cube, which is not linear — the first step is a jump of 95.
_CUBE_LEVELS = (0, 95, 135, 175, 215, 255)

#: The sixteen. Values are xterm's defaults; a terminal may remap them, which is exactly
#: why 16-colour output is the fallback rather than the target.
_ANSI16 = (
    (0, 0, 0), (128, 0, 0), (0, 128, 0), (128, 128, 0),
    (0, 0, 128), (128, 0, 128), (0, 128, 128), (192, 192, 192),
    (128, 128, 128), (255, 0, 0), (0, 255, 0), (255, 255, 0),
    (0, 0, 255), (255, 0, 255), (0, 255, 255), (255, 255, 255),
)


def _nearest_level(value: int) -> Tuple[int, int]:
    """Nearest cube level to one channel, as (index, value)."""
    best = min(range(6), key=lambda i: abs(_CUBE_LEVELS[i] - value))
    return best, _CUBE_LEVELS[best]


@lru_cache(maxsize=512)
def to_256(hex_colour: str) -> int:
    """Nearest xterm-256 index for a hex colour.

    Both the colour cube and the 24-step grey ramp are candidates and the closer one
    wins. Skipping the grey ramp is the usual bug: it turns ``#080e17`` — a near-black
    surface — into cube index 16, pure ``#000000``, and the surface stops being one step
    off the background.
    """
    r, g, b = rgb(hex_colour)

    ri, rv = _nearest_level(r)
    gi, gv = _nearest_level(g)
    bi, bv = _nearest_level(b)
    cube_index = 16 + 36 * ri + 6 * gi + bi
    cube_error = (r - rv) ** 2 + (g - gv) ** 2 + (b - bv) ** 2

    grey = round((r * 299 + g * 587 + b * 114) / 1000)
    if grey < 8:
        grey_index, grey_value = 16, 0
    elif grey > 238:
        grey_index, grey_value = 231, 255
    else:
        step = max(0, min(23, round((grey - 8) / 10)))
        grey_index, grey_value = 232 + step, 8 + 10 * step
    grey_error = (r - grey_value) ** 2 + (g - grey_value) ** 2 + (b - grey_value) ** 2

    return grey_index if grey_error < cube_error else cube_index


@lru_cache(maxsize=512)
def to_16(hex_colour: str) -> int:
    """Nearest of the sixteen basic colours, as an index 0-15."""
    r, g, b = rgb(hex_colour)
    return min(
        range(16),
        key=lambda i: (r - _ANSI16[i][0]) ** 2 + (g - _ANSI16[i][1]) ** 2 + (b - _ANSI16[i][2]) ** 2,
    )


def _colour_params(hex_colour: str, depth: int, background: bool) -> Iterable[str]:
    base = 48 if background else 38
    if depth >= Depth.TRUECOLOR:
        r, g, b = rgb(hex_colour)
        return (str(base), "2", str(r), str(g), str(b))
    if depth >= Depth.ANSI256:
        return (str(base), "5", str(to_256(hex_colour)))
    index = to_16(hex_colour)
    # 30-37 / 90-97 for foreground, 40-47 / 100-107 for background.
    if index < 8:
        return (str((40 if background else 30) + index),)
    return (str((100 if background else 90) + index - 8),)


@lru_cache(maxsize=1024)
def sgr(style: Style, depth: int) -> str:
    """Encode a style as one SGR sequence, or ``""`` when the depth forbids colour.

    Cached on (style, depth). A full-screen repaint asks for the same handful of styles
    tens of thousands of times, and re-deriving the nearest-256 index each time showed up
    as measurable frame cost on a 200-column terminal.
    """
    if depth <= Depth.NONE or style.is_plain:
        return ""
    params = ["0"]
    if style.bold:
        params.append("1")
    if style.dim:
        params.append("2")
    if style.italic:
        params.append("3")
    if style.underline:
        params.append("4")
    if style.reverse:
        params.append("7")
    if style.fg:
        params.extend(_colour_params(style.fg, depth, background=False))
    if style.bg:
        params.extend(_colour_params(style.bg, depth, background=True))
    return f"{CSI}{';'.join(params)}m"


RESET = f"{CSI}0m"


def paint(text: str, style: Style, depth: int) -> str:
    """Wrap ``text`` in one style. The workhorse behind every themed string."""
    prefix = sgr(style, depth)
    return f"{prefix}{text}{RESET}" if prefix else text


# ── cursor and screen control ────────────────────────────────────────────────


def move(row: int, column: int) -> str:
    """Absolute cursor position. 1-indexed, as the terminal counts."""
    return f"{CSI}{row};{column}H"


CLEAR_SCREEN = f"{CSI}2J"
CLEAR_LINE = f"{CSI}2K"
CLEAR_TO_EOL = f"{CSI}0K"
HIDE_CURSOR = f"{CSI}?25l"
SHOW_CURSOR = f"{CSI}?25h"
ENTER_ALT_SCREEN = f"{CSI}?1049h"
EXIT_ALT_SCREEN = f"{CSI}?1049l"
ENABLE_MOUSE = f"{CSI}?1000h{CSI}?1006h"
DISABLE_MOUSE = f"{CSI}?1006l{CSI}?1000l"
ENABLE_BRACKETED_PASTE = f"{CSI}?2004h"
DISABLE_BRACKETED_PASTE = f"{CSI}?2004l"
HOME = f"{CSI}H"

#: Steady bar. Chosen over a block so the cursor reads as an insertion point in the
#: shell rather than as a selected cell in the dashboard.
CURSOR_BAR = f"{CSI}6 q"
CURSOR_DEFAULT = f"{CSI}0 q"


def title(text: str) -> str:
    """Set the window/tab title. OSC 0 sets both icon name and title."""
    return f"{OSC}0;{text}{BEL}"


def set_foreground(hex_colour: str) -> str:
    """OSC 10 — repaint the terminal's *default* foreground."""
    return f"{OSC}10;{hex_colour}{BEL}"


def set_background(hex_colour: str) -> str:
    """OSC 11 — repaint the terminal's *default* background.

    This is what makes the product black rather than merely drawing black rectangles: the
    columns past the last painted cell, and any output from a subprocess, land on black
    too. Terminals that do not implement it ignore the sequence silently.
    """
    return f"{OSC}11;{hex_colour}{BEL}"


def set_cursor_colour(hex_colour: str) -> str:
    """OSC 12 — the cursor itself, in the accent."""
    return f"{OSC}12;{hex_colour}{BEL}"


RESET_COLOURS = f"{OSC}110{BEL}{OSC}111{BEL}{OSC}112{BEL}"
"""OSC 110/111/112 — hand the default fg/bg/cursor colours back to the terminal.

Emitted on exit. A tool that repaints someone's terminal and then dies without undoing
it has broken their terminal, not themed it.
"""


def theme_terminal(background: str = BLACK, foreground: str = BLUE) -> str:
    """The full "make this terminal ours" sequence, as one string."""
    return set_background(background) + set_foreground(foreground) + set_cursor_colour(foreground)


# ── measurement ──────────────────────────────────────────────────────────────


def strip(text: str) -> str:
    """``text`` with every escape sequence removed."""
    return ANSI_RE.sub("", text)


def char_width(char: str) -> int:
    """Display columns for one character: 0 for combining marks, 2 for wide, else 1."""
    if unicodedata.combining(char):
        return 0
    return 2 if unicodedata.east_asian_width(char) in ("W", "F") else 1


def display_width(text: str) -> int:
    """Columns ``text`` occupies once printed, ignoring escapes."""
    return sum(char_width(c) for c in strip(text))


def truncate(text: str, width: int, ellipsis: str = "…") -> str:
    """Cut plain text to ``width`` display columns, marking the cut.

    Operates on unstyled text by design. Truncating a styled string would leave a dangling
    SGR with no reset, and every caller here styles *after* laying out.
    """
    if width <= 0:
        return ""
    if display_width(text) <= width:
        return text
    marker = ellipsis if width > display_width(ellipsis) else ""
    budget = width - display_width(marker)
    out: list = []
    used = 0
    for char in text:
        step = char_width(char)
        if used + step > budget:
            break
        out.append(char)
        used += step
    return "".join(out) + marker


def pad(text: str, width: int, align: str = "left") -> str:
    """Pad plain text to exactly ``width`` display columns, truncating if too long."""
    text = truncate(text, width)
    gap = width - display_width(text)
    if gap <= 0:
        return text
    if align == "right":
        return " " * gap + text
    if align == "center":
        left = gap // 2
        return " " * left + text + " " * (gap - left)
    return text + " " * gap
