"""TUI renderers, exercised without starting the app or touching a chain.

Every live panel has three states — loading, error, and empty — and each is a separate
opportunity to crash on a `None` the happy path never produces. A TUI failure is
particularly bad here because it takes the whole screen with it, so all three are
rendered for every panel.

The app itself is never run; only the pure render functions are called.
"""
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "src"))

try:
    import textual  # noqa: F401
    HAS_TEXTUAL = True
except ImportError:  # pragma: no cover - the library half must work without it
    HAS_TEXTUAL = False


@unittest.skipUnless(HAS_TEXTUAL, "textual is only needed for the TUI")
class RendererTests(unittest.TestCase):
    def setUp(self):
        from alchem_link import tui

        self.tui = tui
        self.empty = {
            "live": [],
            "audit": [],
            "divergence": [],
            "sequencer": [],
            "ccip": [],
            "gas": None,
        }

    def test_every_nav_item_has_a_renderer(self):
        """A nav entry with no renderer is a KeyError the moment it is clicked."""
        for nav, _label in self.tui.NAV_ITEMS:
            with self.subTest(panel=nav):
                self.assertTrue(
                    nav in self.tui.LIVE_RENDERERS
                    or nav in self.tui.REFERENCE_RENDERERS
                    or nav == "recipes",
                    f"nav item {nav!r} has no renderer",
                )

    def test_every_live_panel_has_a_loading_note(self):
        for panel in self.tui.LIVE_PANELS:
            self.assertIn(panel, self.tui.LOADING_NOTES)

    def test_live_panels_render_in_every_state(self):
        for panel in self.tui.LIVE_PANELS:
            renderer = self.tui.LIVE_RENDERERS[panel]
            with self.subTest(panel=panel):
                icon, label, note = self.tui.LOADING_NOTES[panel]
                self.assertTrue(self.tui._loading(icon, label, "ethereum", note))
                self.assertTrue(renderer("ethereum", None, "endpoint unreachable"))
                self.assertTrue(renderer("ethereum", self.empty[panel]))

    def test_reference_panels_render_offline(self):
        for name, renderer in self.tui.REFERENCE_RENDERERS.items():
            with self.subTest(panel=name):
                self.assertTrue(renderer())

    def test_recipes_list_and_detail(self):
        self.assertTrue(self.tui._render_recipes())
        self.assertTrue(self.tui._render_recipes("ccip-cross-chain-transfer"))

    def test_unknown_recipe_id_falls_back_to_the_list(self):
        self.assertTrue(self.tui._render_recipes("does-not-exist"))

    def test_gas_panel_renders_a_populated_report(self):
        from alchem_link.gas import FeeEstimate, GasReport

        report = GasReport(
            network="ethereum",
            native_symbol="ETH",
            base_fee_wei=10**9,
            next_base_fee_wei=11 * 10**8,
            blocks_sampled=20,
            gas_used_ratios=[0.5],
            tiers=[FeeEstimate("standard", 10**8, 11 * 10**8)],
            native_usd=1900.0,
        )
        self.assertTrue(self.tui._render_gas("ethereum", report))

    def test_gas_panel_renders_without_a_usd_price(self):
        """The native feed can be missing; the tier table must still print."""
        from alchem_link.gas import FeeEstimate, GasReport

        report = GasReport(
            network="scroll",
            native_symbol="ETH",
            base_fee_wei=10**9,
            next_base_fee_wei=10**9,
            blocks_sampled=5,
            tiers=[FeeEstimate("standard", 0, 10**9)],
            price_error="no native-token feed registered",
        )
        self.assertTrue(self.tui._render_gas("scroll", report))

    def test_formatters(self):
        self.assertEqual(self.tui._fmt_age(45), "45s")
        self.assertEqual(self.tui._fmt_age(3661), "1h 1m")
        self.assertEqual(self.tui._fmt_age(90061), "1d 1h")
        self.assertEqual(self.tui._fmt_secs(3600), "1h")
        self.assertEqual(self.tui._fmt_secs(86400), "1d")
        self.assertEqual(self.tui._fmt_secs(60), "1m")
        self.assertEqual(self.tui._fmt_price(1900.5), "1,900.50")


if __name__ == "__main__":
    unittest.main()
