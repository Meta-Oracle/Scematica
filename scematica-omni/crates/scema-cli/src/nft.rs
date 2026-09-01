//! `scema nft` — draw a world as a self-contained SVG plate.
//!
//! Reads a `WorldState` (what `scema observe --json` prints, and what all four producers
//! emit) or a sealed decision record, and writes an SVG that depends on nothing outside
//! itself: no fonts to fetch, no image host, no script. That is a requirement rather than
//! tidiness — a token whose picture lives at a URL somebody has to keep paying for is not a
//! record of anything.
//!
//! ## Two things this command deliberately does not do
//!
//! **It does not mint, sign, or spend.** It writes files. `pay` is still an unimplemented
//! verb and a spend policy has to exist before it is not; putting a chain write behind a
//! subcommand that draws pictures would be the fastest possible route to the one class of
//! irreversible action this runtime has been careful never to take. What comes out is an
//! SVG and a metadata JSON, and where they go next is somebody else's decision.
//!
//! **It does not re-verify a record.** When handed one it uses the *stored*
//! `commitment.world`, so an edited record produces a plate whose digest does not match its
//! own world — which is the tamper signal, and is exactly what `scema verify` is for.
//! Recomputing here would quietly repair the evidence.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{bail, Context, Result};
use scema_nft::{load, render_metadata, render_svg, SourceKind};

/// Read a document from a path or `-`.
fn read(locator: &str) -> Result<(String, String)> {
    if locator == "-" || locator.eq_ignore_ascii_case("stdin") {
        let mut text = String::new();
        std::io::stdin()
            .read_to_string(&mut text)
            .context("reading a world from stdin")?;
        if text.trim().is_empty() {
            bail!(
                "nothing arrived on stdin. A producer that printed its help text or failed \
                 silently looks exactly like this — check its exit code."
            );
        }
        return Ok((text, "stdin".into()));
    }
    let p = Path::new(locator);
    let text =
        std::fs::read_to_string(p).with_context(|| format!("reading {}", p.display()))?;
    Ok((text, p.display().to_string()))
}

// Eight parameters, one per flag. Grouping them into a struct would move the argument list
// somewhere else rather than shorten it, and clap already owns the definition — the struct
// would be a second place for a flag to be forgotten.
#[allow(clippy::too_many_arguments)]
pub fn run(
    locator: &str,
    out: Option<&PathBuf>,
    metadata_out: Option<&PathBuf>,
    image: Option<&str>,
    image_format: &str,
    png: Option<&PathBuf>,
    png_size: usize,
    plate: bool,
) -> Result<ExitCode> {
    let (text, from) = read(locator)?;
    let value: serde_json::Value =
        serde_json::from_str(&text).with_context(|| format!("parsing {from} as JSON"))?;

    let source = load(&value)?;
    // The growth by default; the plate is the same data read as an instrument.
    let svg = if plate {
        scema_nft::plate::render(&source.world, &source.digest)
    } else {
        render_svg(&source.world, &source.digest)
    };

    match out {
        Some(p) => {
            std::fs::write(p, &svg).with_context(|| format!("writing {}", p.display()))?;
            // Diagnostics on stderr so `scema nft world.json > plate.svg` stays clean.
            eprintln!("wrote {} ({} bytes)", p.display(), svg.len());
        }
        None => {
            let stdout = std::io::stdout();
            let mut lock = stdout.lock();
            lock.write_all(svg.as_bytes())?;
            lock.write_all(b"\n")?;
        }
    }

    // Rastered once and reused. Writing the file and embedding it in the metadata must not
    // be two renders: they would agree today and are exactly the pair that drifts the moment
    // somebody changes a default on one path.
    let want_png = png.is_some() || image_format.eq_ignore_ascii_case("png");
    let png_bytes = if want_png {
        // The plate has no raster backend — it is an instrument to be read, not an image to
        // be held.
        if plate {
            bail!("PNG renders the growth; it does not apply to --plate");
        }
        let drawn = scema_nft::fractal::render_png(&source.world, &source.digest, png_size);
        // A record rides along inside the image, so the picture is self-contained: it
        // verifies offline and Scema-World can fly it with no vault and no network. A plate
        // that only *names* its record is a claim ticket, and a token whose utility needs a
        // service to be up is a token whose utility can be switched off.
        //
        // The **raw text** is embedded, never a re-serialisation. `serde_json` would collapse
        // `0.0` to `0`, which moves it from the FLOAT tag to the INTEGER tag in the canonical
        // encoding and changes the digest — the record would be intact and would report as
        // tampered, which is the one failure that teaches a reader to stop believing the
        // verifier.
        Some(if source.kind == SourceKind::Record {
            scema_nft::raster::embed_record(&drawn, &text)
        } else {
            drawn
        })
    } else {
        None
    };

    if let (Some(p), Some(bytes)) = (png, png_bytes.as_ref()) {
        std::fs::write(p, bytes).with_context(|| format!("writing {}", p.display()))?;
        eprintln!("wrote {} ({} bytes, {png_size}x{png_size})", p.display(), bytes.len());
        if source.kind == SourceKind::Record {
            eprintln!("  the record travels inside it");
            eprintln!("  this image verifies offline and flies in Scema-World without a vault");
        }
    }

    if let Some(p) = metadata_out {
        // `--image` still wins. A hosted URL is what anything minted at scale should carry,
        // and a flag the caller passed explicitly must not be overridden by a default.
        let embedded;
        let image = match (image, png_bytes.as_ref()) {
            (Some(url), _) => Some(url),
            (None, Some(bytes)) if image_format.eq_ignore_ascii_case("png") => {
                embedded = scema_nft::metadata::data_uri_png(bytes);
                // Base64 inflates by 4/3 and this lands inside the metadata document. Said
                // out loud, because a 4 MB token JSON is rejected by some hosts and merely
                // slow at others, and both failures happen long after this command exits.
                eprintln!(
                    "metadata image is the {png_size}x{png_size} PNG, inline: {} KB of base64 \
                     (use --png-size to trade detail for size, or --image <url> to host it)",
                    embedded.len() / 1024
                );
                Some(embedded.as_str())
            }
            _ => None,
        };
        let meta = render_metadata(&source.world, &svg, &source.digest, image);
        let json = serde_json::to_string_pretty(&meta)?;
        std::fs::write(p, format!("{json}\n"))
            .with_context(|| format!("writing {}", p.display()))?;
        eprintln!("wrote {}", p.display());
    } else if image.is_some() {
        // Silently ignoring a flag is how somebody ends up believing their token points at
        // IPFS when it does not.
        eprintln!("note: --image only affects the metadata; pass --metadata to write one");
    }

    let w = &source.world;
    eprintln!(
        "{} · {} object(s), {} signal(s), {} blind spot(s)",
        match source.kind {
            SourceKind::World => "world",
            SourceKind::Record => "record",
        },
        w.objects.len(),
        w.signals.len(),
        w.blind_spots.len()
    );
    eprintln!("world commitment {}", source.digest);
    if matches!(source.kind, SourceKind::Record) {
        eprintln!(
            "the commitment above is the one the record stores; run `scema verify` to \
             confirm it still matches"
        );
    }

    Ok(ExitCode::SUCCESS)
}
