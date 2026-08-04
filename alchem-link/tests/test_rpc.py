"""RPC client and endpoint-resolution tests, with the transport stubbed.

No sockets are opened: `_post` is replaced so retry, error mapping and header
behaviour can be asserted deterministically.
"""
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "src"))

from alchem_link.health import diagnose
from alchem_link.networks import (
    ALCHEMY_KEY_ENV,
    ALCHEMY_URL_ENV,
    get_network,
    resolve_endpoint,
)
from alchem_link.rpc import RpcClient, RpcError, RpcTransportError, gwei


class StubClient(RpcClient):
    """RpcClient with the single HTTP attempt swapped for a scripted response queue.

    Stubbing `_attempt` (one round trip) rather than `_post` (attempt + retry policy)
    keeps the real retry loop under test.
    """

    def __init__(self, endpoint, responses, **kwargs):
        super().__init__(endpoint, **kwargs)
        self.responses = list(responses)
        self.calls = []

    def _attempt(self, payload):
        import json as _json

        self.calls.append(_json.loads(payload.decode()))
        if not self.responses:
            raise RpcTransportError("stub exhausted")
        nxt = self.responses.pop(0)
        if isinstance(nxt, Exception):
            raise nxt
        return nxt


def endpoint(network="ethereum", env=None):
    return resolve_endpoint(network=network, env=env or {})


class EndpointResolutionTests(unittest.TestCase):
    def test_falls_back_to_public_endpoint(self):
        ep = resolve_endpoint("ethereum", env={})
        self.assertEqual(ep.source, "public fallback")
        self.assertFalse(ep.is_authenticated)
        self.assertEqual(ep.url, get_network("ethereum").public_rpc)

    def test_api_key_builds_alchemy_url(self):
        ep = resolve_endpoint("base", env={ALCHEMY_KEY_ENV: "secret-key"})
        self.assertEqual(ep.source, ALCHEMY_KEY_ENV)
        self.assertTrue(ep.is_authenticated)
        self.assertEqual(ep.url, "https://base-mainnet.g.alchemy.com/v2/secret-key")

    def test_explicit_url_env_wins_over_api_key(self):
        ep = resolve_endpoint(
            "ethereum",
            env={ALCHEMY_URL_ENV: "https://my-node.example/rpc", ALCHEMY_KEY_ENV: "k"},
        )
        self.assertEqual(ep.source, ALCHEMY_URL_ENV)
        self.assertEqual(ep.url, "https://my-node.example/rpc")

    def test_argument_wins_over_everything(self):
        ep = resolve_endpoint(
            "ethereum",
            rpc_url="https://override.example",
            env={ALCHEMY_URL_ENV: "https://env.example", ALCHEMY_KEY_ENV: "k"},
        )
        self.assertEqual(ep.source, "--rpc-url")
        self.assertEqual(ep.url, "https://override.example")

    def test_redaction_hides_the_api_key(self):
        ep = resolve_endpoint("ethereum", env={ALCHEMY_KEY_ENV: "super-secret"})
        self.assertNotIn("super-secret", ep.redacted())
        self.assertTrue(ep.redacted().endswith("/v2/<key>"))

    def test_redaction_leaves_keyless_urls_alone(self):
        ep = resolve_endpoint("ethereum", env={})
        self.assertEqual(ep.redacted(), ep.url)

    def test_blank_env_values_are_ignored(self):
        ep = resolve_endpoint("ethereum", env={ALCHEMY_KEY_ENV: "   "})
        self.assertEqual(ep.source, "public fallback")

    def test_unknown_network_lists_known_ones(self):
        with self.assertRaises(KeyError) as ctx:
            resolve_endpoint("solana-mainnet", env={})
        self.assertIn("ethereum", str(ctx.exception))


class RpcClientTests(unittest.TestCase):
    def test_parses_result(self):
        client = StubClient(endpoint(), [{"jsonrpc": "2.0", "id": 1, "result": "0x10"}])
        self.assertEqual(client.block_number(), 16)

    def test_sets_jsonrpc_envelope(self):
        client = StubClient(endpoint(), [{"result": "0x1"}])
        client.call("eth_blockNumber")
        sent = client.calls[0]
        self.assertEqual(sent["jsonrpc"], "2.0")
        self.assertEqual(sent["method"], "eth_blockNumber")
        self.assertEqual(sent["params"], [])

    def test_increments_request_id(self):
        client = StubClient(endpoint(), [{"result": "0x1"}, {"result": "0x2"}])
        client.call("eth_blockNumber")
        client.call("eth_chainId")
        self.assertEqual([c["id"] for c in client.calls], [1, 2])

    def test_jsonrpc_error_becomes_RpcError(self):
        client = StubClient(
            endpoint(), [{"error": {"code": -32000, "message": "execution reverted"}}]
        )
        with self.assertRaises(RpcError) as ctx:
            client.call("eth_call")
        self.assertIn("execution reverted", str(ctx.exception))
        self.assertIn("-32000", str(ctx.exception))

    def test_missing_result_field_is_an_error(self):
        client = StubClient(endpoint(), [{"jsonrpc": "2.0", "id": 1}])
        with self.assertRaises(RpcError):
            client.call("eth_blockNumber")

    def test_transport_failure_is_retried_then_raised(self):
        client = StubClient(
            endpoint(),
            [RpcTransportError("boom"), RpcTransportError("boom")],
            retries=1,
        )
        with self.assertRaises(RpcTransportError):
            client.call("eth_blockNumber")
        self.assertEqual(len(client.calls), 2)

    def test_retry_succeeds_on_the_second_attempt(self):
        client = StubClient(
            endpoint(), [RpcTransportError("flaky"), {"result": "0x2a"}], retries=1
        )
        self.assertEqual(client.block_number(), 42)
        self.assertEqual(len(client.calls), 2)

    def test_fatal_transport_error_is_not_retried(self):
        client = StubClient(
            endpoint(),
            [RpcTransportError("HTTP 401", retryable=False), {"result": "0x1"}],
            retries=3,
        )
        with self.assertRaises(RpcTransportError):
            client.call("eth_blockNumber")
        self.assertEqual(len(client.calls), 1, "a 4xx must not burn retries")

    def test_chain_id_and_gas_price_decode_hex(self):
        client = StubClient(endpoint(), [{"result": "0x89"}, {"result": "0x3b9aca00"}])
        self.assertEqual(client.chain_id(), 137)
        self.assertEqual(client.gas_price_wei(), 1_000_000_000)

    def test_read_aggregator_issues_three_calls(self):
        client = StubClient(
            endpoint(),
            [{"result": "0xaa"}, {"result": "0xbb"}, {"result": "0xcc"}],
        )
        raw = client.read_aggregator("0x" + "11" * 20)
        self.assertEqual(set(raw), {"latest_round_data", "decimals", "description"})
        self.assertEqual(len(client.calls), 3)


class GweiTests(unittest.TestCase):
    def test_converts_wei(self):
        self.assertAlmostEqual(gwei(1_500_000_000), 1.5)


class DoctorTests(unittest.TestCase):
    def test_flags_a_chain_id_mismatch(self):
        # block number, then a chain id belonging to a different chain
        client = StubClient(
            endpoint(),
            [
                {"result": "0x10"},          # eth_blockNumber
                {"result": "0x89"},          # eth_chainId -> 137, but we asked for ethereum
                {"result": "0x3b9aca00"},    # eth_gasPrice
                RpcTransportError("no feed"),
                RpcTransportError("no feed"),
                RpcTransportError("no feed"),
            ],
        )
        result = diagnose(network="ethereum", client=client)
        chain_check = next(c for c in result.checks if c.name == "chain id")
        self.assertFalse(chain_check.ok)
        self.assertFalse(result.ok)

    def test_unreachable_rpc_stops_early(self):
        client = StubClient(endpoint(), [RpcTransportError("down"), RpcTransportError("down")])
        result = diagnose(network="ethereum", client=client)
        self.assertFalse(result.ok)
        names = [c.name for c in result.checks]
        self.assertIn("rpc reachable", names)
        self.assertNotIn("gas price", names)


if __name__ == "__main__":
    unittest.main()
