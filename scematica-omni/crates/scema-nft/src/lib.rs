//! Scematica Omni — a world, drawn.
//!
//! Takes a `WorldState` (or a sealed decision record containing one) and produces a
//! self-contained SVG plate plus ERC-721-shaped token metadata. Deterministic: the same
//! world produces the same bytes, in this crate and in `web/lib/omni/nft.ts`, which is what
//! makes the picture a *derivative of the record* rather than an illustration of it.
//!
//! ```no_run
//! # use scema_nft::{load, render_svg};
//! let value: serde_json::Value = serde_json::from_str("{}").unwrap();
//! let source = load(&value).unwrap();
//! let svg = render_svg(&source.world, &source.digest);
//! ```
//!
//! ## Why an image at all
//!
//! A decision record is a JSON file with six digests in it, and the honest thing about it
//! is also the thing that stops anybody reading it: the interesting content is the shape of
//! what was *not* known, and that is invisible in a column of numbers. The plate is the
//! same information arranged so ignorance has a size — a perforated ring, a dashed sweep, a
//! hollow cap — and it fits in the one surface people actually look at.
//!
//! That is a real risk, and it is the reason this crate is as strict as it is. A picture is
//! far more persuasive than a table and far less precise, so every rule the rest of the
//! workspace enforces in text is enforced here in geometry, and the ones that cannot be are
//! written into the metadata description instead of being left implicit.
//!
//! ## What the plate proves, and the two things it does not
//!
//! The commitment printed on the plate is the world's canonical digest — the same
//! `commitment.world` a decision record carries, computed by the same code. So:
//!
//! - It **does** bind this picture to that exact world file. Change a byte of the world and
//!   the digest on the plate no longer matches it.
//! - It does **not** prove the world was as described. Provenance carries that, which is
//!   why the plate draws provenance rather than hiding it.
//! - It does **not** prove this is the only plate for that world. Tamper-evident, not
//!   tamper-proof, until the root is anchored somewhere the author does not control.
//!
//! All three are stated in the token description, not merely here, because a comment
//! protects nobody holding the token.
//!
//! ## Determinism, and what it forbids
//!
//! Byte-identical output across Rust and a browser is a hard requirement, not a nicety —
//! see `geom` for the arithmetic and `check:omni` for the test. In practice it means: no
//! trigonometry (integer sine table), no locale, **no clock**, and no randomness. There is
//! deliberately no "minted at" field anywhere: a timestamp taken at render time would make
//! every regeneration a different token, which is precisely the property this crate exists
//! to avoid. The only time on the plate is `WorldState::observed_at`, which the observer
//! measured.

use anyhow::{bail, Result};
use scema_verify::canonical::digest;
use scema_world::WorldState;
use serde_json::Value;

pub mod fixtures;
pub mod fractal;
pub mod geom;
pub mod metadata;
pub mod palette;
pub mod plate;
pub mod raster;

/// What a loaded JSON document turned out to be.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceKind {
    /// A bare `WorldState`, as `scema observe` prints and the four producers emit.
    World,
    /// A sealed decision record. The world is taken from it, and so is the commitment.
    Record,
}

/// A world to draw, and the commitment that identifies it.
#[derive(Clone, Debug)]
pub struct Source {
    pub world: WorldState,
    /// Hex digest of the canonical encoding of `world`.
    pub digest: String,
    pub kind: SourceKind,
}

/// The canonical commitment for a world, as hex.
///
/// The same function a decision record uses for `commitment.world`, reached through
/// `scema-verify` rather than reimplemented — a second hasher would eventually disagree
/// with the first, and the failure would be a plate that appears to belong to a different
/// world than the record it came from.
pub fn world_digest(world: &WorldState) -> String {
    digest(world).to_hex()
}

/// Read either a bare world or a sealed record.
///
/// A record is detected by its `commitment`, and its **stored** `commitment.world` is used
/// rather than a freshly computed one. That is deliberate: if the record has been edited,
/// the plate then carries a digest that does not match its own world, and `scema verify`
/// on the record says which field moved. Recomputing here would quietly paper over exactly
/// the tampering the commitment exists to expose.
pub fn load(value: &Value) -> Result<Source> {
    if value.get("commitment").is_some() && value.get("world").is_some() {
        let world: WorldState = serde_json::from_value(value["world"].clone())?;
        let digest = value["commitment"]["world"]
            .as_str()
            .map(|s| s.to_string())
            .unwrap_or_else(|| world_digest(&world));
        return Ok(Source { world, digest, kind: SourceKind::Record });
    }

    if value.get("entity").is_some() && value.get("observer").is_some() {
        let world: WorldState = serde_json::from_value(value.clone())?;
        let digest = world_digest(&world);
        return Ok(Source { world, digest, kind: SourceKind::World });
    }

    bail!(
        "not a world or a decision record: expected either `observer` + `entity` \
(a WorldState, as `scema observe` prints) or `world` + `commitment` (a sealed record)"
    )
}

/// Draw a world.
///
/// The fractal growth is the default rendering. The plate in [`plate`] is still there and
/// still tested — it is the instrument reading of the same data, reachable with
/// `scema nft --plate` — but the growth is what the world *is*, and the plate is what it
/// measures.
pub fn render_svg(world: &WorldState, digest_hex: &str) -> String {
    fractal::render(world, digest_hex)
}

/// Token metadata, with the plate inlined as a `data:` URI unless `image` overrides it.
pub fn render_metadata(
    world: &WorldState,
    svg: &str,
    digest_hex: &str,
    image: Option<&str>,
) -> Value {
    metadata::metadata(world, svg, digest_hex, image)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures::{parity_world, rich_world};

    #[test]
    fn a_bare_world_is_recognised_and_committed() {
        let w = rich_world();
        let v = serde_json::to_value(&w).unwrap();
        let s = load(&v).unwrap();
        assert_eq!(s.kind, SourceKind::World);
        assert_eq!(s.digest, world_digest(&w));
        assert_eq!(s.digest.len(), 64);
    }

    #[test]
    fn a_record_contributes_its_stored_commitment_not_a_fresh_one() {
        // The stored digest is used verbatim so an edited record produces a plate whose
        // commitment does not match its own world — which is the tamper signal, visible.
        let w = rich_world();
        let v = serde_json::json!({
            "world": serde_json::to_value(&w).unwrap(),
            "commitment": { "world": "0000stored0000" },
        });
        let s = load(&v).unwrap();
        assert_eq!(s.kind, SourceKind::Record);
        assert_eq!(s.digest, "0000stored0000");
        assert_ne!(s.digest, world_digest(&w));
    }

    #[test]
    fn anything_else_is_refused_with_a_message_naming_both_shapes() {
        let e = load(&serde_json::json!({ "hello": 1 })).unwrap_err().to_string();
        assert!(e.contains("WorldState"));
        assert!(e.contains("record"));
    }

    #[test]
    fn the_same_world_renders_to_the_same_bytes() {
        let w = parity_world();
        let d = world_digest(&w);
        assert_eq!(render_svg(&w, &d), render_svg(&w, &d));
    }

    #[test]
    fn a_changed_world_changes_the_commitment_on_the_plate() {
        let a = parity_world();
        let mut b = parity_world();
        b.blind_spots.push("one more".into());
        assert_ne!(world_digest(&a), world_digest(&b));
        assert_ne!(render_svg(&a, &world_digest(&a)), render_svg(&b, &world_digest(&b)));
    }

    #[test]
    fn nothing_in_the_output_depends_on_the_clock() {
        // No "minted at". A timestamp taken at render time would make every regeneration a
        // different token, which defeats the purpose of deriving the image from the record.
        let w = parity_world();
        let d = world_digest(&w);
        let svg = render_svg(&w, &d);
        let meta = render_metadata(&w, &svg, &d, None).to_string();
        for banned in ["minted", "generated_at", "rendered_at", "createdAt"] {
            assert!(!meta.contains(banned), "metadata must not contain {banned}");
        }
    }
}
