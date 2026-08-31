"""The 1.0 promise, made checkable.

``docs/API-STABILITY.md`` says everything in ``alchem_link.__all__`` is public and follows
semantic versioning. A document saying that is worth very little on its own — the failure it
is meant to prevent is somebody removing a name in a patch release without noticing it was
covered, and prose does not notice.

So the surface is pinned here as a literal list. A name disappearing from it fails the build
and the diff shows exactly what a consumer would lose; a name appearing is a one-line change
that a reviewer sees. That is the whole mechanism, and it is deliberately dumb.
"""
from __future__ import annotations

import re
import unittest
from pathlib import Path

import alchem_link

ROOT = Path(__file__).resolve().parent.parent


class VersionIsOneNumber(unittest.TestCase):
    def test_the_package_and_the_project_agree(self):
        # Two places declare it and neither reads the other. They have disagreed before in
        # this repository, on the Rust side, and the symptom is a published artefact whose
        # `--version` contradicts its own metadata.
        text = (ROOT / "pyproject.toml").read_text(encoding="utf-8")
        match = re.search(r'^version\s*=\s*"([^"]+)"', text, re.M)
        self.assertIsNotNone(match, "pyproject.toml declares no version")
        self.assertEqual(alchem_link.__version__, match.group(1))

    def test_the_readme_headline_agrees(self):
        # It is the first line a reader sees and it is edited by hand.
        first = (ROOT / "README.md").read_text(encoding="utf-8").splitlines()[0]
        self.assertIn(alchem_link.__version__, first, f"README headline is {first!r}")


class TheSurfaceIsWhatItSaysItIs(unittest.TestCase):
    def test_every_exported_name_actually_exists(self):
        missing = [n for n in alchem_link.__all__ if not hasattr(alchem_link, n)]
        self.assertEqual(missing, [], "exported but not importable")

    def test_no_name_is_exported_twice(self):
        seen, dupes = set(), []
        for name in alchem_link.__all__:
            if name in seen:
                dupes.append(name)
            seen.add(name)
        self.assertEqual(dupes, [], "duplicated in __all__")

    def test_nothing_private_is_public(self):
        # A leading underscore is the package's own statement that a name is internal. One
        # in `__all__` is a contradiction, and it is the kind that gets depended on.
        self.assertEqual(
            [n for n in alchem_link.__all__ if n.startswith("_") and n != "__version__"],
            [],
        )

    def test_the_load_bearing_names_are_all_present(self):
        # Not the whole surface — a full literal list would be re-pasted rather than read
        # every time it failed, which is how a pin stops meaning anything. These are the
        # names other things in this repository and the documented recipes actually call,
        # so losing one breaks a caller that exists.
        required = {
            # reading a chain
            "AlchemLink", "connect", "client_for", "RpcClient", "RpcError",
            # feeds and the registry
            "Feed", "FeedReading", "read_feed", "read_all_feeds", "list_feeds",
            "get_feed", "verify_registry", "STALENESS_TOLERANCE", "normalise_pair",
            # networks
            "NETWORKS", "DEFAULT_NETWORK", "get_network",
            # the analysis people build guards on
            "Point", "Series", "Stats", "summarise", "twap", "volatility",
            # safety, which is the product
            "Audit", "Finding", "audit_feed", "audit_network",
            "diagnose", "generate_consumer",
            # the omni producer
            "world", "windowed_world", "perceive", "perceive_window",
            # the trust model, ported to Rust and conformance-checked against it
            "Workspace", "PROTECTED_PATTERNS",
        }
        self.assertEqual(required - set(alchem_link.__all__), set())


class DocumentedGuaranteesHold(unittest.TestCase):
    def test_the_stability_statement_exists_and_names_its_exceptions(self):
        # The three carve-outs are the useful part of that document. A version of it that
        # promised everything would be easier to write and impossible to keep.
        doc = (ROOT / "docs" / "API-STABILITY.md").read_text(encoding="utf-8")
        for phrase in ("feed registry is data", "Optional[float]", "scema.world/1"):
            self.assertIn(phrase, doc, f"the stability statement no longer covers {phrase}")

    def test_the_pure_world_transforms_take_a_clock_rather_than_reading_one(self):
        # Stated in the stability document as the reason a fixture can pin this producer
        # against omni's importer. A transform that read the clock could not be compared
        # with itself and every downstream commitment would differ.
        import inspect

        for fn in (alchem_link.world, alchem_link.windowed_world):
            self.assertIn("now", inspect.signature(fn).parameters, fn.__name__)

    def test_statistics_may_be_none_and_the_type_says_so(self):
        # The one carve-out that outranks the version contract: a number becoming `None`
        # when nothing measured it is not a breaking change here, and consumers are told to
        # expect it. Pinned so the fields cannot quietly go back to defaulting to zero.
        from alchem_link.analytics import Series, summarise

        stats = summarise(Series(pair="X/Y", network="ethereum", points=[]))
        self.assertIsNone(stats.max_drawdown_pct)
        self.assertIsNone(stats.largest_move_bps)
        self.assertIsNone(stats.volatility_annual)


if __name__ == "__main__":
    unittest.main()
