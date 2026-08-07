"""Follow a feed and emit one record per new round.

A price printed once is a snapshot. What you usually want to know is *when it changed
and by how much* — whether the feed is publishing on schedule, how large the moves are,
and whether it has gone quiet. This module polls and yields a record only when the round
id advances, so the output is one line per actual publish rather than one line per poll.

Two decisions worth stating:

**The poll interval derives from the feed's own heartbeat.** Polling a Polygon feed
(~60s heartbeat) and an Ethereum one (3600s) at the same rate wastes a rate limit on one
and misses nothing on the other. The default samples several times per heartbeat, which
is enough to catch a deviation-triggered publish without hammering a public endpoint.

**Silence is reported, not implied.** A feed that stops publishing produces no rounds,
which in a change-only stream is indistinguishable from a feed that has not been polled.
Records carry the current age, and once age passes the staleness deadline the record is
flagged — so a stall shows up as an event instead of an absence of them.

Output is JSON Lines by design: one self-contained object per line, so it pipes into
``jq`` or appends to a log without a parser.
"""
from __future__ import annotations

import json
import time
from dataclasses import dataclass, field
from typing import Any, Callable, Dict, Iterator, Optional

from .feeds import FeedReading, get_feed, read_feed
from .networks import DEFAULT_NETWORK
from .rpc import RpcClient, RpcError, RpcTransportError, client_for

#: Polls per heartbeat. Four gives reasonable resolution on a deviation-triggered
#: publish without turning a 60s feed into 60 requests a minute.
POLLS_PER_HEARTBEAT = 4

#: Never poll faster than this, whatever the heartbeat implies.
MIN_INTERVAL_SECS = 5.0

#: Never wait longer than this, so a stall is noticed within a few minutes.
MAX_INTERVAL_SECS = 300.0


def poll_interval_for(heartbeat_secs: int) -> float:
    """Sampling interval for a feed with the given heartbeat."""
    target = heartbeat_secs / POLLS_PER_HEARTBEAT if heartbeat_secs else MAX_INTERVAL_SECS
    return max(MIN_INTERVAL_SECS, min(MAX_INTERVAL_SECS, target))


@dataclass
class WatchEvent:
    """One observation worth emitting."""
    #: ``round`` for a new publish, ``stall`` when the feed passed its deadline,
    #: ``error`` when the read failed.
    kind: str
    pair: str
    network: str
    timestamp: int
    price: Optional[float] = None
    round_id: int = 0
    age_secs: int = 0
    #: Percentage change from the previously observed round.
    change_pct: Optional[float] = None
    #: Seconds since the previously observed round.
    interval_secs: Optional[int] = None
    stale: bool = False
    detail: str = ""

    def as_dict(self) -> Dict[str, Any]:
        payload = {
            "kind": self.kind,
            "pair": self.pair,
            "network": self.network,
            "timestamp": self.timestamp,
            "round_id": self.round_id,
            "price": self.price,
            "age_secs": self.age_secs,
            "stale": self.stale,
        }
        if self.change_pct is not None:
            payload["change_pct"] = round(self.change_pct, 6)
        if self.interval_secs is not None:
            payload["interval_secs"] = self.interval_secs
        if self.detail:
            payload["detail"] = self.detail
        return payload

    def to_json(self) -> str:
        return json.dumps(self.as_dict(), sort_keys=True)


def watch_feed(
    pair: str,
    network: str = DEFAULT_NETWORK,
    client: Optional[RpcClient] = None,
    rpc_url: Optional[str] = None,
    interval: Optional[float] = None,
    max_events: Optional[int] = None,
    duration: Optional[float] = None,
    sleeper: Callable[[float], None] = time.sleep,
    now: Callable[[], int] = lambda: int(time.time()),
) -> Iterator[WatchEvent]:
    """Yield an event per new round until ``max_events`` or ``duration`` is reached.

    ``sleeper`` and ``now`` are injectable so the loop is testable without real time
    passing. The first successful read is always emitted, so a consumer has a baseline
    rather than waiting a whole heartbeat for the first line.
    """
    feed = get_feed(pair, network)
    rpc = client or client_for(network=network, rpc_url=rpc_url)
    wait = interval if interval is not None else poll_interval_for(feed.heartbeat_secs)

    started = now()
    emitted = 0
    last: Optional[FeedReading] = None
    stall_reported = False

    while True:
        if max_events is not None and emitted >= max_events:
            return
        if duration is not None and now() - started >= duration:
            return

        try:
            reading = read_feed(pair, network=network, client=rpc)
        except (RpcError, RpcTransportError) as exc:
            yield WatchEvent(
                kind="error",
                pair=feed.pair,
                network=network.lower(),
                timestamp=now(),
                detail=str(exc),
            )
            emitted += 1
            sleeper(wait)
            continue

        if last is None or reading.round_id != last.round_id:
            change = None
            gap = None
            if last is not None and last.price:
                change = (reading.price - last.price) / abs(last.price) * 100
                gap = reading.updated_at - last.updated_at
            yield WatchEvent(
                kind="round",
                pair=feed.pair,
                network=network.lower(),
                timestamp=now(),
                price=reading.price,
                round_id=reading.round_id,
                age_secs=reading.age_secs,
                change_pct=change,
                interval_secs=gap,
                stale=reading.stale,
            )
            emitted += 1
            last = reading
            stall_reported = False

        elif reading.stale and not stall_reported:
            # Emit once per stall rather than every poll: a stalled feed would otherwise
            # produce an unbounded stream of identical lines.
            yield WatchEvent(
                kind="stall",
                pair=feed.pair,
                network=network.lower(),
                timestamp=now(),
                price=reading.price,
                round_id=reading.round_id,
                age_secs=reading.age_secs,
                stale=True,
                detail=(
                    f"no new round for {reading.age_secs}s, past the "
                    f"{feed.stale_after_secs}s deadline"
                ),
            )
            emitted += 1
            stall_reported = True

        # Check the exit conditions before sleeping, not after. Otherwise the last
        # event is followed by a full poll interval of doing nothing, which on a
        # slow feed means a five-minute pause before the command returns.
        if max_events is not None and emitted >= max_events:
            return
        if duration is not None and now() - started >= duration:
            return

        sleeper(wait)
