"""ABI encoding and decoding, dependency-free.

This used to be four hardcoded function selectors and a decoder for five static words
plus one string — enough to read a Chainlink aggregator and nothing else. With
:mod:`alchem_link.keccak` supplying a real ``keccak256``, selectors are now *computed*
from signatures, so the package can call any function on any contract:

    >>> encode_call("getRoundData(uint80)", 18446744073709551617)[:10]
    '0x9a6fc8f5'

The codec covers what an EVM read path actually needs: ``address``, ``bool``,
``uint<N>``, ``int<N>``, ``bytes<N>``, ``bytes``, ``string``, dynamic arrays of any of
those, and tuples. Tuple arrays matter specifically — ``Multicall3.aggregate3`` takes
``(address,bool,bytes)[]``, and batching every feed read into one round trip depends on
encoding it correctly.

Fixed-size arrays (``uint256[3]``) are not supported. Nothing in this package's read
paths uses one, and the honest failure is a clear exception rather than a silent
mis-encode.
"""
from __future__ import annotations

from dataclasses import dataclass
from typing import Any, Iterable, List, Sequence, Tuple, Union

from .keccak import event_topic, keccak256, selector, to_checksum_address

WORD = 32
_UINT256_CEILING = 1 << 256
_INT256_SIGN_BIT = 1 << 255

# Retained as named constants because they read better at the call sites than
# `selector("latestRoundData()")` does, and because the values are pinned by tests that
# would catch a keccak regression. They are computed, not copied.
SELECTOR_LATEST_ROUND_DATA = selector("latestRoundData()")  # 0xfeaf968c
SELECTOR_DECIMALS = selector("decimals()")                  # 0x313ce567
SELECTOR_DESCRIPTION = selector("description()")            # 0x7284e416
SELECTOR_VERSION = selector("version()")                    # 0x54fd4d50


class AbiError(ValueError):
    """Raised when a payload cannot be encoded or decoded as the expected ABI shape."""


# ── hex plumbing ─────────────────────────────────────────────────────────────────


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
    """Decode a single dynamic ``string`` return value (offset, length, utf-8 bytes)."""
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


# ── type grammar ─────────────────────────────────────────────────────────────────


@dataclass(frozen=True)
class AbiType:
    """A parsed ABI type.

    ``kind`` is one of ``uint``, ``int``, ``address``, ``bool``, ``fixed_bytes``,
    ``bytes``, ``string``, ``array`` or ``tuple``.
    """
    kind: str
    bits: int = 0
    size: int = 0
    item: "AbiType | None" = None
    components: Tuple["AbiType", ...] = ()
    name: str = ""

    @property
    def dynamic(self) -> bool:
        if self.kind in ("bytes", "string", "array"):
            return True
        if self.kind == "tuple":
            return any(c.dynamic for c in self.components)
        return False


def _split_top_level(body: str) -> List[str]:
    """Split ``a,(b,c),d`` on commas that are not inside parentheses."""
    parts: List[str] = []
    depth = 0
    current = ""
    for char in body:
        if char == "," and depth == 0:
            parts.append(current)
            current = ""
            continue
        if char == "(":
            depth += 1
        elif char == ")":
            depth -= 1
            if depth < 0:
                raise AbiError(f"unbalanced parentheses in '{body}'")
        current += char
    if depth:
        raise AbiError(f"unbalanced parentheses in '{body}'")
    if current or parts:
        parts.append(current)
    return [p.strip() for p in parts if p.strip()]


def parse_type(spec: str) -> AbiType:
    """Parse an ABI type string into an :class:`AbiType`."""
    text = spec.strip()
    if not text:
        raise AbiError("empty type")

    if text.endswith("[]"):
        return AbiType(kind="array", item=parse_type(text[:-2]))
    if text.endswith("]"):
        raise AbiError(f"fixed-size arrays are not supported: '{spec}'")

    if text.startswith("("):
        if not text.endswith(")"):
            raise AbiError(f"malformed tuple type '{spec}'")
        return AbiType(
            kind="tuple",
            components=tuple(parse_type(p) for p in _split_top_level(text[1:-1])),
        )

    if text == "address":
        return AbiType(kind="address", bits=160)
    if text == "bool":
        return AbiType(kind="bool", bits=8)
    if text == "bytes":
        return AbiType(kind="bytes")
    if text == "string":
        return AbiType(kind="string")

    if text.startswith("uint") or text.startswith("int"):
        signed = text.startswith("int")
        digits = text[3:] if signed else text[4:]
        bits = int(digits) if digits else 256
        if bits % 8 or not 8 <= bits <= 256:
            raise AbiError(f"invalid integer width in '{spec}'")
        return AbiType(kind="int" if signed else "uint", bits=bits)

    if text.startswith("bytes"):
        size = int(text[5:])
        if not 1 <= size <= 32:
            raise AbiError(f"invalid fixed-bytes width in '{spec}'")
        return AbiType(kind="fixed_bytes", size=size)

    raise AbiError(f"unsupported ABI type: '{spec}'")


def _canonical(node: AbiType) -> str:
    if node.kind == "array":
        return f"{_canonical(node.item)}[]"  # type: ignore[arg-type]
    if node.kind == "tuple":
        return "(" + ",".join(_canonical(c) for c in node.components) + ")"
    if node.kind in ("uint", "int"):
        return f"{node.kind}{node.bits}"
    if node.kind == "fixed_bytes":
        return f"bytes{node.size}"
    return node.kind


# ── encoding ─────────────────────────────────────────────────────────────────────


def _pad_word(raw: bytes, left: bool = True) -> bytes:
    if len(raw) > WORD:
        raise AbiError(f"value does not fit in a word ({len(raw)} bytes)")
    padding = b"\x00" * (WORD - len(raw))
    return padding + raw if left else raw + padding


def _encode_uint(value: int, bits: int) -> bytes:
    if value < 0:
        raise AbiError(f"negative value {value} for an unsigned type")
    if value >= 1 << bits:
        raise AbiError(f"{value} overflows uint{bits}")
    return value.to_bytes(WORD, "big")


def _encode_int(value: int, bits: int) -> bytes:
    limit = 1 << (bits - 1)
    if not -limit <= value < limit:
        raise AbiError(f"{value} overflows int{bits}")
    return (value & (_UINT256_CEILING - 1)).to_bytes(WORD, "big")


def _encode_address(value: str) -> bytes:
    raw = to_bytes(value if isinstance(value, str) else str(value))
    if len(raw) != 20:
        raise AbiError(f"an address is 20 bytes, got {len(raw)}")
    return _pad_word(raw)


def _encode_dynamic_bytes(raw: bytes) -> bytes:
    tail = raw + b"\x00" * ((-len(raw)) % WORD)
    return len(raw).to_bytes(WORD, "big") + tail


def encode_type(node: AbiType, value: Any) -> bytes:
    """Encode one value. Dynamic types return their full body, not an offset."""
    kind = node.kind
    if kind == "uint":
        return _encode_uint(int(value), node.bits)
    if kind == "int":
        return _encode_int(int(value), node.bits)
    if kind == "bool":
        return _encode_uint(1 if value else 0, 8)
    if kind == "address":
        return _encode_address(value)
    if kind == "fixed_bytes":
        raw = value if isinstance(value, (bytes, bytearray)) else to_bytes(value)
        if len(raw) != node.size:
            raise AbiError(f"bytes{node.size} needs {node.size} bytes, got {len(raw)}")
        return _pad_word(bytes(raw), left=False)
    if kind == "bytes":
        raw = value if isinstance(value, (bytes, bytearray)) else to_bytes(value)
        return _encode_dynamic_bytes(bytes(raw))
    if kind == "string":
        return _encode_dynamic_bytes(str(value).encode("utf-8"))
    if kind == "array":
        items = list(value)
        assert node.item is not None
        body = _encode_sequence([node.item] * len(items), items)
        return len(items).to_bytes(WORD, "big") + body
    if kind == "tuple":
        return _encode_sequence(list(node.components), list(value))
    raise AbiError(f"cannot encode kind '{kind}'")


def _encode_sequence(types: Sequence[AbiType], values: Sequence[Any]) -> bytes:
    """The head/tail algorithm: static values inline, dynamic values as offsets."""
    if len(types) != len(values):
        raise AbiError(f"expected {len(types)} value(s), got {len(values)}")

    heads: List[bytes] = []
    tails: List[bytes] = []
    # Offsets are measured from the start of *this* block, and every head slot is one
    # word wide — including the offset slots themselves.
    head_size = WORD * len(types)
    running = head_size
    for node, value in zip(types, values):
        encoded = encode_type(node, value)
        if node.dynamic:
            heads.append(running.to_bytes(WORD, "big"))
            tails.append(encoded)
            running += len(encoded)
        else:
            heads.append(encoded)
    return b"".join(heads) + b"".join(tails)


def encode_args(types: Iterable[Union[str, AbiType]], values: Sequence[Any]) -> bytes:
    parsed = [t if isinstance(t, AbiType) else parse_type(t) for t in types]
    return _encode_sequence(parsed, list(values))


def parse_signature(signature: str) -> Tuple[str, List[AbiType]]:
    """Split ``name(type,type)`` into its name and parsed argument types."""
    text = signature.strip()
    open_paren = text.find("(")
    if open_paren == -1 or not text.endswith(")"):
        raise AbiError(f"not a function signature: '{signature}'")
    name = text[:open_paren].strip()
    return name, [parse_type(p) for p in _split_top_level(text[open_paren + 1:-1])]


def encode_call(signature: str, *args: Any) -> str:
    """Build ``0x<selector><args>`` calldata for a function signature.

    The signature is canonicalised before hashing, so ``uint`` and ``uint256`` — which
    are the same type but *not* the same string — produce the same selector. Getting
    that wrong yields calldata a node answers with a bare revert, which is a
    disproportionately annoying thing to debug.
    """
    name, types = parse_signature(signature)
    canonical = f"{name}({','.join(_canonical(t) for t in types)})"
    body = _encode_sequence(types, list(args))
    return selector(canonical) + body.hex()


# ── decoding ─────────────────────────────────────────────────────────────────────


def _decode_at(node: AbiType, data: bytes, offset: int) -> Any:
    if offset + WORD > len(data) and node.kind not in ("tuple",):
        raise AbiError(f"payload truncated at byte {offset} (have {len(data)})")

    kind = node.kind
    if kind == "uint":
        return int.from_bytes(data[offset:offset + WORD], "big")
    if kind == "int":
        return to_int(data[offset:offset + WORD])
    if kind == "bool":
        return int.from_bytes(data[offset:offset + WORD], "big") != 0
    if kind == "address":
        return to_checksum_address("0x" + data[offset + 12:offset + WORD].hex())
    if kind == "fixed_bytes":
        return data[offset:offset + node.size]
    if kind in ("bytes", "string"):
        length = int.from_bytes(data[offset:offset + WORD], "big")
        start = offset + WORD
        if start + length > len(data):
            raise AbiError(f"{kind} length {length} runs past the end of the payload")
        raw = data[start:start + length]
        return raw.decode("utf-8", errors="replace") if kind == "string" else raw
    if kind == "array":
        count = int.from_bytes(data[offset:offset + WORD], "big")
        assert node.item is not None
        return _decode_sequence([node.item] * count, data, offset + WORD)
    if kind == "tuple":
        return _decode_sequence(list(node.components), data, offset)
    raise AbiError(f"cannot decode kind '{kind}'")


def _decode_sequence(types: Sequence[AbiType], data: bytes, base: int) -> List[Any]:
    out: List[Any] = []
    cursor = base
    for node in types:
        if node.dynamic:
            relative = int.from_bytes(data[cursor:cursor + WORD], "big")
            out.append(_decode_at(node, data, base + relative))
        else:
            out.append(_decode_at(node, data, cursor))
        cursor += WORD
    return out


def decode_args(types: Iterable[Union[str, AbiType]], payload: Union[str, bytes]) -> List[Any]:
    """Decode an ABI-encoded return payload into Python values."""
    parsed = [t if isinstance(t, AbiType) else parse_type(t) for t in types]
    raw = payload if isinstance(payload, (bytes, bytearray)) else to_bytes(payload)
    return _decode_sequence(parsed, bytes(raw), 0)


def decode_revert(payload: Union[str, bytes]) -> str:
    """Turn a revert return-payload into a readable reason.

    Three shapes in the wild: ``Error(string)`` for ``require(cond, "why")``,
    ``Panic(uint256)`` for compiler-inserted asserts (0x11 is the overflow every
    integer-math bug eventually produces), and empty for a bare ``revert()``.
    """
    raw = payload if isinstance(payload, (bytes, bytearray)) else to_bytes(payload)
    if not raw:
        return "reverted without a reason"
    head = bytes(raw[:4])
    if head == to_bytes(selector("Error(string)")):
        try:
            return str(decode_args(["string"], raw[4:])[0])
        except AbiError:
            return "reverted with an undecodable Error(string)"
    if head == to_bytes(selector("Panic(uint256)")):
        code = int.from_bytes(raw[4:36], "big") if len(raw) >= 36 else -1
        meanings = {
            0x01: "assert(false)",
            0x11: "arithmetic overflow or underflow",
            0x12: "division or modulo by zero",
            0x21: "invalid enum conversion",
            0x32: "array index out of bounds",
            0x41: "out of memory",
        }
        return f"panic 0x{code:02x}" + (f" — {meanings[code]}" if code in meanings else "")
    return f"reverted with unrecognised data 0x{bytes(raw[:36]).hex()}"


__all__ = [
    "WORD",
    "AbiError",
    "AbiType",
    "SELECTOR_LATEST_ROUND_DATA",
    "SELECTOR_DECIMALS",
    "SELECTOR_DESCRIPTION",
    "SELECTOR_VERSION",
    "strip0x",
    "to_bytes",
    "words",
    "to_uint",
    "to_int",
    "decode_string",
    "scale",
    "parse_type",
    "parse_signature",
    "encode_type",
    "encode_args",
    "encode_call",
    "decode_args",
    "decode_revert",
    "keccak256",
    "selector",
    "event_topic",
    "to_checksum_address",
]
