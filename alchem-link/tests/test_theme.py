"""The palette is a product decision, so it gets a test.

Two things are worth pinning. The first is that the theme stays black-and-blue rather
than drifting to navy — a reviewer changing one constant should have to change this
file too, which is the point. The second is that the render code keeps sourcing colours
from :mod:`alchem_link.theme`; the failure mode this catches is a new panel pasting a
literal ``[#00d4ff]`` into an f-string, which no amount of editing ``theme.py`` would
then recolour.
"""
from __future__ import annotations

import re
import unittest
from pathlib import Path

from alchem_link import theme

TUI_SOURCE = Path(theme.__file__).with_name("tui.py")

HEX = re.compile(r"#[0-9a-fA-F]{6}\b")


def _rgb(value: str) -> tuple[int, int, int]:
    raw = value.lstrip("#")
    return int(raw[0:2], 16), int(raw[2:4], 16), int(raw[4:6], 16)


class ThemeIsBlackAndBlue(unittest.TestCase):
    def test_surfaces_read_as_black(self) -> None:
        """Backgrounds are black, not a dark blue — every channel stays very low."""
        for name in ("BLACK", "SURFACE", "SURFACE_HI"):
            with self.subTest(name):
                r, g, b = _rgb(getattr(theme, name))
                self.assertLessEqual(max(r, g, b), 40, f"{name} is too bright for a surface")
                # A faint blue lift is intended; a navy background is not.
                self.assertLessEqual(b - r, 30, f"{name} has drifted toward navy")

    def test_accent_is_a_mid_blue(self) -> None:
        """Bright enough to read on black, and unambiguously blue rather than cyan."""
        r, g, b = _rgb(theme.BLUE)
        self.assertGreaterEqual(b, 180, "the accent is too dark to carry signal on black")
        self.assertGreater(b, r + 60, "the accent is not clearly blue")
        # Cyan is green-dominant in its mid channel; a blue keeps green well under blue.
        self.assertGreater(b - g, 40, "the accent has drifted into cyan")

    def test_status_colours_are_distinct(self) -> None:
        values = set(theme.STATUS_COLOUR.values())
        self.assertEqual(len(values), 3, "two statuses share a colour")
        self.assertEqual(set(theme.STATUS_COLOUR), {"FRESH", "STALE", "INVALID"})
        self.assertEqual(set(theme.STATUS_CLASS), set(theme.STATUS_COLOUR))


class ThemeIsTheOnlySourceOfColour(unittest.TestCase):
    def test_tui_holds_no_hardcoded_hex(self) -> None:
        """Colours in tui.py must come from theme.py, or a retheme silently misses them."""
        stray = HEX.findall(TUI_SOURCE.read_text(encoding="utf-8"))
        self.assertEqual(
            stray, [], f"tui.py hardcodes colours instead of importing them: {stray}"
        )

    def test_css_is_fully_interpolated(self) -> None:
        """No unresolved placeholder left in the generated stylesheet.

        The f-string resolves at import, so the realistic mistake is a doubled
        ``{{BLUE}}`` that survives as the literal text ``{BLUE}`` — which Textual
        would then reject as a malformed rule at runtime, not at import.
        """
        leftover = re.findall(r"\{[A-Z_]+\}", theme.CSS)
        self.assertEqual(leftover, [], f"CSS kept an uninterpolated placeholder: {leftover}")
        self.assertIn(theme.BLACK, theme.CSS)
        self.assertIn(theme.BLUE_HI, theme.CSS)


class MarkupHelpers(unittest.TestCase):
    def test_helpers_emit_closed_rich_spans(self) -> None:
        for rendered in (theme.key("K"), theme.value("V"), theme.title("T"), theme.hint("H")):
            with self.subTest(rendered):
                self.assertTrue(rendered.endswith("[/]"))
                self.assertEqual(rendered.count("[/]"), 1)

    def test_status_helper_colours_each_label(self) -> None:
        self.assertIn(theme.GREEN, theme.status("FRESH"))
        self.assertIn(theme.AMBER, theme.status("STALE"))
        self.assertIn(theme.RED, theme.status("INVALID"))

    def test_status_helper_survives_an_unknown_label(self) -> None:
        """A new status must not raise inside a render pass — it falls back to text."""
        self.assertIn(theme.TEXT, theme.status("PENDING"))


if __name__ == "__main__":
    unittest.main()
