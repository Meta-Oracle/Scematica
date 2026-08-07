"""Fan work out across chains without letting one slow endpoint set the pace.

Reading the same pair on eleven networks sequentially takes eleven round trips end to
end — on public endpoints, several seconds, and the *slowest* chain sets the total. Run
concurrently it takes about as long as the slowest single call. That difference is the
gap between a cross-chain view that feels live and one people stop opening.

Two rules make this safe to use everywhere rather than only in the dashboard:

* **A failure is a result, not an exception.** Every task returns an :class:`Outcome`
  carrying either a value or an error string. A cross-chain sweep where one chain is
  down should show ten prices and one error row, which is exactly the information the
  user needs; raising would show none of it.
* **One client per thread.** :class:`~alchem_link.rpc.RpcClient` accumulates request ids
  and statistics without a lock, so sharing one across workers corrupts both. Every
  helper here builds a client inside the task, which is also what keeps the per-chain
  timing honest.

Concurrency is capped and defaults low. These are public endpoints; opening eleven
simultaneous connections to the same provider is how a sweep turns into a 429 and the
tool looks broken when it is being rude.
"""
from __future__ import annotations

import time
from concurrent.futures import ThreadPoolExecutor, as_completed
from dataclasses import dataclass
from typing import Any, Callable, Dict, Iterable, List, Optional, Sequence, TypeVar

from .errors import AlchemLinkError
from .feeds import FeedReading, read_all_feeds, read_feed
from .networks import list_networks
from .registry import networks_carrying

T = TypeVar("T")

#: Default worker count. Enough to cover the registry's networks in two waves without
#: making a public provider think it is being scraped.
DEFAULT_WORKERS = 6

#: Ceiling on total wall time for a sweep. A chain that has not answered by then is
#: reported as timed out; without this a single hung endpoint holds the whole result.
DEFAULT_TIMEOUT = 30.0


@dataclass
class Outcome:
    """One task's result: a value, or an error, plus how long it took."""

    key: str
    value: Any = None
    error: Optional[str] = None
    elapsed_ms: float = 0.0

    @property
    def ok(self) -> bool:
        return self.error is None

    def unwrap(self, default: Any = None) -> Any:
        return self.value if self.ok else default

    def as_dict(self) -> Dict[str, Any]:
        value = self.value
        if hasattr(value, "as_dict"):
            value = value.as_dict()
        elif isinstance(value, list):
            value = [v.as_dict() if hasattr(v, "as_dict") else v for v in value]
        return {
            "key": self.key,
            "ok": self.ok,
            "value": value,
            "error": self.error,
            "elapsed_ms": round(self.elapsed_ms, 1),
        }


@dataclass
class SweepReport:
    """The results of one fan-out, in the order the tasks were requested."""

    outcomes: List[Outcome]
    elapsed_ms: float
    workers: int

    def __iter__(self):
        return iter(self.outcomes)

    def __len__(self) -> int:
        return len(self.outcomes)

    @property
    def ok(self) -> List[Outcome]:
        return [o for o in self.outcomes if o.ok]

    @property
    def failed(self) -> List[Outcome]:
        return [o for o in self.outcomes if not o.ok]

    def get(self, key: str) -> Optional[Outcome]:
        for outcome in self.outcomes:
            if outcome.key == key:
                return outcome
        return None

    def values(self) -> Dict[str, Any]:
        """Successful results only, keyed. The shape most callers actually want."""
        return {o.key: o.value for o in self.outcomes if o.ok}

    @property
    def speedup(self) -> float:
        """How much faster this was than running the same tasks one after another.

        Derived from the summed per-task times against the wall clock. Worth reporting:
        it is the number that tells you whether the fan-out is helping or whether one
        chain is so slow that everything is waiting on it anyway.
        """
        serial = sum(o.elapsed_ms for o in self.outcomes)
        return serial / self.elapsed_ms if self.elapsed_ms > 0 else 1.0

    def as_dict(self) -> Dict[str, Any]:
        return {
            "outcomes": [o.as_dict() for o in self.outcomes],
            "ok": len(self.ok),
            "failed": len(self.failed),
            "elapsed_ms": round(self.elapsed_ms, 1),
            "workers": self.workers,
            "speedup": round(self.speedup, 2),
        }


def run_tasks(tasks: Dict[str, Callable[[], Any]], workers: int = DEFAULT_WORKERS,
              timeout: float = DEFAULT_TIMEOUT) -> SweepReport:
    """Run every callable concurrently and collect outcomes. Never raises.

    Results keep the order of ``tasks`` rather than completion order, so a table built
    from them does not reshuffle between runs depending on which chain answered first.
    """
    if not tasks:
        return SweepReport(outcomes=[], elapsed_ms=0.0, workers=0)

    started = time.perf_counter()
    pool_size = max(1, min(workers, len(tasks)))
    collected: Dict[str, Outcome] = {}

    with ThreadPoolExecutor(max_workers=pool_size, thread_name_prefix="alchem-sweep") as pool:
        futures = {}
        for key, fn in tasks.items():
            futures[pool.submit(_timed, fn)] = key
        try:
            for future in as_completed(futures, timeout=timeout):
                key = futures[future]
                try:
                    value, elapsed = future.result()
                    collected[key] = Outcome(key=key, value=value, elapsed_ms=elapsed)
                except AlchemLinkError as exc:
                    collected[key] = Outcome(key=key, error=str(exc))
                except Exception as exc:  # an unexpected failure is still just one row
                    collected[key] = Outcome(key=key, error=f"{exc.__class__.__name__}: {exc}")
        except TimeoutError:
            # as_completed's deadline expired. Whatever landed is kept; the rest are
            # reported as timed out rather than silently missing.
            for future, key in futures.items():
                if key not in collected:
                    future.cancel()

    elapsed_ms = (time.perf_counter() - started) * 1000
    outcomes = [
        collected.get(key, Outcome(key=key, error=f"timed out after {timeout:.0f}s"))
        for key in tasks
    ]
    return SweepReport(outcomes=outcomes, elapsed_ms=elapsed_ms, workers=pool_size)


def _timed(fn: Callable[[], Any]):
    started = time.perf_counter()
    value = fn()
    return value, (time.perf_counter() - started) * 1000


def map_networks(fn: Callable[[str], T], networks: Optional[Sequence[str]] = None,
                 include_testnets: bool = False, workers: int = DEFAULT_WORKERS,
                 timeout: float = DEFAULT_TIMEOUT) -> SweepReport:
    """Apply ``fn(network)`` across networks concurrently.

    Testnets are excluded by default. Their feeds carry unrelated test data, and folding
    them into a cross-chain aggregate produces a consensus that describes nothing.
    """
    if networks is None:
        networks = [
            n.key for n in list_networks() if include_testnets or not n.testnet
        ]
    return run_tasks(
        {name: (lambda n=name: fn(n)) for name in networks},
        workers=workers, timeout=timeout,
    )


def read_pair_everywhere(pair: str, networks: Optional[Sequence[str]] = None,
                         include_testnets: bool = False,
                         workers: int = DEFAULT_WORKERS) -> SweepReport:
    """Read one pair on every chain that carries it, concurrently.

    The backing call for cross-chain divergence: eleven chains in roughly one chain's
    latency, with unreadable endpoints reported rather than dropped.
    """
    targets = list(networks) if networks else networks_carrying(pair)
    if not include_testnets:
        from .networks import NETWORKS

        targets = [n for n in targets if not NETWORKS[n].testnet]
    return run_tasks(
        {name: (lambda n=name: read_feed(pair, network=n)) for name in targets},
        workers=workers,
    )


def read_all_networks(networks: Optional[Sequence[str]] = None,
                      include_testnets: bool = False,
                      workers: int = DEFAULT_WORKERS) -> SweepReport:
    """Every registered feed on every network. The dashboard's global view.

    Each network's own reads are already batched into one round trip by
    :func:`~alchem_link.feeds.read_all_feeds`, so this is a fan-out of batches — the
    whole registry in about the time the slowest single chain takes.
    """
    return map_networks(
        lambda network: read_all_feeds(network=network),
        networks=networks, include_testnets=include_testnets, workers=workers,
    )


def gather(readings: SweepReport) -> List[FeedReading]:
    """Flatten a sweep of feed lists into one list, dropping the failures.

    Convenience for the common "give me everything readable right now" case; use
    :attr:`SweepReport.failed` when the errors matter.
    """
    out: List[FeedReading] = []
    for outcome in readings.ok:
        value = outcome.value
        if isinstance(value, list):
            out.extend(value)
        elif value is not None:
            out.append(value)
    return out
