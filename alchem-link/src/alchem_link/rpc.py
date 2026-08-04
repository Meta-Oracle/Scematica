"""A small, honest JSON-RPC client built on the standard library.

No ``requests``, no ``web3`` — one stdlib HTTP call plus JSON. The only non-obvious
detail is the User-Agent: several public RPC providers reject Python's default
``Python-urllib/3.x`` with a 403, which reads as "the chain is down" if you have not
seen it before. Sending a real UA fixes it, so this client always sets one.
"""
from __future__ import annotations

import json
import time
import urllib.error
import urllib.request
from dataclasses import dataclass
from typing import Any, Dict, List, Optional

from .abi import SELECTOR_DECIMALS, SELECTOR_DESCRIPTION, SELECTOR_LATEST_ROUND_DATA
from .networks import DEFAULT_NETWORK, Endpoint, resolve_endpoint

DEFAULT_TIMEOUT = 15.0
DEFAULT_RETRIES = 2


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
        self._request_id = 0

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
        for attempt in range(self.retries + 1):
            try:
                return self._attempt(payload)
            except RpcTransportError as exc:
                if not exc.retryable:
                    raise
                last_error = exc
                if attempt < self.retries:
                    time.sleep(0.25 * (attempt + 1))
        raise RpcTransportError(
            f"could not reach {self.endpoint.redacted()} after "
            f"{self.retries + 1} attempt(s): {last_error}"
        )

    def call(self, method: str, params: Optional[List[Any]] = None) -> RpcCallResult:
        self._request_id += 1
        payload = json.dumps(
            {"jsonrpc": "2.0", "id": self._request_id, "method": method, "params": params or []}
        ).encode("utf-8")

        started = time.perf_counter()
        body = self._post(payload)
        elapsed_ms = (time.perf_counter() - started) * 1000

        if isinstance(body, dict) and body.get("error"):
            err = body["error"]
            message = err.get("message", "unknown error") if isinstance(err, dict) else str(err)
            code = err.get("code") if isinstance(err, dict) else None
            raise RpcError(f"{method} failed{f' [{code}]' if code is not None else ''}: {message}")
        if not isinstance(body, dict) or "result" not in body:
            raise RpcError(f"{method} returned no result field")
        return RpcCallResult(result=body["result"], elapsed_ms=elapsed_ms)

    # ── convenience wrappers ─────────────────────────────────────────────────

    def block_number(self) -> int:
        return int(self.call("eth_blockNumber").result, 16)

    def chain_id(self) -> int:
        return int(self.call("eth_chainId").result, 16)

    def gas_price_wei(self) -> int:
        return int(self.call("eth_gasPrice").result, 16)

    def eth_call(self, to: str, data: str) -> str:
        return self.call("eth_call", [{"to": to, "data": data}, "latest"]).result

    def read_aggregator(self, address: str) -> Dict[str, str]:
        """Fetch the three aggregator fields a price read needs, as raw hex."""
        return {
            "latest_round_data": self.eth_call(address, SELECTOR_LATEST_ROUND_DATA),
            "decimals": self.eth_call(address, SELECTOR_DECIMALS),
            "description": self.eth_call(address, SELECTOR_DESCRIPTION),
        }


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
