"""An interactive console: commands and chat in one prompt.

Two things you normally need two windows for. Type a command and it runs exactly as it
would from the shell; type a question and the agent answers it by reading chains. The
mode is inferred rather than declared — a line starting with a known command is a
command, anything else is a question — so you never have to remember which mode you are
in. `:cmd` and `:chat` pin it when the inference is wrong.

Design notes worth stating:

* **One implementation of every command.** The REPL dispatches through the same parser
  and handlers as the CLI (:func:`alchem_link.cli.run_argv`). A shell that reimplements
  its commands drifts from them within a release.
* **Commands work with no LLM.** Chat needs a provider; nothing else does. Without one,
  the console still runs every command and says once what chat would need.
* **Ctrl-C cancels the line, not the session.** A long divergence sweep should be
  interruptible without losing your history and network context.
* **Tool calls are printed.** When the agent answers, the reads it made are shown above
  the answer, so any number can be re-derived with a command you can type yourself.

`readline` is used when present — history, editing, and completion over commands,
network keys and pair names. It is absent on some Windows Pythons, which costs editing
niceties and nothing else.
"""
from __future__ import annotations

import os
import shlex
import sys
from pathlib import Path
from typing import List, Optional

from . import __version__
from .feeds import FEEDS, feed_count, list_feeds
from .llm import PROVIDER_ENV, NoProviderConfigured, available_providers
from .networks import DEFAULT_NETWORK, NETWORKS, get_network
from .approvals import (
    CallbackApprover,
    Decision,
    Request,
    Risk,
    TrustPolicy,
)
from .registry import all_assets
from .render import Console, console
from .term import ansi, boot
from .theme import role
from .workspace import Workspace, WorkspaceError, default_workspace

try:  # pragma: no cover - platform dependent
    import readline
except ImportError:  # pragma: no cover
    readline = None  # type: ignore

HISTORY_FILE = Path.home() / ".alchem_link_history"
HISTORY_LIMIT = 1000

BANNER = r"""
   __ _ | | ___| |__   ___ _ __ ___    | (_)_ __ | | __
  / _` || |/ __| '_ \ / _ \ '_ ` _ \   | | | '_ \| |/ /
 | (_| || | (__| | | |  __/ | | | | |  | | | | | |   <
  \__,_||_|\___|_| |_|\___|_| |_| |_|  |_|_|_| |_|_|\_\
"""

META_HELP = """
  :help              this text
  :commands          every command, with one-line help
  :net <network>     change the default network for bare commands and chat
  :chat              pin chat mode — every line goes to the agent
  :cmd               pin command mode — every line is a command
  :auto              infer per line (default)
  :ui                open the full-screen dashboard, then come back here
  :theme             the palette, and what this terminal negotiated

  :workspace [dir]   show or move the directory the agent may write in
  :cd <dir>          alias for :workspace
  :trust             what the agent is allowed to do without asking
  :trust write|exec|readonly|revoke   change it for this session
  :changes           every file the agent has written this session
  :diff <path>       what the agent changed in one file
  :providers         LLM providers and which are usable
  :tools             what the agent is allowed to call
  :reset             clear the chat history, keep the session
  :quit / :q         exit (Ctrl-D also works)

  Anything starting with `?` is always a question.
  Anything starting with `!` is always a command.
"""


class Palette:
    """Colour helpers for the REPL, sourced from :mod:`alchem_link.theme`.

    This used to hold its own table of xterm-256 indices, which meant the console and the
    dashboard could — and did — drift apart. It now names the same semantic roles
    everything else does and lets :class:`~alchem_link.render.Console` encode them for
    whatever depth the terminal admits to, so a palette change moves the whole product.
    """

    def __init__(self, enabled: Optional[bool] = None, stream=None) -> None:
        self.console = Console(stream)
        if enabled is False:
            self.console.depth = ansi.Depth.NONE
        self.enabled = self.console.colour

    def _role(self, name: str, text: str) -> str:
        return self.console.paint(text, role(name))

    def blue(self, text: str) -> str:
        return self._role("accent", text)

    def dim(self, text: str) -> str:
        return self._role("hint", text)

    def green(self, text: str) -> str:
        return self._role("ok", text)

    def amber(self, text: str) -> str:
        return self._role("warn", text)

    def red(self, text: str) -> str:
        return self._role("bad", text)

    def bold(self, text: str) -> str:
        return self._role("title", text)


class Completer:
    """Tab completion over commands, network keys and pair names."""

    def __init__(self, commands: List[str]) -> None:
        self.commands = sorted(commands)
        self.networks = sorted(NETWORKS)
        self.pairs = sorted({pair for table in FEEDS.values() for pair in table})
        self.assets = all_assets()
        self.meta = [
            ":help", ":commands", ":net", ":chat", ":cmd", ":auto", ":ui", ":theme",
            ":providers", ":tools", ":reset", ":quit", ":q",
            ":workspace", ":cd", ":trust", ":changes", ":diff",
        ]
        self._matches: List[str] = []

    def _candidates(self, line: str, word: str) -> List[str]:
        parts = line.split()
        # `-n <TAB>` and `--network <TAB>` want network keys, not command names.
        if parts and parts[-1] in ("-n", "--network"):
            return self.networks
        if len(parts) >= 2 and parts[-2] in ("-n", "--network"):
            return self.networks
        if word.startswith(":"):
            return self.meta
        # First word is the command; later words are usually a pair.
        if not line.strip() or (len(parts) == 1 and not line.endswith(" ")):
            return self.commands + self.meta
        return self.pairs + self.networks + self.assets

    def complete(self, text: str, state: int):  # pragma: no cover - readline callback
        if state == 0:
            line = readline.get_line_buffer()[: readline.get_endidx()] if readline else text
            pool = self._candidates(line, text)
            upper = text.upper()
            self._matches = [
                c for c in pool
                if c.startswith(text) or (c.isupper() and c.startswith(upper))
            ]
        try:
            return self._matches[state]
        except IndexError:
            return None


class Shell:
    """The REPL."""

    def __init__(self, network: str = DEFAULT_NETWORK, mode: str = "auto",
                 colour: Optional[bool] = None, workspace: Optional[str] = None,
                 policy: Optional[TrustPolicy] = None) -> None:
        from .cli import command_names  # imported here to avoid a circular import

        self.network = get_network(network).key
        self.mode = mode
        self.palette = Palette(colour)
        self.commands = command_names()
        self.workspace: Workspace = default_workspace(workspace)
        self.policy = policy or TrustPolicy.from_env()
        self._agent = None
        self._agent_error = ""

    # ── agent ────────────────────────────────────────────────────────────────

    def _ensure_agent(self):
        """Build the agent lazily — no provider probe until chat is actually used."""
        if self._agent is not None or self._agent_error:
            return self._agent
        try:
            from .agent import build_agent

            self._agent = build_agent(
                network=self.network,
                policy=self.policy,
                # The REPL is the one place there is definitely somebody to ask, so it
                # installs its own prompt rather than taking the default, which refuses
                # whenever it cannot see a terminal.
                approver=CallbackApprover(self.approve),
            )
            self._agent.workspace = self.workspace
        except NoProviderConfigured as exc:
            self._agent_error = str(exc)
        except Exception as exc:
            self._agent_error = f"could not start the agent: {exc}"
        return self._agent

    # ── approval ─────────────────────────────────────────────────────────────

    def approve(self, request: Request) -> Decision:
        """Ask the user about one tool call. Called from inside a chat turn.

        The prompt is written to be answerable: it leads with the verb and the path, not
        the tool name, and `v` shows the actual diff. Approving a write you have not seen
        is a keystroke rather than consent, and the edits where that matters most are the
        ones that look routine.
        """
        tone = "bad" if request.risk is Risk.EXECUTE else "warn"
        print()
        print("  " + self.palette._role(tone, f" {request.risk.value.upper()} ")
              + " " + self.palette.blue(request.tool)
              + (("  " + self.palette._role("accent", request.path)) if request.path else ""))
        if request.summary:
            print("  " + self.palette.dim(request.summary))

        options = "[y] once  [a] always here  [n] no  [d] never here"
        if request.preview:
            options += "  [v] view"

        while True:
            print("  " + self.palette.dim(options))
            try:
                answer = input("  approve? ").strip().lower()[:1]
            except (EOFError, KeyboardInterrupt):
                # An interrupted prompt is a refusal. Treating it as approval would make
                # Ctrl-C the most dangerous key in the session.
                print()
                return Decision.DENY
            if answer == "v" and request.preview:
                print()
                for line in list(request.preview)[:80]:
                    print("    " + self._diff_line(line))
                print()
                continue
            decision = {
                "y": Decision.ALLOW, "a": Decision.ALLOW_ALWAYS,
                "n": Decision.DENY, "d": Decision.DENY_ALWAYS,
            }.get(answer)
            if decision is not None:
                return decision
            print("  " + self.palette.amber("answer y, a, n or d"))

    def _diff_line(self, line: str) -> str:
        if line.startswith(("+++", "---", "@@")):
            return self.palette.blue(line)
        if line.startswith("+"):
            return self.palette.green(line)
        if line.startswith("-"):
            return self.palette.red(line)
        return self.palette.dim(line)

    # ── prompt plumbing ──────────────────────────────────────────────────────

    def _prompt(self) -> str:
        marker = {"chat": "?", "cmd": "$", "auto": "›"}[self.mode]
        return f"{self.palette.blue('alchem')}{self.palette.dim(':' + self.network)} {marker} "

    def _load_history(self) -> None:  # pragma: no cover - readline only
        if readline is None:
            return
        try:
            readline.read_history_file(HISTORY_FILE)
        except (FileNotFoundError, OSError):
            pass
        readline.set_history_length(HISTORY_LIMIT)
        readline.set_completer(Completer(self.commands).complete)
        readline.set_completer_delims(" \t\n")
        readline.parse_and_bind("tab: complete")

    def _save_history(self) -> None:  # pragma: no cover - readline only
        if readline is None:
            return
        try:
            readline.write_history_file(HISTORY_FILE)
        except OSError:
            pass

    # ── classification ───────────────────────────────────────────────────────

    def classify(self, line: str) -> str:
        """``meta``, ``command``, ``chat`` or ``empty`` for one input line.

        Explicit prefixes win, then the pinned mode, then inference: a line whose first
        word is a known command is a command. That last rule is why `price ETH/USD`
        and `is the price stale?` both do the obvious thing without a mode switch.
        """
        text = line.strip()
        if not text:
            return "empty"
        if text.startswith(":"):
            return "meta"
        if text.startswith("?"):
            return "chat"
        if text.startswith("!"):
            return "command"
        if self.mode == "chat":
            return "chat"
        if self.mode == "cmd":
            return "command"
        first = text.split()[0].lower()
        return "command" if first in self.commands else "chat"

    # ── handlers ─────────────────────────────────────────────────────────────

    def run_command(self, text: str) -> int:
        from .cli import run_argv

        try:
            argv = shlex.split(text)
        except ValueError as exc:
            print(self.palette.red(f"could not parse: {exc}"))
            return 2
        if not argv:
            return 0

        # A bare command inherits the session network, but an explicit -n still wins.
        if "-n" not in argv and "--network" not in argv:
            argv += ["-n", self.network]
        try:
            return run_argv(argv)
        except SystemExit as exc:  # argparse exits on bad usage; the REPL must not
            return int(exc.code or 0)
        except KeyboardInterrupt:
            print(self.palette.dim("\ncancelled"))
            return 130

    def run_chat(self, text: str) -> int:
        agent = self._ensure_agent()
        if agent is None:
            print(self.palette.amber(self._agent_error))
            return 2

        agent.network = self.network

        def show(call) -> None:
            """One line per tool call, marked by what kind of act it was.

            A chain read and a file write are not the same event and should not look the
            same in the log. The whole point of printing these is that the user can see
            what the agent did without reading the reply for confessions.
            """
            if call.refused:
                mark, tone = self.palette.amber("○"), self.palette.amber
            elif not call.ok:
                mark, tone = self.palette.red("×"), self.palette.red
            elif call.mutating:
                mark, tone = self.palette.green("✎"), self.palette.green
            else:
                mark, tone = self.palette.dim("·"), self.palette.dim
            line = f"  {mark} {tone(call.summary)}"
            if not call.ok:
                line += self.palette.dim(f"  {call.error[:80]}")
            print(line)

        try:
            turn = agent.ask(text, on_tool=show)
        except KeyboardInterrupt:
            print(self.palette.dim("\ncancelled"))
            return 130

        if turn.error and not turn.tool_calls:
            print(self.palette.red(turn.reply))
            return 1
        print()
        for line in turn.reply.splitlines():
            print(f"  {line}")

        # Restate what actually changed on disk. The model is asked to mention it too,
        # but a written file is a fact and should not depend on the model remembering.
        if turn.changed_paths:
            print()
            print("  " + self.palette.green("changed: ")
                  + self.palette.dim(", ".join(turn.changed_paths)))
        print()
        return 0

    def run_meta(self, text: str) -> int:
        parts = text[1:].split()
        if not parts:
            return 0
        verb, args = parts[0].lower(), parts[1:]

        if verb in ("quit", "q", "exit"):
            raise EOFError
        if verb == "help":
            print(META_HELP)
            return 0
        if verb == "commands":
            self._print_commands()
            return 0
        if verb == "net":
            if not args:
                print(f"  network is {self.network}")
                return 0
            try:
                self.network = get_network(args[0]).key
            except KeyError as exc:
                print(self.palette.red(str(exc)))
                return 2
            feeds = len(list_feeds(self.network))
            print(f"  network → {self.palette.blue(self.network)} ({feeds} feeds)")
            return 0
        if verb in ("chat", "cmd", "auto"):
            self.mode = verb
            print(f"  mode → {verb}")
            return 0
        if verb == "ui":
            # The dashboard takes the alternate screen and gives it back; the REPL's
            # scrollback is untouched, so this really is "look at the board, come back".
            from .dashboard import Dashboard

            Dashboard(network=self.network).run()
            boot.initialize(title=boot.banner_title(__version__))
            return 0
        if verb == "theme":
            return self.run_command("theme")
        if verb in ("workspace", "cd"):
            return self._meta_workspace(args)
        if verb == "trust":
            return self._meta_trust(args)
        if verb == "changes":
            return self._meta_changes()
        if verb == "diff":
            return self._meta_diff(args)
        if verb == "providers":
            self._print_providers()
            return 0
        if verb == "tools":
            self._print_tools()
            return 0
        if verb == "reset":
            if self._agent is not None:
                self._agent.reset()
            print("  chat history cleared")
            return 0

        print(self.palette.red(f"unknown: :{verb}   (try :help)"))
        return 2

    # ── informational output ─────────────────────────────────────────────────

    def _print_commands(self) -> None:
        from .cli import LIVE_COMMANDS, OFFLINE_COMMANDS, REFERENCE_COMMANDS

        for title, group in (("live", LIVE_COMMANDS), ("offline", OFFLINE_COMMANDS),
                             ("reference", REFERENCE_COMMANDS)):
            print(f"\n  {self.palette.bold(title)}")
            for name, help_text in group:
                print(f"    {self.palette.blue(name):<22} {self.palette.dim(help_text)}")
        print()

    def _meta_workspace(self, args: List[str]) -> int:
        """Show the workspace, or move it. Moving invalidates nothing but the root."""
        if not args:
            summary = self.workspace.summary()
            print(f"  workspace {self.palette.blue(str(self.workspace.root))}")
            print(f"  {self.palette.dim(str(summary['changes']) + ' change(s) this session')}")
            return 0
        try:
            self.workspace = Workspace.at(args[0])
        except WorkspaceError as exc:
            print("  " + self.palette.red(str(exc)))
            return 2
        if self._agent is not None:
            self._agent.workspace = self.workspace
        print(f"  workspace → {self.palette.blue(str(self.workspace.root))}")
        return 0

    def _meta_trust(self, args: List[str]) -> int:
        """Show or change what the agent may do without asking.

        Changes are session-scoped and never written to disk. A permission that survives
        the process turns one distracted keystroke into a standing authorisation.
        """
        if not args:
            posture = self.policy.describe()
            print()
            for name, value in posture.items():
                if name in ("session_grants", "rules"):
                    continue
                print(f"  {self.palette.blue(name.replace('_', ' ')):<28} {value}")
            grants = posture["session_grants"]
            if grants:
                print(f"\n  {self.palette.bold('granted this session')}")
                for key, decision in grants.items():
                    tone = self.palette.green if decision == "allow" else self.palette.red
                    print(f"    {tone(decision):<8} {key}")
            print(f"\n  {self.palette.dim(':trust write | exec | readonly | prompt | revoke')}\n")
            return 0

        verb = args[0].lower()
        if verb == "write":
            self.policy.allow_writes, self.policy.read_only = True, False
            print("  " + self.palette.amber("writes no longer prompt for this session"))
        elif verb == "exec":
            self.policy.allow_execute = True
            print("  " + self.palette.red(
                "command execution enabled — each command is still shown for approval"))
        elif verb == "readonly":
            self.policy.read_only = True
            self.policy.allow_writes = self.policy.allow_execute = False
            print("  " + self.palette.green("read-only: writes and commands are refused"))
        elif verb == "prompt":
            self.policy.read_only = self.policy.allow_writes = False
            self.policy.allow_execute = False
            self.policy.revoke()
            print("  " + self.palette.green("back to prompting for every write"))
        elif verb == "revoke":
            gone = self.policy.revoke(args[1] if len(args) > 1 else "")
            print(f"  revoked {gone} session grant(s)")
        else:
            print("  " + self.palette.red(
                "usage: :trust [write|exec|readonly|prompt|revoke]"))
            return 2
        return 0

    def _meta_changes(self) -> int:
        changes = self.workspace.changes
        if not changes:
            print("  " + self.palette.dim("nothing written this session"))
            return 0
        print()
        for change in changes:
            tone = {
                "created": self.palette.green, "modified": self.palette.amber,
                "deleted": self.palette.red, "moved": self.palette.blue,
            }.get(change.action, self.palette.dim)
            print(f"  {tone(change.action):<12} {change.path}"
                  f"   {self.palette.dim(change.detail)}")
        total = self.workspace.summary()["bytes_written"]
        print(f"\n  {self.palette.dim(f'{len(changes)} change(s), {total:,} bytes')}\n")
        return 0

    def _meta_diff(self, args: List[str]) -> int:
        """Diff a file against nothing — i.e. show it — or against what is on disk.

        Useful after a turn: `:diff src/Consumer.sol` shows what is there now, which is
        the cheapest way to check what the agent actually wrote.
        """
        if not args:
            print("  " + self.palette.red("usage: :diff <path>"))
            return 2
        try:
            content = self.workspace.read_text(args[0])
        except WorkspaceError as exc:
            print("  " + self.palette.red(str(exc)))
            return 2
        print()
        for number, line in enumerate(content.splitlines()[:200], start=1):
            print(f"  {self.palette.dim(f'{number:>4}')}  {line}")
        print()
        return 0

    def _print_providers(self) -> None:
        print()
        for entry in available_providers():
            mark = self.palette.green("ready") if entry["ready"] else self.palette.dim("  —  ")
            free = self.palette.green("free") if entry["free"] else self.palette.dim("paid")
            print(f"  [{mark}] {entry['label']:<16} {free}  {self.palette.dim(entry['detail'])}")
            print(f"           {self.palette.dim(entry['model'])}")
        print(f"\n  {self.palette.dim(f'Override with {PROVIDER_ENV}=<provider>.')}\n")

    def _print_tools(self) -> None:
        """Every tool, grouped by what kind of act it performs.

        Grouped by risk rather than listed flat, because that is the question a user
        actually has. "What can this thing do to my machine" is answered by the shape of
        the list, not by reading twenty-eight descriptions.
        """
        from .agent import TOOLS

        headings = {
            "network": "network — reads a chain",
            "read": "read — reads your files",
            "write": "write — changes your files",
            "execute": "execute — runs commands",
        }
        grouped: dict = {key: [] for key in headings}
        for tool in TOOLS.values():
            grouped[tool.risk.value].append(tool)

        print()
        for key, heading in headings.items():
            tools = grouped[key]
            if not tools:
                continue
            print(f"  {self.palette.bold(heading)}")
            for tool in sorted(tools, key=lambda t: t.name):
                summary = tool.description.split(".")[0][:62]
                print(f"    {self.palette.blue(tool.name):<32} {self.palette.dim(summary)}")
            print()

        posture = self.policy.describe()
        print(f"  {self.palette.dim('writes: ')}{posture['writes']}"
              f"   {self.palette.dim('execute: ')}{posture['execute']}")
        print(f"  {self.palette.dim('sandboxed to ' + str(self.workspace.root))}")
        print(f"  {self.palette.dim('Nothing here can sign a transaction or spend gas.')}\n")

    def _print_banner(self) -> None:
        print(self.palette.blue(BANNER))
        print(f"  {self.palette.dim(f'v{__version__} · {feed_count()} feeds · {len(NETWORKS)} networks')}")

        provider = next((p for p in available_providers() if p["ready"]), None)
        if provider:
            print(f"  {self.palette.dim('chat via')} {self.palette.green(provider['label'])} "
                  f"{self.palette.dim(provider['model'])}")
        else:
            print(f"  {self.palette.dim('chat disabled — no LLM provider. Commands work regardless; see :providers')}")
        posture = self.policy.describe()
        if posture["read_only"]:
            trust = self.palette.green("read-only")
        elif posture["execute"] == "allowed":
            trust = self.palette.red("writes + commands allowed")
        elif posture["writes"] == "allowed":
            trust = self.palette.amber("writes allowed without asking")
        else:
            trust = self.palette.dim("writes ask first")
        print(f"  {self.palette.dim('workspace')} "
              f"{self.palette.blue(str(self.workspace.root))}  {trust}")
        print(f"  {self.palette.dim('Type a command, or ask a question. :help for more.')}\n")

    # ── loop ─────────────────────────────────────────────────────────────────

    def handle(self, line: str) -> int:
        kind = self.classify(line)
        if kind == "empty":
            return 0
        text = line.strip()
        if kind == "meta":
            return self.run_meta(text)
        if kind == "command":
            return self.run_command(text[1:].strip() if text.startswith("!") else text)
        return self.run_chat(text[1:].strip() if text.startswith("?") else text)

    def run(self) -> int:
        self._print_banner()
        self._load_history()
        try:
            while True:
                try:
                    line = input(self._prompt())
                except KeyboardInterrupt:
                    # Cancel the line, keep the session. Losing a REPL to a stray Ctrl-C
                    # while a sweep is running would be its own small tragedy.
                    print()
                    continue
                except EOFError:
                    break
                try:
                    self.handle(line)
                except EOFError:
                    break
                except Exception as exc:  # a bad command must not kill the console
                    print(self.palette.red(f"error: {exc}"))
        finally:
            self._save_history()
        print(self.palette.dim("bye"))
        return 0


def launch(network: str = DEFAULT_NETWORK, mode: str = "auto",
           workspace: Optional[str] = None,
           policy: Optional[TrustPolicy] = None) -> int:
    """Run the console, on the product's palette.

    ``initialize`` here as well as in :func:`alchem_link.cli.main` because the shell is
    also reachable directly — and it is idempotent, so the common path of `alchem-link
    shell` does not theme twice.
    """
    boot.initialize(title=boot.banner_title(__version__))
    return Shell(network=network, mode=mode, workspace=workspace, policy=policy).run()


if __name__ == "__main__":
    raise SystemExit(launch())
