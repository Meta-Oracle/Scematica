"""The palette is a product decision, so it gets a test.

Two things are worth pinning. The first is that the theme stays black-and-blue rather
than drifting to navy — a reviewer changing one constant should have to change this file
too, which is the point. The second is that the render code keeps sourcing colours from
:mod:`alchem_link.theme`; the failure mode this catches is a new panel pasting a literal
``#00d4ff`` into an f-string, which no amount of editing ``theme.py`` would then
recolour.

That second check got broader in 0.23.0. There used to be one render module to scan;
there are now five, plus a line-oriented console, and the invariant matters more rather
than less — the palette is what makes the CLI, the dashboard and the boot sequence look
like one product.
"""
from __future__ import annotations

import re
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "src"))

from alchem_link import theme
from alchem_link.term import ansi

HEX = re.compile(r"#[0-9a-fA-F]{6}\b")

#: Every module that paints. None of them may name a colour; all of them ask for a role.
RENDER_MODULES = [
    "render.py",
    "dashboard.py",
    "shell.py",
    "term/widgets.py",
    "term/screen.py",
    "term/app.py",
]

SOURCE_ROOT = Path(theme.__file__).parent


def _rgb(value: str):
    return theme.rgb(value)


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

    def test_surfaces_step_upward(self) -> None:
        """Background < surface < hover, or cards stop being visible against the page."""
        levels = [sum(_rgb(getattr(theme, n))) for n in ("BLACK", "SURFACE", "SURFACE_HI")]
        self.assertEqual(levels, sorted(levels))
        self.assertNotEqual(levels[0], levels[1], "SURFACE is indistinguishable from BLACK")

    def test_status_colours_are_distinct(self) -> None:
        values = set(theme.STATUS_COLOUR.values())
        self.assertEqual(len(values), 3, "two statuses share a colour")
        self.assertEqual(set(theme.STATUS_COLOUR), {"FRESH", "STALE", "INVALID"})
        self.assertEqual(set(theme.STATUS_CLASS), set(theme.STATUS_COLOUR))

    def test_palette_lists_every_named_colour(self) -> None:
        """PALETTE is what the web mirror and `alchem-link theme` read. It must be whole."""
        named = {
            name for name in dir(theme)
            if name.isupper() and isinstance(getattr(theme, name), str)
            and HEX.fullmatch(getattr(theme, name) or "")
        }
        self.assertEqual({n.lower() for n in named}, set(theme.PALETTE))

    def test_every_palette_entry_parses(self) -> None:
        for name, value in theme.PALETTE.items():
            with self.subTest(name):
                self.assertEqual(len(theme.rgb(value)), 3)


#: Roles that deliberately invert — accent background, dark text. A cursor cell is a blue
#: block by design, so it is exempt from the "backgrounds are surfaces" rule.
INVERTED_ROLES = {"cursor"}


class Roles(unittest.TestCase):
    def test_every_role_paints_on_a_defined_surface(self) -> None:
        """A role with no background inherits whatever was there — usually the wrong thing."""
        surfaces = {theme.BLACK, theme.SURFACE, theme.SURFACE_HI}
        palette = set(theme.PALETTE.values())
        for name, style in theme.ROLES.items():
            with self.subTest(name):
                self.assertIsNotNone(style.fg, f"role {name} has no foreground")
                self.assertIn(style.bg, palette, f"role {name} uses an unnamed background")
                if name not in INVERTED_ROLES:
                    self.assertIn(style.bg, surfaces,
                                  f"role {name} paints on an unknown surface")

    def test_inverted_roles_stay_legible(self) -> None:
        """An inverted role must put a dark foreground on its bright background."""
        for name in INVERTED_ROLES:
            style = theme.role(name)
            with self.subTest(name):
                self.assertLess(sum(theme.rgb(style.fg)), sum(theme.rgb(style.bg)),
                                f"role {name} is bright-on-bright")

    def test_unknown_role_falls_back_rather_than_raising(self) -> None:
        """A render pass must never die mid-frame on a renamed role."""
        self.assertEqual(theme.role("no-such-role"), theme.BASE)

    def test_status_and_severity_helpers_cover_their_tables(self) -> None:
        for label, colour in theme.STATUS_COLOUR.items():
            self.assertEqual(theme.status_style(label).fg, colour)
        for severity, colour in theme.SEVERITY_COLOUR.items():
            self.assertEqual(theme.severity_style(severity).fg, colour)
        # Unknown labels fall back rather than raising, for the same reason as roles.
        self.assertEqual(theme.status_style("PENDING").fg, theme.TEXT)
        self.assertEqual(theme.severity_style("novel").fg, theme.MUTED)

    def test_style_with_copies_and_overrides(self) -> None:
        base = theme.role("key")
        shifted = base.with_(bg=theme.SURFACE_HI)
        self.assertEqual(shifted.fg, base.fg)
        self.assertEqual(shifted.bg, theme.SURFACE_HI)
        self.assertEqual(base.bg, theme.BLACK, "with_ mutated the original")

    def test_styles_are_hashable(self) -> None:
        """`ansi.sgr` memoises on (style, depth); an unhashable style would break it."""
        self.assertEqual(len({theme.role("key"), theme.role("key")}), 1)


class ThemeIsTheOnlySourceOfColour(unittest.TestCase):
    def test_render_modules_hold_no_hardcoded_hex(self) -> None:
        """Colours must come from theme.py, or a retheme silently misses them."""
        for relative in RENDER_MODULES:
            path = SOURCE_ROOT / relative
            with self.subTest(module=relative):
                self.assertTrue(path.exists(), f"{relative} is missing — update this list")
                stray = HEX.findall(path.read_text(encoding="utf-8"))
                self.assertEqual(
                    stray, [], f"{relative} hardcodes colours instead of importing them: {stray}"
                )

    def test_the_theme_module_itself_does_no_encoding(self) -> None:
        """theme.py is inert: a palette, not an emitter.

        The separation is what lets one palette drive truecolor, 256-colour, 16-colour and
        NO_COLOR output. An escape sequence appearing here would mean some path is bypassing
        the depth negotiation entirely.
        """
        source = Path(theme.__file__).read_text(encoding="utf-8")
        self.assertNotIn("\\x1b", source)
        self.assertNotIn("\x1b", source)


class PaletteSurvivesEveryDepth(unittest.TestCase):
    """The palette has to still mean something once it is quantised."""

    def test_accent_and_background_stay_distinguishable_at_256(self) -> None:
        self.assertNotEqual(ansi.to_256(theme.BLUE), ansi.to_256(theme.BLACK))
        self.assertNotEqual(ansi.to_256(theme.BLUE_HI), ansi.to_256(theme.BLACK))

    def test_surfaces_do_not_all_collapse_to_one_index_at_256(self) -> None:
        """SURFACE must stay a step off BLACK, or panels vanish on a 256-colour terminal.

        This is the check that catches a grey-ramp regression in `to_256`: without the
        ramp, every near-black surface quantises to cube index 16 and the whole UI becomes
        one flat rectangle.
        """
        self.assertNotEqual(ansi.to_256(theme.SURFACE), ansi.to_256(theme.BLACK))

    def test_status_colours_stay_distinct_at_256(self) -> None:
        indexes = {ansi.to_256(c) for c in theme.STATUS_COLOUR.values()}
        self.assertEqual(len(indexes), 3, "two statuses collapse to one 256-colour index")

    def test_status_colours_stay_distinct_at_16(self) -> None:
        """Green, amber and red must remain three things even on a bare console."""
        indexes = {ansi.to_16(c) for c in theme.STATUS_COLOUR.values()}
        self.assertEqual(len(indexes), 3, "two statuses collapse to one 16-colour index")

    def test_no_colour_depth_emits_nothing(self) -> None:
        for style in theme.ROLES.values():
            self.assertEqual(ansi.sgr(style, ansi.Depth.NONE), "")


if __name__ == "__main__":
    unittest.main()
