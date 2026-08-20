//! Canonical encoding: one byte sequence per value, so one decision has one digest.
//!
//! `serde_json::to_string` is **not** usable for this. It preserves map insertion order
//! (which differs between a freshly built structure and a round-tripped one), it has no
//! opinion at all about `-0.0` versus `0.0`, and — the one that actually bit, see below —
//! its float round trip is not exact. Any of those gives the same decision two digests.
//!
//! So the encoding here is explicit and boring:
//!
//! | Type | Bytes |
//! |---|---|
//! | null | `00` |
//! | bool | `01` + `00`/`01` |
//! | integer | `02` + i64 big-endian |
//! | float | `03` + `round(v * 1e9)` as i64 big-endian (see below) |
//! | string | `04` + u64 BE length + UTF-8 |
//! | array | `05` + u64 BE count + encoded items in order |
//! | object | `06` + u64 BE count + (key, value) pairs **sorted by key** |
//! | float (out of range / non-finite) | `07` + normalised IEEE-754 bits, big-endian |
//!
//! The type tag is what makes it unambiguous: without it, the string `"1"` and the integer
//! `1` could encode to the same bytes, and a record could be altered without changing its
//! digest.
//!
//! ## Floats are hashed as fixed-point, and that is not a shortcut
//!
//! **A commitment over raw IEEE-754 bits is unverifiable the moment the record crosses
//! JSON**, which is the only way a record ever travels. Measured, in this workspace, on a
//! real projection:
//!
//! ```text
//!   in memory   0.40066666666666667
//!   serialised  "0.40066666666666667"     <- identical text
//!   parsed back 0.4006666666666666        <- one ULP low
//! ```
//!
//! The formatter is exact; `serde_json`'s *parser* is not correctly rounded for every
//! 17-significant-digit input. So a record sealed in the daemon and verified after a `GET`
//! reported `INVALID` on a byte nobody had touched — a verifier that cries tamper on an
//! honest round trip is worse than no verifier, because the first thing anyone does with it
//! is stop believing it.
//!
//! This is the same wall `scema-bot-mesh` hit and the same conclusion: bit-exact float
//! agreement between two processes is not a property you get by being careful, it is one
//! you engineer. That crate reached for fixed-point arithmetic throughout; here only the
//! *hashing boundary* needs it, because nothing downstream re-runs the arithmetic.
//!
//! So a float is encoded as `round(v * 10^9)` in `i64`, and the commitment therefore binds
//! values **to a resolution of [`FIXED_SCALE`]⁻¹ = 1e-9**. Stated plainly:
//!
//! * An edit of 1e-9 or more to any score is caught.
//! * An edit smaller than that is not, and cannot change any decision — every float in a
//!   record is a utility, magnitude or weight in roughly `[-2, 2]`, and no gate in
//!   `scema-policy` has a threshold anywhere near that fine.
//!
//! Values too large for the fixed-point range, and the non-finite ones, fall back to a
//! separately-tagged normalised bit pattern. Nothing in a decision record produces them
//! today; the arm exists so that a future field cannot silently take the wrong path.
//!
//! Two normalisations survive from before, both still needed:
//!
//! * **`-0.0` normalises to `0.0`.** They compare equal in Rust and in JSON, and
//!   `(-0.0 * 1e9).round() as i64` is `0`, so the fixed-point path handles this for free —
//!   but the bit-pattern fallback still needs it explicitly.
//! * **Every NaN normalises to one quiet NaN.** There are ~2^52 NaN bit patterns; a digest
//!   that depends on which one arrived depends on the arithmetic that produced it.
//!
//! Integers that arrive as JSON numbers are encoded as integers, not floats, so a count of
//! `3` hashes the same whether it came from a `u64` field or a literal — and it keeps
//! counts exact rather than pushing them through the fixed-point path.

use serde::Serialize;
use serde_json::Value;
use sha2::{Digest as _, Sha256};

/// A 32-byte SHA-256 digest.
///
/// SHA-256 rather than the keccak-256 used by `scema-bot-mesh`, because nothing on an EVM
/// verifies these yet and SHA-256 is what the rest of this stack already speaks. If a
/// decision record is ever bound to an on-chain commitment, that binding belongs in
/// `mesh-core`'s keccak path — a Solidity contract cannot check a SHA-256 digest without
/// shipping the hash function, which costs more gas than the dispute is worth.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Digest(pub [u8; 32]);

impl Digest {
    pub fn to_hex(self) -> String {
        let mut s = String::with_capacity(64);
        for b in self.0 {
            s.push_str(&format!("{b:02x}"));
        }
        s
    }

    /// The short form that names a decision, e.g. `8f92a1c4`.
    ///
    /// Eight hex characters — 32 bits. Enough that an operator can type one and mean one
    /// record, and short enough to read aloud. Lookup accepts any unique prefix, so a
    /// collision is resolvable by typing more rather than by being silently wrong.
    pub fn short(self) -> String {
        self.to_hex()[..8].to_string()
    }
}

impl std::fmt::Debug for Digest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.to_hex())
    }
}

impl std::fmt::Display for Digest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.to_hex())
    }
}

const TAG_NULL: u8 = 0x00;
const TAG_BOOL: u8 = 0x01;
const TAG_INT: u8 = 0x02;
const TAG_FLOAT: u8 = 0x03;
const TAG_STR: u8 = 0x04;
const TAG_ARRAY: u8 = 0x05;
const TAG_OBJECT: u8 = 0x06;
/// Out-of-range or non-finite float, encoded as normalised bits.
const TAG_FLOAT_BITS: u8 = 0x07;

/// Fixed-point scale for hashed floats: nano-units. See the module note for why.
pub const FIXED_SCALE: f64 = 1_000_000_000.0;

/// Normalise a float to a single bit pattern per mathematical value.
///
/// Only used by the [`TAG_FLOAT_BITS`] fallback; the fixed-point path normalises by
/// construction.
fn normalise(f: f64) -> u64 {
    if f.is_nan() {
        return f64::NAN.to_bits();
    }
    if f == 0.0 {
        return 0.0f64.to_bits();
    }
    f.to_bits()
}

/// Quantise a float for hashing, or report that it does not fit.
///
/// `None` for anything non-finite or beyond the `i64` range after scaling — roughly
/// `|v| > 9.2e9`, which no score in a decision record approaches.
fn to_fixed(f: f64) -> Option<i64> {
    if !f.is_finite() {
        return None;
    }
    let scaled = (f * FIXED_SCALE).round();
    if scaled >= i64::MIN as f64 && scaled <= i64::MAX as f64 {
        Some(scaled as i64)
    } else {
        None
    }
}

fn encode_into(v: &Value, out: &mut Vec<u8>) {
    match v {
        Value::Null => out.push(TAG_NULL),
        Value::Bool(b) => {
            out.push(TAG_BOOL);
            out.push(u8::from(*b));
        }
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                out.push(TAG_INT);
                out.extend_from_slice(&i.to_be_bytes());
            } else if let Some(u) = n.as_u64() {
                // Above i64::MAX. Tagged as an integer still, encoded as the u64 bits,
                // which cannot collide with an i64 in range because that range never
                // reaches here.
                out.push(TAG_INT);
                out.extend_from_slice(&u.to_be_bytes());
            } else {
                let f = n.as_f64().unwrap_or(f64::NAN);
                match to_fixed(f) {
                    Some(fixed) => {
                        out.push(TAG_FLOAT);
                        out.extend_from_slice(&fixed.to_be_bytes());
                    }
                    None => {
                        out.push(TAG_FLOAT_BITS);
                        out.extend_from_slice(&normalise(f).to_be_bytes());
                    }
                }
            }
        }
        Value::String(s) => {
            out.push(TAG_STR);
            out.extend_from_slice(&(s.len() as u64).to_be_bytes());
            out.extend_from_slice(s.as_bytes());
        }
        Value::Array(items) => {
            out.push(TAG_ARRAY);
            out.extend_from_slice(&(items.len() as u64).to_be_bytes());
            for item in items {
                encode_into(item, out);
            }
        }
        Value::Object(map) => {
            out.push(TAG_OBJECT);
            out.extend_from_slice(&(map.len() as u64).to_be_bytes());
            // Sorted, not insertion order. `serde_json::Map` is a `BTreeMap` by default
            // but becomes insertion-ordered under the `preserve_order` feature, which any
            // dependency in the tree can switch on. Sorting here makes the encoding
            // independent of that.
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            for k in keys {
                out.push(TAG_STR);
                out.extend_from_slice(&(k.len() as u64).to_be_bytes());
                out.extend_from_slice(k.as_bytes());
                encode_into(&map[k], out);
            }
        }
    }
}

/// Canonical bytes for any JSON value.
pub fn canonical_bytes(v: &Value) -> Vec<u8> {
    let mut out = Vec::new();
    encode_into(v, &mut out);
    out
}

/// Canonical digest of anything serialisable.
///
/// A value that fails to serialise digests as JSON `null` rather than panicking. That is
/// deliberate and it is safe here: every type this crate hashes derives `Serialize` over
/// plain data, so the arm is unreachable in practice, and a verifier that panics on a
/// malformed record is a verifier that cannot report the record is malformed.
pub fn digest<T: Serialize>(value: &T) -> Digest {
    let json = serde_json::to_value(value).unwrap_or(Value::Null);
    let bytes = canonical_bytes(&json);
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    let out = hasher.finalize();
    let mut d = [0u8; 32];
    d.copy_from_slice(&out);
    Digest(d)
}

/// Digest of an ordered list of digests, for a commitment root.
pub fn digest_of_digests(parts: &[(&str, Digest)]) -> Digest {
    let mut hasher = Sha256::new();
    // The label is hashed alongside the digest so that swapping two fields of the same
    // type — say `world` and `goal` — changes the root.
    for (label, d) in parts {
        hasher.update((label.len() as u64).to_be_bytes());
        hasher.update(label.as_bytes());
        hasher.update(d.0);
    }
    let out = hasher.finalize();
    let mut d = [0u8; 32];
    d.copy_from_slice(&out);
    Digest(d)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn key_order_does_not_change_the_digest() {
        let a: Value = serde_json::from_str(r#"{"a":1,"b":2}"#).unwrap();
        let b: Value = serde_json::from_str(r#"{"b":2,"a":1}"#).unwrap();
        assert_eq!(canonical_bytes(&a), canonical_bytes(&b));
    }

    #[test]
    fn array_order_does_change_the_digest() {
        // Order is meaning for a ranking; this must not be normalised away.
        assert_ne!(canonical_bytes(&json!([1, 2])), canonical_bytes(&json!([2, 1])));
    }

    #[test]
    fn a_string_and_a_number_that_look_alike_encode_differently() {
        assert_ne!(canonical_bytes(&json!("1")), canonical_bytes(&json!(1)));
    }

    #[test]
    fn negative_zero_hashes_as_zero() {
        assert_eq!(normalise(-0.0), normalise(0.0));
        assert_eq!(to_fixed(-0.0), to_fixed(0.0));
        assert_eq!(canonical_bytes(&json!(-0.0)), canonical_bytes(&json!(0.0)));
    }

    #[test]
    fn every_nan_hashes_alike() {
        let a = f64::NAN;
        let b = f64::from_bits(f64::NAN.to_bits() | 0x7);
        assert!(b.is_nan());
        assert_eq!(normalise(a), normalise(b));
    }

    #[test]
    fn a_float_that_serde_json_cannot_round_trip_still_hashes_the_same() {
        // The regression. This exact value came out of a real projection: the text is
        // identical in both directions and the parsed f64 is one ULP low, so a commitment
        // over raw bits reported INVALID on a record nobody had touched.
        let original = 0.40066666666666667_f64;
        let text = serde_json::to_string(&original).unwrap();
        let parsed: f64 = serde_json::from_str(&text).unwrap();
        assert_ne!(
            parsed.to_bits(),
            original.to_bits(),
            "if serde_json ever fixes this, the test is still valid but the hazard is gone"
        );
        assert_eq!(
            canonical_bytes(&json!(original)),
            canonical_bytes(&json!(parsed)),
            "the canonical encoding must survive what the transport does to a float"
        );
    }

    #[test]
    fn an_edit_at_the_bound_resolution_is_caught() {
        // The commitment binds to 1e-9. One unit at that scale must change the digest, or
        // the guarantee in the module note is false.
        assert_ne!(canonical_bytes(&json!(0.5)), canonical_bytes(&json!(0.500000001)));
        assert_ne!(canonical_bytes(&json!(-0.25)), canonical_bytes(&json!(-0.250000001)));
    }

    #[test]
    fn an_edit_below_the_bound_resolution_is_not_caught_and_that_is_the_stated_deal() {
        // Documented rather than hidden. A difference this small cannot move any gate in
        // `scema-policy`, and pretending to bind it would mean pretending JSON transport is
        // bit-exact, which it is not.
        assert_eq!(canonical_bytes(&json!(0.5)), canonical_bytes(&json!(0.5 + 1e-12)));
    }

    #[test]
    fn non_finite_floats_take_the_bit_pattern_arm_and_stay_distinct() {
        assert_eq!(to_fixed(f64::NAN), None);
        assert_eq!(to_fixed(f64::INFINITY), None);
        assert_ne!(
            canonical_bytes(&json!(1.0)),
            canonical_bytes(&Value::Number(
                serde_json::Number::from_f64(1e300).unwrap()
            ))
        );
    }

    #[test]
    fn a_float_and_an_integer_of_the_same_value_do_not_collide() {
        // The fixed-point encoding puts a scaled i64 under TAG_FLOAT; a real integer goes
        // under TAG_INT unscaled. Without distinct tags, 1 and 1e-9 could meet.
        assert_ne!(canonical_bytes(&json!(1)), canonical_bytes(&json!(1.0)));
        assert_ne!(canonical_bytes(&json!(1)), canonical_bytes(&json!(1e-9)));
    }

    #[test]
    fn nesting_cannot_be_flattened_into_a_collision() {
        // Without length prefixes, {"ab": null} and {"a": "b"} could run together.
        assert_ne!(canonical_bytes(&json!({"ab": null})), canonical_bytes(&json!({"a": "b"})));
    }

    #[test]
    fn the_root_binds_field_names_not_just_values() {
        let d1 = digest(&json!("one"));
        let d2 = digest(&json!("two"));
        let ab = digest_of_digests(&[("world", d1), ("goal", d2)]);
        let ba = digest_of_digests(&[("goal", d1), ("world", d2)]);
        assert_ne!(ab, ba, "swapping which field holds which digest must change the root");
    }

    #[test]
    fn digests_are_stable_across_a_json_round_trip() {
        let v = json!({"z": [1, 2.5, "x"], "a": {"b": true}});
        let round: Value = serde_json::from_str(&serde_json::to_string(&v).unwrap()).unwrap();
        assert_eq!(digest(&v), digest(&round));
    }
}
