#!/usr/bin/env python3
"""Generate the BeatByte app icon as a PNG — procedurally, no deps.

Draws the icon: deep-navy rounded tile with faint lane guides, a
chunky yellow "B", and the five round gems (green red yellow blue
orange, white core + dark ring — the game's receptor row) along the
bottom. Writes icon.png (1024x1024) next to this script. The PNG
encoder is hand-rolled (zlib + struct are in the standard library),
keeping the repository free of binary sources.
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

# A few falling note squares above the gem row (col, row, lane).
NOTES = [
    (11, 3, 3), (2, 5, 4), (12, 6, 0), (3, 9, 1),
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

    # Faint vertical lane guides behind everything.
    for lane in range(5):
        gx = int((lane + 1.5) * SIZE / 8)
        for y in range(CELL, SIZE - CELL):
            for x in range(gx - 2, gx + 3):
                if px[y][x][3] != 0 and px[y][x] != NAVY_LIGHT:
                    px[y][x] = (24, 25, 40, 255)

    for col, row in B_CELLS:
        cell(col, row, BRAND, inset=3)
    for col, row, lane in NOTES:
        cell(col, row, LANES[lane], inset=10)

    def disc(cx, cy, radius, color):
        # Anti-aliased filled circle blended onto the tile.
        for y in range(int(cy - radius - 2), int(cy + radius + 3)):
            for x in range(int(cx - radius - 2), int(cx + radius + 3)):
                if not (0 <= x < SIZE and 0 <= y < SIZE) or px[y][x][3] == 0:
                    continue
                d = ((x - cx) ** 2 + (y - cy) ** 2) ** 0.5
                a = max(0.0, min(1.0, radius - d + 0.5))
                if a <= 0.0:
                    continue
                br, bg, bb, _ = px[y][x]
                r, g, b, _ = color
                px[y][x] = (
                    int(r * a + br * (1 - a)),
                    int(g * a + bg * (1 - a)),
                    int(b * a + bb * (1 - a)),
                    255,
                )

    # The receptor row: five round gems, white core, dark ring.
    gem_y = SIZE * 13.1 / 16
    radius = SIZE * 0.052
    for lane, color in enumerate(LANES):
        gem_x = (lane + 1.5) * SIZE / 8
        disc(gem_x, gem_y, radius, color)
        disc(gem_x, gem_y, radius * 0.72, tuple(int(c * 0.35) for c in color[:3]) + (255,))
        disc(gem_x, gem_y, radius * 0.55, color)
        disc(gem_x, gem_y, radius * 0.28, (255, 255, 255, 255))
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
