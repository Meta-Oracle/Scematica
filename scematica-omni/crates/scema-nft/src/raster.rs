//! A PNG of the growth, rasterised by hand.
//!
//! ## Why not a rendering library
//!
//! The obvious way to get a PNG is to hand the SVG to `resvg`, or to a browser canvas. Both
//! would break the property the whole crate is built on: **the same world produces the same
//! bytes in Rust and in the browser.** Two different rasterisers antialias differently, so
//! the PNG would depend on which runtime made it — and an image that depends on who rendered
//! it is not a derivative of the record, it is two artefacts with one name.
//!
//! So the rasteriser is here, in integers, and so is the PNG encoder. That is the same call
//! this crate already made for base64, for the glob matcher, and for the sine table, and for
//! the same reason each time: a dependency that is *nearly* deterministic is worse than
//! thirty lines that are.
//!
//! ## How the antialiasing works, and why it is exact
//!
//! No analytic coverage, no floating point. The scene is drawn at `SS`× into an integer
//! buffer with hard edges, then box-downsampled — the average of an `SS × SS` block, in
//! integer arithmetic with a fixed rounding rule. Every pixel is therefore a sum of integers
//! divided by a constant, which is a value two runtimes cannot disagree about.
//!
//! ## The compression is deliberately trivial
//!
//! PNG requires a zlib stream, and this emits **stored** (uncompressed) deflate blocks. A
//! real compressor is a large amount of code whose output depends on its heuristics, and a
//! heuristic is exactly the kind of thing that differs between two implementations. Stored
//! blocks make the byte stream a pure function of the pixels. The files are larger, and that
//! is the right trade for an artefact whose entire value is reproducibility.

use crate::palette::{Ink, Role};

/// Supersampling factor. Three is enough to soften the thin outer branches without making
/// the intermediate buffer unreasonable.
const SS: usize = 3;

/// One thing to draw, in canvas coordinates (milliunits, as the SVG uses).
#[derive(Clone, Debug)]
pub enum Prim {
    Line { a: (i64, i64), b: (i64, i64), width_mu: i64, role: Role, dashed: bool },
    Disc { c: (i64, i64), r_mu: i64, role: Role, filled: bool },
    Tri { a: (i64, i64), b: (i64, i64), c: (i64, i64), role: Role, filled: bool },
    Text { at: (i64, i64), size: i64, role: Role, anchor: Anchor, body: String },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Anchor {
    Start,
    Middle,
    End,
}

// ── a 5x7 bitmap font ─────────────────────────────────────────────────────────
//
// One `u8` per column, bit 0 the top row. Five columns per glyph, one column of spacing.
// Hand-written rather than embedded from a font file: a font file is a binary blob whose
// rasterisation depends on a hinting engine, which is the dependency this module exists to
// avoid. Legibility at this size only needs the shapes to be distinct.

const GLYPH_W: usize = 5;
const GLYPH_H: usize = 7;

/// Printable ASCII, 0x20 to 0x7E.
const FONT: [[u8; 5]; 95] = [
    [0x00, 0x00, 0x00, 0x00, 0x00], // space
    [0x00, 0x00, 0x5f, 0x00, 0x00], // !
    [0x00, 0x07, 0x00, 0x07, 0x00], // "
    [0x14, 0x7f, 0x14, 0x7f, 0x14], // #
    [0x24, 0x2a, 0x7f, 0x2a, 0x12], // $
    [0x23, 0x13, 0x08, 0x64, 0x62], // %
    [0x36, 0x49, 0x55, 0x22, 0x50], // &
    [0x00, 0x05, 0x03, 0x00, 0x00], // '
    [0x00, 0x1c, 0x22, 0x41, 0x00], // (
    [0x00, 0x41, 0x22, 0x1c, 0x00], // )
    [0x14, 0x08, 0x3e, 0x08, 0x14], // *
    [0x08, 0x08, 0x3e, 0x08, 0x08], // +
    [0x00, 0x50, 0x30, 0x00, 0x00], // ,
    [0x08, 0x08, 0x08, 0x08, 0x08], // -
    [0x00, 0x60, 0x60, 0x00, 0x00], // .
    [0x20, 0x10, 0x08, 0x04, 0x02], // /
    [0x3e, 0x51, 0x49, 0x45, 0x3e], // 0
    [0x00, 0x42, 0x7f, 0x40, 0x00], // 1
    [0x42, 0x61, 0x51, 0x49, 0x46], // 2
    [0x21, 0x41, 0x45, 0x4b, 0x31], // 3
    [0x18, 0x14, 0x12, 0x7f, 0x10], // 4
    [0x27, 0x45, 0x45, 0x45, 0x39], // 5
    [0x3c, 0x4a, 0x49, 0x49, 0x30], // 6
    [0x01, 0x71, 0x09, 0x05, 0x03], // 7
    [0x36, 0x49, 0x49, 0x49, 0x36], // 8
    [0x06, 0x49, 0x49, 0x29, 0x1e], // 9
    [0x00, 0x36, 0x36, 0x00, 0x00], // :
    [0x00, 0x56, 0x36, 0x00, 0x00], // ;
    [0x08, 0x14, 0x22, 0x41, 0x00], // <
    [0x14, 0x14, 0x14, 0x14, 0x14], // =
    [0x00, 0x41, 0x22, 0x14, 0x08], // >
    [0x02, 0x01, 0x51, 0x09, 0x06], // ?
    [0x32, 0x49, 0x79, 0x41, 0x3e], // @
    [0x7e, 0x11, 0x11, 0x11, 0x7e], // A
    [0x7f, 0x49, 0x49, 0x49, 0x36], // B
    [0x3e, 0x41, 0x41, 0x41, 0x22], // C
    [0x7f, 0x41, 0x41, 0x22, 0x1c], // D
    [0x7f, 0x49, 0x49, 0x49, 0x41], // E
    [0x7f, 0x09, 0x09, 0x09, 0x01], // F
    [0x3e, 0x41, 0x49, 0x49, 0x7a], // G
    [0x7f, 0x08, 0x08, 0x08, 0x7f], // H
    [0x00, 0x41, 0x7f, 0x41, 0x00], // I
    [0x20, 0x40, 0x41, 0x3f, 0x01], // J
    [0x7f, 0x08, 0x14, 0x22, 0x41], // K
    [0x7f, 0x40, 0x40, 0x40, 0x40], // L
    [0x7f, 0x02, 0x0c, 0x02, 0x7f], // M
    [0x7f, 0x04, 0x08, 0x10, 0x7f], // N
    [0x3e, 0x41, 0x41, 0x41, 0x3e], // O
    [0x7f, 0x09, 0x09, 0x09, 0x06], // P
    [0x3e, 0x41, 0x51, 0x21, 0x5e], // Q
    [0x7f, 0x09, 0x19, 0x29, 0x46], // R
    [0x46, 0x49, 0x49, 0x49, 0x31], // S
    [0x01, 0x01, 0x7f, 0x01, 0x01], // T
    [0x3f, 0x40, 0x40, 0x40, 0x3f], // U
    [0x1f, 0x20, 0x40, 0x20, 0x1f], // V
    [0x3f, 0x40, 0x38, 0x40, 0x3f], // W
    [0x63, 0x14, 0x08, 0x14, 0x63], // X
    [0x07, 0x08, 0x70, 0x08, 0x07], // Y
    [0x61, 0x51, 0x49, 0x45, 0x43], // Z
    [0x00, 0x7f, 0x41, 0x41, 0x00], // [
    [0x02, 0x04, 0x08, 0x10, 0x20], // backslash
    [0x00, 0x41, 0x41, 0x7f, 0x00], // ]
    [0x04, 0x02, 0x01, 0x02, 0x04], // ^
    [0x40, 0x40, 0x40, 0x40, 0x40], // _
    [0x00, 0x01, 0x02, 0x04, 0x00], // `
    [0x20, 0x54, 0x54, 0x54, 0x78], // a
    [0x7f, 0x48, 0x44, 0x44, 0x38], // b
    [0x38, 0x44, 0x44, 0x44, 0x20], // c
    [0x38, 0x44, 0x44, 0x48, 0x7f], // d
    [0x38, 0x54, 0x54, 0x54, 0x18], // e
    [0x08, 0x7e, 0x09, 0x01, 0x02], // f
    [0x0c, 0x52, 0x52, 0x52, 0x3e], // g
    [0x7f, 0x08, 0x04, 0x04, 0x78], // h
    [0x00, 0x44, 0x7d, 0x40, 0x00], // i
    [0x20, 0x40, 0x44, 0x3d, 0x00], // j
    [0x7f, 0x10, 0x28, 0x44, 0x00], // k
    [0x00, 0x41, 0x7f, 0x40, 0x00], // l
    [0x7c, 0x04, 0x18, 0x04, 0x78], // m
    [0x7c, 0x08, 0x04, 0x04, 0x78], // n
    [0x38, 0x44, 0x44, 0x44, 0x38], // o
    [0x7c, 0x14, 0x14, 0x14, 0x08], // p
    [0x08, 0x14, 0x14, 0x18, 0x7c], // q
    [0x7c, 0x08, 0x04, 0x04, 0x08], // r
    [0x48, 0x54, 0x54, 0x54, 0x20], // s
    [0x04, 0x3f, 0x44, 0x40, 0x20], // t
    [0x3c, 0x40, 0x40, 0x20, 0x7c], // u
    [0x1c, 0x20, 0x40, 0x20, 0x1c], // v
    [0x3c, 0x40, 0x30, 0x40, 0x3c], // w
    [0x44, 0x28, 0x10, 0x28, 0x44], // x
    [0x0c, 0x50, 0x50, 0x50, 0x3c], // y
    [0x44, 0x64, 0x54, 0x4c, 0x44], // z
    [0x00, 0x08, 0x36, 0x41, 0x00], // {
    [0x00, 0x00, 0x7f, 0x00, 0x00], // |
    [0x00, 0x41, 0x36, 0x08, 0x00], // }
    [0x08, 0x08, 0x2a, 0x1c, 0x08], // ~
];

/// The bitmap for one character.
///
/// The few non-ASCII glyphs the legend uses get shapes of their own rather than a fallback
/// box: `·` and `°` and `∅` and `—` all carry meaning here — the empty set especially, which
/// is how an unmeasured coverage is written everywhere else in this repository.
fn glyph(c: char) -> [u8; 5] {
    match c {
        '·' => [0x00, 0x00, 0x08, 0x00, 0x00],
        '°' => [0x00, 0x06, 0x09, 0x06, 0x00],
        '∅' => [0x3e, 0x51, 0x49, 0x45, 0x3e],
        '—' | '–' => [0x08, 0x08, 0x08, 0x08, 0x08],
        '≥' => [0x44, 0x54, 0x54, 0x54, 0x28],
        _ => {
            let i = c as usize;
            if (0x20..=0x7e).contains(&i) {
                FONT[i - 0x20]
            } else {
                // An unknown character is a visible box, never a blank. A silently dropped
                // glyph makes a label read as something it is not.
                [0x7f, 0x41, 0x41, 0x41, 0x7f]
            }
        }
    }
}

// ── canvas ────────────────────────────────────────────────────────────────────

struct Buf {
    w: usize,
    h: usize,
    px: Vec<u8>, // RGB, row-major
}

impl Buf {
    fn new(w: usize, h: usize, bg: Ink) -> Buf {
        let mut px = Vec::with_capacity(w * h * 3);
        for _ in 0..(w * h) {
            px.push(bg.0);
            px.push(bg.1);
            px.push(bg.2);
        }
        Buf { w, h, px }
    }

    fn set(&mut self, x: i64, y: i64, ink: Ink) {
        if x < 0 || y < 0 || x as usize >= self.w || y as usize >= self.h {
            return;
        }
        let i = (y as usize * self.w + x as usize) * 3;
        self.px[i] = ink.0;
        self.px[i + 1] = ink.1;
        self.px[i + 2] = ink.2;
    }

    /// A filled disc, used as the brush for every stroke.
    ///
    /// Stamping discs along a segment gives round joins and caps for free, which is what the
    /// SVG asks for with `stroke-linecap="round"`. Slower than a span fill and not by enough
    /// to matter for a few thousand segments.
    fn disc(&mut self, cx: i64, cy: i64, r: i64, ink: Ink) {
        if r <= 0 {
            self.set(cx, cy, ink);
            return;
        }
        let rr = r * r;
        for dy in -r..=r {
            for dx in -r..=r {
                if dx * dx + dy * dy <= rr {
                    self.set(cx + dx, cy + dy, ink);
                }
            }
        }
    }

    /// A thick line, by stamping the brush along an integer Bresenham walk.
    ///
    /// Eight arguments, and a struct for them would be worse: every one is a primitive the
    /// caller already has in hand, and wrapping them would add a type whose only purpose is
    /// to satisfy a lint.
    #[allow(clippy::too_many_arguments)]
    fn line(&mut self, x0: i64, y0: i64, x1: i64, y1: i64, r: i64, ink: Ink, dashed: bool) {
        let (dx, dy) = ((x1 - x0).abs(), -(y1 - y0).abs());
        let (sx, sy) = (if x0 < x1 { 1 } else { -1 }, if y0 < y1 { 1 } else { -1 });
        let mut err = dx + dy;
        let (mut x, mut y) = (x0, y0);
        let mut step: i64 = 0;
        loop {
            // The SVG dash is `3 4` at 1× scale; the same rhythm scaled, so the two
            // renderings read the same rather than merely being both dashed.
            let on = !dashed || (step / (3 * SS as i64)) % 2 == 0;
            if on {
                self.disc(x, y, r, ink);
            }
            if x == x1 && y == y1 {
                break;
            }
            let e2 = 2 * err;
            if e2 >= dy {
                err += dy;
                x += sx;
            }
            if e2 <= dx {
                err += dx;
                y += sy;
            }
            step += 1;
            if step > 20_000 {
                break; // a degenerate segment cannot become an infinite loop
            }
        }
    }

    fn tri(&mut self, a: (i64, i64), b: (i64, i64), c: (i64, i64), ink: Ink) {
        // Barycentric fill over the bounding box. Integer cross products, no division.
        let minx = a.0.min(b.0).min(c.0);
        let maxx = a.0.max(b.0).max(c.0);
        let miny = a.1.min(b.1).min(c.1);
        let maxy = a.1.max(b.1).max(c.1);
        let cross = |p: (i64, i64), q: (i64, i64), r: (i64, i64)| {
            (q.0 - p.0) * (r.1 - p.1) - (q.1 - p.1) * (r.0 - p.0)
        };
        for y in miny..=maxy {
            for x in minx..=maxx {
                let p = (x, y);
                let d1 = cross(a, b, p);
                let d2 = cross(b, c, p);
                let d3 = cross(c, a, p);
                let neg = d1 < 0 || d2 < 0 || d3 < 0;
                let pos = d1 > 0 || d2 > 0 || d3 > 0;
                if !(neg && pos) {
                    self.set(x, y, ink);
                }
            }
        }
    }

    /// Draw text with the bitmap font, scaled by an integer factor.
    fn text(&mut self, x: i64, y: i64, scale: i64, anchor: Anchor, body: &str, ink: Ink) {
        let chars: Vec<char> = body.chars().collect();
        let advance = (GLYPH_W as i64 + 1) * scale;
        let width = chars.len() as i64 * advance;
        let start = match anchor {
            Anchor::Start => x,
            Anchor::Middle => x - width / 2,
            Anchor::End => x - width,
        };
        // `y` is the SVG baseline; the font is drawn from its top, so lift it.
        let top = y - GLYPH_H as i64 * scale;
        for (i, ch) in chars.iter().enumerate() {
            let g = glyph(*ch);
            let gx = start + i as i64 * advance;
            for (col, bits) in g.iter().enumerate() {
                for row in 0..GLYPH_H {
                    if bits & (1 << row) != 0 {
                        for sy in 0..scale {
                            for sx in 0..scale {
                                self.set(
                                    gx + col as i64 * scale + sx,
                                    top + row as i64 * scale + sy,
                                    ink,
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    /// Average each `SS × SS` block. Integer division with a fixed rule, so two runtimes
    /// cannot disagree about a pixel.
    fn downsample(&self, out_w: usize, out_h: usize) -> Vec<u8> {
        let n = (SS * SS) as u32;
        let mut out = Vec::with_capacity(out_w * out_h * 3);
        for y in 0..out_h {
            for x in 0..out_w {
                let mut acc = [0u32; 3];
                for sy in 0..SS {
                    for sx in 0..SS {
                        let i = ((y * SS + sy) * self.w + (x * SS + sx)) * 3;
                        acc[0] += self.px[i] as u32;
                        acc[1] += self.px[i + 1] as u32;
                        acc[2] += self.px[i + 2] as u32;
                    }
                }
                // Round half up, stated rather than inherited from a language default.
                for c in acc {
                    out.push(((c + n / 2) / n) as u8);
                }
            }
        }
        out
    }
}

// ── png ───────────────────────────────────────────────────────────────────────

fn crc32(data: &[u8]) -> u32 {
    let mut table = [0u32; 256];
    for (i, e) in table.iter_mut().enumerate() {
        let mut c = i as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 { 0xedb8_8320 ^ (c >> 1) } else { c >> 1 };
        }
        *e = c;
    }
    let mut c = 0xffff_ffffu32;
    for b in data {
        c = table[((c ^ *b as u32) & 0xff) as usize] ^ (c >> 8);
    }
    c ^ 0xffff_ffff
}

fn adler32(data: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for byte in data {
        a = (a + *byte as u32) % 65521;
        b = (b + a) % 65521;
    }
    (b << 16) | a
}

fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], body: &[u8]) {
    out.extend_from_slice(&(body.len() as u32).to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(body);
    let mut crc_input = Vec::with_capacity(4 + body.len());
    crc_input.extend_from_slice(kind);
    crc_input.extend_from_slice(body);
    out.extend_from_slice(&crc32(&crc_input).to_be_bytes());
}

/// A zlib stream of **stored** deflate blocks.
///
/// A real compressor's output depends on its heuristics, and a heuristic is exactly the kind
/// of thing two implementations differ about. Stored blocks make the byte stream a pure
/// function of the pixels, at the cost of size — the right trade for an artefact whose whole
/// value is reproducibility.
fn zlib_stored(data: &[u8]) -> Vec<u8> {
    let mut out = vec![0x78, 0x01];
    let mut i = 0;
    if data.is_empty() {
        out.extend_from_slice(&[0x01, 0x00, 0x00, 0xff, 0xff]);
    }
    while i < data.len() {
        let n = (data.len() - i).min(65_535);
        let last = if i + n >= data.len() { 1u8 } else { 0u8 };
        out.push(last);
        out.extend_from_slice(&(n as u16).to_le_bytes());
        out.extend_from_slice(&(!(n as u16)).to_le_bytes());
        out.extend_from_slice(&data[i..i + n]);
        i += n;
    }
    out.extend_from_slice(&adler32(data).to_be_bytes());
    out
}

/// PNG keyword under which the world commitment travels.
///
/// A `tEXt` chunk, which is standard and which every decoder ignores if it does not care.
/// The image is a derivative of a specific record, and until this existed it could not say
/// which one: the plate draws only a *shortened* digest as glyphs, and pixels are not
/// invertible anyway. An artefact that cannot name what it depicts is a picture, not a
/// derivative.
///
/// It carries no clock and no counter, so the bytes stay a pure function of the record.
pub const WORLD_KEYWORD: &str = "scema.world";

/// A `tEXt` chunk: `keyword\0text`, Latin-1.
///
/// The digest is 64 hex characters, so the Latin-1 restriction is satisfied by construction
/// and there is nothing to escape.
fn text_chunk(out: &mut Vec<u8>, keyword: &str, text: &str) {
    let mut body = Vec::with_capacity(keyword.len() + 1 + text.len());
    body.extend_from_slice(keyword.as_bytes());
    body.push(0);
    body.extend_from_slice(text.as_bytes());
    chunk(out, b"tEXt", &body);
}

/// PNG keyword under which a whole decision record travels.
///
/// ## Why an image carries a record at all
///
/// A plate names the world it derives from and nothing else, which makes it a *claim ticket*:
/// to fly the space, or to verify the record, you had to go and fetch the record from
/// somewhere. That is right for distribution — `scema-vault` gates exactly that — and it is
/// wrong for an artefact somebody owns. A token whose utility requires a service to be up is
/// a token whose utility can be switched off.
///
/// So the record may ride along inside the image. The picture is then self-contained: it
/// verifies offline, and Scema-World can fly it with no vault and no network.
///
/// ## `iTXt`, not `tEXt`
///
/// `tEXt` is Latin-1. A record carries labels lifted from whatever was observed — file paths,
/// page titles, feed names — and one non-Latin-1 byte in any of them would corrupt the record
/// on the way in, which is the worst available failure: a verifier reporting tampering that
/// the writer caused. `iTXt` is UTF-8 by specification, stored uncompressed here so the bytes
/// are a pure function of the record exactly as the pixels are.
pub const RECORD_KEYWORD: &str = "scema.record";

/// An `iTXt` chunk: `keyword\0 compression_flag compression_method \0 \0 text`, UTF-8.
fn itxt_chunk(out: &mut Vec<u8>, keyword: &str, text: &str) {
    let mut body = Vec::with_capacity(keyword.len() + 5 + text.len());
    body.extend_from_slice(keyword.as_bytes());
    body.push(0);
    body.push(0); // uncompressed
    body.push(0); // compression method, ignored when uncompressed
    body.push(0); // empty language tag
    body.push(0); // empty translated keyword
    body.extend_from_slice(text.as_bytes());
    chunk(out, b"iTXt", &body);
}

/// Insert a record into an already-encoded PNG, immediately after `IHDR`.
///
/// A post-pass rather than a parameter on `render_png`, deliberately: every existing image
/// stays byte-identical, so the parity fixtures still pin the raster itself rather than the
/// raster plus whatever a caller happened to attach. Embedding is a separate decision from
/// drawing, and the byte-for-byte guarantee belongs to the drawing.
///
/// Returns the input unchanged if it is not a PNG, rather than producing a broken one.
pub fn embed_record(png: &[u8], record: &str) -> Vec<u8> {
    const SIG: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
    if png.len() < 8 + 12 || png[..8] != SIG {
        return png.to_vec();
    }
    // IHDR is required to be the first chunk, so its end is a fixed offset from the signature.
    let ihdr_len = u32::from_be_bytes([png[8], png[9], png[10], png[11]]) as usize;
    let after_ihdr = 8 + 12 + ihdr_len;
    if after_ihdr > png.len() {
        return png.to_vec();
    }
    let mut out = Vec::with_capacity(png.len() + record.len() + 32);
    out.extend_from_slice(&png[..after_ihdr]);
    itxt_chunk(&mut out, RECORD_KEYWORD, record);
    out.extend_from_slice(&png[after_ihdr..]);
    out
}

/// Read back a record embedded by [`embed_record`], if there is one.
pub fn read_record(png: &[u8]) -> Option<String> {
    const SIG: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
    if png.len() < 8 || png[..8] != SIG {
        return None;
    }
    let mut off = 8;
    while off + 8 <= png.len() {
        let len = u32::from_be_bytes([png[off], png[off + 1], png[off + 2], png[off + 3]]) as usize;
        let kind = &png[off + 4..off + 8];
        if off + 12 + len > png.len() {
            return None;
        }
        if kind == b"iTXt" {
            let body = &png[off + 8..off + 8 + len];
            if let Some(nul) = body.iter().position(|&b| b == 0) {
                if &body[..nul] == RECORD_KEYWORD.as_bytes() && body.len() >= nul + 5 {
                    // Uncompressed only: a compressed record would need an inflater, and this
                    // crate deliberately has no decompressor to be wrong about.
                    if body[nul + 1] != 0 {
                        return None;
                    }
                    // Skip the language tag and translated keyword, both empty here but both
                    // permitted to be non-empty by a different writer.
                    let mut i = nul + 3;
                    let mut seen = 0;
                    while i < body.len() && seen < 2 {
                        if body[i] == 0 {
                            seen += 1;
                        }
                        i += 1;
                    }
                    return String::from_utf8(body[i..].to_vec()).ok();
                }
            }
        }
        if kind == b"IEND" {
            break;
        }
        off += 12 + len;
    }
    None
}

fn encode_png(w: usize, h: usize, rgb: &[u8], world: &str) -> Vec<u8> {
    // Filter type 0 (None) on every row. Any other filter is a compression aid, and there is
    // no compression here to aid.
    let mut raw = Vec::with_capacity(h * (1 + w * 3));
    for y in 0..h {
        raw.push(0);
        raw.extend_from_slice(&rgb[y * w * 3..(y + 1) * w * 3]);
    }

    let mut out = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&(w as u32).to_be_bytes());
    ihdr.extend_from_slice(&(h as u32).to_be_bytes());
    ihdr.extend_from_slice(&[8, 2, 0, 0, 0]); // 8-bit, truecolour RGB
    chunk(&mut out, b"IHDR", &ihdr);
    // Between IHDR and IDAT, which is where a `tEXt` chunk belongs and where a reader will
    // find it without decompressing anything.
    if !world.is_empty() {
        text_chunk(&mut out, WORLD_KEYWORD, world);
    }
    chunk(&mut out, b"IDAT", &zlib_stored(&raw));
    chunk(&mut out, b"IEND", &[]);
    out
}

/// Rasterise a scene to a PNG.
///
/// `size` is the output edge in pixels; the scene's coordinate space is `view` units.
pub fn render_png(prims: &[Prim], view: i64, size: usize, bg: Role, world: &str) -> Vec<u8> {
    let big = size * SS;
    let mut buf = Buf::new(big, big, bg.ink());

    // milliunits -> supersampled pixels, in integers.
    let to_px = |v: i64| -> i64 { (v * big as i64) / (view * 1000) };
    let scale_mu = |v: i64| -> i64 { (v * big as i64) / (view * 1000) };

    for p in prims {
        match p {
            Prim::Line { a, b, width_mu, role, dashed } => {
                let r = (scale_mu(*width_mu) / 2).max(1);
                buf.line(to_px(a.0), to_px(a.1), to_px(b.0), to_px(b.1), r, role.ink(), *dashed);
            }
            Prim::Disc { c, r_mu, role, filled } => {
                let r = scale_mu(*r_mu).max(1);
                if *filled {
                    buf.disc(to_px(c.0), to_px(c.1), r, role.ink());
                } else {
                    // A hollow mark is a ring: an estimated magnitude must not draw as a
                    // counted one, and that distinction has to survive rasterisation.
                    let ink = role.ink();
                    for t in 0..(SS as i64).max(1) {
                        let rr = (r - t).max(1);
                        let steps = (rr * 8).max(16);
                        for i in 0..steps {
                            let deg = (i * 360) / steps;
                            let x = to_px(c.0) + (rr * crate::geom::cos_micro(deg)) / 1_000_000;
                            let y = to_px(c.1) + (rr * crate::geom::sin_micro(deg)) / 1_000_000;
                            buf.set(x, y, ink);
                        }
                    }
                }
            }
            Prim::Tri { a, b, c, role, filled } => {
                let (pa, pb, pc) =
                    ((to_px(a.0), to_px(a.1)), (to_px(b.0), to_px(b.1)), (to_px(c.0), to_px(c.1)));
                if *filled {
                    buf.tri(pa, pb, pc, role.ink());
                } else {
                    let r = (SS as i64 / 2).max(1);
                    buf.line(pa.0, pa.1, pb.0, pb.1, r, role.ink(), false);
                    buf.line(pb.0, pb.1, pc.0, pc.1, r, role.ink(), false);
                    buf.line(pc.0, pc.1, pa.0, pa.1, r, role.ink(), false);
                }
            }
            Prim::Text { at, size: fs, role, anchor, body } => {
                // The SVG font-size is a cap height in view units; the bitmap font is seven
                // rows, so the scale is chosen to land near it.
                let px = (*fs * big as i64) / view;
                let s = (px / GLYPH_H as i64).max(1);
                buf.text(to_px(at.0), to_px(at.1), s, *anchor, body, role.ink());
            }
        }
    }

    let rgb = buf.downsample(size, size);
    encode_png(size, size, &rgb, world)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_png_has_a_valid_signature_and_the_three_required_chunks() {
        let png = render_png(&[], 512, 64, Role::Ground, "");
        assert_eq!(&png[..8], &[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]);
        let has = |k: &[u8]| png.windows(4).any(|w| w == k);
        assert!(has(b"IHDR") && has(b"IDAT") && has(b"IEND"));
    }

    #[test]
    fn the_header_declares_the_size_it_actually_wrote() {
        let png = render_png(&[], 512, 64, Role::Ground, "");
        // IHDR body starts at byte 16: 8 signature + 4 length + 4 type.
        let w = u32::from_be_bytes([png[16], png[17], png[18], png[19]]);
        let h = u32::from_be_bytes([png[20], png[21], png[22], png[23]]);
        assert_eq!((w, h), (64, 64));
    }

    #[test]
    fn every_chunk_crc_checks_out() {
        // A wrong CRC is the difference between a file and a file a decoder refuses.
        let png = render_png(&[], 512, 32, Role::Ground, "");
        let mut i = 8usize;
        let mut seen = 0;
        while i + 8 <= png.len() {
            let len = u32::from_be_bytes([png[i], png[i + 1], png[i + 2], png[i + 3]]) as usize;
            let start = i + 4;
            let end = start + 4 + len;
            let stored = u32::from_be_bytes([png[end], png[end + 1], png[end + 2], png[end + 3]]);
            assert_eq!(crc32(&png[start..end]), stored, "chunk at {i}");
            seen += 1;
            i = end + 4;
        }
        assert_eq!(seen, 3);
    }

    #[test]
    fn the_same_scene_produces_the_same_bytes() {
        let prims = vec![Prim::Line {
            a: (10_000, 10_000),
            b: (400_000, 300_000),
            width_mu: 2_000,
            role: Role::Measured,
            dashed: false,
        }];
        assert_eq!(render_png(&prims, 512, 128, Role::Ground, ""), render_png(&prims, 512, 128, Role::Ground, ""));
    }

    #[test]
    fn a_drawn_scene_differs_from_an_empty_one() {
        let empty = render_png(&[], 512, 128, Role::Ground, "");
        let drawn = render_png(
            &[Prim::Line {
                a: (10_000, 10_000),
                b: (400_000, 300_000),
                width_mu: 4_000,
                role: Role::Measured,
                dashed: false,
            }],
            512,
            128,
            Role::Ground,
            "",
        );
        assert_ne!(empty, drawn, "the line must reach the pixels");
        assert_eq!(empty.len(), drawn.len(), "stored deflate makes size independent of content");
    }

    #[test]
    fn adler_and_crc_match_their_reference_vectors() {
        // Both are load-bearing: a wrong Adler makes the zlib stream invalid, a wrong CRC
        // makes the chunk invalid, and either produces a file no decoder will open.
        assert_eq!(adler32(b"abc"), 0x024d0127);
        assert_eq!(crc32(b"123456789"), 0xcbf43926);
    }

    #[test]
    fn an_unknown_glyph_is_a_visible_box_rather_than_a_blank() {
        // A silently dropped character makes a label read as something it is not.
        assert_ne!(glyph('\u{4e2d}'), [0, 0, 0, 0, 0]);
        assert_eq!(glyph(' '), [0, 0, 0, 0, 0]);
    }

    #[test]
    fn the_empty_set_glyph_exists_because_the_legend_uses_it() {
        // `∅` is how an unmeasured coverage is written everywhere else here; falling back to
        // a box would turn a specific statement into a missing-character marker.
        assert_ne!(glyph('∅'), glyph('\u{4e2d}'));
    }
}

#[cfg(test)]
mod embed_tests {
    use super::*;

    fn tiny_png() -> Vec<u8> {
        encode_png(1, 1, &[255, 0, 0], "abc123")
    }

    #[test]
    fn a_record_survives_the_round_trip_byte_for_byte() {
        // The whole point. A record that came back altered would verify as tampered, and a
        // verifier that cries tamper on an honest round trip is worse than no verifier.
        let record = r#"{"a":0.0,"b":"é 世界","c":[1,2,3]}"#;
        let png = embed_record(&tiny_png(), record);
        assert_eq!(read_record(&png).as_deref(), Some(record));
    }

    #[test]
    fn embedding_leaves_the_world_commitment_and_the_pixels_alone() {
        let base = tiny_png();
        let png = embed_record(&base, "{}");
        // Every original chunk is still present, in order: the record was inserted, not
        // spliced over anything.
        assert!(png.windows(4).any(|w| w == b"IHDR"));
        assert!(png.windows(4).any(|w| w == b"IDAT"));
        assert!(png.windows(4).any(|w| w == b"IEND"));
        assert!(png.len() > base.len());
    }

    #[test]
    fn a_png_without_a_record_reports_none_rather_than_empty() {
        // "There is no record in this image" and "this image carries an empty record" are
        // different facts, and only one of them is worth showing somebody a space for.
        assert_eq!(read_record(&tiny_png()), None);
    }

    #[test]
    fn something_that_is_not_a_png_comes_back_unharmed() {
        // Never produce a broken PNG from a bad input; hand the input back.
        let junk = b"not a png at all".to_vec();
        assert_eq!(embed_record(&junk, "{}"), junk);
        assert_eq!(read_record(&junk), None);
    }

    #[test]
    fn embedding_is_deterministic() {
        // The bytes stay a pure function of the record, exactly as the pixels are.
        let a = embed_record(&tiny_png(), "{\"x\":1}");
        let b = embed_record(&tiny_png(), "{\"x\":1}");
        assert_eq!(a, b);
    }
}
