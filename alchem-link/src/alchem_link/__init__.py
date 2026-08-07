"""Alchem-Link — an Alchemy x Chainlink developer toolkit that reads chains, not docs.

Standard library only, user interface included. Every capability below works against a
live network, and the keyless public endpoints make that true before you have an API key.

**Connect**
    :class:`AlchemLink` holds a network, a connection and a per-feed cache, so a session
    is one client rather than one per call. ``connect("base").price("ETH/USD")``.

**Read**
    :func:`read_feed`, :func:`read_all_feeds` — verified feeds with a staleness verdict
    measured against the feed's real heartbeat. :func:`batch_call` collapses many
    contract reads into one block-atomic round trip via Multicall3, and
    :func:`read_pair_everywhere` fans one pair across every chain concurrently.

**Audit**
    :func:`audit_feed` runs the consumer-safety checks that ``latestRoundData()`` will
    never fail for you: stale rounds, carried answers, pinned circuit breakers, and the
    L2 sequencer gate. :func:`generate_consumer` emits code that passes them.

**Measure**
    :func:`profile_feed` recovers a feed's real heartbeat and deviation threshold from
    round history. :func:`compare_pair` measures the same pair across every chain that
    carries it, in basis points. :func:`summarise` turns a history into TWAP, volatility
    and drawdown; :func:`answer_updates` reads that history from event logs.

**Simulate**
    :func:`audit_guard` replays your consumer's checks against every known oracle failure
    mode — the LUNA bounded-crash shape, a frozen feed, an L2 sequencer outage — and tells
    you which ones get through.

**Compose**
    :func:`analyse_gas` prices EIP-1559 fees in USD through the chain's own oracle;
    :func:`value_holdings` does the same for a portfolio; :func:`export` renders any of it
    as CSV, NDJSON, Markdown or a Prometheus scrape body.

**Show**
    :mod:`alchem_link.term` is a complete terminal UI toolkit — screen diffing, colour
    depth negotiation, widgets, an event loop — with no dependencies, which is what lets
    the dashboard and the palette survive the zero-dependency claim.

The package implements :func:`keccak256` itself — ``hashlib`` ships SHA3-256, whose
padding differs — so function selectors are computed rather than trusted.
"""

__version__ = "0.23.0"

from .abi import (
    AbiError,
    AbiType,
    decode_args,
    decode_revert,
    decode_string,
    encode_args,
    encode_call,
    parse_signature,
    parse_type,
    scale,
    to_int,
    to_uint,
    words,
)
from .aggregator import (
    AggregatorInfo,
    Round,
    describe_aggregator,
    join_round_id,
    round_history,
    split_round_id,
)
from .analytics import (
    Point,
    Series,
    Stats,
    align,
    correlation,
    largest_move,
    log_returns,
    max_drawdown,
    mean_price,
    median_interval,
    outliers,
    simple_returns,
    spread_bps,
    summarise,
    twap,
    volatility,
)
from .cache import TTLCache, key_for, memoize, ttl_for_feed
from .cadence import CadenceProfile, profile_feed, profile_rounds
from .ccip import (
    CHAIN_SELECTORS,
    ROUTERS,
    Lane,
    ccip_networks,
    list_lanes,
    summarize_chainlink_capabilities,
    verify_lanes,
)
from .client import AlchemLink, connect
from .codegen import GeneratedConsumer, generate_consumer
from .divergence import (
    DivergenceReport,
    Leg,
    common_pairs,
    compare_all,
    compare_pair,
    networks_carrying,
)
from .enhanced import (
    Holdings,
    NeedsAlchemyKey,
    TokenBalance,
    get_asset_transfers,
    get_token_balances,
    summarize_alchemy_capabilities,
    value_holdings,
)
from .errors import (
    AlchemLinkError,
    ConfigurationError,
    EncodingError,
    FeedError,
    InvalidAnswer,
    MissingCredential,
    ProtocolError,
    SimulationError,
    StaleFeed,
    TransportError,
    UnknownFeed,
    UnknownNetwork,
    UnreadableFeed,
)
from .exporters import (
    FORMATS,
    export,
    to_csv,
    to_json,
    to_markdown,
    to_ndjson,
    to_prometheus,
    to_table,
)
from .feeds import (
    FEEDS,
    STALENESS_TOLERANCE,
    Feed,
    FeedReading,
    decode_reading,
    feed_count,
    get_feed,
    list_feeds,
    read_all_feeds,
    read_feed,
    verify_registry,
)
from .gas import FeeEstimate, GasReport, analyse_gas
from .health import Check, Diagnosis, diagnose
from .integration import build_integration_map, build_package_blueprint
from .keccak import (
    event_topic,
    is_checksum_address,
    keccak256,
    keccak256_hex,
    selector,
    to_checksum_address,
)
from .logs import (
    ANSWER_UPDATED,
    NEW_ROUND,
    TRANSFER,
    AnswerUpdate,
    DecodedLog,
    EventSpec,
    Log,
    answer_updates,
    decode_log,
    fetch_events,
    get_logs,
    parse_event,
)
from .multicall import (
    MULTICALL3_ADDRESS,
    BatchReport,
    Call,
    CallResult,
    batch_call,
    multicall3,
    supports_multicall3,
)
from .networks import (
    DEFAULT_NETWORK,
    NETWORKS,
    Endpoint,
    Network,
    get_network,
    list_networks,
    resolve_endpoint,
)
from .parallel import (
    Outcome,
    SweepReport,
    gather,
    map_networks,
    read_all_networks,
    read_pair_everywhere,
    run_tasks,
)
from .recipes import get_recipe_by_id, get_recipes
from .registry import (
    FeedLocation,
    all_assets,
    all_locations,
    all_pairs,
    by_address,
    common_assets,
    coverage,
    describe_feed,
    fastest,
    find,
    normalise_pair,
    resolve,
    suggest,
)
from .rpc import (
    BatchOutcome,
    RpcClient,
    RpcError,
    RpcStats,
    RpcTransportError,
    client_for,
    gwei,
)
from .safety import Audit, Finding, audit_feed, audit_network
from .sequencer import (
    GRACE_PERIOD_SECS,
    SEQUENCER_FEEDS,
    SequencerStatus,
    is_l2,
    list_sequencer_feeds,
    read_sequencer,
)
from .simulate import (
    SCENARIOS,
    AuditResult,
    Guard,
    Observation,
    ReplayReport,
    Scenario,
    Verdict,
    audit_guard,
    evaluate,
    observations_from_series,
    replay,
    run_scenario,
)
from .watch import WatchEvent, poll_interval_for, watch_feed

__all__ = [
    "__version__",
    # client facade
    "AlchemLink",
    "connect",
    # feeds
    "Feed",
    "FeedReading",
    "FEEDS",
    "STALENESS_TOLERANCE",
    "read_feed",
    "read_all_feeds",
    "list_feeds",
    "get_feed",
    "feed_count",
    "decode_reading",
    "verify_registry",
    # registry search
    "FeedLocation",
    "all_assets",
    "all_locations",
    "all_pairs",
    "by_address",
    "common_assets",
    "coverage",
    "describe_feed",
    "fastest",
    "find",
    "normalise_pair",
    "resolve",
    "suggest",
    # rpc + batching
    "RpcClient",
    "RpcError",
    "RpcTransportError",
    "RpcStats",
    "BatchOutcome",
    "client_for",
    "gwei",
    "Call",
    "CallResult",
    "BatchReport",
    "batch_call",
    "multicall3",
    "supports_multicall3",
    "MULTICALL3_ADDRESS",
    # concurrency
    "Outcome",
    "SweepReport",
    "gather",
    "map_networks",
    "read_all_networks",
    "read_pair_everywhere",
    "run_tasks",
    # caching
    "TTLCache",
    "key_for",
    "memoize",
    "ttl_for_feed",
    # networks
    "Network",
    "NETWORKS",
    "DEFAULT_NETWORK",
    "Endpoint",
    "get_network",
    "list_networks",
    "resolve_endpoint",
    # aggregator introspection
    "AggregatorInfo",
    "Round",
    "describe_aggregator",
    "round_history",
    "split_round_id",
    "join_round_id",
    # event logs
    "ANSWER_UPDATED",
    "NEW_ROUND",
    "TRANSFER",
    "AnswerUpdate",
    "DecodedLog",
    "EventSpec",
    "Log",
    "answer_updates",
    "decode_log",
    "fetch_events",
    "get_logs",
    "parse_event",
    # safety
    "Audit",
    "Finding",
    "audit_feed",
    "audit_network",
    "SequencerStatus",
    "SEQUENCER_FEEDS",
    "GRACE_PERIOD_SECS",
    "read_sequencer",
    "list_sequencer_feeds",
    "is_l2",
    # simulation
    "SCENARIOS",
    "AuditResult",
    "Guard",
    "Observation",
    "ReplayReport",
    "Scenario",
    "Verdict",
    "audit_guard",
    "evaluate",
    "observations_from_series",
    "replay",
    "run_scenario",
    # measurement
    "CadenceProfile",
    "profile_feed",
    "profile_rounds",
    "DivergenceReport",
    "Leg",
    "compare_pair",
    "compare_all",
    "common_pairs",
    "networks_carrying",
    "WatchEvent",
    "watch_feed",
    "poll_interval_for",
    # analytics
    "Point",
    "Series",
    "Stats",
    "align",
    "correlation",
    "largest_move",
    "log_returns",
    "max_drawdown",
    "mean_price",
    "median_interval",
    "outliers",
    "simple_returns",
    "spread_bps",
    "summarise",
    "twap",
    "volatility",
    # alchemy side
    "GasReport",
    "FeeEstimate",
    "analyse_gas",
    "Holdings",
    "TokenBalance",
    "NeedsAlchemyKey",
    "value_holdings",
    "get_token_balances",
    "get_asset_transfers",
    "summarize_alchemy_capabilities",
    # ccip
    "Lane",
    "ROUTERS",
    "CHAIN_SELECTORS",
    "verify_lanes",
    "list_lanes",
    "ccip_networks",
    "summarize_chainlink_capabilities",
    # codegen
    "GeneratedConsumer",
    "generate_consumer",
    # export
    "FORMATS",
    "export",
    "to_csv",
    "to_json",
    "to_markdown",
    "to_ndjson",
    "to_prometheus",
    "to_table",
    # diagnostics
    "diagnose",
    "Diagnosis",
    "Check",
    # errors
    "AlchemLinkError",
    "ConfigurationError",
    "EncodingError",
    "FeedError",
    "InvalidAnswer",
    "MissingCredential",
    "ProtocolError",
    "SimulationError",
    "StaleFeed",
    "TransportError",
    "UnknownFeed",
    "UnknownNetwork",
    "UnreadableFeed",
    # abi + keccak
    "AbiError",
    "AbiType",
    "words",
    "to_uint",
    "to_int",
    "decode_string",
    "scale",
    "parse_type",
    "parse_signature",
    "encode_call",
    "encode_args",
    "decode_args",
    "decode_revert",
    "keccak256",
    "keccak256_hex",
    "selector",
    "event_topic",
    "to_checksum_address",
    "is_checksum_address",
    # reference
    "build_package_blueprint",
    "build_integration_map",
    "get_recipes",
    "get_recipe_by_id",
]
