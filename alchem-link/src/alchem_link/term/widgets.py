"""Drawing primitives above the cell grid: panels, tables, sparklines, gauges, tabs.

Everything here paints into a :class:`~alchem_link.term.screen.Screen` region described
by a :class:`Rect`, and everything here is a plain function or a small dataclass. There
is no widget tree, no layout solver, and no reactive system — the dashboard recomputes
its whole layout every frame, which at terminal sizes costs nothing measurable and
removes an entire category of "the pane did not resize" bugs.

Two conventions hold throughout:

* **Layout is done on plain text; colour is applied by the cell.** Column arithmetic over
  a string containing escape sequences is wrong by however much colour it carries, so no
  function here ever receives pre-styled text.
* **Nothing draws outside its rect.** Every helper clips. A table given more rows than
  fit renders what fits and reports the overflow, rather than painting over the panel
  border below it.
"""
from __future__ import annotations

from dataclasses import dataclass, field
from typing import Callable, Iterable, List, Optional, Sequence, Tuple

from ..theme import BASE, Style, role
from . import ansi
from .screen import BOX_HEAVY, BOX_LIGHT, Screen

#: Eight levels of block, the classic sparkline ramp.
SPARK_CHARS = "▁▂▃▄▅▆▇█"
#: Gauge track and fill. The fill is a full block so it reads as a solid bar even where
#: the terminal's font renders the shade characters as hollow.
GAUGE_FULL = "█"
GAUGE_PARTIAL = "▌"
GAUGE_EMPTY = "─"


@dataclass(frozen=True)
class Rect:
    """A region of the screen, in 0-indexed cell coordinates."""

    row: int
    column: int
    width: int
    height: int

    @property
    def bottom(self) -> int:
        return self.row + self.height

    @property
    def right(self) -> int:
        return self.column + self.width

    def inset(self, top: int = 0, right: int = 0, bottom: int = 0, left: int = 0) -> "Rect":
        """Shrink on each side, clamping to a non-negative size."""
        return Rect(
            row=self.row + top,
            column=self.column + left,
            width=max(0, self.width - left - right),
            height=max(0, self.height - top - bottom),
        )

    def pad(self, amount: int) -> "Rect":
        return self.inset(amount, amount, amount, amount)

    def split_h(self, at: int) -> Tuple["Rect", "Rect"]:
        """Split into left and right. ``at`` is a column count; negative counts from the right."""
        cut = max(0, min(self.width, self.width + at if at < 0 else at))
        left = Rect(self.row, self.column, cut, self.height)
        right = Rect(self.row, self.column + cut, self.width - cut, self.height)
        return left, right

    def split_v(self, at: int) -> Tuple["Rect", "Rect"]:
        """Split into top and bottom. ``at`` is a row count; negative counts from the bottom."""
        cut = max(0, min(self.height, self.height + at if at < 0 else at))
        top = Rect(self.row, self.column, self.width, cut)
        bottom = Rect(self.row + cut, self.column, self.width, self.height - cut)
        return top, bottom

    def rows(self) -> Iterable[int]:
        return range(self.row, self.bottom)

    @property
    def is_empty(self) -> bool:
        return self.width <= 0 or self.height <= 0


# ── text ─────────────────────────────────────────────────────────────────────


def wrap(text: str, width: int) -> List[str]:
    """Greedy word wrap on display width, breaking over-long words rather than dropping them."""
    if width <= 0:
        return []
    lines: List[str] = []
    for paragraph in text.split("\n"):
        if not paragraph.strip():
            lines.append("")
            continue
        current = ""
        for word in paragraph.split():
            candidate = f"{current} {word}" if current else word
            if ansi.display_width(candidate) <= width:
                current = candidate
                continue
            if current:
                lines.append(current)
            # A single word longer than the line — hard-break it into full-width pieces.
            while ansi.display_width(word) > width:
                lines.append(ansi.truncate(word, width, ellipsis=""))
                cut = len(ansi.truncate(word, width, ellipsis=""))
                word = word[cut:]
            current = word
        if current:
            lines.append(current)
    return lines


def draw_text(screen: Screen, rect: Rect, text: str, style: Optional[Style] = None,
              align: str = "left") -> None:
    """One line of text, clipped and aligned inside ``rect``."""
    if rect.is_empty:
        return
    screen.put(rect.row, rect.column, ansi.pad(text, rect.width, align), style)


def draw_paragraph(screen: Screen, rect: Rect, text: str, style: Optional[Style] = None) -> int:
    """Wrapped text, clipped to the rect. Returns the rows used."""
    if rect.is_empty:
        return 0
    lines = wrap(text, rect.width)[: rect.height]
    for offset, line in enumerate(lines):
        screen.put(rect.row + offset, rect.column, line, style)
    return len(lines)


# ── panels ───────────────────────────────────────────────────────────────────


def panel(screen: Screen, rect: Rect, title: str = "", focused: bool = False,
          style: Optional[Style] = None, title_style: Optional[Style] = None,
          fill: Optional[Style] = None) -> Rect:
    """Draw a bordered panel and return the rect its contents may use.

    The focused panel gets a heavy border rather than only a brighter one, because at
    16-colour depth ``border`` and ``border_focus`` collapse to the same index and the
    focus indication would vanish exactly where it is most needed.
    """
    if rect.width < 2 or rect.height < 2:
        return rect
    border = style or role("border_focus" if focused else "border")
    screen.box(rect.row, rect.column, rect.width, rect.height, border,
               chars=BOX_HEAVY if focused else BOX_LIGHT, fill_style=fill)
    if title:
        label = ansi.truncate(f" {title} ", max(0, rect.width - 4))
        screen.put(rect.row, rect.column + 2, label, title_style or role("title"))
    return rect.pad(1)


def rule(screen: Screen, rect: Rect, style: Optional[Style] = None, label: str = "") -> None:
    """A horizontal divider, optionally with an inline label."""
    if rect.is_empty:
        return
    line_style = style or role("rule")
    screen.hline(rect.row, rect.column, rect.width, line_style)
    if label:
        text = f" {label} "
        screen.put(rect.row, rect.column + 2, ansi.truncate(text, rect.width - 4),
                   role("muted"))


# ── key/value ────────────────────────────────────────────────────────────────


def key_value(screen: Screen, rect: Rect, pairs: Sequence[Tuple[str, str]],
              key_width: Optional[int] = None, value_style: Optional[Style] = None) -> int:
    """A column of ``key   value`` rows. Returns rows used.

    ``key_width`` defaults to the widest key, which is what makes a block of these line
    up without every caller measuring first.
    """
    if rect.is_empty or not pairs:
        return 0
    width = key_width or max(ansi.display_width(k) for k, _ in pairs)
    width = min(width, max(1, rect.width - 4))
    used = 0
    for name, value in pairs:
        if used >= rect.height:
            break
        row = rect.row + used
        screen.put(row, rect.column, ansi.pad(name, width), role("key"))
        screen.put(row, rect.column + width + 2,
                   ansi.truncate(value, max(0, rect.width - width - 2)),
                   value_style or role("value"))
        used += 1
    return used


# ── tables ───────────────────────────────────────────────────────────────────


@dataclass
class Column:
    """One table column.

    ``width`` is a fixed cell count; ``flex`` shares out whatever is left over. A column
    with neither sizes to its widest cell, which is what you want for a pair name and
    never what you want for a description.
    """

    title: str
    width: Optional[int] = None
    flex: int = 0
    align: str = "left"
    style: Optional[Style] = None


@dataclass
class Row:
    """One table row: the cell texts, plus optional per-cell and whole-row styling."""

    cells: Sequence[str]
    style: Optional[Style] = None
    cell_styles: Sequence[Optional[Style]] = field(default_factory=tuple)

    def style_for(self, index: int, default: Optional[Style]) -> Optional[Style]:
        if index < len(self.cell_styles) and self.cell_styles[index] is not None:
            return self.cell_styles[index]
        return self.style or default


def resolve_widths(columns: Sequence[Column], rows: Sequence[Row], total: int,
                   gap: int = 2) -> List[int]:
    """Share ``total`` columns out among ``columns``.

    Fixed widths are honoured, content-sized columns measure their own cells, and flex
    columns divide the remainder in proportion. When the result overflows — a narrow
    terminal — every column is scaled down together rather than the last one being cut
    off the screen, so a table stays readable at 60 columns instead of losing its
    rightmost field entirely.
    """
    count = len(columns)
    if count == 0 or total <= 0:
        return []
    gaps = gap * (count - 1)
    available = max(count, total - gaps)

    widths: List[Optional[int]] = []
    for index, column in enumerate(columns):
        if column.width is not None:
            widths.append(column.width)
        elif column.flex:
            widths.append(None)
        else:
            longest = ansi.display_width(column.title)
            for row in rows:
                if index < len(row.cells):
                    longest = max(longest, ansi.display_width(str(row.cells[index])))
            widths.append(longest)

    fixed = sum(w for w in widths if w is not None)
    flex_total = sum(c.flex for c in columns if c.flex)
    remaining = max(0, available - fixed)
    if flex_total:
        assigned = 0
        flex_indexes = [i for i, c in enumerate(columns) if c.flex]
        for position, index in enumerate(flex_indexes):
            if position == len(flex_indexes) - 1:
                share = remaining - assigned  # the last one absorbs the rounding
            else:
                share = remaining * columns[index].flex // flex_total
                assigned += share
            widths[index] = max(1, share)

    final = [w if w is not None else 1 for w in widths]
    overflow = sum(final) + gaps - total
    if overflow > 0:
        # Scale everything proportionally, never below one cell.
        scale = max(0.0, (total - gaps)) / max(1, sum(final))
        final = [max(1, int(w * scale)) for w in final]
        # Integer scaling can still leave a cell or two over; shave the widest.
        while sum(final) + gaps > total and max(final) > 1:
            final[final.index(max(final))] -= 1
    return final


def table(screen: Screen, rect: Rect, columns: Sequence[Column], rows: Sequence[Row],
          offset: int = 0, selected: Optional[int] = None, gap: int = 2,
          header: bool = True) -> int:
    """Draw a table, scrolled to ``offset``. Returns the number of rows drawn.

    ``selected`` is an absolute row index, not a visible one, so the caller keeps a single
    selection number and does not have to translate it through the scroll position.
    """
    if rect.is_empty:
        return 0
    widths = resolve_widths(columns, rows, rect.width, gap)
    body = rect
    if header:
        x = rect.column
        for column, width in zip(columns, widths):
            screen.put(rect.row, x, ansi.pad(ansi.truncate(column.title, width), width,
                                             column.align), role("column"))
            x += width + gap
        body = rect.inset(top=1)

    drawn = 0
    for index in range(offset, len(rows)):
        if drawn >= body.height:
            break
        row = rows[index]
        y = body.row + drawn
        is_selected = selected is not None and index == selected
        if is_selected:
            screen.fill(y, rect.column, rect.width, role("selected"))
        x = body.column
        for position, (column, width) in enumerate(zip(columns, widths)):
            text = str(row.cells[position]) if position < len(row.cells) else ""
            cell_style = (
                role("selected") if is_selected
                else row.style_for(position, column.style or role("value"))
            )
            screen.put(y, x, ansi.pad(ansi.truncate(text, width), width, column.align),
                       cell_style)
            x += width + gap
        drawn += 1
    return drawn


# ── charts ───────────────────────────────────────────────────────────────────


def sparkline(values: Sequence[float], width: Optional[int] = None) -> str:
    """A one-line block-character chart of ``values``.

    Flat input renders as a mid-level line rather than an empty one: a feed whose price
    has not moved is a real and interesting state, and drawing it as blank makes it look
    like missing data.
    """
    numbers = [float(v) for v in values if v is not None]
    if not numbers:
        return ""
    if width and len(numbers) > width:
        # Downsample by averaging buckets — picking every Nth point hides spikes, which
        # are the only reason to look at a sparkline.
        bucket = len(numbers) / width
        numbers = [
            sum(numbers[int(i * bucket):max(int((i + 1) * bucket), int(i * bucket) + 1)])
            / max(1, len(numbers[int(i * bucket):max(int((i + 1) * bucket), int(i * bucket) + 1)]))
            for i in range(width)
        ]
    low, high = min(numbers), max(numbers)
    if high == low:
        return SPARK_CHARS[len(SPARK_CHARS) // 2] * len(numbers)
    span = high - low
    steps = len(SPARK_CHARS) - 1
    return "".join(SPARK_CHARS[int((v - low) / span * steps + 0.5)] for v in numbers)


def gauge(screen: Screen, rect: Rect, fraction: float, full_style: Optional[Style] = None,
          empty_style: Optional[Style] = None, label: str = "") -> None:
    """A horizontal bar filled to ``fraction`` (clamped to 0..1), with an optional label."""
    if rect.is_empty:
        return
    fraction = max(0.0, min(1.0, fraction))
    label_width = ansi.display_width(label) + 1 if label else 0
    track = max(0, rect.width - label_width)
    if track <= 0:
        return
    filled = int(track * fraction)
    partial = track * fraction - filled >= 0.5 and filled < track

    screen.fill(rect.row, rect.column, filled, full_style or role("gauge_full"), GAUGE_FULL)
    if partial:
        screen.put_char(rect.row, rect.column + filled, GAUGE_PARTIAL,
                        full_style or role("gauge_full"))
    start = rect.column + filled + (1 if partial else 0)
    screen.fill(rect.row, start, rect.column + track - start,
                empty_style or role("gauge_empty"), GAUGE_EMPTY)
    if label:
        screen.put(rect.row, rect.column + track + 1, label, role("muted"))


def bar_rows(screen: Screen, rect: Rect, entries: Sequence[Tuple[str, float]],
             label_width: Optional[int] = None,
             style_for: Optional[Callable[[str, float], Style]] = None) -> int:
    """A vertical list of labelled horizontal bars, scaled to the largest value."""
    if rect.is_empty or not entries:
        return 0
    width = label_width or min(20, max(ansi.display_width(name) for name, _ in entries))
    peak = max((abs(value) for _, value in entries), default=0.0) or 1.0
    used = 0
    for name, value in entries:
        if used >= rect.height:
            break
        row = rect.row + used
        screen.put(row, rect.column, ansi.pad(ansi.truncate(name, width), width), role("key"))
        bar = Rect(row, rect.column + width + 1, max(0, rect.width - width - 1), 1)
        gauge(screen, bar, abs(value) / peak,
              full_style=style_for(name, value) if style_for else None,
              label=f"{value:,.2f}")
        used += 1
    return used


# ── navigation chrome ────────────────────────────────────────────────────────


def tabs(screen: Screen, rect: Rect, labels: Sequence[str], active: int,
         gap: int = 1) -> List[Tuple[int, int, int]]:
    """A single row of tabs. Returns (index, start_column, width) so clicks can hit-test."""
    if rect.is_empty:
        return []
    spans: List[Tuple[int, int, int]] = []
    x = rect.column
    for index, label in enumerate(labels):
        text = f" {label} "
        width = ansi.display_width(text)
        if x + width > rect.right:
            break
        style = role("selected") if index == active else role("muted")
        screen.put(rect.row, x, text, style)
        spans.append((index, x, width))
        x += width + gap
    return spans


def sidebar(screen: Screen, rect: Rect, items: Sequence[str], active: int,
            title: str = "") -> None:
    """The dashboard's left rail: a title, then one selectable row per item."""
    if rect.is_empty:
        return
    screen.fill_rect(rect.row, rect.column, rect.width, rect.height, role("sidebar"))
    top = rect
    if title:
        screen.put(rect.row, rect.column,
                   ansi.pad(ansi.truncate(f" {title}", rect.width), rect.width),
                   role("sidebar_title"))
        top = rect.inset(top=2)
    for offset, label in enumerate(items):
        if offset >= top.height:
            break
        row = top.row + offset
        style = role("selected") if offset == active else role("sidebar")
        screen.put(row, top.column, ansi.pad(ansi.truncate(f" {label}", top.width), top.width),
                   style)


def status_bar(screen: Screen, rect: Rect, left: str = "", right: str = "",
               style: Optional[Style] = None) -> None:
    """The bottom strip: context on the left, keybindings on the right."""
    if rect.is_empty:
        return
    bar = style or role("footer")
    screen.fill(rect.row, rect.column, rect.width, bar)
    screen.put(rect.row, rect.column + 1, ansi.truncate(left, rect.width - 2), bar)
    if right:
        text = ansi.truncate(right, max(0, rect.width - ansi.display_width(left) - 3))
        screen.put(rect.row, rect.right - ansi.display_width(text) - 1, text, bar)


def badge(screen: Screen, row: int, column: int, label: str, style: Style) -> int:
    """A padded, coloured label. Returns the columns used."""
    text = f" {label} "
    screen.put(row, column, text, style)
    return ansi.display_width(text)


# ── scrolling ────────────────────────────────────────────────────────────────


@dataclass
class Scroll:
    """Viewport state for a scrollable list — offset, selection, and the clamping.

    Kept as its own object because every scrollable panel needs the same six lines of
    "did the selection just leave the window" arithmetic, and each hand-written copy got
    the boundary case slightly differently wrong.
    """

    offset: int = 0
    selected: int = 0

    def clamp(self, total: int, height: int) -> None:
        """Bring offset and selection back into range for a list of ``total`` items."""
        if total <= 0:
            self.offset = self.selected = 0
            return
        self.selected = max(0, min(total - 1, self.selected))
        height = max(1, height)
        self.offset = max(0, min(self.offset, max(0, total - height)))
        if self.selected < self.offset:
            self.offset = self.selected
        elif self.selected >= self.offset + height:
            self.offset = self.selected - height + 1

    def move(self, delta: int, total: int, height: int) -> None:
        self.selected += delta
        self.clamp(total, height)

    def page(self, direction: int, total: int, height: int) -> None:
        self.move(direction * max(1, height - 1), total, height)

    def home(self, total: int, height: int) -> None:
        self.selected = 0
        self.clamp(total, height)

    def end(self, total: int, height: int) -> None:
        self.selected = max(0, total - 1)
        self.clamp(total, height)


def scrollbar(screen: Screen, rect: Rect, total: int, offset: int, height: int) -> None:
    """A one-column indicator on the right edge of a scrollable region."""
    if rect.is_empty or total <= height or rect.height < 2:
        return
    thumb = max(1, int(rect.height * height / total))
    top = int(rect.height * offset / total)
    top = min(top, rect.height - thumb)
    for y in range(rect.height):
        inside = top <= y < top + thumb
        screen.put_char(rect.row + y, rect.column, "│" if inside else "╎",
                        role("accent") if inside else role("hint"))
