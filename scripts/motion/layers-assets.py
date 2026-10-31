#!/usr/bin/env python3
"""Deterministic assets for concat-motion's multi-layer example.

Writes, into the directory given as the only argument:

  assets/plate_dark.png    a dark footage plate, gridded so what shows
                           under an overlay is identifiable in a diff
  assets/plate_studio.png  a warm midtone plate, gridded the same way
  assets/leak.png          a radial light leak, white-hot centre fading out
  assets/grain.png         a 256x256 monochrome grain tile
  assets/Archivo.ttf       the faces the example's comps name, copied from
  assets/Inter.ttf         the repository's fonts (variable faces renamed;
  assets/JetBrainsMono.ttf the engine reads them as-is)

Everything is PNG written with the standard library alone - zlib and
struct - and the grain is a fixed LCG seed, so the same script on a CI
runner and on a laptop writes byte-identical assets and the example's
cache key survives a re-run.

usage: python3 scripts/motion/layers-assets.py OUT_DIR
"""

import os
import shutil
import struct
import sys
import zlib


def write_png(path, width, height, rows, alpha=True):
    """rows: one bytes object per row; 6 bytes per pixel when alpha."""

    def chunk(tag, data):
        body = tag + data
        return struct.pack(">I", len(data)) + body + struct.pack(
            ">I", zlib.crc32(body) & 0xFFFFFFFF
        )

    raw = b"".join(b"\x00" + row for row in rows)
    colour = 6 if alpha else 2
    header = struct.pack(">IIBBBBB", width, height, 8, colour, 0, 0, 0)
    with open(path, "wb") as fh:
        fh.write(b"\x89PNG\r\n\x1a\n")
        fh.write(chunk(b"IHDR", header))
        fh.write(chunk(b"IDAT", zlib.compress(raw, 9)))
        fh.write(chunk(b"IEND", b""))


def plate_rows(width, height, top, bottom, grid, grid_alpha):
    """A diagonal gradient with a 240x135 grid drawn over it."""
    rows = []
    for y in range(height):
        t = y / (height - 1)
        row = bytearray(width * (3 if grid_alpha is None else 4))
        for x in range(width):
            xt = x / (width - 1)
            mix = t * 0.75 + xt * 0.25
            base = [int(top[i] + (bottom[i] - top[i]) * mix) for i in range(3)]
            on_grid = (x % 240 < 3) or (y % 135 < 3)
            if on_grid:
                pixel = [
                    int(c * grid_alpha + b * (1 - grid_alpha))
                    for c, b in zip(grid, base)
                ]
            else:
                pixel = base
            at = x * 4
            row[at:at + 3] = bytes(pixel)
            row[at + 3] = 255
        rows.append(bytes(row))
    return rows


def leak_rows(width, height):
    """A warm radial highlight, opaque at the centre, gone at the rim."""
    rows = []
    cx, cy = width * 0.42, height * 0.38
    radius = width * 0.55
    for y in range(height):
        row = bytearray()
        for x in range(width):
            d = ((x - cx) ** 2 + (y - cy) ** 2) ** 0.5 / radius
            a = max(0.0, 1.0 - d) ** 2.2
            row += bytes([255, 244, 214, int(a * 255)])
        rows.append(bytes(row))
    return rows


def grain_rows(size, seed):
    """256x256 of monochrome noise from a fixed LCG: the same tile forever."""
    state = seed
    rows = []
    for _ in range(size):
        row = bytearray()
        for _ in range(size):
            state = (state * 6364136223846793005 + 1442695040888963407) % (1 << 64)
            value = (state >> 33) & 0xFF
            row += bytes([value, value, value, 255])
        rows.append(bytes(row))
    return rows


def main():
    if len(sys.argv) != 2:
        print(__doc__)
        return 2
    out = os.path.join(sys.argv[1], "assets")
    os.makedirs(out, exist_ok=True)

    write_png(
        os.path.join(out, "plate_dark.png"),
        1920,
        1080,
        plate_rows(
            1920, 1080,
            top=(10, 14, 26), bottom=(24, 33, 58),
            grid=(58, 76, 120), grid_alpha=0.75,
        ),
    )
    write_png(
        os.path.join(out, "plate_studio.png"),
        1920, 1080,
        plate_rows(
            1920, 1080,
            top=(58, 44, 34), bottom=(107, 74, 51),
            grid=(150, 110, 70), grid_alpha=0.8,
        ),
    )
    write_png(os.path.join(out, "leak.png"), 1920, 1080, leak_rows(1920, 1080))
    write_png(os.path.join(out, "grain.png"), 256, 256, grain_rows(256, 20261030))

    here = os.path.dirname(os.path.abspath(__file__))
    repo = os.path.dirname(os.path.dirname(here))
    fonts = {
        "Archivo.ttf": "Archivo-Variable.ttf",
        "Inter.ttf": "Inter-Variable.ttf",
        "JetBrainsMono.ttf": "JetBrainsMono-Variable.ttf",
    }
    for want, have in fonts.items():
        src = os.path.join(repo, "assets", "fonts", have)
        dst = os.path.join(out, want)
        if os.path.isfile(src):
            shutil.copyfile(src, dst)
        else:
            print(f"warning: {src} not found; the engine will use its fallback face")
    print("assets written to", out)
    return 0


if __name__ == "__main__":
    sys.exit(main())
