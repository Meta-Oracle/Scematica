//! Proving control of an address, without custody and without replay.
//!
//! Ownership says a token sits at an address. It says nothing about whether the person asking
//! *controls* that address — addresses are public, so a request naming one is a claim, not
//! evidence. The standard remedy is a challenge the holder signs.
//!
//! **Signature verification is deliberately not here.** It needs secp256k1 or ed25519
//! depending on the chain, and this crate has no crypto dependency for the same reason it has
//! no chain client: everything that decides *meaning* stays testable without one. What is
//! here is the part that is easy to get wrong and does not need crypto — issuing, expiring
//! and binding a challenge so that a valid signature over the wrong thing is still refused.
//!
//! The three failures this guards, in the order they are usually shipped:
//!
//! 1. **No expiry.** A signature harvested once works forever.
//! 2. **No binding to the request.** A signature proving control is replayed to authorise a
//!    *different* world than the one it was collected for.
//! 3. **Clock trust.** `issued_at` supplied by the requester lets them mint a challenge that
//!    never expires. The verifier supplies the time; the challenge only carries it.

use serde::{Deserialize, Serialize};

/// How long a challenge stays valid, in seconds.
///
/// Short on purpose. The only thing a longer window buys is convenience for a holder who
/// cannot sign within two minutes, and the thing it costs is the window in which a leaked
/// signature is useful.
pub const TTL_SECS: u64 = 120;

/// A nonce the holder must sign, bound to exactly what it authorises.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Challenge {
    /// Random, single-use. Supplied by the verifier, never by the requester.
    pub nonce: String,
    /// Unix seconds, stamped by the verifier.
    pub issued_at: u64,
    /// The world this challenge authorises reading. Binding it here is what stops a
    /// signature collected for one world being replayed for another.
    pub world_commitment: String,
    /// The address the challenge was issued to.
    pub holder: String,
}

/// Why a challenge did not hold up.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ChallengeError {
    Expired { age_secs: u64 },
    /// Issued in the future. Not pedantry: it is what a forged `issued_at` looks like, and
    /// silently accepting it makes the expiry check ornamental.
    NotYetValid,
    WrongWorld,
    WrongHolder,
    EmptyNonce,
}

impl ChallengeError {
    pub fn explain(&self) -> String {
        match self {
            ChallengeError::Expired { age_secs } => {
                format!("the challenge is {age_secs}s old and expires after {TTL_SECS}s")
            }
            ChallengeError::NotYetValid => {
                "the challenge is stamped in the future, which a valid one never is".into()
            }
            ChallengeError::WrongWorld => {
                "this challenge authorises a different world than the one requested".into()
            }
            ChallengeError::WrongHolder => {
                "this challenge was issued to a different address".into()
            }
            ChallengeError::EmptyNonce => "a challenge with no nonce is not a challenge".into(),
        }
    }
}

impl Challenge {
    /// Issue a challenge. `now` and `nonce` come from the verifier — never from the request.
    pub fn issue(nonce: impl Into<String>, holder: impl Into<String>, world: impl Into<String>, now: u64) -> Self {
        Challenge {
            nonce: nonce.into(),
            issued_at: now,
            world_commitment: world.into(),
            holder: holder.into(),
        }
    }

    /// The exact bytes the holder signs.
    ///
    /// Every field that scopes the grant appears here. A signature is only evidence about the
    /// string it covers, so anything omitted from this is unbound and replayable — which is
    /// the whole of failure mode 2 above.
    pub fn message(&self) -> String {
        format!(
            "scema-entitlement/1\nnonce={}\nissued_at={}\nholder={}\nworld={}",
            self.nonce, self.issued_at, self.holder, self.world_commitment
        )
    }

    /// Check everything that does not need crypto.
    ///
    /// A caller must *also* verify a signature over [`Self::message`]. This returning `Ok`
    /// means the challenge is fresh and covers what is being asked for; it means nothing at
    /// all about who signed it.
    pub fn validate(&self, holder: &str, world: &str, now: u64) -> Result<(), ChallengeError> {
        if self.nonce.trim().is_empty() {
            return Err(ChallengeError::EmptyNonce);
        }
        if now < self.issued_at {
            return Err(ChallengeError::NotYetValid);
        }
        let age = now - self.issued_at;
        if age > TTL_SECS {
            return Err(ChallengeError::Expired { age_secs: age });
        }
        if self.holder != holder {
            return Err(ChallengeError::WrongHolder);
        }
        if self.world_commitment != world {
            return Err(ChallengeError::WrongWorld);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const W: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const V: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

    fn ch(now: u64) -> Challenge {
        Challenge::issue("n1", "0xabc", W, now)
    }

    #[test]
    fn a_fresh_challenge_for_the_right_holder_and_world_validates() {
        assert_eq!(ch(1000).validate("0xabc", W, 1000 + 30), Ok(()));
    }

    #[test]
    fn a_stale_challenge_expires() {
        // Without this, a signature harvested once works forever.
        let e = ch(1000).validate("0xabc", W, 1000 + TTL_SECS + 1);
        assert_eq!(e, Err(ChallengeError::Expired { age_secs: TTL_SECS + 1 }));
    }

    #[test]
    fn the_boundary_is_inclusive_so_a_challenge_is_valid_for_its_full_ttl() {
        assert_eq!(ch(1000).validate("0xabc", W, 1000 + TTL_SECS), Ok(()));
    }

    #[test]
    fn a_challenge_stamped_in_the_future_is_refused() {
        // What a forged `issued_at` looks like. Accepting it makes the expiry ornamental,
        // because the requester could simply stamp it far enough ahead.
        assert_eq!(ch(2000).validate("0xabc", W, 1000), Err(ChallengeError::NotYetValid));
    }

    #[test]
    fn a_signature_for_one_world_does_not_authorise_another() {
        // Failure mode 2: control is genuinely proven, for something else.
        assert_eq!(ch(1000).validate("0xabc", V, 1000), Err(ChallengeError::WrongWorld));
    }

    #[test]
    fn a_challenge_issued_to_one_address_does_not_serve_another() {
        assert_eq!(ch(1000).validate("0xdef", W, 1000), Err(ChallengeError::WrongHolder));
    }

    #[test]
    fn an_empty_nonce_is_not_a_challenge() {
        let c = Challenge::issue("   ", "0xabc", W, 1000);
        assert_eq!(c.validate("0xabc", W, 1000), Err(ChallengeError::EmptyNonce));
    }

    #[test]
    fn everything_that_scopes_the_grant_appears_in_the_signed_message() {
        // A signature is evidence only about the string it covers. Anything missing here is
        // unbound, and unbound means replayable.
        let c = ch(1000);
        let m = c.message();
        for part in [&c.nonce, &c.holder, &c.world_commitment] {
            assert!(m.contains(part.as_str()), "{part} is not bound into {m}");
        }
        assert!(m.contains("1000"), "issued_at must be bound or expiry is unsigned");
        assert!(m.starts_with("scema-entitlement/1"), "domain separation");
    }

    #[test]
    fn two_challenges_differing_anywhere_produce_different_messages() {
        let base = ch(1000);
        for other in [
            Challenge::issue("n2", "0xabc", W, 1000),
            Challenge::issue("n1", "0xdef", W, 1000),
            Challenge::issue("n1", "0xabc", V, 1000),
            Challenge::issue("n1", "0xabc", W, 1001),
        ] {
            assert_ne!(base.message(), other.message());
        }
    }

    #[test]
    fn validating_says_nothing_about_who_signed() {
        // Stated as a test because the name `validate` invites the opposite reading, and a
        // caller that skipped signature verification would still see `Ok(())` here.
        let c = ch(1000);
        assert_eq!(c.validate("0xabc", W, 1000), Ok(()));
        // No key, no signature, no crypto — by construction.
    }
}
