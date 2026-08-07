"""The same pair, read on every chain at once, and what the disagreement means.

ETH/USD is not one number. It is a separate oracle deployment per chain, each with its
own aggregator, its own node set, and its own deviation threshold — and they do not
agree. Usually the gap is single-digit basis points and means nothing. Sometimes it is
large, and then it means one of three quite different things:

* **A stale leg.** One chain's feed has not published recently and is quoting an old
  price. Nothing is broken; the number is just old, and anything reading both chains is
  comparing two different moments.
* **A tight-threshold chain leading a loose one.** A feed with a 0.05% deviation
  threshold tracks a fast move that a 0.5% feed has not published yet. Both are behaving
  exactly as configured. This is normal and self-correcting.
* **A genuinely broken leg.** Everything else.

Anything holding positions across chains — a bridge, a cross-chain money market, an
arbitrage bot — is exposed to the difference regardless of which cause is operating.
This module measures it, and attributes it, so the three do not get confused.

**Consensus excludes stale legs.** A feed past its heartbeat is not evidence about the
current price, and letting it drag the median would hide the very outlier it is. Stale
legs are still reported, marked, and measured against the consensus of the fresh ones.

One caveat this module cannot engineer away: these are separate chains, so the reads are
not simultaneous. A few hundred milliseconds of skew is single-digit bps of noise on a
volatile pair. Treat small divergences as noise; the threshold defaults accordingly.
"""
from __future__ import annotations

import statistics
import time
from dataclasses import dataclass, field
from typing import Any, Dict, List, Optional

from .feeds import FEEDS, FeedReading, decode_reading, get_feed, list_feeds
from .networks import list_networks
from .rpc import RpcClient, client_for

#: Below this, a gap is indistinguishable from read skew and threshold lag.
DEFAULT_OUTLIER_BPS = 50.0

#: One basis point is 0.01%.
BPS = 10_000.0


@dataclass
class Leg:
    """One chain's answer for the pair."""
    network: str
    address: str
    description: str
    price: float
    age_secs: int
    heartbeat_secs: int
    stale: bool
    #: Signed deviation from consensus, in basis points. Positive means this chain quotes
    #: higher than the rest.
    deviation_bps: float = 0.0
    error: str = ""

    @property
    def ok(self) -> bool:
        return not self.error

    def as_dict(self) -> Dict[str, Any]:
        return {
            "network": self.network,
            "address": self.address,
            "description": self.description,
            "price": self.price,
            "age_secs": self.age_secs,
            "heartbeat_secs": self.heartbeat_secs,
            "stale": self.stale,
            "deviation_bps": round(self.deviation_bps, 2),
            "error": self.error,
        }


@dataclass
class DivergenceReport:
    """Cross-chain agreement for one pair."""
    pair: str
    legs: List[Leg] = field(default_factory=list)
    consensus: Optional[float] = None
    outlier_bps: float = DEFAULT_OUTLIER_BPS

    @property
    def readable(self) -> List[Leg]:
        return [leg for leg in self.legs if leg.ok]

    @property
    def fresh(self) -> List[Leg]:
        return [leg for leg in self.readable if not leg.stale]

    @property
    def spread_bps(self) -> float:
        """Widest gap between any two *fresh* legs, in bps."""
        prices = [leg.price for leg in self.fresh if leg.price > 0]
        if len(prices) < 2:
            return 0.0
        low, high = min(prices), max(prices)
        return (high - low) / low * BPS

    @property
    def outliers(self) -> List[Leg]:
        return [leg for leg in self.readable if abs(leg.deviation_bps) > self.outlier_bps]

    @property
    def verdict(self) -> str:
        if len(self.fresh) < 2:
            return "insufficient"
        if not self.outliers:
            return "agree"
        return "diverged"

    @property
    def detail(self) -> str:
        if len(self.fresh) < 2:
            return (
                f"{len(self.fresh)} fresh leg(s) — at least two chains must carry the "
                "pair for a comparison to mean anything"
            )
        if self.verdict == "agree":
            # The threshold is per-leg against consensus, so the widest pairwise spread
            # can legitimately reach twice it. Reporting the spread against the per-leg
            # threshold reads as a contradiction ("agree within 58 bps, threshold 50"),
            # so name both for what they are.
            worst = max((abs(leg.deviation_bps) for leg in self.readable), default=0.0)
            return (
                f"{len(self.fresh)} chains agree — worst leg {worst:.1f} bps from "
                f"consensus (threshold {self.outlier_bps:.0f}), widest pairwise spread "
                f"{self.spread_bps:.1f} bps"
            )

        parts = []
        for leg in sorted(self.outliers, key=lambda leg: -abs(leg.deviation_bps)):
            cause = (
                f"stale by {leg.age_secs}s against a {leg.heartbeat_secs}s heartbeat"
                if leg.stale
                else f"fresh ({leg.age_secs}s old) — not explained by staleness"
            )
            parts.append(f"{leg.network} {leg.deviation_bps:+.1f} bps, {cause}")
        return "; ".join(parts)

    def as_dict(self) -> Dict[str, Any]:
        return {
            "pair": self.pair,
            "consensus": self.consensus,
            "spread_bps": round(self.spread_bps, 2),
            "outlier_bps": self.outlier_bps,
            "verdict": self.verdict,
            "detail": self.detail,
            "networks": len(self.readable),
            "fresh": len(self.fresh),
            "legs": [leg.as_dict() for leg in self.legs],
        }


def networks_carrying(pair: str, include_testnets: bool = False) -> List[str]:
    """Every registered network with a feed for ``pair``.

    Testnets are excluded by default and this is not a convenience. Sepolia's ETH/USD is
    a test deployment fed by a separate node set; it tracks mainnet loosely and drifts
    freely. Including it in a consensus produces a median that describes neither network,
    and it showed up as the widest "divergence" in every early run of this module.
    """
    key = pair.upper().replace("-", "/").strip()
    return [
        net.key
        for net in list_networks()
        if key in FEEDS.get(net.key, {}) and (include_testnets or not net.testnet)
    ]


def common_pairs(minimum_networks: int = 2, include_testnets: bool = False) -> List[str]:
    """Pairs carried on at least ``minimum_networks`` chains — the comparable ones."""
    counts: Dict[str, int] = {}
    for net in list_networks():
        if not include_testnets and net.testnet:
            continue
        for feed in list_feeds(net.key):
            counts[feed.pair] = counts.get(feed.pair, 0) + 1
    return sorted(pair for pair, count in counts.items() if count >= minimum_networks)


def compare_pair(
    pair: str,
    networks: Optional[List[str]] = None,
    clients: Optional[Dict[str, RpcClient]] = None,
    outlier_bps: float = DEFAULT_OUTLIER_BPS,
    now: Optional[int] = None,
) -> DivergenceReport:
    """Read ``pair`` on every chain that carries it and measure the disagreement.

    Each chain needs its own endpoint, so this is one connection per network — the one
    place in the package where round trips genuinely cannot be collapsed.
    """
    targets = networks or networks_carrying(pair)
    report = DivergenceReport(pair=pair.upper().replace("-", "/").strip(), outlier_bps=outlier_bps)
    pool = clients if clients is not None else {}
    current = int(time.time()) if now is None else now

    for network in targets:
        try:
            feed = get_feed(pair, network)
        except KeyError:
            continue

        rpc = pool.get(network) or client_for(network=network)
        pool.setdefault(network, rpc)

        try:
            reading: FeedReading = decode_reading(
                feed, network.lower(), rpc.read_aggregator(feed.address), now=current
            )
        except Exception as exc:
            report.legs.append(Leg(
                network=network,
                address=feed.address,
                description="",
                price=0.0,
                age_secs=0,
                heartbeat_secs=feed.heartbeat_secs,
                stale=True,
                error=str(exc),
            ))
            continue

        report.legs.append(Leg(
            network=network,
            address=feed.address,
            description=reading.description,
            price=reading.price,
            age_secs=reading.age_secs,
            heartbeat_secs=reading.heartbeat_secs,
            stale=reading.stale,
        ))

    # Consensus is the median of the fresh legs. Median rather than mean so a single
    # broken leg cannot move the reference it is being measured against.
    fresh_prices = [leg.price for leg in report.fresh if leg.price > 0]
    if fresh_prices:
        report.consensus = statistics.median(fresh_prices)
        for leg in report.readable:
            if leg.price > 0 and report.consensus:
                leg.deviation_bps = (leg.price - report.consensus) / report.consensus * BPS

    return report


def compare_all(
    pairs: Optional[List[str]] = None,
    outlier_bps: float = DEFAULT_OUTLIER_BPS,
    now: Optional[int] = None,
) -> List[DivergenceReport]:
    """Compare every multi-chain pair, widest divergence first.

    Clients are shared across pairs, so each network is connected to once rather than
    once per pair.
    """
    pool: Dict[str, RpcClient] = {}
    reports = [
        compare_pair(pair, clients=pool, outlier_bps=outlier_bps, now=now)
        for pair in (pairs or common_pairs())
    ]
    return sorted(reports, key=lambda r: -r.spread_bps)
