"""Line-oriented output: the formatters, and the rule that colour is only decoration.

The invariant this file exists to protect is that ``NO_COLOR``, a pipe, and a terminal
that admits to no colour all produce *the same text* as a full-colour terminal. That
output goes into CI logs, issue reports and ``jq`` at least as often as it goes to a
person's screen, and layout that only survives with escape sequences in it is layout that
breaks exactly where it is hardest to debug.
"""
from __future__ import annotations

import io
import json
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "src"))

from alchem_link.render import Console, fmt_age, fmt_bps, fmt_price, fmt_pct, fmt_secs
from alchem_link.term import ansi


def coloured() -> tuple:
    stream = io.StringIO()
    return Console(stream, depth=ansi.Depth.TRUECOLOR), stream


def plain() -> tuple:
    stream = io.StringIO()
    return Console(stream, depth=ansi.Depth.NONE), stream


class Formatters(unittest.TestCase):
    def test_price_precision_follows_magnitude(self) -> None:
        self.assertEqual(fmt_price(1900.5), "1,900.50")
        self.assertEqual(fmt_price(1.23456789), "1.2346")
        self.assertEqual(fmt_price(0.000012345678), "0.00001235")

    def test_price_never_eats_integer_digits(self) -> None:
        """Naive trailing-zero stripping turns "1,900.00000000" into "1,9"."""
        self.assertEqual(fmt_price(1900.0), "1,900.00")

    def test_negative_prices_keep_their_sign(self) -> None:
        self.assertTrue(fmt_price(-1900.5).startswith("-"))

    def test_age_reads_in_the_largest_useful_units(self) -> None:
        self.assertEqual(fmt_age(45), "45s")
        self.assertEqual(fmt_age(3661), "1h 1m")
        self.assertEqual(fmt_age(90061), "1d 1h")
        self.assertEqual(fmt_age(90), "1m 30s")

    def test_interval_collapses_to_whole_units(self) -> None:
        self.assertEqual(fmt_secs(3600), "1h")
        self.assertEqual(fmt_secs(86400), "1d")
        self.assertEqual(fmt_secs(60), "1m")
        self.assertEqual(fmt_secs(1200), "20m")
        self.assertEqual(fmt_secs(451), "451s")

    def test_a_missing_interval_reads_as_unknown(self) -> None:
        self.assertEqual(fmt_secs(0), "?")

    def test_signed_formats_always_carry_a_sign(self) -> None:
        self.assertTrue(fmt_bps(12.0).startswith("+"))
        self.assertTrue(fmt_pct(-1.5).startswith("-"))


class ColourIsDecoration(unittest.TestCase):
    """The same text, with or without escapes. Every helper is checked."""

    def _both(self, call) -> tuple:
        rich, rich_stream = coloured()
        flat, flat_stream = plain()
        call(rich)
        call(flat)
        return ansi.strip(rich_stream.getvalue()), flat_stream.getvalue()

    def test_headings_kvs_and_notes_match(self) -> None:
        for call in (
            lambda c: c.heading("TITLE", "detail"),
            lambda c: c.kv("price", "1,930.24"),
            lambda c: c.kvs([("a", "1"), ("longer", "2")]),
            lambda c: c.note("a hint"),
            lambda c: c.bullet("an item"),
            lambda c: c.ok("fine"),
            lambda c: c.warn("careful"),
            lambda c: c.error("broken"),
            lambda c: c.check(True, "endpoint", "reachable", "hint"),
            lambda c: c.status("FRESH", "18m ago"),
            lambda c: c.finding("high", "STALE", "past heartbeat", "detail", "fix it"),
            lambda c: c.rule(40, "section"),
        ):
            with self.subTest(call):
                rich, flat = self._both(call)
                self.assertEqual(rich, flat)

    def test_tables_match(self) -> None:
        def call(console):
            console.table(["pair", "price"], [["ETH/USD", "1,930.24"], ["BTC/USD", "68,000"]],
                          aligns=["left", "right"])

        rich, flat = self._both(call)
        self.assertEqual(rich, flat)

    def test_a_coloured_console_actually_emits_colour(self) -> None:
        """The parity tests above would also pass if colour were silently never applied."""
        console, stream = coloured()
        console.kv("price", "1")
        self.assertIn("\x1b[", stream.getvalue())

    def test_a_plain_console_emits_no_escapes(self) -> None:
        console, stream = plain()
        console.kv("price", "1")
        self.assertNotIn("\x1b", stream.getvalue())


class Layout(unittest.TestCase):
    def test_key_value_blocks_align_to_the_widest_key(self) -> None:
        console, stream = plain()
        console.kvs([("a", "1"), ("longer", "2")])
        first, second = stream.getvalue().splitlines()
        self.assertEqual(first.index("1"), second.index("2"))

    def test_table_columns_align_across_rows(self) -> None:
        console, stream = plain()
        console.table(["pair", "price"], [["ETH/USD", "1"], ["A/B", "22222"]],
                      aligns=["left", "right"])
        lines = stream.getvalue().splitlines()
        self.assertEqual(len(lines), 3)
        self.assertEqual(len(lines[1]), len(lines[2]))

    def test_table_measures_display_width_not_string_length(self) -> None:
        """A wide glyph occupies two columns; measuring with len() shifts the next one."""
        console, stream = plain()
        console.table(["pair", "x"], [["日本", "1"], ["ab", "2"]])
        rows = stream.getvalue().splitlines()[1:]
        self.assertEqual(*(ansi.display_width(row) for row in rows))

    def test_an_empty_table_prints_nothing(self) -> None:
        console, stream = plain()
        console.table(["a", "b"], [])
        self.assertEqual(stream.getvalue(), "")

    def test_rows_shorter_than_the_header_are_padded(self) -> None:
        console, stream = plain()
        console.table(["a", "b", "c"], [["1"]])
        self.assertEqual(len(stream.getvalue().splitlines()), 2)


class Structured(unittest.TestCase):
    def test_json_output_is_never_coloured(self) -> None:
        """This is somebody's `jq` input; an escape sequence makes it unparseable."""
        console, stream = coloured()
        console.json({"pair": "ETH/USD", "price": 1930.24})
        body = stream.getvalue()
        self.assertNotIn("\x1b", body)
        self.assertEqual(json.loads(body)["pair"], "ETH/USD")

    def test_json_sorts_keys_so_runs_diff_cleanly(self) -> None:
        console, stream = plain()
        console.json({"b": 1, "a": 2})
        self.assertLess(stream.getvalue().index('"a"'), stream.getvalue().index('"b"'))


class Detection(unittest.TestCase):
    def test_a_console_over_a_pipe_reports_no_colour(self) -> None:
        import os

        saved = {k: os.environ.pop(k, None) for k in
                 ("NO_COLOR", "FORCE_COLOR", "ALCHEM_COLOR", "COLORTERM")}
        try:
            self.assertFalse(Console(io.StringIO()).colour)
        finally:
            for key, value in saved.items():
                if value is not None:
                    os.environ[key] = value

    def test_reset_console_forces_re_detection(self) -> None:
        """Needed after Windows VT is enabled — a console built before it saw no colour."""
        from alchem_link import render

        render.reset_console()
        first = render.console()
        self.assertIs(render.console(), first)
        render.reset_console()
        self.assertIsNot(render.console(), first)


if __name__ == "__main__":
    unittest.main()
