"""The sandbox every filesystem tool goes through.

Once an agent can write files, "which files" stops being a detail and becomes the whole
security model. This module is that model. Every path a tool touches is resolved here
first, and a path that resolves outside the workspace root is refused before any tool
sees it.

Three properties are worth stating explicitly, because each closes a hole that a naive
implementation leaves open.

**Escapes are checked after resolution, not before.** Rejecting a path because it
contains ``..`` is theatre: ``a/../../etc/passwd`` is obvious, but a symlink inside the
workspace pointing at ``/`` is not, and neither is a Windows junction. Paths are fully
resolved — symlinks followed, ``..`` collapsed, made absolute — and only then compared
against the resolved root.

**Reading a secret is an exfiltration, not just a read.** This is the non-obvious one.
Tool results are sent to a third-party language model, so a tool that reads ``.env``
hands the user's API keys to whoever runs the inference endpoint. The same goes for SSH
keys, PEM files, cloud credentials, and — in the repository this package ships inside —
Solana keypairs. :data:`PROTECTED_PATTERNS` refuses those *by name*, inside the
workspace, for reads as well as writes, and the refusal is not overridable by an approval
prompt. A user cannot meaningfully consent to a disclosure they have not been shown.

**Size is bounded on the way in.** A tool that reads a 2 GB file does not fail usefully;
it exhausts memory, or it succeeds and blows out an LLM context window at real cost.
Limits are applied before the read, from the file's own stat.
"""
from __future__ import annotations

import fnmatch
import os
import shutil
from dataclasses import dataclass, field
from pathlib import Path
from typing import Dict, Iterable, List, Optional, Sequence

from .errors import AlchemLinkError

#: Largest file a tool will read, in bytes. Comfortably more than any source file and
#: far less than anything that would hurt.
MAX_READ_BYTES = 512 * 1024

#: Largest file a tool will write.
MAX_WRITE_BYTES = 2 * 1024 * 1024

#: Cap on entries returned by one directory listing or search.
MAX_ENTRIES = 500

#: Names that are never read and never written, matched case-insensitively against both
#: the file name and its path relative to the root.
#:
#: This list is a denylist, which is normally the weaker choice — but the alternative,
#: an allowlist of readable files, makes the agent useless for the codebases people
#: actually have. The mitigation is that it is *not* the only protection: writes are
#: additionally confined to the workspace and gated behind approval, and this list only
#: has to cover the things whose disclosure is catastrophic rather than merely unwanted.
PROTECTED_PATTERNS: Sequence[str] = (
    # Environment and secret files
    ".env", ".env.*", "*.env", "secrets*", "*.secret", "*.secrets",
    "credentials", "credentials.*", "*credentials.json",
    # Keys and certificates
    "*.pem", "*.key", "*.pfx", "*.p12", "*.jks", "*.keystore",
    "id_rsa*", "id_dsa*", "id_ecdsa*", "id_ed25519*", "*.ppk",
    # Wallets and keypairs — this package ships inside a trading repository
    "*keypair*.json", "wallet*.json", "*.wallet", "id.json",
    # Cloud and tool credentials
    ".npmrc", ".pypirc", ".netrc", "_netrc", ".git-credentials",
    ".htpasswd", "*.kdbx",
)

#: Directories that are never descended into, for the same reason plus noise control.
PROTECTED_DIRS: Sequence[str] = (
    ".ssh", ".gnupg", ".aws", ".azure", ".kube", ".docker",
    "node_modules", ".git/objects", "__pycache__", ".venv", "venv",
    "target/debug", "target/release", ".mypy_cache", ".pytest_cache",
)


class WorkspaceError(AlchemLinkError):
    """A path was refused: outside the root, protected, missing, or too large."""


class PathEscape(WorkspaceError):
    """A path resolved outside the workspace root."""


class ProtectedPath(WorkspaceError):
    """A path matched :data:`PROTECTED_PATTERNS`. Not overridable by approval."""

    @property
    def hint(self) -> str:
        return ("secrets are refused before the approval prompt — tool results are sent "
                "to a third-party model, so this is a disclosure, not a read")


@dataclass
class Change:
    """One filesystem mutation, recorded so a session can be reviewed or reported."""

    action: str          # created | modified | deleted | moved
    path: str            # workspace-relative
    detail: str = ""
    bytes_written: int = 0

    def as_dict(self) -> Dict[str, object]:
        return {
            "action": self.action,
            "path": self.path,
            "detail": self.detail,
            "bytes_written": self.bytes_written,
        }


@dataclass
class Workspace:
    """A root directory, and the only legal way to turn a string into a path inside it."""

    root: Path
    #: Every mutation performed this session, oldest first.
    changes: List[Change] = field(default_factory=list)
    #: Extra patterns the caller wants refused, on top of :data:`PROTECTED_PATTERNS`.
    extra_protected: Sequence[str] = ()

    def __post_init__(self) -> None:
        root = Path(self.root).expanduser()
        try:
            resolved = root.resolve(strict=False)
        except (OSError, RuntimeError) as exc:  # pragma: no cover - exotic filesystems
            raise WorkspaceError(f"cannot resolve workspace root {root}: {exc}") from exc
        if not resolved.exists():
            raise WorkspaceError(f"workspace root does not exist: {resolved}")
        if not resolved.is_dir():
            raise WorkspaceError(f"workspace root is not a directory: {resolved}")
        self.root = resolved

    # ── construction ─────────────────────────────────────────────────────────

    @classmethod
    def cwd(cls) -> "Workspace":
        return cls(Path.cwd())

    @classmethod
    def at(cls, path: str) -> "Workspace":
        return cls(Path(path))

    # ── protection ───────────────────────────────────────────────────────────

    def _patterns(self) -> List[str]:
        return list(PROTECTED_PATTERNS) + list(self.extra_protected)

    def is_protected(self, path: Path) -> bool:
        """True when this path is a secret, or lives under a protected directory.

        Matched on the name *and* on the workspace-relative path, so both ``.env`` and
        ``config/.env`` are caught, and case-insensitively, because Windows will happily
        serve ``ID_RSA`` for ``id_rsa``.
        """
        try:
            relative = path.relative_to(self.root).as_posix()
        except ValueError:
            relative = path.as_posix()
        name = path.name.lower()
        lowered = relative.lower()

        for pattern in self._patterns():
            if fnmatch.fnmatch(name, pattern.lower()):
                return True
            if fnmatch.fnmatch(lowered, pattern.lower()):
                return True

        parts = [p.lower() for p in Path(lowered).parts]
        for protected in PROTECTED_DIRS:
            segments = [s.lower() for s in Path(protected).parts]
            if not segments:
                continue
            # A protected directory matches anywhere in the path, so `a/.ssh/b` is caught
            # as well as `.ssh/b`.
            for index in range(len(parts) - len(segments) + 1):
                if parts[index:index + len(segments)] == segments:
                    return True
        return False

    # ── resolution ───────────────────────────────────────────────────────────

    def resolve(self, path: str, must_exist: bool = False,
                for_write: bool = False) -> Path:
        """Turn a tool-supplied string into an absolute path inside the workspace.

        Every filesystem tool calls this and nothing else. An absolute path is accepted
        only if it already lies inside the root, which lets a model echo back a path it
        was shown without that becoming an escape hatch.
        """
        if not path or not str(path).strip():
            raise WorkspaceError("empty path")

        candidate = Path(str(path).strip()).expanduser()
        if not candidate.is_absolute():
            candidate = self.root / candidate

        try:
            resolved = candidate.resolve(strict=False)
        except (OSError, RuntimeError) as exc:  # pragma: no cover
            raise WorkspaceError(f"cannot resolve {path!r}: {exc}") from exc

        # Resolution first, comparison second. Checking the raw string for ".." would
        # miss a symlink inside the workspace pointing anywhere at all.
        if not self._inside(resolved):
            raise PathEscape(
                f"{path!r} resolves to {resolved}, outside the workspace {self.root}",
                path=str(path),
            )
        if self.is_protected(resolved):
            raise ProtectedPath(
                f"{self.relative(resolved)} is a protected path and is never read or written",
                path=self.relative(resolved),
            )
        if must_exist and not resolved.exists():
            raise WorkspaceError(f"no such path: {self.relative(resolved)}",
                                 path=self.relative(resolved))
        if for_write and resolved.is_dir():
            raise WorkspaceError(f"{self.relative(resolved)} is a directory",
                                 path=self.relative(resolved))
        return resolved

    def _inside(self, resolved: Path) -> bool:
        try:
            resolved.relative_to(self.root)
            return True
        except ValueError:
            return False

    def relative(self, path: Path) -> str:
        """Workspace-relative POSIX form, for display and for recording changes."""
        try:
            return path.resolve(strict=False).relative_to(self.root).as_posix() or "."
        except ValueError:
            return str(path)

    # ── reading ──────────────────────────────────────────────────────────────

    def read_text(self, path: str, max_bytes: int = MAX_READ_BYTES) -> str:
        """Read a text file, refusing anything oversized or binary-looking.

        The size check reads the file's stat rather than the file, so an oversized file
        costs nothing to refuse. The binary check is a NUL scan over the first block:
        crude, but it reliably stops a compiled artefact from being pasted into an LLM
        context as mojibake.
        """
        target = self.resolve(path, must_exist=True)
        if target.is_dir():
            raise WorkspaceError(f"{self.relative(target)} is a directory — use list_dir")
        size = target.stat().st_size
        if size > max_bytes:
            raise WorkspaceError(
                f"{self.relative(target)} is {size:,} bytes, over the {max_bytes:,} limit",
                path=self.relative(target),
            )
        raw = target.read_bytes()
        if b"\x00" in raw[:8192]:
            raise WorkspaceError(f"{self.relative(target)} looks like a binary file",
                                 path=self.relative(target))
        return raw.decode("utf-8", errors="replace")

    def list_dir(self, path: str = ".", limit: int = MAX_ENTRIES) -> List[Dict[str, object]]:
        """One directory level. Protected entries are omitted rather than errored on."""
        target = self.resolve(path, must_exist=True)
        if not target.is_dir():
            raise WorkspaceError(f"{self.relative(target)} is not a directory")
        entries: List[Dict[str, object]] = []
        for child in sorted(target.iterdir(), key=lambda c: (c.is_file(), c.name.lower())):
            if self.is_protected(child):
                continue
            try:
                stat = child.stat()
            except OSError:  # pragma: no cover - a broken symlink
                continue
            entries.append({
                "name": child.name,
                "path": self.relative(child),
                "type": "dir" if child.is_dir() else "file",
                "bytes": 0 if child.is_dir() else stat.st_size,
            })
            if len(entries) >= limit:
                break
        return entries

    def walk(self, path: str = ".", pattern: str = "*",
             limit: int = MAX_ENTRIES) -> List[str]:
        """Recursive glob, skipping protected files and directories."""
        target = self.resolve(path, must_exist=True)
        found: List[str] = []
        for candidate in sorted(target.rglob(pattern)):
            if candidate.is_dir() or self.is_protected(candidate):
                continue
            found.append(self.relative(candidate))
            if len(found) >= limit:
                break
        return found

    def tree(self, path: str = ".", depth: int = 3, limit: int = MAX_ENTRIES) -> List[str]:
        """An indented directory listing, depth-limited, for orienting the model."""
        root = self.resolve(path, must_exist=True)
        lines: List[str] = []

        def descend(directory: Path, level: int, prefix: str) -> None:
            if level > depth or len(lines) >= limit:
                return
            try:
                children = sorted(directory.iterdir(),
                                  key=lambda c: (c.is_file(), c.name.lower()))
            except OSError:  # pragma: no cover
                return
            for child in children:
                if self.is_protected(child) or len(lines) >= limit:
                    continue
                lines.append(f"{prefix}{child.name}{'/' if child.is_dir() else ''}")
                if child.is_dir():
                    descend(child, level + 1, prefix + "  ")

        lines.append(f"{self.relative(root)}/")
        descend(root, 1, "  ")
        return lines

    def search(self, pattern: str, glob: str = "*", path: str = ".",
               limit: int = 100) -> List[Dict[str, object]]:
        """Regex search across text files. Returns file, line number and the line."""
        import re

        try:
            expression = re.compile(pattern)
        except re.error as exc:
            raise WorkspaceError(f"bad regular expression: {exc}") from exc

        hits: List[Dict[str, object]] = []
        for relative in self.walk(path, glob, limit=MAX_ENTRIES):
            try:
                content = self.read_text(relative)
            except WorkspaceError:
                continue  # too big, binary, or protected — not an error for a search
            for number, line in enumerate(content.splitlines(), start=1):
                if expression.search(line):
                    hits.append({"path": relative, "line": number, "text": line.strip()[:200]})
                    if len(hits) >= limit:
                        return hits
        return hits

    # ── writing ──────────────────────────────────────────────────────────────

    def write_text(self, path: str, content: str, overwrite: bool = True) -> Change:
        """Write a file, creating parent directories. Records the change."""
        target = self.resolve(path, for_write=True)
        encoded = content.encode("utf-8")
        if len(encoded) > MAX_WRITE_BYTES:
            raise WorkspaceError(
                f"refusing to write {len(encoded):,} bytes, over the "
                f"{MAX_WRITE_BYTES:,} limit"
            )
        existed = target.exists()
        if existed and not overwrite:
            raise WorkspaceError(f"{self.relative(target)} exists — pass overwrite")
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(content, encoding="utf-8", newline="\n")
        change = Change(
            action="modified" if existed else "created",
            path=self.relative(target),
            detail=f"{len(content.splitlines())} lines",
            bytes_written=len(encoded),
        )
        self.changes.append(change)
        return change

    def edit_text(self, path: str, find: str, replace: str, count: int = 1) -> Change:
        """Exact-string replacement inside an existing file.

        Deliberately exact rather than fuzzy, and it refuses an ambiguous match. A model
        asked to change "the timeout" in a file with four timeouts will otherwise change
        whichever one the implementation happened to reach first, and the edit looks
        successful. Making ambiguity an error forces a more specific ``find``.
        """
        target = self.resolve(path, must_exist=True, for_write=True)
        original = self.read_text(path)
        occurrences = original.count(find)
        if occurrences == 0:
            raise WorkspaceError(
                f"{self.relative(target)} does not contain that text",
                path=self.relative(target),
            )
        if occurrences > 1 and count == 1:
            raise WorkspaceError(
                f"{self.relative(target)} contains that text {occurrences} times — "
                "include more surrounding context to make it unique, or pass count=0 "
                "to replace every occurrence",
                path=self.relative(target), occurrences=occurrences,
            )
        updated = original.replace(find, replace, -1 if count == 0 else count)
        target.write_text(updated, encoding="utf-8", newline="\n")
        change = Change(
            action="modified",
            path=self.relative(target),
            detail=f"replaced {occurrences if count == 0 else count} occurrence(s)",
            bytes_written=len(updated.encode("utf-8")),
        )
        self.changes.append(change)
        return change

    def make_dir(self, path: str) -> Change:
        target = self.resolve(path)
        if target.exists() and target.is_dir():
            return Change(action="unchanged", path=self.relative(target),
                          detail="already exists")
        target.mkdir(parents=True, exist_ok=True)
        change = Change(action="created", path=self.relative(target), detail="directory")
        self.changes.append(change)
        return change

    def delete(self, path: str, recursive: bool = False) -> Change:
        """Delete a file, or a directory when ``recursive``.

        A non-empty directory without ``recursive`` is an error rather than a silent
        success, because "delete the build folder" and "delete everything" differ by one
        argument and the mistake is unrecoverable.
        """
        target = self.resolve(path, must_exist=True)
        if target == self.root:
            raise WorkspaceError("refusing to delete the workspace root")
        if target.is_dir():
            if not recursive and any(target.iterdir()):
                raise WorkspaceError(
                    f"{self.relative(target)} is not empty — pass recursive to delete it"
                )
            shutil.rmtree(target) if recursive else target.rmdir()
            detail = "directory"
        else:
            target.unlink()
            detail = "file"
        change = Change(action="deleted", path=self.relative(target), detail=detail)
        self.changes.append(change)
        return change

    def move(self, source: str, destination: str, overwrite: bool = False) -> Change:
        origin = self.resolve(source, must_exist=True)
        target = self.resolve(destination)
        if target.exists() and not overwrite:
            raise WorkspaceError(f"{self.relative(target)} exists — pass overwrite")
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.move(str(origin), str(target))
        change = Change(action="moved", path=self.relative(target),
                        detail=f"from {self.relative(origin)}")
        self.changes.append(change)
        return change

    def copy(self, source: str, destination: str, overwrite: bool = False) -> Change:
        origin = self.resolve(source, must_exist=True)
        target = self.resolve(destination)
        if target.exists() and not overwrite:
            raise WorkspaceError(f"{self.relative(target)} exists — pass overwrite")
        target.parent.mkdir(parents=True, exist_ok=True)
        if origin.is_dir():
            shutil.copytree(origin, target, dirs_exist_ok=overwrite)
        else:
            shutil.copy2(origin, target)
        change = Change(action="created", path=self.relative(target),
                        detail=f"copied from {self.relative(origin)}")
        self.changes.append(change)
        return change

    # ── review ───────────────────────────────────────────────────────────────

    def summary(self) -> Dict[str, object]:
        """What this session did to the filesystem."""
        by_action: Dict[str, List[str]] = {}
        for change in self.changes:
            by_action.setdefault(change.action, []).append(change.path)
        return {
            "root": str(self.root),
            "changes": len(self.changes),
            "by_action": by_action,
            "bytes_written": sum(c.bytes_written for c in self.changes),
        }

    def preview(self, path: str, content: str, context: int = 3) -> List[str]:
        """A unified diff of a proposed write against what is on disk.

        Shown in the approval prompt. Approving a write without seeing what changes is
        not consent, it is a keystroke, and the difference matters most on the edits that
        look routine.
        """
        import difflib

        try:
            target = self.resolve(path)
            existing = self.read_text(path).splitlines() if target.exists() else []
            label = self.relative(target)
        except WorkspaceError:
            existing, label = [], path
        return list(difflib.unified_diff(
            existing, content.splitlines(),
            fromfile=f"a/{label}", tofile=f"b/{label}",
            lineterm="", n=context,
        ))


def default_workspace(path: Optional[str] = None) -> Workspace:
    """The workspace for a session: an explicit path, ``ALCHEM_WORKSPACE``, or the cwd."""
    if path:
        return Workspace.at(path)
    from_env = os.environ.get("ALCHEM_WORKSPACE", "").strip()
    return Workspace.at(from_env) if from_env else Workspace.cwd()
