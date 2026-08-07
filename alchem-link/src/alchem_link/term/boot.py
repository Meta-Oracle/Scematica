"""Terminal initialisation — the black-and-blue that happens before anything renders.

This is the module the product's look actually comes from. Drawing black rectangles gets
you a black *pane*; the columns past the last painted cell, the scrollback above the
prompt, and anything a subprocess writes all stay whatever colour the user's terminal
was. :func:`initialize` instead repaints the terminal's own defaults via OSC 10/11/12,
so the surface under everything — including plain ``alchem-link price`` output and
including a PyInstaller binary double-clicked into a fresh console — is the palette.

It runs from three entry points and must be safe in all of them: the CLI's ``main``, the
interactive shell, and the frozen binary. Hence :data:`_STATE` and the idempotence — the
shell dispatches into the CLI, which would otherwise re-theme mid-session.

The other half of this module's job is putting it back. A tool that repaints someone's
terminal and dies without undoing it has broken their terminal, not themed it, so the
restore is wired to :mod:`atexit` *and* to the signal handlers, and it survives being
called twice.
"""
from __future__ import annotations

import atexit
import os
import signal
import sys
from dataclasses import dataclass
from typing import Optional, TextIO

from ..theme import BLACK, BLUE, TEXT
from . import ansi


@dataclass
class BootState:
    """What initialisation actually managed to do, so callers can degrade honestly."""

    themed: bool = False
    vt: bool = False
    depth: int = ansi.Depth.NONE
    alt_screen: bool = False
    frozen: bool = False
    title: str = ""

    def as_dict(self) -> dict:
        return {
            "themed": self.themed,
            "vt_enabled": self.vt,
            "colour_depth": ansi.Depth.name(self.depth),
            "alt_screen": self.alt_screen,
            "frozen": self.frozen,
        }


_STATE = BootState()


def state() -> BootState:
    """The current boot state. ``alchem-link doctor`` reports this."""
    return _STATE


def is_frozen() -> bool:
    """True inside a PyInstaller (or similar) one-file binary.

    Worth distinguishing because a frozen binary is usually launched by double-click into
    a brand-new console with default colours and no ``TERM``, which is exactly the case
    where theming the terminal matters most and where the environment gives the fewest
    hints that colour is supported.
    """
    return bool(getattr(sys, "frozen", False)) or hasattr(sys, "_MEIPASS")


def configure_streams() -> None:
    """Make stdout/stderr survive non-ASCII on a legacy Windows codepage.

    This package prints box-drawing glyphs and em-dashes; a console defaulting to cp1252
    raises ``UnicodeEncodeError`` on them mid-line, turning a diagnostic into a crash.
    Given the toolkit's whole purpose is telling you when something is wrong, it must not
    fall over while doing so.
    """
    for stream in (sys.stdout, sys.stderr):
        reconfigure = getattr(stream, "reconfigure", None)
        if reconfigure is None:
            continue
        try:
            reconfigure(encoding="utf-8", errors="replace")
        except (ValueError, OSError):  # pragma: no cover - already closed, or piped oddly
            pass


def initialize(title: str = "", theme_terminal: Optional[bool] = None,
               stream: Optional[TextIO] = None) -> BootState:
    """Prepare the terminal. Idempotent; safe to call from every entry point.

    ``theme_terminal=None`` means "decide": repaint when writing to a real terminal that
    accepts colour, and do nothing when the output is a pipe, a file, or ``NO_COLOR``.
    Passing ``True`` forces it, which is what the frozen binary does — a fresh console
    reports no ``TERM`` at all, and refusing to theme it would make the binary the one
    place the product does not look like itself.
    """
    stream = stream or sys.stdout
    _STATE.frozen = is_frozen()

    configure_streams()
    _STATE.vt = ansi.enable_vt()
    _STATE.depth = ansi.detect_depth(stream)

    if theme_terminal is None:
        theme_terminal = _STATE.depth > ansi.Depth.NONE
    if not theme_terminal:
        return _STATE

    if not _STATE.themed:
        try:
            stream.write(ansi.theme_terminal(BLACK, TEXT))
            stream.write(ansi.set_cursor_colour(BLUE))
            if title:
                stream.write(ansi.title(title))
                _STATE.title = title
            stream.flush()
        except (OSError, ValueError):  # pragma: no cover - stream vanished under us
            return _STATE
        _STATE.themed = True
        atexit.register(restore)
    elif title and title != _STATE.title:
        try:
            stream.write(ansi.title(title))
            stream.flush()
            _STATE.title = title
        except (OSError, ValueError):  # pragma: no cover
            pass
    return _STATE


def restore(stream: Optional[TextIO] = None) -> None:
    """Hand the terminal back: default colours, visible cursor, main screen buffer.

    Deliberately tolerant. It runs from ``atexit`` during interpreter shutdown, from
    signal handlers, and from ``finally`` blocks that may be unwinding an exception, and
    a restore that raises would replace a clean error message with a traceback about the
    terminal.
    """
    if not (_STATE.themed or _STATE.alt_screen):
        return
    stream = stream or sys.stdout
    try:
        if _STATE.alt_screen:
            stream.write(ansi.DISABLE_MOUSE)
            stream.write(ansi.DISABLE_BRACKETED_PASTE)
            stream.write(ansi.EXIT_ALT_SCREEN)
            _STATE.alt_screen = False
        stream.write(ansi.SHOW_CURSOR)
        stream.write(ansi.CURSOR_DEFAULT)
        stream.write(ansi.RESET)
        stream.write(ansi.RESET_COLOURS)
        stream.flush()
    except Exception:  # pragma: no cover - shutdown is not the place to be strict
        pass
    _STATE.themed = False


def enter_fullscreen(stream: Optional[TextIO] = None, mouse: bool = True) -> None:
    """Switch to the alternate screen buffer for a full-screen app.

    The alternate buffer is what makes quitting the dashboard return the user to their
    scrollback intact rather than to a screenful of dead frames.
    """
    stream = stream or sys.stdout
    try:
        stream.write(ansi.ENTER_ALT_SCREEN)
        stream.write(ansi.HIDE_CURSOR)
        stream.write(ansi.CLEAR_SCREEN)
        stream.write(ansi.HOME)
        if mouse:
            stream.write(ansi.ENABLE_MOUSE)
        stream.write(ansi.ENABLE_BRACKETED_PASTE)
        stream.flush()
    except (OSError, ValueError):  # pragma: no cover
        return
    _STATE.alt_screen = True


def exit_fullscreen(stream: Optional[TextIO] = None) -> None:
    """Leave the alternate buffer but keep the palette — the shell still wants it."""
    if not _STATE.alt_screen:
        return
    stream = stream or sys.stdout
    try:
        stream.write(ansi.DISABLE_MOUSE)
        stream.write(ansi.DISABLE_BRACKETED_PASTE)
        stream.write(ansi.SHOW_CURSOR)
        stream.write(ansi.EXIT_ALT_SCREEN)
        stream.flush()
    except Exception:  # pragma: no cover
        pass
    _STATE.alt_screen = False


def install_signal_handlers(on_signal=None) -> None:
    """Restore the terminal on SIGINT/SIGTERM/SIGHUP before doing anything else.

    Without this, Ctrl-C out of the dashboard leaves the alternate buffer active and the
    cursor hidden, and the user's next command runs into an invisible prompt.
    """
    def handler(signum, frame):  # pragma: no cover - signal delivery
        restore()
        if on_signal is not None:
            on_signal(signum)
        else:
            raise KeyboardInterrupt

    for name in ("SIGINT", "SIGTERM", "SIGHUP"):
        sig = getattr(signal, name, None)
        if sig is None:
            continue
        try:
            signal.signal(sig, handler)
        except (ValueError, OSError):  # pragma: no cover - not the main thread
            pass


def banner_title(version: str) -> str:
    """The window title. Frozen builds say so, because a bug report should."""
    suffix = " [binary]" if is_frozen() else ""
    return f"Alchem-Link v{version}{suffix}"


def describe() -> dict:
    """Everything the boot layer knows, for ``doctor`` and the dashboard's about pane."""
    return {
        **_STATE.as_dict(),
        "platform": sys.platform,
        "is_tty": bool(getattr(sys.stdout, "isatty", lambda: False)()),
        "term": os.environ.get("TERM", ""),
        "colorterm": os.environ.get("COLORTERM", ""),
        "no_color": bool(os.environ.get("NO_COLOR")),
    }
