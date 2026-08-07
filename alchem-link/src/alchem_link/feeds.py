"""Chainlink price feed registry and reader.

Two promises hold this file together, and both are checkable rather than asserted.

**Every address was called before it was written down.** Each entry was queried for
``description()`` and ``decimals()``, and is filed under the pair the contract itself
reports. That check keeps catching things: the address widely passed around as Base
"BTC/USD" reports ``WBTC / USD``, and the Gnosis address commonly labelled "xDAI/USD"
reports ``DAI / USD``. Both are registered under the names they answer to. Re-run the
check any time with ``alchem-link verify -n <network>``.

**Every heartbeat was measured, not copied.** This registry used to declare 3600s for
everything, inherited from Ethereum mainnet. That is wrong almost everywhere: Polygon's
feeds publish every ~60 seconds, Optimism and Base every ~1200, Arbitrum's USDC every
~300. A 3600s staleness check on a Polygon feed will not fire until the feed has been
dead for an hour. The values below come from :mod:`alchem_link.cadence` walking each
feed's round history — regenerate them with ``alchem-link cadence -n <network>``.

Heartbeats marked ``measured=False`` are *bounds*, not measurements: the sampling window
never contained a quiet period long enough for the clock rather than a price move to
trigger the publish, so all that is known is that the heartbeat is at least the observed
ceiling. Those carry a deliberately conservative value, because a too-tight heartbeat
produces false staleness alarms, and a tool that cries wolf gets ignored.
"""
from __future__ import annotations

import time
from dataclasses import dataclass
from typing import Any, Dict, List, Optional

from .abi import decode_string, scale, to_int, to_uint, words
from .errors import UnknownFeed
from .multicall import Call, batch_call
from .networks import DEFAULT_NETWORK, get_network
from .rpc import RpcClient, client_for

#: Fallback staleness threshold when a feed has no explicit heartbeat, in seconds.
DEFAULT_HEARTBEAT_SECS = 3600

#: Slack allowed on top of the heartbeat before calling a feed stale.
#:
#: A "1 hour" heartbeat does not mean 3600.000 seconds. Measured ceilings run a percent
#: or two over — mainnet ETH/USD was observed at 3684s against a 3600s configuration —
#: because the publish is triggered by block timestamps, not a wall clock. Without slack
#: every feed would flicker STALE at the top of its cycle, which trains people to ignore
#: the flag exactly when it starts meaning something.
STALENESS_TOLERANCE = 0.15

_ROUND_RETURNS = ["uint80", "int256", "uint256", "uint256", "uint80"]


@dataclass(frozen=True)
class Feed:
    pair: str
    address: str
    decimals: int
    #: Publish interval measured from round history, in seconds.
    heartbeat_secs: int = DEFAULT_HEARTBEAT_SECS
    #: True when a heartbeat-triggered publish was actually observed. False means the
    #: value is a conservative upper bound — see the module docstring.
    heartbeat_measured: bool = True
    note: str = ""

    @property
    def stale_after_secs(self) -> int:
        """Age at which this feed is treated as stale, tolerance included."""
        return int(self.heartbeat_secs * (1 + STALENESS_TOLERANCE))


FEEDS: Dict[str, Dict[str, Feed]] = {
    "ethereum": {
        "ETH/USD":   Feed("ETH/USD",   "0x5f4eC3Df9cbd43714FE2740f5E3616155c5b8419", 8, 3600),
        "BTC/USD":   Feed("BTC/USD",   "0xF4030086522a5bEEa4988F8cA5B36dbC97BeE88c", 8, 3600),
        "LINK/USD":  Feed("LINK/USD",  "0x2c1d072e956AFFC0D435Cb7AC38EF18d24d9127c", 8, 3600),
        "DAI/USD":   Feed("DAI/USD",   "0xAed0c38402a5d19df6E4c03F4E2DceD6e29c1ee9", 8, 3600),
        "AAVE/USD":  Feed("AAVE/USD",  "0x547a514d5e3769680Ce22B2361c10Ea13619e8a9", 8, 3600),
        "UNI/USD":   Feed("UNI/USD",   "0x553303d460EE0afB37EdFf9bE42922D8FF63220e", 8, 3600),
        "ETH/BTC":   Feed("ETH/BTC",   "0xAc559F25B1619171CbC396a50854A3240b6A4e99", 8, 3600),
        "STETH/USD": Feed(
            "STETH/USD", "0xCfE54B5cD566aB89272946F602D76Ea879CAb4a8", 8, 3600,
            note="Liquid-staked ETH, not spot ETH — trades at its own price and can discount.",
        ),
        "XAU/USD":   Feed("XAU/USD",   "0x214eD9Da11D2fbe465a6fc601a91E62EbEc1a0D6", 8, 14400),
        # Cross-chain asset prices published on mainnet update far more slowly than the
        # same pair on that asset's own chain. SOL/USD here is a day's heartbeat; on
        # Polygon it is a minute.
        "SOL/USD":   Feed("SOL/USD",   "0x4ffC43a60e009B551865A93d232E33Fce9f01507", 8, 86400),
        "AVAX/USD":  Feed("AVAX/USD",  "0xFF3EEb22B5E3dE6e705b44749C2559d704923FD7", 8, 86400),
        "BNB/USD":   Feed("BNB/USD",   "0x14e613AC84a31f709eadbdF89C6CC390fDc9540A", 8, 86400),
        "MATIC/USD": Feed("MATIC/USD", "0x7bAC85A8a13A4BcD8abb3eB7d6b4d632c5a57676", 8, 86400),
        "EUR/USD":   Feed("EUR/USD",   "0xb49f677943BC038e9857d61E7d053CaA2C1734C1", 8, 86400),
        "USDC/USD":  Feed("USDC/USD",  "0x8fFfFfd4AfB6115b954Bd326cbe7B4BA576818f6", 8, 86400),
        "USDT/USD":  Feed("USDT/USD",  "0x3E7d1eAB13ad0104d2750B8863b489D65364e32D", 8, 86400),
    },
    "sepolia": {
        "ETH/USD":  Feed("ETH/USD",  "0x694AA1769357215DE4FAC081bf1f309aDC325306", 8, 3600),
        "BTC/USD":  Feed("BTC/USD",  "0x1b44F3514812d835EB1BDB0acB33d3fA3351Ee43", 8, 3600),
        "LINK/USD": Feed("LINK/USD", "0xc59E3633BAAC79493d908e63626716e204A45EdF", 8, 3600),
    },
    "base": {
        "ETH/USD":  Feed("ETH/USD",  "0x71041dddad3595F9CEd3DcCFBe3D1F4b0a16Bb70", 8, 1200),
        # Reports "WBTC / USD" on-chain. Registered under its real name on purpose:
        # WBTC can depeg from BTC, so quoting it as BTC/USD would be a live foot-gun.
        "WBTC/USD": Feed(
            "WBTC/USD", "0xCCADC697c55bbB68dc5bCdf8d3CBe83CdD4E071E", 8, 1200,
            note="Wrapped BTC, not spot BTC — can depeg.",
        ),
        "CBETH/USD": Feed(
            "CBETH/USD", "0xd7818272B9e248357d13057AAb0B417aF31E817d", 8, 1200,
            note="Coinbase staked ETH — priced against its own market, not ETH spot.",
        ),
        "LINK/USD": Feed("LINK/USD", "0x17CAb8FE31E32f08326e5E27412894e49B0f9D65", 8, 43200, False),
        "USDC/USD": Feed("USDC/USD", "0x7e860098F58bBFC8648a4311b374B1D669a2bc6B", 8, 86400),
        "DAI/USD":  Feed("DAI/USD",  "0x591e79239a7d679378eC8c847e5038150364C78F", 8, 86400),
    },
    "arbitrum": {
        "ARB/USD":  Feed("ARB/USD",  "0xb2A824043730FE05F3DA2efaFa1CBbe83fa548D6", 8, 300),
        "USDC/USD": Feed("USDC/USD", "0x50834F3163758fcC1Df9973b6e91f0F0F0434aD3", 8, 300),
        "USDT/USD": Feed("USDT/USD", "0x3f3f5dF88dC9F13eac63DF89EC16ef6e7E25DdE7", 8, 300),
        "ETH/USD":  Feed("ETH/USD",  "0x639Fe6ab55C921f74e7fac1ee960C0B6293ba612", 8, 600),
        "BTC/USD":  Feed("BTC/USD",  "0x6ce185860a4963106506C203335A2910413708e9", 8, 900, False),
        "LINK/USD": Feed("LINK/USD", "0x86E53CF1B870786351Da77A57575e79CB55812CB", 8, 1800),
        "SOL/USD":  Feed("SOL/USD",  "0x24ceA4b8ce57cdA5058b924B9B9987992450590c", 8, 3600, False),
        "DAI/USD":  Feed("DAI/USD",  "0xc5C8E77B397E531B8EC06BFb0048328B30E9eCfB", 8, 86400),
    },
    "optimism": {
        "ETH/USD":  Feed("ETH/USD",  "0x13e3Ee699D1909E989722E753853AE30b17e08c5", 8, 1200),
        "BTC/USD":  Feed("BTC/USD",  "0xD702DD976Fb76Fffc2D3963D037dfDae5b04E593", 8, 1200),
        "LINK/USD": Feed("LINK/USD", "0xCc232dcFAAE6354cE191Bd574108c1aD03f86450", 8, 1200),
        "OP/USD":   Feed("OP/USD",   "0x0D276FC14719f9292D5C1eA2198673d1f4269246", 8, 1200),
        "USDC/USD": Feed("USDC/USD", "0x16a9FA2FDa030272Ce99B29CF780dFA30361E0f3", 8, 86400),
        "DAI/USD":  Feed("DAI/USD",  "0x8dBa75e83DA73cc766A7e5a0ee71F656BAb470d6", 8, 86400),
    },
    # Polygon runs the fastest feeds in the registry — a ~60s heartbeat across the board.
    "polygon": {
        "ETH/USD":   Feed("ETH/USD",   "0xF9680D99D6C9589e2a93a78A04A279e509205945", 8, 60),
        "BTC/USD":   Feed("BTC/USD",   "0xc907E116054Ad103354f2D350FD2514433D57F6f", 8, 60),
        "MATIC/USD": Feed("MATIC/USD", "0xAB594600376Ec9fD91F8e885dADF0CE036862dE0", 8, 60),
        "LINK/USD":  Feed("LINK/USD",  "0xd9FFdb71EbE7496cC440152d43986Aae0AB76665", 8, 60),
        "SOL/USD":   Feed("SOL/USD",   "0x10C8264C0935b3B9870013e057f330Ff3e9C56dC", 8, 60),
        "USDC/USD":  Feed("USDC/USD",  "0xfE4A8cc5b5B2366C1B58Bea3858e81843581b2F7", 8, 60),
        "DAI/USD":   Feed("DAI/USD",   "0x4746DeC9e833A82EC7C2C1356372CcF2cfcD2F3D", 8, 60),
    },
    "avalanche": {
        "AVAX/USD": Feed("AVAX/USD", "0x0A77230d17318075983913bC2145DB16C7366156", 8, 120),
        "ETH/USD":  Feed("ETH/USD",  "0x976B3D034E162d8bD72D6b9C989d545b839003b0", 8, 7200, False),
        "BTC/USD":  Feed("BTC/USD",  "0x2779D32d5166BAaa2B2b658333bA7e6Ec0C65743", 8, 7200, False),
        "LINK/USD": Feed("LINK/USD", "0x49ccd9ca821EfEab2b98c60dC60F518E765EDe9a", 8, 14400),
        "USDC/USD": Feed("USDC/USD", "0xF096872672F44d6EBA71458D74fe67F9a77a23B9", 8, 86400),
    },
    "bnb": {
        "BNB/USD":  Feed("BNB/USD",  "0x0567F2323251f0Aab15c8dFb1967E4e8A7D42aeE", 8, 60),
        "ETH/USD":  Feed("ETH/USD",  "0x9ef1B8c0E4F7dc8bF5719Ea496883DC6401d5b2e", 8, 60),
        "BTC/USD":  Feed("BTC/USD",  "0x264990fbd0A4796A3E3d8E37C4d5F87a3aCa5Ebf", 8, 60),
        "LINK/USD": Feed("LINK/USD", "0xca236E327F629f9Fc2c30A4E95775EbF0B89fac8", 8, 600),
        "USDC/USD": Feed("USDC/USD", "0x51597f405303C4377E36123cBc172b13269EA163", 8, 900),
    },
    "gnosis": {
        # Widely circulated as "xDAI/USD"; the contract reports "DAI / USD".
        "DAI/USD":  Feed(
            "DAI/USD",  "0x678df3415fc31947dA4324eC63212874be5a82f8", 8, 86400,
            note="Often shared as xDAI/USD — the contract reports DAI / USD.",
        ),
        "ETH/USD":  Feed("ETH/USD",  "0xa767f745331D267c7751297D982b050c93985627", 8, 86400, False),
        "BTC/USD":  Feed("BTC/USD",  "0x6C1d7e76EF7304a40e8456ce883BC56d3dEA3F7d", 8, 86400, False),
        "LINK/USD": Feed("LINK/USD", "0xed322A5ac55BAE091190dFf9066760b86751947B", 8, 43200, False),
    },
    "scroll": {
        "ETH/USD":  Feed("ETH/USD",  "0x6bF14CB0A831078629D993FDeBcB182b21A8774C", 8, 86400, False),
        "BTC/USD":  Feed("BTC/USD",  "0xCaca6BFdeDA537236Ee406437D2F8a400026C589", 8, 86400, False),
        "USDC/USD": Feed("USDC/USD", "0x43d12Fb3AfCAd5347fA764EeAB105478337b7200", 8, 86400),
    },
    "linea": {
        "ETH/USD":  Feed("ETH/USD",  "0x3c6Cd9Cc7c7a4c2Cf5a82734CD249D7D593354dA", 8, 86400, False),
        "BTC/USD":  Feed("BTC/USD",  "0x7A99092816C8BD5ec8ba229e3a6E6Da1E628E1F9", 8, 86400, False),
        "USDC/USD": Feed("USDC/USD", "0xAADAa473C1bDF7317ec07c915680Af29DeBfdCb5", 8, 86400),
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
        raise UnknownFeed(pair, network, known=list(table))
    return table[key]


def feed_count() -> int:
    return sum(len(table) for table in FEEDS.values())


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
    answered_in_round: int = 0
    heartbeat_measured: bool = True

    @property
    def carried_over(self) -> bool:
        """The answer was carried from an earlier round rather than freshly produced."""
        return bool(self.answered_in_round) and self.answered_in_round < self.round_id

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
            "heartbeat_measured": self.heartbeat_measured,
            "stale": self.stale,
            "carried_over": self.carried_over,
            "status": self.status,
            "note": self.note,
        }


def _build_reading(
    feed: Feed,
    network: str,
    round_values: List[Any],
    decimals: int,
    description: str,
    now: Optional[int] = None,
) -> FeedReading:
    round_id, answer, _started, updated_at, answered_in = round_values
    current = int(time.time()) if now is None else now
    # A feed timestamped in the future is a clock problem, not negative age.
    age = max(0, current - updated_at)

    return FeedReading(
        pair=feed.pair,
        network=network,
        address=feed.address,
        description=description.strip(),
        price=scale(answer, decimals),
        answer_raw=answer,
        decimals=decimals,
        round_id=round_id,
        updated_at=updated_at,
        age_secs=age,
        heartbeat_secs=feed.heartbeat_secs,
        stale=age > feed.stale_after_secs,
        note=feed.note,
        answered_in_round=answered_in,
        heartbeat_measured=feed.heartbeat_measured,
    )


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
    values = [
        to_uint(round_words[0]),
        to_int(round_words[1]),
        to_uint(round_words[2]),
        to_uint(round_words[3]),
        to_uint(round_words[4]),
    ]
    return _build_reading(
        feed,
        network,
        values,
        decimals=to_uint(words(raw["decimals"])[0]),
        description=decode_string(raw["description"]),
        now=now,
    )


def read_feed(
    pair: str,
    network: str = DEFAULT_NETWORK,
    client: Optional[RpcClient] = None,
    rpc_url: Optional[str] = None,
    now: Optional[int] = None,
) -> FeedReading:
    """Read one Chainlink feed live and return a checked reading."""
    feed = get_feed(pair, network)
    rpc = client or client_for(network=network, rpc_url=rpc_url)
    return decode_reading(feed, network.lower(), rpc.read_aggregator(feed.address), now=now)


def _read_many(
    feeds: List[Feed],
    network: str,
    rpc: RpcClient,
    now: Optional[int] = None,
) -> Dict[str, FeedReading]:
    """Read a list of feeds in one batch. Failures are omitted, not raised."""
    calls: List[Call] = []
    for feed in feeds:
        calls.append(Call(feed.address, "latestRoundData()", (), _ROUND_RETURNS, f"{feed.pair}|round"))
        calls.append(Call(feed.address, "decimals()", (), ["uint8"], f"{feed.pair}|decimals"))
        calls.append(Call(feed.address, "description()", (), ["string"], f"{feed.pair}|description"))

    report = batch_call(rpc, calls)
    readings: Dict[str, FeedReading] = {}
    for feed in feeds:
        round_result = report.by_label(f"{feed.pair}|round")
        decimals_result = report.by_label(f"{feed.pair}|decimals")
        description_result = report.by_label(f"{feed.pair}|description")
        if not round_result or not round_result.success:
            continue
        readings[feed.pair] = _build_reading(
            feed,
            network.lower(),
            round_result.values,
            decimals=int(decimals_result.one(feed.decimals) if decimals_result else feed.decimals),
            description=str((description_result.one("") if description_result else "") or feed.pair),
            now=now,
        )
    return readings


def read_all_feeds(
    network: str = DEFAULT_NETWORK,
    client: Optional[RpcClient] = None,
    rpc_url: Optional[str] = None,
    now: Optional[int] = None,
) -> List[FeedReading]:
    """Read every registered feed on a network in a single batched round trip.

    One unreachable aggregator should not blank the whole board, so failures are
    reported per-feed by omission rather than raised.
    """
    rpc = client or client_for(network=network, rpc_url=rpc_url)
    feeds = list_feeds(network)
    if not feeds:
        return []
    readings = _read_many(feeds, network, rpc, now=now)
    return [readings[feed.pair] for feed in feeds if feed.pair in readings]


def verify_registry(
    network: str = DEFAULT_NETWORK,
    client: Optional[RpcClient] = None,
    rpc_url: Optional[str] = None,
    now: Optional[int] = None,
) -> List[Dict[str, object]]:
    """Check each registered address still reports the pair we filed it under.

    This is the check that caught the Base WBTC and Gnosis xDAI mislabels. Run it after
    editing FEEDS.
    """
    rpc = client or client_for(network=network, rpc_url=rpc_url)
    feeds = list_feeds(network)
    readings = _read_many(feeds, network, rpc, now=now) if feeds else {}

    results: List[Dict[str, object]] = []
    for feed in feeds:
        entry: Dict[str, object] = {"pair": feed.pair, "address": feed.address}
        reading = readings.get(feed.pair)
        if reading is None:
            entry.update({"ok": False, "error": "could not read the aggregator"})
            results.append(entry)
            continue
        on_chain = reading.description.replace(" ", "").upper()
        entry.update({
            "ok": on_chain == feed.pair.replace(" ", "").upper()
                  and reading.decimals == feed.decimals,
            "description": reading.description,
            "decimals": reading.decimals,
            "declared_decimals": feed.decimals,
            "price": reading.price,
            "status": reading.status,
            "heartbeat_secs": feed.heartbeat_secs,
            "heartbeat_measured": feed.heartbeat_measured,
        })
        results.append(entry)
    return results
