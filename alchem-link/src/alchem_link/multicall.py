"""Batched contract reads, three tiers deep.

Reading six feeds means eighteen ``eth_call``s. Done naively that is eighteen HTTP round
trips, and on a public endpoint at ~400 ms each it is the difference between a dashboard
that feels live and one that does not. There are two independent ways to collapse them,
and this module uses whichever is available:

1. **Multicall3** — one ``eth_call`` to ``0xcA11bde05977b3631167028862bE2a173976CA11``
   that executes every sub-call inside a single EVM invocation. Deployed at that same
   address on essentially every EVM chain. Best case: eighteen reads, one round trip,
   and every result comes from *the same block*.
2. **JSON-RPC batching** — an array of requests in one POST. Works everywhere Multicall3
   is not deployed, still one round trip, but the sub-calls are independent so they can
   straddle a block boundary.
3. **Sequential** — one request at a time. Always works.

The atomicity in (1) is not a footnote. Comparing two feeds read one round trip apart is
comparing two different moments, and for divergence analysis that noise is the signal
you are trying to measure. :func:`batch_call` reports which tier it used so callers can
say whether a comparison was block-atomic.
"""
from __future__ import annotations

from dataclasses import dataclass, field
from typing import Any, Dict, List, Optional, Sequence

from .abi import AbiError, decode_args, decode_revert, encode_call, to_bytes
from .rpc import RpcClient, RpcError, RpcTransportError

#: Multicall3, deployed via a deterministic deployer so the address is identical on
#: essentially every EVM chain. Presence is still probed rather than assumed.
MULTICALL3_ADDRESS = "0xcA11bde05977b3631167028862bE2a173976CA11"

_AGGREGATE3 = "aggregate3((address,bool,bytes)[])"
_AGGREGATE3_RETURN = ["(bool,bytes)[]"]


@dataclass
class Call:
    """One contract read: where to send it, what to send, and how to read it back."""
    target: str
    signature: str
    args: tuple = ()
    returns: Sequence[str] = ()
    #: Free-form tag so callers can match results to intent without counting positions.
    label: str = ""
    #: When False a revert fails the whole batch. Default True: one dead feed should not
    #: blank the board.
    allow_failure: bool = True

    @property
    def data(self) -> str:
        return encode_call(self.signature, *self.args)


@dataclass
class CallResult:
    """The outcome of one :class:`Call`."""
    call: Call
    success: bool
    raw: str = "0x"
    error: str = ""
    values: List[Any] = field(default_factory=list)

    @property
    def label(self) -> str:
        return self.call.label or self.call.signature

    def one(self, default: Any = None) -> Any:
        """The single return value, or ``default`` when the call failed."""
        return self.values[0] if self.success and self.values else default

    def as_dict(self) -> Dict[str, Any]:
        return {
            "label": self.label,
            "target": self.call.target,
            "signature": self.call.signature,
            "success": self.success,
            "values": [v.hex() if isinstance(v, bytes) else v for v in self.values],
            "error": self.error,
        }


@dataclass
class BatchReport:
    """What :func:`batch_call` did, so callers can qualify their conclusions."""
    results: List[CallResult]
    #: ``multicall3``, ``json-rpc-batch`` or ``sequential``.
    tier: str
    http_round_trips: int

    @property
    def block_atomic(self) -> bool:
        """True when every read came from one block — required for a fair comparison."""
        return self.tier == "multicall3"

    @property
    def ok(self) -> List[CallResult]:
        return [r for r in self.results if r.success]

    def by_label(self, label: str) -> Optional[CallResult]:
        for result in self.results:
            if result.label == label:
                return result
        return None


def _decode_into(call: Call, raw: str) -> CallResult:
    """Turn a returned payload into a :class:`CallResult`, never raising."""
    if raw in ("0x", "", None):
        # An eth_call to an address with no code returns empty rather than reverting,
        # so this is where a typo'd or not-yet-deployed target surfaces.
        return CallResult(call=call, success=False, raw="0x", error="empty return (no code at target?)")
    try:
        values = list(decode_args(call.returns, raw)) if call.returns else []
    except AbiError as exc:
        return CallResult(call=call, success=False, raw=raw, error=f"decode failed: {exc}")
    return CallResult(call=call, success=True, raw=raw, values=values)


def supports_multicall3(client: RpcClient) -> bool:
    """Probe for Multicall3 rather than assuming the canonical address is populated.

    Cached on the client: the answer cannot change under a running process, and paying a
    round trip to re-ask before every batch would eat most of what batching buys.
    """
    if client.multicall3_supported is None:
        client.multicall3_supported = client.has_code(MULTICALL3_ADDRESS)
    return client.multicall3_supported


def multicall3(client: RpcClient, calls: Sequence[Call], block: str = "latest") -> List[CallResult]:
    """Execute every call in one EVM invocation. Raises if Multicall3 itself fails."""
    if not calls:
        return []
    payload = encode_call(
        _AGGREGATE3,
        [(call.target, call.allow_failure, to_bytes(call.data)) for call in calls],
    )
    raw = client.eth_call(MULTICALL3_ADDRESS, payload, block)
    # The sub-calls never become JSON-RPC requests, so record them explicitly or the
    # stats will report this as a single cheap read.
    client.stats.batched_reads += len(calls)
    decoded = decode_args(_AGGREGATE3_RETURN, raw)[0]
    if len(decoded) != len(calls):
        raise RpcError(f"aggregate3 returned {len(decoded)} results for {len(calls)} calls")

    out: List[CallResult] = []
    for call, (success, return_data) in zip(calls, decoded):
        if not success:
            out.append(
                CallResult(
                    call=call,
                    success=False,
                    error=decode_revert(return_data),
                )
            )
            continue
        out.append(_decode_into(call, "0x" + bytes(return_data).hex()))
    return out


def batch_call(
    client: RpcClient,
    calls: Sequence[Call],
    block: str = "latest",
    prefer_multicall: bool = True,
) -> BatchReport:
    """Read many contracts in as few round trips as the endpoint allows.

    Tries Multicall3, falls back to JSON-RPC batching, and reports which one it got. A
    Multicall3 failure is not fatal — some chains have it at a different address, and a
    few RPC providers cap ``eth_call`` return size below what a large aggregate3 needs.
    """
    if not calls:
        return BatchReport(results=[], tier="sequential", http_round_trips=0)

    before = client.stats.http_posts

    if prefer_multicall:
        try:
            if supports_multicall3(client):
                results = multicall3(client, calls, block)
                return BatchReport(
                    results=results,
                    tier="multicall3",
                    http_round_trips=client.stats.http_posts - before,
                )
        except (RpcError, RpcTransportError, AbiError):
            # Fall through to JSON-RPC batching. Worth no more than a tier downgrade:
            # the caller still gets its data, just without block atomicity.
            pass

    outcomes = client.batch(
        [("eth_call", [{"to": c.target, "data": c.data}, block]) for c in calls]
    )
    results: List[CallResult] = []
    for call, outcome in zip(calls, outcomes):
        if not outcome.ok:
            results.append(CallResult(call=call, success=False, error=outcome.error or "failed"))
        else:
            results.append(_decode_into(call, outcome.result))

    round_trips = client.stats.http_posts - before
    return BatchReport(
        results=results,
        tier="json-rpc-batch" if round_trips < len(calls) else "sequential",
        http_round_trips=round_trips,
    )
