"""Searching the feed registry: by pair, by asset, by address, or by a near miss.

:func:`alchem_link.feeds.get_feed` answers one question — "give me exactly this pair on
exactly this network" — and raises otherwise. Most real questions are looser than that.
*Where can I read SOL?* *What is at this address?* *Which chain has the fastest ETH/USD?*
Answering those by iterating ``FEEDS`` is four lines each and everyone writes them
slightly differently, so they live here.

The resolver is the part worth attention. Pair names arrive as ``eth/usd``, ``ETH-USD``,
``eth usd``, ``ETHUSD``, and — often enough to matter — misspelled. :func:`resolve`
normalises the first four, then falls back to a similarity search so that ``ETH/USDC``
on a chain that carries no such feed produces "did you mean ETH/USD?" rather than a bare
failure. It never *silently* substitutes: a fuzzy match is returned as a suggestion on
the exception, and the caller decides.

Nothing here touches a chain. It is a table lookup, which makes it instant, offline, and
usable for shell completion.
"""
from __future__ import annotations

import difflib
from dataclasses import dataclass
from typing import Dict, Iterable, List, Optional, Sequence, Tuple

from .errors import UnknownFeed
from .feeds import FEEDS, Feed, get_feed
from .networks import NETWORKS, Network, get_network, list_networks

#: Below this ratio a "did you mean" is noise rather than help.
SUGGESTION_CUTOFF = 0.6


@dataclass(frozen=True)
class FeedLocation:
    """A feed together with the network it lives on.

    ``Feed`` alone does not carry its network, which is fine inside a per-network table
    and useless the moment results span chains — every cross-chain answer here returns
    one of these instead.
    """

    feed: Feed
    network: str

    @property
    def pair(self) -> str:
        return self.feed.pair

    @property
    def address(self) -> str:
        return self.feed.address

    @property
    def base(self) -> str:
        return self.feed.pair.split("/")[0]

    @property
    def quote(self) -> str:
        parts = self.feed.pair.split("/")
        return parts[1] if len(parts) > 1 else ""

    def as_dict(self) -> Dict[str, object]:
        return {
            "pair": self.pair,
            "network": self.network,
            "address": self.address,
            "decimals": self.feed.decimals,
            "heartbeat_secs": self.feed.heartbeat_secs,
            "heartbeat_measured": self.feed.heartbeat_measured,
            "note": self.feed.note,
        }


def normalise_pair(text: str) -> str:
    """``" eth-usd "`` → ``"ETH/USD"``. Separator-agnostic, case-insensitive.

    ``ETHUSD`` with no separator at all is handled by :func:`resolve` rather than here,
    because splitting it requires knowing the quote assets and this function does not.
    """
    cleaned = text.strip().upper().replace("-", "/").replace("\\", "/").replace(" ", "/")
    while "//" in cleaned:
        cleaned = cleaned.replace("//", "/")
    return cleaned.strip("/")


#: Quote assets seen in the registry, longest first so ``ETHUSDC`` splits before
#: ``ETHUSD`` gets a chance to claim the prefix.
def _quote_assets() -> List[str]:
    quotes = {pair.split("/")[1] for table in FEEDS.values() for pair in table if "/" in pair}
    return sorted(quotes, key=len, reverse=True)


def all_locations() -> List[FeedLocation]:
    """Every registered feed on every network, as flat records."""
    return [
        FeedLocation(feed=feed, network=network)
        for network, table in FEEDS.items()
        for feed in table.values()
    ]


def all_pairs() -> List[str]:
    """Every distinct pair name in the registry, sorted."""
    return sorted({pair for table in FEEDS.values() for pair in table})


def all_assets() -> List[str]:
    """Every distinct asset symbol appearing on either side of a pair."""
    assets = set()
    for pair in all_pairs():
        assets.update(part for part in pair.split("/") if part)
    return sorted(assets)


def resolve(pair: str, network: Optional[str] = None) -> str:
    """Normalise a pair name to one the registry knows, or raise with a suggestion.

    Separator-free input (``ETHUSD``) is split against the quote assets actually present
    in the registry. Everything else is normalised and then, if still unknown, matched
    for similarity so the error can say what was probably meant.
    """
    candidate = normalise_pair(pair)
    known = list(FEEDS.get((network or "").lower(), {})) if network else all_pairs()

    if candidate in known:
        return candidate

    if "/" not in candidate:
        for quote in _quote_assets():
            if candidate.endswith(quote) and len(candidate) > len(quote):
                split = f"{candidate[: -len(quote)]}/{quote}"
                if split in known:
                    return split

    matches = difflib.get_close_matches(candidate, known, n=3, cutoff=SUGGESTION_CUTOFF)
    error = UnknownFeed(pair, network or "any network", known=known)
    if matches:
        error.message += f". Did you mean {' or '.join(matches)}?"
        error.suggestions = matches  # type: ignore[attr-defined]
    raise error


def suggest(pair: str, network: Optional[str] = None, limit: int = 5) -> List[str]:
    """Close matches for ``pair``, best first. Never raises — for completion and hints."""
    known = list(FEEDS.get((network or "").lower(), {})) if network else all_pairs()
    candidate = normalise_pair(pair)
    exact = [p for p in known if p.startswith(candidate)]
    fuzzy = [
        p for p in difflib.get_close_matches(candidate, known, n=limit, cutoff=0.4)
        if p not in exact
    ]
    return (exact + fuzzy)[:limit]


def find(query: str = "", network: Optional[str] = None,
         asset: Optional[str] = None) -> List[FeedLocation]:
    """Search the registry. Every filter is optional and they compose.

    ``query`` is a substring match on the pair name or the address, which is what makes
    ``find("0x71041")`` and ``find("eth")`` both do something sensible.
    """
    results = all_locations()
    if network:
        key = get_network(network).key
        results = [r for r in results if r.network == key]
    if asset:
        wanted = asset.strip().upper()
        results = [r for r in results if wanted in (r.base, r.quote)]
    if query:
        needle = normalise_pair(query) if "/" in query else query.strip().upper()
        results = [
            r for r in results
            if needle in r.pair or needle in r.address.upper()
        ]
        # Exact pair matches rank above substring hits. Without this, searching
        # "ETH/USD" buries it under "CBETH/USD", which contains it — and the first
        # result is what a caller taking `[0]` gets.
        return sorted(results, key=lambda r: (r.pair != needle, r.pair, r.network))
    return sorted(results, key=lambda r: (r.pair, r.network))


def networks_carrying(pair: str) -> List[str]:
    """Every network with a feed for ``pair``, mainnets first then testnets.

    Ordering is not cosmetic: a caller taking the first entry as a reference price must
    not land on Sepolia, whose feeds carry unrelated test data.
    """
    wanted = normalise_pair(pair)
    keys = [name for name, table in FEEDS.items() if wanted in table]
    return sorted(keys, key=lambda k: (NETWORKS[k].testnet, k))


def by_address(address: str) -> Optional[FeedLocation]:
    """Reverse lookup: which registered feed is this address?

    Case-insensitive, because addresses are passed around in checksummed and lowercase
    form interchangeably and a case-sensitive comparison silently finds nothing.
    """
    wanted = address.strip().lower()
    for location in all_locations():
        if location.address.lower() == wanted:
            return location
    return None


def fastest(pair: str) -> Optional[FeedLocation]:
    """The chain where ``pair`` publishes most often.

    Only measured heartbeats compete. An unmeasured entry is a conservative *bound*, so
    treating its number as a cadence would let a feed nobody has profiled win the
    comparison by having been guessed optimistically.
    """
    candidates = [
        FeedLocation(FEEDS[net][normalise_pair(pair)], net)
        for net in networks_carrying(pair)
        if not NETWORKS[net].testnet and FEEDS[net][normalise_pair(pair)].heartbeat_measured
    ]
    if not candidates:
        return None
    return min(candidates, key=lambda loc: loc.feed.heartbeat_secs)


def coverage() -> Dict[str, Dict[str, object]]:
    """Per-network summary: feed count, cadence range, and how much is measured.

    This is the table that makes the registry's honesty visible — a network where most
    heartbeats are bounds rather than measurements is a network whose staleness verdicts
    are conservative, and a consumer should know that before relying on them.
    """
    out: Dict[str, Dict[str, object]] = {}
    for network in list_networks():
        feeds = list(FEEDS.get(network.key, {}).values())
        if not feeds:
            out[network.key] = {"feeds": 0}
            continue
        measured = [f for f in feeds if f.heartbeat_measured]
        out[network.key] = {
            "feeds": len(feeds),
            "measured": len(measured),
            "bounded": len(feeds) - len(measured),
            "fastest_secs": min(f.heartbeat_secs for f in feeds),
            "slowest_secs": max(f.heartbeat_secs for f in feeds),
            "testnet": network.testnet,
            "layer2": network.layer2,
            "pairs": sorted(f.pair for f in feeds),
        }
    return out


def common_assets(networks: Sequence[str]) -> List[str]:
    """Assets present on every one of ``networks``. Empty when they share nothing."""
    sets = []
    for name in networks:
        key = get_network(name).key
        sets.append({p.split("/")[0] for p in FEEDS.get(key, {})})
    if not sets:
        return []
    shared = set.intersection(*sets)
    return sorted(shared)


def describe_feed(pair: str, network: str) -> Dict[str, object]:
    """Everything the registry knows about one feed, without reading a chain.

    Includes where else the same pair lives, which is the context that turns "this feed
    has a 24-hour heartbeat" into "…and the same pair updates every 60s on Polygon".
    """
    resolved = resolve(pair, network)
    feed = get_feed(resolved, network)
    elsewhere = [n for n in networks_carrying(resolved) if n != get_network(network).key]
    quickest = fastest(resolved)
    return {
        **FeedLocation(feed, get_network(network).key).as_dict(),
        "also_on": elsewhere,
        "fastest_network": quickest.network if quickest else None,
        "fastest_heartbeat_secs": quickest.feed.heartbeat_secs if quickest else None,
        "stale_after_secs": feed.stale_after_secs,
    }
