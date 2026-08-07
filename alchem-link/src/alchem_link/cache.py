"""A TTL cache sized in the units that matter here: a feed's own heartbeat.

Generic caching gets oracle work wrong. A five-second TTL is far too short for an
Ethereum mainnet feed that publishes hourly — you re-read the same round six hundred
times — and far too long for a Polygon feed on a sixty-second heartbeat, where it serves
a price that has since moved. The right answer is per-feed, and it is already known:
:attr:`alchem_link.feeds.Feed.heartbeat_secs` is a *measured* publish interval.

So :func:`ttl_for_feed` derives the TTL from the feed rather than from a constant, and
errs short — a fraction of the heartbeat — because the cost of a redundant read is one
round trip and the cost of a stale one is a wrong number presented as current.

The cache is thread-safe because the dashboard's worker pool shares one across panels.
It is deliberately *not* persistent: a cache that survives the process would have to
reason about clock skew and chain reorgs, and no part of this toolkit is improved by
answering from a file.
"""
from __future__ import annotations

import threading
import time
from dataclasses import dataclass, field
from typing import Any, Callable, Dict, Hashable, Optional, Tuple

#: Fraction of a heartbeat a cached reading may be served for. A third means at most one
#: publish interval of drift in the worst case, and typically far less.
HEARTBEAT_FRACTION = 1 / 3

#: Floor and ceiling on the derived TTL. The floor stops a hypothetical one-second
#: heartbeat from defeating the cache entirely; the ceiling keeps a daily feed from
#: serving a twelve-hour-old number just because it publishes rarely.
MIN_TTL_SECS = 2.0
MAX_TTL_SECS = 120.0


def ttl_for_feed(heartbeat_secs: int) -> float:
    """TTL for a feed publishing every ``heartbeat_secs``, clamped to sane bounds."""
    if heartbeat_secs <= 0:
        return MIN_TTL_SECS
    return max(MIN_TTL_SECS, min(MAX_TTL_SECS, heartbeat_secs * HEARTBEAT_FRACTION))


@dataclass
class Entry:
    value: Any
    expires_at: float
    stored_at: float

    @property
    def age(self) -> float:
        return time.monotonic() - self.stored_at

    def is_live(self, now: Optional[float] = None) -> bool:
        return (now if now is not None else time.monotonic()) < self.expires_at


@dataclass
class CacheStats:
    hits: int = 0
    misses: int = 0
    evictions: int = 0
    expirations: int = 0

    @property
    def hit_rate(self) -> float:
        total = self.hits + self.misses
        return self.hits / total if total else 0.0

    def as_dict(self) -> Dict[str, Any]:
        return {
            "hits": self.hits,
            "misses": self.misses,
            "evictions": self.evictions,
            "expirations": self.expirations,
            "hit_rate": round(self.hit_rate, 4),
        }


class TTLCache:
    """A bounded, thread-safe, time-expiring key/value store.

    Eviction is least-recently-used, implemented on an insertion-ordered dict: reading a
    key moves it to the end, so the oldest untouched key is always the first. That is
    enough for a cache measured in hundreds of entries and avoids carrying a heap.
    """

    def __init__(self, maxsize: int = 512, default_ttl: float = 30.0) -> None:
        self.maxsize = max(1, maxsize)
        self.default_ttl = default_ttl
        self.stats = CacheStats()
        self._entries: Dict[Hashable, Entry] = {}
        self._lock = threading.RLock()

    def __len__(self) -> int:
        with self._lock:
            return len(self._entries)

    def __contains__(self, key: Hashable) -> bool:
        return self.get(key) is not None

    def get(self, key: Hashable, default: Any = None) -> Any:
        """Fetch a live value, or ``default``. Expired entries are dropped on the way."""
        with self._lock:
            entry = self._entries.get(key)
            if entry is None:
                self.stats.misses += 1
                return default
            if not entry.is_live():
                del self._entries[key]
                self.stats.expirations += 1
                self.stats.misses += 1
                return default
            # Refresh LRU position.
            self._entries[key] = self._entries.pop(key)
            self.stats.hits += 1
            return entry.value

    def peek(self, key: Hashable) -> Optional[Entry]:
        """The raw entry, live or not, without touching LRU order or statistics.

        The dashboard uses this to show "last read 40s ago" for data it is currently
        refreshing — reading it through :meth:`get` would count as a hit and reorder the
        cache for a purely cosmetic query.
        """
        with self._lock:
            return self._entries.get(key)

    def set(self, key: Hashable, value: Any, ttl: Optional[float] = None) -> None:
        with self._lock:
            now = time.monotonic()
            self._entries.pop(key, None)
            self._entries[key] = Entry(
                value=value,
                expires_at=now + (self.default_ttl if ttl is None else ttl),
                stored_at=now,
            )
            while len(self._entries) > self.maxsize:
                self._entries.pop(next(iter(self._entries)))
                self.stats.evictions += 1

    def get_or_set(self, key: Hashable, factory: Callable[[], Any],
                   ttl: Optional[float] = None) -> Any:
        """Return the cached value, computing and storing it on a miss.

        ``factory`` runs *outside* the lock. Holding it across an RPC round trip would
        serialise every panel in the dashboard behind whichever one missed first — which
        would make the cache slower than not having one. The cost is that two threads
        missing the same key concurrently both fetch; that is one redundant read, and it
        is much cheaper than the alternative.
        """
        found = self.get(key, _MISSING)
        if found is not _MISSING:
            return found
        value = factory()
        self.set(key, value, ttl)
        return value

    def invalidate(self, key: Hashable) -> bool:
        with self._lock:
            return self._entries.pop(key, None) is not None

    def invalidate_prefix(self, prefix: str) -> int:
        """Drop every string key starting with ``prefix``. Returns how many went.

        This is what a network switch uses: keys are namespaced ``"<network>:<what>"``,
        so changing chains drops that chain's data without flushing the others.
        """
        with self._lock:
            doomed = [k for k in self._entries if isinstance(k, str) and k.startswith(prefix)]
            for key in doomed:
                del self._entries[key]
            return len(doomed)

    def invalidate_containing(self, needle: str) -> int:
        """Drop every string key containing ``needle``. Returns how many went.

        Keys are ``"<network>:<kind>:<pair>[:args]"``, so the pair sits in the middle and
        a prefix match cannot reach it. Refreshing one feed across all of its cached
        views — price, history, cadence, inspect — is a substring match or nothing.
        """
        wanted = needle.lower()
        with self._lock:
            doomed = [k for k in self._entries if isinstance(k, str) and wanted in k]
            for key in doomed:
                del self._entries[key]
            return len(doomed)

    def clear(self) -> None:
        with self._lock:
            self._entries.clear()

    def prune(self) -> int:
        """Drop everything expired. Returns how many entries went."""
        with self._lock:
            now = time.monotonic()
            doomed = [k for k, e in self._entries.items() if not e.is_live(now)]
            for key in doomed:
                del self._entries[key]
            self.stats.expirations += len(doomed)
            return len(doomed)

    def describe(self) -> Dict[str, Any]:
        with self._lock:
            return {
                "entries": len(self._entries),
                "maxsize": self.maxsize,
                "default_ttl": self.default_ttl,
                **self.stats.as_dict(),
            }


class _Missing:
    """Sentinel distinguishing "not cached" from a cached ``None``."""

    def __repr__(self) -> str:  # pragma: no cover - debugging aid
        return "<missing>"


_MISSING = _Missing()


def key_for(*parts: Any) -> str:
    """Build a namespaced cache key: ``key_for("base", "price", "ETH/USD")``.

    Keys are strings rather than tuples so :meth:`TTLCache.invalidate_prefix` can work,
    which is the operation a network or feed switch actually needs.
    """
    return ":".join(str(p).lower() for p in parts if p is not None)


def memoize(cache: TTLCache, ttl: Optional[float] = None,
            key: Optional[Callable[..., str]] = None):
    """Decorator caching a function's return in ``cache``.

    ``key`` builds the cache key from the arguments; the default uses the function name
    and the stringified arguments, which is right for the small, hashable arguments
    everything in this package passes and wrong for anything else — hence the hook.
    """
    def decorate(fn: Callable) -> Callable:
        def wrapper(*args, **kwargs):
            cache_key = key(*args, **kwargs) if key else key_for(fn.__name__, *args,
                                                                 *sorted(kwargs.items()))
            return cache.get_or_set(cache_key, lambda: fn(*args, **kwargs), ttl)

        wrapper.__name__ = fn.__name__
        wrapper.__doc__ = fn.__doc__
        wrapper.cache = cache  # type: ignore[attr-defined]
        return wrapper

    return decorate
