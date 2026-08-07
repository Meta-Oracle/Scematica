"""Turning readings into the formats other systems eat: CSV, NDJSON, Prometheus, Markdown.

Every result object in this package exposes ``as_dict()``, which makes ``--json`` free.
The formats here are the ones that ``--json`` does not cover, and each exists for a
specific consumer:

``csv``         a spreadsheet, or ``pandas.read_csv``
``ndjson``      log pipelines and ``jq`` — one object per line, streamable
``prometheus``  a scrape endpoint, so a staleness verdict becomes an alert rule
``markdown``    a pull request comment or a runbook

The Prometheus exporter is the one that changes what the toolkit is for. A price is a
gauge; so is an age in seconds; so is a boolean staleness verdict, as 0 or 1. Emit those
on a timer and ``alchem-link`` stops being something you run when you are worried and
becomes something that tells you when to be — ``alchem_link_feed_stale == 1`` is a
complete alert rule.

Serialisation is deliberately strict about floats. ``inf`` and ``nan`` are valid Python
and are *not* valid in any of these formats; Prometheus rejects a scrape containing them,
and ``json.dumps`` emits a bare ``NaN`` that most parsers refuse. They are dropped with a
comment rather than silently becoming ``0``.

The module is ``exporters`` and the dispatch function is ``export``. The names differ on
purpose: re-exporting a function called ``export`` from a module called ``export`` shadows
the module on the package object, so ``alchem_link.export.to_csv`` becomes an
``AttributeError`` while ``alchem_link.export(...)`` still works — a confusing failure for
no benefit. The function keeps the good name; the module takes the plural.
"""
from __future__ import annotations

import csv
import io
import json
import math
import time
from typing import Any, Dict, Iterable, List, Optional, Sequence

#: Prometheus metric namespace.
PREFIX = "alchem_link"

FORMATS = ("json", "ndjson", "csv", "prometheus", "markdown", "table")


def _as_dict(item: Any) -> Dict[str, Any]:
    """Best-effort conversion of any result object into a flat mapping."""
    if isinstance(item, dict):
        return item
    converter = getattr(item, "as_dict", None)
    if callable(converter):
        return converter()
    if hasattr(item, "__dict__"):
        return {k: v for k, v in vars(item).items() if not k.startswith("_")}
    return {"value": item}


def _rows(items: Iterable[Any]) -> List[Dict[str, Any]]:
    return [_as_dict(item) for item in items]


def _columns(rows: Sequence[Dict[str, Any]], columns: Optional[Sequence[str]] = None) -> List[str]:
    """Column order: explicit if given, else first-seen across every row.

    Union rather than the first row's keys — result objects legitimately vary (a failed
    leg carries ``error`` and no ``price``), and taking only the first row's keys would
    silently drop every field that appears later.
    """
    if columns:
        return list(columns)
    seen: List[str] = []
    for row in rows:
        for key in row:
            if key not in seen:
                seen.append(key)
    return seen


def _scalar(value: Any) -> Any:
    """Flatten a nested value into something a cell can hold."""
    if isinstance(value, (list, tuple)):
        return ";".join(str(_scalar(v)) for v in value)
    if isinstance(value, dict):
        return json.dumps(value, sort_keys=True, default=str)
    if isinstance(value, float) and not math.isfinite(value):
        return ""
    return value


# ── formats ──────────────────────────────────────────────────────────────────


def to_json(items: Iterable[Any], indent: int = 2) -> str:
    """Pretty JSON. ``sort_keys`` so a diff between two runs shows real changes only."""
    return json.dumps([_as_dict(i) for i in items], indent=indent, sort_keys=True, default=str)


def to_ndjson(items: Iterable[Any]) -> str:
    """One compact JSON object per line — streamable, greppable, ``jq``-able."""
    return "\n".join(
        json.dumps(_as_dict(item), sort_keys=True, default=str) for item in items
    )


def to_csv(items: Iterable[Any], columns: Optional[Sequence[str]] = None) -> str:
    """RFC 4180 CSV via :mod:`csv`, so quoting and embedded commas are handled.

    ``lineterminator="\\n"`` because the default ``\\r\\n`` produces blank lines when the
    result is printed to a terminal that also translates newlines — which on Windows is
    every terminal.
    """
    rows = _rows(items)
    if not rows:
        return ""
    fields = _columns(rows, columns)
    buffer = io.StringIO()
    writer = csv.DictWriter(buffer, fieldnames=fields, extrasaction="ignore",
                            lineterminator="\n")
    writer.writeheader()
    for row in rows:
        writer.writerow({key: _scalar(row.get(key, "")) for key in fields})
    return buffer.getvalue().rstrip("\n")


def to_markdown(items: Iterable[Any], columns: Optional[Sequence[str]] = None) -> str:
    """A GitHub-flavoured Markdown table. For PR comments and runbooks."""
    rows = _rows(items)
    if not rows:
        return ""
    fields = _columns(rows, columns)
    widths = [
        max(len(field), *(len(str(_scalar(row.get(field, "")))) for row in rows))
        for field in fields
    ]
    header = "| " + " | ".join(f.ljust(w) for f, w in zip(fields, widths)) + " |"
    divider = "|" + "|".join("-" * (w + 2) for w in widths) + "|"
    body = [
        "| " + " | ".join(
            str(_scalar(row.get(field, ""))).ljust(width)
            for field, width in zip(fields, widths)
        ) + " |"
        for row in rows
    ]
    return "\n".join([header, divider, *body])


def to_table(items: Iterable[Any], columns: Optional[Sequence[str]] = None) -> str:
    """Fixed-width plain text. The format that survives being pasted into a chat window."""
    rows = _rows(items)
    if not rows:
        return ""
    fields = _columns(rows, columns)
    widths = [
        max(len(field), *(len(str(_scalar(row.get(field, "")))) for row in rows))
        for field in fields
    ]
    lines = ["  ".join(f.ljust(w) for f, w in zip(fields, widths)).rstrip()]
    lines.append("  ".join("-" * w for w in widths))
    for row in rows:
        lines.append("  ".join(
            str(_scalar(row.get(field, ""))).ljust(width)
            for field, width in zip(fields, widths)
        ).rstrip())
    return "\n".join(lines)


# ── prometheus ───────────────────────────────────────────────────────────────


def _escape_label(value: Any) -> str:
    """Prometheus label values escape backslash, quote and newline. Nothing else."""
    text = str(value)
    return text.replace("\\", "\\\\").replace('"', '\\"').replace("\n", "\\n")


def _metric(name: str, labels: Dict[str, Any], value: Any,
            timestamp_ms: Optional[int] = None) -> Optional[str]:
    """One exposition line, or ``None`` when the value cannot be represented.

    Non-finite values are dropped rather than coerced. Prometheus rejects a scrape body
    containing ``NaN`` outright, so emitting one would lose every *other* metric in the
    same response — a much worse failure than one missing series.
    """
    try:
        number = float(value)
    except (TypeError, ValueError):
        return None
    if not math.isfinite(number):
        return None
    rendered = ",".join(f'{k}="{_escape_label(v)}"' for k, v in sorted(labels.items()) if v != "")
    suffix = f" {timestamp_ms}" if timestamp_ms is not None else ""
    return f"{PREFIX}_{name}{{{rendered}}} {number:g}{suffix}"


def to_prometheus(readings: Iterable[Any], timestamp: bool = False) -> str:
    """Feed readings as a Prometheus exposition body.

    Emits four series per feed — price, age, heartbeat, staleness — with ``HELP`` and
    ``TYPE`` headers so the scrape is self-describing. ``alchem_link_feed_stale == 1`` is
    then a complete alert rule, which is the whole point of this exporter.
    """
    rows = _rows(readings)
    if not rows:
        return ""

    stamp = int(time.time() * 1000) if timestamp else None
    series: Dict[str, List[str]] = {
        "feed_price": [], "feed_age_seconds": [],
        "feed_heartbeat_seconds": [], "feed_stale": [], "feed_carried_round": [],
    }

    for row in rows:
        labels = {
            "pair": row.get("pair", ""),
            "network": row.get("network", ""),
            "address": row.get("address", ""),
        }
        for metric, key in (
            ("feed_price", "price"),
            ("feed_age_seconds", "age_secs"),
            ("feed_heartbeat_seconds", "heartbeat_secs"),
        ):
            if key in row:
                line = _metric(metric, labels, row[key], stamp)
                if line:
                    series[metric].append(line)
        for metric, key in (("feed_stale", "stale"), ("feed_carried_round", "carried_over")):
            if key in row:
                line = _metric(metric, labels, 1 if row[key] else 0, stamp)
                if line:
                    series[metric].append(line)

    help_text = {
        "feed_price": ("Latest answer from the aggregator, scaled by its decimals", "gauge"),
        "feed_age_seconds": ("Seconds since the feed last published", "gauge"),
        "feed_heartbeat_seconds": ("Measured publish interval for this feed", "gauge"),
        "feed_stale": ("1 when the answer is older than its heartbeat plus tolerance", "gauge"),
        "feed_carried_round": ("1 when answeredInRound < roundId — no fresh answer", "gauge"),
    }

    out: List[str] = []
    for metric, lines in series.items():
        if not lines:
            continue
        description, kind = help_text[metric]
        out.append(f"# HELP {PREFIX}_{metric} {description}")
        out.append(f"# TYPE {PREFIX}_{metric} {kind}")
        out.extend(lines)
    return "\n".join(out)


# ── dispatch ─────────────────────────────────────────────────────────────────


def export(items: Iterable[Any], fmt: str = "json",
           columns: Optional[Sequence[str]] = None) -> str:
    """Render ``items`` in one of :data:`FORMATS`.

    A single entry point so the CLI's ``--format`` flag is one lookup rather than a
    branch per command, and so adding a format reaches every command at once.
    """
    materialised = list(items)
    key = fmt.strip().lower()
    if key == "json":
        return to_json(materialised)
    if key == "ndjson":
        return to_ndjson(materialised)
    if key == "csv":
        return to_csv(materialised, columns)
    if key == "prometheus":
        return to_prometheus(materialised)
    if key == "markdown":
        return to_markdown(materialised, columns)
    if key == "table":
        return to_table(materialised, columns)
    raise ValueError(f"unknown format '{fmt}'. Known: {', '.join(FORMATS)}")


def write(items: Iterable[Any], path: str, fmt: Optional[str] = None,
          columns: Optional[Sequence[str]] = None) -> str:
    """Export to a file, inferring the format from the extension when not given."""
    if fmt is None:
        suffix = path.rsplit(".", 1)[-1].lower() if "." in path else "json"
        fmt = {
            "csv": "csv", "ndjson": "ndjson", "jsonl": "ndjson",
            "md": "markdown", "markdown": "markdown", "prom": "prometheus",
            "txt": "table", "json": "json",
        }.get(suffix, "json")
    body = export(items, fmt, columns)
    with open(path, "w", encoding="utf-8", newline="\n") as handle:
        handle.write(body + "\n")
    return path
