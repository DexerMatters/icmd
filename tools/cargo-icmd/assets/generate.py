#!/usr/bin/env python3
"""Generate the documentation guide's bundled raster asset.

The output, ``guide.png``, is an original work owned by this repository. It is
committed so the documentation browser can demonstrate raster rendering without
any network access or runtime path dependency. Re-run this script only to change
the artwork deliberately:

    python3 tools/cargo-icmd/assets/generate.py

License: MIT OR Apache-2.0, matching the rest of the repository.
"""

from __future__ import annotations

import math
import struct
import zlib
from pathlib import Path

WIDTH = 144
HEIGHT = 96

BACKGROUND_TOP = (0x2E, 0x34, 0x40)
BACKGROUND_BOTTOM = (0x3B, 0x42, 0x52)
BORDER = (0x4C, 0x56, 0x6A)
AREA = (0x5E, 0x81, 0xAC)
LINE = (0x88, 0xC0, 0xD0)
MARK = (0xA3, 0xBE, 0x8C)
GRID = (0x43, 0x4C, 0x5E)


def blend(top: tuple[int, int, int], bottom: tuple[int, int, int], t: float) -> tuple[int, int, int]:
    return tuple(round(a + (b - a) * t) for a, b in zip(top, bottom))


def rounded_border(pixels: list[list[tuple[int, int, int]]]) -> None:
    margin = 3
    for x in range(margin, WIDTH - margin):
        for y in (margin, HEIGHT - margin - 1):
            pixels[y][x] = BORDER
    for y in range(margin, HEIGHT - margin):
        for x in (margin, WIDTH - margin - 1):
            pixels[y][x] = BORDER
    for corner_x, corner_y in (
        (margin, margin),
        (WIDTH - margin - 1, margin),
        (margin, HEIGHT - margin - 1),
        (WIDTH - margin - 1, HEIGHT - margin - 1),
    ):
        pixels[corner_y][corner_x] = BACKGROUND_TOP


def main() -> None:
    pixels: list[list[tuple[int, int, int]]] = []
    for y in range(HEIGHT):
        t = y / (HEIGHT - 1)
        row = [blend(BACKGROUND_TOP, BACKGROUND_BOTTOM, t) for _ in range(WIDTH)]
        pixels.append(row)

    for x in range(8, WIDTH - 8, 24):
        for y in range(12, HEIGHT - 12):
            pixels[y][x] = GRID

    baseline = HEIGHT - 18
    for x in range(10, WIDTH - 10):
        phase = (x - 10) / 12.0
        amplitude = 18.0 + 10.0 * math.sin(phase / 2.4)
        value = baseline - int(amplitude * (0.5 + 0.5 * math.sin(phase)))
        for y in range(value, baseline):
            pixels[y][x] = AREA
        for dy in (-1, 0, 1):
            yy = value + dy
            if 0 <= yy < HEIGHT:
                pixels[yy][x] = LINE

    for x in range(WIDTH // 2 - 7, WIDTH // 2 + 8):
        for y in range(HEIGHT // 2 - 7, HEIGHT // 2 + 8):
            if abs(x - WIDTH // 2) + abs(y - HEIGHT // 2) <= 7:
                pixels[y][x] = MARK

    rounded_border(pixels)

    raw = b"".join(
        b"\x00" + b"".join(struct.pack("BBB", *pixel) for pixel in row) for row in pixels
    )

    def chunk(tag: bytes, data: bytes) -> bytes:
        return (
            struct.pack(">I", len(data))
            + tag
            + data
            + struct.pack(">I", zlib.crc32(tag + data) & 0xFFFFFFFF)
        )

    header = struct.pack(">IIBBBBB", WIDTH, HEIGHT, 8, 2, 0, 0, 0)
    png = (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", header)
        + chunk(b"IDAT", zlib.compress(raw, 9))
        + chunk(b"IEND", b"")
    )
    output = Path(__file__).with_name("guide.png")
    output.write_bytes(png)
    print(f"wrote {output} ({len(png)} bytes, {WIDTH}x{HEIGHT})")


if __name__ == "__main__":
    main()
