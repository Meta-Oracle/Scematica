"""The event loop: input, background work, and repaint, without a UI framework.

An :class:`App` subclass supplies :meth:`App.render` and, usually, :meth:`App.on_key`.
Everything else — raw mode, the alternate screen, resize detection, worker threads,
frame scheduling, and putting the terminal back — is handled here.

The loop is single-threaded and repaints only when something changed. Terminal UIs that
repaint on a fixed clock burn a core doing nothing and, over SSH, stream a frame's worth
of bytes every tick; this one blocks in :meth:`~.input.InputReader.read` with a short
budget, wakes on a keystroke or a worker result, and otherwise emits nothing at all.

**Network work never runs on this thread.** A public RPC round trip is hundreds of
milliseconds and a cross-chain sweep is several seconds; doing either inline freezes the
UI mid-frame, which users correctly read as a crash. :meth:`App.submit` runs the call on
a pool thread and delivers the result — or the exception — back to the loop, so a panel
can render "loading", then data, then an error, and never a hang.
"""
from __future__ import annotations

import sys
import time
import traceback
from concurrent.futures import Future, ThreadPoolExecutor
from dataclasses import dataclass
from typing import Any, Callable, Dict, Optional, Tuple

from ..theme import BASE
from . import ansi, boot
from .input import ESCAPE, InputReader, Key, Mouse, raw_mode
from .screen import Screen, terminal_size
from .widgets import Rect


@dataclass
class Job:
    """A background call and whatever came back from it.

    Both ``value`` and ``error`` are carried so a panel can distinguish "still running"
    (``done`` is False) from "finished with nothing" (``done`` with ``value=None``) from
    "failed" — three states that render differently and that a bare Optional collapses.
    """

    key: str
    future: Future
    started: float
    done: bool = False
    value: Any = None
    error: Optional[str] = None

    @property
    def elapsed(self) -> float:
        return time.monotonic() - self.started


class App:
    """Base class for a full-screen terminal application."""

    #: Shown in the window title and the header.
    title = "alchem-link"
    #: Seconds the input read waits before returning control. Also the maximum delay
    #: before a landed worker result becomes visible.
    tick = 0.1
    #: Workers. Four is enough to fan a divergence sweep across chains without opening
    #: enough sockets that a public endpoint starts rate-limiting the run.
    workers = 4
    #: Whether to report mouse events. Off by default: enabling mouse reporting takes
    #: text selection away from the user in most terminals, which is a bad trade for a
    #: tool people copy addresses out of.
    mouse = False

    def __init__(self, stream=None, stdin=None, depth: Optional[int] = None) -> None:
        self.stream = stream or sys.stdout
        self.reader = InputReader(stdin)
        self.depth = depth if depth is not None else ansi.detect_depth(self.stream)
        columns, rows = terminal_size()
        self.screen = Screen(columns, rows, depth=self.depth, base=BASE)
        self.running = False
        self.dirty = True
        self._size = (columns, rows)
        self._pool: Optional[ThreadPoolExecutor] = None
        self._jobs: Dict[str, Job] = {}
        self._notice: Tuple[str, float] = ("", 0.0)
        self._frames = 0
        self._last_frame_bytes = 0

    # ── geometry ─────────────────────────────────────────────────────────────

    @property
    def rect(self) -> Rect:
        """The whole screen, as a rect — the root of every layout."""
        return Rect(0, 0, self.screen.width, self.screen.height)

    def _check_resize(self) -> None:
        size = terminal_size()
        if size != self._size:
            self._size = size
            self.screen.resize(*size)
            self.dirty = True
            self.on_resize(*size)

    # ── background work ──────────────────────────────────────────────────────

    def submit(self, key: str, fn: Callable[[], Any], *, replace: bool = False) -> Job:
        """Run ``fn`` on a worker thread, filed under ``key``.

        A key already in flight is not restarted unless ``replace`` is set, which is what
        stops a held-down refresh key from opening one RPC connection per keystroke.
        """
        existing = self._jobs.get(key)
        if existing is not None and not existing.done and not replace:
            return existing
        if self._pool is None:
            self._pool = ThreadPoolExecutor(max_workers=self.workers,
                                            thread_name_prefix="alchem-link")
        job = Job(key=key, future=self._pool.submit(fn), started=time.monotonic())
        self._jobs[key] = job
        self.dirty = True
        return job

    def job(self, key: str) -> Optional[Job]:
        return self._jobs.get(key)

    def forget(self, key: str) -> None:
        """Drop a cached result so the next :meth:`submit` re-runs it."""
        self._jobs.pop(key, None)

    def _harvest(self) -> None:
        """Move finished futures into their jobs. Called once per loop iteration."""
        for job in self._jobs.values():
            if job.done or not job.future.done():
                continue
            job.done = True
            try:
                job.value = job.future.result()
            except Exception as exc:  # a failed panel must not take the app with it
                job.error = str(exc) or exc.__class__.__name__
                job.value = None
            self.dirty = True
            self.on_job(job)

    # ── notices ──────────────────────────────────────────────────────────────

    def notify(self, text: str, seconds: float = 3.0) -> None:
        """A transient message in the status bar."""
        self._notice = (text, time.monotonic() + seconds)
        self.dirty = True

    @property
    def notice(self) -> str:
        text, expiry = self._notice
        if text and time.monotonic() > expiry:
            self._notice = ("", 0.0)
            return ""
        return text

    # ── hooks ────────────────────────────────────────────────────────────────

    def render(self, screen: Screen) -> None:
        """Paint one frame. Subclasses must implement this."""
        raise NotImplementedError

    def on_key(self, key: Key) -> bool:
        """Handle a key. Return True when it was consumed.

        Unconsumed keys fall through to :meth:`_default_key`, which owns quit and
        refresh so every screen gets them without repeating the bindings.
        """
        return False

    def on_mouse(self, event: Mouse) -> bool:
        return False

    def on_tick(self) -> None:
        """Called every loop iteration, whether or not anything happened."""

    def on_resize(self, columns: int, rows: int) -> None:
        """Called after the screen has already been resized."""

    def on_job(self, job: Job) -> None:
        """Called on the loop thread when a background job finishes."""

    def on_start(self) -> None:
        """Called once, after the terminal is set up and before the first frame."""

    def on_stop(self) -> None:
        """Called once, before the terminal is handed back."""

    # ── loop ─────────────────────────────────────────────────────────────────

    def quit(self) -> None:
        self.running = False

    def invalidate(self) -> None:
        """Force a full repaint — after anything wrote to the terminal behind our back."""
        self.screen.invalidate()
        self.dirty = True

    def _default_key(self, key: Key) -> bool:
        if key.name in ("q", ESCAPE) or (key.ctrl and key.name == "c"):
            self.quit()
            return True
        if key.ctrl and key.name == "l":
            self.invalidate()
            return True
        return False

    def _frame(self) -> None:
        self.screen.clear()
        try:
            self.render(self.screen)
        except Exception:
            # A render that raises would otherwise leave the alternate buffer holding a
            # half-drawn frame with no way to see why. Paint the traceback instead.
            self._render_crash(traceback.format_exc())
        self._last_frame_bytes = self.screen.flush(self.stream)
        self._frames += 1
        self.dirty = False

    def _render_crash(self, text: str) -> None:
        from ..theme import role

        self.screen.clear()
        self.screen.put(0, 0, "render error — press q to quit", role("bad"))
        for offset, line in enumerate(text.splitlines()[: self.screen.height - 2]):
            self.screen.put(offset + 2, 0, line[: self.screen.width], role("muted"))

    def run(self) -> int:
        """Take over the terminal, loop until :meth:`quit`, then hand it back."""
        boot.initialize(title=self.title, stream=self.stream)
        boot.enter_fullscreen(self.stream, mouse=self.mouse)
        self.running = True
        try:
            with raw_mode(self.reader.stream):
                self.on_start()
                while self.running:
                    self._check_resize()
                    self._harvest()
                    self.on_tick()
                    if self.dirty:
                        self._frame()
                    event = self.reader.read(self.tick)
                    if event is None:
                        continue
                    if isinstance(event, Mouse):
                        if self.on_mouse(event):
                            self.dirty = True
                        continue
                    if self.on_key(event) or self._default_key(event):
                        self.dirty = True
        except KeyboardInterrupt:
            pass
        finally:
            self.running = False
            self.on_stop()
            if self._pool is not None:
                # Do not wait: a worker blocked on a 15-second RPC timeout must not hold
                # the user's terminal hostage after they pressed q.
                self._pool.shutdown(wait=False)
            boot.exit_fullscreen(self.stream)
            boot.restore(self.stream)
        return 0

    # ── diagnostics ──────────────────────────────────────────────────────────

    def stats(self) -> dict:
        """Frame and job counters, for the dashboard's debug overlay."""
        return {
            "frames": self._frames,
            "last_frame_bytes": self._last_frame_bytes,
            "size": f"{self.screen.width}x{self.screen.height}",
            "depth": ansi.Depth.name(self.depth),
            "jobs": len(self._jobs),
            "in_flight": sum(1 for j in self._jobs.values() if not j.done),
        }
