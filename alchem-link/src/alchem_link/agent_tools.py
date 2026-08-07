"""The tools that let the agent write code rather than only talk about it.

:mod:`alchem_link.agent` gave the model a read-only view of chains. This module gives it
a workspace: files it can read, create and edit, directories it can make and search,
project scaffolding it can generate, results it can export, and — only when explicitly
switched on — commands it can run.

Every tool here is one :class:`Tool` record carrying four things: the JSON schema the
model sees, the implementation, a :class:`~alchem_link.approvals.Risk` classification,
and a function that turns a pending call into a human-readable approval request. That
last field is the one that makes the difference between a permission prompt people read
and one they dismiss. "write_file" is not a question anybody can answer; "create
src/EthUsdConsumer.sol, 84 lines, new file" is.

Two design rules run through the file.

**A tool never touches a path directly.** Everything goes through
:class:`~alchem_link.workspace.Workspace`, which resolves symlinks, refuses escapes, and
refuses secrets. A tool implementation that called ``open()`` would silently bypass the
entire security model, so none does.

**Failures are returned, not raised.** A bad path or a missing file comes back to the
model as an error string it can react to, because the useful behaviour is the model
correcting itself and trying again rather than the session dying. The dispatcher in
:mod:`alchem_link.agent` enforces this; the tools here are free to raise.
"""
from __future__ import annotations

import os
import shlex
import subprocess
from dataclasses import dataclass, field
from typing import Any, Callable, Dict, List, Optional, Sequence

from .approvals import Approver, Request, Risk, TrustPolicy
from .workspace import MAX_ENTRIES, Workspace

#: Wall-clock ceiling on one `run_command`. A build that takes longer than this should be
#: started by the user, not by an agent that will then sit blocked holding the terminal.
COMMAND_TIMEOUT_SECS = 120

#: Cap on captured command output, in characters.
MAX_COMMAND_OUTPUT = 8000


@dataclass
class ToolContext:
    """Everything a tool needs beyond its own arguments.

    Passed to every implementation that declares ``needs_context``. Holding the policy
    and approver here rather than in globals is what lets a test drive the whole agent
    with an auto-approver against a temporary directory.
    """

    workspace: Workspace
    policy: TrustPolicy
    approver: Approver
    network: str = "ethereum"


@dataclass
class Tool:
    """One callable the model can invoke."""

    name: str
    description: str
    parameters: Dict[str, Any]
    required: List[str]
    impl: Callable[..., Any]
    risk: Risk = Risk.NETWORK
    #: True when ``impl`` takes a :class:`ToolContext` as its first argument.
    needs_context: bool = False
    #: Builds the approval request. Defaults to a bare summary; tools that touch a path
    #: override it so the prompt can name the file and show a diff.
    describe: Optional[Callable[["ToolContext", Dict[str, Any]], Request]] = None

    @property
    def schema(self) -> Dict[str, Any]:
        return {
            "type": "function",
            "function": {
                "name": self.name,
                "description": self.description,
                "parameters": {
                    "type": "object",
                    "properties": self.parameters,
                    "required": self.required,
                },
            },
        }

    def request_for(self, context: ToolContext, arguments: Dict[str, Any]) -> Request:
        if self.describe is not None:
            return self.describe(context, arguments)
        return Request(tool=self.name, risk=self.risk, arguments=arguments,
                       summary=f"{self.name}({_argstring(arguments)})")


def _argstring(arguments: Dict[str, Any], limit: int = 60) -> str:
    parts = []
    for key, value in arguments.items():
        text = repr(value)
        if len(text) > limit:
            text = text[:limit] + "…"
        parts.append(f"{key}={text}")
    return ", ".join(parts)


# ── schema fragments ─────────────────────────────────────────────────────────

_PATH = {"type": "string", "description": "Path relative to the workspace root"}
_CONTENT = {"type": "string", "description": "Full file content"}


# ── reading ──────────────────────────────────────────────────────────────────


def _read_file(context: ToolContext, path: str,
               start_line: int = 0, max_lines: int = 0) -> Dict[str, Any]:
    """Read a text file, optionally a slice of it."""
    content = context.workspace.read_text(path)
    lines = content.splitlines()
    total = len(lines)
    if start_line or max_lines:
        begin = max(0, start_line - 1 if start_line else 0)
        end = begin + max_lines if max_lines else total
        lines = lines[begin:end]
        offset = begin + 1
    else:
        offset = 1
    return {
        "path": path,
        "total_lines": total,
        "first_line": offset,
        # Numbered so the model can quote a location back, and so `edit_file` arguments
        # can be checked against something.
        "content": "\n".join(f"{offset + i:>5}  {line}" for i, line in enumerate(lines)),
    }


def _list_dir(context: ToolContext, path: str = ".") -> Dict[str, Any]:
    entries = context.workspace.list_dir(path)
    return {"path": path, "entries": entries, "count": len(entries)}


def _tree(context: ToolContext, path: str = ".", depth: int = 3) -> Dict[str, Any]:
    lines = context.workspace.tree(path, depth=max(1, min(depth, 6)))
    return {"path": path, "tree": "\n".join(lines), "entries": len(lines)}


def _find_files(context: ToolContext, pattern: str = "*", path: str = ".") -> Dict[str, Any]:
    found = context.workspace.walk(path, pattern)
    return {"pattern": pattern, "matches": found, "count": len(found)}


def _search_text(context: ToolContext, pattern: str, glob: str = "*",
                 path: str = ".") -> Dict[str, Any]:
    hits = context.workspace.search(pattern, glob=glob, path=path)
    return {"pattern": pattern, "hits": hits, "count": len(hits)}


# ── writing ──────────────────────────────────────────────────────────────────


def _write_file(context: ToolContext, path: str, content: str,
                overwrite: bool = True) -> Dict[str, Any]:
    change = context.workspace.write_text(path, content, overwrite=overwrite)
    return {"path": change.path, "action": change.action, "detail": change.detail,
            "bytes": change.bytes_written}


def _describe_write(context: ToolContext, arguments: Dict[str, Any]) -> Request:
    path = str(arguments.get("path", ""))
    content = str(arguments.get("content", ""))
    try:
        target = context.workspace.resolve(path)
        relative = context.workspace.relative(target)
        exists = target.exists()
    except Exception:
        relative, exists = path, False
    lines = len(content.splitlines())
    verb = "overwrite" if exists else "create"
    return Request(
        tool="write_file", risk=Risk.WRITE, arguments=arguments, path=relative,
        summary=f"{verb} {relative} — {lines} lines, {len(content.encode('utf-8')):,} bytes",
        preview=context.workspace.preview(path, content),
    )


def _edit_file(context: ToolContext, path: str, find: str, replace: str,
               count: int = 1) -> Dict[str, Any]:
    change = context.workspace.edit_text(path, find, replace, count=count)
    return {"path": change.path, "action": change.action, "detail": change.detail}


def _describe_edit(context: ToolContext, arguments: Dict[str, Any]) -> Request:
    path = str(arguments.get("path", ""))
    find = str(arguments.get("find", ""))
    replace = str(arguments.get("replace", ""))
    preview: List[str] = []
    try:
        original = context.workspace.read_text(path)
        if find in original:
            updated = original.replace(find, replace, -1 if arguments.get("count") == 0 else 1)
            preview = context.workspace.preview(path, updated)
    except Exception:
        preview = [f"- {find[:120]}", f"+ {replace[:120]}"]
    return Request(
        tool="edit_file", risk=Risk.WRITE, arguments=arguments, path=path,
        summary=f"edit {path} — replace {len(find.splitlines())} line(s)",
        preview=preview,
    )


def _append_file(context: ToolContext, path: str, content: str) -> Dict[str, Any]:
    try:
        existing = context.workspace.read_text(path)
    except Exception:
        existing = ""
    joined = existing + ("" if existing.endswith("\n") or not existing else "\n") + content
    change = context.workspace.write_text(path, joined)
    return {"path": change.path, "action": "appended", "bytes": change.bytes_written}


def _make_dir(context: ToolContext, path: str) -> Dict[str, Any]:
    change = context.workspace.make_dir(path)
    return {"path": change.path, "action": change.action}


def _delete_path(context: ToolContext, path: str, recursive: bool = False) -> Dict[str, Any]:
    change = context.workspace.delete(path, recursive=recursive)
    return {"path": change.path, "action": change.action, "detail": change.detail}


def _describe_delete(context: ToolContext, arguments: Dict[str, Any]) -> Request:
    path = str(arguments.get("path", ""))
    recursive = bool(arguments.get("recursive"))
    detail = "recursively, including everything under it" if recursive else ""
    return Request(
        tool="delete_path", risk=Risk.WRITE, arguments=arguments, path=path,
        summary=f"DELETE {path} {detail}".strip(),
    )


def _move_path(context: ToolContext, source: str, destination: str,
               overwrite: bool = False) -> Dict[str, Any]:
    change = context.workspace.move(source, destination, overwrite=overwrite)
    return {"path": change.path, "action": change.action, "detail": change.detail}


def _copy_path(context: ToolContext, source: str, destination: str,
               overwrite: bool = False) -> Dict[str, Any]:
    change = context.workspace.copy(source, destination, overwrite=overwrite)
    return {"path": change.path, "action": change.action, "detail": change.detail}


def _describe_two_path(tool: str, verb: str):
    def describe(context: ToolContext, arguments: Dict[str, Any]) -> Request:
        source = str(arguments.get("source", ""))
        destination = str(arguments.get("destination", ""))
        return Request(tool=tool, risk=Risk.WRITE, arguments=arguments, path=destination,
                       summary=f"{verb} {source} → {destination}")
    return describe


# ── codegen ──────────────────────────────────────────────────────────────────


def _generate_consumer(context: ToolContext, pair: str, network: str = "",
                       language: str = "solidity", path: str = "") -> Dict[str, Any]:
    """Emit a consumer with every audited check wired in, optionally to a file.

    Routed through the package's own generator rather than asked of the model. The
    generator bakes in the *measured* heartbeat for that feed on that chain and every
    check :mod:`alchem_link.safety` audits for; a model writing the same contract from
    memory produces something that looks right and hardcodes 3600.
    """
    from .codegen import generate_consumer

    result = generate_consumer(pair, network=network or context.network, language=language)
    payload: Dict[str, Any] = {
        "pair": result.pair,
        "language": language,
        "lines": len(result.code.splitlines()),
    }
    if path:
        change = context.workspace.write_text(path, result.code)
        payload.update({"path": change.path, "action": change.action})
    else:
        payload["code"] = result.code
    return payload


def _describe_generate(context: ToolContext, arguments: Dict[str, Any]) -> Request:
    path = str(arguments.get("path", ""))
    pair = arguments.get("pair", "")
    if not path:
        # Nothing is written, so this is a chain-backed read like any other.
        return Request(tool="generate_consumer", risk=Risk.NETWORK, arguments=arguments,
                       summary=f"generate a {pair} consumer (not written to disk)")
    return Request(
        tool="generate_consumer", risk=Risk.WRITE, arguments=arguments, path=path,
        summary=f"generate a {pair} consumer and write it to {path}",
    )


def _generate_project(context: ToolContext, pair: str, out: str,
                      network: str = "", framework: str = "foundry") -> Dict[str, Any]:
    """Scaffold a full project — consumer, mocks, tests, deploy script — into ``out``.

    The generator writes through :class:`pathlib.Path` directly rather than through the
    workspace, so ``out`` is resolved here first: that resolution is what confines the
    whole scaffold to the sandbox, and skipping it would let one tool bypass what every
    other tool goes through.
    """
    from pathlib import Path

    from .codegen import generate_project
    from .workspace import Change

    project = generate_project(pair, network=network or context.network, framework=framework)
    directory = context.workspace.resolve(out)
    directory.mkdir(parents=True, exist_ok=True)

    written = project.write(str(directory), overwrite=True)
    relative = [context.workspace.relative(Path(entry)) for entry in written]
    for entry in relative:
        context.workspace.changes.append(
            Change(action="created", path=entry, detail="generated project file")
        )
    return {
        "project": project.name,
        "framework": project.framework,
        "pairs": list(project.pairs),
        "guards": list(project.guards),
        "written": relative,
        "count": len(relative),
    }


def _describe_project(context: ToolContext, arguments: Dict[str, Any]) -> Request:
    out = str(arguments.get("out", ""))
    return Request(
        tool="generate_project", risk=Risk.WRITE, arguments=arguments, path=out,
        summary=f"scaffold a {arguments.get('framework', 'foundry')} project for "
                f"{arguments.get('pair', '')} into {out}/ (consumer, mocks, tests, deploy)",
    )


# ── export ───────────────────────────────────────────────────────────────────


def _export_data(context: ToolContext, dataset: str, path: str,
                 fmt: str = "csv", network: str = "") -> Dict[str, Any]:
    """Run a toolkit query and write the result out in a machine format."""
    from .exporters import export
    from .feeds import read_all_feeds
    from .registry import coverage, find
    from .safety import audit_network

    target = network or context.network
    if dataset == "feeds":
        rows = [r.as_dict() for r in read_all_feeds(network=target)]
    elif dataset == "audit":
        rows = [a.as_dict() for a in audit_network(network=target)]
    elif dataset == "registry":
        rows = [r.as_dict() for r in find(network=target)]
    elif dataset == "coverage":
        rows = [{"network": k, **v} for k, v in coverage().items()]
    else:
        raise ValueError(
            f"unknown dataset '{dataset}'. Known: feeds, audit, registry, coverage"
        )
    body = export(rows, fmt)
    change = context.workspace.write_text(path, body + "\n")
    return {"dataset": dataset, "format": fmt, "rows": len(rows),
            "path": change.path, "bytes": change.bytes_written}


def _describe_export(context: ToolContext, arguments: Dict[str, Any]) -> Request:
    return Request(
        tool="export_data", risk=Risk.WRITE, arguments=arguments,
        path=str(arguments.get("path", "")),
        summary=f"export {arguments.get('dataset')} as "
                f"{arguments.get('fmt', 'csv')} to {arguments.get('path')}",
    )


# ── execution ────────────────────────────────────────────────────────────────


def split_command(command: str) -> List[str]:
    """Split a command string into an argument vector, correctly on both platforms.

    Neither :mod:`shlex` mode is right on Windows on its own. ``posix=True`` treats the
    backslash as an escape character, so ``C:\\Users\\dev`` becomes ``C:Usersdev``.
    ``posix=False`` keeps the backslashes but also keeps the quotes, so a perfectly
    ordinary ``"C:\\Program Files\\Git\\bin\\git.exe" --version`` is looked up as a
    binary whose name begins with a quote mark and is never found.

    So on Windows: split in non-POSIX mode to preserve paths, then strip one layer of
    surrounding quotes from each token, which is what the quotes were there to do.
    """
    if os.name != "nt":
        return shlex.split(command)
    tokens = shlex.split(command, posix=False)
    return [
        token[1:-1] if len(token) >= 2 and token[0] == token[-1] in ("\"", "'") else token
        for token in tokens
    ]


def _run_command(context: ToolContext, command: str,
                 timeout: int = COMMAND_TIMEOUT_SECS) -> Dict[str, Any]:
    """Run a command in the workspace and return its output.

    Deliberately **not** through a shell. The command is split with :mod:`shlex` and
    executed as an argument vector, which means no pipes, no redirection, no globbing and
    no ``;``. That costs some convenience and removes an entire class of injection: what
    the user reads in the approval prompt is exactly the argument list that runs, with no
    second layer of interpretation between the two.
    """
    try:
        argv = split_command(command)
    except ValueError as exc:
        raise ValueError(f"could not parse command: {exc}") from exc
    if not argv:
        raise ValueError("empty command")

    try:
        completed = subprocess.run(
            argv,
            cwd=str(context.workspace.root),
            capture_output=True,
            text=True,
            timeout=max(1, min(timeout, COMMAND_TIMEOUT_SECS)),
            shell=False,
            check=False,
        )
    except FileNotFoundError as exc:
        raise ValueError(f"command not found: {argv[0]}") from exc
    except subprocess.TimeoutExpired:
        raise TimeoutError(f"'{argv[0]}' did not finish within {timeout}s")

    def clip(text: str) -> str:
        text = text or ""
        if len(text) <= MAX_COMMAND_OUTPUT:
            return text
        return text[:MAX_COMMAND_OUTPUT] + f"\n… [truncated at {MAX_COMMAND_OUTPUT} chars]"

    return {
        "command": " ".join(argv),
        "exit_code": completed.returncode,
        "stdout": clip(completed.stdout),
        "stderr": clip(completed.stderr),
    }


def _describe_command(context: ToolContext, arguments: Dict[str, Any]) -> Request:
    command = str(arguments.get("command", ""))
    try:
        argv = split_command(command)
    except ValueError:
        argv = [command]
    return Request(
        tool="run_command", risk=Risk.EXECUTE, arguments=arguments,
        summary=f"run: {' '.join(argv)}   (in {context.workspace.root})",
        preview=[f"$ {' '.join(argv)}", f"  cwd: {context.workspace.root}",
                 "  no shell — argv is executed directly, so pipes and redirection "
                 "are not interpreted"],
    )


# ── workspace introspection ──────────────────────────────────────────────────


def _workspace_info(context: ToolContext) -> Dict[str, Any]:
    """Where the agent is working and what it has changed. Costs nothing to call."""
    return {
        **context.workspace.summary(),
        "trust": context.policy.describe(),
        "network": context.network,
    }


# ── registry ─────────────────────────────────────────────────────────────────

CODING_TOOLS: List[Tool] = [
    Tool(
        name="workspace_info",
        description="Where the workspace root is, what has been changed this session, "
                    "and what the trust policy currently permits. Call this first if "
                    "unsure whether you may write.",
        parameters={}, required=[], impl=_workspace_info,
        risk=Risk.READ, needs_context=True,
    ),
    Tool(
        name="list_dir",
        description="List one directory level in the workspace.",
        parameters={"path": _PATH}, required=[], impl=_list_dir,
        risk=Risk.READ, needs_context=True,
    ),
    Tool(
        name="tree",
        description="An indented recursive listing, depth-limited. Use to orient "
                    "yourself in an unfamiliar project before reading files.",
        parameters={"path": _PATH,
                    "depth": {"type": "integer", "description": "Levels to descend, 1-6"}},
        required=[], impl=_tree, risk=Risk.READ, needs_context=True,
    ),
    Tool(
        name="read_file",
        description="Read a text file from the workspace. Returns numbered lines. Read "
                    "before editing — edit_file needs the exact existing text.",
        parameters={
            "path": _PATH,
            "start_line": {"type": "integer", "description": "1-based first line"},
            "max_lines": {"type": "integer", "description": "Lines to return, 0 for all"},
        },
        required=["path"], impl=_read_file, risk=Risk.READ, needs_context=True,
    ),
    Tool(
        name="find_files",
        description="Find files by glob pattern, e.g. '*.sol' or 'test_*.py'.",
        parameters={"pattern": {"type": "string", "description": "Glob, e.g. '*.sol'"},
                    "path": _PATH},
        required=[], impl=_find_files, risk=Risk.READ, needs_context=True,
    ),
    Tool(
        name="search_text",
        description="Regular-expression search across the workspace's text files. "
                    "Returns path, line number and the matching line.",
        parameters={"pattern": {"type": "string", "description": "Regular expression"},
                    "glob": {"type": "string", "description": "Restrict to files matching"},
                    "path": _PATH},
        required=["pattern"], impl=_search_text, risk=Risk.READ, needs_context=True,
    ),
    Tool(
        name="write_file",
        description="Create a file, or replace one entirely. Provide the complete "
                    "content. For a small change to a large file use edit_file instead.",
        parameters={"path": _PATH, "content": _CONTENT,
                    "overwrite": {"type": "boolean",
                                  "description": "Replace an existing file"}},
        required=["path", "content"], impl=_write_file,
        risk=Risk.WRITE, needs_context=True, describe=_describe_write,
    ),
    Tool(
        name="edit_file",
        description="Replace an exact string inside an existing file. The 'find' text "
                    "must appear exactly once, so include surrounding context to make it "
                    "unique. Read the file first.",
        parameters={
            "path": _PATH,
            "find": {"type": "string", "description": "Exact existing text, must be unique"},
            "replace": {"type": "string", "description": "Replacement text"},
            "count": {"type": "integer",
                      "description": "0 replaces every occurrence; default 1"},
        },
        required=["path", "find", "replace"], impl=_edit_file,
        risk=Risk.WRITE, needs_context=True, describe=_describe_edit,
    ),
    Tool(
        name="append_file",
        description="Append text to the end of a file, creating it if absent.",
        parameters={"path": _PATH, "content": _CONTENT},
        required=["path", "content"], impl=_append_file,
        risk=Risk.WRITE, needs_context=True,
    ),
    Tool(
        name="make_dir",
        description="Create a directory, including parents.",
        parameters={"path": _PATH}, required=["path"], impl=_make_dir,
        risk=Risk.WRITE, needs_context=True,
    ),
    Tool(
        name="delete_path",
        description="Delete a file, or a directory when recursive is true. "
                    "Irreversible — prefer moving to a scratch directory when unsure.",
        parameters={"path": _PATH,
                    "recursive": {"type": "boolean",
                                  "description": "Required for a non-empty directory"}},
        required=["path"], impl=_delete_path,
        risk=Risk.WRITE, needs_context=True, describe=_describe_delete,
    ),
    Tool(
        name="move_path",
        description="Move or rename a file or directory.",
        parameters={"source": _PATH, "destination": _PATH,
                    "overwrite": {"type": "boolean"}},
        required=["source", "destination"], impl=_move_path,
        risk=Risk.WRITE, needs_context=True,
        describe=_describe_two_path("move_path", "move"),
    ),
    Tool(
        name="copy_path",
        description="Copy a file or directory.",
        parameters={"source": _PATH, "destination": _PATH,
                    "overwrite": {"type": "boolean"}},
        required=["source", "destination"], impl=_copy_path,
        risk=Risk.WRITE, needs_context=True,
        describe=_describe_two_path("copy_path", "copy"),
    ),
    Tool(
        name="generate_consumer",
        description="Emit an oracle consumer contract with every safety check wired in "
                    "and that feed's MEASURED heartbeat baked in. ALWAYS use this rather "
                    "than writing a Chainlink consumer yourself — it gets the per-chain "
                    "heartbeat and the sequencer gate right. Omit 'path' to return the "
                    "code, give one to write it.",
        parameters={
            "pair": {"type": "string", "description": "Feed pair, e.g. ETH/USD"},
            "network": {"type": "string", "description": "Network key"},
            "language": {"type": "string",
                         "description": "solidity, typescript, python or rust"},
            "path": {"type": "string", "description": "Optional: write it here"},
        },
        required=["pair"], impl=_generate_consumer,
        risk=Risk.WRITE, needs_context=True, describe=_describe_generate,
    ),
    Tool(
        name="generate_project",
        description="Scaffold a complete project for a feed: consumer, mock aggregator, "
                    "tests covering the failure modes, and a deploy script.",
        parameters={
            "pair": {"type": "string", "description": "Feed pair, e.g. ETH/USD"},
            "out": {"type": "string", "description": "Directory to create it in"},
            "network": {"type": "string", "description": "Network key"},
            "framework": {"type": "string", "description": "foundry or hardhat"},
        },
        required=["pair", "out"], impl=_generate_project,
        risk=Risk.WRITE, needs_context=True, describe=_describe_project,
    ),
    Tool(
        name="export_data",
        description="Run a toolkit query and write the result to a file. Datasets: "
                    "feeds (live prices), audit, registry, coverage. Formats: csv, json, "
                    "ndjson, markdown, prometheus.",
        parameters={
            "dataset": {"type": "string",
                        "description": "feeds, audit, registry or coverage"},
            "path": _PATH,
            "fmt": {"type": "string",
                    "description": "csv, json, ndjson, markdown, table or prometheus"},
            "network": {"type": "string", "description": "Network key"},
        },
        required=["dataset", "path"], impl=_export_data,
        risk=Risk.WRITE, needs_context=True, describe=_describe_export,
    ),
    Tool(
        name="run_command",
        description="Run a command in the workspace and return its output. No shell: "
                    "pipes, redirection and ';' are not interpreted. Off unless the user "
                    "has enabled execution.",
        parameters={
            "command": {"type": "string", "description": "Command with arguments"},
            "timeout": {"type": "integer", "description": f"Seconds, max {COMMAND_TIMEOUT_SECS}"},
        },
        required=["command"], impl=_run_command,
        risk=Risk.EXECUTE, needs_context=True, describe=_describe_command,
    ),
]


def coding_tools() -> List[Tool]:
    return list(CODING_TOOLS)


__all__ = [
    "Tool",
    "split_command",
    "ToolContext",
    "CODING_TOOLS",
    "coding_tools",
    "COMMAND_TIMEOUT_SECS",
    "MAX_COMMAND_OUTPUT",
]
