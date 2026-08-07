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
  :providers         LLM providers and which are usable
  :tools             what the agent is allowed to call
  :reset             clear the chat history, keep the session
  :quit / :q         exit (Ctrl-D also works)

  Anything starting with `?` is always a question.
  Anything starting with `!` is always a command.
"""


def _supports_colour() -> bool:
    if os.environ.get("NO_COLOR"):
        return False
    return sys.stdout.isatty()


class Palette:
    """Mirrors `theme.py`'s roles so the console matches the TUI."""

    def __init__(self, enabled: bool) -> None:
        self.enabled = enabled

    def _wrap(self, code: str, text: str) -> str:
        return f"\033[{code}m{text}\033[0m" if self.enabled else text

    def blue(self, text: str) -> str:
        return self._wrap("38;5;75", text)

    def dim(self, text: str) -> str:
        return self._wrap("38;5;244", text)

    def green(self, text: str) -> str:
        return self._wrap("38;5;79", text)

    def amber(self, text: str) -> str:
        return self._wrap("38;5;214", text)

    def red(self, text: str) -> str:
        return self._wrap("38;5;204", text)

    def bold(self, text: str) -> str:
        return self._wrap("1", text)


class Completer:
    """Tab completion over commands, network keys and pair names."""

    def __init__(self, commands: List[str]) -> None:
        self.commands = sorted(commands)
        self.networks = sorted(NETWORKS)
        self.pairs = sorted({pair for table in FEEDS.values() for pair in table})
        self.meta = [
            ":help", ":commands", ":net", ":chat", ":cmd", ":auto",
            ":providers", ":tools", ":reset", ":quit", ":q",
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
        return self.pairs + self.networks

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
                 colour: Optional[bool] = None) -> None:
        from .cli import command_names  # imported here to avoid a circular import

        self.network = get_network(network).key
        self.mode = mode
        self.palette = Palette(_supports_colour() if colour is None else colour)
        self.commands = command_names()
        self._agent = None
        self._agent_error = ""

    # ── agent ────────────────────────────────────────────────────────────────

    def _ensure_agent(self):
        """Build the agent lazily — no provider probe until chat is actually used."""
        if self._agent is not None or self._agent_error:
            return self._agent
        try:
            from .agent import build_agent

            self._agent = build_agent(network=self.network)
        except NoProviderConfigured as exc:
            self._agent_error = str(exc)
        except Exception as exc:
            self._agent_error = f"could not start the agent: {exc}"
        return self._agent

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
            mark = self.palette.dim("·") if call.ok else self.palette.red("×")
            detail = "" if call.ok else self.palette.red(f"  {call.error[:70]}")
            print(f"  {mark} {self.palette.dim(call.summary)}{detail}")

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
        from .cli import LIVE_COMMANDS, REFERENCE_COMMANDS

        for title, group in (("live", LIVE_COMMANDS), ("reference", REFERENCE_COMMANDS)):
            print(f"\n  {self.palette.bold(title)}")
            for name, help_text in group:
                print(f"    {self.palette.blue(name):<22} {self.palette.dim(help_text)}")
        print()

    def _print_providers(self) -> None:
        print()
        for entry in available_providers():
            mark = self.palette.green("ready") if entry["ready"] else self.palette.dim("  —  ")
            free = self.palette.green("free") if entry["free"] else self.palette.dim("paid")
            print(f"  [{mark}] {entry['label']:<16} {free}  {self.palette.dim(entry['detail'])}")
            print(f"           {self.palette.dim(entry['model'])}")
        print(f"\n  {self.palette.dim(f'Override with {PROVIDER_ENV}=<provider>.')}\n")

    def _print_tools(self) -> None:
        from .agent import TOOL_SCHEMAS

        print()
        for schema in TOOL_SCHEMAS:
            function = schema["function"]
            summary = function["description"].split(".")[0]
            print(f"  {self.palette.blue(function['name']):<32} {self.palette.dim(summary)}")
        print(f"\n  {self.palette.dim('All read-only. Nothing here can sign, spend or write.')}\n")

    def _print_banner(self) -> None:
        print(self.palette.blue(BANNER))
        print(f"  {self.palette.dim(f'v{__version__} · {feed_count()} feeds · {len(NETWORKS)} networks')}")

        provider = next((p for p in available_providers() if p["ready"]), None)
        if provider:
            print(f"  {self.palette.dim('chat via')} {self.palette.green(provider['label'])} "
                  f"{self.palette.dim(provider['model'])}")
        else:
            print(f"  {self.palette.dim('chat disabled — no LLM provider. Commands work regardless; see :providers')}")
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


def launch(network: str = DEFAULT_NETWORK, mode: str = "auto") -> int:
    return Shell(network=network, mode=mode).run()


if __name__ == "__main__":
    raise SystemExit(launch())
