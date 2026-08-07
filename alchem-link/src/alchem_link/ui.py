"""Entry point for ``alchem-link-ui``.

The TUI is the one part of this package that needs a third-party library, so ``textual``
is an optional extra rather than a hard dependency — ``pip install alchem-link`` pulls
nothing, which is the claim the rest of the package makes and should therefore be true
at install time too.

That leaves one rough edge worth smoothing: without the extra, a console script pointing
straight at :mod:`alchem_link.tui` would fail with a bare ``ModuleNotFoundError`` from
somewhere inside the import machinery. This shim turns that into the one line the user
actually needs.
"""
from __future__ import annotations

import sys


def launch() -> None:
    try:
        from .tui import launch as _launch
    except ImportError as exc:
        print(
            "alchem-link-ui needs the TUI extra:\n\n"
            "    pip install 'alchem-link[tui]'\n\n"
            "Every other command works without it — try `alchem-link --help`.",
            file=sys.stderr,
        )
        raise SystemExit(1) from exc
    _launch()


if __name__ == "__main__":
    launch()
