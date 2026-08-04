"""Chainlink price feed registry and reader.

Every address in this registry was verified against its live aggregator: each one was
called for ``description()`` and ``decimals()``, and the pair label below is the
contract's own reported description, not a guess. That check is worth doing — the
address widely passed around as Base "BTC/USD" reports ``WBTC / USD``, and it is
registered here under the name it actually answers to.

Re-run the check any time with ``alchem-link verify --network <net>``.
"""
from __future__ import annotations

import time
from dataclasses import dataclass
from typing import Dict, List, Optional

from .abi import (
    decode_string,
    scale,
    to_int,
    to_uint,
    words,
)
from .networks import DEFAULT_NETWORK, get_network
from .rpc import RpcClient, client_for

#: Fallback staleness threshold when a feed has no explicit heartbeat, in seconds.
DEFAULT_HEARTBEAT_SECS = 3600


@dataclass(frozen=True)
class Feed:
    pair: str
    address: str
    decimals: int
    #: Publish interval the feed is expected to beat. Treat as a conservative default,
    #: not gospel — confirm the official heartbeat for anything you put money behind.
    heartbeat_secs: int = DEFAULT_HEARTBEAT_SECS
    note: str = ""


# Stablecoin feeds publish on a much longer cadence than volatile pairs; mainnet
# USDC/USD was observed ~13 hours between updates while ETH/USD moved every few minutes.
_STABLE_HEARTBEAT = 86_400

FEEDS: Dict[str, Dict[str, Feed]] = {
    "ethereum": {
        "ETH/USD":  Feed("ETH/USD",  "0x5f4eC3Df9cbd43714FE2740f5E3616155c5b8419", 8),
        "BTC/USD":  Feed("BTC/USD",  "0xF4030086522a5bEEa4988F8cA5B36dbC97BeE88c", 8),
        "LINK/USD": Feed("LINK/USD", "0x2c1d072e956AFFC0D435Cb7AC38EF18d24d9127c", 8),
        "SOL/USD":  Feed("SOL/USD",  "0x4ffC43a60e009B551865A93d232E33Fce9f01507", 8),
        "USDC/USD": Feed("USDC/USD", "0x8fFfFfd4AfB6115b954Bd326cbe7B4BA576818f6", 8, _STABLE_HEARTBEAT),
        "DAI/USD":  Feed("DAI/USD",  "0xAed0c38402a5d19df6E4c03F4E2DceD6e29c1ee9", 8, _STABLE_HEARTBEAT),
    },
    "sepolia": {
        "ETH/USD":  Feed("ETH/USD",  "0x694AA1769357215DE4FAC081bf1f309aDC325306", 8),
        "BTC/USD":  Feed("BTC/USD",  "0x1b44F3514812d835EB1BDB0acB33d3fA3351Ee43", 8),
        "LINK/USD": Feed("LINK/USD", "0xc59E3633BAAC79493d908e63626716e204A45EdF", 8),
    },
    "base": {
        "ETH/USD":  Feed("ETH/USD",  "0x71041dddad3595F9CEd3DcCFBe3D1F4b0a16Bb70", 8),
        # Reports "WBTC / USD" on-chain. Registered under its real name on purpose:
        # WBTC can depeg from BTC, so quoting it as BTC/USD would be a live foot-gun.
        "WBTC/USD": Feed(
            "WBTC/USD", "0xCCADC697c55bbB68dc5bCdf8d3CBe83CdD4E071E", 8,
            note="Wrapped BTC, not spot BTC — can depeg.",
        ),
    },
    "arbitrum": {
        "ETH/USD":  Feed("ETH/USD",  "0x639Fe6ab55C921f74e7fac1ee960C0B6293ba612", 8),
        "BTC/USD":  Feed("BTC/USD",  "0x6ce185860a4963106506C203335A2910413708e9", 8),
        "LINK/USD": Feed("LINK/USD", "0x86E53CF1B870786351Da77A57575e79CB55812CB", 8),
    },
    "optimism": {
        "ETH/USD":  Feed("ETH/USD",  "0x13e3Ee699D1909E989722E753853AE30b17e08c5", 8),
        "BTC/USD":  Feed("BTC/USD",  "0xD702DD976Fb76Fffc2D3963D037dfDae5b04E593", 8),
    },
    "polygon": {
        "ETH/USD":   Feed("ETH/USD",   "0xF9680D99D6C9589e2a93a78A04A279e509205945", 8),
        "BTC/USD":   Feed("BTC/USD",   "0xc907E116054Ad103354f2D350FD2514433D57F6f", 8),
        "MATIC/USD": Feed("MATIC/USD", "0xAB594600376Ec9fD91F8e885dADF0CE036862dE0", 8),
    },
}


def list_feeds(network: str = DEFAULT_NETWORK) -> List[Feed]:
    get_network(network)  # validates the network name
    return list(FEEDS.get(network.lower(), {}).values())


def get_feed(pair: str, network: str = DEFAULT_NETWORK) -> Feed:
    get_network(network)
    table = FEEDS.get(network.lower(), {})
    key = pair.upper().replace("-", "/").strip()
    if key not in table:
        known = ", ".join(sorted(table)) or "none"
        raise KeyError(f"no feed '{pair}' on {network}. Known pairs: {known}")
    return table[key]


@dataclass
class FeedReading:
    pair: str
    network: str
    address: str
    #: Contract's own description(), so a mislabelled registry entry is visible.
    description: str
    price: float
    answer_raw: int
    decimals: int
    round_id: int
    updated_at: int
    age_secs: int
    heartbeat_secs: int
    stale: bool
    note: str = ""

    @property
    def status(self) -> str:
        if self.answer_raw <= 0:
            return "INVALID"
        return "STALE" if self.stale else "FRESH"

    def as_dict(self) -> Dict[str, object]:
        return {
            "pair": self.pair,
            "network": self.network,
            "address": self.address,
            "description": self.description,
            "price": self.price,
            "answer_raw": self.answer_raw,
            "decimals": self.decimals,
            "round_id": self.round_id,
            "updated_at": self.updated_at,
            "age_secs": self.age_secs,
            "heartbeat_secs": self.heartbeat_secs,
            "stale": self.stale,
            "status": self.status,
            "note": self.note,
        }


def decode_reading(
    feed: Feed,
    network: str,
    raw: Dict[str, str],
    now: Optional[int] = None,
) -> FeedReading:
    """Turn the three raw aggregator responses into a checked reading.

    Pure: no I/O, so the staleness and decoding logic is testable offline.
    """
    round_words = words(raw["latest_round_data"])
    if len(round_words) < 5:
        raise ValueError(
            f"latestRoundData() returned {len(round_words)} words, expected 5 — "
            f"is {feed.address} an AggregatorV3 contract?"
        )
    round_id = to_uint(round_words[0])
    answer = to_int(round_words[1])
    updated_at = to_uint(round_words[3])

    decimals = to_uint(words(raw["decimals"])[0])
    description = decode_string(raw["description"]).strip()

    current = int(time.time()) if now is None else now
    # A feed timestamped in the future is a clock problem, not negative age.
    age = max(0, current - updated_at)

    return FeedReading(
        pair=feed.pair,
        network=network,
        address=feed.address,
        description=description,
        price=scale(answer, decimals),
        answer_raw=answer,
        decimals=decimals,
        round_id=round_id,
        updated_at=updated_at,
        age_secs=age,
        heartbeat_secs=feed.heartbeat_secs,
        stale=age > feed.heartbeat_secs,
        note=feed.note,
    )


def read_feed(
    pair: str,
    network: str = DEFAULT_NETWORK,
    client: Optional[RpcClient] = None,
    rpc_url: Optional[str] = None,
) -> FeedReading:
    """Read one Chainlink feed live and return a checked reading."""
    feed = get_feed(pair, network)
    rpc = client or client_for(network=network, rpc_url=rpc_url)
    return decode_reading(feed, network.lower(), rpc.read_aggregator(feed.address))


def read_all_feeds(
    network: str = DEFAULT_NETWORK,
    client: Optional[RpcClient] = None,
    rpc_url: Optional[str] = None,
) -> List[FeedReading]:
    """Read every registered feed on a network, skipping ones that fail.

    One unreachable aggregator should not blank the whole board, so failures are
    reported per-feed by omission rather than raised.
    """
    rpc = client or client_for(network=network, rpc_url=rpc_url)
    readings: List[FeedReading] = []
    for feed in list_feeds(network):
        try:
            readings.append(decode_reading(feed, network.lower(), rpc.read_aggregator(feed.address)))
        except Exception:
            continue
    return readings


def verify_registry(
    network: str = DEFAULT_NETWORK,
    client: Optional[RpcClient] = None,
    rpc_url: Optional[str] = None,
) -> List[Dict[str, object]]:
    """Check each registered address still reports the pair we filed it under.

    This is the check that caught the Base WBTC mislabel. Run it after editing FEEDS.
    """
    rpc = client or client_for(network=network, rpc_url=rpc_url)
    results: List[Dict[str, object]] = []
    for feed in list_feeds(network):
        entry: Dict[str, object] = {"pair": feed.pair, "address": feed.address}
        try:
            reading = decode_reading(feed, network.lower(), rpc.read_aggregator(feed.address))
            on_chain = reading.description.replace(" ", "").upper()
            entry.update(
                {
                    "ok": on_chain == feed.pair.replace(" ", "").upper(),
                    "description": reading.description,
                    "decimals": reading.decimals,
                    "declared_decimals": feed.decimals,
                    "price": reading.price,
                    "status": reading.status,
                }
            )
        except Exception as exc:
            entry.update({"ok": False, "error": str(exc)})
        results.append(entry)
    return results
