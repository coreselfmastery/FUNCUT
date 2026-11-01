#!/usr/bin/env python3
"""Pixel evidence that one layout rendered in three styles is three scenes.

The style example renders the same stat-card preset as calm, punchy and
editorial over the same plate. Three claims, each measured rather than
eyeballed:

1. Three visibly different scenes: at the rising moment and at the
   settled one, every pair of personalities differs by more colour than
   a dither - the typography, the grade and the motion state together.
2. The colour runs the way the personalities say: on the plate, punchy
   is hot (red above blue), calm is cool (blue above red), editorial
   keeps the plate's own cool order with no cast at all - and its
   crushed shadows sit darker than calm's lifted ones.
3. The tempo is the personality's: punchy has landed while the others
   are still moving, because 0.72 times the rhythm arrives first.

usage: python3 scripts/motion/inspect-styles.py RENDER_DIR
"""

import importlib.util
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
FRAMES = os.path.join(HERE, "inspect-frames.py")
spec = importlib.util.spec_from_file_location("inspect_frames", FRAMES)
frames = importlib.util.module_from_spec(spec)
spec.loader.exec_module(frames)

read_png = frames.read_png
colour_diff_map = frames.colour_diff_map
region_stats = frames.region_stats


def card_region(pixels):
    """The stat cards' own span, minus the frame's border."""
    width, height = pixels[0], pixels[1]
    return (180, 300, width - 180, 760)


def plate_row(pixels):
    """A row of the plate above the cards, as channel means."""
    width, _, channels, rows = pixels
    count = width // 4
    r = sum(rows[40][x * channels] for x in range(0, width, 4)) / count
    g = sum(rows[40][x * channels + 1] for x in range(0, width, 4)) / count
    b = sum(rows[40][x * channels + 2] for x in range(0, width, 4)) / count
    return r, g, b


def luma(rgb):
    return 0.299 * rgb[0] + 0.587 * rgb[1] + 0.114 * rgb[2]


def main():
    render = sys.argv[1] if len(sys.argv) > 1 else "."
    names = ("calm", "punchy", "editorial")
    settled = {n: read_png(os.path.join(render, f"style-{n}.png")) for n in names}
    rising = {n: read_png(os.path.join(render, f"rise-{n}.png")) for n in names}

    # --- 1. three visibly different scenes ------------------------------
    for moment, book in (("settled", settled), ("rising", rising)):
        for a, b in (("calm", "punchy"), ("calm", "editorial"), ("punchy", "editorial")):
            diff = colour_diff_map(book[a], book[b])
            x0, y0, x1, y1 = card_region(book[a])
            mean, peak = region_stats(diff, x0, y0, x1, y1)
            print(f"{moment:8s} {a:9s} vs {b:9s}: card colour diff mean {mean:5.1f} (peak {peak})")
            assert mean > 8, (
                f"{a} and {b} are the same scene at the {moment} moment: "
                f"the style changed nothing a eye could see (diff {mean:.1f})"
            )

    # --- 2. the colour runs the way the personalities say ---------------
    plate = {n: plate_row(settled[n]) for n in names}
    for n in names:
        r, g, b = plate[n]
        print(f"{n:9s} plate: R {r:5.1f} G {g:5.1f} B {b:5.1f}")
    assert plate["punchy"][0] - plate["punchy"][2] > 20, (
        "punchy is not hot: its red does not sit above its blue"
    )
    assert plate["calm"][2] - plate["calm"][0] > 15, (
        "calm is not cool: its blue does not sit above its red"
    )
    assert plate["editorial"][2] - plate["editorial"][0] > 0, (
        "editorial wears a cast: its channel order left the plate's own"
    )
    assert luma(plate["editorial"]) < luma(plate["calm"]), (
        "editorial's shadows are not crushed: its contrast move did nothing"
    )

    # --- 3. the tempo is the personality's -------------------------------
    # Punchy's cards have landed by 0.6 s - 0.72 times the rhythm arrives
    # first - so its own rising-versus-settled difference is the smallest
    # of the three, and smaller than the still-moving calm and editorial.
    self_diff = {}
    for n in names:
        diff = colour_diff_map(rising[n], settled[n])
        x0, y0, x1, y1 = card_region(settled[n])
        mean, _ = region_stats(diff, x0, y0, x1, y1)
        self_diff[n] = mean
        print(f"{n:9s} own rise-vs-settled diff: {mean:5.1f}")
    assert self_diff["punchy"] < 10, (
        "punchy had not landed at the rising moment: the tempo did not hurry it"
    )
    assert self_diff["calm"] > 12 and self_diff["editorial"] > 12, (
        "the others had already landed: the tempo hurried them too"
    )

    print("ALL STYLE CLAIMS HOLD")
    return 0


if __name__ == "__main__":
    sys.exit(main())
