"""One exception hierarchy for the whole toolkit.

Before this module the package raised ``KeyError`` for an unknown network, ``ValueError``
for a malformed ABI type, and two unrelated ``RuntimeError`` subclasses for the two ways
an RPC call fails. That is workable at the command line, where everything funnels into
one handler, but it is hostile to anyone importing the library: there was no way to write
``except AlchemLinkError`` and mean it, and no way to distinguish "your input was wrong"
from "the chain was unreachable" without matching on message text.

Everything now descends from :class:`AlchemLinkError`. The classes that replaced a
builtin still inherit that builtin as well — :class:`UnknownNetwork` is a ``KeyError``,
:class:`EncodingError` is a ``ValueError`` — so existing ``except KeyError`` code keeps
working and nobody's script breaks on upgrade. That dual inheritance is deliberate and
should stay.

The distinction worth preserving as new errors are added is **retryable versus not**.
:attr:`AlchemLinkError.retryable` tells a caller whether a second attempt could plausibly
succeed. A timeout is retryable; a feed that is not in the registry never will be, and
retrying it just burns rate limit against an answer that cannot change.
"""
from __future__ import annotations

from typing import Any, Dict, Optional


class AlchemLinkError(Exception):
    """Base class for everything this package raises deliberately.

    ``context`` carries structured detail — the network, the pair, the address — so a
    caller can react to the specifics without parsing the message, and so
    :meth:`as_dict` can put a failure into the same JSON shape as a success.
    """

    #: Whether a second attempt could plausibly succeed.
    retryable = False

    def __init__(self, message: str, **context: Any) -> None:
        super().__init__(message)
        self.message = message
        self.context: Dict[str, Any] = {k: v for k, v in context.items() if v is not None}

    def as_dict(self) -> Dict[str, Any]:
        return {
            "error": self.__class__.__name__,
            "message": self.message,
            "retryable": self.retryable,
            **self.context,
        }

    @property
    def hint(self) -> str:
        """A next step for a human, when there is an obvious one. Empty otherwise."""
        return ""


# ── configuration and lookup ─────────────────────────────────────────────────


class ConfigurationError(AlchemLinkError):
    """Something about the request itself is wrong. Retrying will not help."""


class UnknownNetwork(ConfigurationError, KeyError):
    """A network key that is not in the registry.

    Also a ``KeyError`` because :func:`alchem_link.networks.get_network` raised one for
    several releases and callers catch it.
    """

    def __init__(self, network: str, known: Optional[list] = None) -> None:
        listing = ", ".join(sorted(known)) if known else ""
        super().__init__(
            f"unknown network '{network}'" + (f". Known networks: {listing}" if listing else ""),
            network=network,
        )
        self.known = list(known or [])

    def __str__(self) -> str:
        # KeyError's __str__ repr-quotes the argument, which turns a helpful sentence
        # into `"unknown network 'foo'. Known: ..."` with visible quotes around it.
        return self.message

    @property
    def hint(self) -> str:
        return "run `alchem-link networks` for the list"


class UnknownFeed(ConfigurationError, KeyError):
    """A pair that is not registered on the requested network."""

    def __init__(self, pair: str, network: str, known: Optional[list] = None) -> None:
        listing = ", ".join(sorted(known)) if known else "none"
        super().__init__(
            f"no feed '{pair}' on {network}. Known pairs: {listing}",
            pair=pair, network=network,
        )
        self.pair = pair
        self.network = network
        self.known = list(known or [])

    def __str__(self) -> str:
        return self.message

    @property
    def hint(self) -> str:
        return f"run `alchem-link feeds -n {self.context.get('network')}` to list them"


class MissingCredential(ConfigurationError):
    """A capability that needs an API key was used without one."""

    def __init__(self, message: str, env_var: str = "ALCHEMY_API_KEY") -> None:
        super().__init__(message, env_var=env_var)
        self.env_var = env_var

    @property
    def hint(self) -> str:
        return f"set {self.env_var} — everything else works without it"


# ── transport and protocol ───────────────────────────────────────────────────


class TransportError(AlchemLinkError):
    """The node could not be reached, or did not answer in time.

    ``retryable`` is False for failures that cannot improve on a second attempt — a 4xx
    other than 429, for instance. Retrying those just burns rate limit.
    """

    retryable = True

    def __init__(self, message: str, retryable: bool = True, **context: Any) -> None:
        super().__init__(message, **context)
        self.retryable = retryable


class ProtocolError(AlchemLinkError):
    """A JSON-RPC level error — the node answered, and the answer was an error."""


class EncodingError(AlchemLinkError, ValueError):
    """ABI encoding or decoding failed. Also a ``ValueError`` for compatibility."""


# ── feed semantics ───────────────────────────────────────────────────────────


class FeedError(AlchemLinkError):
    """The feed answered, but the answer cannot be consumed."""

    def __init__(self, message: str, pair: str = "", network: str = "", **context: Any) -> None:
        super().__init__(message, pair=pair or None, network=network or None, **context)
        self.pair = pair
        self.network = network


class StaleFeed(FeedError):
    """The last update is older than the heartbeat plus tolerance.

    Raised only by the strict-read paths — :meth:`alchem_link.client.AlchemLink.price`
    with ``strict=True`` and the simulation guards. The default read *returns* a stale
    reading with ``stale=True`` set, because a caller usually wants to see the number and
    the verdict rather than an exception in place of both.
    """

    def __init__(self, pair: str, network: str, age_secs: int, heartbeat_secs: int) -> None:
        super().__init__(
            f"{pair} on {network} last updated {age_secs}s ago, past its "
            f"{heartbeat_secs}s heartbeat",
            pair=pair, network=network, age_secs=age_secs, heartbeat_secs=heartbeat_secs,
        )

    @property
    def hint(self) -> str:
        return "do not trade on this value; check `alchem-link cadence` for the real interval"


class InvalidAnswer(FeedError):
    """A zero or negative answer, which is never a real quote."""


class UnreadableFeed(FeedError):
    """The aggregator could not be read at all."""

    retryable = True


class SimulationError(AlchemLinkError):
    """A scenario could not be built or replayed. See :mod:`alchem_link.simulate`."""


#: Legacy aliases. ``RpcError``/``RpcTransportError`` were the public names before the
#: hierarchy existed and are re-exported from :mod:`alchem_link.rpc`; the assignment
#: lives there rather than here so there is exactly one class object per concept.
__all__ = [
    "AlchemLinkError",
    "ConfigurationError",
    "UnknownNetwork",
    "UnknownFeed",
    "MissingCredential",
    "TransportError",
    "ProtocolError",
    "EncodingError",
    "FeedError",
    "StaleFeed",
    "InvalidAnswer",
    "UnreadableFeed",
    "SimulationError",
]
