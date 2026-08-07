"""Keyboard input: raw mode, and a parser that turns byte soup into named keys.

Terminals do not report keys. They report *characters*, and for anything that is not a
character they report a short escape sequence whose grammar predates most of the people
reading this. ``Up`` arrives as ``ESC [ A``; ``Ctrl-Up`` as ``ESC [ 1;5A``; ``F5`` as
``ESC [ 15~`` — and a bare ``Esc`` arrives as ``ESC`` with nothing after it, which is
indistinguishable from the start of any of those until you wait and see.

:func:`parse_key` is a pure function over a buffer, returning the key it consumed and
what is left. Keeping it pure is what makes the interesting cases testable — every
sequence below is covered in ``tests/test_term.py`` without a terminal in sight.

Raw mode is platform-split. POSIX uses ``termios`` in cbreak; Windows reads through
``msvcrt``, which delivers special keys as a ``\\x00``/``\\xe0`` prefix pair rather than
an escape sequence, so those are translated into the same :class:`Key` values. The rest
of the package never learns which platform it is on.
"""
from __future__ import annotations

import contextlib
import os
import sys
import time
from dataclasses import dataclass
from typing import Iterator, List, Optional, Tuple

# ── key model ────────────────────────────────────────────────────────────────

UP, DOWN, LEFT, RIGHT = "up", "down", "left", "right"
HOME, END, PAGE_UP, PAGE_DOWN = "home", "end", "pageup", "pagedown"
ENTER, TAB, BACKTAB, BACKSPACE, ESCAPE = "enter", "tab", "backtab", "backspace", "escape"
DELETE, INSERT = "delete", "insert"
RESIZE = "resize"


@dataclass(frozen=True)
class Key:
    """One key press.

    ``name`` is the canonical name — ``"up"``, ``"enter"``, or the character itself for
    ordinary typing. ``char`` is set only when the key produced text, so an input widget
    can append ``key.char`` without filtering out ``"pagedown"`` first.
    """

    name: str
    char: str = ""
    ctrl: bool = False
    alt: bool = False
    shift: bool = False

    def __str__(self) -> str:
        parts = []
        if self.ctrl:
            parts.append("ctrl")
        if self.alt:
            parts.append("alt")
        if self.shift:
            parts.append("shift")
        parts.append(self.name)
        return "+".join(parts)

    @property
    def is_text(self) -> bool:
        """True when this key should insert into a text buffer."""
        return bool(self.char) and not self.ctrl and not self.alt


@dataclass(frozen=True)
class Mouse:
    """An SGR-encoded mouse event, 0-indexed to match :class:`~.screen.Screen`."""

    row: int
    column: int
    button: int
    pressed: bool
    #: 64 and 65 are wheel up/down in the SGR protocol; surfaced as a flag because
    #: scrolling a list is a different action from clicking it.
    wheel: int = 0


# ── parsing ──────────────────────────────────────────────────────────────────

_CSI_FINAL = {
    "A": UP, "B": DOWN, "C": RIGHT, "D": LEFT,
    "H": HOME, "F": END,
    "Z": BACKTAB,
}

_TILDE = {
    1: HOME, 2: INSERT, 3: DELETE, 4: END, 5: PAGE_UP, 6: PAGE_DOWN,
    11: "f1", 12: "f2", 13: "f3", 14: "f4", 15: "f5",
    17: "f6", 18: "f7", 19: "f8", 20: "f9", 21: "f10", 23: "f11", 24: "f12",
}

_SS3 = {"P": "f1", "Q": "f2", "R": "f3", "S": "f4",
        "A": UP, "B": DOWN, "C": RIGHT, "D": LEFT, "H": HOME, "F": END}


def _modifiers(code: int) -> Tuple[bool, bool, bool]:
    """xterm's modifier encoding: 1 + shift(1) + alt(2) + ctrl(4)."""
    bits = max(0, code - 1)
    return bool(bits & 4), bool(bits & 2), bool(bits & 1)  # ctrl, alt, shift


def parse_key(buffer: str, expect_more: bool = False):
    """Consume one event from the front of ``buffer``.

    Returns ``(event, remainder)``. ``event`` is a :class:`Key`, a :class:`Mouse`, or
    ``None`` when the buffer holds only the start of a sequence and more bytes may
    arrive. ``expect_more=False`` says the read timed out, which is what resolves the
    ambiguity between a bare ``Esc`` and the beginning of an arrow key: with nothing
    following it after a timeout, it was the key.
    """
    if not buffer:
        return None, buffer

    first = buffer[0]

    if first != "\x1b":
        return _parse_plain(first), buffer[1:]

    if len(buffer) == 1:
        return (None, buffer) if expect_more else (Key(ESCAPE), buffer[1:])

    second = buffer[1]

    # ESC O <final> — the "application keypad" form of the arrows and F1-F4.
    if second == "O":
        if len(buffer) < 3:
            return (None, buffer) if expect_more else (Key(ESCAPE), buffer[1:])
        name = _SS3.get(buffer[2])
        return (Key(name) if name else Key(ESCAPE)), buffer[3:]

    if second == "[":
        return _parse_csi(buffer, expect_more)

    # ESC <char> is Alt-<char>. Alt-Esc-something is not a thing worth modelling.
    return Key(second, char=second, alt=True), buffer[2:]


def _parse_plain(char: str) -> Key:
    if char in ("\r", "\n"):
        return Key(ENTER)
    if char == "\t":
        return Key(TAB)
    if char in ("\x7f", "\x08"):
        return Key(BACKSPACE)
    code = ord(char)
    if code < 0x20:
        # Ctrl-A..Ctrl-Z land at 1..26. Ctrl-I/J/M are tab/newline/enter and were
        # already claimed above, which is correct: the terminal cannot tell them apart
        # and neither should we.
        letter = chr(code + 0x60)
        return Key(letter, ctrl=True)
    return Key(char, char=char)


def _parse_csi(buffer: str, expect_more: bool):
    """Parse ``ESC [ <private> <params> <intermediates> <final>``.

    The grammar is followed rather than approximated because the failure is silent and
    nasty: a sequence the app did not ask for — ``ESC [ ? 1004 h``, a focus-reporting
    acknowledgement — must be consumed *whole*. Recognising only the shapes we want and
    stopping at the first unexpected byte leaves ``1004h`` in the buffer, and the next
    parse turns it into five fake keystrokes that move the selection under the user.
    """
    index = 2
    params: List[str] = []
    current = ""
    private = ""

    # Private markers: `<` (SGR mouse), `?` (DEC private), `=` and `>`.
    if index < len(buffer) and buffer[index] in "<=>?":
        private = buffer[index]
        index += 1

    while index < len(buffer) and (buffer[index].isdigit() or buffer[index] in ";:"):
        char = buffer[index]
        if char == ";":
            params.append(current)
            current = ""
        elif char == ":":
            # Sub-parameters (used by some colour and key protocols) are not modelled;
            # they belong to the parameter being read and are dropped, not split on.
            pass
        else:
            current += char
        index += 1

    # Intermediate bytes, then the final byte in @-~ that ends the sequence.
    while index < len(buffer) and " " <= buffer[index] <= "/":
        index += 1
    if index >= len(buffer):
        return (None, buffer) if expect_more else (Key(ESCAPE), buffer[1:])

    params.append(current)
    final = buffer[index]
    rest = buffer[index + 1:]

    # A DEC private sequence is never a key press — it is the terminal replying to a mode
    # set. Consumed and discarded.
    if private in ("?", "=", ">"):
        return None, rest
    numbers = [int(p) if p.isdigit() else 0 for p in params]

    if private == "<" and final in ("M", "m"):
        return _parse_mouse(numbers, final == "M"), rest

    if final == "~":
        name = _TILDE.get(numbers[0] if numbers else 0)
        if name is None:
            return Key(ESCAPE), rest
        ctrl, alt, shift = _modifiers(numbers[1]) if len(numbers) > 1 else (False, False, False)
        return Key(name, ctrl=ctrl, alt=alt, shift=shift), rest

    name = _CSI_FINAL.get(final)
    if name is None:
        # An unrecognised CSI is dropped rather than surfaced. Terminals emit sequences
        # we did not ask for (focus events, cursor reports); turning those into fake
        # keystrokes would move the selection under the user's hands.
        return None, rest
    ctrl, alt, shift = _modifiers(numbers[1]) if len(numbers) > 1 else (False, False, False)
    return Key(name, ctrl=ctrl, alt=alt, shift=shift), rest


def _parse_mouse(numbers: List[int], pressed: bool) -> Mouse:
    button = numbers[0] if numbers else 0
    column = (numbers[1] if len(numbers) > 1 else 1) - 1
    row = (numbers[2] if len(numbers) > 2 else 1) - 1
    wheel = 0
    if button in (64, 65):
        wheel = -1 if button == 64 else 1
    return Mouse(row=row, column=column, button=button & 0x03, pressed=pressed, wheel=wheel)


# ── Windows key translation ──────────────────────────────────────────────────

#: ``msvcrt`` reports special keys as a two-byte pair whose first byte is \x00 or \xe0.
_WIN_SPECIAL = {
    "H": UP, "P": DOWN, "K": LEFT, "M": RIGHT,
    "G": HOME, "O": END, "I": PAGE_UP, "Q": PAGE_DOWN,
    "R": INSERT, "S": DELETE,
    ";": "f1", "<": "f2", "=": "f3", ">": "f4", "?": "f5",
    "@": "f6", "A": "f7", "B": "f8", "C": "f9", "D": "f10",
    "\x85": "f11", "\x86": "f12",
    # Ctrl-arrows.
    "\x8d": UP, "\x91": DOWN, "s": LEFT, "t": RIGHT,
}
_WIN_CTRL_ARROWS = {"\x8d", "\x91", "s", "t"}

#: Private-use lead byte marking "the next character is a Windows scan code, not text".
#: A real sentinel is needed because the discriminating byte is an ordinary letter — the
#: ``H`` of Up is the same ``H`` you get from typing one.
WIN_SENTINEL = ""


def _translate_windows(code: str) -> Optional[Key]:
    name = _WIN_SPECIAL.get(code)
    if name is None:
        return None
    return Key(name, ctrl=code in _WIN_CTRL_ARROWS)


# ── raw mode ─────────────────────────────────────────────────────────────────


@contextlib.contextmanager
def raw_mode(stream=None) -> Iterator[bool]:
    """Put the terminal in cbreak for the duration. Yields whether it worked.

    Yielding a bool rather than raising is deliberate. Input redirected from a file has
    no terminal to configure, and the caller — a dashboard being screenshotted, or the
    shell under a pipe — should degrade to line input rather than refuse to start.

    The restore is in a ``finally`` because leaving a terminal in cbreak after a crash
    means the user's shell stops echoing and they have to type ``reset`` blind.
    """
    stream = stream or sys.stdin
    if os.name == "nt":
        yield _stream_is_tty(stream)
        return
    try:
        import termios
        import tty
    except ImportError:  # pragma: no cover - not POSIX
        yield False
        return
    try:
        fd = stream.fileno()
        saved = termios.tcgetattr(fd)
    except Exception:
        yield False
        return
    try:
        tty.setcbreak(fd)
        yield True
    finally:
        with contextlib.suppress(Exception):
            termios.tcsetattr(fd, termios.TCSADRAIN, saved)


def _stream_is_tty(stream) -> bool:
    try:
        return bool(stream.isatty())
    except (AttributeError, ValueError):
        return False


class InputReader:
    """Reads events, blocking at most ``timeout`` seconds.

    The timeout is what lets an app loop stay responsive without a second thread: the
    dashboard asks for a key with a 0.1s budget, gets ``None`` most of the time, and uses
    those returns to poll its workers and repaint.
    """

    def __init__(self, stream=None) -> None:
        self.stream = stream or sys.stdin
        self.buffer = ""
        self.is_tty = _stream_is_tty(self.stream)

    # -- platform reads ---------------------------------------------------

    def _read_posix(self, timeout: float) -> str:
        import select

        try:
            ready, _, _ = select.select([self.stream], [], [], timeout)
        except (OSError, ValueError):  # pragma: no cover - stream closed mid-run
            return ""
        if not ready:
            return ""
        try:
            return os.read(self.stream.fileno(), 1024).decode("utf-8", "replace")
        except (OSError, ValueError):  # pragma: no cover
            return ""

    def _read_windows(self, timeout: float) -> str:  # pragma: no cover - Windows only
        try:
            import msvcrt
        except ImportError:
            return ""
        deadline = time.monotonic() + timeout
        out = ""
        while True:
            while msvcrt.kbhit():
                char = msvcrt.getwch()
                if char in ("\x00", "\xe0"):
                    # A special key: the next call yields the discriminating byte. It is
                    # marked with a private-use lead so the parser cannot mistake it for
                    # typed text.
                    follow = msvcrt.getwch() if msvcrt.kbhit() else ""
                    out += WIN_SENTINEL + follow
                else:
                    out += char
            if out or time.monotonic() >= deadline:
                return out
            time.sleep(0.005)

    def _fill(self, timeout: float) -> None:
        chunk = (
            self._read_windows(timeout) if os.name == "nt" else self._read_posix(timeout)
        )
        if chunk:
            self.buffer += chunk

    # -- public -----------------------------------------------------------

    def read(self, timeout: float = 0.1):
        """One event, or ``None`` if nothing arrived within ``timeout``."""
        if not self.buffer:
            self._fill(timeout)
        if not self.buffer:
            return None

        # Windows special keys, injected by `_read_windows`.
        if self.buffer[0] == WIN_SENTINEL:
            if len(self.buffer) < 2:
                self.buffer = ""
                return None
            code, self.buffer = self.buffer[1], self.buffer[2:]
            return _translate_windows(code) or None

        # A lone ESC may be a key or the head of a sequence. Give the rest one short
        # grace period to arrive before committing to "the user pressed Escape".
        if self.buffer == "\x1b":
            self._fill(0.02)

        event, remainder = parse_key(self.buffer, expect_more=False)
        self.buffer = remainder
        return event

    def drain(self, timeout: float = 0.0) -> List:
        """Every event currently available. Used to collapse held-down arrow repeats."""
        events = []
        event = self.read(timeout)
        while event is not None:
            events.append(event)
            if not self.buffer:
                break
            event = self.read(0.0)
        return events
