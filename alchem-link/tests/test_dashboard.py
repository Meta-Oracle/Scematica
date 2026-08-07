"""Dashboard panels, exercised without starting the app or touching a chain.

Every live panel has three states — loading, error, and empty — and each is a separate
opportunity to crash on a ``None`` the happy path never produces. A dashboard failure is
particularly bad because it takes the whole screen with it, so all three are rendered for
every panel.

This is the payoff for panels rendering to a list of ``(text, style)`` lines rather than
straight to the screen: the renderers are pure functions and the assertions are about
what a person would read.
"""
from __future__ import annotations

import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "src"))

from alchem_link import dashboard
from alchem_link.gas import FeeEstimate, GasReport
from alchem_link.term import ansi
from alchem_link.term.input import Key
from alchem_link.term.screen import Screen
from alchem_link.theme import BASE

TRUE = ansi.Depth.TRUECOLOR

#: The shape each live panel's fetch returns when the network has nothing to give.
EMPTY = {
    "feeds": [],
    "audit": [],
    "analytics": [],
    "divergence": [],
    "sequencer": [],
    "ccip": [],
    "gas": None,
}


def text_of(lines) -> str:
    """Flatten rendered lines back to plain text, the way the screen would show them."""
    return "\n".join("".join(segment for segment, _ in line) for line in lines)


class PanelWiring(unittest.TestCase):
    def test_every_panel_has_a_renderer(self) -> None:
        for panel in dashboard.PANELS:
            with self.subTest(panel.key):
                self.assertTrue(callable(panel.render))

    def test_every_live_panel_has_a_loading_note(self) -> None:
        """Without one the panel shows an empty box while an RPC call is in flight."""
        for panel in dashboard.PANELS:
            if panel.live:
                with self.subTest(panel.key):
                    self.assertTrue(panel.loading, f"{panel.key} has no loading note")

    def test_panel_keys_are_unique(self) -> None:
        keys = [p.key for p in dashboard.PANELS]
        self.assertEqual(len(keys), len(set(keys)))

    def test_every_live_panel_has_a_fixture(self) -> None:
        """Guards this test file: a new live panel must be covered here too."""
        live = {p.key for p in dashboard.PANELS if p.live}
        self.assertEqual(live, set(EMPTY), "EMPTY is out of date with PANELS")

    def test_global_panels_are_the_cross_chain_ones(self) -> None:
        """A global panel must not refetch when the selected network changes."""
        globals_ = {p.key for p in dashboard.PANELS if p.global_scope}
        self.assertEqual(globals_, {"divergence", "sequencer"})


class LivePanelStates(unittest.TestCase):
    def test_error_state_renders_and_offers_a_retry(self) -> None:
        for panel in dashboard.PANELS:
            if not panel.live:
                continue
            with self.subTest(panel.key):
                rendered = panel.render("base", None, "endpoint unreachable")
                body = text_of(rendered)
                self.assertIn("endpoint unreachable", body)
                self.assertIn("retry", body)

    def test_empty_state_renders_without_raising(self) -> None:
        for panel in dashboard.PANELS:
            if not panel.live:
                continue
            with self.subTest(panel.key):
                self.assertTrue(panel.render("base", EMPTY[panel.key]))

    def test_offline_panels_render_with_no_arguments(self) -> None:
        for panel in dashboard.PANELS:
            if panel.live or panel.key in ("recipes", "about"):
                continue
            with self.subTest(panel.key):
                self.assertTrue(panel.render())


class PopulatedPanels(unittest.TestCase):
    def test_gas_renders_a_full_report(self) -> None:
        report = GasReport(
            network="ethereum", native_symbol="ETH", base_fee_wei=10**9,
            next_base_fee_wei=11 * 10**8, blocks_sampled=20, gas_used_ratios=[0.5],
            tiers=[FeeEstimate("standard", 10**8, 11 * 10**8)], native_usd=1900.0,
        )
        body = text_of(dashboard.render_gas("ethereum", report))
        self.assertIn("standard", body)
        self.assertIn("ETH/USD", body)

    def test_gas_renders_without_a_usd_price(self) -> None:
        """The native feed can be missing; the tier table must still print."""
        report = GasReport(
            network="scroll", native_symbol="ETH", base_fee_wei=10**9,
            next_base_fee_wei=10**9, blocks_sampled=5,
            tiers=[FeeEstimate("standard", 0, 10**9)],
            price_error="no native-token feed registered",
        )
        body = text_of(dashboard.render_gas("scroll", report))
        self.assertIn("standard", body)
        self.assertIn("no native-token feed registered", body)

    def test_simulation_panel_shows_the_preset_ladder(self) -> None:
        """The panel's whole point: more guards catch strictly more failure modes."""
        body = text_of(dashboard.render_simulate())
        self.assertIn("bounded_crash", body)
        self.assertIn("MISSED", body, "the naive preset must visibly fail something")
        self.assertIn("100%", body, "the strict preset must visibly pass everything")

    def test_registry_panel_counts_measured_against_bounded(self) -> None:
        body = text_of(dashboard.render_registry())
        self.assertIn("polygon", body)
        self.assertIn("measured", body)

    def test_recipes_list_and_detail(self) -> None:
        self.assertTrue(dashboard.render_recipes())
        detail = text_of(dashboard.render_recipes("ccip-cross-chain-transfer"))
        self.assertIn("STEPS", detail)

    def test_unknown_recipe_id_falls_back_to_the_list(self) -> None:
        body = text_of(dashboard.render_recipes("does-not-exist"))
        self.assertIn("RECIPES", body)

    def test_about_panel_reports_the_terminal_and_palette(self) -> None:
        body = text_of(dashboard.render_about())
        self.assertIn("colour depth", body)
        self.assertIn("blue", body)


class Painting(unittest.TestCase):
    def _app(self, width: int = 100, height: int = 30) -> dashboard.Dashboard:
        app = dashboard.Dashboard(depth=TRUE)
        app.screen = Screen(width, height, depth=TRUE, base=BASE)
        return app

    def test_a_frame_paints_chrome_and_content(self) -> None:
        app = self._app()
        app.active = [p.key for p in dashboard.PANELS].index("simulate")
        app.screen.clear()
        app.render(app.screen)
        body = app.screen.text()
        self.assertIn("Alchem-Link", body, "header is missing")
        self.assertIn("Live Feeds", body, "sidebar is missing")
        self.assertIn("bounded_crash", body, "panel content is missing")
        self.assertIn("q quit", body, "status bar is missing")

    def test_nothing_paints_outside_the_screen(self) -> None:
        app = self._app()
        app.screen.clear()
        app.render(app.screen)
        for row in app.screen.text_rows():
            self.assertLessEqual(ansi.display_width(row), app.screen.width)

    def test_a_tiny_terminal_does_not_crash(self) -> None:
        """Users resize windows to absurd sizes; a render that raises loses the screen."""
        for width, height in ((20, 6), (10, 3), (1, 1), (200, 60)):
            with self.subTest(size=(width, height)):
                app = self._app(width, height)
                app.screen.clear()
                app.render(app.screen)

    def test_a_loading_panel_paints_its_note(self) -> None:
        app = self._app()
        app.active = [p.key for p in dashboard.PANELS].index("feeds")
        body = text_of(app.lines())
        self.assertIn("Reading aggregators", body)

    def test_every_panel_paints_a_frame(self) -> None:
        for index, panel in enumerate(dashboard.PANELS):
            with self.subTest(panel.key):
                app = self._app()
                app.active = index
                app.screen.clear()
                app.render(app.screen)
                self.assertTrue(app.screen.text().strip())


class Navigation(unittest.TestCase):
    def _app(self) -> dashboard.Dashboard:
        app = dashboard.Dashboard(depth=ansi.Depth.NONE)
        app.screen = Screen(100, 30, depth=ansi.Depth.NONE)
        return app

    def test_sidebar_selection_moves_and_clamps(self) -> None:
        app = self._app()
        app.on_key(Key("down"))
        self.assertEqual(app.active, 1)
        for _ in range(100):
            app.on_key(Key("down"))
        self.assertEqual(app.active, len(dashboard.PANELS) - 1)
        for _ in range(100):
            app.on_key(Key("up"))
        self.assertEqual(app.active, 0)

    def test_tab_moves_focus_between_panes(self) -> None:
        app = self._app()
        self.assertTrue(app.focus_sidebar)
        app.on_key(Key("tab"))
        self.assertFalse(app.focus_sidebar)

    def test_scrolling_only_applies_to_the_content_pane(self) -> None:
        app = self._app()
        app.active = [p.key for p in dashboard.PANELS].index("simulate")
        app.on_key(Key("tab"))
        app.on_key(Key("down"))
        self.assertGreater(app.scroll.selected, 0)
        self.assertEqual(app.active, [p.key for p in dashboard.PANELS].index("simulate"))

    def test_network_cycles_in_both_directions(self) -> None:
        app = self._app()
        first = app.network
        app.on_key(Key("n"))
        self.assertNotEqual(app.network, first)
        app.on_key(Key("N"))
        self.assertEqual(app.network, first)

    def test_switching_panel_resets_the_scroll(self) -> None:
        app = self._app()
        app.on_key(Key("tab"))
        app.on_key(Key("pagedown"))
        app.on_key(Key("tab"))
        app.on_key(Key("down"))
        self.assertEqual(app.scroll.offset, 0)

    def test_number_keys_jump_to_a_panel(self) -> None:
        app = self._app()
        app.on_key(Key("3"))
        self.assertEqual(app.active, 2)

    def test_the_job_key_ignores_the_network_for_global_panels(self) -> None:
        app = self._app()
        app.active = [p.key for p in dashboard.PANELS].index("divergence")
        first = app.job_key
        app.network = "polygon"
        self.assertEqual(app.job_key, first, "a global panel refetched on a network switch")

    def test_the_job_key_tracks_the_network_for_scoped_panels(self) -> None:
        app = self._app()
        app.active = [p.key for p in dashboard.PANELS].index("feeds")
        first = app.job_key
        app.network = "polygon"
        self.assertNotEqual(app.job_key, first)

    def test_q_quits(self) -> None:
        app = self._app()
        app.running = True
        self.assertTrue(app._default_key(Key("q")))
        self.assertFalse(app.running)


if __name__ == "__main__":
    unittest.main()
