"""Trust: deciding which tool calls run, and who decides.

:mod:`alchem_link.workspace` answers *where* a tool may act. This module answers
*whether* it acts at all. The two are separate on purpose — a path can be perfectly
legal and the operation still be one the user wants to see first.

The model is deliberately small, because a permission system nobody understands is one
people disable. Four risk levels, three decisions, and one policy object.

**Risk is a property of the tool, not of the moment.** Every tool declares its risk once,
in its definition, so a new tool cannot arrive without a classification. Reading a chain
is not the same act as writing a file, which is not the same act as running a command,
and collapsing them into "dangerous / not dangerous" is what produces prompts people
click through.

**Default-deny for anything non-interactive.** When there is nobody at the terminal — a
piped `alchem-link chat`, a CI job — a prompt cannot be answered, and treating silence as
consent is how an agent quietly rewrites a repository at three in the morning.
:class:`DenyApprover` is the default in that case, and the caller has to say ``--yes``
explicitly to get anything else.

**Execution is off unless switched on.** Shell access is the one capability where a
mistake is unbounded, so it is not merely "ask first" — it is refused outright until the
session opts in, and even then each command is prompted individually unless the user
grants otherwise.

Grants are session-scoped and never persisted. A tool that remembers "always allow" to
disk turns one distracted keystroke into a permanent standing authorisation, which is
exactly the property you do not want the mechanism to have.
"""
from __future__ import annotations

import fnmatch
import os
import sys
from dataclasses import dataclass, field
from enum import Enum
from typing import Callable, Dict, List, Optional, Sequence


class Risk(str, Enum):
    """What kind of act a tool performs. Ordered from least to most consequential."""

    #: Reads local files or directory structure. Cheap, but the result goes to a
    #: third-party model, so it is not free of consequence.
    READ = "read"
    #: Reads a blockchain or an HTTP endpoint. No local effect.
    NETWORK = "network"
    #: Creates, modifies, moves or deletes files inside the workspace.
    WRITE = "write"
    #: Runs an arbitrary command. Unbounded.
    EXECUTE = "execute"

    @property
    def rank(self) -> int:
        return {"read": 0, "network": 0, "write": 2, "execute": 3}[self.value]

    @property
    def mutating(self) -> bool:
        return self in (Risk.WRITE, Risk.EXECUTE)


class Decision(str, Enum):
    ALLOW = "allow"
    DENY = "deny"
    #: Allow this call and every later call matching the same rule, for this session.
    ALLOW_ALWAYS = "allow_always"
    #: Deny this call and every later call matching the same rule, for this session.
    DENY_ALWAYS = "deny_always"

    @property
    def allowed(self) -> bool:
        return self in (Decision.ALLOW, Decision.ALLOW_ALWAYS)

    @property
    def sticky(self) -> bool:
        return self in (Decision.ALLOW_ALWAYS, Decision.DENY_ALWAYS)


@dataclass
class Request:
    """One tool call awaiting a decision, with everything needed to judge it."""

    tool: str
    risk: Risk
    arguments: Dict[str, object] = field(default_factory=dict)
    #: The workspace-relative path this call would touch, when there is one. The grant
    #: key is built from it, so "always allow writes to docs/" is expressible.
    path: str = ""
    #: One line describing the effect, shown in the prompt.
    summary: str = ""
    #: A unified diff or command preview, shown when the user asks to see it.
    preview: Sequence[str] = ()

    @property
    def grant_key(self) -> str:
        """The key a sticky decision is remembered under.

        Tool plus the *directory* of the path, not the file, so approving one write into
        a directory covers the rest of that directory rather than prompting per file —
        which is the behaviour that makes a session usable — while still not covering the
        whole workspace.
        """
        if not self.path:
            return self.tool
        parent = self.path.rsplit("/", 1)[0] if "/" in self.path else "."
        return f"{self.tool}:{parent}"

    def describe(self) -> str:
        return self.summary or f"{self.tool}({self.arguments})"


@dataclass
class Rule:
    """A standing decision, matched by tool glob and optional path glob."""

    tool: str
    decision: Decision
    path: str = "*"
    reason: str = ""

    def matches(self, request: Request) -> bool:
        if not fnmatch.fnmatch(request.tool, self.tool):
            return False
        if self.path in ("*", ""):
            return True
        return fnmatch.fnmatch(request.path or "", self.path)


class TrustPolicy:
    """What is allowed without asking, what must be asked, and what is refused.

    The constructor arguments are the three knobs a user actually turns:
    ``read_only`` refuses every mutation, ``allow_writes`` stops prompting for them, and
    ``allow_execute`` is what turns shell access on at all.
    """

    def __init__(self, read_only: bool = False, allow_writes: bool = False,
                 allow_execute: bool = False, rules: Optional[Sequence[Rule]] = None) -> None:
        self.read_only = read_only
        self.allow_writes = allow_writes
        self.allow_execute = allow_execute
        self.rules: List[Rule] = list(rules or [])
        #: Sticky decisions granted during this session, by :attr:`Request.grant_key`.
        self.session_grants: Dict[str, Decision] = {}
        #: Every decision made, for `:trust` and for the session summary.
        self.log: List[tuple] = []

    # ── construction ─────────────────────────────────────────────────────────

    @classmethod
    def from_env(cls) -> "TrustPolicy":
        """Read the three knobs from the environment, for non-interactive use."""
        def flag(name: str) -> bool:
            return os.environ.get(name, "").strip().lower() in ("1", "true", "yes", "on")

        return cls(
            read_only=flag("ALCHEM_READ_ONLY"),
            allow_writes=flag("ALCHEM_ALLOW_WRITES"),
            allow_execute=flag("ALCHEM_ALLOW_EXEC"),
        )

    @classmethod
    def read_only_policy(cls) -> "TrustPolicy":
        return cls(read_only=True)

    @classmethod
    def trusted(cls) -> "TrustPolicy":
        """Everything except execution. What ``--yes`` gives you."""
        return cls(allow_writes=True, allow_execute=False)

    # ── decisions ────────────────────────────────────────────────────────────

    def preflight(self, request: Request) -> Optional[Decision]:
        """A decision that needs no prompt, or ``None`` meaning "ask".

        Order matters and is: hard refusals, then explicit rules, then session grants,
        then the standing configuration. A refusal must not be overridable by a grant the
        user gave for something else.
        """
        if self.read_only and request.risk.mutating:
            return Decision.DENY
        if request.risk is Risk.EXECUTE and not self.allow_execute:
            return Decision.DENY

        for rule in self.rules:
            if rule.matches(request):
                return rule.decision

        granted = self.session_grants.get(request.grant_key)
        if granted is not None:
            return granted

        if not request.risk.mutating:
            return Decision.ALLOW
        if request.risk is Risk.WRITE and self.allow_writes:
            return Decision.ALLOW
        return None

    def remember(self, request: Request, decision: Decision) -> None:
        """Record a sticky decision for the rest of the session."""
        if decision.sticky:
            self.session_grants[request.grant_key] = (
                Decision.ALLOW if decision is Decision.ALLOW_ALWAYS else Decision.DENY
            )

    def record(self, request: Request, decision: Decision) -> None:
        self.log.append((request.tool, request.risk.value, request.path, decision.value))

    def describe(self) -> Dict[str, object]:
        """The current posture, for the `:trust` command."""
        return {
            "read_only": self.read_only,
            "writes": "allowed" if self.allow_writes else "prompt",
            "execute": "allowed" if self.allow_execute else "refused",
            "session_grants": {k: v.value for k, v in self.session_grants.items()},
            "rules": [
                {"tool": r.tool, "path": r.path, "decision": r.decision.value}
                for r in self.rules
            ],
            "decisions_made": len(self.log),
        }

    def revoke(self, key: str = "") -> int:
        """Drop session grants — one, or all of them. Returns how many went."""
        if not key:
            count = len(self.session_grants)
            self.session_grants.clear()
            return count
        matched = [k for k in self.session_grants if fnmatch.fnmatch(k, key)]
        for k in matched:
            del self.session_grants[k]
        return len(matched)


# ── approvers ────────────────────────────────────────────────────────────────


class Approver:
    """Answers the question a policy could not. Subclasses supply :meth:`prompt`."""

    def prompt(self, request: Request) -> Decision:  # pragma: no cover - abstract
        raise NotImplementedError

    def decide(self, policy: TrustPolicy, request: Request) -> Decision:
        """The full path: policy first, prompt only when the policy abstains."""
        decision = policy.preflight(request)
        if decision is None:
            decision = self.prompt(request)
            policy.remember(request, decision)
        policy.record(request, decision)
        return decision


class AutoApprover(Approver):
    """Approves anything the policy did not already refuse. For ``--yes`` and tests."""

    def prompt(self, request: Request) -> Decision:
        return Decision.ALLOW


class DenyApprover(Approver):
    """Refuses anything the policy did not already allow.

    The default whenever there is no terminal. An agent that cannot ask must not assume.
    """

    def prompt(self, request: Request) -> Decision:
        return Decision.DENY


class CallbackApprover(Approver):
    """Delegates to a callable — how the shell installs its own prompt."""

    def __init__(self, callback: Callable[[Request], Decision]) -> None:
        self.callback = callback

    def prompt(self, request: Request) -> Decision:
        return self.callback(request)


def default_approver(interactive: Optional[bool] = None) -> Approver:
    """Pick an approver from the environment.

    ``interactive=None`` means "work it out": a real terminal on stdin gets a prompt,
    anything else gets a refusal. That default is the whole safety story for piped and
    scripted use, so it errs toward the boring answer.
    """
    if interactive is None:
        try:
            interactive = bool(sys.stdin.isatty() and sys.stdout.isatty())
        except (AttributeError, ValueError):  # pragma: no cover
            interactive = False
    return TerminalApprover() if interactive else DenyApprover()


class TerminalApprover(Approver):
    """Prompts at the terminal, in the product palette.

    Kept here rather than in the shell so that `alchem-link chat` — which is not the REPL
    — gets the same prompt, and so there is one place where the wording of a consent
    question lives.
    """

    #: What each key does. `v` is not a decision; it prints the preview and re-asks.
    KEYS = {
        "y": Decision.ALLOW,
        "a": Decision.ALLOW_ALWAYS,
        "n": Decision.DENY,
        "d": Decision.DENY_ALWAYS,
    }

    def __init__(self, stream=None) -> None:
        self.stream = stream

    def prompt(self, request: Request) -> Decision:
        from .render import Console
        from .theme import role

        console = Console(self.stream or sys.stderr)
        tone = "bad" if request.risk is Risk.EXECUTE else "warn"

        console.blank()
        console.write("  " + console.paint(f" {request.risk.value.upper()} ", role(tone))
                      + " " + console.paint(request.tool, role("key"))
                      + ("  " + console.paint(request.path, role("accent"))
                         if request.path else ""))
        if request.summary:
            console.note("  " + request.summary)

        while True:
            console.write("  " + console.paint(
                "[y] once   [a] always here   [n] no   [d] never here"
                + ("   [v] view" if request.preview else ""),
                role("hint"),
            ))
            try:
                answer = input("  approve? ").strip().lower()
            except (EOFError, KeyboardInterrupt):
                # A refused prompt is a refusal, not a crash and not an approval.
                console.blank()
                return Decision.DENY
            if answer in ("v", "view") and request.preview:
                console.blank()
                for line in list(request.preview)[:80]:
                    console.write("    " + console.paint(line, _diff_style(line)))
                console.blank()
                continue
            decision = self.KEYS.get(answer[:1] if answer else "n")
            if decision is not None:
                return decision
            console.warn("  answer y, a, n or d")


def _diff_style(line: str):
    """Colour a unified-diff line by its marker."""
    from .theme import role

    if line.startswith("+++") or line.startswith("---") or line.startswith("@@"):
        return role("accent")
    if line.startswith("+"):
        return role("ok")
    if line.startswith("-"):
        return role("bad")
    return role("muted")


__all__ = [
    "Risk",
    "Decision",
    "Request",
    "Rule",
    "TrustPolicy",
    "Approver",
    "AutoApprover",
    "DenyApprover",
    "CallbackApprover",
    "TerminalApprover",
    "default_approver",
]
