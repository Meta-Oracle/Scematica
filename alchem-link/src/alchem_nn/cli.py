"""`alchem-nn` — train and query the feed-behaviour models.

    alchem-nn train -n ethereum          # learn cadence from live round history
    alchem-nn predict ETH/USD -n base    # when will it publish next?
    alchem-nn anomaly -n polygon         # which feeds are behaving unlike themselves
    alchem-nn info                       # what is in the checkpoints

Training pulls round history through `alchem_link` and labels itself: every historical
round knows when the next one arrived. Nothing is fabricated and no dataset is shipped.

Every report prints the trivial-baseline score next to the model's. If the model loses,
that is what it says.
"""
from __future__ import annotations

import argparse
import json
import statistics
import sys
from typing import Any, List, Optional

from alchem_link.aggregator import round_history
from alchem_link.feeds import get_feed, list_feeds
from alchem_link.networks import DEFAULT_NETWORK, get_network
from alchem_link.rpc import RpcError, RpcTransportError, client_for

from . import __version__
from .features import WINDOW, build_features, rounds_to_series
from .model import (
    DEFAULT_ANOMALY_CHECKPOINT,
    DEFAULT_CHECKPOINT,
    AnomalyModel,
    CadenceModel,
    samples_from_rounds,
    split_by_feed,
)

EXIT_OK = 0
EXIT_UNUSABLE = 1
EXIT_USAGE = 2
EXIT_NETWORK = 3


def _configure_output() -> None:
    for stream in (sys.stdout, sys.stderr):
        reconfigure = getattr(stream, "reconfigure", None)
        if reconfigure is not None:
            try:
                reconfigure(encoding="utf-8", errors="replace")
            except (ValueError, OSError):
                pass


def _print_json(payload: Any) -> None:
    print(json.dumps(payload, indent=2, sort_keys=True, default=str))


def _fmt_secs(seconds: float) -> str:
    seconds = int(seconds)
    if seconds < 60:
        return f"{seconds}s"
    if seconds < 3600:
        return f"{seconds // 60}m {seconds % 60}s"
    return f"{seconds // 3600}h {(seconds % 3600) // 60}m"


def _collect(network: str, rounds: int, pairs: Optional[List[str]] = None, quiet: bool = False):
    """Walk round history for the requested feeds and build samples."""
    client = client_for(network=network)
    feeds = [get_feed(p, network) for p in pairs] if pairs else list_feeds(network)
    # Grouped per feed, never pooled here — `split_by_feed` needs the boundaries intact
    # to split each feed's own timeline rather than splitting by feed.
    grouped: List[tuple] = []
    used: List[str] = []

    for feed in feeds:
        try:
            history = round_history(feed.address, count=rounds, network=network, client=client)
        except (RpcError, RpcTransportError) as exc:
            if not quiet:
                print(f"  {feed.pair:<11} skipped — {str(exc)[:60]}", file=sys.stderr)
            continue
        produced = samples_from_rounds(history)
        if produced:
            grouped.append((f"{network}:{feed.pair}", produced))
            used.append(f"{network}:{feed.pair}")
            if not quiet:
                print(f"  {feed.pair:<11} {len(history):>4} rounds → {len(produced):>4} samples")
    return grouped, used


def _cmd_train(args: argparse.Namespace) -> int:
    print(f"collecting round history on {args.network} ({args.rounds} rounds per feed)")
    grouped, used = _collect(args.network, args.rounds, args.pairs)
    train_samples, test_samples = split_by_feed(grouped)
    samples = [s for _name, block in grouped for s in block]

    if len(samples) < 40:
        print(
            f"\nonly {len(samples)} samples — need 40 to train and hold out a test set.\n"
            "Try --rounds 120, or a network with more feeds.",
            file=sys.stderr,
        )
        return EXIT_UNUSABLE

    print(f"\ntraining on {len(train_samples)} samples from {len(used)} feeds "
          f"({len(test_samples)} held out, each feed split by time)")
    model = CadenceModel()
    report = model.fit(
        train_samples, epochs=args.epochs, feeds=used, test_samples=test_samples
    )

    # The autoencoder learns "normal", so it sees everything — there is no future
    # leakage to worry about when the target is the input.
    anomaly = AnomalyModel()
    anomaly_report = anomaly.fit(samples, epochs=args.epochs, feeds=used)

    if not args.dry_run:
        model.save(args.out)
        anomaly.save(args.anomaly_out)

    if args.json:
        _print_json({
            "cadence": report.as_dict(),
            "anomaly": anomaly_report,
            "parameters": model.net.parameters,
            "saved": not args.dry_run,
        })
        return EXIT_OK if report.evaluation.beats_baseline else EXIT_UNUSABLE

    evaluation = report.evaluation
    print(f"\ncadence model   {model.net.parameters} parameters, "
          f"{report.train_samples} train / {report.test_samples} test (per-feed time split)")
    print(f"  model         MAE {_fmt_secs(evaluation.model_mae_secs)} "
          f"({evaluation.model_mae_secs:.0f}s)")
    print(f"  baseline      MAE {_fmt_secs(evaluation.baseline_mae_secs)} "
          f"({evaluation.baseline_mae_secs:.0f}s)   [predict the window median]")
    print(f"  verdict       {evaluation.verdict} ({evaluation.improvement_pct:+.1f}%)")

    print(f"\nanomaly model   {anomaly.net.parameters} parameters, "
          f"median reconstruction error {anomaly_report['median_error']:.4f}")

    if not args.dry_run:
        print(f"\nsaved           {args.out}, {args.anomaly_out}")

    if not evaluation.beats_baseline:
        print(
            "\n  The model does not beat predicting the window median. On feeds whose\n"
            "  cadence is dominated by a fixed heartbeat that is the expected outcome —\n"
            "  there is little left to learn. Reported rather than hidden."
        )
        return EXIT_UNUSABLE
    return EXIT_OK


def _cmd_predict(args: argparse.Namespace) -> int:
    if not args.pair:
        print("usage: alchem-nn predict ETH/USD -n base", file=sys.stderr)
        return EXIT_USAGE

    try:
        model = CadenceModel.load(args.out)
    except FileNotFoundError:
        print(f"error: no model at {args.out} — run `alchem-nn train` first", file=sys.stderr)
        return EXIT_USAGE

    feed = get_feed(args.pair, args.network)
    client = client_for(network=args.network)
    history = round_history(feed.address, count=max(args.rounds, WINDOW + 2),
                            network=args.network, client=client)
    intervals, moves = rounds_to_series(history)

    if len(intervals) < WINDOW:
        print(f"error: only {len(intervals)} intervals of history — need {WINDOW}",
              file=sys.stderr)
        return EXIT_UNUSABLE

    predicted = model.predict_seconds(intervals, moves)
    median = statistics.median(intervals[-WINDOW:])
    newest = max(history, key=lambda r: r.updated_at)

    import time

    age = max(0, int(time.time()) - newest.updated_at)
    remaining = predicted - age

    if args.json:
        _print_json({
            "pair": feed.pair,
            "network": args.network,
            "age_secs": age,
            "predicted_interval_secs": round(predicted, 1),
            "baseline_interval_secs": round(median, 1),
            "predicted_remaining_secs": round(remaining, 1),
            "overdue": remaining < 0,
            "declared_heartbeat": feed.heartbeat_secs,
        })
        return EXIT_OK

    print(f"{feed.pair}  ({args.network})")
    print(f"  last publish     {_fmt_secs(age)} ago")
    print(f"  model predicts   {_fmt_secs(predicted)} between publishes")
    print(f"  baseline (median) {_fmt_secs(median)}")
    if remaining >= 0:
        print(f"  next expected in ~{_fmt_secs(remaining)}")
    else:
        print(f"  OVERDUE by ~{_fmt_secs(-remaining)} against the model's estimate")
    print(f"  declared heartbeat {_fmt_secs(feed.heartbeat_secs)}")
    return EXIT_OK


def _cmd_anomaly(args: argparse.Namespace) -> int:
    try:
        model = AnomalyModel.load(args.anomaly_out)
    except FileNotFoundError:
        print(f"error: no model at {args.anomaly_out} — run `alchem-nn train` first",
              file=sys.stderr)
        return EXIT_USAGE

    client = client_for(network=args.network)
    feeds = [get_feed(p, args.network) for p in args.pairs] if args.pairs \
        else list_feeds(args.network)

    results = []
    for feed in feeds:
        try:
            history = round_history(feed.address, count=args.rounds,
                                    network=args.network, client=client)
        except (RpcError, RpcTransportError) as exc:
            results.append({"pair": feed.pair, "error": str(exc)[:80]})
            continue
        intervals, moves = rounds_to_series(history)
        if len(intervals) < WINDOW:
            results.append({"pair": feed.pair, "error": "not enough history"})
            continue
        score = model.score(build_features(intervals, moves))
        results.append({"pair": feed.pair, **score})

    if args.json:
        _print_json(results)
        return EXIT_UNUSABLE if any(r.get("anomalous") for r in results) else EXIT_OK

    flagged = 0
    for entry in sorted(results, key=lambda r: -(r.get("percentile") or 0)):
        if entry.get("error"):
            print(f"  {entry['pair']:<11} unreadable — {entry['error']}")
            continue
        mark = "FLAG" if entry["anomalous"] else "ok  "
        if entry["anomalous"]:
            flagged += 1
        print(f"  [{mark}] {entry['pair']:<11} p{entry['percentile']:>5.1f}  {entry['detail']}")
    print(f"\n{flagged}/{len(results)} feeds flagged as behaving unlike their own history")
    return EXIT_UNUSABLE if flagged else EXIT_OK


def _cmd_info(args: argparse.Namespace) -> int:
    payload: dict = {"version": __version__}
    for label, path, loader in (
        ("cadence", args.out, CadenceModel),
        ("anomaly", args.anomaly_out, AnomalyModel),
    ):
        try:
            model = loader.load(path)
            payload[label] = {
                "path": path,
                "parameters": model.net.parameters,
                "train_steps": model.net.steps,
                "layers": model.net.sizes,
                "trained_on": model.trained_on,
            }
        except FileNotFoundError:
            payload[label] = {"path": path, "status": "not trained"}
        except ValueError as exc:
            payload[label] = {"path": path, "status": str(exc)}

    if args.json:
        _print_json(payload)
        return EXIT_OK

    print(f"alchem-nn {__version__}")
    for label in ("cadence", "anomaly"):
        entry = payload[label]
        print(f"\n{label}")
        if "status" in entry:
            print(f"  {entry['status']}  ({entry['path']})")
            continue
        print(f"  {entry['parameters']} parameters  {'→'.join(map(str, entry['layers']))}")
        print(f"  {entry['train_steps']} training steps")
        print(f"  trained on {len(entry['trained_on'])} feeds: "
              f"{', '.join(entry['trained_on'][:6])}"
              f"{' …' if len(entry['trained_on']) > 6 else ''}")
    return EXIT_OK


HANDLERS = {
    "train": _cmd_train,
    "predict": _cmd_predict,
    "anomaly": _cmd_anomaly,
    "info": _cmd_info,
}


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        prog="alchem-nn",
        description="Neural feed-behaviour models for alchem-link — cadence prediction "
                    "and anomaly detection, trained on live round history",
    )
    parser.add_argument("--version", action="version", version=f"alchem-nn {__version__}")
    subparsers = parser.add_subparsers(dest="command", metavar="<command>")

    for name, help_text in (
        ("train", "Learn cadence from live round history"),
        ("predict", "Predict when a feed will publish next"),
        ("anomaly", "Score feeds against their own learned rhythm"),
        ("info", "What is in the checkpoints"),
    ):
        sub = subparsers.add_parser(name, help=help_text, description=help_text)
        sub.add_argument("-n", "--network", default=DEFAULT_NETWORK)
        sub.add_argument("--json", action="store_true")
        sub.add_argument("--out", default=DEFAULT_CHECKPOINT, help="Cadence checkpoint")
        sub.add_argument("--anomaly-out", default=DEFAULT_ANOMALY_CHECKPOINT,
                         help="Anomaly checkpoint")
        if name in ("train", "anomaly"):
            sub.add_argument("--pairs", nargs="*", help="Specific pairs (default: all)")
        if name == "predict":
            sub.add_argument("pair", nargs="?", help="Feed pair, e.g. ETH/USD")
        if name in ("train", "predict", "anomaly"):
            sub.add_argument("--rounds", type=int, default=80,
                             help="Rounds of history per feed (default: 80)")
        if name == "train":
            sub.add_argument("--epochs", type=int, default=60)
            sub.add_argument("--dry-run", action="store_true",
                             help="Train and report without writing checkpoints")
    return parser


def main(argv: Optional[List[str]] = None) -> int:
    _configure_output()
    args = build_parser().parse_args(argv)
    if not args.command:
        build_parser().print_help()
        return EXIT_OK
    try:
        return int(HANDLERS[args.command](args) or EXIT_OK)
    except (KeyError, ValueError) as exc:
        print(f"error: {exc}", file=sys.stderr)
        return EXIT_USAGE
    except RpcTransportError as exc:
        print(f"network error: {exc}", file=sys.stderr)
        return EXIT_NETWORK
    except KeyboardInterrupt:
        return 130


if __name__ == "__main__":
    raise SystemExit(main())
