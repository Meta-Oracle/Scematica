"""ABI decoding tests.

Fixtures are real responses captured from mainnet aggregator 0x5f4eC3Df...19 (ETH/USD).
Pinning them here means a change to the decoder is caught offline, and the selector
constants stay honest without a keccak dependency.
"""
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "src"))

from alchem_link.abi import (
    AbiError,
    SELECTOR_DECIMALS,
    SELECTOR_DESCRIPTION,
    SELECTOR_LATEST_ROUND_DATA,
    decode_string,
    scale,
    to_int,
    to_uint,
    words,
)

# Captured live: eth_call(latestRoundData()) on ETH/USD mainnet.
LATEST_ROUND_DATA = (
    "0x0000000000000000000000000000000000000000000000070000000000007dd0"
    "0000000000000000000000000000000000000000000000000000002bb96bfc45"
    "000000000000000000000000000000000000000000000000000000006a724e85"
    "000000000000000000000000000000000000000000000000000000006a724e97"
    "0000000000000000000000000000000000000000000000070000000000007dd0"
)
DECIMALS_8 = "0x0000000000000000000000000000000000000000000000000000000000000008"
DESCRIPTION_ETH_USD = (
    "0x0000000000000000000000000000000000000000000000000000000000000020"
    "0000000000000000000000000000000000000000000000000000000000000009"
    "455448202f205553440000000000000000000000000000000000000000000000"
)


class SelectorTests(unittest.TestCase):
    def test_selectors_are_four_byte_hex(self):
        for selector in (
            SELECTOR_LATEST_ROUND_DATA,
            SELECTOR_DECIMALS,
            SELECTOR_DESCRIPTION,
        ):
            self.assertTrue(selector.startswith("0x"))
            self.assertEqual(len(selector), 10, selector)


class WordTests(unittest.TestCase):
    def test_splits_into_32_byte_words(self):
        self.assertEqual(len(words(LATEST_ROUND_DATA)), 5)

    def test_rejects_partial_word(self):
        with self.assertRaises(AbiError):
            words("0x1234")

    def test_rejects_non_hex(self):
        with self.assertRaises(AbiError):
            words("0xzzzz")

    def test_rejects_odd_length(self):
        with self.assertRaises(AbiError):
            words("0x123")


class IntegerTests(unittest.TestCase):
    def test_decodes_positive_answer(self):
        answer = to_int(words(LATEST_ROUND_DATA)[1])
        self.assertEqual(answer, 0x2BB96BFC45)

    def test_decodes_uint_round_id(self):
        self.assertEqual(to_uint(words(LATEST_ROUND_DATA)[0]), 0x070000000000007DD0)

    def test_decodes_updated_at(self):
        self.assertEqual(to_uint(words(LATEST_ROUND_DATA)[3]), 0x6A724E97)

    def test_negative_int256_uses_twos_complement(self):
        # -1 is all bits set. Feeds can and do report negative answers.
        minus_one = bytes.fromhex("ff" * 32)
        self.assertEqual(to_int(minus_one), -1)

    def test_most_negative_int256(self):
        word = bytes.fromhex("80" + "00" * 31)
        self.assertEqual(to_int(word), -(2 ** 255))

    def test_max_positive_int256(self):
        word = bytes.fromhex("7f" + "ff" * 31)
        self.assertEqual(to_int(word), 2 ** 255 - 1)

    def test_uint_and_int_diverge_above_the_sign_bit(self):
        word = bytes.fromhex("ff" * 32)
        self.assertEqual(to_uint(word), 2 ** 256 - 1)
        self.assertEqual(to_int(word), -1)


class StringTests(unittest.TestCase):
    def test_decodes_dynamic_string(self):
        self.assertEqual(decode_string(DESCRIPTION_ETH_USD), "ETH / USD")

    def test_rejects_short_payload(self):
        with self.assertRaises(AbiError):
            decode_string("0x00")

    def test_rejects_offset_past_end(self):
        bad = "0x" + ("ff" * 32)
        with self.assertRaises(AbiError):
            decode_string(bad)


class ScaleTests(unittest.TestCase):
    def test_scales_by_decimals(self):
        answer = to_int(words(LATEST_ROUND_DATA)[1])
        decimals = to_uint(words(DECIMALS_8)[0])
        self.assertEqual(decimals, 8)
        self.assertAlmostEqual(scale(answer, decimals), 1877.94455621, places=8)

    def test_zero_decimals_is_identity(self):
        self.assertEqual(scale(42, 0), 42)

    def test_negative_decimals_rejected(self):
        with self.assertRaises(AbiError):
            scale(1, -1)


if __name__ == "__main__":
    unittest.main()
