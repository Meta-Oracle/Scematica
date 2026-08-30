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

pub fn run(
    locator: &str,
    out: Option<&PathBuf>,
    metadata_out: Option<&PathBuf>,
    image: Option<&str>,
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

    if let Some(p) = png {
        // Rasterised from the same primitives the SVG is built from, so the two cannot
        // depict different trees. The plate has no raster backend — it is an instrument to
        // be read, not an image to be held.
        if plate {
            bail!("--png renders the growth; it does not apply to --plate");
        }
        let bytes = scema_nft::fractal::render_png(&source.world, &source.digest, png_size);
        std::fs::write(p, &bytes).with_context(|| format!("writing {}", p.display()))?;
        eprintln!("wrote {} ({} bytes, {png_size}x{png_size})", p.display(), bytes.len());
    }

    if let Some(p) = metadata_out {
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
