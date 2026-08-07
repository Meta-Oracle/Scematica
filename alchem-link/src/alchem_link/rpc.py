"""A small, honest JSON-RPC client built on the standard library.

No ``requests``, no ``web3`` — one stdlib HTTP call plus JSON. Two details are not
obvious and both cost real debugging time if you meet them cold:

* **User-Agent.** Several public RPC providers reject Python's default
  ``Python-urllib/3.x`` with a 403, which reads as "the chain is down". This client
  always sends a real one.
* **Batching.** JSON-RPC 2.0 lets you POST an *array* of requests and get an array back,
  which turns twenty feed reads into one round trip. Providers may return the responses
  in any order, so results are re-sorted by request id rather than by position — a
  mistake that silently pairs each answer with the wrong question.
"""
from __future__ import annotations

import json
import time
import urllib.error
import urllib.request
from dataclasses import dataclass, field
from typing import Any, Dict, List, Optional, Sequence, Tuple

from .abi import (
    SELECTOR_DECIMALS,
    SELECTOR_DESCRIPTION,
    SELECTOR_LATEST_ROUND_DATA,
)
from .networks import DEFAULT_NETWORK, Endpoint, resolve_endpoint

DEFAULT_TIMEOUT = 15.0
DEFAULT_RETRIES = 2

#: Providers vary, but a few hundred sub-requests is where batch payloads start getting
#: rejected outright. Chunking below that keeps a large read from failing as one unit.
MAX_BATCH_SIZE = 100


class RpcError(RuntimeError):
    """A JSON-RPC level error — the node answered, and the answer was an error."""


class RpcTransportError(RuntimeError):
    """The node could not be reached, or did not answer in time.

    ``retryable`` is False for failures that cannot improve on a second attempt —
    a 4xx other than 429, for instance. Retrying those just burns rate limit.
    """

    def __init__(self, message: str, retryable: bool = True) -> None:
        super().__init__(message)
        self.retryable = retryable


@dataclass
class RpcCallResult:
    result: Any
    elapsed_ms: float


@dataclass
class BatchOutcome:
    """One entry of a batch response. Either ``result`` is set, or ``error`` is."""
    result: Any = None
    error: Optional[str] = None

    @property
    def ok(self) -> bool:
        return self.error is None


@dataclass
class RpcStats:
    """Cheap counters, so `doctor` and the TUI can show what a run actually cost."""
    requests: int = 0
    http_posts: int = 0
    retries: int = 0
    total_ms: float = 0.0
    #: Contract reads executed *inside* a Multicall3 aggregate. These never become
    #: JSON-RPC requests at all, so counting only `requests` understates the saving.
    batched_reads: int = 0

    @property
    def logical_reads(self) -> int:
        """What the same work would have cost as one request each."""
        return self.requests + self.batched_reads

    def as_dict(self) -> Dict[str, Any]:
        return {
            "requests": self.requests,
            "batched_reads": self.batched_reads,
            "logical_reads": self.logical_reads,
            "http_posts": self.http_posts,
            "retries": self.retries,
            "total_ms": round(self.total_ms, 1),
            "round_trips_saved": max(0, self.logical_reads - self.http_posts),
        }


class RpcClient:
    """Minimal JSON-RPC 2.0 client.

    Retries only transport failures. A JSON-RPC error is a real answer from the node —
    retrying a bad request just wastes the caller's rate limit.
    """

    def __init__(
        self,
        endpoint: Endpoint,
        timeout: float = DEFAULT_TIMEOUT,
        retries: int = DEFAULT_RETRIES,
        user_agent: str = "alchem-link",
    ) -> None:
        self.endpoint = endpoint
        self.timeout = timeout
        self.retries = max(0, retries)
        self.user_agent = user_agent
        self.stats = RpcStats()
        self._request_id = 0
        #: Cached Multicall3 probe. One round trip per client, not per batch.
        self.multicall3_supported: Optional[bool] = None

    # ── transport ────────────────────────────────────────────────────────────

    def _attempt(self, payload: bytes) -> Any:
        """Exactly one HTTP round trip. Retry policy lives in `_post`, not here."""
        request = urllib.request.Request(
            self.endpoint.url,
            data=payload,
            headers={
                "Content-Type": "application/json",
                # Required: some providers 403 the default urllib agent outright.
                "User-Agent": self.user_agent,
                "Accept": "application/json",
            },
        )
        try:
            with urllib.request.urlopen(request, timeout=self.timeout) as response:
                return json.loads(response.read().decode("utf-8"))
        except urllib.error.HTTPError as exc:
            detail = ""
            try:
                detail = exc.read().decode("utf-8", "replace")[:200]
            except Exception:  # pragma: no cover - diagnostic best effort
                pass
            # 4xx will not become 2xx on a retry; fail fast and say why. 429 is the
            # exception — backing off is exactly the right response there.
            fatal = 400 <= exc.code < 500 and exc.code != 429
            raise RpcTransportError(
                f"HTTP {exc.code} from {self.endpoint.redacted()}"
                f"{': ' + detail if detail else ''}",
                retryable=not fatal,
            ) from exc
        except json.JSONDecodeError as exc:
            raise RpcTransportError(
                f"{self.endpoint.redacted()} returned a non-JSON body", retryable=False
            ) from exc
        except (urllib.error.URLError, TimeoutError, OSError) as exc:
            raise RpcTransportError(f"{self.endpoint.redacted()}: {exc}") from exc

    def _post(self, payload: bytes) -> Any:
        last_error: Optional[Exception] = None
        started = time.perf_counter()
        for attempt in range(self.retries + 1):
            try:
                body = self._attempt(payload)
                self.stats.http_posts += 1
                self.stats.total_ms += (time.perf_counter() - started) * 1000
                return body
            except RpcTransportError as exc:
                if not exc.retryable:
                    self.stats.http_posts += 1
                    raise
                last_error = exc
                if attempt < self.retries:
                    self.stats.retries += 1
                    time.sleep(0.25 * (attempt + 1))
        self.stats.http_posts += 1
        raise RpcTransportError(
            f"could not reach {self.endpoint.redacted()} after "
            f"{self.retries + 1} attempt(s): {last_error}"
        )

    def _next_id(self) -> int:
        self._request_id += 1
        return self._request_id

    def call(self, method: str, params: Optional[List[Any]] = None) -> RpcCallResult:
        request_id = self._next_id()
        self.stats.requests += 1
        payload = json.dumps(
            {"jsonrpc": "2.0", "id": request_id, "method": method, "params": params or []}
        ).encode("utf-8")

        started = time.perf_counter()
        body = self._post(payload)
        elapsed_ms = (time.perf_counter() - started) * 1000

        if isinstance(body, dict) and body.get("error"):
            raise RpcError(f"{method} failed: {_error_text(body['error'])}")
        if not isinstance(body, dict) or "result" not in body:
            raise RpcError(f"{method} returned no result field")
        return RpcCallResult(result=body["result"], elapsed_ms=elapsed_ms)

    def batch(self, requests: Sequence[Tuple[str, List[Any]]]) -> List[BatchOutcome]:
        """Send many JSON-RPC requests in as few HTTP round trips as possible.

        Per-request failures come back as :class:`BatchOutcome` with ``error`` set rather
        than raising, because the whole point is that one bad call should not discard the
        other ninety-nine. Transport failures still raise — nothing came back at all.
        """
        if not requests:
            return []

        outcomes: List[BatchOutcome] = []
        for start in range(0, len(requests), MAX_BATCH_SIZE):
            chunk = list(requests[start:start + MAX_BATCH_SIZE])
            ids = [self._next_id() for _ in chunk]
            self.stats.requests += len(chunk)
            payload = json.dumps(
                [
                    {"jsonrpc": "2.0", "id": rid, "method": method, "params": params or []}
                    for rid, (method, params) in zip(ids, chunk)
                ]
            ).encode("utf-8")

            body = self._post(payload)

            # A provider that does not support batching may answer a single object, or
            # an error object, instead of an array. Fall back rather than crash.
            if isinstance(body, dict):
                detail = _error_text(body.get("error")) if body.get("error") else "not an array"
                outcomes.extend(self._sequential(chunk, reason=detail))
                continue
            if not isinstance(body, list):
                outcomes.extend(self._sequential(chunk, reason="unrecognised batch reply"))
                continue

            # Responses may arrive in any order; pair them by id, never by position.
            by_id = {entry.get("id"): entry for entry in body if isinstance(entry, dict)}
            for rid in ids:
                entry = by_id.get(rid)
                if entry is None:
                    outcomes.append(BatchOutcome(error="no response for this request id"))
                elif entry.get("error"):
                    outcomes.append(BatchOutcome(error=_error_text(entry["error"])))
                elif "result" not in entry:
                    outcomes.append(BatchOutcome(error="response had no result field"))
                else:
                    outcomes.append(BatchOutcome(result=entry["result"]))
        return outcomes

    def _sequential(
        self, chunk: Sequence[Tuple[str, List[Any]]], reason: str
    ) -> List[BatchOutcome]:
        """One-at-a-time fallback for endpoints that do not honour batch payloads."""
        out: List[BatchOutcome] = []
        for method, params in chunk:
            try:
                out.append(BatchOutcome(result=self.call(method, params).result))
            except RpcError as exc:
                out.append(BatchOutcome(error=str(exc)))
            except RpcTransportError as exc:
                out.append(BatchOutcome(error=f"{reason}; and then: {exc}"))
        return out

    # ── convenience wrappers ─────────────────────────────────────────────────

    def block_number(self) -> int:
        return int(self.call("eth_blockNumber").result, 16)

    def chain_id(self) -> int:
        return int(self.call("eth_chainId").result, 16)

    def gas_price_wei(self) -> int:
        return int(self.call("eth_gasPrice").result, 16)

    def eth_call(self, to: str, data: str, block: str = "latest") -> str:
        return self.call("eth_call", [{"to": to, "data": data}, block]).result

    def get_code(self, address: str, block: str = "latest") -> str:
        return self.call("eth_getCode", [address, block]).result

    def has_code(self, address: str) -> bool:
        """True when something is deployed at ``address``.

        An ``eth_call`` to an empty address returns ``0x`` rather than reverting, so a
        typo'd address looks exactly like a function that returned nothing.
        """
        try:
            return len(self.get_code(address)) > 2
        except (RpcError, RpcTransportError):
            return False

    def get_block(self, block: str = "latest", full: bool = False) -> Dict[str, Any]:
        return self.call("eth_getBlockByNumber", [block, full]).result

    def fee_history(
        self, blocks: int = 20, newest: str = "latest", percentiles: Optional[List[float]] = None
    ) -> Dict[str, Any]:
        return self.call(
            "eth_feeHistory", [hex(blocks), newest, percentiles or [10.0, 50.0, 90.0]]
        ).result

    def read_aggregator(self, address: str) -> Dict[str, str]:
        """Fetch the three aggregator fields a price read needs, as raw hex.

        Batched into one HTTP round trip. Kept as a method so callers that hold a client
        do not need to know about :mod:`alchem_link.multicall`.
        """
        selectors = [
            ("latest_round_data", SELECTOR_LATEST_ROUND_DATA),
            ("decimals", SELECTOR_DECIMALS),
            ("description", SELECTOR_DESCRIPTION),
        ]
        outcomes = self.batch(
            [("eth_call", [{"to": address, "data": data}, "latest"]) for _, data in selectors]
        )
        out: Dict[str, str] = {}
        for (name, _), outcome in zip(selectors, outcomes):
            if not outcome.ok:
                raise RpcError(f"{name}() on {address}: {outcome.error}")
            out[name] = outcome.result
        return out


def _error_text(err: Any) -> str:
    if isinstance(err, dict):
        code = err.get("code")
        message = err.get("message", "unknown error")
        return f"{message}{f' [{code}]' if code is not None else ''}"
    return str(err)


def client_for(
    network: str = DEFAULT_NETWORK,
    rpc_url: Optional[str] = None,
    timeout: float = DEFAULT_TIMEOUT,
    user_agent: str = "alchem-link",
) -> RpcClient:
    """Build a client for a network, resolving the endpoint from args and environment."""
    return RpcClient(
        resolve_endpoint(network=network, rpc_url=rpc_url),
        timeout=timeout,
        user_agent=user_agent,
    )


def gwei(wei: int) -> float:
    return wei / 1_000_000_000
