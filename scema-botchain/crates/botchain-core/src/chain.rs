//! BOT Chain network constants.
//!
//! Values are from BOT Chain's own developer guide, with two corrections made after
//! checking them against the live chain and the public registry. Both are recorded here
//! rather than silently patched, because someone will otherwise "fix" this file back to
//! matching the docs.

/// A BOT Chain network.
#[derive(Debug, Clone)]
pub struct Network {
    pub name: &'static str,
    pub chain_id: u64,
    /// RPC endpoints in the order they should be tried.
    pub endpoints: &'static [Endpoint],
    pub explorer: &'static str,
    pub symbol: &'static str,
    pub decimals: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EndpointKind {
    /// A full node's JSON-RPC. Everything works, including sending transactions.
    Node,
    /// A block explorer's JSON-RPC proxy. **Reads only** — treat writes as unavailable.
    ExplorerProxy,
}

#[derive(Debug, Clone, Copy)]
pub struct Endpoint {
    pub url: &'static str,
    pub kind: EndpointKind,
    /// Why it is in the list at this position.
    pub note: &'static str,
}

/// Mainnet — chain 677.
pub const MAINNET: Network = Network {
    name: "BOT Chain Mainnet",
    chain_id: 677,
    endpoints: &[
        Endpoint {
            url: "https://rpc.botchain.ai",
            kind: EndpointKind::Node,
            note: "official node RPC; the only endpoint that can send transactions",
        },
        Endpoint {
            // Measured 2026-08: the official RPC resolves (52.198.35.211, AWS Tokyo) but
            // does not complete a TCP connection from every network, while this host is
            // Cloudflare-fronted and answers reliably. It returned the correct chain id
            // (0x2a5) and full block bodies. Reads only — a sniper still needs the node
            // RPC to trade, but a dashboard should not go dark because one origin is
            // unreachable.
            url: "https://scan.botchain.ai/api/eth-rpc",
            kind: EndpointKind::ExplorerProxy,
            note: "explorer JSON-RPC proxy; reachable when the node RPC is not",
        },
    ],
    explorer: "https://scan.botchain.ai",
    symbol: "BOT",
    decimals: 18,
};

/// Testnet — chain 968.
///
/// # The chain ID is not unique, and that matters
///
/// 968 is registered on ChainList as **Datagram** (`mainnet.datagram.network`), not as
/// BOT Chain testnet. It is also `RialtoChainConfig` in the bsc source BOT Chain is
/// forked from — which is where it came from.
///
/// Consequence: **never identify this network by chain id alone.** A wallet
/// "add network" flow driven by a registry lookup resolves 968 to the wrong chain, and
/// any code that treats `eth_chainId == 968` as proof of BOT Chain testnet is trusting a
/// number two other chains also answer with. Pin the endpoint, then verify the chain id
/// against it — that ordering is the whole safeguard.
pub const TESTNET: Network = Network {
    name: "BOT Chain Testnet",
    chain_id: 968,
    endpoints: &[Endpoint {
        url: "https://rpc.bohr.life",
        kind: EndpointKind::Node,
        note: "official testnet RPC (host was unreachable from some networks in testing)",
    }],
    explorer: "https://scan.bohr.life",
    symbol: "BOT",
    decimals: 18,
};

pub const NETWORKS: &[&Network] = &[&MAINNET, &TESTNET];

pub fn by_name(name: &str) -> Option<&'static Network> {
    match name.to_lowercase().as_str() {
        "mainnet" | "bot" | "677" => Some(&MAINNET),
        "testnet" | "bohr" | "968" => Some(&TESTNET),
        _ => None,
    }
}

/// A DEX venue, resolved on-chain rather than taken from documentation.
///
/// Each `factory` was read by calling `factory()` (selector `0xc45a0155`) on the router
/// found in live transactions — not copied from a docs page, which is how you end up
/// watching an address nobody uses.
#[derive(Debug, Clone, Copy)]
pub struct Venue {
    pub name: &'static str,
    pub router: &'static str,
    pub factory: &'static str,
}

/// Mainnet venues, as of August 2026.
///
/// Two of them, which is the minimum for cross-DEX arb to be conceivable. Note
/// `CASwapRouter` reverts on `WETH()`, so it is **not** a stock Uniswap-V2 router — do
/// not assume a V2 ABI when building swaps against it.
pub const MAINNET_VENUES: &[Venue] = &[
    Venue {
        name: "SwapRouter (V3-style)",
        router: "0x07032d47A1b9f8460cBeE9dC17c1d3E438693929",
        factory: "0x1c51c173323ec11bb4e3c4fd2314c225dc4b5419",
    },
    Venue {
        name: "CASwapRouter",
        router: "0x5b90611D4eB8FC82Fc2E3d1F0501Dd6F434441AD",
        factory: "0x9c937ebc3748825026677e20b13b5e306494a38d",
    },
];

/// Well-known mainnet tokens.
///
/// `WBOT` is listed twice on the explorer under the same name; this is the one carrying
/// the holder count (57k+). Verify before routing anything through it — picking the
/// wrong wrapped-native address means swapping into a token with no liquidity.
pub const WBOT: &str = "0xD5452816194a3784dBa983426cCe7c122F4abd30";
pub const USDT: &str = "0xaBabc7Ddc03e501d190C676BF3d92ef0e6e87a3C";
/// `CA` / CaryPact — the dominant token by holders (451k+).
pub const CA: &str = "0x546307af427902A75771434Df831d88219784E19";

/// Parlia's validator-set system contract. Its per-block `deposit` dominates chain
/// activity, so exclude it before drawing conclusions about *user* volume.
pub const VALIDATOR_SET: &str = "0x0000000000000000000000000000000000001000";

#[cfg(test)]
mod venue_tests {
    use super::*;

    #[test]
    fn venues_are_distinct_and_well_formed() {
        // Two venues is the floor for arb to mean anything; one venue is a swap shop.
        assert!(MAINNET_VENUES.len() >= 2);
        for v in MAINNET_VENUES {
            assert!(v.router.starts_with("0x") && v.router.len() == 42);
            assert!(v.factory.starts_with("0x") && v.factory.len() == 42);
        }
        assert_ne!(MAINNET_VENUES[0].factory, MAINNET_VENUES[1].factory);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mainnet_prefers_a_node_over_the_explorer_proxy() {
        // Order is load-bearing: the proxy cannot send transactions, so it must never be
        // the first thing a trading path reaches for.
        assert_eq!(MAINNET.endpoints[0].kind, EndpointKind::Node);
        assert!(MAINNET
            .endpoints
            .iter()
            .any(|e| e.kind == EndpointKind::ExplorerProxy));
    }

    #[test]
    fn networks_resolve_by_alias() {
        assert_eq!(by_name("mainnet").unwrap().chain_id, 677);
        assert_eq!(by_name("968").unwrap().chain_id, 968);
        assert!(by_name("ethereum").is_none());
    }

    #[test]
    fn every_endpoint_is_https() {
        // Plain HTTP would put the RPC on the wire in clear text, and on the trading path
        // that is a transaction anyone on the route can read before it lands.
        for n in NETWORKS {
            assert!(n.endpoints.iter().all(|e| e.url.starts_with("https://")));
        }
    }
}
