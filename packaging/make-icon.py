#!/usr/bin/env python3
"""Generate the BeatByte app icon as a PNG — procedurally, no deps.

Draws a pixel-art icon: deep-navy rounded tile, a chunky yellow "B",
and falling note squares in the five lane colors. Writes icon.png
(1024x1024) next to this script. The PNG encoder is hand-rolled
(zlib + struct are in the standard library), keeping the repository
free of binary sources.
"""

import struct
import zlib
from pathlib import Path

SIZE = 1024
CELL = SIZE // 16  # 16x16 pixel-art grid

NAVY = (11, 11, 22, 255)
NAVY_LIGHT = (19, 20, 33, 255)
BRAND = (255, 217, 64, 255)
LANES = [
    (61, 219, 133, 255),
    (255, 82, 82, 255),
    (255, 214, 64, 255),
    (64, 196, 255, 255),
    (255, 171, 64, 255),
]

# A chunky 8x10 "B" on the 16x16 grid (col, row) cells.
B_CELLS = [
    (4, 3), (5, 3), (6, 3), (7, 3), (8, 3),
    (4, 4), (8, 4), (9, 4),
    (4, 5), (8, 5), (9, 5),
    (4, 6), (5, 6), (6, 6), (7, 6), (8, 6),
    (4, 7), (8, 7), (9, 7),
    (4, 8), (9, 8),
    (4, 9), (8, 9), (9, 9),
    (4, 10), (5, 10), (6, 10), (7, 10), (8, 10),
]

# Falling note squares (col, row, lane-color-index).
NOTES = [
    (11, 2, 0), (12, 5, 3), (11, 8, 1), (12, 11, 4), (11, 12, 2),
    (2, 12, 3), (2, 5, 4),
]


def build_pixels():
    px = [[NAVY for _ in range(SIZE)] for _ in range(SIZE)]
    # Rounded tile: cut the outer cells at the corners.
    corner = CELL
    for y in range(SIZE):
        for x in range(SIZE):
            in_corner = (
                (x < corner and y < corner)
                or (x >= SIZE - corner and y < corner)
                or (x < corner and y >= SIZE - corner)
                or (x >= SIZE - corner and y >= SIZE - corner)
            )
            if in_corner:
                px[y][x] = (0, 0, 0, 0)
    # Subtle top glow rows.
    for y in range(CELL, 3 * CELL):
        for x in range(CELL, SIZE - CELL):
            px[y][x] = NAVY_LIGHT

    def cell(col, row, color, inset=6):
        x0, y0 = col * CELL + inset, row * CELL + inset
        x1, y1 = (col + 1) * CELL - inset, (row + 1) * CELL - inset
        for y in range(y0, y1):
            for x in range(x0, x1):
                px[y][x] = color

    for col, row in B_CELLS:
        cell(col, row, BRAND, inset=3)
    for col, row, lane in NOTES:
        cell(col, row, LANES[lane], inset=10)
    return px


def write_png(path, px):
    raw = b"".join(
        b"\x00" + b"".join(struct.pack("4B", *px[y][x]) for x in range(SIZE))
        for y in range(SIZE)
    )

    def chunk(tag, data):
        block = tag + data
        return struct.pack(">I", len(data)) + block + struct.pack(
            ">I", zlib.crc32(block) & 0xFFFFFFFF
        )

    header = struct.pack(">IIBBBBB", SIZE, SIZE, 8, 6, 0, 0, 0)
    png = (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", header)
        + chunk(b"IDAT", zlib.compress(raw, 9))
        + chunk(b"IEND", b"")
    )
    path.write_bytes(png)


if __name__ == "__main__":
    out = Path(__file__).parent / "icon.png"
    write_png(out, build_pixels())
    print(f"wrote {out} ({out.stat().st_size} bytes)")
