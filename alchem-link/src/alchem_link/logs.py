"""Event logs: fetching them, and decoding them without an ABI file.

``latestRoundData()`` gives you the present. Logs give you the past — every publish, with
its block, its timestamp, and its answer — and they give it to you far more cheaply than
walking ``getRoundData`` one round at a time. A hundred rounds of history is a hundred
``eth_call``s through the round walker and a single ``eth_getLogs`` through this module.

Three things about Chainlink logs are not obvious:

* **The proxy emits nothing.** ``AnswerUpdated`` is emitted by the *implementation*
  aggregator. Filtering on the address you consume returns an empty list and looks like
  a dead feed. :func:`answer_updates` resolves the proxy first; the low-level helpers
  take whatever address you give them and say so.
* **Indexed parameters are not in the data.** ``AnswerUpdated`` declares ``current`` and
  ``roundId`` indexed, so they arrive as topics 1 and 2 and only ``updatedAt`` is in the
  data blob. A decoder that ABI-decodes the data against the full parameter list gets
  three fields' worth of garbage from one field's worth of bytes. :class:`EventSpec`
  splits the two groups.
* **Providers cap the block range.** Public endpoints reject ``eth_getLogs`` spans of
  more than a few thousand blocks, usually with an error that does not say so.
  :func:`get_logs` chunks the range and merges, so the caller asks for what it wants.

Everything here is a plain JSON-RPC read. No indexer, no API key, no ABI JSON.
"""
from __future__ import annotations

import time
from dataclasses import dataclass, field
from typing import Any, Dict, Iterable, List, Optional, Sequence, Tuple

from .abi import AbiError, decode_args, parse_type, scale, to_int, to_uint
from .aggregator import split_round_id
from .keccak import event_topic
from .networks import DEFAULT_NETWORK
from .rpc import RpcClient, RpcError, RpcTransportError, client_for

#: Conservative chunk size. Alchemy allows far more; several public endpoints cap at
#: 10,000 and at least one caps at 2,000 without documenting it.
MAX_BLOCK_SPAN = 2000

#: Rough seconds per block, per network, for turning "the last 6 hours" into a block
#: count. Approximate on purpose — used only to *size* a query, never to timestamp a
#: result, which always comes from the block itself.
BLOCK_TIME_SECS = {
    "ethereum": 12.0, "sepolia": 12.0, "base": 2.0, "arbitrum": 0.25,
    "optimism": 2.0, "polygon": 2.1, "avalanche": 2.0, "bnb": 3.0,
    "gnosis": 5.0, "scroll": 3.0, "linea": 3.0,
}
DEFAULT_BLOCK_TIME = 12.0


@dataclass(frozen=True)
class EventSpec:
    """A parsed event signature, split into indexed and unindexed parameters.

    Built from the human-readable form — ``AnswerUpdated(int256 indexed current,
    uint256 indexed roundId, uint256 updatedAt)`` — because carrying ABI JSON around for
    three events would be the only file this package needed to ship.
    """

    name: str
    #: (type, name, indexed) in declaration order.
    params: Tuple[Tuple[str, str, bool], ...]

    @property
    def signature(self) -> str:
        """The canonical form keccak is taken over — types only, no names, no ``indexed``."""
        return f"{self.name}({','.join(t for t, _, _ in self.params)})"

    @property
    def topic0(self) -> str:
        return event_topic(self.signature)

    @property
    def indexed(self) -> List[Tuple[str, str]]:
        return [(t, n) for t, n, is_indexed in self.params if is_indexed]

    @property
    def unindexed(self) -> List[Tuple[str, str]]:
        return [(t, n) for t, n, is_indexed in self.params if not is_indexed]


def parse_event(declaration: str) -> EventSpec:
    """Parse ``Name(type indexed name, type name)`` into an :class:`EventSpec`."""
    text = declaration.strip()
    open_paren = text.find("(")
    if open_paren == -1 or not text.endswith(")"):
        raise AbiError(f"not an event declaration: '{declaration}'")
    name = text[:open_paren].strip()
    body = text[open_paren + 1:-1].strip()
    params: List[Tuple[str, str, bool]] = []
    if body:
        for raw in body.split(","):
            tokens = raw.split()
            if not tokens:
                raise AbiError(f"empty parameter in '{declaration}'")
            indexed = "indexed" in tokens
            tokens = [t for t in tokens if t != "indexed"]
            param_type = tokens[0]
            parse_type(param_type)  # validate now rather than at decode time
            param_name = tokens[1] if len(tokens) > 1 else f"arg{len(params)}"
            params.append((param_type, param_name, indexed))
    return EventSpec(name=name, params=tuple(params))


#: The two events every Chainlink aggregator emits, and ERC-20 Transfer for the
#: portfolio side. Declared here so callers name a concept rather than a hash.
ANSWER_UPDATED = parse_event(
    "AnswerUpdated(int256 indexed current, uint256 indexed roundId, uint256 updatedAt)"
)
NEW_ROUND = parse_event(
    "NewRound(uint256 indexed roundId, address indexed startedBy, uint256 startedAt)"
)
TRANSFER = parse_event(
    "Transfer(address indexed from, address indexed to, uint256 value)"
)

KNOWN_EVENTS = {
    "AnswerUpdated": ANSWER_UPDATED,
    "NewRound": NEW_ROUND,
    "Transfer": TRANSFER,
}


@dataclass
class Log:
    """One raw log entry, with the hex fields already turned into integers."""

    address: str
    topics: List[str]
    data: str
    block_number: int
    transaction_hash: str
    log_index: int
    removed: bool = False

    @property
    def topic0(self) -> str:
        return self.topics[0] if self.topics else ""

    def as_dict(self) -> Dict[str, Any]:
        return {
            "address": self.address,
            "topics": self.topics,
            "data": self.data,
            "block_number": self.block_number,
            "transaction_hash": self.transaction_hash,
            "log_index": self.log_index,
            "removed": self.removed,
        }


@dataclass
class DecodedLog:
    """A log with its parameters resolved to Python values."""

    event: str
    args: Dict[str, Any]
    log: Log

    @property
    def block_number(self) -> int:
        return self.log.block_number

    def as_dict(self) -> Dict[str, Any]:
        return {
            "event": self.event,
            "args": {k: (v.hex() if isinstance(v, bytes) else v) for k, v in self.args.items()},
            "block_number": self.block_number,
            "transaction_hash": self.log.transaction_hash,
            "log_index": self.log.log_index,
        }


def _to_log(entry: Dict[str, Any]) -> Log:
    return Log(
        address=entry.get("address", ""),
        topics=list(entry.get("topics") or []),
        data=entry.get("data", "0x"),
        block_number=int(entry.get("blockNumber", "0x0"), 16),
        transaction_hash=entry.get("transactionHash", ""),
        log_index=int(entry.get("logIndex", "0x0"), 16),
        removed=bool(entry.get("removed", False)),
    )


def get_logs(client: RpcClient, address: str, topics: Optional[Sequence[Any]] = None,
             from_block: int = 0, to_block: Optional[int] = None,
             max_span: int = MAX_BLOCK_SPAN) -> List[Log]:
    """``eth_getLogs`` over a block range, chunked to survive provider limits.

    A chunk that fails is skipped rather than aborting the query: a partial history is
    still useful, and the alternative — one 400 from one chunk discarding a thousand
    blocks of good data — is not. The result is sorted by (block, log index) because
    merged chunks arrive grouped, not ordered.
    """
    if to_block is None:
        to_block = client.block_number()
    from_block = max(0, from_block)
    if to_block < from_block:
        return []

    collected: List[Log] = []
    start = from_block
    while start <= to_block:
        end = min(to_block, start + max_span - 1)
        params = {
            "address": address,
            "fromBlock": hex(start),
            "toBlock": hex(end),
        }
        if topics:
            params["topics"] = list(topics)
        try:
            entries = client.call("eth_getLogs", [params]).result or []
            collected.extend(_to_log(e) for e in entries if isinstance(e, dict))
        except (RpcError, RpcTransportError):
            pass
        start = end + 1

    collected.sort(key=lambda log: (log.block_number, log.log_index))
    return collected


def decode_log(spec: EventSpec, log: Log) -> DecodedLog:
    """Resolve one log against an event spec.

    Indexed parameters come from topics 1..n and unindexed from the data blob — mixing
    the two is the classic decoding bug and produces plausible-looking nonsense rather
    than an error.
    """
    args: Dict[str, Any] = {}

    for position, (param_type, name) in enumerate(spec.indexed, start=1):
        if position >= len(log.topics):
            continue
        raw = bytes.fromhex(log.topics[position][2:].rjust(64, "0"))
        args[name] = _decode_topic(param_type, raw)

    unindexed = spec.unindexed
    if unindexed:
        try:
            values = decode_args([t for t, _ in unindexed], log.data)
        except AbiError:
            values = []
        for (_, name), value in zip(unindexed, values):
            args[name] = value

    return DecodedLog(event=spec.name, args=args, log=log)


def _decode_topic(param_type: str, raw: bytes) -> Any:
    """Decode one 32-byte topic word.

    Dynamic types (``string``, ``bytes``) are *hashed* when indexed — the topic holds
    keccak of the value, not the value — so they are returned as the hash with no
    pretence of having recovered the original.
    """
    if param_type == "address":
        return "0x" + raw[-20:].hex()
    if param_type == "bool":
        return bool(to_uint(raw))
    if param_type.startswith("uint"):
        return to_uint(raw)
    if param_type.startswith("int"):
        return to_int(raw)
    if param_type in ("string", "bytes") or param_type.startswith("bytes"):
        return "0x" + raw.hex()
    return "0x" + raw.hex()


def fetch_events(address: str, spec: EventSpec, from_block: int = 0,
                 to_block: Optional[int] = None, network: str = DEFAULT_NETWORK,
                 client: Optional[RpcClient] = None,
                 rpc_url: Optional[str] = None) -> List[DecodedLog]:
    """Fetch and decode every occurrence of one event on one contract."""
    rpc = client or client_for(network=network, rpc_url=rpc_url)
    logs = get_logs(rpc, address, [spec.topic0], from_block, to_block)
    return [decode_log(spec, log) for log in logs]


def blocks_for_seconds(seconds: float, network: str = DEFAULT_NETWORK) -> int:
    """How many blocks roughly span ``seconds`` on ``network``.

    Used only to size a query window. Every timestamp reported by this module comes from
    the chain, never from this estimate.
    """
    return max(1, int(seconds / BLOCK_TIME_SECS.get(network.lower(), DEFAULT_BLOCK_TIME)))


@dataclass
class AnswerUpdate:
    """One publish, as the aggregator logged it."""

    round_id: int
    phase_id: int
    aggregator_round: int
    answer: int
    price: float
    updated_at: int
    block_number: int
    transaction_hash: str

    @property
    def age_secs(self) -> int:
        return max(0, int(time.time()) - self.updated_at)

    def as_dict(self) -> Dict[str, Any]:
        return {
            "round_id": self.round_id,
            "phase_id": self.phase_id,
            "aggregator_round": self.aggregator_round,
            "answer": self.answer,
            "price": self.price,
            "updated_at": self.updated_at,
            "block_number": self.block_number,
            "transaction_hash": self.transaction_hash,
        }


def answer_updates(address: str, hours: float = 6.0, network: str = DEFAULT_NETWORK,
                   client: Optional[RpcClient] = None, rpc_url: Optional[str] = None,
                   decimals: Optional[int] = None,
                   resolve_proxy: bool = True) -> List[AnswerUpdate]:
    """Every publish by a feed in the last ``hours``, oldest first.

    ``resolve_proxy`` is on by default and is the thing that makes this work at all: the
    address in the feed registry is a proxy that emits no events, so it is resolved to
    the implementation before filtering. Pass an implementation address directly and turn
    it off to save the round trip.

    The ``roundId`` in this event is the *aggregator's* round, not the proxy's packed
    form, so it is joined with the phase for consistency with
    :func:`alchem_link.aggregator.round_history`.
    """
    from .aggregator import describe_aggregator

    rpc = client or client_for(network=network, rpc_url=rpc_url)
    target = address
    phase = 0

    if resolve_proxy or decimals is None:
        info = describe_aggregator(address, network=network, client=rpc)
        if decimals is None:
            decimals = info.decimals or 8
        if resolve_proxy and info.implementation:
            target = info.implementation
        if info.phase_id is not None:
            phase = int(info.phase_id)

    latest = rpc.block_number()
    span = blocks_for_seconds(hours * 3600, network)
    logs = fetch_events(target, ANSWER_UPDATED, from_block=max(0, latest - span),
                        to_block=latest, network=network, client=rpc)

    updates: List[AnswerUpdate] = []
    for decoded in logs:
        answer = int(decoded.args.get("current", 0))
        aggregator_round = int(decoded.args.get("roundId", 0))
        updates.append(AnswerUpdate(
            round_id=(phase << 64) | aggregator_round if phase else aggregator_round,
            phase_id=phase,
            aggregator_round=aggregator_round,
            answer=answer,
            price=scale(answer, decimals or 8),
            updated_at=int(decoded.args.get("updatedAt", 0)),
            block_number=decoded.block_number,
            transaction_hash=decoded.log.transaction_hash,
        ))
    return updates


def transfers_to(address: str, token: str, hours: float = 24.0,
                 network: str = DEFAULT_NETWORK, client: Optional[RpcClient] = None,
                 rpc_url: Optional[str] = None) -> List[DecodedLog]:
    """ERC-20 transfers into ``address`` for one token, from logs alone.

    The keyless equivalent of Alchemy's ``alchemy_getAssetTransfers`` for a single known
    token. It cannot enumerate *which* tokens an address holds — that genuinely requires
    an indexer — but given the token it needs no key at all.
    """
    rpc = client or client_for(network=network, rpc_url=rpc_url)
    latest = rpc.block_number()
    span = blocks_for_seconds(hours * 3600, network)
    padded = "0x" + address.lower().replace("0x", "").rjust(64, "0")
    logs = get_logs(rpc, token, [TRANSFER.topic0, None, padded],
                    from_block=max(0, latest - span), to_block=latest)
    return [decode_log(TRANSFER, log) for log in logs]
