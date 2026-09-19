#!/usr/bin/env python3
"""Build all BeatByte desktop icon assets from bb-app-icon.png.

Developer tool: requires Pillow. Release packaging consumes the generated,
committed assets and therefore does not need Pillow on CI.
"""

from __future__ import annotations

import math
import shutil
import struct
import subprocess
from pathlib import Path

from PIL import Image

ROOT = Path(__file__).parent
SOURCE = ROOT / "bb-app-icon.png"
MASTER = ROOT / "icon.png"
ICONS = ROOT / "icons"
MASTER_SIZE = 1024
LINUX_SIZES = (16, 32, 48, 64, 128, 256, 512)
WINDOWS_SIZES = (16, 24, 32, 48, 64, 128, 256)
MAC_SIZES = (16, 32, 64, 128, 256, 512)


def rounded_rect_distance(x: float, y: float) -> float:
    """Signed distance to the measured opaque face in the source artwork."""
    x0, y0 = 0.073 * MASTER_SIZE, 0.064 * MASTER_SIZE
    x1, y1 = 0.928 * MASTER_SIZE, 0.937 * MASTER_SIZE
    radius = 0.19 * MASTER_SIZE
    cx, cy = (x0 + x1) / 2, (y0 + y1) / 2
    qx = abs(x - cx) - ((x1 - x0) / 2 - radius)
    qy = abs(y - cy) - ((y1 - y0) / 2 - radius)
    return math.hypot(max(qx, 0.0), max(qy, 0.0)) + min(max(qx, qy), 0.0) - radius


def build_master() -> Image.Image:
    source = Image.open(SOURCE).convert("RGB").resize(
        (MASTER_SIZE, MASTER_SIZE), Image.Resampling.LANCZOS
    )
    pixels = source.load()
    rgba = Image.new("RGBA", source.size, (0, 0, 0, 0))
    output = rgba.load()
    for y in range(MASTER_SIZE):
        for x in range(MASTER_SIZE):
            r, g, b = pixels[x, y]
            distance = rounded_rect_distance(x + 0.5, y + 0.5)
            if distance <= -2.0:
                alpha = 255
            elif distance < 2.0:
                alpha = round(255 * (2.0 - distance) / 4.0)
            else:
                # Outside the face, only coloured neon energy survives.
                # Pure/near black becomes fully transparent with RGB zero.
                alpha = max(0, min(190, round((max(r, g, b) - 4) * 2.2)))
            output[x, y] = (r, g, b, alpha) if alpha else (0, 0, 0, 0)
    return rgba


def save_png(image: Image.Image, path: Path, size: int) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    resized = image.resize((size, size), Image.Resampling.LANCZOS)
    if size <= 48:
        # A light sharpen keeps the b stem and equalizer bars separate at
        # launcher/taskbar sizes without redrawing or changing the mark.
        from PIL import ImageFilter

        resized = resized.filter(ImageFilter.UnsharpMask(radius=0.65, percent=145, threshold=2))
    # Resampling can leave invisible colour data below fully transparent
    # pixels. Clear it so launchers never reveal a dark fringe.
    pixels = resized.load()
    for y in range(size):
        for x in range(size):
            if pixels[x, y][3] == 0:
                pixels[x, y] = (0, 0, 0, 0)
    resized.save(path, "PNG", optimize=True)


def write_ico(path: Path, entries: list[tuple[int, bytes]]) -> None:
    """ICO container with one PNG payload per requested size."""
    offset = 6 + 16 * len(entries)
    directory = bytearray(struct.pack("<HHH", 0, 1, len(entries)))
    payload = bytearray()
    for size, png in entries:
        dimension = 0 if size == 256 else size
        directory.extend(
            struct.pack(
                "<BBBBHHII", dimension, dimension, 0, 0, 1, 32, len(png), offset
            )
        )
        payload.extend(png)
        offset += len(png)
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(directory + payload)


def main() -> None:
    if not SOURCE.is_file():
        raise SystemExit(f"missing source artwork: {SOURCE}")
    master = build_master()
    master.save(MASTER, "PNG", optimize=True)

    for size in LINUX_SIZES:
        save_png(master, ICONS / "linux" / f"beatbyte-{size}.png", size)

    mac = ICONS / "macos" / "BeatByte.iconset"
    for size in MAC_SIZES:
        save_png(master, mac / f"icon_{size}x{size}.png", size)
        save_png(master, mac / f"icon_{size}x{size}@2x.png", size * 2)

    windows_entries = []
    for size in WINDOWS_SIZES:
        png_path = ICONS / "windows" / f"beatbyte-{size}.png"
        save_png(master, png_path, size)
        windows_entries.append((size, png_path.read_bytes()))
    write_ico(ICONS / "windows" / "BeatByte.ico", windows_entries)

    iconutil = shutil.which("iconutil")
    if iconutil:
        subprocess.run(
            [iconutil, "-c", "icns", str(mac), "-o", str(ICONS / "macos" / "BeatByte.icns")],
            check=True,
        )
    print(f"wrote RGBA master {MASTER} and platform icons in {ICONS}")


if __name__ == "__main__":
    main()
