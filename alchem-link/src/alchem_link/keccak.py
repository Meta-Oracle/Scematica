"""Keccak-256, in pure Python, because the standard library does not ship it.

``hashlib.sha3_256`` is *not* Keccak-256. The permutation is identical; the padding is
not. NIST changed the domain-separation byte from ``0x01`` to ``0x06`` between the
Keccak submission and the final SHA-3 standard, and Ethereum froze on the original.
Feed the same bytes to both and you get two unrelated digests — which is why so much
Ethereum tooling reaches for a native extension for one hash function.

This module is that hash function, in about a hundred lines of integer arithmetic with
no dependencies. It exists so the rest of the package can *compute* function selectors
instead of storing them as constants somebody has to trust:

    >>> selector("latestRoundData()")
    '0xfeaf968c'

That matters beyond tidiness. With a working keccak the package can encode a call to
any contract function from its signature, which is the difference between a reader that
knows four hardcoded selectors and one that can talk to an arbitrary aggregator.

Correctness is pinned two ways in ``tests/test_keccak.py``: the standard empty-input
vector, and the four selectors this package previously shipped as hand-verified
constants. Those constants were confirmed against live mainnet contracts, so they are a
genuine end-to-end check on the permutation and the padding, not a restatement of it.
"""
from __future__ import annotations

from typing import List

_MASK64 = (1 << 64) - 1

#: Rate in bytes for Keccak-256: (1600 - 2*256) / 8.
_RATE_BYTES = 136

#: Ethereum/original-Keccak domain byte. SHA-3 uses 0x06 here; that single byte is the
#: entire difference between this function and ``hashlib.sha3_256``.
_DOMAIN = 0x01

_ROUND_CONSTANTS = (
    0x0000000000000001, 0x0000000000008082, 0x800000000000808A, 0x8000000080008000,
    0x000000000000808B, 0x0000000080000001, 0x8000000080008081, 0x8000000000008009,
    0x000000000000008A, 0x0000000000000088, 0x0000000080008009, 0x000000008000000A,
    0x000000008000808B, 0x800000000000008B, 0x8000000000008089, 0x8000000000008003,
    0x8000000000008002, 0x8000000000000080, 0x000000000000800A, 0x800000008000000A,
    0x8000000080008081, 0x8000000000008080, 0x0000000080000001, 0x8000000080008008,
)

#: Rho rotation offsets, indexed [x][y].
_ROTATIONS = (
    (0, 36, 3, 41, 18),
    (1, 44, 10, 45, 2),
    (62, 6, 43, 15, 61),
    (28, 55, 25, 21, 56),
    (27, 20, 39, 8, 14),
)


def _rotl64(value: int, shift: int) -> int:
    if shift == 0:
        return value
    return ((value << shift) | (value >> (64 - shift))) & _MASK64


def _keccak_f1600(state: List[int]) -> None:
    """The permutation, in place. ``state`` is 25 lanes indexed ``x + 5*y``."""
    for round_constant in _ROUND_CONSTANTS:
        # θ — diffuse each column's parity into the two neighbouring columns.
        parity = [
            state[x] ^ state[x + 5] ^ state[x + 10] ^ state[x + 15] ^ state[x + 20]
            for x in range(5)
        ]
        delta = [parity[(x - 1) % 5] ^ _rotl64(parity[(x + 1) % 5], 1) for x in range(5)]
        for x in range(5):
            for y in range(5):
                state[x + 5 * y] ^= delta[x]

        # ρ and π — rotate every lane, then transpose the lanes into new positions.
        scratch = [0] * 25
        for x in range(5):
            for y in range(5):
                scratch[y + 5 * ((2 * x + 3 * y) % 5)] = _rotl64(
                    state[x + 5 * y], _ROTATIONS[x][y]
                )

        # χ — the only non-linear step.
        for x in range(5):
            for y in range(5):
                state[x + 5 * y] = scratch[x + 5 * y] ^ (
                    (scratch[(x + 1) % 5 + 5 * y] ^ _MASK64) & scratch[(x + 2) % 5 + 5 * y]
                )

        # ι — break the round symmetry.
        state[0] ^= round_constant


def keccak256(data: bytes) -> bytes:
    """Keccak-256 digest of ``data`` — the hash Ethereum means by ``keccak256``."""
    state = [0] * 25

    # Multi-rate padding: domain byte, zeroes, high bit of the final rate byte. When the
    # message leaves exactly one byte of room those two bits land in the same byte.
    padded = bytearray(data)
    padded.append(_DOMAIN)
    while len(padded) % _RATE_BYTES != 0:
        padded.append(0x00)
    padded[-1] |= 0x80

    for offset in range(0, len(padded), _RATE_BYTES):
        block = padded[offset:offset + _RATE_BYTES]
        for lane in range(_RATE_BYTES // 8):
            state[lane] ^= int.from_bytes(block[lane * 8:lane * 8 + 8], "little")
        _keccak_f1600(state)

    # Squeeze: 32 bytes fit inside the rate, so one pass is enough — no second permutation.
    out = bytearray()
    for lane in range(4):
        out += state[lane].to_bytes(8, "little")
    return bytes(out)


def keccak256_hex(data: bytes) -> str:
    return "0x" + keccak256(data).hex()


def selector(signature: str) -> str:
    """First four bytes of ``keccak256(signature)`` — a function selector.

    The signature is the canonical form: name, then argument types with no spaces and
    no parameter names, e.g. ``"getRoundData(uint80)"``.
    """
    return "0x" + keccak256(signature.encode("ascii")).hex()[:8]


def event_topic(signature: str) -> str:
    """Full 32-byte ``keccak256(signature)`` — topic0 for a log filter."""
    return keccak256_hex(signature.encode("ascii"))


def to_checksum_address(address: str) -> str:
    """EIP-55 checksum casing for a hex address.

    Mixed case is a typo detector: the case of each hex letter encodes a bit of the
    address's own hash, so a single wrong character fails the check. Needs keccak, which
    is why it lives here rather than in ``abi``.
    """
    raw = address.lower().removeprefix("0x")
    if len(raw) != 40:
        raise ValueError(f"an address is 20 bytes / 40 hex chars, got {len(raw)}")
    try:
        int(raw, 16)
    except ValueError as exc:
        raise ValueError(f"address is not hex: {address}") from exc

    digest = keccak256(raw.encode("ascii")).hex()
    return "0x" + "".join(
        char.upper() if char in "abcdef" and int(digest[i], 16) >= 8 else char
        for i, char in enumerate(raw)
    )


def is_checksum_address(address: str) -> bool:
    """True when ``address`` is all-lower, all-upper, or correctly EIP-55 cased."""
    raw = address.removeprefix("0x")
    if len(raw) != 40:
        return False
    if raw == raw.lower() or raw == raw.upper():
        return True  # unchecksummed, but not *wrong*
    try:
        return to_checksum_address(address) == "0x" + raw
    except ValueError:
        return False
