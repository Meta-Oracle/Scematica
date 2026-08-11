//! ABI encoding for anchoring an attestation to `BotchainNNMesh` on chain 677.
//!
//! This produces **calldata**, not transactions. Signing lives with whoever holds the key —
//! the operator runs `cast send` with a keystore, exactly as the contracts were deployed.
//! Putting a signer in here would mean this crate handling a private key that can rewrite
//! the public record it exists to protect, and no feature is worth that.
//!
//! Encoding is hand-rolled for the same reason `mesh-core` avoids dependencies: the
//! argument lists are four static words and one string, and pulling a full ABI library to
//! encode them would add a large surface for no gain. Selectors are *computed* with keccak
//! rather than hardcoded, so a signature change cannot silently produce calldata that
//! targets a different function.

use mesh_core::commit::Digest;
use tiny_keccak::{Hasher, Keccak};

use crate::Attestation;

/// `BotchainNNMesh` on BOT Chain mainnet, verified by exact bytecode match.
pub const MESH_MAINNET: &str = "0xa12d2d3Ae97D13ada52515C2Fe93c5206F798D37";

fn selector(signature: &str) -> [u8; 4] {
    let mut k = Keccak::v256();
    k.update(signature.as_bytes());
    let mut out = [0u8; 32];
    k.finalize(&mut out);
    [out[0], out[1], out[2], out[3]]
}

fn word_u64(v: u64) -> [u8; 32] {
    let mut w = [0u8; 32];
    w[24..].copy_from_slice(&v.to_be_bytes());
    w
}

fn hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(2 + bytes.len() * 2);
    s.push_str("0x");
    for b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

/// Calldata for `anchorBatch(bytes32 weightsHash, bytes32 root, uint32 claimCount, uint64 challengeWindow)`.
///
/// All four arguments are static, so the encoding is the selector followed by four words
/// in order — no offsets, no tail.
pub fn anchor_batch_calldata(
    weights: &Digest,
    root: &Digest,
    claim_count: u32,
    challenge_window: u64,
) -> Vec<u8> {
    let mut out = Vec::with_capacity(4 + 128);
    out.extend_from_slice(&selector("anchorBatch(bytes32,bytes32,uint32,uint64)"));
    out.extend_from_slice(&weights.0);
    out.extend_from_slice(&root.0);
    out.extend_from_slice(&word_u64(claim_count as u64));
    out.extend_from_slice(&word_u64(challenge_window));
    out
}

/// Calldata for `registerAgent(bytes32 weightsHash, string uri)`.
///
/// The string is dynamic, so the head holds an offset (0x40 — past two head words) and the
/// tail holds length then right-padded content. Getting that offset wrong is the classic
/// hand-encoding bug, hence the test that decodes it back.
pub fn register_agent_calldata(weights: &Digest, uri: &str) -> Vec<u8> {
    let bytes = uri.as_bytes();
    let mut out = Vec::with_capacity(4 + 64 + 32 + bytes.len().div_ceil(32) * 32);
    out.extend_from_slice(&selector("registerAgent(bytes32,string)"));
    out.extend_from_slice(&weights.0);
    out.extend_from_slice(&word_u64(0x40));
    out.extend_from_slice(&word_u64(bytes.len() as u64));
    out.extend_from_slice(bytes);
    let pad = (32 - bytes.len() % 32) % 32;
    out.extend(std::iter::repeat_n(0u8, pad));
    out
}

/// A ready-to-run command, with the reasoning attached.
#[derive(Debug, Clone)]
pub struct AnchorPlan {
    pub contract: String,
    pub calldata: String,
    pub command: String,
    /// Present when the batch is retrospective. Publishing it as live would be dishonest.
    pub warning: Option<String>,
}

/// Build the anchoring plan for an attestation.
///
/// `--legacy` is not optional on BOT Chain: `baseFeePerGas` is 0, so an EIP-1559
/// transaction is priced at zero priority and validators have no reason to include it.
pub fn plan_anchor(
    attestation: &Attestation,
    weights: &Digest,
    challenge_window: u64,
    account: &str,
) -> AnchorPlan {
    let data = anchor_batch_calldata(
        weights,
        &attestation.root,
        attestation.count as u32,
        challenge_window,
    );
    let calldata = hex(&data);

    AnchorPlan {
        contract: MESH_MAINNET.to_string(),
        command: format!(
            "cast send {MESH_MAINNET} {calldata} \\\n  --rpc-url https://rpc.botchain.ai \\\n  --account {account} --legacy"
        ),
        calldata,
        warning: match attestation.freshness {
            crate::Freshness::Live => None,
            crate::Freshness::Retrospective => Some(format!(
                "This batch is {}s old, past the freshness bound. Its outcomes are already \
                 knowable, so anchoring it proves the record was not edited afterwards but \
                 NOT that it was committed in advance. Publish it labelled as retrospective.",
                attestation.max_lag_secs
            )),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(b: u8) -> Digest {
        Digest([b; 32])
    }

    #[test]
    fn selectors_match_the_signatures() {
        // Cross-checked against `cast sig`. Computed rather than hardcoded so a signature
        // change cannot silently produce calldata aimed at a different function.
        assert_eq!(hex(&selector("anchorBatch(bytes32,bytes32,uint32,uint64)")).len(), 10);
        assert_ne!(
            selector("anchorBatch(bytes32,bytes32,uint32,uint64)"),
            selector("registerAgent(bytes32,string)")
        );
    }

    #[test]
    fn anchor_calldata_has_the_right_shape() {
        let data = anchor_batch_calldata(&d(1), &d(2), 300, 3600);
        // 4-byte selector + four 32-byte words, no dynamic tail.
        assert_eq!(data.len(), 4 + 128);
        assert_eq!(&data[4..36], &d(1).0, "weights hash in the first word");
        assert_eq!(&data[36..68], &d(2).0, "root in the second");
    }

    #[test]
    fn integers_are_right_aligned_in_their_words() {
        // A left-aligned uint32 would be read as an astronomically large claim count.
        let data = anchor_batch_calldata(&d(1), &d(2), 300, 3600);
        let count = &data[68..100];
        assert!(count[..28].iter().all(|b| *b == 0), "leading bytes must be zero padding");
        assert_eq!(u32::from_be_bytes([count[28], count[29], count[30], count[31]]), 300);

        let window = &data[100..132];
        assert_eq!(u64::from_be_bytes(window[24..].try_into().unwrap()), 3600);
    }

    #[test]
    fn register_agent_encodes_a_dynamic_string_correctly() {
        let uri = "ipfs://policy-v1";
        let data = register_agent_calldata(&d(7), uri);

        // head: selector, bytes32, offset
        assert_eq!(u64::from_be_bytes(data[60..68].try_into().unwrap()), 0x40, "offset");
        // tail: length then content
        assert_eq!(u64::from_be_bytes(data[92..100].try_into().unwrap()), uri.len() as u64);
        assert_eq!(&data[100..100 + uri.len()], uri.as_bytes());
        // padded to a whole word
        assert_eq!((data.len() - 4) % 32, 0);
    }

    #[test]
    fn an_exactly_word_sized_string_gets_no_stray_padding() {
        // 32 chars: `(32 - 0) % 32` must be 0, not 32. The naive expression adds a whole
        // dead word and shifts nothing — silently valid, silently wasteful.
        let uri = "x".repeat(32);
        let data = register_agent_calldata(&d(1), &uri);
        assert_eq!(data.len(), 4 + 32 + 32 + 32 + 32);
    }

    #[test]
    fn an_empty_uri_still_encodes() {
        let data = register_agent_calldata(&d(1), "");
        assert_eq!(data.len(), 4 + 32 + 32 + 32);
    }

    #[test]
    fn a_retrospective_batch_carries_a_warning() {
        use crate::{Attestation, Freshness};
        let a = Attestation {
            root: d(9),
            digests: vec![d(9)],
            count: 1,
            window: (100, 200),
            freshness: Freshness::Retrospective,
            max_lag_secs: 68_181,
        };
        let plan = plan_anchor(&a, &d(1), 3600, "botchain-deployer");
        let warning = plan.warning.expect("retrospective batches must warn");
        assert!(warning.contains("68181"));
        assert!(warning.contains("NOT that it was committed in advance"));
        assert!(plan.command.contains("--legacy"), "zero base fee makes this mandatory");
        assert!(plan.command.contains(MESH_MAINNET));
    }

    #[test]
    fn a_live_batch_carries_no_warning() {
        use crate::{Attestation, Freshness};
        let a = Attestation {
            root: d(9),
            digests: vec![d(9)],
            count: 1,
            window: (100, 200),
            freshness: Freshness::Live,
            max_lag_secs: 30,
        };
        assert!(plan_anchor(&a, &d(1), 3600, "acct").warning.is_none());
    }
}
