"""``AlchemLink`` — one object that holds a network, a connection and a cache.

The module-level functions each take ``network=`` and ``rpc_url=`` and each build their
own :class:`~alchem_link.rpc.RpcClient`. That is the right shape for a script that reads
one price, and the wrong shape for anything longer: five calls means five clients, five
Multicall3 probes, and five sets of statistics that add up to nothing you can report.

This class holds those once.

    link = AlchemLink("base")
    link.price("ETH/USD")          # one round trip
    link.price("ETH/USD")          # cached — no round trip
    link.audit()                   # reuses the same client and Multicall3 probe
    link.rpc_stats()               # what the whole session actually cost

Caching is on by default and sized per feed from its *measured* heartbeat, so a Polygon
feed on a 60-second cadence caches for 20 seconds and an Ethereum feed on an hourly one
caches for the full two minutes — see :mod:`alchem_link.cache`. Pass ``cache=False`` for a
guaranteed-live read, or call :meth:`refresh` to drop what is held.

Every method returns the same objects the functional API returns. This is a facade, not
a second implementation — there is no behaviour here that is not reachable without it.
"""
from __future__ import annotations

from typing import Any, Dict, Iterable, List, Optional, Sequence

from .aggregator import AggregatorInfo, Round, describe_aggregator, round_history
from .analytics import Series, Stats, summarise
from .cache import TTLCache, key_for, ttl_for_feed
from .cadence import CadenceProfile, profile_feed
from .ccip import Lane, verify_lanes
from .divergence import DivergenceReport, compare_pair
from .errors import InvalidAnswer, StaleFeed
from .exporters import export
from .feeds import Feed, FeedReading, get_feed, list_feeds, read_all_feeds, read_feed
from .gas import GasReport, analyse_gas
from .health import Diagnosis, diagnose
from .logs import AnswerUpdate, answer_updates
from .networks import DEFAULT_NETWORK, Endpoint, Network, get_network, resolve_endpoint
from .parallel import SweepReport, read_pair_everywhere
from .registry import FeedLocation, describe_feed, find, resolve
from .rpc import RpcClient, client_for
from .safety import Audit, audit_feed, audit_network
from .sequencer import SequencerStatus, is_l2, read_sequencer
from .simulate import AuditResult, Guard, ReplayReport, audit_guard, observations_from_series, replay


class AlchemLink:
    """A network-bound, connection-reusing, caching handle on the whole toolkit."""

    def __init__(self, network: str = DEFAULT_NETWORK, rpc_url: Optional[str] = None,
                 cache: bool = True, timeout: float = 15.0,
                 cache_size: int = 512) -> None:
        self.network = get_network(network).key
        self.rpc_url = rpc_url
        self.timeout = timeout
        self._client: Optional[RpcClient] = None
        self._cache: Optional[TTLCache] = TTLCache(maxsize=cache_size) if cache else None

    # ── plumbing ─────────────────────────────────────────────────────────────

    def __repr__(self) -> str:  # pragma: no cover - debugging aid
        return f"<AlchemLink {self.network} via {self.endpoint.redacted()}>"

    def __enter__(self) -> "AlchemLink":
        return self

    def __exit__(self, *exc) -> None:
        self.close()

    def close(self) -> None:
        """Drop the cache and the client. The connection is stateless, so this is cheap."""
        if self._cache is not None:
            self._cache.clear()
        self._client = None

    @property
    def client(self) -> RpcClient:
        """The shared RPC client, built on first use.

        Lazy so constructing an :class:`AlchemLink` never touches the network — which is
        what lets the dashboard build one per network up front without eleven probes.
        """
        if self._client is None:
            self._client = client_for(
                network=self.network, rpc_url=self.rpc_url, timeout=self.timeout
            )
        return self._client

    @property
    def endpoint(self) -> Endpoint:
        return resolve_endpoint(network=self.network, rpc_url=self.rpc_url)

    @property
    def net(self) -> Network:
        return get_network(self.network)

    def on(self, network: str) -> "AlchemLink":
        """A handle on another network, with its own connection and cache.

        Returns a new object rather than mutating this one: the dashboard keeps several
        alive at once, and a mutating switch would invalidate results already in flight.
        """
        return AlchemLink(network, rpc_url=self.rpc_url,
                          cache=self._cache is not None, timeout=self.timeout)

    def refresh(self, pair: Optional[str] = None) -> int:
        """Drop cached data — all of it, or one pair's. Returns entries removed."""
        if self._cache is None:
            return 0
        if pair is None:
            count = len(self._cache)
            self._cache.clear()
            return count
        # Substring, not prefix: the pair sits after the kind in the key, so every cached
        # view of one feed — price, history, cadence, inspect — has to be matched inside.
        return self._cache.invalidate_containing(resolve(pair, self.network))

    def _cached(self, key: str, factory, ttl: Optional[float] = None):
        if self._cache is None:
            return factory()
        return self._cache.get_or_set(key, factory, ttl)

    def _feed_ttl(self, pair: str) -> float:
        try:
            return ttl_for_feed(get_feed(pair, self.network).heartbeat_secs)
        except KeyError:
            return 30.0

    # ── registry (offline) ───────────────────────────────────────────────────

    def feeds(self) -> List[Feed]:
        """Registered feeds on this network. No network access."""
        return list_feeds(self.network)

    def feed(self, pair: str) -> Feed:
        return get_feed(resolve(pair, self.network), self.network)

    def describe(self, pair: str) -> Dict[str, Any]:
        """Registry facts about one feed, including where else the pair lives."""
        return describe_feed(pair, self.network)

    def search(self, query: str = "", asset: Optional[str] = None,
               everywhere: bool = False) -> List[FeedLocation]:
        """Search the registry; scoped to this network unless ``everywhere``."""
        return find(query, network=None if everywhere else self.network, asset=asset)

    # ── live reads ───────────────────────────────────────────────────────────

    def price(self, pair: str, strict: bool = False) -> FeedReading:
        """Read one feed.

        ``strict=True`` raises :class:`~alchem_link.errors.StaleFeed` or
        :class:`~alchem_link.errors.InvalidAnswer` instead of returning a reading that
        says so. The default returns the reading, because a caller usually wants to see
        both the number and the verdict — but a contract-facing path wants the exception,
        and having to remember to check ``.stale`` is exactly how people forget to.
        """
        resolved = resolve(pair, self.network)
        reading = self._cached(
            key_for(self.network, "price", resolved),
            lambda: read_feed(resolved, network=self.network, client=self.client),
            ttl=self._feed_ttl(resolved),
        )
        if strict:
            if reading.answer_raw <= 0:
                raise InvalidAnswer(
                    f"{reading.pair} on {reading.network} answered {reading.price}",
                    pair=reading.pair, network=reading.network,
                )
            if reading.stale:
                raise StaleFeed(reading.pair, reading.network,
                                reading.age_secs, reading.heartbeat_secs)
        return reading

    def prices(self) -> List[FeedReading]:
        """Every registered feed on this network, in one batched round trip."""
        return self._cached(
            key_for(self.network, "prices"),
            lambda: read_all_feeds(network=self.network, client=self.client),
            ttl=30.0,
        )

    def everywhere(self, pair: str) -> SweepReport:
        """Read one pair on every chain that carries it, concurrently.

        Not cached: this fans out across networks and therefore past this object's own
        client, so the per-network caches do not apply and pretending otherwise would
        serve one chain's stale number inside a comparison.
        """
        return read_pair_everywhere(resolve(pair))

    # ── analysis ─────────────────────────────────────────────────────────────

    def audit(self, pair: Optional[str] = None) -> List[Audit]:
        """Consumer-safety audit for one feed, or every feed on this network."""
        if pair:
            resolved = resolve(pair, self.network)
            return [audit_feed(resolved, network=self.network, client=self.client)]
        return audit_network(network=self.network, client=self.client)

    def inspect(self, target: str) -> AggregatorInfo:
        """Resolve a proxy and read its bounds and type. Accepts a pair or an address."""
        address = target if target.startswith("0x") else self.feed(target).address
        return self._cached(
            key_for(self.network, "inspect", address),
            lambda: describe_aggregator(address, network=self.network, client=self.client),
            ttl=120.0,
        )

    def history(self, pair: str, rounds: int = 30) -> List[Round]:
        """Walk a feed's round history, newest first."""
        address = self.feed(pair).address
        return self._cached(
            key_for(self.network, "history", pair, rounds),
            lambda: round_history(address, count=rounds, network=self.network,
                                  client=self.client),
            ttl=60.0,
        )

    def series(self, pair: str, rounds: int = 30) -> Series:
        """A feed's history as an analysable :class:`~alchem_link.analytics.Series`."""
        resolved = resolve(pair, self.network)
        return Series.from_rounds(self.history(resolved, rounds), resolved, self.network)

    def stats(self, pair: str, rounds: int = 30) -> Stats:
        """TWAP, volatility, drawdown and the rest, over the last ``rounds`` publishes."""
        return summarise(self.series(pair, rounds))

    def updates(self, pair: str, hours: float = 6.0) -> List[AnswerUpdate]:
        """Every publish in the last ``hours``, from event logs rather than round walking.

        Far cheaper than :meth:`history` for a long window — one ``eth_getLogs`` against
        a hundred ``eth_call``s — at the cost of depending on log retention.
        """
        address = self.feed(pair).address
        return self._cached(
            key_for(self.network, "updates", pair, hours),
            lambda: answer_updates(address, hours=hours, network=self.network,
                                   client=self.client),
            ttl=60.0,
        )

    def cadence(self, pair: str, rounds: int = 30) -> CadenceProfile:
        """Measure the real heartbeat and deviation threshold from round history."""
        resolved = resolve(pair, self.network)
        return self._cached(
            key_for(self.network, "cadence", resolved, rounds),
            lambda: profile_feed(resolved, self.network, rounds=rounds, client=self.client),
            ttl=300.0,
        )

    def divergence(self, pair: str, outlier_bps: float = 50.0) -> DivergenceReport:
        """Compare one pair across every chain that carries it."""
        return compare_pair(resolve(pair), outlier_bps=outlier_bps)

    # ── chain state ──────────────────────────────────────────────────────────

    def sequencer(self) -> Optional[SequencerStatus]:
        """This network's L2 uptime feed, or ``None`` when it is not a rollup."""
        if not is_l2(self.network):
            return None
        return self._cached(
            key_for(self.network, "sequencer"),
            lambda: read_sequencer(self.network),
            ttl=30.0,
        )

    def gas(self, blocks: int = 20) -> GasReport:
        """EIP-1559 fee tiers, priced in USD through this chain's own oracle."""
        return self._cached(
            key_for(self.network, "gas", blocks),
            lambda: analyse_gas(network=self.network, rpc_url=self.rpc_url, blocks=blocks),
            ttl=15.0,
        )

    def block(self) -> int:
        return self.client.block_number()

    def lanes(self) -> List[Lane]:
        """Live CCIP lane status from this network's router."""
        return self._cached(
            key_for(self.network, "lanes"),
            lambda: verify_lanes(self.network, rpc_url=self.rpc_url),
            ttl=300.0,
        )

    def doctor(self) -> Diagnosis:
        """End-to-end readiness check for this endpoint."""
        return diagnose(network=self.network, rpc_url=self.rpc_url)

    # ── simulation ───────────────────────────────────────────────────────────

    def simulate(self, guard: Optional[Guard] = None) -> AuditResult:
        """Replay a consumer guard against every known oracle failure mode. Offline."""
        return audit_guard(guard)

    def backtest(self, pair: str, guard: Optional[Guard] = None,
                 rounds: int = 50) -> ReplayReport:
        """Replay a guard against this feed's *real* history.

        The complement to :meth:`simulate`: that one asks whether the guard catches the
        known disasters, this one asks whether it would have rejected rounds the feed
        legitimately produced. A guard that scores perfectly on the scenarios and rejects
        a third of real history is not a guard anyone can ship.
        """
        resolved = resolve(pair, self.network)
        feed = self.feed(resolved)
        series = self.series(resolved, rounds)
        observations = observations_from_series(series, heartbeat_secs=feed.heartbeat_secs)
        return replay(guard or Guard(), observations,
                      name=f"{resolved}@{self.network}",
                      expectation="real history — rejections here are false positives")

    # ── output ───────────────────────────────────────────────────────────────

    def export(self, items: Iterable[Any], fmt: str = "json",
               columns: Optional[Sequence[str]] = None) -> str:
        """Render results as CSV, NDJSON, Prometheus, Markdown or JSON."""
        return export(items, fmt, columns)

    # ── introspection ────────────────────────────────────────────────────────

    def rpc_stats(self) -> Dict[str, Any]:
        """What this session actually cost: requests, batched reads, round trips saved."""
        if self._client is None:
            return {"requests": 0, "http_posts": 0, "note": "no connection opened yet"}
        return self._client.stats.as_dict()

    def cache_stats(self) -> Dict[str, Any]:
        # `is not None`, not truthiness: TTLCache defines __len__, so an empty cache is
        # falsy and a plain `if self._cache` reports caching as disabled until the first
        # entry lands.
        return self._cache.describe() if self._cache is not None else {"enabled": False}

    def summary(self) -> Dict[str, Any]:
        """Everything about this handle, without touching the network."""
        return {
            "network": self.network,
            "chain_id": self.net.chain_id,
            "label": self.net.label,
            "endpoint": self.endpoint.redacted(),
            "endpoint_source": self.endpoint.source,
            "authenticated": self.endpoint.is_authenticated,
            "layer2": self.net.layer2,
            "testnet": self.net.testnet,
            "feeds": len(self.feeds()),
            "cache": self.cache_stats(),
            "rpc": self.rpc_stats(),
        }


def connect(network: str = DEFAULT_NETWORK, rpc_url: Optional[str] = None,
            cache: bool = True) -> AlchemLink:
    """Build an :class:`AlchemLink`. The one-liner most scripts want.

        from alchem_link import connect
        print(connect("base").price("ETH/USD").price)
    """
    return AlchemLink(network, rpc_url=rpc_url, cache=cache)
