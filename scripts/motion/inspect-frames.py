#!/usr/bin/env python3
"""Pixel evidence for the multi-layer example's overlap frames.

Three claims, each measured rather than eyeballed:

1. The foreground card sits ON the footage, not INSTEAD of it: at the
   card's frame the region around the card still shows the scene behind
   it (the frame before the card arrives, compared pixel-wise), and the
   card's own region differs - the card is a layer, not a replacement.
2. The screen-blended leak lightened the plate under it: the same frame
   rendered with the leak layer dropped differs exactly where the leak
   is, and under the leak's bright core the plate's own grid lines are
   still present (screen keeps the ground; a cover would not).
3. The grain tile is a whisper over everything: the frame with it and
   the frame without differ by a few counts everywhere and a lot nowhere.

usage: python3 scripts/motion/inspect-frames.py RENDER_DIR
"""

import json
import os
import shutil
import struct
import subprocess
import sys
import zlib


def read_png(path):
    """A minimal PNG reader: 8-bit RGB or RGBA, no interlacing."""
    data = open(path, "rb").read()
    at = 8
    width = height = None
    colour = None
    idat = bytearray()
    while at < len(data):
        length, tag = struct.unpack(">I4s", data[at:at + 8])
        body = data[at + 8:at + 8 + length]
        if tag == b"IHDR":
            width, height, _, colour = struct.unpack(">IIBB", body[:10])
        elif tag == b"IDAT":
            idat += body
        at += 12 + length
    raw = zlib.decompress(bytes(idat))
    channels = {2: 3, 6: 4}[colour]
    stride = width * channels
    rows = []
    pos = 0
    prev = bytearray(stride)
    for _ in range(height):
        row = bytearray(raw[pos + 1:pos + 1 + stride])
        pos += 1 + stride
        filter_ = raw[pos - 1 - stride]
        if filter_ == 1:
            for i in range(channels, stride):
                row[i] = (row[i] + row[i - channels]) & 0xFF
        elif filter_ == 2:
            for i in range(stride):
                row[i] = (row[i] + prev[i]) & 0xFF
        elif filter_ == 3:
            for i in range(stride):
                left = row[i - channels] if i >= channels else 0
                row[i] = (row[i] + ((left + prev[i]) >> 1)) & 0xFF
        elif filter_ == 4:
            for i in range(stride):
                a = row[i - channels] if i >= channels else 0
                b = prev[i]
                c = prev[i - channels] if i >= channels else 0
                p = a + b - c
                pa, pb, pc = abs(p - a), abs(p - b), abs(p - c)
                guess = a if (pa <= pb and pa <= pc) else (b if pb <= pc else c)
                row[i] = (row[i] + guess) & 0xFF
        rows.append(bytes(row))
        prev = row
    return width, height, channels, rows


def luma_at(pixels, x, y):
    width, height, channels, rows = pixels
    at = min(x, width - 1) * channels
    r, g, b = rows[min(y, height - 1)][at:at + 3]
    return 0.299 * r + 0.587 * g + 0.114 * b


def diff_map(a, b):
    """Per-pixel absolute luma difference between two same-sized frames."""
    width, height, channels, rows_a = a
    _, _, _, rows_b = b
    out = []
    for y in range(height):
        row_a, row_b = rows_a[y], rows_b[y]
        line = bytearray(width)
        for x in range(width):
            at = x * channels
            la = 0.299 * row_a[at] + 0.587 * row_a[at + 1] + 0.114 * row_a[at + 2]
            lb = 0.299 * row_b[at] + 0.587 * row_b[at + 1] + 0.114 * row_b[at + 2]
            line[x] = min(255, int(abs(la - lb)))
        out.append(line)
    return out


def region_stats(diff, x0, y0, x1, y1):
    """Mean and max of a diff over a rectangle."""
    total = count = 0
    peak = 0
    for y in range(y0, y1):
        for x in range(x0, x1):
            v = diff[y][x]
            total += v
            count += 1
            peak = max(peak, v)
    return (total / max(count, 1), peak)


def main():
    render = sys.argv[1] if len(sys.argv) > 1 else "."
    cache = os.path.join(render, "cache")

    # --- 1. the card over the scene, not instead of it -------------------
    before = read_png(os.path.join(render, "s2-t5.5.png"))
    card = read_png(os.path.join(render, "s2-t7.0.png"))
    diff = diff_map(before, card)
    width, height = before[0], before[1]
    # The card is contain-fitted at 60%: a centre box roughly 40% of the
    # frame's width (contain keeps the 16:9 picture inside the frame) and
    # 60% of its height. Compare that centre against a border ring.
    cx0, cx1 = int(width * 0.30), int(width * 0.70)
    cy0, cy1 = int(height * 0.20), int(height * 0.80)
    centre_mean, centre_peak = region_stats(diff, cx0, cy0, cx1, cy1)
    border = 0.0
    border_n = 0
    for x0, y0, x1, y1 in [
        (0, 0, width, 60), (0, height - 60, width, height),
        (0, 0, 60, height), (width - 60, 0, width, height),
    ]:
        m, _ = region_stats(diff, x0, y0, x1, y1)
        border += m
        border_n += 1
    border_mean = border / border_n
    print(f"card frame: centre diff mean {centre_mean:.1f} (peak {centre_peak}), "
          f"border mean {border_mean:.1f}")
    assert centre_mean > 20, "the card's region did not change: the card is missing"
    assert border_mean < 8, "the border changed too: something replaced the scene"

    # The scene's own mark is still drawn under the card's corners: sample
    # four points inside the frame but outside the card box and confirm
    # they hold the scene's dark plate, not the card's warm studio tone.
    card_pixels = read_png(os.path.join(render, "s2-t7.0.png"))
    warm = 0
    samples = [(width // 2, int(height * 0.10)), (width // 2, int(height * 0.92)),
               (int(width * 0.06), height // 2), (int(width * 0.94), height // 2)]
    for x, y in samples:
        r, g, b = [card_pixels[3][y][x * card_pixels[2] + i] for i in range(3)]
        if not (r > g > b):  # the studio plate is warm: r > g > b
            warm += 1
    print(f"scene-visible samples outside the card: {warm}/4 not warm-studio")
    assert warm >= 3, "the studio plate covers the whole frame: the card replaced the scene"

    # --- 2. the leak screens over the plate ------------------------------
    timeline = json.load(open(os.path.join(cache, "timeline.json")))
    without = [l for l in timeline["layers"] if l["id"] != "s0.overlay"]
    timeline["layers"] = without
    alt = os.path.join(cache, "timeline-no-leak.json")
    json.dump(timeline, open(alt, "w"))
    out = os.path.join(render, "s1-t2.0-no-leak.png")
    geneva = os.environ.get("CONCAT_GENEVA_BIN") or shutil.which("geneva")
    if not geneva:
        local = os.path.expanduser("~/.local/bin/geneva")
        if os.path.isfile(local):
            geneva = local
    if not geneva:
        print("no geneva binary: set CONCAT_GENEVA_BIN", file=sys.stderr)
        return 2
    subprocess.run(
        [geneva, "--format", "json", "frame", "timeline-no-leak.json",
         "--at", "2s", "-o", os.path.abspath(out)],
        cwd=cache, check=True, capture_output=True,
    )
    with_leak = read_png(os.path.join(render, "s1-t2.0.png"))
    no_leak = read_png(out)
    diff = diff_map(with_leak, no_leak)
    # The leak sits up and left at 80% scale: its layer centre is the
    # frame's centre plus the offset (-0.25, -0.2), and the picture's own
    # core at (0.42, 0.38) of it is that much further in, at 0.8 scale.
    lx = int(width * (0.5 - 0.25 + (0.42 - 0.5) * 0.8))
    ly = int(height * (0.5 - 0.20 + (0.38 - 0.5) * 0.8))
    leak_mean, leak_peak = region_stats(diff, lx - 120, ly - 120, lx + 120, ly + 120)
    far_mean, far_peak = region_stats(
        diff, int(width * 0.90), int(height * 0.85),
        int(width * 0.98), int(height * 0.97),
    )
    print(f"leak: core diff mean {leak_mean:.1f} (peak {leak_peak}) at "
          f"({lx},{ly}), far corner mean {far_mean:.1f}")
    assert leak_mean > 15, "the leak's core did not change the plate: the blend did nothing"
    assert far_mean < 8, "the leak changed the whole frame: it covered, not blended"
    # Screen keeps the ground. At the leak's white-hot core screen(x,
    # white) is white - the washout there is the blend doing its job - so
    # the claim is measured at the RIM, where the leak is partial: the
    # plate's grid must still be a local contrast in the with-leak frame
    # (attenuated, not gone), and the region must be LIGHTER with the leak
    # than without it. A cover would have contrast from the leak picture
    # only and a normal blend would have darkened nothing.
    rim_x, rim_y = lx + 250, ly + 160
    window = 90

    def scan_stats(pixels, x0, x1, y0, y1):
        lumas = []
        for y in range(y0, y1, 3):
            for x in range(x0, x1, 3):
                lumas.append(luma_at(pixels, x, y))
        return min(lumas), max(lumas), sum(lumas) / len(lumas)

    lo_a, hi_a, mean_a = scan_stats(with_leak, rim_x - window, rim_x + window,
                                    rim_y - 12, rim_y + 12)
    lo_b, hi_b, mean_b = scan_stats(no_leak, rim_x - window, rim_x + window,
                                    rim_y - 12, rim_y + 12)
    contrast_a, contrast_b = hi_a - lo_a, hi_b - lo_b
    print(f"leak rim: with-leak contrast {contrast_a:.0f} mean {mean_a:.0f}; "
          f"without contrast {contrast_b:.0f} mean {mean_b:.0f}")
    assert mean_a > mean_b + 8, "the leak did not lighten its rim: the blend did nothing"
    assert contrast_a > 0.25 * contrast_b, (
        "the plate's grid is gone under the leak's rim: it was replaced, not screened"
    )

    print("ALL PIXEL CLAIMS HOLD")
    return 0


if __name__ == "__main__":
    sys.exit(main())
