"""What is actually behind a feed address, and what it has been doing.

The address you consume is almost never the contract that holds the price. It is an
``EACAggregatorProxy`` that forwards to whichever aggregator is current, and the
interesting properties — the answer bounds, the contract type, the round history — live
one hop down. This module makes that hop, in two batched round trips, and exposes three
things a price read alone will never tell you:

* **What it is.** ``typeAndVersion()`` distinguishes an OCR2 aggregator from the older
  FluxAggregator, which matters because they have different failure modes.
* **What it can say.** ``minAnswer``/``maxAnswer`` are circuit breakers, and a feed
  physically cannot report a price outside them. When the real price leaves that range,
  the feed keeps returning the bound — reporting a number that is wrong, fresh, and
  perfectly well-formed. See :mod:`alchem_link.safety`.
* **What it has been doing.** Round IDs are sequential within a phase, so the history
  walks backwards from the latest round; :mod:`alchem_link.cadence` turns that into a
  measured publish interval rather than a declared one.

Round IDs on a proxy are not counters. They pack a 16-bit phase into the high bits:
``roundId = (phaseId << 64) | aggregatorRoundId``. Printing one raw gives the
twenty-digit number people mistake for a bug — 129127208515966893596 is phase 7, round
32284.
"""
from __future__ import annotations

import time
from dataclasses import dataclass, field
from typing import Any, Dict, List, Optional

from .abi import scale
from .multicall import Call, batch_call
from .rpc import RpcClient, client_for
from .networks import DEFAULT_NETWORK

#: Round ids pack the phase into the bits above this width.
PHASE_SHIFT = 64
_ROUND_MASK = (1 << PHASE_SHIFT) - 1

_ROUND_RETURNS = ["uint80", "int256", "uint256", "uint256", "uint80"]


def split_round_id(round_id: int) -> tuple[int, int]:
    """Split a proxy round id into ``(phase_id, aggregator_round)``."""
    return round_id >> PHASE_SHIFT, round_id & _ROUND_MASK


def join_round_id(phase_id: int, aggregator_round: int) -> int:
    return (phase_id << PHASE_SHIFT) | aggregator_round


@dataclass
class Round:
    """One published round."""
    round_id: int
    phase_id: int
    aggregator_round: int
    answer: int
    price: float
    started_at: int
    updated_at: int
    answered_in_round: int

    @property
    def carried_over(self) -> bool:
        """True when the answer was carried from an earlier round.

        ``answeredInRound < roundId`` means this round did not produce a fresh answer.
        Chainlink's own consumer guidance says to reject it; almost nobody checks.
        """
        return self.answered_in_round < self.round_id

    def as_dict(self) -> Dict[str, Any]:
        return {
            "round_id": self.round_id,
            "phase_id": self.phase_id,
            "aggregator_round": self.aggregator_round,
            "answer": self.answer,
            "price": self.price,
            "started_at": self.started_at,
            "updated_at": self.updated_at,
            "answered_in_round": self.answered_in_round,
            "carried_over": self.carried_over,
        }


@dataclass
class AggregatorInfo:
    """Everything one batched introspection pass can learn about a feed address."""
    address: str
    network: str
    description: str = ""
    decimals: int = 0
    version: Optional[int] = None
    phase_id: Optional[int] = None
    implementation: Optional[str] = None
    type_and_version: str = ""
    owner: Optional[str] = None
    min_answer: Optional[int] = None
    max_answer: Optional[int] = None
    latest: Optional[Round] = None
    notes: List[str] = field(default_factory=list)

    @property
    def is_proxy(self) -> bool:
        return self.implementation is not None

    @property
    def min_price(self) -> Optional[float]:
        return None if self.min_answer is None else scale(self.min_answer, self.decimals)

    @property
    def max_price(self) -> Optional[float]:
        return None if self.max_answer is None else scale(self.max_answer, self.decimals)

    @property
    def floor_headroom(self) -> Optional[float]:
        """How many times the price would have to *fall* to hit ``minAnswer``.

        This is the number the LUNA/Venus incident turned into money: the feed's floor
        was close enough to the market price that a crash reached it, after which the
        oracle reported the floor as though it were the price.
        """
        if self.latest is None or self.min_answer is None or self.min_answer <= 0:
            return None
        if self.latest.answer <= 0:
            return None
        return self.latest.answer / self.min_answer

    @property
    def ceiling_headroom(self) -> Optional[float]:
        """How many times the price would have to *rise* to hit ``maxAnswer``."""
        if self.latest is None or self.max_answer is None or self.latest.answer <= 0:
            return None
        return self.max_answer / self.latest.answer

    @property
    def bounds_are_binding(self) -> bool:
        """True when the circuit breakers sit close enough to matter.

        A 100x move in either direction is the threshold. Modern OCR2 deployments set
        the bounds to the extremes of ``int192`` — headroom in the trillions — which is
        the same as having no circuit breaker at all. Older aggregators sometimes carry
        genuinely tight bounds, and those are the ones worth knowing about.
        """
        floor, ceiling = self.floor_headroom, self.ceiling_headroom
        return (floor is not None and floor < 100) or (ceiling is not None and ceiling < 100)

    def as_dict(self) -> Dict[str, Any]:
        return {
            "address": self.address,
            "network": self.network,
            "description": self.description,
            "decimals": self.decimals,
            "version": self.version,
            "phase_id": self.phase_id,
            "implementation": self.implementation,
            "type_and_version": self.type_and_version,
            "owner": self.owner,
            "is_proxy": self.is_proxy,
            "min_answer": self.min_answer,
            "max_answer": self.max_answer,
            "min_price": self.min_price,
            "max_price": self.max_price,
            "floor_headroom": self.floor_headroom,
            "ceiling_headroom": self.ceiling_headroom,
            "bounds_are_binding": self.bounds_are_binding,
            "latest": self.latest.as_dict() if self.latest else None,
            "notes": self.notes,
        }


def _to_round(values: List[Any], decimals: int) -> Round:
    round_id, answer, started_at, updated_at, answered_in = values
    phase, agg_round = split_round_id(round_id)
    return Round(
        round_id=round_id,
        phase_id=phase,
        aggregator_round=agg_round,
        answer=answer,
        price=scale(answer, decimals),
        started_at=started_at,
        updated_at=updated_at,
        answered_in_round=answered_in,
    )


def describe_aggregator(
    address: str,
    network: str = DEFAULT_NETWORK,
    client: Optional[RpcClient] = None,
    rpc_url: Optional[str] = None,
) -> AggregatorInfo:
    """Introspect a feed address and whatever it proxies to.

    Two batched round trips: the proxy's own surface, then the implementation's. Every
    call tolerates failure, because these interfaces are optional — a bare aggregator
    has no ``aggregator()``, and only some deployments expose ``minAnswer``.
    """
    rpc = client or client_for(network=network, rpc_url=rpc_url)
    info = AggregatorInfo(address=address, network=network.lower())

    if not rpc.has_code(address):
        info.notes.append("no contract deployed at this address")
        return info

    surface = batch_call(rpc, [
        Call(address, "description()", (), ["string"], "description"),
        Call(address, "decimals()", (), ["uint8"], "decimals"),
        Call(address, "version()", (), ["uint256"], "version"),
        Call(address, "latestRoundData()", (), _ROUND_RETURNS, "latest"),
        Call(address, "aggregator()", (), ["address"], "aggregator"),
        Call(address, "phaseId()", (), ["uint16"], "phaseId"),
        Call(address, "owner()", (), ["address"], "owner"),
    ])

    def value(label: str, default: Any = None) -> Any:
        result = surface.by_label(label)
        return result.one(default) if result else default

    info.description = str(value("description", "") or "")
    info.decimals = int(value("decimals", 0) or 0)
    info.version = value("version")
    info.phase_id = value("phaseId")
    info.owner = value("owner")

    latest = surface.by_label("latest")
    if latest and latest.success:
        info.latest = _to_round(latest.values, info.decimals)
    else:
        info.notes.append(
            f"latestRoundData() failed: {latest.error if latest else 'no result'} — "
            "is this an AggregatorV3 contract?"
        )

    implementation = value("aggregator")
    if implementation and int(implementation, 16) != 0:
        info.implementation = implementation
    else:
        info.notes.append("no aggregator() — this address looks like a bare aggregator, not a proxy")

    # The bounds and the contract type live on the implementation. Reading them from the
    # proxy returns nothing, which is why a naive check reports "unbounded" for
    # everything.
    target = info.implementation or address
    deep = batch_call(rpc, [
        Call(target, "minAnswer()", (), ["int192"], "minAnswer"),
        Call(target, "maxAnswer()", (), ["int192"], "maxAnswer"),
        Call(target, "typeAndVersion()", (), ["string"], "typeAndVersion"),
    ])

    def deep_value(label: str, default: Any = None) -> Any:
        result = deep.by_label(label)
        return result.one(default) if result else default

    info.min_answer = deep_value("minAnswer")
    info.max_answer = deep_value("maxAnswer")
    info.type_and_version = str(deep_value("typeAndVersion", "") or "")

    if info.min_answer is None and info.max_answer is None:
        info.notes.append(
            "no minAnswer/maxAnswer exposed — bounds cannot be checked from chain state"
        )
    return info


def round_history(
    address: str,
    count: int = 24,
    network: str = DEFAULT_NETWORK,
    client: Optional[RpcClient] = None,
    rpc_url: Optional[str] = None,
    decimals: Optional[int] = None,
    latest: Optional[Round] = None,
) -> List[Round]:
    """Walk backwards from the latest round, newest first.

    Round ids decrement within a phase, so this is a straight countdown on the low 64
    bits. Walking off the start of a phase reverts rather than returning zeroes, and
    those rounds are simply omitted — the history is short, not wrong.
    """
    rpc = client or client_for(network=network, rpc_url=rpc_url)

    if latest is None or decimals is None:
        head = batch_call(rpc, [
            Call(address, "latestRoundData()", (), _ROUND_RETURNS, "latest"),
            Call(address, "decimals()", (), ["uint8"], "decimals"),
        ])
        head_round = head.by_label("latest")
        head_decimals = head.by_label("decimals")
        if not head_round or not head_round.success:
            return []
        decimals = int(head_decimals.one(8) if head_decimals else 8)
        latest = _to_round(head_round.values, decimals)

    phase, newest = split_round_id(latest.round_id)
    # Never walk below round 1 of the phase; round 0 does not exist.
    wanted = [
        join_round_id(phase, newest - offset)
        for offset in range(1, count)
        if newest - offset >= 1
    ]
    if not wanted:
        return [latest]

    report = batch_call(rpc, [
        Call(address, "getRoundData(uint80)", (rid,), _ROUND_RETURNS, str(rid))
        for rid in wanted
    ])

    history = [latest]
    for result in report.results:
        # A pruned or pre-phase round reverts; skip it rather than inventing a gap.
        if result.success and result.values and result.values[3] > 0:
            history.append(_to_round(result.values, decimals))
    return history
