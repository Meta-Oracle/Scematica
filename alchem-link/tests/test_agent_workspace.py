"""The sandbox, the trust layer, and the coding tools.

This is the security-relevant test file in the package, and it is written accordingly:
the interesting cases are the ones where something should *not* happen. A test that a
write succeeds proves a feature works. A test that a write to ``../../etc`` fails proves
the feature cannot be turned against the user, which is the property that actually
matters once a language model is choosing the arguments.

Every test runs against a temporary directory. Nothing here touches the real filesystem
outside one, and nothing here reaches a network or a model.
"""
from __future__ import annotations

import os
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "src"))

from alchem_link.agent import TOOLS, execute_tool, tools_for
from alchem_link.agent_tools import ToolContext
from alchem_link.approvals import (
    AutoApprover,
    CallbackApprover,
    Decision,
    DenyApprover,
    Request,
    Risk,
    Rule,
    TrustPolicy,
)
from alchem_link.workspace import (
    PROTECTED_PATTERNS,
    PathEscape,
    ProtectedPath,
    Workspace,
    WorkspaceError,
)


class Sandboxed(unittest.TestCase):
    """Base class giving each test a populated temporary workspace."""

    def setUp(self) -> None:
        self._tmp = tempfile.TemporaryDirectory()
        self.root = Path(self._tmp.name).resolve()
        (self.root / "src").mkdir()
        (self.root / "src" / "main.py").write_text("print('hello')\n", encoding="utf-8")
        (self.root / "README.md").write_text("# demo\n", encoding="utf-8")
        (self.root / ".env").write_text("ALCHEMY_API_KEY=supersecret\n", encoding="utf-8")
        self.workspace = Workspace(self.root)

    def tearDown(self) -> None:
        self._tmp.cleanup()

    def context(self, policy: TrustPolicy = None, approver=None) -> ToolContext:
        return ToolContext(
            workspace=self.workspace,
            policy=policy or TrustPolicy(),
            approver=approver or AutoApprover(),
        )

    def run_tool(self, name: str, context: ToolContext = None, **arguments):
        return execute_tool(name, arguments, context or self.context())


class PathConfinement(Sandboxed):
    def test_relative_paths_resolve_under_the_root(self) -> None:
        self.assertEqual(self.workspace.resolve("src/main.py").parent.name, "src")

    def test_parent_traversal_is_refused(self) -> None:
        for attempt in ("../outside.txt", "../../etc/passwd", "src/../../escape"):
            with self.subTest(attempt):
                with self.assertRaises(PathEscape):
                    self.workspace.resolve(attempt)

    def test_absolute_paths_outside_the_root_are_refused(self) -> None:
        outside = Path(tempfile.gettempdir()).resolve() / "alchem-link-should-not-exist"
        with self.assertRaises(PathEscape):
            self.workspace.resolve(str(outside))

    def test_an_absolute_path_inside_the_root_is_accepted(self) -> None:
        """The model is shown absolute paths, so echoing one back must not be an escape."""
        inside = self.root / "src" / "main.py"
        self.assertEqual(self.workspace.resolve(str(inside)), inside)

    @unittest.skipIf(os.name == "nt", "symlink creation needs privileges on Windows")
    def test_a_symlink_pointing_outside_is_refused(self) -> None:
        """The case a string check for '..' cannot catch."""
        link = self.root / "escape"
        link.symlink_to(Path(tempfile.gettempdir()).resolve())
        with self.assertRaises(PathEscape):
            self.workspace.resolve("escape/anything.txt")

    def test_empty_paths_are_refused(self) -> None:
        for attempt in ("", "   "):
            with self.assertRaises(WorkspaceError):
                self.workspace.resolve(attempt)

    def test_a_missing_root_is_refused_at_construction(self) -> None:
        with self.assertRaises(WorkspaceError):
            Workspace(self.root / "does-not-exist")

    def test_a_file_cannot_be_a_root(self) -> None:
        with self.assertRaises(WorkspaceError):
            Workspace(self.root / "README.md")


class SecretsAreNeverRead(Sandboxed):
    """Tool output goes to a third-party model, so a read of a secret is a disclosure."""

    def test_dotenv_is_refused(self) -> None:
        with self.assertRaises(ProtectedPath):
            self.workspace.read_text(".env")

    def test_refusal_survives_a_roundabout_path(self) -> None:
        with self.assertRaises(ProtectedPath):
            self.workspace.read_text("src/../.env")

    def test_the_usual_secret_shapes_are_covered(self) -> None:
        for name in ("id_rsa", "server.pem", "deploy.key", ".env.production",
                     "wallet.json", "my-keypair.json", ".npmrc", "credentials"):
            (self.root / name).write_text("secret", encoding="utf-8")
            with self.subTest(name):
                self.assertTrue(self.workspace.is_protected(self.root / name))

    def test_protection_is_case_insensitive(self) -> None:
        """Windows will serve ID_RSA for id_rsa; a case-sensitive check is no check."""
        self.assertTrue(self.workspace.is_protected(self.root / "ID_RSA"))
        self.assertTrue(self.workspace.is_protected(self.root / ".ENV"))

    def test_protected_directories_match_at_any_depth(self) -> None:
        self.assertTrue(self.workspace.is_protected(self.root / ".ssh" / "config"))
        self.assertTrue(self.workspace.is_protected(self.root / "home" / ".ssh" / "id"))

    def test_ordinary_files_are_not_protected(self) -> None:
        for name in ("main.py", "README.md", "Consumer.sol", "config.toml"):
            self.assertFalse(self.workspace.is_protected(self.root / name), name)

    def test_secrets_are_omitted_from_listings(self) -> None:
        """Not merely unreadable — absent, so the model does not go looking."""
        names = [e["name"] for e in self.workspace.list_dir(".")]
        self.assertIn("README.md", names)
        self.assertNotIn(".env", names)

    def test_secrets_are_omitted_from_search(self) -> None:
        hits = self.workspace.search("supersecret")
        self.assertEqual(hits, [])

    def test_writing_over_a_secret_is_refused(self) -> None:
        with self.assertRaises(ProtectedPath):
            self.workspace.write_text(".env", "clobbered")
        self.assertIn("supersecret", (self.root / ".env").read_text(encoding="utf-8"))

    def test_the_tool_layer_refuses_rather_than_erroring(self) -> None:
        """A refusal must be flagged as one, so the model is told to stop."""
        call = self.run_tool("read_file", path=".env")
        self.assertFalse(call.ok)
        self.assertTrue(call.refused)
        self.assertIn("protected", call.error.lower())
        self.assertIn("disclosure", call.error)


class SizeAndTypeLimits(Sandboxed):
    def test_an_oversized_file_is_refused(self) -> None:
        (self.root / "big.txt").write_text("x" * 2000, encoding="utf-8")
        with self.assertRaises(WorkspaceError):
            self.workspace.read_text("big.txt", max_bytes=1000)

    def test_a_binary_file_is_refused(self) -> None:
        (self.root / "blob.bin").write_bytes(b"\x00\x01\x02" * 100)
        with self.assertRaises(WorkspaceError):
            self.workspace.read_text("blob.bin")

    def test_reading_a_directory_says_so(self) -> None:
        with self.assertRaises(WorkspaceError) as caught:
            self.workspace.read_text("src")
        self.assertIn("list_dir", str(caught.exception))


class FileOperations(Sandboxed):
    def test_write_creates_and_records(self) -> None:
        change = self.workspace.write_text("src/new.py", "x = 1\n")
        self.assertEqual(change.action, "created")
        self.assertEqual((self.root / "src" / "new.py").read_text(encoding="utf-8"), "x = 1\n")
        self.assertEqual(self.workspace.changes[-1].path, "src/new.py")

    def test_write_creates_parent_directories(self) -> None:
        self.workspace.write_text("a/b/c/deep.txt", "hi")
        self.assertTrue((self.root / "a" / "b" / "c" / "deep.txt").exists())

    def test_rewriting_records_a_modification(self) -> None:
        self.workspace.write_text("README.md", "# changed\n")
        self.assertEqual(self.workspace.changes[-1].action, "modified")

    def test_edit_replaces_exact_text(self) -> None:
        self.workspace.edit_text("src/main.py", "hello", "goodbye")
        self.assertIn("goodbye", (self.root / "src" / "main.py").read_text(encoding="utf-8"))

    def test_edit_refuses_an_ambiguous_match(self) -> None:
        """Otherwise the model changes whichever one came first and it looks successful."""
        self.workspace.write_text("dup.py", "timeout = 1\ntimeout = 2\n")
        with self.assertRaises(WorkspaceError) as caught:
            self.workspace.edit_text("dup.py", "timeout", "delay")
        self.assertIn("2 times", str(caught.exception))

    def test_edit_can_replace_every_occurrence_when_asked(self) -> None:
        self.workspace.write_text("dup.py", "a = 1\na = 2\n")
        self.workspace.edit_text("dup.py", "a =", "b =", count=0)
        self.assertNotIn("a =", (self.root / "dup.py").read_text(encoding="utf-8"))

    def test_edit_of_absent_text_is_an_error(self) -> None:
        with self.assertRaises(WorkspaceError):
            self.workspace.edit_text("src/main.py", "not present", "x")

    def test_delete_needs_recursive_for_a_populated_directory(self) -> None:
        with self.assertRaises(WorkspaceError):
            self.workspace.delete("src")
        self.assertTrue((self.root / "src").exists())
        self.workspace.delete("src", recursive=True)
        self.assertFalse((self.root / "src").exists())

    def test_the_root_itself_cannot_be_deleted(self) -> None:
        with self.assertRaises(WorkspaceError):
            self.workspace.delete(".", recursive=True)

    def test_move_and_copy(self) -> None:
        self.workspace.copy("README.md", "docs/README.md")
        self.assertTrue((self.root / "docs" / "README.md").exists())
        self.workspace.move("docs/README.md", "docs/INDEX.md")
        self.assertTrue((self.root / "docs" / "INDEX.md").exists())
        self.assertFalse((self.root / "docs" / "README.md").exists())

    def test_move_refuses_to_clobber_without_overwrite(self) -> None:
        with self.assertRaises(WorkspaceError):
            self.workspace.move("README.md", "src/main.py")

    def test_preview_produces_a_diff(self) -> None:
        diff = self.workspace.preview("src/main.py", "print('goodbye')\n")
        body = "\n".join(diff)
        self.assertIn("-print('hello')", body)
        self.assertIn("+print('goodbye')", body)

    def test_preview_of_a_new_file_is_all_additions(self) -> None:
        diff = self.workspace.preview("brand/new.txt", "one\ntwo\n")
        self.assertTrue(any(line.startswith("+one") for line in diff))

    def test_summary_counts_what_happened(self) -> None:
        self.workspace.write_text("a.txt", "a")
        self.workspace.write_text("b.txt", "b")
        self.workspace.delete("a.txt")
        summary = self.workspace.summary()
        self.assertEqual(summary["changes"], 3)
        self.assertIn("deleted", summary["by_action"])


class Searching(Sandboxed):
    def test_walk_finds_by_glob(self) -> None:
        self.assertIn("src/main.py", self.workspace.walk(".", "*.py"))

    def test_search_reports_file_and_line(self) -> None:
        hits = self.workspace.search("hello")
        self.assertEqual(hits[0]["path"], "src/main.py")
        self.assertEqual(hits[0]["line"], 1)

    def test_a_bad_regex_is_an_error_not_a_crash(self) -> None:
        with self.assertRaises(WorkspaceError):
            self.workspace.search("[unclosed")

    def test_tree_is_depth_limited(self) -> None:
        self.workspace.write_text("a/b/c/d/e.txt", "deep")
        shallow = "\n".join(self.workspace.tree(".", depth=2))
        self.assertNotIn("e.txt", shallow)


class Trust(unittest.TestCase):
    def _request(self, risk: Risk = Risk.WRITE, path: str = "src/a.py") -> Request:
        return Request(tool="write_file", risk=risk, path=path)

    def test_non_mutating_calls_never_prompt(self) -> None:
        policy = TrustPolicy()
        for risk in (Risk.READ, Risk.NETWORK):
            self.assertEqual(policy.preflight(self._request(risk)), Decision.ALLOW)

    def test_writes_prompt_by_default(self) -> None:
        self.assertIsNone(TrustPolicy().preflight(self._request()))

    def test_allow_writes_stops_prompting(self) -> None:
        self.assertEqual(TrustPolicy(allow_writes=True).preflight(self._request()),
                         Decision.ALLOW)

    def test_read_only_refuses_before_anything_else(self) -> None:
        """A read-only session must not be openable by a grant given for something else."""
        policy = TrustPolicy(read_only=True, allow_writes=True, allow_execute=True)
        self.assertEqual(policy.preflight(self._request()), Decision.DENY)
        self.assertEqual(policy.preflight(self._request(Risk.EXECUTE)), Decision.DENY)

    def test_execution_is_refused_unless_enabled(self) -> None:
        self.assertEqual(TrustPolicy(allow_writes=True).preflight(self._request(Risk.EXECUTE)),
                         Decision.DENY)
        self.assertIsNone(TrustPolicy(allow_execute=True).preflight(self._request(Risk.EXECUTE)))

    def test_a_sticky_grant_covers_the_same_directory(self) -> None:
        policy = TrustPolicy()
        first = self._request(path="src/a.py")
        policy.remember(first, Decision.ALLOW_ALWAYS)
        self.assertEqual(policy.preflight(self._request(path="src/b.py")), Decision.ALLOW)

    def test_a_sticky_grant_does_not_cover_another_directory(self) -> None:
        policy = TrustPolicy()
        policy.remember(self._request(path="src/a.py"), Decision.ALLOW_ALWAYS)
        self.assertIsNone(policy.preflight(self._request(path="config/b.py")))

    def test_a_sticky_denial_sticks(self) -> None:
        policy = TrustPolicy()
        policy.remember(self._request(), Decision.DENY_ALWAYS)
        self.assertEqual(policy.preflight(self._request()), Decision.DENY)

    def test_grants_can_be_revoked(self) -> None:
        policy = TrustPolicy()
        policy.remember(self._request(), Decision.ALLOW_ALWAYS)
        self.assertEqual(policy.revoke(), 1)
        self.assertIsNone(policy.preflight(self._request()))

    def test_explicit_rules_beat_the_standing_configuration(self) -> None:
        policy = TrustPolicy(allow_writes=True,
                             rules=[Rule(tool="write_file", decision=Decision.DENY,
                                         path="secrets/*")])
        self.assertEqual(policy.preflight(self._request(path="secrets/a.py")), Decision.DENY)
        self.assertEqual(policy.preflight(self._request(path="src/a.py")), Decision.ALLOW)

    def test_deny_approver_refuses_what_the_policy_left_open(self) -> None:
        policy = TrustPolicy()
        self.assertFalse(DenyApprover().decide(policy, self._request()).allowed)

    def test_auto_approver_allows_what_the_policy_left_open(self) -> None:
        policy = TrustPolicy()
        self.assertTrue(AutoApprover().decide(policy, self._request()).allowed)

    def test_an_approver_cannot_override_a_policy_refusal(self) -> None:
        """The prompt is only reached when the policy abstains, never to reverse a no."""
        policy = TrustPolicy(read_only=True)
        self.assertFalse(AutoApprover().decide(policy, self._request()).allowed)

    def test_the_prompt_is_asked_once_then_remembered(self) -> None:
        asked = []

        def callback(request: Request) -> Decision:
            asked.append(request.path)
            return Decision.ALLOW_ALWAYS

        policy = TrustPolicy()
        approver = CallbackApprover(callback)
        approver.decide(policy, self._request(path="src/a.py"))
        approver.decide(policy, self._request(path="src/b.py"))
        self.assertEqual(len(asked), 1, "a session grant should stop the second prompt")

    def test_decisions_are_logged(self) -> None:
        policy = TrustPolicy()
        AutoApprover().decide(policy, self._request())
        self.assertEqual(len(policy.log), 1)

    def test_from_env_reads_the_three_knobs(self) -> None:
        saved = {k: os.environ.get(k) for k in
                 ("ALCHEM_READ_ONLY", "ALCHEM_ALLOW_WRITES", "ALCHEM_ALLOW_EXEC")}
        try:
            os.environ["ALCHEM_ALLOW_WRITES"] = "1"
            os.environ.pop("ALCHEM_READ_ONLY", None)
            os.environ.pop("ALCHEM_ALLOW_EXEC", None)
            policy = TrustPolicy.from_env()
            self.assertTrue(policy.allow_writes)
            self.assertFalse(policy.allow_execute)
        finally:
            for key, value in saved.items():
                if value is None:
                    os.environ.pop(key, None)
                else:
                    os.environ[key] = value


class ToolDispatch(Sandboxed):
    def test_every_tool_declares_a_risk_and_a_schema(self) -> None:
        for name, tool in TOOLS.items():
            with self.subTest(name):
                self.assertIsInstance(tool.risk, Risk)
                self.assertTrue(tool.description.strip())
                self.assertEqual(tool.schema["function"]["name"], name)

    def test_required_arguments_are_declared_parameters(self) -> None:
        for name, tool in TOOLS.items():
            for argument in tool.required:
                with self.subTest(f"{name}.{argument}"):
                    self.assertIn(argument, tool.parameters)

    def test_an_unknown_tool_is_an_error_not_a_crash(self) -> None:
        call = self.run_tool("no_such_tool")
        self.assertFalse(call.ok)
        self.assertIn("unknown tool", call.error)

    def test_bad_arguments_come_back_as_text(self) -> None:
        call = self.run_tool("read_file", nonsense=1)
        self.assertFalse(call.ok)
        self.assertIn("bad arguments", call.error)

    def test_a_refused_write_does_not_touch_the_disk(self) -> None:
        context = self.context(approver=DenyApprover())
        call = self.run_tool("write_file", context, path="src/nope.py", content="x")
        self.assertTrue(call.refused)
        self.assertFalse((self.root / "src" / "nope.py").exists())

    def test_a_refusal_tells_the_model_not_to_retry(self) -> None:
        context = self.context(approver=DenyApprover())
        call = self.run_tool("write_file", context, path="a.py", content="x")
        self.assertIn("Do not retry", call.error)

    def test_a_policy_refusal_does_not_claim_the_user_declined(self) -> None:
        """Reporting a prompt that never happened makes the assistant argue with people."""
        context = self.context(policy=TrustPolicy.read_only_policy())
        call = self.run_tool("write_file", context, path="a.py", content="x")
        self.assertTrue(call.refused)
        self.assertIn("read-only", call.error)
        self.assertNotIn("declined", call.error)

    def test_execution_refusal_names_the_flag_that_enables_it(self) -> None:
        call = self.run_tool("run_command", command="echo hi")
        self.assertTrue(call.refused)
        self.assertIn("--allow-exec", call.error)

    def test_writes_are_tagged_as_mutating(self) -> None:
        call = self.run_tool("write_file", path="a.py", content="x")
        self.assertTrue(call.ok)
        self.assertTrue(call.mutating)
        self.assertEqual(call.path, "a.py")

    def test_reads_are_not_tagged_as_mutating(self) -> None:
        self.assertFalse(self.run_tool("read_file", path="README.md").mutating)

    def test_read_file_returns_numbered_lines(self) -> None:
        call = self.run_tool("read_file", path="src/main.py")
        self.assertIn("1  print('hello')", call.result["content"])

    def test_workspace_info_reports_the_posture(self) -> None:
        call = self.run_tool("workspace_info")
        self.assertEqual(call.result["trust"]["execute"], "refused")
        self.assertIn("root", call.result)

    def test_tools_are_not_advertised_when_they_cannot_run(self) -> None:
        offered = {s["function"]["name"] for s in tools_for(TrustPolicy.read_only_policy())}
        self.assertIn("read_file", offered)
        self.assertNotIn("write_file", offered)
        self.assertNotIn("run_command", offered)

    def test_execution_is_advertised_once_enabled(self) -> None:
        offered = {s["function"]["name"] for s in tools_for(TrustPolicy(allow_execute=True))}
        self.assertIn("run_command", offered)


class Execution(Sandboxed):
    def _enabled(self) -> ToolContext:
        return self.context(policy=TrustPolicy(allow_execute=True))

    def test_a_command_runs_in_the_workspace(self) -> None:
        call = self.run_tool("run_command", self._enabled(),
                             command=f'"{sys.executable}" -c "import os;print(os.getcwd())"')
        self.assertTrue(call.ok, call.error)
        self.assertEqual(call.result["exit_code"], 0)
        self.assertIn(str(self.root), call.result["stdout"])

    def test_a_failing_command_reports_its_exit_code(self) -> None:
        call = self.run_tool("run_command", self._enabled(),
                             command=f'"{sys.executable}" -c "raise SystemExit(3)"')
        self.assertTrue(call.ok, "a non-zero exit is a result, not a tool failure")
        self.assertEqual(call.result["exit_code"], 3)

    def test_shell_metacharacters_are_not_interpreted(self) -> None:
        """No shell means no injection through a second layer of parsing."""
        call = self.run_tool(
            "run_command", self._enabled(),
            command=f'"{sys.executable}" -c "print(\'a\')" ; echo pwned',
        )
        # Either the extra tokens are passed as arguments or the call fails — what must
        # not happen is a second command running.
        if call.ok:
            self.assertNotIn("pwned", call.result["stdout"])

    def test_a_missing_binary_is_a_clean_error(self) -> None:
        call = self.run_tool("run_command", self._enabled(),
                             command="definitely-not-a-real-binary-xyz")
        self.assertFalse(call.ok)
        self.assertIn("not found", call.error)

    def test_an_empty_command_is_rejected(self) -> None:
        self.assertFalse(self.run_tool("run_command", self._enabled(), command="   ").ok)


class Codegen(Sandboxed):
    def test_generate_consumer_writes_a_contract(self) -> None:
        call = self.run_tool("generate_consumer", pair="ETH/USD", network="base",
                             path="src/EthUsd.sol")
        self.assertTrue(call.ok, call.error)
        body = (self.root / "src" / "EthUsd.sol").read_text(encoding="utf-8")
        self.assertIn("pragma solidity", body)

    def test_the_generated_contract_carries_the_measured_heartbeat(self) -> None:
        """The reason this is a tool rather than something the model writes.

        Base's ETH/USD heartbeat is 1200s. A model writing this from memory emits 3600.
        """
        call = self.run_tool("generate_consumer", pair="ETH/USD", network="base")
        self.assertIn("1200", call.result["code"])

    def test_generate_consumer_without_a_path_writes_nothing(self) -> None:
        call = self.run_tool("generate_consumer", pair="ETH/USD", network="base")
        self.assertIn("code", call.result)
        self.assertEqual(self.workspace.changes, [])

    def test_generate_project_is_confined_to_the_workspace(self) -> None:
        """The generator writes through Path directly, so `out` must be resolved first."""
        with self.assertRaises(PathEscape):
            self.workspace.resolve("../escape-project")
        call = self.run_tool("generate_project", pair="ETH/USD", network="base",
                             out="../escape-project")
        self.assertFalse(call.ok)

    def test_generate_project_scaffolds_into_the_workspace(self) -> None:
        call = self.run_tool("generate_project", pair="ETH/USD", network="base", out="proj")
        self.assertTrue(call.ok, call.error)
        self.assertGreater(call.result["count"], 1)
        self.assertTrue((self.root / "proj").is_dir())


class Export(Sandboxed):
    def test_an_offline_dataset_exports_to_a_file(self) -> None:
        call = self.run_tool("export_data", dataset="coverage", path="out/coverage.csv",
                             fmt="csv")
        self.assertTrue(call.ok, call.error)
        body = (self.root / "out" / "coverage.csv").read_text(encoding="utf-8")
        self.assertTrue(body.startswith("network,"))

    def test_an_unknown_dataset_is_an_error(self) -> None:
        call = self.run_tool("export_data", dataset="nonsense", path="x.csv")
        self.assertFalse(call.ok)
        self.assertIn("unknown dataset", call.error)

    def test_export_respects_the_sandbox(self) -> None:
        call = self.run_tool("export_data", dataset="coverage", path="../escape.csv")
        self.assertFalse(call.ok)


if __name__ == "__main__":
    unittest.main()
