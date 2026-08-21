#!/usr/bin/env python3
"""Topologically order a cargo workspace's publishable crates.

crates.io requires every dependency of a crate to already exist on the registry when that
crate is published, so the order is not a preference — a wrong one fails halfway through and
leaves a partial release that cannot be undone, only yanked.

Reads `cargo metadata`, keeps workspace members that are publishable, and orders them so
that every internal dependency precedes its dependents. Dev-dependencies are excluded: they
are not part of the published dependency graph, and including them can manufacture a cycle
that does not really exist.
"""
import json
import subprocess
import sys


def main() -> int:
    raw = subprocess.run(
        ["cargo", "metadata", "--format-version", "1", "--no-deps"],
        capture_output=True,
        check=True,
    ).stdout
    # Decode as UTF-8 explicitly: cargo emits the manifests' own bytes, and this repo's
    # Cargo.toml comments are full of en dashes. Python's default on Windows is cp1252,
    # which dies on the first one.
    meta = json.loads(raw.decode("utf-8", errors="replace"))

    members = {}
    for pkg in meta["packages"]:
        # `publish` is null when unrestricted, or a list of allowed registries.
        if pkg.get("publish") == []:
            continue
        members[pkg["name"]] = pkg

    names = set(members)
    edges = {}
    for name, pkg in members.items():
        deps = set()
        for d in pkg["dependencies"]:
            if d["kind"] == "dev":
                continue
            if d["name"] in names:
                deps.add(d["name"])
        edges[name] = deps

    ordered, seen, stack = [], set(), set()

    def visit(n):
        if n in seen:
            return
        if n in stack:
            print(f"CYCLE involving {n}", file=sys.stderr)
            sys.exit(2)
        stack.add(n)
        for d in sorted(edges[n]):
            visit(d)
        stack.discard(n)
        seen.add(n)
        ordered.append(n)

    for n in sorted(names):
        visit(n)

    for i, n in enumerate(ordered, 1):
        pkg = members[n]
        internal = sorted(edges[n])
        tail = "<- " + ", ".join(internal) if internal else "(no internal deps)"
        print(f"{i:2}. {n:<26} {pkg['version']:<10} {tail}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
