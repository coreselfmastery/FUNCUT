#!/usr/bin/env python3
"""Pixel evidence for the multi-layer example's overlap frames.

Four claims, each measured rather than eyeballed:

1. The foreground card sits ON the footage, not INSTEAD of it: at the
   card's frame the region around the card still shows the scene behind
   it (the frame before the card arrives, compared pixel-wise), and the
   card's own region differs - the card is a layer, not a replacement.
   The comparison is by colour, not brightness alone: the scene's ground
   is a plate whose luma sits near the card's own, so the claim that
   would once have been made in luma is made in the channel that moved.
2. The screen-blended leak lightened the plate under it: the same frame
   rendered with the leak layer dropped differs exactly where the leak
   is, and under the leak's bright core the plate's own grid lines are
   still present (screen keeps the ground; a cover would not).
3. The grain tile is a whisper over everything: the frame with it and
   the frame without differ by a few counts everywhere and a lot nowhere.
4. The grades are worn: the first scene cold, the second warm, each
   rendered again with its grade layer dropped and compared channel by
   channel - the cold scene's blue rises while its red falls, the warm
   scene's red rises far above its blue, and the comp's own words sit
   above the grade and are not tinted by it.

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


def colour_diff_map(a, b):
    """Per-pixel absolute colour difference: the largest single channel
    that moved. Two frames can agree in brightness and disagree utterly
    in colour - a warm card over a blue plate - and this is the eye that
    sees it."""
    width, height, channels, rows_a = a
    _, _, _, rows_b = b
    out = []
    for y in range(height):
        row_a, row_b = rows_a[y], rows_b[y]
        line = bytearray(width)
        for x in range(width):
            at = x * channels
            line[x] = max(abs(row_a[at] - row_b[at]),
                          abs(row_a[at + 1] - row_b[at + 1]),
                          abs(row_a[at + 2] - row_b[at + 2]))
        out.append(line)
    return out


def channel_shift(a, b, region=None):
    """The mean per-channel move from `b` to `a` - graded minus plain -
    over the whole frame or a rectangle (x0, y0, x1, y1). The sign is
    the direction the grade pushed each channel."""
    width, height, channels, rows_a = a
    _, _, _, rows_b = b
    x0, y0, x1, y1 = region or (0, 0, width, height)
    totals = [0, 0, 0]
    count = 0
    for y in range(y0, y1):
        row_a, row_b = rows_a[y], rows_b[y]
        for x in range(x0, x1):
            at = x * channels
            for i in range(3):
                totals[i] += row_a[at + i] - row_b[at + i]
            count += 1
    return [t / max(count, 1) for t in totals]


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
    diff = colour_diff_map(before, card)
    width, height = before[0], before[1]
    # The card is contain-fitted at 60%: a centre box roughly 40% of the
    # frame's width (contain keeps the 16:9 picture inside the frame) and
    # 60% of its height. Compare that centre against a border ring - by
    # colour, because the scene's ground is now a plate whose brightness
    # sits near the card's own and only the channels tell them apart.
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
    # they hold the scene's graded dark plate - blue under its warm grade,
    # as dark plates are - and not the card's warm studio tone.
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

    # --- 3. the grain is a whisper over everything ------------------------
    # The frame with the grain tile and the frame without differ by a
    # few counts everywhere (it is drawn over the whole scene) and a lot
    # nowhere (it is eight percent opacity, not a texture). The
    # timeline is read back from disk: claim 2 rewrote its own copy with
    # the leak dropped, and this one wants the whole document.
    timeline = json.load(open(os.path.join(cache, "timeline.json")))
    alt = os.path.join(cache, "timeline-no-grain.json")
    stripped = [l for l in timeline["layers"] if l["id"] != "s0.overlay2"]
    doc = dict(timeline)
    doc["layers"] = stripped
    json.dump(doc, open(alt, "w"))
    out = os.path.join(render, "s1-t2.0-no-grain.png")
    subprocess.run(
        [geneva, "--format", "json", "frame", os.path.basename(alt),
         "--at", "2s", "-o", os.path.abspath(out)],
        cwd=cache, check=True, capture_output=True,
    )
    with_grain = read_png(os.path.join(render, "s1-t2.0.png"))
    without_grain = read_png(out)
    diff = diff_map(with_grain, without_grain)
    grain_mean, grain_peak = region_stats(diff, 0, 0, width, height)
    print(f"grain: luma diff mean {grain_mean:.1f} (peak {grain_peak})")
    assert grain_mean > 2, "the grain drew nothing: the layer is missing"
    assert grain_mean < 15, "the grain is not a whisper: it is a texture"
    assert grain_peak < 60, "the grain shouts somewhere: a lot nowhere it is not"

    # --- 4. the grades are worn, and the words are not --------------------
    # Each scene's grade layer is dropped from the staged timeline and
    # the same frame rendered again: the cold scene must have moved blue
    # up and red down, the warm scene red up far above blue, and the
    # wordmark - painted above the grade - must have kept its ink.
    ids = [layer["id"] for layer in timeline["layers"]]
    for grade_id in ("s0.funcut-grade", "s1.funcut-grade"):
        assert grade_id in ids, f"the plan baked no {grade_id}: the grade never landed"
    for scene, frame, at in [
        ("s0", "s1-t2.0.png", "2s"),      # the hook: cold
        ("s1", "s2-t7.0.png", "7s"),      # the payoff: warm
    ]:
        alt = os.path.join(cache, f"timeline-no-grade-{scene}.json")
        stripped = [l for l in timeline["layers"]
                    if l["id"] != f"{scene}.funcut-grade"]
        doc = dict(timeline)
        doc["layers"] = stripped
        json.dump(doc, open(alt, "w"))
        out = os.path.join(render, f"no-grade-{frame}")
        subprocess.run(
            [geneva, "--format", "json", "frame", os.path.basename(alt),
             "--at", at, "-o", os.path.abspath(out)],
            cwd=cache, check=True, capture_output=True,
        )
        graded = read_png(os.path.join(render, frame))
        plain = read_png(out)
        dr, dg, db = channel_shift(graded, plain)
        temperature = "cold" if scene == "s0" else "warm"
        print(f"grade {temperature}: dR {dr:+.1f} dG {dg:+.1f} dB {db:+.1f}")
        if scene == "s0":
            assert dr < -8 and db > 3, (
                f"the cold grade did not cool the frame: dR {dr:+.1f} dB {db:+.1f}"
            )
            assert db - dr > 10, (
                f"the cold grade's blue did not rise above its red: dB {db:+.1f} dR {dr:+.1f}"
            )
        else:
            assert dr > 15, f"the warm grade did not warm the frame: dR {dr:+.1f}"
            assert dr - db > 15, (
                f"the warm grade's red did not rise above its blue: dR {dr:+.1f} dB {db:+.1f}"
            )
    # The comp's own words sit above the grade and keep their ink: the
    # wordmark's ivory core (the comp's #f4f1ea ink) is as bright with
    # the grade as without it, give or take the anti-aliasing at its
    # edges - a count over 235 catches the ink and nothing the dark
    # plate ever shows.
    graded = read_png(os.path.join(render, "s1-t2.0.png"))
    plain = read_png(os.path.join(render, "no-grade-s1-t2.0.png"))

    def ink_count(pixels):
        width, height, channels, rows = pixels
        return sum(
            1
            for y in range(0, height, 2)
            for x in range(0, width, 2)
            if min(rows[y][x * channels + i] for i in range(3)) >= 235
        )

    ink_with = ink_count(graded)
    ink_without = ink_count(plain)
    share = ink_with / ink_without if ink_without else float(ink_with == 0)
    print(f"wordmark's ivory core: {ink_with} sampled with the grade, "
          f"{ink_without} without")
    assert share > 0.97, (
        f"the grade tinted the comp's own words: {ink_with} ink pixels with, "
        f"{ink_without} without"
    )

    print("ALL PIXEL CLAIMS HOLD")
    return 0


if __name__ == "__main__":
    sys.exit(main())
