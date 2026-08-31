"""Statistics over a feed's history: TWAP, volatility, drawdown, correlation.

Pure functions over ``(timestamp, price)`` points. No I/O, which means every number here
is reproducible from a fixture and the whole module is testable offline — a property
worth protecting, because these are the numbers people size positions with.

Two decisions run through all of it.

**Time-weighted, not sample-weighted.** An oracle publishes irregularly: on a heartbeat
when nothing happens, and immediately when the price moves. The mean of the published
answers therefore over-weights volatile periods, because that is exactly when the feed
publishes most. A price that sat at 1,900 for fifty minutes and then printed six rounds
walking to 1,950 has a *sample* mean near 1,940 and a *time-weighted* mean near 1,905.
The second one is what a TWAP oracle would have reported, so :func:`twap` weights each
observation by how long it stood.

**Volatility is scaled by measured spacing, not assumed.** Annualising a standard
deviation requires knowing the sampling interval. Assuming a fixed one is how the same
feed reports wildly different volatility on Polygon (60s publishes) and Ethereum (3600s)
despite tracking the same asset. :func:`volatility` derives the interval from the
timestamps it was given.
"""
from __future__ import annotations

import math
import statistics
from dataclasses import dataclass, field
from typing import Any, Dict, Iterable, List, Optional, Sequence, Tuple

#: Seconds in a year, for annualising. 365 days — crypto does not close at weekends.
SECONDS_PER_YEAR = 365 * 24 * 3600


@dataclass(frozen=True)
class Point:
    """One observation: when the feed said it, and what it said."""

    timestamp: int
    price: float

    def as_dict(self) -> Dict[str, Any]:
        return {"timestamp": self.timestamp, "price": self.price}


@dataclass
class Series:
    """An ordered price history for one feed.

    Construction sorts and de-duplicates by timestamp. Both matter: round history arrives
    newest-first, log history arrives oldest-first, and a feed that publishes twice in
    one second yields two points with an interval of zero that would divide by zero in
    every rate calculation downstream.
    """

    pair: str = ""
    network: str = ""
    points: List[Point] = field(default_factory=list)

    def __post_init__(self) -> None:
        seen: Dict[int, Point] = {}
        for point in self.points:
            seen[point.timestamp] = point  # a later duplicate wins
        self.points = [seen[t] for t in sorted(seen)]

    def __len__(self) -> int:
        return len(self.points)

    def __iter__(self):
        return iter(self.points)

    @property
    def prices(self) -> List[float]:
        return [p.price for p in self.points]

    @property
    def timestamps(self) -> List[int]:
        return [p.timestamp for p in self.points]

    @property
    def span_secs(self) -> int:
        return self.points[-1].timestamp - self.points[0].timestamp if len(self) > 1 else 0

    @property
    def first(self) -> Optional[Point]:
        return self.points[0] if self.points else None

    @property
    def last(self) -> Optional[Point]:
        return self.points[-1] if self.points else None

    def window(self, seconds: float) -> "Series":
        """The tail of the series covering the last ``seconds``."""
        if not self.points:
            return Series(self.pair, self.network, [])
        cutoff = self.points[-1].timestamp - seconds
        return Series(self.pair, self.network, [p for p in self.points if p.timestamp >= cutoff])

    def as_dict(self) -> Dict[str, Any]:
        return {
            "pair": self.pair,
            "network": self.network,
            "points": [p.as_dict() for p in self.points],
            "count": len(self),
            "span_secs": self.span_secs,
        }

    # ── constructors ─────────────────────────────────────────────────────────

    @classmethod
    def from_rounds(cls, rounds: Sequence[Any], pair: str = "", network: str = "") -> "Series":
        """From :class:`alchem_link.aggregator.Round` objects (any order)."""
        return cls(pair, network, [
            Point(timestamp=int(r.updated_at), price=float(r.price))
            for r in rounds if getattr(r, "updated_at", 0) > 0
        ])

    @classmethod
    def from_updates(cls, updates: Sequence[Any], pair: str = "", network: str = "") -> "Series":
        """From :class:`alchem_link.logs.AnswerUpdate` records."""
        return cls(pair, network, [
            Point(timestamp=int(u.updated_at), price=float(u.price))
            for u in updates if getattr(u, "updated_at", 0) > 0
        ])

    @classmethod
    def from_pairs(cls, points: Iterable[Tuple[int, float]], pair: str = "",
                   network: str = "") -> "Series":
        return cls(pair, network, [Point(int(t), float(p)) for t, p in points])


# ── averages ─────────────────────────────────────────────────────────────────


def twap(series: Series, window_secs: Optional[float] = None) -> Optional[float]:
    """Time-weighted average price: each observation weighted by how long it stood.

    This is the average a TWAP oracle would have reported, and it is materially different
    from the mean of the answers whenever publishing is irregular — which, for a feed with
    a deviation threshold, is always. See the module docstring.
    """
    target = series.window(window_secs) if window_secs else series
    points = target.points
    if not points:
        return None
    if len(points) == 1:
        return points[0].price

    weighted = 0.0
    duration = 0.0
    for current, following in zip(points, points[1:]):
        gap = following.timestamp - current.timestamp
        if gap <= 0:
            continue
        weighted += current.price * gap
        duration += gap
    if duration <= 0:
        return statistics.fmean(p.price for p in points)
    return weighted / duration


def mean_price(series: Series) -> Optional[float]:
    """Plain arithmetic mean of the answers. Provided for comparison against :func:`twap`."""
    return statistics.fmean(series.prices) if series.points else None


def median_price(series: Series) -> Optional[float]:
    return statistics.median(series.prices) if series.points else None


# ── returns and risk ─────────────────────────────────────────────────────────


def log_returns(series: Series) -> List[float]:
    """Natural-log returns between consecutive observations.

    Log rather than simple returns because they are additive over time, which is what
    makes the square-root-of-time scaling in :func:`volatility` valid. Non-positive
    prices are skipped — an oracle reporting zero is a fault, not a −100% return.
    """
    out: List[float] = []
    for previous, current in zip(series.points, series.points[1:]):
        if previous.price > 0 and current.price > 0:
            out.append(math.log(current.price / previous.price))
    return out


def simple_returns(series: Series) -> List[float]:
    """Percentage change between consecutive observations, as fractions."""
    return [
        (current.price - previous.price) / previous.price
        for previous, current in zip(series.points, series.points[1:])
        if previous.price > 0
    ]


def median_interval(series: Series) -> Optional[float]:
    """Median seconds between publishes. The sampling interval volatility scales by.

    Median rather than mean: one long quiet gap in an otherwise fast series would drag a
    mean far from anything the feed actually does.
    """
    gaps = [
        following.timestamp - current.timestamp
        for current, following in zip(series.points, series.points[1:])
        if following.timestamp > current.timestamp
    ]
    return statistics.median(gaps) if gaps else None


def volatility(series: Series, annualise: bool = True) -> Optional[float]:
    """Standard deviation of log returns, optionally annualised.

    Annualisation scales by the square root of the number of measured intervals in a
    year, derived from the series' own median spacing rather than assumed — see the
    module docstring for why that matters across chains.
    """
    returns = log_returns(series)
    if len(returns) < 2:
        return None
    sigma = statistics.stdev(returns)
    if not annualise:
        return sigma
    interval = median_interval(series)
    if not interval or interval <= 0:
        return sigma
    return sigma * math.sqrt(SECONDS_PER_YEAR / interval)


def max_drawdown(series: Series) -> Dict[str, Any]:
    """Largest peak-to-trough decline in the window.

    Returns the depth as a fraction plus where it happened, because "8% drawdown" is far
    less useful than "8%, from the peak at 14:02 to the trough at 14:19" when you are
    trying to work out whether an oracle tracked a real move or glitched.
    """
    if len(series) < 2:
        # `None`, not `0.0`. A single print cannot decline, so there is no drawdown to
        # report — and `0.0` here is a claim the price held, made from one observation. The
        # keys are the same as the measured branch: an early return that omitted
        # `drawdown_pct` made `summarise` raise `KeyError` on every one-point series, which
        # is a perfectly ordinary thing for a slow feed to produce over a short window.
        return {
            "drawdown": None,
            "drawdown_pct": None,
            "peak": None,
            "trough": None,
            "recovered": None,
        }

    peak = series.points[0]
    worst = 0.0
    worst_peak = peak
    worst_trough = peak
    for point in series.points[1:]:
        if point.price > peak.price:
            peak = point
            continue
        if peak.price <= 0:
            continue
        decline = (peak.price - point.price) / peak.price
        if decline > worst:
            worst, worst_peak, worst_trough = decline, peak, point
    return {
        "drawdown": worst,
        "drawdown_pct": worst * 100,
        "peak": worst_peak.as_dict() if worst else None,
        "trough": worst_trough.as_dict() if worst else None,
        # Recovery is measured against the peak, not the trough: the question is whether
        # the price got back to where it fell from.
        "recovered": bool(worst) and series.points[-1].price >= worst_peak.price,
    }


def largest_move(series: Series) -> Dict[str, Any]:
    """The biggest single-publish jump, in basis points.

    Worth surfacing on its own: a feed's largest observed move bounds its deviation
    threshold from below, and an outlier here is usually either a real market event or a
    bad round, both of which you want to look at.
    """
    if len(series) < 2:
        # No pair, so no move was observed. Distinguished from a genuinely flat window,
        # which also leaves `at` unset but whose zero is a real measurement — which is why
        # this tests the length rather than `at`.
        return {"move_bps": None, "move_pct": None, "from": None, "to": None}

    best = 0.0
    at: Optional[Tuple[Point, Point]] = None
    for previous, current in zip(series.points, series.points[1:]):
        if previous.price <= 0:
            continue
        move = abs(current.price - previous.price) / previous.price
        if move > best:
            best, at = move, (previous, current)
    return {
        "move_bps": best * 10_000,
        "move_pct": best * 100,
        "from": at[0].as_dict() if at else None,
        "to": at[1].as_dict() if at else None,
    }


# ── cross-series ─────────────────────────────────────────────────────────────


def align(left: Series, right: Series, tolerance_secs: float = 300.0) -> List[Tuple[Point, Point]]:
    """Pair observations from two series that fall within ``tolerance_secs``.

    Necessary because two feeds never publish on the same clock — even the same pair on
    two chains. Naively zipping them correlates observations minutes apart and produces a
    number that means nothing. Each left point takes its nearest right point, and a right
    point is used at most once.
    """
    pairs: List[Tuple[Point, Point]] = []
    used: set = set()
    right_points = right.points
    if not right_points:
        return pairs

    for point in left.points:
        best_index = None
        best_gap = tolerance_secs + 1
        for index, candidate in enumerate(right_points):
            if index in used:
                continue
            gap = abs(candidate.timestamp - point.timestamp)
            if gap < best_gap:
                best_gap, best_index = gap, index
            elif candidate.timestamp > point.timestamp + tolerance_secs:
                break  # sorted, so nothing further can be closer
        if best_index is not None and best_gap <= tolerance_secs:
            used.add(best_index)
            pairs.append((point, right_points[best_index]))
    return pairs


def correlation(left: Series, right: Series, tolerance_secs: float = 300.0) -> Optional[float]:
    """Pearson correlation of the two series' log returns over aligned observations."""
    pairs = align(left, right, tolerance_secs)
    if len(pairs) < 3:
        return None
    left_returns: List[float] = []
    right_returns: List[float] = []
    for (l_prev, r_prev), (l_now, r_now) in zip(pairs, pairs[1:]):
        if min(l_prev.price, r_prev.price, l_now.price, r_now.price) <= 0:
            continue
        left_returns.append(math.log(l_now.price / l_prev.price))
        right_returns.append(math.log(r_now.price / r_prev.price))
    if len(left_returns) < 2:
        return None
    try:
        return statistics.correlation(left_returns, right_returns)
    except (statistics.StatisticsError, ValueError):
        # Zero variance on either side — a feed that did not move has no correlation to
        # report, which is different from a correlation of zero.
        return None


def spread_bps(left: Series, right: Series, tolerance_secs: float = 300.0) -> Dict[str, Any]:
    """How far apart two series ran, in basis points, over aligned observations.

    The cross-chain question in its measured form: not "do these differ right now" but
    "how far apart have they been, and how far apart do they usually sit".
    """
    pairs = align(left, right, tolerance_secs)
    if not pairs:
        return {"samples": 0, "mean_bps": None, "max_bps": None, "current_bps": None}
    diffs = [
        (l.price - r.price) / r.price * 10_000
        for l, r in pairs if r.price > 0
    ]
    if not diffs:
        return {"samples": 0, "mean_bps": None, "max_bps": None, "current_bps": None}
    return {
        "samples": len(diffs),
        "mean_bps": statistics.fmean(diffs),
        "median_bps": statistics.median(diffs),
        "max_bps": max(diffs, key=abs),
        "current_bps": diffs[-1],
    }


# ── summary ──────────────────────────────────────────────────────────────────


@dataclass
class Stats:
    """Everything :func:`summarise` computes, in one record."""

    pair: str
    network: str
    samples: int
    span_secs: int
    first: Optional[float] = None
    last: Optional[float] = None
    low: Optional[float] = None
    high: Optional[float] = None
    mean: Optional[float] = None
    median: Optional[float] = None
    twap: Optional[float] = None
    change_pct: Optional[float] = None
    volatility_annual: Optional[float] = None
    volatility_period: Optional[float] = None
    median_interval_secs: Optional[float] = None
    #: `None` when the window held fewer than two prints. A zero here would claim the
    #: price held through a span nobody observed.
    max_drawdown_pct: Optional[float] = None
    largest_move_bps: Optional[float] = None

    @property
    def range_pct(self) -> Optional[float]:
        """High-to-low range as a percentage of the low. The window's total travel."""
        if self.low is None or self.high is None or self.low <= 0:
            return None
        return (self.high - self.low) / self.low * 100

    @property
    def twap_divergence_bps(self) -> Optional[float]:
        """How far the last print sits from the window's TWAP.

        A large positive number means the spot answer is well above what a time-weighted
        oracle would report — which is precisely the condition a TWAP-based protocol is
        protected against and a spot-based one is not.
        """
        if self.twap is None or self.last is None or self.twap <= 0:
            return None
        return (self.last - self.twap) / self.twap * 10_000

    def as_dict(self) -> Dict[str, Any]:
        return {
            "pair": self.pair,
            "network": self.network,
            "samples": self.samples,
            "span_secs": self.span_secs,
            "first": self.first,
            "last": self.last,
            "low": self.low,
            "high": self.high,
            "mean": self.mean,
            "median": self.median,
            "twap": self.twap,
            "change_pct": self.change_pct,
            "range_pct": self.range_pct,
            "volatility_annual": self.volatility_annual,
            "volatility_period": self.volatility_period,
            "median_interval_secs": self.median_interval_secs,
            "max_drawdown_pct": self.max_drawdown_pct,
            "largest_move_bps": self.largest_move_bps,
            "twap_divergence_bps": self.twap_divergence_bps,
        }


def summarise(series: Series) -> Stats:
    """Every statistic in this module, computed once over one series."""
    prices = series.prices
    stats = Stats(
        pair=series.pair,
        network=series.network,
        samples=len(series),
        span_secs=series.span_secs,
    )
    if not prices:
        return stats

    stats.first = prices[0]
    stats.last = prices[-1]
    stats.low = min(prices)
    stats.high = max(prices)
    stats.mean = mean_price(series)
    stats.median = median_price(series)
    stats.twap = twap(series)
    if stats.first:
        stats.change_pct = (stats.last - stats.first) / stats.first * 100
    stats.volatility_annual = volatility(series, annualise=True)
    stats.volatility_period = volatility(series, annualise=False)
    stats.median_interval_secs = median_interval(series)
    drawdown = max_drawdown(series)["drawdown_pct"]
    stats.max_drawdown_pct = None if drawdown is None else float(drawdown)
    move = largest_move(series)["move_bps"]
    stats.largest_move_bps = None if move is None else float(move)
    return stats


def outliers(series: Series, z: float = 3.0) -> List[Dict[str, Any]]:
    """Observations whose return is more than ``z`` standard deviations from the mean.

    The bad-round detector. A genuine market move usually appears as a run of large
    returns; a single isolated spike that immediately reverts is far more often a
    reporting fault, and this is what surfaces it for inspection.
    """
    returns = log_returns(series)
    if len(returns) < 3:
        return []
    mean = statistics.fmean(returns)
    try:
        sigma = statistics.stdev(returns)
    except statistics.StatisticsError:  # pragma: no cover - guarded by the length check
        return []
    if sigma <= 0:
        return []

    found: List[Dict[str, Any]] = []
    for index, value in enumerate(returns):
        score = (value - mean) / sigma
        if abs(score) >= z:
            previous, current = series.points[index], series.points[index + 1]
            found.append({
                "z": score,
                "return_pct": (math.exp(value) - 1) * 100,
                "from": previous.as_dict(),
                "to": current.as_dict(),
                "interval_secs": current.timestamp - previous.timestamp,
            })
    return found
