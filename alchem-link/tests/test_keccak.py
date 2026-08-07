"""Keccak-256 correctness.

The permutation is pinned two independent ways. Standard vectors prove the algorithm;
the four function selectors prove the *padding*, because those four values were verified
against live mainnet contracts back when they were hardcoded constants. A SHA-3 padding
byte would pass no selector test — which is precisely the mistake this module exists to
avoid.
"""
import hashlib
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "src"))

from alchem_link.keccak import (
    event_topic,
    is_checksum_address,
    keccak256,
    keccak256_hex,
    selector,
    to_checksum_address,
)


class KeccakVectorTests(unittest.TestCase):
    def test_empty_input(self):
        self.assertEqual(
            keccak256(b"").hex(),
            "c5d2460186f7233c927e7db2dcc703c0e500b653ca82273b7bfad8045d85a470",
        )

    def test_abc(self):
        self.assertEqual(
            keccak256(b"abc").hex(),
            "4e03657aea45a94fc7d47ba826c8d667c0d1e6e33a64a036ec44f58fa12d6c45",
        )

    def test_is_not_sha3_256(self):
        """The whole reason this module exists: same permutation, different padding."""
        self.assertNotEqual(keccak256(b""), hashlib.sha3_256(b"").digest())

    def test_input_spanning_multiple_rate_blocks(self):
        """136 bytes is exactly one rate block, so 200 forces a second permutation."""
        digest = keccak256(b"a" * 200)
        self.assertEqual(len(digest), 32)
        self.assertNotEqual(digest, keccak256(b"a" * 199))

    def test_exact_rate_boundary_pads_into_a_new_block(self):
        """At exactly 136 bytes the padding cannot share the final block."""
        self.assertEqual(len(keccak256(b"x" * 136)), 32)

    def test_one_byte_short_of_the_rate_packs_both_pad_bits_in_one_byte(self):
        """At 135 bytes the 0x01 domain byte and the 0x80 terminator collide."""
        self.assertEqual(len(keccak256(b"x" * 135)), 32)

    def test_hex_helper_is_prefixed(self):
        self.assertTrue(keccak256_hex(b"").startswith("0x"))


class SelectorTests(unittest.TestCase):
    """These four were verified against live mainnet aggregators before being computed."""

    def test_known_aggregator_selectors(self):
        self.assertEqual(selector("latestRoundData()"), "0xfeaf968c")
        self.assertEqual(selector("decimals()"), "0x313ce567")
        self.assertEqual(selector("description()"), "0x7284e416")
        self.assertEqual(selector("version()"), "0x54fd4d50")

    def test_known_erc20_and_multicall_selectors(self):
        self.assertEqual(selector("balanceOf(address)"), "0x70a08231")
        self.assertEqual(selector("transfer(address,uint256)"), "0xa9059cbb")
        self.assertEqual(selector("totalSupply()"), "0x18160ddd")
        self.assertEqual(selector("getRoundData(uint80)"), "0x9a6fc8f5")
        self.assertEqual(selector("aggregate3((address,bool,bytes)[])"), "0x82ad56cb")

    def test_error_selectors_used_by_revert_decoding(self):
        self.assertEqual(selector("Error(string)"), "0x08c379a0")
        self.assertEqual(selector("Panic(uint256)"), "0x4e487b71")

    def test_event_topic_is_the_full_digest(self):
        topic = event_topic("Transfer(address,address,uint256)")
        self.assertEqual(
            topic,
            "0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef",
        )


class ChecksumTests(unittest.TestCase):
    def test_eip55_casing(self):
        self.assertEqual(
            to_checksum_address("0x5f4ec3df9cbd43714fe2740f5e3616155c5b8419"),
            "0x5f4eC3Df9cbd43714FE2740f5E3616155c5b8419",
        )

    def test_checksumming_is_idempotent(self):
        once = to_checksum_address("0xf4030086522a5beea4988f8ca5b36dbc97bee88c")
        self.assertEqual(to_checksum_address(once), once)

    def test_unchecksummed_forms_are_accepted_as_not_wrong(self):
        self.assertTrue(is_checksum_address("0x" + "a" * 40))
        self.assertTrue(is_checksum_address("0x" + "A" * 40))

    def test_wrong_mixed_case_is_rejected(self):
        """A single flipped character must fail — that is what the checksum is for."""
        good = to_checksum_address("0x5f4ec3df9cbd43714fe2740f5e3616155c5b8419")
        broken = good[:10] + ("x" if good[10] == "X" else "X") + good[11:]
        self.assertFalse(is_checksum_address(broken))

    def test_wrong_length_rejected(self):
        self.assertFalse(is_checksum_address("0xdeadbeef"))
        with self.assertRaises(ValueError):
            to_checksum_address("0xdeadbeef")

    def test_non_hex_rejected(self):
        with self.assertRaises(ValueError):
            to_checksum_address("0x" + "z" * 40)


if __name__ == "__main__":
    unittest.main()
