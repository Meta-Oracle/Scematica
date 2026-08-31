//! Who may read which world tree.
//!
//! A `scema-nft` token is not a picture with a world attached — the token's metadata *is* a
//! commitment to one specific world, carried in `scema.world_commitment`. That binding
//! already exists, so entitlement needs no new identifier and no tier table: **holding the
//! token for world `X` entitles the holder to the record behind digest `X`.** Nothing else.
//!
//! ## The three answers, and why there are three
//!
//! [`Decision`] is `Granted`, `Denied` or `Undetermined`. Collapsing the third into the
//! second is the obvious simplification and it is wrong for the same reason
//! `alchem_link.agent._refusal` distinguishes a policy refusal from a declined prompt: "you
//! do not own this" and "the chain would not answer" are different facts, and only one is
//! about the holder. A reader told the first goes away; a reader told the second retries.
//!
//! Access control still **fails closed** — `Undetermined` serves nothing. Failing closed and
//! reporting accurately are independent choices, and this makes both.
//!
//! ## What an entitlement is not
//!
//! **It never grants write.** This answers *what may be read*; `scema_tools::Workspace`
//! answers *where* and `scema-trust` answers *whether an action may happen*. Merging any two
//! of those is how a grant for one silently becomes a grant for another.
//!
//! **It does not make a sealed record less verifiable.** A record somebody already holds
//! verifies with no server, no key and no permission — that is the entire point of
//! `scema verify` and `/omni`, and gating the *corpus* must never be mistaken for gating
//! *verification*. If this crate ever appears in a verification path, something has gone
//! wrong. It gates distribution, not truth.
//!
//! **It is not a paywall on your own records.** Records under your own `.scema/` are yours;
//! this exists for a server distributing a corpus to holders.

use serde::{Deserialize, Serialize};

pub mod challenge;

pub use challenge::{Challenge, ChallengeError};

/// A token on some chain. Opaque strings, because this crate does not talk to any chain and
/// must not imply it knows which ones exist.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenRef {
    pub chain: String,
    pub contract: String,
    pub token_id: String,
}

/// An address claiming to hold a token. How the caller established that this really is the
/// requester is the caller's problem — see [`Challenge`] for the replay-resistant form.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Holder(pub String);

/// What a token entitles its holder to read.
///
/// Exactly one world. A token minted from world `X` says so in its own metadata, and this
/// carries the same digest — so the grant is derived from the artefact rather than recorded
/// beside it, and the two cannot drift apart.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entitlement {
    pub token: TokenRef,
    /// Hex digest of the canonical encoding of the world. 64 lowercase hex characters.
    pub world_commitment: String,
}

impl Entitlement {
    /// Read the entitlement out of token metadata as `scema-nft` writes it.
    ///
    /// Returns `None` rather than a default when the metadata carries no commitment: a token
    /// that does not name a world entitles its holder to nothing, and inventing an empty
    /// digest would make it entitle them to a record that cannot exist — or, worse, match a
    /// record whose commitment field was also empty.
    pub fn from_metadata(token: TokenRef, metadata: &serde_json::Value) -> Option<Self> {
        let d = metadata.get("scema")?.get("world_commitment")?.as_str()?;
        if !is_digest(d) {
            return None;
        }
        Some(Entitlement { token, world_commitment: d.to_string() })
    }
}

/// A well-formed commitment: 64 lowercase hex characters.
///
/// Checked rather than assumed. A digest compared case-insensitively, or one accepted at the
/// wrong length, turns an exact-match gate into a prefix game.
pub fn is_digest(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// Whether an address holds a token, right now.
///
/// **Tri-state, and `Unknown` is not a denial.** An RPC timeout, a rate limit and a
/// reorganised chain all produce it, and none of them is a fact about the holder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Ownership {
    Held,
    NotHeld,
    Unknown { why: String },
}

/// Answers ownership. The only part of this crate that needs a chain.
///
/// A trait so the meaning stays testable offline — and so a deployment can source ownership
/// from an indexer, a node, or a signed attestation without any of that leaking into the
/// decision logic below.
pub trait OwnershipOracle {
    fn holds(&self, token: &TokenRef, holder: &Holder) -> Ownership;
}

/// The outcome of an access request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Decision {
    /// Serve the world behind this commitment, and nothing else.
    Granted { world_commitment: String },
    /// A fact about the request. The holder can act on this.
    Denied { reason: DenialReason },
    /// A fact about the *infrastructure*. Nothing is served, and the holder should retry.
    Undetermined { why: String },
}

impl Decision {
    /// Whether data may be served. `Undetermined` is false — this fails closed.
    pub fn permits(&self) -> bool {
        matches!(self, Decision::Granted { .. })
    }

    /// One line for a log or an error body.
    pub fn headline(&self) -> String {
        match self {
            Decision::Granted { world_commitment } => {
                format!("granted: world {}", &world_commitment[..16.min(world_commitment.len())])
            }
            Decision::Denied { reason } => format!("denied: {}", reason.explain()),
            Decision::Undetermined { why } => {
                format!("undetermined: {why} — this is not a denial; retry")
            }
        }
    }
}

/// Why a request was refused. Each is a different instruction to the requester.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DenialReason {
    /// The address does not hold the token.
    NotAHolder,
    /// The token is held, but it commits to a different world than the one requested.
    WrongWorld { entitled_to: String, requested: String },
    /// The requested commitment is not a well-formed digest.
    MalformedRequest,
    /// The proof of control was not valid — expired, replayed, or for another challenge.
    Unproven { detail: String },
}

impl DenialReason {
    pub fn explain(&self) -> String {
        match self {
            DenialReason::NotAHolder => {
                "that address does not hold the token".to_string()
            }
            DenialReason::WrongWorld { entitled_to, requested } => format!(
                "the token commits to world {} and you asked for {} — one token, one world",
                &entitled_to[..16.min(entitled_to.len())],
                &requested[..16.min(requested.len())]
            ),
            DenialReason::MalformedRequest => {
                "the requested commitment is not 64 lowercase hex characters".to_string()
            }
            DenialReason::Unproven { detail } => {
                format!("control of the address was not proven: {detail}")
            }
        }
    }
}

/// Decide whether `holder` may read the world at `requested`.
///
/// Order matters and is fixed: shape of the request, then what the token entitles, then
/// whether it is held. Checking ownership first would leak, by timing and by error message,
/// whether an arbitrary address holds a token — for a request that was never going to be
/// granted anyway.
pub fn authorise(
    oracle: &dyn OwnershipOracle,
    entitlement: &Entitlement,
    holder: &Holder,
    requested: &str,
) -> Decision {
    if !is_digest(requested) {
        return Decision::Denied { reason: DenialReason::MalformedRequest };
    }
    if entitlement.world_commitment != requested {
        return Decision::Denied {
            reason: DenialReason::WrongWorld {
                entitled_to: entitlement.world_commitment.clone(),
                requested: requested.to_string(),
            },
        };
    }
    match oracle.holds(&entitlement.token, holder) {
        Ownership::Held => Decision::Granted { world_commitment: requested.to_string() },
        Ownership::NotHeld => Decision::Denied { reason: DenialReason::NotAHolder },
        Ownership::Unknown { why } => Decision::Undetermined { why },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixed(Ownership);
    impl OwnershipOracle for Fixed {
        fn holds(&self, _t: &TokenRef, _h: &Holder) -> Ownership {
            self.0.clone()
        }
    }

    fn token() -> TokenRef {
        TokenRef { chain: "eip155:1".into(), contract: "0xabc".into(), token_id: "7".into() }
    }

    const A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

    fn ent() -> Entitlement {
        Entitlement { token: token(), world_commitment: A.into() }
    }

    #[test]
    fn a_holder_may_read_the_world_their_token_commits_to() {
        let d = authorise(&Fixed(Ownership::Held), &ent(), &Holder("0x1".into()), A);
        assert_eq!(d, Decision::Granted { world_commitment: A.into() });
        assert!(d.permits());
    }

    #[test]
    fn one_token_grants_one_world() {
        // The binding is the whole design: a token is minted from a world and commits to it
        // in its own metadata. Holding it is not a subscription to the corpus.
        let d = authorise(&Fixed(Ownership::Held), &ent(), &Holder("0x1".into()), B);
        assert!(!d.permits());
        match d {
            Decision::Denied { reason: DenialReason::WrongWorld { .. } } => {}
            other => panic!("expected WrongWorld, got {other:?}"),
        }
    }

    #[test]
    fn a_non_holder_is_denied_and_told_so() {
        let d = authorise(&Fixed(Ownership::NotHeld), &ent(), &Holder("0x2".into()), A);
        assert_eq!(d, Decision::Denied { reason: DenialReason::NotAHolder });
    }

    #[test]
    fn an_unreadable_chain_is_undetermined_and_never_a_denial() {
        // The rule this type exists for. "You do not own this" and "the chain would not
        // answer" are different facts and only one is about the holder — reporting the
        // second as the first sends somebody to buy a token they already have.
        let d = authorise(
            &Fixed(Ownership::Unknown { why: "rpc timeout".into() }),
            &ent(),
            &Holder("0x1".into()),
            A,
        );
        match &d {
            Decision::Undetermined { why } => assert_eq!(why, "rpc timeout"),
            other => panic!("expected Undetermined, got {other:?}"),
        }
        assert!(!d.permits(), "but it still fails closed");
        assert!(d.headline().contains("not a denial"));
    }

    #[test]
    fn the_request_shape_is_checked_before_ownership_is_consulted() {
        // Consulting the chain first would leak, by timing and by message, whether an
        // arbitrary address holds a token — for a request that could never be granted.
        struct Exploding;
        impl OwnershipOracle for Exploding {
            fn holds(&self, _t: &TokenRef, _h: &Holder) -> Ownership {
                panic!("ownership must not be consulted for a malformed request");
            }
        }
        let d = authorise(&Exploding, &ent(), &Holder("0x1".into()), "not-a-digest");
        assert_eq!(d, Decision::Denied { reason: DenialReason::MalformedRequest });
    }

    #[test]
    fn a_digest_is_exact_lowercase_hex_of_the_right_length() {
        assert!(is_digest(A));
        assert!(!is_digest(&A[..63]), "short");
        assert!(!is_digest(&A.to_uppercase()), "case-insensitive matching is a prefix game");
        assert!(!is_digest("g".repeat(64).as_str()), "not hex");
        assert!(!is_digest(""));
    }

    #[test]
    fn an_entitlement_is_read_out_of_the_token_metadata() {
        // Derived from the artefact rather than recorded beside it, so the two cannot drift.
        let meta = serde_json::json!({
            "name": "Omni world · x",
            "scema": { "world_commitment": A, "observed_at": 1, "schema": "scema.world/1" }
        });
        let e = Entitlement::from_metadata(token(), &meta).expect("commitment present");
        assert_eq!(e.world_commitment, A);
    }

    #[test]
    fn metadata_without_a_commitment_entitles_nobody_to_anything() {
        // `None`, never a default. An empty digest would either match nothing or — far worse
        // — match a record whose own commitment field was also empty.
        for meta in [
            serde_json::json!({}),
            serde_json::json!({ "scema": {} }),
            serde_json::json!({ "scema": { "world_commitment": "" } }),
            serde_json::json!({ "scema": { "world_commitment": "short" } }),
        ] {
            assert!(Entitlement::from_metadata(token(), &meta).is_none(), "{meta}");
        }
    }

    #[test]
    fn every_denial_says_something_the_requester_can_act_on() {
        for r in [
            DenialReason::NotAHolder,
            DenialReason::WrongWorld { entitled_to: A.into(), requested: B.into() },
            DenialReason::MalformedRequest,
            DenialReason::Unproven { detail: "expired".into() },
        ] {
            assert!(!r.explain().is_empty());
            assert!(r.explain().len() > 20, "a reason nobody can act on is not a reason");
        }
    }
}
