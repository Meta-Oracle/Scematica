"""Measure a feed's publish behaviour instead of trusting a table.

Every Chainlink feed has two documented triggers, and it publishes when *either* fires:

* **Heartbeat** — publish at least this often, even if nothing moved.
* **Deviation threshold** — publish immediately if the price moves more than this,
  however recently the last update was.

Registries record those numbers as constants, and constants go out of date. Feeds get
reconfigured, migrated between aggregators, and deprecated without anybody updating the
table you copied them from — and a heartbeat that is wrong in the optimistic direction
means a staleness check that never fires.

Both parameters are recoverable from round history, because the two triggers leave
different fingerprints:

* Intervals pile up against a ceiling. That ceiling is the real heartbeat — the longest
  the feed will stay quiet when the price is flat.
* Any update arriving *well before* the ceiling was deviation-triggered. The smallest
  price move among those is an upper bound on the deviation threshold: it was enough to
  fire, so the real threshold is no larger.

For mainnet ETH/USD this recovers roughly a one-hour heartbeat and a sub-1% deviation
threshold, which is what Chainlink documents — derived from chain state rather than
read off a page.

Block timestamps make intervals jittery by a few seconds, so the heartbeat estimate is
quantised to a sensible unit rather than reported as ``3624s``.
"""
from __future__ import annotations

import statistics
import time
from dataclasses import dataclass, field
from typing import Any, Dict, List, Optional

from .aggregator import Round, round_history
from .feeds import get_feed
from .networks import DEFAULT_NETWORK
from .rpc import RpcClient, client_for

#: Intervals shorter than this fraction of the observed ceiling were deviation-triggered
#: rather than heartbeat-triggered. Loose enough to absorb block-timestamp jitter.
DEVIATION_INTERVAL_RATIO = 0.9

#: Candidate heartbeats, in seconds. Feeds are configured in round units; snapping to
#: this ladder turns "3624s" into "1h" instead of implying false precision.
COMMON_HEARTBEATS = (60, 120, 300, 600, 900, 1200, 1800, 3600, 7200, 14400, 21600, 43200, 86400)

#: Below this many intervals the estimates are noise and are reported as such.
MIN_SAMPLES = 4


def _snap_heartbeat(seconds: float) -> int:
    """Snap a measured ceiling to the nearest plausible configured heartbeat."""
    if seconds <= 0:
        return 0
    return min(COMMON_HEARTBEATS, key=lambda candidate: abs(candidate - seconds))


@dataclass
class CadenceProfile:
    """What the round history says about how a feed actually behaves."""
    pair: str
    network: str
    address: str
    samples: int
    intervals: List[int] = field(default_factory=list)
    deviations_pct: List[float] = field(default_factory=list)
    declared_heartbeat: int = 0
    observed_ceiling_secs: int = 0
    observed_heartbeat: int = 0
    median_interval: int = 0
    deviation_triggered: int = 0
    heartbeat_triggered: int = 0
    inferred_deviation_pct: Optional[float] = None
    largest_move_pct: float = 0.0
    window_secs: int = 0
    current_age_secs: int = 0

    @property
    def confident(self) -> bool:
        return self.samples >= MIN_SAMPLES

    @property
    def heartbeat_observed(self) -> bool:
        """Did the window actually contain a heartbeat publish?

        The max interval only equals the heartbeat if the price was quiet long enough for
        the clock, rather than a price move, to trigger a publish. On a fast L2 feed with
        a tight deviation threshold that may never happen: Arbitrum ETH/USD was measured
        at 28 of 29 rounds deviation-triggered over a 49-minute window, whose 451s maximum
        says nothing about its heartbeat except that it is *at least* that.

        Requiring two rounds to have landed at the ceiling is what separates "the feed
        waited this long" from "the window ended".
        """
        return self.heartbeat_triggered >= 2

    @property
    def heartbeat_verdict(self) -> str:
        """Whether the declared heartbeat matches what the feed does."""
        if not self.confident or not self.declared_heartbeat:
            return "unknown"
        if not self.heartbeat_observed:
            return "not observed"
        if self.observed_heartbeat > self.declared_heartbeat:
            return "declared too tight"
        if self.observed_heartbeat < self.declared_heartbeat:
            return "declared too loose"
        return "matches"

    @property
    def verdict_detail(self) -> str:
        verdict = self.heartbeat_verdict
        if verdict == "unknown":
            return f"only {self.samples} interval(s) — not enough history to judge"
        if verdict == "not observed":
            return (
                f"every round in this {self.window_secs}s window was deviation-triggered, "
                f"so the heartbeat was never exercised — it is at least "
                f"{self.observed_ceiling_secs}s, and the declared {self.declared_heartbeat}s "
                "is not contradicted. Widen the window to measure it."
            )
        if verdict == "matches":
            return f"declared {self.declared_heartbeat}s matches the observed {self.observed_heartbeat}s"
        if verdict == "declared too tight":
            return (
                f"declared {self.declared_heartbeat}s, but the feed has gone "
                f"{self.observed_ceiling_secs}s between publishes — a staleness check at "
                "the declared value will fire on healthy rounds"
            )
        return (
            f"declared {self.declared_heartbeat}s, but the feed publishes at least every "
            f"{self.observed_heartbeat}s — a staleness check at the declared value is "
            "looser than it needs to be, and will miss a genuinely stalled feed"
        )

    @property
    def stalled(self) -> bool:
        """True when the current round is already older than the observed ceiling."""
        return bool(self.observed_ceiling_secs) and self.current_age_secs > self.observed_ceiling_secs

    def as_dict(self) -> Dict[str, Any]:
        return {
            "pair": self.pair,
            "network": self.network,
            "address": self.address,
            "samples": self.samples,
            "confident": self.confident,
            "declared_heartbeat": self.declared_heartbeat,
            "observed_heartbeat": self.observed_heartbeat,
            "observed_ceiling_secs": self.observed_ceiling_secs,
            "median_interval": self.median_interval,
            "heartbeat_verdict": self.heartbeat_verdict,
            "verdict_detail": self.verdict_detail,
            "deviation_triggered": self.deviation_triggered,
            "heartbeat_triggered": self.heartbeat_triggered,
            "inferred_deviation_pct": self.inferred_deviation_pct,
            "largest_move_pct": self.largest_move_pct,
            "window_secs": self.window_secs,
            "current_age_secs": self.current_age_secs,
            "stalled": self.stalled,
        }


def profile_rounds(
    rounds: List[Round],
    declared_heartbeat: int = 0,
    pair: str = "",
    network: str = "",
    address: str = "",
    now: Optional[int] = None,
) -> CadenceProfile:
    """Derive a cadence profile from round history. Pure — no I/O, so it is testable.

    ``rounds`` is newest-first, as :func:`alchem_link.aggregator.round_history` returns it.
    """
    current = int(time.time()) if now is None else now
    ordered = sorted(rounds, key=lambda r: r.updated_at)

    profile = CadenceProfile(
        pair=pair,
        network=network,
        address=address,
        samples=0,
        declared_heartbeat=declared_heartbeat,
    )
    if len(ordered) < 2:
        if ordered:
            profile.current_age_secs = max(0, current - ordered[-1].updated_at)
        return profile

    intervals: List[int] = []
    moves: List[float] = []
    for previous, following in zip(ordered, ordered[1:]):
        gap = following.updated_at - previous.updated_at
        if gap <= 0:
            continue  # duplicate or non-monotonic timestamp; nothing to learn from it
        intervals.append(gap)
        if previous.price:
            moves.append(abs(following.price - previous.price) / abs(previous.price) * 100)
        else:
            moves.append(0.0)

    if not intervals:
        return profile

    profile.samples = len(intervals)
    profile.intervals = intervals
    profile.deviations_pct = moves
    profile.window_secs = ordered[-1].updated_at - ordered[0].updated_at
    profile.current_age_secs = max(0, current - ordered[-1].updated_at)
    profile.median_interval = int(statistics.median(intervals))
    profile.observed_ceiling_secs = max(intervals)
    profile.largest_move_pct = max(moves) if moves else 0.0

    # An update that landed well inside the ceiling was triggered by price movement, not
    # by the clock.
    cutoff = profile.observed_ceiling_secs * DEVIATION_INTERVAL_RATIO
    early_moves = [
        move for gap, move in zip(intervals, moves) if gap < cutoff and move > 0
    ]
    profile.deviation_triggered = sum(1 for gap in intervals if gap < cutoff)
    profile.heartbeat_triggered = profile.samples - profile.deviation_triggered

    # Only snap to a configured heartbeat when one was actually exercised. Snapping an
    # unexercised ceiling manufactures a precise-looking number out of where the window
    # happened to end.
    profile.observed_heartbeat = (
        _snap_heartbeat(profile.observed_ceiling_secs)
        if profile.heartbeat_triggered >= 2
        else 0
    )

    if early_moves:
        # The smallest move that was nonetheless enough to publish bounds the threshold
        # from above: the real setting cannot be larger than this.
        profile.inferred_deviation_pct = round(min(early_moves), 4)

    return profile


def profile_feed(
    pair: str,
    network: str = DEFAULT_NETWORK,
    rounds: int = 24,
    client: Optional[RpcClient] = None,
    rpc_url: Optional[str] = None,
    address: Optional[str] = None,
    declared_heartbeat: Optional[int] = None,
    now: Optional[int] = None,
) -> CadenceProfile:
    """Read a feed's round history and profile its cadence."""
    rpc = client or client_for(network=network, rpc_url=rpc_url)

    if address is None:
        feed = get_feed(pair, network)
        address = feed.address
        declared = feed.heartbeat_secs if declared_heartbeat is None else declared_heartbeat
    else:
        declared = declared_heartbeat or 0

    history = round_history(address, count=rounds, network=network, client=rpc)
    return profile_rounds(
        history,
        declared_heartbeat=declared,
        pair=pair,
        network=network.lower(),
        address=address,
        now=now,
    )
