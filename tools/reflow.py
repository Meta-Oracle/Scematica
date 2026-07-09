#!/usr/bin/env python3
"""Reflow prose: join hard-wrapped paragraph lines into one line each, separated by
blank lines. Leaves headers, rules, lists, tables, code, and aligned data blocks alone.
Usage: python tools/reflow.py <file> [--write]
"""
import re
import sys

path = sys.argv[1]
write = "--write" in sys.argv[2:]

with open(path, encoding="utf-8") as f:
    text = f.read().replace("\r\n", "\n").replace("\r", "\n")

lines = text.split("\n")


def is_rule(l):
    s = l.strip()
    return len(s) >= 3 and set(s) <= set("=-_*~")


def aligned(l):
    # columnar / data / table lines we must NOT join into prose
    s = l.strip()
    return ("...." in l) or ("   " in s) or ("\t" in l) or l.lstrip().startswith("|")


def structural(block):
    for l in block:
        s = l.lstrip()
        if s.startswith(("#", "- ", "* ", "> ", "|", "```", "1.", "2.", "3.")):
            return True
        if re.match(r"\d+[.)]\s", s):
            return True
        if is_rule(l) or aligned(l):
            return True
    return False


# split into blocks on blank lines, tracking fenced code so it's never reflowed
blocks, cur, in_code = [], [], False
for l in lines:
    if l.strip().startswith("```"):
        in_code = not in_code
        cur.append(l)
        continue
    if not in_code and l.strip() == "":
        if cur:
            blocks.append(cur)
            cur = []
        else:
            blocks.append([])  # preserve intentional extra blank
    else:
        cur.append(l)
if cur:
    blocks.append(cur)

out = []
for b in blocks:
    if not b:
        continue
    if structural(b):
        out.append("\n".join(b))
    else:
        para = ""
        for x in b:
            x = x.strip()
            if not para:
                para = x
            elif re.search(r"[A-Za-z0-9]-$", para):
                para += x  # wrapped hyphenated word (e.g. "cross-\nDEX") — no space
            else:
                para += " " + x
        out.append(para)

result = "\n\n".join(out).rstrip() + "\n"

if write:
    with open(path, "w", encoding="utf-8") as f:
        f.write(result)
    print(f"[reflow] rewrote {path}  ({len(lines)} lines -> {result.count(chr(10))} lines)")
else:
    sys.stdout.reconfigure(encoding="utf-8")
    print(result[:1600])
    print("\n... [preview; run with --write to apply] ...")
