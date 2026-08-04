"""Minimal ABI encode/decode for the calls this package makes.

Deliberately dependency-free. Reading a Chainlink aggregator needs four function
selectors and the ability to decode static words plus one dynamic string — a few dozen
lines, versus pulling in ``web3`` and its transitive tree for the same result.

Function selectors are the first four bytes of ``keccak256(signature)``. The stdlib
ships SHA3-256, which is *not* Keccak-256 (the padding differs), so rather than add a
hashing dependency the selectors are stored as the constants they are. Each one below
was verified against a live mainnet aggregator; see ``tests/test_abi.py`` for the
decoded fixtures that pin the behaviour.
"""
from __future__ import annotations

from typing import List

WORD = 32
_UINT256_CEILING = 1 << 256
_INT256_SIGN_BIT = 1 << 255

# keccak256(signature)[:4] — verified live against 0x5f4eC3Df...19 (ETH/USD, mainnet).
SELECTOR_LATEST_ROUND_DATA = "0xfeaf968c"  # latestRoundData()
SELECTOR_DECIMALS = "0x313ce567"           # decimals()
SELECTOR_DESCRIPTION = "0x7284e416"        # description()
SELECTOR_VERSION = "0x54fd4d50"            # version()


class AbiError(ValueError):
    """Raised when a response cannot be decoded as the expected ABI shape."""


def strip0x(value: str) -> str:
    return value[2:] if value.startswith(("0x", "0X")) else value


def to_bytes(hexstr: str) -> bytes:
    raw = strip0x(hexstr)
    if len(raw) % 2:
        raise AbiError(f"odd-length hex payload ({len(raw)} chars)")
    try:
        return bytes.fromhex(raw)
    except ValueError as exc:
        raise AbiError(f"payload is not valid hex: {exc}") from exc


def words(hexstr: str) -> List[bytes]:
    """Split an ABI payload into 32-byte words."""
    raw = to_bytes(hexstr)
    if len(raw) % WORD:
        raise AbiError(f"payload is not a whole number of 32-byte words ({len(raw)} bytes)")
    return [raw[i:i + WORD] for i in range(0, len(raw), WORD)]


def to_uint(word: bytes) -> int:
    return int.from_bytes(word, "big")


def to_int(word: bytes) -> int:
    """Decode a two's-complement int256. Chainlink answers are signed and can go negative."""
    value = int.from_bytes(word, "big")
    return value - _UINT256_CEILING if value >= _INT256_SIGN_BIT else value


def decode_string(hexstr: str) -> str:
    """Decode a single dynamic `string` return value (offset, length, utf-8 bytes)."""
    raw = to_bytes(hexstr)
    if len(raw) < WORD:
        raise AbiError("string payload too short to hold an offset")
    offset = int.from_bytes(raw[0:WORD], "big")
    if offset + WORD > len(raw):
        raise AbiError("string offset points past the end of the payload")
    length = int.from_bytes(raw[offset:offset + WORD], "big")
    start = offset + WORD
    if start + length > len(raw):
        raise AbiError("string length runs past the end of the payload")
    return raw[start:start + length].decode("utf-8", errors="replace")


def scale(answer: int, decimals: int) -> float:
    """Convert a raw feed answer into a human float."""
    if decimals < 0:
        raise AbiError(f"negative decimals: {decimals}")
    return answer / (10 ** decimals)
