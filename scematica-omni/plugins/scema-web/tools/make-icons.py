#!/usr/bin/env python3
"""Regenerate the extension icons from the omni palette.

Pure stdlib (zlib + struct). Pillow is not a dependency of this extension, and adding one
to draw four small squares would be the opposite of the "no build step, no bundler, no
dependencies" rule the whole plugin is built on. The PNGs are checked in; this exists so
that when the palette moves in `crates/scema-tui/src/theme.rs`, the icons can follow from
one place rather than being redrawn by hand.

    python tools/make-icons.py

The mark is a violet iris on the void with a soft-blue pupil. It reads as an eye, which is
what perception is, and at 16 px it survives as a violet ring with a blue centre.
Deliberately NOT a letterform: a glyph in a 16 px slot is a smudge.

Hex values are the same ones `theme.rs` calls VOID / VIOLET / VIOLET_LO / AZURE. Rust is
authoritative; if they drift, these are what is wrong.
"""

import math
import pathlib
import struct
import zlib

VOID = (0x08, 0x06, 0x0F)
VIOLET = (0xA9, 0x6B, 0xFF)
VIOLET_LO = (0x6D, 0x40, 0xC4)
AZURE = (0x7D, 0xD3, 0xFC)

# Supersampling factor. A hard-edged circle at 16 px looks broken, and the browser does not
# antialias an icon it was handed at exactly the right size.
SS = 4


def render(size: int) -> bytes:
    big = size * SS
    centre = (big - 1) / 2.0
    r_outer = big * 0.48
    r_iris = big * 0.34
    r_pupil = big * 0.15

    grid = [[(0, 0, 0, 0)] * big for _ in range(big)]
    for y in range(big):
        for x in range(big):
            d = math.hypot(x - centre, y - centre)
            if d <= r_pupil:
                grid[y][x] = AZURE + (255,)
            elif d <= r_iris:
                t = (d - r_pupil) / max(1e-6, r_iris - r_pupil)
                grid[y][x] = tuple(
                    int(VIOLET[i] + (VIOLET_LO[i] - VIOLET[i]) * t) for i in range(3)
                ) + (255,)
            elif d <= r_outer:
                grid[y][x] = VOID + (255,)

    raw = bytearray()
    for y in range(size):
        raw.append(0)  # PNG filter type: none
        for x in range(size):
            acc = [0, 0, 0, 0]
            for dy in range(SS):
                for dx in range(SS):
                    p = grid[y * SS + dy][x * SS + dx]
                    a = p[3]
                    acc[0] += p[0] * a
                    acc[1] += p[1] * a
                    acc[2] += p[2] * a
                    acc[3] += a
            if acc[3] == 0:
                raw += bytes((0, 0, 0, 0))
            else:
                raw += bytes(
                    (
                        acc[0] // acc[3],
                        acc[1] // acc[3],
                        acc[2] // acc[3],
                        acc[3] // (SS * SS),
                    )
                )

    def chunk(tag: bytes, data: bytes) -> bytes:
        return (
            struct.pack(">I", len(data))
            + tag
            + data
            + struct.pack(">I", zlib.crc32(tag + data) & 0xFFFFFFFF)
        )

    ihdr = struct.pack(">IIBBBBB", size, size, 8, 6, 0, 0, 0)  # 8-bit RGBA
    return (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", ihdr)
        + chunk(b"IDAT", zlib.compress(bytes(raw), 9))
        + chunk(b"IEND", b"")
    )


def main() -> None:
    out = pathlib.Path(__file__).resolve().parent.parent / "icons"
    out.mkdir(exist_ok=True)
    for size in (16, 32, 48, 128):
        path = out / f"omni-{size}.png"
        blob = render(size)
        path.write_bytes(blob)
        print(f"{path.name}  {len(blob)} bytes")


if __name__ == "__main__":
    main()
