"""Themed line output — the black-and-blue for everything that is not full-screen.

The dashboard paints cells; ``alchem-link price ETH/USD`` prints lines. Both should look
like the same product, and before this module they did not: the dashboard had a palette
and the CLI had bare ``print``. :class:`Console` closes that gap by giving line-oriented
output the same :mod:`~alchem_link.theme` roles the cell grid uses.

It also ends a smaller duplication. ``_fmt_price``, ``_fmt_age`` and ``_fmt_secs`` existed
in two copies — one in the CLI, one in the TUI — and had already drifted. They live here
now, once, and both callers import them.

The one rule that matters: **colour is a decoration, never the message.** Every function
here produces the same text with colour switched off, and the tests assert it, because
this output goes into pipes, CI logs and issue reports at least as often as it goes to a
terminal. ``NO_COLOR``, a redirected stdout, or a terminal that admits to no colour each
independently reduce this to plain text with the layout intact.
"""
from __future__ import annotations

import json
import sys
from typing import Any, Dict, Iterable, List, Optional, Sequence, TextIO, Tuple

from .theme import Style, role, severity_style, status_style
from .term import ansi

# ── formatters ───────────────────────────────────────────────────────────────


def fmt_price(value: float) -> str:
    """Pick a precision for the magnitude.

    Naive trailing-zero stripping is wrong here: ``"1,900.00000000".rstrip("0")`` eats the
    integer digits too and yields ``"1,9"``.
    """
    magnitude = abs(value)
    if magnitude >= 1000:
        return f"{value:,.2f}"
    if magnitude >= 1:
        return f"{value:,.4f}"
    return f"{value:,.8f}"


def fmt_age(seconds: int) -> str:
    """A duration since something happened: ``45s``, ``18m 7s``, ``1h 1m``, ``1d 1h``."""
    seconds = int(seconds)
    if seconds < 60:
        return f"{seconds}s"
    if seconds < 3600:
        return f"{seconds // 60}m {seconds % 60}s"
    if seconds < 86400:
        return f"{seconds // 3600}h {(seconds % 3600) // 60}m"
    return f"{seconds // 86400}d {(seconds % 86400) // 3600}h"


def fmt_secs(seconds: int) -> str:
    """A configured interval, in its largest whole unit: ``1h``, ``20m``, ``451s``."""
    if not seconds:
        return "?"
    for unit, size in (("d", 86400), ("h", 3600), ("m", 60)):
        if seconds % size == 0:
            return f"{seconds // size}{unit}"
    return f"{seconds}s"


def fmt_bps(value: float) -> str:
    return f"{value:+,.1f} bps"


def fmt_pct(value: float, places: int = 2) -> str:
    return f"{value:+,.{places}f}%"


def fmt_usd(value: float) -> str:
    return f"${value:,.2f}" if abs(value) >= 0.01 else f"${value:,.6f}"


# ── console ──────────────────────────────────────────────────────────────────

#: Padding before the first column, so command output sits off the terminal edge the way
#: the dashboard's panels do.
INDENT = "  "


class Console:
    """Themed line output to a stream.

    ``depth`` is negotiated once at construction from the stream itself, so a piped run
    and a terminal run take exactly the same code path and differ only in whether
    :func:`~alchem_link.term.ansi.sgr` returns anything.
    """

    def __init__(self, stream: Optional[TextIO] = None, depth: Optional[int] = None) -> None:
        self.stream = stream if stream is not None else sys.stdout
        self.depth = depth if depth is not None else ansi.detect_depth(self.stream)

    @property
    def colour(self) -> bool:
        return self.depth > ansi.Depth.NONE

    def paint(self, text: str, style: Style) -> str:
        """Style a string without printing it — for building a line piece by piece."""
        return ansi.paint(text, style, self.depth)

    def write(self, text: str = "") -> None:
        try:
            self.stream.write(text + "\n")
        except (BrokenPipeError, ValueError):  # `| head` closed the pipe
            raise

    # ── blocks ───────────────────────────────────────────────────────────────

    def heading(self, text: str, detail: str = "") -> None:
        """A section title, optionally with a dimmer trailing detail on the same line."""
        line = self.paint(text, role("title"))
        if detail:
            line += "  " + self.paint(detail, role("hint"))
        self.write(line)

    def subheading(self, text: str) -> None:
        self.write(INDENT + self.paint(text, role("subtitle")))

    def rule(self, width: int = 60, label: str = "") -> None:
        bar = "─" * max(1, width)
        if label:
            bar = f"─── {label} " + "─" * max(1, width - len(label) - 5)
        self.write(INDENT + self.paint(bar, role("rule")))

    def line(self, text: str = "", style: str = "value", indent: bool = True) -> None:
        prefix = INDENT if indent else ""
        self.write(prefix + (self.paint(text, role(style)) if text else ""))

    def blank(self) -> None:
        self.write("")

    def kv(self, name: str, value: str, width: int = 10, value_style: str = "value") -> None:
        """One ``key   value`` row, keys aligned at ``width``."""
        self.write(
            INDENT
            + self.paint(name.ljust(width), role("key"))
            + " "
            + self.paint(value, role(value_style))
        )

    def kvs(self, pairs: Sequence[Tuple[str, str]], value_style: str = "value") -> None:
        """A block of key/value rows, aligned to the widest key."""
        if not pairs:
            return
        width = max(len(name) for name, _ in pairs)
        for name, value in pairs:
            self.kv(name, value, width=width, value_style=value_style)

    def bullet(self, text: str, marker: str = "·", style: str = "value") -> None:
        self.write(
            INDENT + self.paint(marker, role("accent")) + " " + self.paint(text, role(style))
        )

    def note(self, text: str) -> None:
        self.write(INDENT + self.paint(text, role("hint")))

    # ── verdicts ─────────────────────────────────────────────────────────────

    def status(self, label: str, text: str = "") -> None:
        """A FRESH/STALE/INVALID badge, in the colour the whole product agrees on."""
        line = INDENT + self.paint(label.ljust(7), status_style(label))
        if text:
            line += " " + self.paint(text, role("value"))
        self.write(line)

    def finding(self, severity: str, code: str, title: str, detail: str = "",
                remedy: str = "") -> None:
        """One audit finding, severity-coloured, with its detail and fix indented under."""
        mark = {"critical": "CRIT", "high": "HIGH", "medium": "MED ", "low": "LOW ",
                "info": "info"}.get(severity, severity[:4].upper())
        self.write(
            INDENT
            + self.paint(f"[{mark}]", severity_style(severity))
            + " "
            + self.paint(code, role("key"))
            + ": "
            + self.paint(title, role("value"))
        )
        if detail:
            self.write(INDENT * 4 + self.paint(detail, role("muted")))
        if remedy:
            self.write(INDENT * 4 + self.paint("fix: " + remedy, role("ok")))

    def ok(self, text: str) -> None:
        self.write(INDENT + self.paint(text, role("ok")))

    def warn(self, text: str) -> None:
        self.write(INDENT + self.paint(text, role("warn")))

    def error(self, text: str) -> None:
        self.write(INDENT + self.paint(text, role("bad")))

    def check(self, passed: bool, name: str, detail: str = "", hint: str = "") -> None:
        """A ``[ ok ] name  detail`` row — what ``doctor`` and ``verify`` print."""
        mark = "ok  " if passed else "FAIL"
        self.write(
            INDENT
            + "[" + self.paint(mark, role("ok" if passed else "bad")) + "] "
            + self.paint(name.ljust(18), role("key"))
            + " " + self.paint(detail, role("value"))
        )
        if hint:
            self.write(INDENT * 5 + self.paint(hint, role("hint")))

    # ── tables ───────────────────────────────────────────────────────────────

    def table(self, columns: Sequence[str], rows: Sequence[Sequence[str]],
              aligns: Optional[Sequence[str]] = None,
              styles: Optional[Sequence[Optional[str]]] = None,
              row_styles: Optional[Sequence[Optional[str]]] = None,
              header: bool = True) -> None:
        """A column-aligned table.

        Widths are measured with :func:`~alchem_link.term.ansi.display_width` rather than
        ``len``, so a pair name containing a wide glyph does not shift its column — and so
        the same measurement code serves this and the full-screen grid.
        """
        if not rows:
            return
        cells = [[str(c) for c in row] for row in rows]
        count = max(len(columns), max(len(r) for r in cells))
        aligns = list(aligns or []) + ["left"] * (count - len(aligns or []))
        widths = [
            max(
                ansi.display_width(columns[i]) if i < len(columns) else 0,
                *(ansi.display_width(row[i]) if i < len(row) else 0 for row in cells),
            )
            for i in range(count)
        ]

        if header:
            self.write(INDENT + "  ".join(
                self.paint(
                    ansi.pad(columns[i] if i < len(columns) else "", widths[i], aligns[i]),
                    role("column"),
                )
                for i in range(count)
            ))

        for index, row in enumerate(cells):
            row_style = (row_styles[index] if row_styles and index < len(row_styles) else None)
            self.write(INDENT + "  ".join(
                self.paint(
                    ansi.pad(row[i] if i < len(row) else "", widths[i], aligns[i]),
                    role(row_style or (styles[i] if styles and i < len(styles) and styles[i]
                                       else "value")),
                )
                for i in range(count)
            ))

    # ── structured ───────────────────────────────────────────────────────────

    def json(self, payload: Any) -> None:
        """Machine-readable output. Never coloured — this is somebody's ``jq`` input."""
        self.write(json.dumps(payload, indent=2, sort_keys=True, default=str))

    def banner(self, lines: Sequence[str], subtitle: str = "") -> None:
        for line in lines:
            self.write(self.paint(line, role("banner")))
        if subtitle:
            self.write(INDENT + self.paint(subtitle, role("hint")))


#: The default console. Built lazily so importing the package never probes a stream.
_console: Optional[Console] = None


def console(stream: Optional[TextIO] = None) -> Console:
    """The shared console, or a fresh one bound to ``stream``."""
    global _console
    if stream is not None:
        return Console(stream)
    if _console is None:
        _console = Console()
    return _console


def reset_console() -> None:
    """Forget the cached console so the next call re-detects colour depth.

    Needed after :func:`alchem_link.term.boot.initialize` enables VT on Windows: a console
    built before that call negotiated its depth against a stream that could not yet render
    escapes, and would print the rest of the session in plain text.
    """
    global _console
    _console = None
