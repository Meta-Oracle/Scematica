"""The terminal system, exercised without a terminal.

Every test here runs against a :class:`~alchem_link.term.screen.Screen` in memory or a
pure parser function, which is the property the whole subpackage was designed around: a
UI you cannot test is a UI whose empty and error states get discovered by users.

The cases worth having are the ones that are invisible until they are catastrophic —
a diff that reports "nothing changed" when something did, a wide character that shifts
every cell after it by one column, an escape sequence emitted at a terminal that renders
it as literal digits, and a raw-mode restore that does not happen.
"""
from __future__ import annotations

import io
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "src"))

from alchem_link.term import ansi, boot, input as term_input, widgets
from alchem_link.term.screen import Screen
from alchem_link.term.widgets import Column, Rect, Row, Scroll
from alchem_link.theme import BASE, BLACK, BLUE, GREEN, RED, Style, role

TRUE = ansi.Depth.TRUECOLOR


class ColourDepth(unittest.TestCase):
    def test_truecolor_emits_rgb(self) -> None:
        encoded = ansi.sgr(Style(fg="#4d9fff"), TRUE)
        self.assertIn("38;2;77;159;255", encoded)

    def test_256_emits_an_index(self) -> None:
        encoded = ansi.sgr(Style(fg=BLUE), ansi.Depth.ANSI256)
        self.assertIn(f"38;5;{ansi.to_256(BLUE)}", encoded)

    def test_16_emits_a_basic_code(self) -> None:
        """No ``38;5;`` and no ``38;2;`` — those render as literal digits at this depth."""
        encoded = ansi.sgr(Style(fg=BLUE), ansi.Depth.ANSI16)
        self.assertNotIn("38;5", encoded)
        self.assertNotIn("38;2", encoded)
        self.assertRegex(encoded, r"\x1b\[[0-9;]*(3[0-7]|9[0-7])")

    def test_none_emits_nothing(self) -> None:
        self.assertEqual(ansi.sgr(Style(fg=BLUE, bold=True), ansi.Depth.NONE), "")

    def test_plain_style_emits_nothing_at_any_depth(self) -> None:
        for depth in (ansi.Depth.ANSI16, ansi.Depth.ANSI256, TRUE):
            self.assertEqual(ansi.sgr(Style(), depth), "")

    def test_attributes_survive_the_downgrade(self) -> None:
        for depth in (ansi.Depth.ANSI16, ansi.Depth.ANSI256, TRUE):
            encoded = ansi.sgr(Style(fg=BLUE, bold=True, underline=True), depth)
            self.assertIn("1", encoded.split("m")[0])
            self.assertIn("4", encoded.split("m")[0])

    def test_grey_ramp_beats_the_cube_for_near_blacks(self) -> None:
        """Without the grey ramp every dark surface collapses to cube index 16.

        That is not a cosmetic loss: SURFACE and BLACK become the same colour and every
        panel in the dashboard stops having an edge.
        """
        self.assertNotEqual(ansi.to_256("#080e17"), ansi.to_256("#000000"))

    def test_no_color_beats_every_other_signal(self) -> None:
        env = {"NO_COLOR": "1", "COLORTERM": "truecolor", "FORCE_COLOR": "3"}
        import os

        saved = {k: os.environ.get(k) for k in env}
        os.environ.update(env)
        try:
            self.assertEqual(ansi.detect_depth(io.StringIO()), ansi.Depth.NONE)
        finally:
            for key, value in saved.items():
                if value is None:
                    os.environ.pop(key, None)
                else:
                    os.environ[key] = value

    def test_a_non_tty_gets_no_colour(self) -> None:
        import os

        saved = {k: os.environ.pop(k, None) for k in
                 ("NO_COLOR", "FORCE_COLOR", "ALCHEM_COLOR", "COLORTERM")}
        try:
            self.assertEqual(ansi.detect_depth(io.StringIO()), ansi.Depth.NONE)
        finally:
            for key, value in saved.items():
                if value is not None:
                    os.environ[key] = value


class Measurement(unittest.TestCase):
    def test_display_width_ignores_escapes(self) -> None:
        styled = ansi.paint("hello", role("key"), TRUE)
        self.assertGreater(len(styled), 5)
        self.assertEqual(ansi.display_width(styled), 5)

    def test_wide_characters_count_two(self) -> None:
        self.assertEqual(ansi.display_width("日本"), 4)

    def test_combining_marks_count_zero(self) -> None:
        self.assertEqual(ansi.display_width("é"), 1)

    def test_truncate_respects_display_width(self) -> None:
        self.assertEqual(ansi.display_width(ansi.truncate("日本語です", 5)), 5)

    def test_truncate_leaves_short_text_alone(self) -> None:
        self.assertEqual(ansi.truncate("abc", 10), "abc")

    def test_pad_produces_exact_width(self) -> None:
        for align in ("left", "right", "center"):
            self.assertEqual(ansi.display_width(ansi.pad("ab", 7, align)), 7)

    def test_pad_truncates_rather_than_overflowing(self) -> None:
        self.assertEqual(ansi.display_width(ansi.pad("abcdefghij", 4)), 4)

    def test_strip_removes_osc_as_well_as_csi(self) -> None:
        noisy = ansi.set_background(BLACK) + ansi.move(1, 1) + "text" + ansi.RESET
        self.assertEqual(ansi.strip(noisy), "text")


class ScreenBuffer(unittest.TestCase):
    def setUp(self) -> None:
        self.screen = Screen(20, 4, depth=TRUE)

    def test_put_writes_text(self) -> None:
        self.screen.put(1, 2, "hello", role("key"))
        self.assertEqual(self.screen.text_rows()[1], "  hello")

    def test_put_clips_at_the_right_edge(self) -> None:
        self.screen.put(0, 15, "abcdefghij")
        self.assertEqual(len(self.screen.text_rows()[0]), 20)

    def test_put_outside_the_grid_is_a_no_op(self) -> None:
        self.assertEqual(self.screen.put(99, 0, "nope"), 0)
        self.assertEqual(self.screen.put(0, 99, "nope"), 0)

    def test_wide_character_occupies_two_cells(self) -> None:
        self.screen.put(0, 0, "日x")
        self.assertEqual(self.screen.char_at(0, 0), "日")
        self.assertEqual(self.screen.char_at(0, 1), "", "no continuation cell")
        self.assertEqual(self.screen.char_at(0, 2), "x")

    def test_overwriting_a_wide_characters_lead_erases_its_tail(self) -> None:
        """Otherwise the terminal draws half a glyph and our column arithmetic drifts."""
        self.screen.put(0, 0, "日")
        self.screen.put(0, 0, "a")
        self.assertEqual(self.screen.char_at(0, 0), "a")
        self.assertEqual(self.screen.char_at(0, 1), " ")

    def test_overwriting_a_wide_characters_tail_erases_its_lead(self) -> None:
        self.screen.put(0, 0, "日")
        self.screen.put(0, 1, "b")
        self.assertEqual(self.screen.char_at(0, 0), " ")
        self.assertEqual(self.screen.char_at(0, 1), "b")

    def test_a_wide_character_at_the_edge_becomes_a_blank(self) -> None:
        """Half a glyph in the last column would desynchronise the cursor."""
        self.screen.put(0, 19, "日")
        self.assertEqual(self.screen.char_at(0, 19), " ")

    def test_escapes_in_input_are_stripped_not_printed(self) -> None:
        self.screen.put(0, 0, ansi.paint("hi", role("key"), TRUE))
        self.assertEqual(self.screen.text_rows()[0], "hi")

    def test_style_is_recorded_per_cell(self) -> None:
        self.screen.put(0, 0, "ok", role("ok"))
        self.assertEqual(self.screen.style_at(0, 0).fg, GREEN)

    def test_box_draws_corners_and_edges(self) -> None:
        self.screen.box(0, 0, 6, 3, role("border"))
        rows = self.screen.text_rows()
        self.assertTrue(rows[0].startswith("┌────┐"))
        self.assertTrue(rows[2].startswith("└────┘"))
        self.assertEqual(self.screen.char_at(1, 0), "│")

    def test_box_smaller_than_two_cells_is_a_no_op(self) -> None:
        self.screen.box(0, 0, 1, 1, role("border"))
        self.assertEqual(self.screen.text(), "\n" * 3)


class ScreenDiff(unittest.TestCase):
    def test_first_flush_paints_everything(self) -> None:
        screen = Screen(10, 2, depth=TRUE)
        screen.put(0, 0, "hello")
        self.assertGreater(screen.flush(io.StringIO()), 0)

    def test_an_unchanged_frame_writes_nothing(self) -> None:
        """The whole point of double buffering. An idle dashboard must cost zero bytes."""
        screen = Screen(10, 2, depth=TRUE)
        screen.put(0, 0, "hello")
        screen.flush(io.StringIO())
        screen.clear()
        screen.put(0, 0, "hello")
        self.assertEqual(screen.flush(io.StringIO()), 0)

    def test_a_changed_cell_is_repainted(self) -> None:
        screen = Screen(10, 2, depth=TRUE)
        screen.put(0, 0, "hello")
        screen.flush(io.StringIO())
        screen.clear()
        screen.put(0, 0, "hellX")
        frame = screen.render()
        self.assertIn("X", frame)

    def test_the_diff_only_touches_changed_rows(self) -> None:
        screen = Screen(10, 4, depth=ansi.Depth.NONE)
        for row in range(4):
            screen.put(row, 0, f"row{row}")
        screen.flush(io.StringIO())
        screen.clear()
        for row in range(4):
            screen.put(row, 0, "row9" if row == 2 else f"row{row}")
        frame = screen.render()
        # One cursor move, to row 3 (1-indexed). No other row is addressed.
        self.assertIn("\x1b[3;", frame)
        self.assertNotIn("\x1b[1;", frame)
        self.assertNotIn("\x1b[2;", frame)

    def test_invalidate_forces_a_full_repaint(self) -> None:
        screen = Screen(10, 2, depth=TRUE)
        screen.put(0, 0, "hello")
        screen.flush(io.StringIO())
        screen.clear()
        screen.put(0, 0, "hello")
        screen.invalidate()
        self.assertGreater(screen.flush(io.StringIO()), 0)

    def test_flush_adopts_the_back_buffer_even_when_empty(self) -> None:
        """If it did not, an unchanged frame would repaint forever after the first."""
        screen = Screen(10, 2, depth=TRUE)
        screen.flush(io.StringIO())
        self.assertEqual(screen.flush(io.StringIO()), 0)

    def test_resize_discards_both_buffers(self) -> None:
        screen = Screen(10, 2, depth=TRUE)
        screen.put(0, 0, "hello")
        screen.flush(io.StringIO())
        screen.resize(20, 4)
        self.assertEqual(screen.width, 20)
        screen.put(0, 0, "hello")
        self.assertGreater(screen.flush(io.StringIO()), 0)

    def test_no_colour_depth_produces_a_frame_with_no_sgr(self) -> None:
        screen = Screen(10, 1, depth=ansi.Depth.NONE)
        screen.put(0, 0, "hi", role("ok"))
        frame = screen.render()
        self.assertNotIn("38;", frame)
        self.assertIn("hi", frame)


class KeyParsing(unittest.TestCase):
    def test_plain_characters(self) -> None:
        key, rest = term_input.parse_key("a")
        self.assertEqual((key.name, key.char, rest), ("a", "a", ""))

    def test_control_characters(self) -> None:
        key, _ = term_input.parse_key("\x03")
        self.assertEqual((key.name, key.ctrl), ("c", True))

    def test_enter_tab_and_backspace(self) -> None:
        for raw, expected in (("\r", "enter"), ("\n", "enter"), ("\t", "tab"),
                              ("\x7f", "backspace"), ("\x08", "backspace")):
            key, _ = term_input.parse_key(raw)
            self.assertEqual(key.name, expected, raw)

    def test_arrow_keys(self) -> None:
        for final, expected in (("A", "up"), ("B", "down"), ("C", "right"), ("D", "left")):
            key, rest = term_input.parse_key(f"\x1b[{final}")
            self.assertEqual((key.name, rest), (expected, ""))

    def test_modified_arrows(self) -> None:
        key, _ = term_input.parse_key("\x1b[1;5A")   # ctrl
        self.assertTrue(key.ctrl and key.name == "up")
        key, _ = term_input.parse_key("\x1b[1;2B")   # shift
        self.assertTrue(key.shift and key.name == "down")
        key, _ = term_input.parse_key("\x1b[1;3C")   # alt
        self.assertTrue(key.alt and key.name == "right")

    def test_navigation_and_function_keys(self) -> None:
        for raw, expected in (("\x1b[5~", "pageup"), ("\x1b[6~", "pagedown"),
                              ("\x1b[3~", "delete"), ("\x1b[H", "home"),
                              ("\x1b[F", "end"), ("\x1b[15~", "f5"),
                              ("\x1bOP", "f1"), ("\x1b[Z", "backtab")):
            key, _ = term_input.parse_key(raw)
            self.assertEqual(key.name, expected, raw)

    def test_alt_prefixed_character(self) -> None:
        key, _ = term_input.parse_key("\x1bx")
        self.assertTrue(key.alt and key.name == "x")

    def test_a_lone_escape_resolves_once_the_read_times_out(self) -> None:
        """The ambiguity at the heart of terminal input: Esc, or the start of an arrow?"""
        pending, rest = term_input.parse_key("\x1b", expect_more=True)
        self.assertIsNone(pending, "committed to Escape while more bytes could arrive")
        self.assertEqual(rest, "\x1b")

        key, rest = term_input.parse_key("\x1b", expect_more=False)
        self.assertEqual((key.name, rest), ("escape", ""))

    def test_a_partial_sequence_is_held_rather_than_mangled(self) -> None:
        pending, rest = term_input.parse_key("\x1b[1;", expect_more=True)
        self.assertIsNone(pending)
        self.assertEqual(rest, "\x1b[1;")

    def test_a_split_sequence_parses_once_complete(self) -> None:
        buffer = "\x1b["
        event, buffer = term_input.parse_key(buffer, expect_more=True)
        self.assertIsNone(event)
        buffer += "A"
        event, buffer = term_input.parse_key(buffer, expect_more=True)
        self.assertEqual(event.name, "up")

    def test_unrecognised_csi_is_dropped_not_surfaced(self) -> None:
        """Terminals emit sequences nobody asked for; a fake keystroke moves the cursor."""
        event, rest = term_input.parse_key("\x1b[?1004h" + "a")
        self.assertIsNone(event)
        self.assertEqual(rest, "a")

    def test_multiple_keys_in_one_buffer(self) -> None:
        buffer = "\x1b[Aab"
        names = []
        while buffer:
            event, buffer = term_input.parse_key(buffer)
            if event is not None:
                names.append(event.name)
        self.assertEqual(names, ["up", "a", "b"])

    def test_sgr_mouse_press(self) -> None:
        event, _ = term_input.parse_key("\x1b[<0;10;5M")
        self.assertIsInstance(event, term_input.Mouse)
        self.assertEqual((event.column, event.row, event.pressed), (9, 4, True))

    def test_sgr_mouse_wheel(self) -> None:
        event, _ = term_input.parse_key("\x1b[<64;1;1M")
        self.assertEqual(event.wheel, -1)

    def test_key_str_names_its_modifiers(self) -> None:
        self.assertEqual(str(term_input.Key("up", ctrl=True)), "ctrl+up")

    def test_is_text_excludes_control_and_navigation(self) -> None:
        self.assertTrue(term_input.Key("a", char="a").is_text)
        self.assertFalse(term_input.Key("a", char="a", ctrl=True).is_text)
        self.assertFalse(term_input.Key("pagedown").is_text)

    def test_raw_mode_degrades_rather_than_raising_without_a_terminal(self) -> None:
        with term_input.raw_mode(io.StringIO()) as engaged:
            self.assertFalse(engaged)


class Widgets(unittest.TestCase):
    def test_rect_splits_and_insets(self) -> None:
        rect = Rect(0, 0, 100, 40)
        left, right = rect.split_h(20)
        self.assertEqual((left.width, right.width, right.column), (20, 80, 20))
        top, bottom = rect.split_v(-1)
        self.assertEqual((top.height, bottom.height, bottom.row), (39, 1, 39))
        self.assertEqual(rect.pad(1), Rect(1, 1, 98, 38))

    def test_inset_clamps_rather_than_going_negative(self) -> None:
        self.assertTrue(Rect(0, 0, 2, 2).inset(left=5).is_empty)

    def test_wrap_breaks_on_words(self) -> None:
        self.assertEqual(widgets.wrap("the quick brown fox", 10),
                         ["the quick", "brown fox"])

    def test_wrap_hard_breaks_a_word_longer_than_the_line(self) -> None:
        lines = widgets.wrap("0xcA11bde05977b3631167028862bE2a173976CA11", 10)
        self.assertTrue(all(len(line) <= 10 for line in lines))
        self.assertEqual("".join(lines).replace("…", ""),
                         "0xcA11bde05977b3631167028862bE2a173976CA11"[:len("".join(lines))])

    def test_wrap_of_zero_width_returns_nothing(self) -> None:
        self.assertEqual(widgets.wrap("anything", 0), [])

    def test_sparkline_maps_low_and_high_to_the_ramp_ends(self) -> None:
        line = widgets.sparkline([1, 2, 3, 4, 5, 6, 7, 8])
        self.assertEqual(line[0], widgets.SPARK_CHARS[0])
        self.assertEqual(line[-1], widgets.SPARK_CHARS[-1])

    def test_sparkline_of_a_flat_series_is_a_mid_line(self) -> None:
        """A price that has not moved is real data, not missing data."""
        line = widgets.sparkline([5, 5, 5, 5])
        self.assertEqual(set(line), {widgets.SPARK_CHARS[len(widgets.SPARK_CHARS) // 2]})

    def test_sparkline_downsamples_to_the_requested_width(self) -> None:
        self.assertEqual(len(widgets.sparkline(list(range(200)), 20)), 20)

    def test_sparkline_of_nothing_is_empty(self) -> None:
        self.assertEqual(widgets.sparkline([]), "")

    def test_column_widths_fit_the_available_space(self) -> None:
        columns = [Column("pair", width=10), Column("detail", flex=1)]
        rows = [Row(["ETH/USD", "something"])]
        widths = widgets.resolve_widths(columns, rows, total=40, gap=2)
        self.assertEqual(sum(widths) + 2, 40)

    def test_column_widths_shrink_together_when_cramped(self) -> None:
        """A narrow terminal must keep every column, not push the last one off-screen."""
        columns = [Column("a", width=30), Column("b", width=30), Column("c", width=30)]
        widths = widgets.resolve_widths(columns, [], total=40, gap=2)
        self.assertLessEqual(sum(widths) + 4, 40)
        self.assertTrue(all(w >= 1 for w in widths))

    def test_content_sized_columns_measure_their_cells(self) -> None:
        columns = [Column("p"), Column("q")]
        rows = [Row(["longer-value", "x"])]
        widths = widgets.resolve_widths(columns, rows, total=100, gap=2)
        self.assertEqual(widths[0], len("longer-value"))

    def test_table_draws_a_header_and_rows(self) -> None:
        screen = Screen(40, 5, depth=TRUE)
        drawn = widgets.table(
            screen, Rect(0, 0, 40, 5),
            [Column("pair", width=10), Column("price", width=10, align="right")],
            [Row(["ETH/USD", "1,930.24"]), Row(["BTC/USD", "68,000.00"])],
        )
        self.assertEqual(drawn, 2)
        rows = screen.text_rows()
        self.assertIn("pair", rows[0])
        self.assertIn("ETH/USD", rows[1])

    def test_table_clips_to_its_rect(self) -> None:
        screen = Screen(40, 3, depth=TRUE)
        drawn = widgets.table(
            screen, Rect(0, 0, 40, 3), [Column("n", width=6)],
            [Row([str(i)]) for i in range(20)],
        )
        self.assertEqual(drawn, 2, "a table must not paint past its rect")

    def test_panel_returns_its_interior(self) -> None:
        screen = Screen(20, 6, depth=TRUE)
        inner = widgets.panel(screen, Rect(0, 0, 20, 6), "Title")
        self.assertEqual((inner.row, inner.column, inner.width, inner.height), (1, 1, 18, 4))
        self.assertIn("Title", screen.text_rows()[0])

    def test_focused_panel_uses_a_heavy_border(self) -> None:
        """At 16 colours the focus colour collapses into the border colour; shape survives."""
        screen = Screen(20, 4, depth=TRUE)
        widgets.panel(screen, Rect(0, 0, 20, 4), "T", focused=True)
        self.assertEqual(screen.char_at(0, 0), "┏")

    def test_gauge_fills_proportionally(self) -> None:
        screen = Screen(20, 1, depth=TRUE)
        widgets.gauge(screen, Rect(0, 0, 20, 1), 0.5)
        row = screen.text_rows()[0]
        self.assertEqual(row.count(widgets.GAUGE_FULL), 10)

    def test_gauge_clamps_out_of_range_fractions(self) -> None:
        screen = Screen(10, 1, depth=TRUE)
        widgets.gauge(screen, Rect(0, 0, 10, 1), 5.0)
        self.assertEqual(screen.text_rows()[0].count(widgets.GAUGE_FULL), 10)
        screen.clear()
        widgets.gauge(screen, Rect(0, 0, 10, 1), -1.0)
        self.assertEqual(screen.text_rows()[0].count(widgets.GAUGE_FULL), 0)

    def test_tabs_report_hit_test_spans(self) -> None:
        screen = Screen(40, 1, depth=TRUE)
        spans = widgets.tabs(screen, Rect(0, 0, 40, 1), ["one", "two", "three"], 1)
        self.assertEqual(len(spans), 3)
        self.assertEqual(spans[0][1], 0)

    def test_tabs_stop_at_the_edge(self) -> None:
        screen = Screen(10, 1, depth=TRUE)
        spans = widgets.tabs(screen, Rect(0, 0, 10, 1), ["aaaa", "bbbb", "cccc"], 0)
        self.assertLess(len(spans), 3)

    def test_key_value_aligns_to_the_widest_key(self) -> None:
        screen = Screen(40, 3, depth=TRUE)
        widgets.key_value(screen, Rect(0, 0, 40, 3),
                          [("a", "1"), ("longer", "2")])
        rows = screen.text_rows()
        self.assertEqual(rows[0].index("1"), rows[1].index("2"))


class Scrolling(unittest.TestCase):
    def test_selection_pulls_the_offset_along(self) -> None:
        scroll = Scroll()
        scroll.move(15, total=50, height=10)
        self.assertEqual(scroll.selected, 15)
        self.assertEqual(scroll.offset, 6)

    def test_selection_clamps_at_both_ends(self) -> None:
        scroll = Scroll()
        scroll.move(-5, total=50, height=10)
        self.assertEqual(scroll.selected, 0)
        scroll.move(500, total=50, height=10)
        self.assertEqual(scroll.selected, 49)

    def test_offset_never_scrolls_past_the_end(self) -> None:
        scroll = Scroll(offset=100, selected=49)
        scroll.clamp(total=50, height=10)
        self.assertEqual(scroll.offset, 40)

    def test_a_list_shorter_than_the_window_never_scrolls(self) -> None:
        scroll = Scroll()
        scroll.end(total=3, height=10)
        self.assertEqual(scroll.offset, 0)

    def test_an_empty_list_resets(self) -> None:
        scroll = Scroll(offset=5, selected=5)
        scroll.clamp(total=0, height=10)
        self.assertEqual((scroll.offset, scroll.selected), (0, 0))

    def test_paging_moves_almost_a_full_window(self) -> None:
        scroll = Scroll()
        scroll.page(1, total=100, height=10)
        self.assertEqual(scroll.selected, 9)

    def test_scrollbar_is_omitted_when_everything_fits(self) -> None:
        screen = Screen(5, 5, depth=TRUE)
        widgets.scrollbar(screen, Rect(0, 4, 1, 5), total=3, offset=0, height=5)
        self.assertEqual(screen.text().strip(), "")


class Boot(unittest.TestCase):
    def test_theme_sequence_sets_background_foreground_and_cursor(self) -> None:
        sequence = ansi.theme_terminal(BLACK, BLUE)
        self.assertIn("]11;" + BLACK, sequence)
        self.assertIn("]10;" + BLUE, sequence)
        self.assertIn("]12;" + BLUE, sequence)

    def test_initialize_writes_the_palette_when_asked(self) -> None:
        stream = io.StringIO()
        boot.restore(stream)  # start from a known state
        state = boot.initialize(title="test", theme_terminal=True, stream=stream)
        try:
            self.assertTrue(state.themed)
            written = stream.getvalue()
            self.assertIn("]11;", written, "background was not repainted")
            self.assertIn("test", written, "title was not set")
        finally:
            boot.restore(stream)

    def test_initialize_is_idempotent(self) -> None:
        stream = io.StringIO()
        boot.restore(stream)
        boot.initialize(title="a", theme_terminal=True, stream=stream)
        first = len(stream.getvalue())
        boot.initialize(title="a", theme_terminal=True, stream=stream)
        try:
            self.assertEqual(len(stream.getvalue()), first,
                             "re-theming on every entry point would flicker the terminal")
        finally:
            boot.restore(stream)

    def test_restore_hands_the_colours_back(self) -> None:
        stream = io.StringIO()
        boot.initialize(title="t", theme_terminal=True, stream=stream)
        stream.truncate(0)
        stream.seek(0)
        boot.restore(stream)
        written = stream.getvalue()
        self.assertIn("]111", written, "the default background was never restored")
        self.assertIn(ansi.SHOW_CURSOR, written)

    def test_restore_is_safe_to_call_twice(self) -> None:
        stream = io.StringIO()
        boot.restore(stream)
        boot.restore(stream)

    def test_initialize_declines_when_the_stream_takes_no_colour(self) -> None:
        stream = io.StringIO()
        boot.restore(stream)
        state = boot.initialize(theme_terminal=False, stream=stream)
        self.assertFalse(state.themed)
        self.assertEqual(stream.getvalue(), "")

    def test_describe_reports_the_negotiated_state(self) -> None:
        info = boot.describe()
        for field in ("themed", "colour_depth", "frozen", "platform", "is_tty"):
            self.assertIn(field, info)

    def test_banner_title_marks_a_frozen_build(self) -> None:
        self.assertIn("0.0.1", boot.banner_title("0.0.1"))


if __name__ == "__main__":
    unittest.main()
