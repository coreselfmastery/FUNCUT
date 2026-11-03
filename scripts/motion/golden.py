#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-or-later
# SPDX-FileCopyrightText: 2026 Jareer Abdullah and Concat contributors
"""golden.py - the visual regression suite: golden frames, held with DSSIM.

The unit tests prove the code. The inspection scripts prove one render's
claims. This one proves the *engine's determinism* the way the contract
promises it: a fixed, committed scene - the multi-layer, multi-cut board
in `golden/board.json` - renders at fixed timestamps, and the frames are
compared against committed golden images with a structural-similarity
metric. A change that shifts a layer by two pixels, drops a font, kills
an entrance or breaks a choreographed cut's shared window turns the
metric and the suite with it, whatever the unit tests say - and an
honest, intended change is `--update`, said in the commit that made it.

What the board exercises, one frame each:

    open    0.8s   the chapter chip settled: the gridded plate, the
                    pulse's bump, the wobbling drift
    whip    1.88s  the whip pan's SHARED WINDOW - both scenes on screen
                    at once, the outgoing leaving left, the incoming
                    arriving right; the frame the temporal-overlap fix
                    exists to make possible
    land    2.3s   the wordmark settled after the cut
    rack    3.6s   the rack focus mid-window: outgoing blurring out,
                    incoming resolving in
    settle  5.0s   the stat cards over the second wobbling plate

The metric is a DSSIM in the classic single-scale block-SSIM shape:
8x8 blocks, the luma channel, `1 - mean(SSIM)` reported per frame. Zero
is identical pixels; the gate holds at THRESHOLD (0.005 by default - a
font hinting difference nudges it, a missing layer clears it).

The determinism claim itself is proven twice over on every run: the
`whip` frame renders TWO TIMES and the two PNGs must be the same bytes -
same machine, same binary, same seed, same PNG - because "bit-identical
across runs" is the first half of the contract and nothing else in the
suite would notice if it quietly stopped being true.

Usage:
    python3 scripts/motion/golden.py --agent src/target/debug/concat-motion-agent
    python3 scripts/motion/golden.py --agent ... --update      # re-write the goldens
    python3 scripts/motion/golden.py --agent ... --threshold 0.005

The script needs nothing but the standard library and the sidecar the
agent itself drives; the plates are generated here, deterministically,
and the fonts are the repository's own, staged as assets so the render
is the same on every machine that has the repo - which is the point.
"""

import argparse
import hashlib
import json
import os
import shutil
import struct
import subprocess
import sys
import tempfile
import zlib

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(os.path.dirname(HERE))
GOLDEN = os.path.join(HERE, "golden")
BOARD = os.path.join(GOLDEN, "board.json")

# The frames the suite holds, and the second of the board each sits at.
FRAMES = [
    ("open", 0.8),
    ("whip", 1.88),
    ("land", 2.3),
    ("rack", 3.6),
    ("settle", 5.0),
    ("grain", 6.6),
]

WIDTH, HEIGHT = 640, 360


def build_assets(out):
    """The board's pictures and fonts, written and copied into `out`.

    The plates are gridded - a bright line every thirty-two pixels on two
    different tones - because a plate that is one flat colour cannot
    show a wobble, a pan or a shift: the grid makes every pixel of the
    frame say where it came from. Everything is generated from a fixed
    seed with no dependency on the machine, and the fonts are copied
    from the repository's own `assets/fonts/`, so a render here and a
    render on a CI runner read the same bytes.
    """
    write_png(os.path.join(out, "grid_a.png"), grid_rows(0x2A1F3D, 0x120E1A, 20261030))
    write_png(os.path.join(out, "grid_b.png"), grid_rows(0x1F2A38, 0x0E121A, 20261031))
    fonts = {
        "Inter.ttf": "Inter-Variable.ttf",
        "Archivo.ttf": "Archivo-Variable.ttf",
    }
    for want, have in fonts.items():
        src = os.path.join(REPO, "assets", "fonts", have)
        dst = os.path.join(out, want)
        if os.path.isfile(src):
            shutil.copyfile(src, dst)
        else:
            print(f"warning: {src} not found; the engine will use its fallback face")


def grid_rows(dark, dim, seed):
    """Two tones and a bright line every thirty-two pixels, from an LCG
    that is fixed by `seed`: deterministic, uninteresting, and readable."""
    rows = []
    state = seed
    for y in range(HEIGHT):
        row = bytearray(WIDTH * 3)
        for x in range(WIDTH):
            state = (state * 6364136223846793005 + 1442695040888963407) & (1 << 64) - 1
            noise = (state >> 33) % 5
            colour = dark if (x // 32 + y // 32) % 2 == 0 else dim
            if x % 32 == 0 or y % 32 == 0:
                colour = ((colour >> 16) + 64) << 16 | (colour >> 8 & 0xFF) << 8 | (colour & 0xFF)
            for channel in range(3):
                shift = 16 - 8 * channel
                value = max(0, min(255, ((colour >> shift) & 0xFF) + noise - 2))
                row[x * 3 + channel] = value
        rows.append(bytes(row))
    return rows


def write_png(path, rows):
    """A minimal 8-bit RGB PNG writer: the standard library's zlib, one
    IDAT, no interlace - the same shape every other motion script uses,
    so nothing here depends on an imaging package being installed."""
    height = len(rows)
    raw = b"".join(b"\x00" + row for row in rows)
    def chunk(tag, data):
        head = struct.pack(">I", len(data)) + tag + data
        return head + struct.pack(">I", zlib.crc32(tag + data) & 0xFFFFFFFF)
    png = b"\x89PNG\r\n\x1a\n"
    png += chunk(b"IHDR", struct.pack(">IIBBBBB", WIDTH, height, 8, 2, 0, 0, 0))
    png += chunk(b"IDAT", zlib.compress(raw, 9))
    png += chunk(b"IEND", b"")
    with open(path, "wb") as handle:
        handle.write(png)


def read_png_luma(path):
    """The PNG a render wrote, as one luma plane: a decoder for the one
    shape the sidecar writes (8-bit, RGB or RGBA, non-interlaced), with
    the five filters implemented honestly because a scanline filter
    skimped is a scanline wrong."""
    data = open(path, "rb").read()
    if data[:8] != b"\x89PNG\r\n\x1a\n":
        raise ValueError(f"{path}: not a PNG")
    at, width, height, depth, colour = 8, 0, 0, 0, 0
    idat = b""
    while at < len(data):
        length = struct.unpack(">I", data[at:at + 4])[0]
        tag = data[at + 4:at + 8]
        body = data[at + 8:at + 8 + length]
        if tag == b"IHDR":
            width, height, depth, colour = struct.unpack(">IIBB", body[:10])
        elif tag == b"IDAT":
            idat += body
        at += 12 + length
    if depth != 8 or colour not in (2, 6):
        raise ValueError(f"{path}: bit depth {depth}, colour type {colour} - not the 8-bit RGB(A) this reads")
    channels = 3 if colour == 2 else 4
    stride = width * channels
    raw = zlib.decompress(idat)
    rows, previous = [], bytearray(stride)
    position = 0
    for y in range(height):
        filter_byte = raw[position]
        position += 1
        row = bytearray(raw[position:position + stride])
        position += stride
        if filter_byte == 1:
            for i in range(channels, stride):
                row[i] = (row[i] + row[i - channels]) & 0xFF
        elif filter_byte == 2:
            for i in range(stride):
                row[i] = (row[i] + previous[i]) & 0xFF
        elif filter_byte == 3:
            for i in range(stride):
                left = row[i - channels] if i >= channels else 0
                row[i] = (row[i] + ((left + previous[i]) >> 1)) & 0xFF
        elif filter_byte == 4:
            for i in range(stride):
                left = row[i - channels] if i >= channels else 0
                up = previous[i]
                corner = previous[i - channels] if i >= channels else 0
                p = left + up - corner
                pa, pb, pc = abs(p - left), abs(p - up), abs(p - corner)
                predictor = left if pa <= pb and pa <= pc else (up if pb <= pc else corner)
                row[i] = (row[i] + predictor) & 0xFF
        rows.append(row)
        previous = row
    luma = []
    for row in rows:
        line = []
        for x in range(width):
            r, g, b = row[x * channels], row[x * channels + 1], row[x * channels + 2]
            # Rec. 709 luma, the channel the metric reads.
            line.append(0.2126 * r + 0.7152 * g + 0.0722 * b)
        luma.append(line)
    return luma, width, height


def dssim(left, right):
    """A DSSIM in the classic single-scale block-SSIM shape: the luma
    planes cut into 8x8 blocks, each block's SSIM taken with the usual
    stabilising constants, `1 - mean(SSIM)` reported. Zero is identical;
    the scale is the familiar one where a threshold of 0.005 is "the
    same picture" and a missing layer is a hundred times that."""
    _, width, height = left
    c1, c2 = (0.01 ** 2), (0.03 ** 2)
    total, blocks = 0.0, 0
    for by in range(0, height - 7, 8):
        for bx in range(0, width - 7, 8):
            mu_x = mu_y = xx = yy = xy = 0.0
            for y in range(by, by + 8):
                row_x, row_y = left[0][y], right[0][y]
                for x in range(bx, bx + 8):
                    a, b = row_x[x] / 255.0, row_y[x] / 255.0
                    mu_x += a
                    mu_y += b
                    xx += a * a
                    yy += b * b
                    xy += a * b
            n = 64.0
            mu_x /= n
            mu_y /= n
            variance_x = xx / n - mu_x * mu_x
            variance_y = yy / n - mu_y * mu_y
            covariance = xy / n - mu_x * mu_y
            ssim = ((2 * mu_x * mu_y + c1) * (2 * covariance + c2)) / (
                (mu_x * mu_x + mu_y * mu_y + c1) * (variance_x + variance_y + c2)
            )
            total += 1.0 - ssim
            blocks += 1
    return total / max(blocks, 1)


def render_frame(agent, board_dir, at, out):
    """One still of the golden board, through the agent, asserted ok."""
    reply = subprocess.run(
        [agent, "render", "board.json", "--frame", str(at), "-o", out, "--cache", "cache-frame"],
        cwd=board_dir,
        capture_output=True,
        text=True,
    )
    if reply.returncode != 0:
        print(reply.stdout or reply.stderr)
        raise SystemExit(f"the frame at {at}s refused to render")
    document = json.loads(reply.stdout)
    if not document.get("ok"):
        raise SystemExit(f"the frame at {at}s did not render ok: {document}")
    if not os.path.isfile(out):
        raise SystemExit(f"the frame at {at}s rendered nothing")
    return document


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--agent", required=True, help="the concat-motion-agent binary to drive")
    parser.add_argument("--update", action="store_true", help="write the goldens instead of holding them")
    parser.add_argument("--threshold", type=float, default=0.005, help="the DSSIM a frame must beat")
    args = parser.parse_args()
    agent = os.path.abspath(args.agent)
    if not os.path.isfile(agent):
        raise SystemExit(f"no agent binary at {agent}; build it: cargo build -p concat-motion --bin concat-motion-agent")

    with tempfile.TemporaryDirectory(prefix="golden-") as scratch:
        board_dir = os.path.join(scratch, "board")
        os.makedirs(board_dir)
        shutil.copyfile(BOARD, os.path.join(board_dir, "board.json"))
        build_assets(board_dir)

        worst = 0.0
        for name, at in FRAMES:
            path = os.path.join(scratch, f"{name}.png")
            document = render_frame(agent, board_dir, at, path)
            del document
            if name == "whip":
                # The determinism claim, proven per run: the same frame,
                # rendered twice, must be the same PNG bytes - same
                # machine, same binary, same seed. A render that drifts
                # inside a single machine is a render the cross-machine
                # promise cannot begin to make.
                twin = os.path.join(scratch, "whip-twin.png")
                render_frame(agent, board_dir, at, twin)
                first = hashlib.sha256(open(path, "rb").read()).hexdigest()
                second = hashlib.sha256(open(twin, "rb").read()).hexdigest()
                if first != second:
                    raise SystemExit(
                        f"the whip frame rendered twice is not the same PNG: {first[:16]} vs {second[:16]} - "
                        "the engine has stopped being deterministic on this machine"
                    )
                print(f"    determinism: the whip frame twice, byte-identical ({first[:16]})")

            golden_path = os.path.join(GOLDEN, f"{name}.png")
            if args.update:
                shutil.copyfile(path, golden_path)
                print(f"    golden written: {name}.png")
                continue
            if not os.path.isfile(golden_path):
                raise SystemExit(
                    f"{name}.png has no golden to hold it against - run with --update and commit the goldens"
                )
            score = dssim(read_png_luma(golden_path), read_png_luma(path))
            worst = max(worst, score)
            verdict = "holds" if score <= args.threshold else "BROKE"
            print(f"    {name:7} at {at:>5}s: DSSIM {score:.5f} (threshold {args.threshold}) {verdict}")
            if score > args.threshold:
                raise SystemExit(
                    f"the {name} frame broke its golden: DSSIM {score:.5f} over {args.threshold}. "
                    "If the change is intended, --update and say so in the commit"
                )

        if args.update:
            print("goldens updated; commit them with the change that made them move")
        else:
            print(f"every golden frame holds (worst DSSIM {worst:.5f})")


if __name__ == "__main__":
    sys.exit(main() or 0)
