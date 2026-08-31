//! Token metadata for a plate.
//!
//! ERC-721-shaped, because that is what a wallet or a marketplace will try to read, with
//! the image inlined as a `data:` URI so the token is complete on its own. A plate whose
//! picture lives at a URL somebody has to keep paying for is not a record of anything.
//!
//! ## The rule that governs every attribute here
//!
//! **No attribute may be a number nobody measured, and none may be a score.**
//!
//! Both halves matter and they fail differently.
//!
//! The first is the em-dash rule again, in the one place it is easiest to lose: a trait
//! list is a flat map of names to values, it is rendered by software nobody here controls,
//! and `0` is what a missing field turns into on the way. So an unbounded extent is the
//! *string* `unbounded`, not `0` and not `1`; a world with no objects has legibility `∅`,
//! not `0`. A marketplace showing "Legibility: 0" for a world nobody looked at is a
//! fabricated observation with a nice card around it.
//!
//! The second is about what a trait list invites. Every NFT convention in existence wants a
//! rarity roll, a tier, a score out of a hundred — and it would be trivial to compute one
//! from these numbers. There is none, and there will not be one. A rank invented here would
//! be a number of exactly the right shape with nothing behind it, laundered through a
//! signed artefact into somebody's wallet, and the entire point of deriving the plate from
//! a `WorldState` is that every mark on it traces back to something an observer counted.

use scema_world::{Provenance, WorldState};
use serde_json::{json, Value};

use crate::plate::fixed2;

/// Base64, standard alphabet with padding.
///
/// Hand-rolled rather than pulled in as a dependency, and the port hand-rolls the same
/// thing. `btoa` in a browser operates on a binary string and mangles anything above U+00FF,
/// so a label with an accent in it would encode differently there — which is exactly the
/// kind of divergence that turns a reproducible artefact into two artefacts. Both sides
/// encode the UTF-8 bytes explicitly.
pub fn base64(bytes: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = *chunk.get(1).unwrap_or(&0) as u32;
        let b2 = *chunk.get(2).unwrap_or(&0) as u32;
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(A[(n >> 18) as usize & 63] as char);
        out.push(A[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 { A[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if chunk.len() > 2 { A[n as usize & 63] as char } else { '=' });
    }
    out
}

/// The SVG as a self-contained `data:` URI.
pub fn data_uri(svg: &str) -> String {
    format!("data:image/svg+xml;base64,{}", base64(svg.as_bytes()))
}

/// The PNG as a self-contained `data:` URI.
///
/// The SVG is the better default and stays the default: it is two orders of magnitude
/// smaller, and it is *the* drawing rather than a sampling of it. But an SVG `image` is
/// unusable in a surprising number of places — several marketplaces, most previews, and
/// anything that composites into a bitmap — and "your token renders everywhere except where
/// people look at it" is not a property worth defending on aesthetic grounds.
///
/// The cost is stated rather than hidden: base64 inflates by 4/3, so a 1024px raster is
/// roughly 4 MB of metadata. `scema nft --image-format png` prints the resulting size and
/// `--png-size` is the dial. For anything minted at scale the right answer is still a hosted
/// URL through `--image`.
pub fn data_uri_png(bytes: &[u8]) -> String {
    format!("data:image/png;base64,{}", base64(bytes))
}

/// Build the token metadata.
///
/// `image` overrides the inlined data URI — for a deployment that pins the SVG somewhere
/// content-addressed and would rather the token point at it.
pub fn metadata(
    world: &WorldState,
    svg: &str,
    digest_hex: &str,
    image: Option<&str>,
) -> Value {
    let (live, stale, absent, simulated) = counts(world);
    let counted = world.signals.iter().filter(|s| s.measured).count();
    let estimated = world.signals.len() - counted;

    // Legibility: a string when there was nothing to read, a number when there was.
    // `WorldState::legibility` returns 0.0 for both cases and cannot tell them apart.
    let legibility: Value = if world.objects.is_empty() {
        json!("∅")
    } else {
        json!(fixed2(world.legibility()))
    };

    // Extent: never a fraction when the denominator is unknown.
    let extent: Value = match world.extent.total {
        Some(t) => json!(format!("{}/{}", world.extent.observed, t)),
        None => json!(format!("{} · unbounded", world.extent.observed)),
    };

    let attributes = json!([
        { "trait_type": "Domain", "value": world.domain.as_str() },
        { "trait_type": "Entity kind", "value": world.entity.kind.as_str() },
        { "trait_type": "Observer", "value": world.observer },
        { "trait_type": "Extent", "value": extent },
        { "trait_type": "Legibility", "value": legibility },
        { "trait_type": "Objects", "value": world.objects.len() },
        { "trait_type": "Signals counted", "value": counted },
        { "trait_type": "Signals estimated", "value": estimated },
        { "trait_type": "Blind spots", "value": world.blind_spots.len() },
        { "trait_type": "Live", "value": live },
        { "trait_type": "Stale", "value": stale },
        { "trait_type": "Absent", "value": absent },
        { "trait_type": "Simulated", "value": simulated },
        { "trait_type": "World schema", "value": world.schema.clone().unwrap_or_else(|| "undeclared".into()) },
    ]);

    json!({
        "name": format!("Omni world · {}", world.entity.label),
        "description": description(world),
        "image": image.map(|s| s.to_string()).unwrap_or_else(|| data_uri(svg)),
        "external_url": world.entity.locator,
        "attributes": attributes,
        // Not an attribute: it is the identity of the thing, not a trait of it, and a
        // marketplace that renders traits as filter facets would offer to filter by it.
        "scema": {
            "world_commitment": digest_hex,
            "observed_at": world.observed_at,
            "schema": world.schema,
        },
    })
}

/// Prose that states what the plate does and does not prove.
///
/// The same three limits `/omni` renders twice and the Claude Code skill exists to keep a
/// model from eliding. A picture is more persuasive than a table, so the caveat travels
/// with it.
fn description(w: &WorldState) -> String {
    let mut d = format!(
        "A Scematica Omni world plate: the state of `{}` as one observer found it at unix {}, drawn to scale. \
Every mark is a measurement or the absence of one. Dashed means nobody measured it; a notch in the outer ring is something the observer could not read; a hollow cap is a magnitude that was estimated rather than counted.",
        w.entity.locator, w.observed_at
    );
    d.push_str(
        " The commitment binds this plate to that world file. It does not prove the world was as described \
— provenance carries that — and it does not prove this is the only plate for it.",
    );
    if !w.blind_spots.is_empty() {
        d.push_str(&format!(
            " {} blind spot(s) were reported and are drawn as notches.",
            w.blind_spots.len()
        ));
    }
    d
}

fn counts(w: &WorldState) -> (usize, usize, usize, usize) {
    let mut c = (0, 0, 0, 0);
    for o in &w.objects {
        match o.provenance {
            Provenance::Live { .. } => c.0 += 1,
            Provenance::Stale { .. } => c.1 += 1,
            Provenance::Absent => c.2 += 1,
            Provenance::Simulated => c.3 += 1,
        }
    }
    c
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures::{empty_world, parity_world, rich_world};

    #[test]
    fn a_png_data_uri_declares_png_and_round_trips_to_the_same_bytes() {
        // The whole point of embedding rather than linking is that the token carries the
        // image. If the base64 does not decode back to the exact file the CLI wrote, the
        // token and the artefact on disk are two different pictures with one commitment.
        let w = parity_world();
        let d = crate::world_digest(&w);
        let png = crate::fractal::render_png(&w, &d, 64);

        let uri = data_uri_png(&png);
        assert!(uri.starts_with("data:image/png;base64,"), "{}", &uri[..40]);

        // Decoded with an independent implementation rather than by inverting `base64`,
        // which would agree with its own bug.
        let body = uri.split_once(',').unwrap().1;
        let alphabet = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut bits = Vec::new();
        for c in body.bytes() {
            if c == b'=' {
                break;
            }
            let v = alphabet.iter().position(|a| *a == c).expect("alphabet") as u32;
            bits.push(v);
        }
        let mut out = Vec::new();
        for chunk in bits.chunks(4) {
            let mut n = 0u32;
            for (i, v) in chunk.iter().enumerate() {
                n |= v << (18 - 6 * i);
            }
            let take = chunk.len() * 6 / 8;
            for i in 0..take {
                out.push(((n >> (16 - 8 * i)) & 0xff) as u8);
            }
        }
        assert_eq!(out, png, "the embedded image is not the file that was rendered");
    }

    #[test]
    fn an_explicit_image_url_outranks_an_inlined_drawing() {
        // A hosted URL is what anything minted at scale should carry, and a flag the caller
        // passed explicitly must never lose to a default.
        let w = parity_world();
        let d = crate::world_digest(&w);
        let meta = metadata(&w, "<svg/>", &d, Some("ipfs://bafy.../plate.png"));
        assert_eq!(meta["image"], "ipfs://bafy.../plate.png");
    }

    #[test]
    fn the_default_image_is_still_the_inline_svg() {
        // Changing this silently would repoint every token anybody has already minted from
        // a self-contained drawing to something else.
        let w = parity_world();
        let d = crate::world_digest(&w);
        let meta = metadata(&w, "<svg/>", &d, None);
        assert!(
            meta["image"].as_str().unwrap().starts_with("data:image/svg+xml;base64,"),
            "{}",
            meta["image"]
        );
    }

    #[test]
    fn base64_matches_the_reference_vectors() {
        // RFC 4648 section 10. Pinned because the port implements this independently.
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foob"), "Zm9vYg==");
        assert_eq!(base64(b"fooba"), "Zm9vYmE=");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn base64_encodes_utf8_bytes_not_code_units() {
        // The `btoa` trap, pinned from this side too. A label with an accent must encode
        // from its UTF-8 bytes or the two runtimes produce different tokens.
        assert_eq!(base64("ä".as_bytes()), "w6Q=");
        assert_eq!(base64("∅".as_bytes()), "4oiF");
    }

    #[test]
    fn an_empty_world_reports_legibility_as_a_glyph_not_a_zero() {
        // The failure this whole file is arranged around: a marketplace rendering
        // "Legibility: 0" for a world nobody looked at.
        let m = metadata(&empty_world(), "<svg/>", "aa", None);
        let attrs = m["attributes"].as_array().unwrap();
        let leg = attrs.iter().find(|a| a["trait_type"] == "Legibility").unwrap();
        assert_eq!(leg["value"], "∅");
    }

    #[test]
    fn an_unbounded_extent_is_never_a_fraction() {
        let mut w = rich_world();
        w.extent = scema_world::Extent::partial(4, "cap");
        let m = metadata(&w, "<svg/>", "aa", None);
        let attrs = m["attributes"].as_array().unwrap();
        let e = attrs.iter().find(|a| a["trait_type"] == "Extent").unwrap();
        assert!(e["value"].as_str().unwrap().contains("unbounded"));
        assert!(!e["value"].as_str().unwrap().contains('/'));
    }

    #[test]
    fn there_is_no_score_no_rank_and_no_rarity() {
        // Guarding an absence, because the pressure to add one of these is permanent and
        // the reviewer who adds it will be doing something that looks helpful.
        let m = metadata(&parity_world(), "<svg/>", "aa", None);
        let text = serde_json::to_string(&m).unwrap().to_lowercase();
        for banned in ["rarity", "\"score\"", "\"rank\"", "\"tier\"", "\"level\""] {
            assert!(!text.contains(banned), "metadata must not contain {banned}");
        }
    }

    #[test]
    fn the_description_states_what_the_commitment_does_not_prove() {
        let m = metadata(&parity_world(), "<svg/>", "aa", None);
        let d = m["description"].as_str().unwrap();
        assert!(d.contains("does not prove"));
    }

    #[test]
    fn the_image_is_self_contained_unless_overridden() {
        let m = metadata(&rich_world(), "<svg/>", "aa", None);
        assert!(m["image"].as_str().unwrap().starts_with("data:image/svg+xml;base64,"));

        let m = metadata(&rich_world(), "<svg/>", "aa", Some("ipfs://abc"));
        assert_eq!(m["image"], "ipfs://abc");
    }

    #[test]
    fn the_commitment_is_identity_not_a_filterable_trait() {
        let m = metadata(&parity_world(), "<svg/>", "deadbeef", None);
        assert_eq!(m["scema"]["world_commitment"], "deadbeef");
        let attrs = serde_json::to_string(&m["attributes"]).unwrap();
        assert!(!attrs.contains("deadbeef"));
    }
}
