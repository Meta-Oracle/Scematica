"""A double-buffered character grid, and the diff that gets it onto a terminal.

The dashboard repaints on a timer, on every keystroke, and whenever a worker thread
lands a result. Redrawing the whole screen each time is what makes a terminal UI flicker
and what makes it unusable over SSH — a 120x40 screen is ~4800 cells, and with colour
that is tens of kilobytes per frame.

So :class:`Screen` keeps two buffers. Widgets paint into the back buffer, which is cheap
because it is just list assignment. :meth:`Screen.flush` then compares it against what is
actually on the terminal and emits only the runs that changed, with one cursor move per
run and one SGR per style change inside it. A frame where nothing moved costs zero bytes.

Wide characters get explicit handling. A CJK glyph occupies two columns, so it is stored
as the character plus a continuation cell, and writing over either half must erase the
other — otherwise the terminal's idea of the cursor column drifts from ours and every
subsequent cell on that row lands one place off.
"""
from __future__ import annotations

import sys
from typing import List, Optional, Sequence, TextIO, Tuple

from ..theme import BASE, Style
from . import ansi

#: A cell is (character, style). ``""`` is the right half of a wide character and is
#: never emitted — the lead cell already advanced the terminal's cursor past it.
Cell = Tuple[str, Style]

BLANK = " "

# Box-drawing sets. `light` is the default; `heavy` marks the focused pane without
# needing a second colour, which matters at 16-colour depth where the border and the
# focus border collapse to the same index.
BOX_LIGHT = "┌┐└┘─│├┤┬┴┼"
BOX_HEAVY = "┏┓┗┛━┃┣┫┳┻╋"
BOX_DOUBLE = "╔╗╚╝═║╠╣╦╩╬"
BOX_ASCII = "++++-|++++"


class Screen:
    """A grid of styled cells with a minimal-diff flush.

    ``depth`` is fixed at construction. It comes from :func:`alchem_link.term.ansi
    .detect_depth` in real use and is passed explicitly in tests, which is how the same
    render code is exercised at truecolor and at no-colour without a terminal.
    """

    def __init__(self, width: int, height: int, depth: int = ansi.Depth.TRUECOLOR,
                 base: Style = BASE) -> None:
        self.width = max(1, width)
        self.height = max(1, height)
        self.depth = depth
        self.base = base
        self._back: List[List[Cell]] = self._blank_grid()
        # Front starts deliberately unequal to back so the first flush paints everything.
        self._front: List[List[Cell]] = [[("\x00", base)] * self.width for _ in range(self.height)]
        self._cursor: Optional[Tuple[int, int]] = None

    # ── buffer management ────────────────────────────────────────────────────

    def _blank_grid(self) -> List[List[Cell]]:
        return [[(BLANK, self.base) for _ in range(self.width)] for _ in range(self.height)]

    def resize(self, width: int, height: int) -> None:
        """Adopt a new size. Both buffers are discarded — the next flush is a full paint.

        Trying to preserve content across a resize is a false economy: the layout above
        recomputes from scratch anyway, and a partially-migrated front buffer would make
        the diff believe cells are already correct when they hold the old layout.
        """
        self.width = max(1, width)
        self.height = max(1, height)
        self._back = self._blank_grid()
        self._front = [[("\x00", self.base)] * self.width for _ in range(self.height)]

    def clear(self, style: Optional[Style] = None) -> None:
        """Reset the back buffer to blanks. Called at the top of every frame."""
        fill = style or self.base
        blank = (BLANK, fill)
        for row in self._back:
            for x in range(self.width):
                row[x] = blank
        self._cursor = None

    def set_cursor(self, row: int, column: int) -> None:
        """Park the hardware cursor here after the flush, and show it.

        Only the shell uses this; the dashboard leaves it None and hides the cursor.
        """
        self._cursor = (row, column)

    # ── primitives ───────────────────────────────────────────────────────────

    def _in_bounds(self, row: int, column: int) -> bool:
        return 0 <= row < self.height and 0 <= column < self.width

    def _clobber(self, row: int, column: int) -> None:
        """Erase the other half of any wide character overlapping this cell.

        Two cases. Writing onto a continuation cell orphans the lead to its left; writing
        onto a lead orphans the continuation to its right. Both leave the terminal drawing
        half a glyph and, worse, disagreeing with us about the column.
        """
        line = self._back[row]
        if line[column][0] == "" and column > 0:
            line[column - 1] = (BLANK, self.base)
        if (
            ansi.char_width(line[column][0] or " ") == 2
            and column + 1 < self.width
            and line[column + 1][0] == ""
        ):
            line[column + 1] = (BLANK, self.base)

    def put_char(self, row: int, column: int, char: str, style: Style) -> int:
        """Write one character. Returns the columns it consumed (0, 1 or 2)."""
        if not self._in_bounds(row, column) or not char:
            return 0
        width = ansi.char_width(char)
        if width == 0:  # a combining mark has no cell of its own
            return 0
        if column + width > self.width:  # a wide glyph would straddle the edge
            self._clobber(row, column)
            self._back[row][column] = (BLANK, style)
            return 1
        self._clobber(row, column)
        self._back[row][column] = (char, style)
        if width == 2:
            self._clobber(row, column + 1)
            self._back[row][column + 1] = ("", style)
        return width

    def put(self, row: int, column: int, text: str, style: Optional[Style] = None,
            max_width: Optional[int] = None) -> int:
        """Write plain text at (row, column). Returns the columns consumed.

        ``text`` must be unstyled — this package lays out first and styles second, so a
        string arriving here with escapes in it is a bug upstream, and the escapes are
        stripped rather than printed as glyphs.
        """
        if not self._in_bounds(row, 0) or column >= self.width:
            return 0
        style = style or self.base
        if "\x1b" in text:
            text = ansi.strip(text)
        limit = self.width if max_width is None else min(self.width, column + max_width)
        used = 0
        x = column
        for char in text:
            if char in ("\n", "\r"):
                break
            if x >= limit:
                break
            step = self.put_char(row, x, char, style)
            x += step
            used += step
        return used

    def fill(self, row: int, column: int, width: int, style: Style, char: str = BLANK) -> None:
        """Paint a horizontal run — the backing for panel bodies and gauge tracks."""
        for x in range(column, min(self.width, column + width)):
            if self._in_bounds(row, x):
                self._clobber(row, x)
                self._back[row][x] = (char, style)

    def fill_rect(self, row: int, column: int, width: int, height: int, style: Style,
                  char: str = BLANK) -> None:
        for y in range(row, min(self.height, row + height)):
            self.fill(y, column, width, style, char)

    def hline(self, row: int, column: int, width: int, style: Style, char: str = "─") -> None:
        self.fill(row, column, width, style, char)

    def vline(self, row: int, column: int, height: int, style: Style, char: str = "│") -> None:
        for y in range(row, min(self.height, row + height)):
            self.put_char(y, column, char, style)

    def box(self, row: int, column: int, width: int, height: int, style: Style,
            chars: str = BOX_LIGHT, fill_style: Optional[Style] = None) -> None:
        """Draw a border, optionally clearing the interior first.

        ``fill_style`` exists because a panel drawn over another panel's leftovers looks
        like a rendering bug. Passing it paints the interior in one go rather than making
        every caller remember a matching :meth:`fill_rect`.
        """
        if width < 2 or height < 2:
            return
        tl, tr, bl, br, horizontal, vertical = chars[0], chars[1], chars[2], chars[3], chars[4], chars[5]
        if fill_style is not None:
            self.fill_rect(row + 1, column + 1, width - 2, height - 2, fill_style)
        self.hline(row, column + 1, width - 2, style, horizontal)
        self.hline(row + height - 1, column + 1, width - 2, style, horizontal)
        self.vline(row + 1, column, height - 2, style, vertical)
        self.vline(row + 1, column + width - 1, height - 2, style, vertical)
        self.put_char(row, column, tl, style)
        self.put_char(row, column + width - 1, tr, style)
        self.put_char(row + height - 1, column, bl, style)
        self.put_char(row + height - 1, column + width - 1, br, style)

    # ── output ───────────────────────────────────────────────────────────────

    def _row_diff(self, y: int) -> str:
        """Escape sequence bringing terminal row ``y`` in line with the back buffer."""
        back, front = self._back[y], self._front[y]
        if back == front:
            return ""

        out: List[str] = []
        x = 0
        while x < self.width:
            if back[x] == front[x]:
                x += 1
                continue

            # A run of changed cells. Short unchanged gaps inside it are cheaper to
            # repaint than to break the run for — a cursor move costs ~6 bytes, so
            # anything under that threshold is not worth the jump.
            start = x
            gap = 0
            end = x
            while x < self.width:
                if back[x] == front[x]:
                    gap += 1
                    if gap > 6:
                        break
                else:
                    gap = 0
                    end = x
                x += 1
            end += 1

            out.append(ansi.move(y + 1, start + 1))
            current: Optional[Style] = None
            for cx in range(start, end):
                char, style = back[cx]
                if char == "":  # continuation of a wide glyph — already emitted
                    continue
                if style != current:
                    encoded = ansi.sgr(style, self.depth)
                    if encoded:
                        out.append(encoded)
                    elif current is not None:
                        out.append(ansi.RESET)
                    current = style
                out.append(char or BLANK)
            if current is not None and self.depth > ansi.Depth.NONE:
                out.append(ansi.RESET)
        return "".join(out)

    def render(self) -> str:
        """The escape sequence for this frame, without writing it anywhere.

        Split out from :meth:`flush` so tests can assert on the bytes, and so an app can
        buffer a frame and decide not to send it.
        """
        parts = [self._row_diff(y) for y in range(self.height)]
        frame = "".join(parts)
        if not frame:
            return ""
        if self._cursor is not None:
            row, column = self._cursor
            frame += ansi.move(row + 1, column + 1) + ansi.SHOW_CURSOR
        return frame

    def flush(self, stream: Optional[TextIO] = None) -> int:
        """Write this frame's diff and adopt the back buffer as the new front.

        Returns the byte count written, which the dashboard's debug overlay reports —
        seeing "0 bytes" on an idle frame is how you confirm the diff is doing its job.
        """
        frame = self.render()
        # Swap regardless of whether anything was written: the front buffer must always
        # describe the terminal, and an empty frame means they already agreed.
        self._front = [list(row) for row in self._back]
        if not frame:
            return 0
        target = stream if stream is not None else sys.stdout
        target.write(frame)
        target.flush()
        return len(frame)

    def invalidate(self) -> None:
        """Forget what is on the terminal, forcing the next flush to repaint everything.

        Needed after anything writes to the terminal behind the screen's back — a resize,
        a subprocess, or returning from a suspended session.
        """
        self._front = [[("\x00", self.base)] * self.width for _ in range(self.height)]

    # ── inspection (tests, snapshots) ────────────────────────────────────────

    def text_rows(self) -> List[str]:
        """The back buffer as plain text, one string per row, right-trimmed.

        This is what makes widgets testable without a terminal: paint into a Screen, read
        the rows back, assert on what a person would see.
        """
        rows = []
        for line in self._back:
            rows.append("".join(char for char, _ in line if char != "").rstrip())
        return rows

    def text(self) -> str:
        return "\n".join(self.text_rows())

    def style_at(self, row: int, column: int) -> Style:
        """The style of one cell — for asserting that a status badge is actually red."""
        if not self._in_bounds(row, column):
            raise IndexError(f"({row}, {column}) is outside {self.width}x{self.height}")
        return self._back[row][column][1]

    def char_at(self, row: int, column: int) -> str:
        if not self._in_bounds(row, column):
            raise IndexError(f"({row}, {column}) is outside {self.width}x{self.height}")
        return self._back[row][column][0]


def terminal_size(fallback: Sequence[int] = (100, 30)) -> Tuple[int, int]:
    """Current terminal size as (columns, rows), with a usable fallback.

    ``shutil.get_terminal_size`` already falls back, but it returns 80x24 when it cannot
    tell, and the dashboard's sidebar plus a table does not fit in 80 columns. A slightly
    generous default degrades better than a cramped one.
    """
    import shutil

    try:
        size = shutil.get_terminal_size()
        columns, rows = size.columns, size.lines
    except (OSError, ValueError):  # pragma: no cover - no controlling terminal
        columns, rows = fallback
    if columns <= 0 or rows <= 0:
        columns, rows = fallback
    return columns, rows
