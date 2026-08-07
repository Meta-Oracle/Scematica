"""ABI encoding and decoding.

The tuple-array cases are the ones that matter most: ``Multicall3.aggregate3`` takes
``(address,bool,bytes)[]``, and every batched read in the package depends on the head/tail
offsets being right. A wrong offset produces calldata that a node answers with a bare
revert and no explanation.
"""
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "src"))

from alchem_link.abi import (
    AbiError,
    decode_args,
    decode_revert,
    encode_args,
    encode_call,
    parse_signature,
    parse_type,
)

ADDRESS = "0x5f4eC3Df9cbd43714FE2740f5E3616155c5b8419"


class TypeParsingTests(unittest.TestCase):
    def test_bare_uint_defaults_to_256(self):
        self.assertEqual(parse_type("uint").bits, 256)

    def test_dynamic_types_are_flagged(self):
        self.assertTrue(parse_type("bytes").dynamic)
        self.assertTrue(parse_type("string").dynamic)
        self.assertTrue(parse_type("uint256[]").dynamic)
        self.assertFalse(parse_type("uint256").dynamic)
        self.assertFalse(parse_type("address").dynamic)

    def test_tuple_is_dynamic_only_if_a_component_is(self):
        self.assertFalse(parse_type("(address,bool)").dynamic)
        self.assertTrue(parse_type("(address,bool,bytes)").dynamic)

    def test_nested_tuple_commas_are_not_split(self):
        node = parse_type("(address,(uint256,bool),bytes)")
        self.assertEqual(len(node.components), 3)
        self.assertEqual(node.components[1].kind, "tuple")

    def test_fixed_size_arrays_are_rejected_loudly(self):
        """Unsupported rather than silently mis-encoded."""
        with self.assertRaises(AbiError):
            parse_type("uint256[3]")

    def test_invalid_integer_width_rejected(self):
        with self.assertRaises(AbiError):
            parse_type("uint257")
        with self.assertRaises(AbiError):
            parse_type("uint7")

    def test_unknown_type_rejected(self):
        with self.assertRaises(AbiError):
            parse_type("widget")

    def test_signature_parsing(self):
        name, types = parse_signature("getRoundData(uint80)")
        self.assertEqual(name, "getRoundData")
        self.assertEqual([t.kind for t in types], ["uint"])


class EncodingTests(unittest.TestCase):
    def test_no_argument_call_is_just_the_selector(self):
        self.assertEqual(encode_call("latestRoundData()"), "0xfeaf968c")

    def test_uint_argument(self):
        data = encode_call("getRoundData(uint80)", 5)
        self.assertEqual(data[:10], "0x9a6fc8f5")
        self.assertEqual(int(data[10:], 16), 5)

    def test_address_argument_is_left_padded(self):
        data = encode_call("balanceOf(address)", ADDRESS)
        self.assertEqual(data[:10], "0x70a08231")
        self.assertTrue(data[10:].startswith("0" * 24))

    def test_uint_alias_canonicalises_before_hashing(self):
        """`uint` and `uint256` are the same type but not the same string."""
        self.assertEqual(encode_call("f(uint)", 1), encode_call("f(uint256)", 1))

    def test_overflow_is_rejected(self):
        with self.assertRaises(AbiError):
            encode_args(["uint8"], [256])

    def test_negative_into_unsigned_is_rejected(self):
        with self.assertRaises(AbiError):
            encode_args(["uint256"], [-1])

    def test_signed_round_trip_handles_negatives(self):
        """Chainlink answers are int256 and can legitimately go negative."""
        payload = encode_args(["int256"], [-12345])
        self.assertEqual(decode_args(["int256"], payload)[0], -12345)

    def test_arity_mismatch_is_rejected(self):
        with self.assertRaises(AbiError):
            encode_args(["uint256", "uint256"], [1])


class RoundTripTests(unittest.TestCase):
    def test_static_tuple(self):
        values = [[ADDRESS, True]]
        payload = encode_args(["(address,bool)"], values)
        self.assertEqual(decode_args(["(address,bool)"], payload), values)

    def test_dynamic_string_and_bytes(self):
        payload = encode_args(["string", "bytes"], ["ETH / USD", b"\xfe\xaf\x96\x8c"])
        decoded = decode_args(["string", "bytes"], payload)
        self.assertEqual(decoded[0], "ETH / USD")
        self.assertEqual(decoded[1], b"\xfe\xaf\x96\x8c")

    def test_multicall3_tuple_array(self):
        """The exact shape aggregate3 takes."""
        calls = [
            [ADDRESS, True, b"\xfe\xaf\x96\x8c"],
            ["0x0000000000000000000000000000000000000001", False, b""],
        ]
        payload = encode_args(["(address,bool,bytes)[]"], [calls])
        self.assertEqual(decode_args(["(address,bool,bytes)[]"], payload)[0], calls)

    def test_multicall3_return_shape(self):
        results = [[True, b"\x01\x02"], [False, b""]]
        payload = encode_args(["(bool,bytes)[]"], [results])
        self.assertEqual(decode_args(["(bool,bytes)[]"], payload)[0], results)

    def test_empty_array_round_trips(self):
        payload = encode_args(["(address,bool,bytes)[]"], [[]])
        self.assertEqual(decode_args(["(address,bool,bytes)[]"], payload)[0], [])

    def test_mixed_static_and_dynamic_ordering(self):
        """Static values inline, dynamic ones behind offsets — order must survive."""
        values = [42, "hello", ADDRESS, [1, 2, 3]]
        types = ["uint256", "string", "address", "uint256[]"]
        self.assertEqual(decode_args(types, encode_args(types, values)), values)

    def test_decoded_addresses_come_back_checksummed(self):
        decoded = decode_args(["address"], encode_args(["address"], [ADDRESS.lower()]))
        self.assertEqual(decoded[0], ADDRESS)

    def test_latest_round_data_shape(self):
        types = ["uint80", "int256", "uint256", "uint256", "uint80"]
        values = [129127208515966893596, 192960000000, 1786108151, 1786108151, 129127208515966893596]
        self.assertEqual(decode_args(types, encode_args(types, values)), values)


class RevertDecodingTests(unittest.TestCase):
    def test_error_string(self):
        payload = "0x08c379a0" + encode_args(["string"], ["price is stale"]).hex()
        self.assertEqual(decode_revert(payload), "price is stale")

    def test_panic_code_is_named(self):
        payload = "0x4e487b71" + encode_args(["uint256"], [0x11]).hex()
        self.assertIn("overflow", decode_revert(payload))

    def test_empty_revert(self):
        self.assertEqual(decode_revert("0x"), "reverted without a reason")

    def test_unrecognised_payload_is_reported_not_guessed(self):
        self.assertIn("unrecognised", decode_revert("0xdeadbeef" + "00" * 32))


class MalformedPayloadTests(unittest.TestCase):
    def test_odd_length_hex(self):
        with self.assertRaises(AbiError):
            decode_args(["uint256"], "0xabc")

    def test_non_hex(self):
        with self.assertRaises(AbiError):
            decode_args(["uint256"], "0xzz")

    def test_truncated_payload(self):
        with self.assertRaises(AbiError):
            decode_args(["uint256", "uint256"], "0x" + "00" * 32)


if __name__ == "__main__":
    unittest.main()
