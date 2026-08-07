"""Alchem-Link — an Alchemy x Chainlink developer toolkit that reads chains, not docs.

Standard library only. Every capability below works against a live network, and the
keyless public endpoints make that true before you have an API key.

**Read**
    :func:`read_feed`, :func:`read_all_feeds` — verified feeds with a staleness verdict
    measured against the feed's real heartbeat. :func:`batch_call` collapses many
    contract reads into one block-atomic round trip via Multicall3.

**Audit**
    :func:`audit_feed` runs the consumer-safety checks that ``latestRoundData()`` will
    never fail for you: stale rounds, carried answers, pinned circuit breakers, and the
    L2 sequencer gate. :func:`generate_consumer` emits code that passes them.

**Measure**
    :func:`profile_feed` recovers a feed's real heartbeat and deviation threshold from
    round history. :func:`compare_pair` measures the same pair across every chain that
    carries it, in basis points.

**Compose**
    :func:`analyse_gas` prices EIP-1559 fees in USD through the chain's own oracle;
    :func:`value_holdings` does the same for a portfolio.

The package implements :func:`keccak256` itself — ``hashlib`` ships SHA3-256, whose
padding differs — so function selectors are computed rather than trusted.
"""

__version__ = "0.4.0"

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
from .recipes import get_recipe_by_id, get_recipes
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
from .watch import WatchEvent, poll_interval_for, watch_feed

__all__ = [
    "__version__",
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
    # diagnostics
    "diagnose",
    "Diagnosis",
    "Check",
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
