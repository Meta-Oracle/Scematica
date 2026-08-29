"""The trust model's conformance vectors, run against the reference implementation.

``vectors/trust-model.json`` is the shared contract described in ``docs/TRUST-MODEL.md``.
Scematica Omni needs the same model in Rust before it can grow an action path, and the one
thing that keeps two implementations of a security decision from drifting is a file they
both run. Same arrangement as ``canonical.rs`` / ``canonical.ts``.

These tests are deliberately thin. They exist to prove the vectors are *runnable and
correct against Python*, so that a second implementation failing them is evidence about the
second implementation and not about the file. The behavioural reasoning lives in
``test_agent_workspace.py``; this is the wire.
"""
from __future__ import annotations

import json
import unittest
from pathlib import Path

from alchem_link.approvals import Decision, Request, Risk, Rule, TrustPolicy
from alchem_link.workspace import PROTECTED_PATTERNS, Workspace

VECTORS = Path(__file__).resolve().parents[1] / "vectors" / "trust-model.json"


def load() -> dict:
    with VECTORS.open(encoding="utf-8") as fh:
        return json.load(fh)


def policy_from(spec: dict) -> TrustPolicy:
    policy = TrustPolicy(
        read_only=spec.get("read_only", False),
        allow_writes=spec.get("allow_writes", False),
        allow_execute=spec.get("allow_execute", False),
    )
    for rule in spec.get("rules", []):
        policy.rules.append(
            Rule(
                tool=rule["tool"],
                decision=Decision(rule["decision"]),
                path=rule.get("path", "*"),
            )
        )
    for key, decision in spec.get("session_grants", {}).items():
        policy.session_grants[key] = Decision(decision)
    return policy


class TrustVectors(unittest.TestCase):
    def test_every_case_matches_the_reference_implementation(self) -> None:
        data = load()
        self.assertEqual(data["version"], 1)
        failures = []

        for case in data["cases"]:
            policy = policy_from(case["policy"])
            req = case["request"]
            request = Request(
                tool=req["tool"],
                risk=Risk(req["risk"]),
                path=req.get("path", ""),
            )
            got = policy.preflight(request)
            actual = "ask" if got is None else got.value
            # `allow_always` / `deny_always` are prompt answers, never preflight results;
            # a grant is stored already normalised to allow/deny.
            if actual not in ("allow", "deny", "ask"):
                failures.append(f"{case['name']}: preflight returned {actual!r}")
                continue
            if actual != case["expected"]:
                failures.append(
                    f"{case['name']}: expected {case['expected']!r}, got {actual!r}"
                )

        self.assertEqual(failures, [], "\n" + "\n".join(failures))

    def test_ask_is_a_distinct_outcome_and_is_actually_exercised(self) -> None:
        # A vector file where nothing asks would pass against an implementation that never
        # prompts — which is the most dangerous way for this to be wrong.
        data = load()
        outcomes = {c["expected"] for c in data["cases"]}
        self.assertEqual(outcomes, {"allow", "deny", "ask"})

    def test_the_ordering_properties_are_covered(self) -> None:
        # The vectors exist for the cases where a wrong implementation still looks
        # plausible. Losing one of these to a tidy-up would leave the file passing and
        # meaningless, so the coverage itself is asserted.
        names = {c["name"] for c in load()["cases"]}
        for required in (
            "a session grant does not survive a hard refusal",
            "an explicit rule outranks a session grant",
            "a deny_always grant outranks a permissive configuration",
            "a grant does not leak into a sibling directory",
            "the first matching rule wins",
        ):
            self.assertIn(required, names, f"vector coverage lost: {required}")

    def test_protected_paths_are_refused_for_reads(self) -> None:
        # Workspace-level, before any prompt, and for reads as well as writes. A user
        # cannot consent to a disclosure they have not been shown.
        import fnmatch

        data = load()["protected_paths"]

        def protected(name: str) -> bool:
            base = name.rsplit("/", 1)[-1].lower()
            low = name.lower()
            return any(
                fnmatch.fnmatch(base, pat) or fnmatch.fnmatch(low, pat)
                for pat in PROTECTED_PATTERNS
            )

        for path in data["refused"]:
            self.assertTrue(protected(path), f"{path} must be protected")
        for path in data["allowed"]:
            self.assertFalse(protected(path), f"{path} must not be protected")

    def test_the_workspace_itself_agrees_with_the_vectors(self) -> None:
        # The helper above reimplements pattern matching, so it could agree with the
        # vectors while `Workspace` disagrees with both. Ask the real thing.
        #
        # `is_protected` rather than `assertRaises` around `resolve`: the first version of
        # this test asserted that resolving raised *something*, and it passed for every
        # path because none of them existed in the temporary root. A missing file and a
        # refused secret are different outcomes, and a test that cannot tell them apart is
        # a test that would keep passing after the protection was removed.
        import tempfile

        data = load()["protected_paths"]
        with tempfile.TemporaryDirectory() as root:
            ws = Workspace(Path(root))
            for path in data["refused"]:
                self.assertTrue(
                    ws.is_protected(Path(root) / path),
                    f"{path} must be refused by Workspace itself",
                )
            for path in data["allowed"]:
                self.assertFalse(
                    ws.is_protected(Path(root) / path),
                    f"{path} must not be treated as a secret",
                )


if __name__ == "__main__":
    unittest.main()
