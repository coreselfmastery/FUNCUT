#!/usr/bin/env python3
"""concat-visual-sense: what a shot actually DOES, measured.

Pure Python standard library, the same contract as the other sense
scripts: `visual_sense.py <analyse|selftest> --spec -` reads one JSON
spec from stdin and prints one JSON object on stdout. The spec names the
tool in `tool`.

WHY THIS EXISTS

The editor already measures the voice and the script. It does not
measure the picture. A cut is placed on a pause or a phrase boundary,
stock is chosen by one still frame and a caption, and motion design is
sized by the delivery alone. Nothing in the run knows whether the shot
moved, whether the camera panned, whether the frame is soft or noisy, or
whether a cut lands on the moment something happened on screen.

This module is the missing eye. It measures the frames the editor
already can produce and answers the questions an edit actually asks:

  measure_motion   per-frame optical flow (global and local), its peak
                   and its onsets - the visual equivalent of the audio
                   flux onset
  camera_motion    pan / tilt / zoom / static, so a shot already moving
                   is not given a redundant Ken Burns push
  shot_scale       wide / medium / close-up, derived from the subject's
                   measured area (the editing primitive that did not
                   exist)
  visual_sync      does this sound land on that movement? audio onsets
                   correlated with motion onsets
  scene_cuts       shot boundaries from frame change, merged with
                   analyze_scenes' own cuts when they are given
  visual_quality   sharpness, noise, brightness, contrast, letterbox,
                   blown highlights - the technical veto
  palette          dominant colours, temperature, saturation - a real
                   seam and a style match
  saliency         a classical center-surround attention map and its
                   peak, with no learned model
  measure_clips    the compact batch read stock ranking spends
  bench            does cutting on a motion peak beat cutting on a
                   pause? the measurement the wiring must answer
  models           the visual model doors and whether each file is on
                   disk
  action_class     MoViNet temporal action door (ONNX)
  depth_map        Depth Anything V2 door (ONNX)
  subject_mask     FastSAM/SAM door (ONNX)
  visual_embedding DINOv2 / CLIP door (ONNX)
  describe_scene   a bounded VLM description through the existing
                   local_vlm helper

WHAT IT IS NOT

No model is downloaded and no model is required. The classical tools
run anywhere Python runs, on any device, and their cost is bounded by
the frame width the caller asks for. The ONNX doors are real doors: they
run when the model file and `onnxruntime` + `numpy` are present, and
they refuse with the reason when they are not. A door that cannot run
never returns a zero-filled reading pretending to be one.

Everything degrades loudly: frames that cannot be decoded are an error
with the reason, never a made-up measurement.
"""

from __future__ import annotations

import argparse
import base64
import json
import math
import os
import shutil
import struct
import subprocess
import sys
import tempfile
import zlib
from pathlib import Path

try:  # optional acceleration and the ONNX doors
    import numpy as np
except Exception:  # pragma: no cover - the classical tools need no numpy
    np = None

try:  # the existing local VLM helper, for describe_scene only
    import vlm as _VLM
except Exception:  # pragma: no cover
    _VLM = None

# The default width every measurement runs at. A 64-pixel work frame is
# enough to see a camera pan and a subject move, and small enough that a
# six-frame read of a stock candidate costs milliseconds on a laptop.
WORK_WIDTH = 64
# Below this the frame is treated as still: 0.004 of the frame width per
# second is a slow drift, not an entrance.
STILL = 0.004
# A cut is a change this much above the clip's own median change.
CUT_FLOOR = 0.12

# What each reading in a `measure_clips` row is spent on, in the words the
# program's own guard checks. A row key with no entry here is a
# measurement the edit paid for and threw away - the audio layer's
# recurring bug, moved to the picture. `motion.*` names the sub-keys the
# cut spends; the rest of the motion row is the bench's evidence.
CONSUMERS = {
    "motion": "the in-point (peakAt), the drift veto (mean, still) and the "
              "continuity gate's travel (onsets are the SFX sync evidence)",
    "camera": "the continuity gate's direction and the drift veto: a shot "
              "already panning gets no added push",
    "shotScale": "the cut rhythm and the drift: a close-up holds, a wide "
                 "shot pushes, two of one scale in a row is flagged",
    "quality": "the stock veto: a soft, noisy, letterboxed, flat, dark or "
               "blown candidate sinks behind one that passed",
    "palette": "recorded, not spent: the sheet's own colour temperature is "
               "the continuity seam today; a palette seam is a later bench",
    "saliency": "the caption band: words move away from where the eye is",
    "scene": "the stock veto: a clip that cuts inside itself is a montage, "
             "not a take",
    "visualScore": "the stock ranking: the sheet's combined score breaks "
                   "the choice within a class",
    "motion.mean": "the drift veto and the continuity gate's motion floor",
    "motion.peak": "the ranking's motion term and the bench",
    "motion.peakAt": "the in-point: the shot's entrance lands on the movement",
    "motion.onsets": "the visual sync the SFX stage spends",
    "motion.still": "the drift veto and the continuity gate's stillness floor",
    "camera.travel": "the continuity gate's direction: a pan left never "
                     "reads as a pan right, so a reversal is caught",
}


# ---------------------------------------------------------------------------
# Small helpers
# ---------------------------------------------------------------------------
def _num(spec, key, default):
    try:
        value = spec.get(key, default)
        if value is None:
            return float(default)
        return float(value)
    except (TypeError, ValueError):
        return float(default)


def _int(spec, key, default):
    try:
        return int(spec.get(key, default))
    except (TypeError, ValueError):
        return int(default)


def _clamp(value, low=0.0, high=1.0):
    return max(low, min(high, value))


def _r(value, places=4):
    try:
        return round(float(value), places)
    except (TypeError, ValueError):
        return 0.0


def _median(values):
    if not values:
        return 0.0
    ordered = sorted(values)
    middle = len(ordered) // 2
    if len(ordered) % 2:
        return ordered[middle]
    return (ordered[middle - 1] + ordered[middle]) / 2.0


def _ffmpeg():
    return os.environ.get("FUNCUT_FFMPEG") or shutil.which("ffmpeg")


def _ffprobe():
    return os.environ.get("FUNCUT_FFPROBE") or shutil.which("ffprobe")


# ---------------------------------------------------------------------------
# PNG, both directions. The editor's preview frames are PNGs, and a PNG
# decoder in the standard library is what lets the whole read run with no
# ffmpeg CLI at all.
# ---------------------------------------------------------------------------
def png_decode(data):
    if data[:8] != b"\x89PNG\r\n\x1a\n":
        raise ValueError("not a PNG")
    pos, idat, ihdr = 8, [], None
    while pos + 8 <= len(data):
        n = struct.unpack(">I", data[pos : pos + 4])[0]
        tag = data[pos + 4 : pos + 8]
        body = data[pos + 8 : pos + 8 + n]
        pos += 12 + n
        if tag == b"IHDR":
            ihdr = struct.unpack(">IIBBBBB", body)
        elif tag == b"IDAT":
            idat.append(body)
        elif tag == b"IEND":
            break
    if not ihdr:
        raise ValueError("PNG has no IHDR")
    w, h, depth, colour, _, _, interlace = ihdr
    if depth != 8 or colour not in (0, 2, 6) or interlace != 0:
        raise ValueError(
            f"unsupported PNG: depth={depth} colour={colour} interlace={interlace}"
        )
    channels = {0: 1, 2: 3, 6: 4}[colour]
    raw = zlib.decompress(b"".join(idat))
    stride = w * channels
    out = bytearray(h * stride)
    prev = bytearray(stride)
    at = 0
    for y in range(h):
        ft = raw[at]
        at += 1
        line = bytearray(raw[at : at + stride])
        at += stride
        for x in range(stride):
            a = line[x - channels] if x >= channels else 0
            b = prev[x]
            c = prev[x - channels] if x >= channels else 0
            if ft == 1:
                line[x] = (line[x] + a) & 0xFF
            elif ft == 2:
                line[x] = (line[x] + b) & 0xFF
            elif ft == 3:
                line[x] = (line[x] + ((a + b) >> 1)) & 0xFF
            elif ft == 4:
                p = a + b - c
                pa, pb, pc = abs(p - a), abs(p - b), abs(p - c)
                pr = a if (pa <= pb and pa <= pc) else (b if pb <= pc else c)
                line[x] = (line[x] + pr) & 0xFF
            elif ft != 0:
                raise ValueError(f"unknown PNG filter {ft}")
        out[y * stride : (y + 1) * stride] = line
        prev = line
    rgb = bytearray(w * h * 3)
    for i in range(w * h):
        j = i * channels
        if channels == 1:
            rgb[i * 3 : i * 3 + 3] = bytes((out[j], out[j], out[j]))
        else:
            rgb[i * 3 : i * 3 + 3] = out[j : j + 3]
    return bytes(rgb), w, h


def _png_chunk(tag, data):
    return (
        struct.pack(">I", len(data))
        + tag
        + data
        + struct.pack(">I", zlib.crc32(tag + data) & 0xFFFFFFFF)
    )


def _png_rgb(rgb, w, h):
    raw = bytearray()
    stride = w * 3
    for y in range(h):
        raw.append(0)
        raw.extend(rgb[y * stride : (y + 1) * stride])
    return (
        b"\x89PNG\r\n\x1a\n"
        + _png_chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 2, 0, 0, 0))
        + _png_chunk(b"IDAT", zlib.compress(bytes(raw), 6))
        + _png_chunk(b"IEND", b"")
    )


def _png_grey(grey, w, h):
    raw = bytearray()
    for y in range(h):
        raw.append(0)
        raw.extend(bytes(grey[y * w : (y + 1) * w]))
    return (
        b"\x89PNG\r\n\x1a\n"
        + _png_chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 0, 0, 0, 0))
        + _png_chunk(b"IDAT", zlib.compress(bytes(raw), 6))
        + _png_chunk(b"IEND", b"")
    )


# ---------------------------------------------------------------------------
# Frames in: PNG blocks, a directory, or a media file through ffmpeg.
# ---------------------------------------------------------------------------
def _probe_duration(path):
    probe = _ffprobe()
    if probe:
        try:
            out = subprocess.run(
                [probe, "-v", "error", "-show_entries", "format=duration",
                 "-of", "default=nw=1:nk=1", path],
                capture_output=True, text=True, timeout=20,
            ).stdout.strip()
            if out:
                return float(out)
        except Exception:
            pass
    ff = _ffmpeg()
    if not ff:
        return None
    try:
        proc = subprocess.run(
            [ff, "-hide_banner", "-i", path],
            capture_output=True, text=True, timeout=20,
        )
    except Exception:
        return None
    for line in proc.stderr.splitlines():
        line = line.strip()
        if line.startswith("Duration:"):
            stamp = line.split("Duration:", 1)[1].split(",", 1)[0].strip()
            parts = stamp.split(":")
            if len(parts) == 3:
                try:
                    return float(parts[0]) * 3600 + float(parts[1]) * 60 + float(parts[2])
                except ValueError:
                    return None
    return None


def _ffmpeg_frame(path, at, width):
    ff = _ffmpeg()
    if not ff:
        raise ValueError(
            "no ffmpeg: give `frames` (pngB64 blocks) or a directory of PNGs, "
            "or put ffmpeg on PATH / set FUNCUT_FFMPEG"
        )
    proc = subprocess.run(
        [ff, "-v", "error", "-ss", f"{max(0.0, at):.3f}", "-i", path,
         "-frames:v", "1", "-vf", f"scale={int(width)}:-2",
         "-f", "image2pipe", "-vcodec", "png", "-"],
        capture_output=True, timeout=60,
    )
    if proc.returncode != 0 or not proc.stdout:
        raise ValueError(
            f"ffmpeg could not read a frame at {at:.3f}s from {path}: "
            + proc.stderr.decode("utf-8", "replace")[-200:]
        )
    return png_decode(proc.stdout)


def _load_frames(spec):
    """A list of (at, rgb, w, h). The caller's frames are believed; a
    media path is sampled through ffmpeg; a directory is read in name
    order. Every failure names what was missing."""
    width = max(16, _int(spec, "width", WORK_WIDTH))
    frames = spec.get("frames")
    if isinstance(frames, list) and frames:
        out = []
        for index, item in enumerate(frames):
            at = float(index)
            if isinstance(item, str):
                b64 = item
                path = None
            elif isinstance(item, dict):
                b64 = item.get("pngB64") or item.get("png")
                path = item.get("path")
                if item.get("at") is not None:
                    at = float(item["at"])
            else:
                raise ValueError(f"frame {index} is neither a PNG string nor an object")
            if b64:
                rgb, w, h = png_decode(base64.b64decode(b64))
            elif path:
                data = Path(path).read_bytes()
                if data[:8] == b"\x89PNG\r\n\x1a\n":
                    rgb, w, h = png_decode(data)
                else:
                    rgb, w, h = _ffmpeg_frame(path, at, width)
            else:
                raise ValueError(f"frame {index} names no pngB64 or path")
            out.append((at, rgb, w, h))
        return out
    directory = spec.get("dir")
    if directory:
        files = sorted(
            p for p in Path(directory).iterdir()
            if p.suffix.lower() == ".png"
        )
        if not files:
            raise ValueError(f"no PNG files under {directory}")
        return [
            (float(index), *png_decode(p.read_bytes()))
            for index, p in enumerate(files)
        ]
    path = spec.get("path")
    if not path:
        raise ValueError(
            "give `frames` (pngB64 blocks), `dir` (a folder of PNGs) or `path` (a media file)"
        )
    if Path(path).is_dir():
        return _load_frames({**spec, "dir": path})
    if Path(path).suffix.lower() == ".png":
        rgb, w, h = png_decode(Path(path).read_bytes())
        return [(0.0, rgb, w, h)]
    start = _num(spec, "from", 0.0)
    duration = _probe_duration(path)
    end = spec.get("to")
    end = float(end) if end is not None else (duration if duration else start + 1.0)
    if end <= start:
        end = start + 1.0
    times = spec.get("times")
    if isinstance(times, list) and times:
        points = [float(t) for t in times]
    else:
        samples = max(2, min(24, _int(spec, "samples", 8)))
        span = end - start
        points = [start + (i + 0.5) * span / samples for i in range(samples)]
    return [(t, *_ffmpeg_frame(path, t, width)) for t in points]


def _resize(rgb, w, h, target):
    target = max(2, int(target))
    if target >= w:
        return rgb, w, h
    th = max(2, int(round(h * target / w)))
    out = bytearray(target * th * 3)
    for ty in range(th):
        y0 = ty * h // th
        y1 = max(y0 + 1, (ty + 1) * h // th)
        for tx in range(target):
            x0 = tx * w // target
            x1 = max(x0 + 1, (tx + 1) * w // target)
            r = g = b = n = 0
            for y in range(y0, y1):
                base = y * w * 3
                for x in range(x0, x1):
                    i = base + x * 3
                    r += rgb[i]
                    g += rgb[i + 1]
                    b += rgb[i + 2]
                    n += 1
            j = (ty * target + tx) * 3
            out[j] = r // n
            out[j + 1] = g // n
            out[j + 2] = b // n
    return bytes(out), target, th


def _grey(rgb, w, h):
    return [(77 * rgb[i] + 150 * rgb[i + 1] + 29 * rgb[i + 2]) >> 8
            for i in range(0, w * h * 3, 3)]


# ---------------------------------------------------------------------------
# Motion: optical flow by block matching. The global shift is read from
# row and column projections (a pan moves every row together); the local
# flow is read per block, which is what separates a subject walking from
# a camera panning.
# ---------------------------------------------------------------------------
def _shift_1d(a, b, rng):
    n = len(a)
    best, best_sad, zero = 0, None, 0.0
    for shift in range(-rng, rng + 1):
        lo = max(0, -shift)
        hi = min(n, n - shift)
        if hi - lo < max(2, n // 3):
            continue
        sad = 0
        for i in range(lo, hi):
            sad += abs(a[i] - b[i + shift])
        sad /= (hi - lo)
        if shift == 0:
            zero = sad
        if best_sad is None or sad < best_sad:
            best_sad, best = sad, shift
    return best, best_sad or 0.0, zero


def _global_shift(prev, cur, w, h, rng):
    rows_prev = [sum(prev[y * w : (y + 1) * w]) for y in range(h)]
    rows_cur = [sum(cur[y * w : (y + 1) * w]) for y in range(h)]
    dy, row_best, row_zero = _shift_1d(rows_prev, rows_cur, rng)
    cols_prev = [sum(prev[x::w]) for x in range(w)]
    cols_cur = [sum(cur[x::w]) for x in range(w)]
    dx, col_best, col_zero = _shift_1d(cols_prev, cols_cur, rng)
    zero = row_zero + col_zero
    confidence = _clamp(1.0 - (row_best + col_best) / zero) if zero > 0 else 0.0
    return dx, dy, confidence


def _local_flow(prev, cur, w, h, block, search):
    vectors = []
    blocks = 0
    for by in range(0, h - block + 1, block):
        for bx in range(0, w - block + 1, block):
            blocks += 1
            zero = 0
            for y in range(by, by + block):
                base = y * w
                for x in range(bx, bx + block):
                    zero += abs(prev[base + x] - cur[base + x])
            best, bdx, bdy = zero, 0, 0
            for dy in range(-search, search + 1):
                if by + dy < 0 or by + dy + block > h:
                    continue
                for dx in range(-search, search + 1):
                    if bx + dx < 0 or bx + dx + block > w:
                        continue
                    if dx == 0 and dy == 0:
                        continue
                    sad = 0
                    for y in range(by, by + block):
                        base, other = y * w, (y + dy) * w
                        for x in range(bx, bx + block):
                            sad += abs(prev[base + x] - cur[other + x + dx])
                    if sad < best:
                        best, bdx, bdy = sad, dx, dy
            if zero > 0 and best < zero * 0.94:
                vectors.append(
                    (bdx, bdy, bx + block / 2, by + block / 2, (zero - best) / zero)
                )
    if not vectors:
        return {"mean": 0.0, "coverage": 0.0, "divergence": 0.0,
                "radialAgreement": 0.0, "directionDeg": 0.0, "count": 0}
    magnitudes = [math.hypot(dx, dy) for dx, dy, _, _, _ in vectors]
    centre_x, centre_y = (w - 1) / 2, (h - 1) / 2
    divergence, weight_sum = 0.0, 0.0
    radial_out, radial_in, radial_total = 0.0, 0.0, 0.0
    for dx, dy, bcx, bcy, weight in vectors:
        rx, ry = bcx - centre_x, bcy - centre_y
        radius = math.hypot(rx, ry)
        if radius > 1e-6:
            dot = (dx * rx + dy * ry) / radius
            divergence += dot * weight
            weight_sum += weight
            radial_total += weight
            if dot > 0.25:
                radial_out += weight
            elif dot < -0.25:
                radial_in += weight
    sum_dx = sum(dx * weight for dx, _, _, _, weight in vectors)
    sum_dy = sum(dy * weight for _, dy, _, _, weight in vectors)
    direction = math.degrees(math.atan2(sum_dy, sum_dx)) if (sum_dx or sum_dy) else 0.0
    radial_agreement = (
        max(radial_out, radial_in) / radial_total if radial_total else 0.0
    )
    return {
        "mean": sum(magnitudes) / len(magnitudes),
        "coverage": len(vectors) / max(1, blocks),
        "divergence": divergence / weight_sum if weight_sum else 0.0,
        "radialAgreement": radial_agreement,
        "directionDeg": direction,
        "count": len(vectors),
    }


def _motion_profile(frames, work_width, search):
    small = []
    for at, rgb, w, h in frames:
        resized, w2, h2 = _resize(rgb, w, h, work_width)
        small.append((at, _grey(resized, w2, h2), w2, h2))
    profile = []
    for (t0, g0, w0, _), (t1, g1, _, _) in zip(small, small[1:]):
        dt = t1 - t0 if t1 > t0 else 1 / 30.0
        dx, dy, confidence = _global_shift(g0, g1, w0, len(g0) // w0,
                                           max(2, work_width // 8))
        local = _local_flow(g0, g1, w0, len(g0) // w0,
                            max(4, work_width // 8), search)
        global_mag = math.hypot(dx, dy) / work_width / dt
        local_mag = local["mean"] / work_width / dt
        profile.append({
            "at": _r((t0 + t1) / 2, 3),
            "dt": _r(dt, 4),
            "globalDx": dx,
            "globalDy": dy,
            "globalConfidence": _r(confidence, 3),
            "global": _r(global_mag),
            "local": _r(local_mag),
            "combined": _r(local_mag + 0.5 * global_mag),
            "coverage": _r(local["coverage"]),
            "divergence": _r(local["divergence"]),
            "radialAgreement": _r(local["radialAgreement"]),
            "directionDeg": _r(local["directionDeg"], 1),
        })
    if not profile:
        return {"profile": [], "peakAt": None, "peak": 0.0, "onsets": [],
                "still": True, "mean": 0.0}
    combined = [row["combined"] for row in profile]
    peak_index = max(range(len(combined)), key=lambda i: combined[i])
    peak = combined[peak_index]
    median = _median(combined)
    threshold = max(STILL, 2.0 * median)
    onsets = []
    for index, row in enumerate(profile):
        if row["combined"] < threshold:
            continue
        if index and profile[index - 1]["combined"] >= threshold:
            continue
        onsets.append(row["at"])
    # Collapse an onset that repeats inside 80 ms: one movement is one
    # onset, not a run of frames that all cross the line.
    collapsed = []
    for at in onsets:
        if not collapsed or at - collapsed[-1] > 0.08:
            collapsed.append(at)
    return {
        "profile": profile,
        "peakAt": profile[peak_index]["at"],
        "peak": _r(peak),
        "onsets": collapsed,
        "still": peak < STILL,
        "mean": _r(sum(combined) / len(combined)),
        "threshold": _r(threshold),
    }


def _travel(dx, dy, kind):
    """The camera's own travel, as the continuity gate names it.

    Image flow runs opposite the camera: content drifting left across the
    frame is a camera panning right. The kind says the axis; the sign of
    the flow says which way along it, which the kind alone cannot.
    """
    if kind == "static":
        return "still"
    if kind == "zoom_in":
        return "in"
    if kind == "zoom_out":
        return "out"
    if abs(dx) >= abs(dy):
        return "right" if dx < 0 else "left"
    return "down" if dy < 0 else "up"


def _camera(profile, work_width):
    if not profile:
        return {"kind": "static", "moving": False, "cameraMoving": False,
                "subjectMoving": False, "travel": "still",
                "dxPerSec": 0.0, "dyPerSec": 0.0,
                "zoomPerSec": 0.0, "magnitude": 0.0, "localPerSec": 0.0,
                "confidence": 0.0}
    n = len(profile)
    dx_per = sum(row["globalDx"] / work_width / row["dt"] for row in profile) / n
    dy_per = sum(row["globalDy"] / work_width / row["dt"] for row in profile) / n
    zoom = sum(row["divergence"] / work_width / row["dt"] for row in profile) / n
    radial = sum(row.get("radialAgreement", 0.0) for row in profile) / n
    coverage = sum(row.get("coverage", 0.0) for row in profile) / n
    local = sum(row["local"] for row in profile) / n
    magnitude = math.hypot(dx_per, dy_per)
    # A zoom moves most of the frame; a subject moving toward the camera
    # produces divergence too, but only over its own blocks. Requiring
    # both radial agreement and broad coverage is what separates them.
    if abs(zoom) >= 0.01 and radial >= 0.7 and coverage >= 0.5 and local >= 0.01:
        kind = "zoom_in" if zoom > 0 else "zoom_out"
    elif magnitude < 0.01:
        kind = "static"
    elif abs(dx_per) > 1.6 * abs(dy_per):
        kind = "pan"
    elif abs(dy_per) > 1.6 * abs(dx_per):
        kind = "tilt"
    else:
        kind = "diagonal"
    camera_moving = kind != "static"
    subject_moving = local >= 0.01
    return {
        "kind": kind,
        "moving": camera_moving or subject_moving,
        "cameraMoving": camera_moving,
        "subjectMoving": subject_moving,
        "travel": _travel(dx_per, dy_per, kind),
        "dxPerSec": _r(dx_per),
        "dyPerSec": _r(dy_per),
        "zoomPerSec": _r(zoom),
        "magnitude": _r(magnitude),
        "localPerSec": _r(local),
        "confidence": _r(sum(row["globalConfidence"] for row in profile) / n, 3),
    }


# ---------------------------------------------------------------------------
# Saliency, quality, palette, shot scale, scene cuts. All classical: no
# model, no download, bounded by the work width.
# ---------------------------------------------------------------------------
def _cells(grey, w, h, cols, rows):
    cell_w, cell_h = w / cols, h / rows
    out = []
    for r in range(rows):
        y0, y1 = int(r * cell_h), max(int(r * cell_h) + 1, int((r + 1) * cell_h))
        for c in range(cols):
            x0, x1 = int(c * cell_w), max(int(c * cell_w) + 1, int((c + 1) * cell_w))
            total, count = 0, 0
            for y in range(y0, min(y1, h)):
                base = y * w
                for x in range(x0, min(x1, w)):
                    total += grey[base + x]
                    count += 1
            out.append(total / max(1, count))
    return out, cell_w, cell_h


def _saliency(grey, w, h, cols=32, rows=18):
    cells, _, _ = _cells(grey, w, h, cols, rows)
    mean = sum(cells) / len(cells)
    values = []
    for r in range(rows):
        for c in range(cols):
            contrast = abs(cells[r * cols + c] - mean)
            dx, dy = (c + 0.5) / cols - 0.5, (r + 0.5) / rows - 0.5
            bias = 1.0 - 0.6 * min(1.0, math.hypot(dx, dy) / 0.7071)
            values.append(contrast * bias)
    top = max(values) or 1.0
    normalised = [value / top for value in values]
    peak = max(range(len(normalised)), key=lambda i: normalised[i])
    pr, pc = divmod(peak, cols)
    px, py = (pc + 0.5) / cols, (pr + 0.5) / rows
    if px < 0.34:
        focus = "left"
    elif px > 0.66:
        focus = "right"
    elif py < 0.34:
        focus = "upper"
    elif py > 0.66:
        focus = "lower"
    else:
        focus = "center"
    blob = _saliency_blob(normalised, cols, rows)
    return {
        "map": [[round(normalised[r * cols + c], 2) for c in range(cols)]
                for r in range(rows)],
        "cols": cols,
        "rows": rows,
        "peak": {"x": _r(px, 3), "y": _r(py, 3)},
        "focus": focus,
        "blob": blob,
        "contrast": _r(max(values), 1),
    }


def _saliency_blob(normalised, cols, rows):
    mean = sum(normalised) / len(normalised)
    threshold = mean * 1.5
    xs, ys = [], []
    for index, value in enumerate(normalised):
        if value >= threshold:
            r, c = divmod(index, cols)
            xs.append(c)
            ys.append(r)
    if not xs:
        return None
    x0, x1, y0, y1 = min(xs), max(xs), min(ys), max(ys)
    return {
        "x": _r((x0 + 0.5) / cols, 3),
        "y": _r((y0 + 0.5) / rows, 3),
        "w": _r((x1 - x0 + 1) / cols, 3),
        "h": _r((y1 - y0 + 1) / rows, 3),
        "area": _r(((x1 - x0 + 1) / cols) * ((y1 - y0 + 1) / rows), 3),
    }


def _quality_frame(grey, w, h):
    n = w * h
    mean = sum(grey) / n
    variance = sum((value - mean) ** 2 for value in grey) / n
    lap, lap_count = 0, 0
    for y in range(1, h - 1):
        base = y * w
        for x in range(1, w - 1):
            i = base + x
            lap += abs(4 * grey[i] - grey[i - 1] - grey[i + 1]
                       - grey[i - w] - grey[i + w])
            lap_count += 1
    sharpness = lap / max(1, lap_count)
    noise, noise_count = 0, 0
    for y in range(h):
        base = y * w
        for x in range(1, w):
            i = base + x
            delta = abs(grey[i] - grey[i - 1])
            if delta < 6:
                noise += delta
                noise_count += 1
    noise = noise / max(1, noise_count)
    dark_rows = sum(
        1 for y in range(h) if sum(grey[y * w : (y + 1) * w]) / w < 16
    )
    blown = sum(1 for value in grey if value > 250) / n
    return {
        "sharpness": _r(sharpness, 2),
        "noise": _r(noise, 2),
        "brightness": _r(mean / 255.0, 3),
        "contrast": _r(math.sqrt(variance), 1),
        "letterbox": _r(dark_rows / h, 3),
        "blown": _r(blown, 3),
    }


def _quality(frames, work_width):
    rows = []
    for at, rgb, w, h in frames:
        resized, w2, h2 = _resize(rgb, w, h, work_width)
        row = _quality_frame(_grey(resized, w2, h2), w2, h2)
        row["at"] = _r(at, 3)
        rows.append(row)
    if not rows:
        return {"verdict": "unknown", "frames": []}
    sharpness = _median([row["sharpness"] for row in rows])
    noise = _median([row["noise"] for row in rows])
    brightness = _median([row["brightness"] for row in rows])
    contrast = _median([row["contrast"] for row in rows])
    letterbox = max(row["letterbox"] for row in rows)
    blown = max(row["blown"] for row in rows)
    if letterbox > 0.15:
        verdict = "letterbox"
    elif noise > 10.0:
        verdict = "noisy"
    elif sharpness < 2.5:
        verdict = "soft"
    elif contrast < 10.0:
        verdict = "flat"
    elif brightness < 0.07:
        verdict = "dark"
    elif brightness > 0.93:
        verdict = "blown"
    else:
        verdict = "pass"
    return {
        "verdict": verdict,
        "sharpness": _r(sharpness, 2),
        "noise": _r(noise, 2),
        "brightness": _r(brightness, 3),
        "contrast": _r(contrast, 1),
        "letterbox": _r(letterbox, 3),
        "blown": _r(blown, 3),
        "frames": rows,
    }


def _palette_frame(rgb):
    bins = {}
    for i in range(0, len(rgb), 3):
        r, g, b = rgb[i], rgb[i + 1], rgb[i + 2]
        key = (r >> 3, g >> 3, b >> 3)
        entry = bins.get(key)
        if entry is None:
            bins[key] = [1, r, g, b]
        else:
            entry[0] += 1
            entry[1] += r
            entry[2] += g
            entry[3] += b
    top = sorted(bins.values(), key=lambda entry: -entry[0])[:6]
    colours = [(entry[1] // entry[0], entry[2] // entry[0], entry[3] // entry[0])
               for entry in top]
    total = sum(entry[0] for entry in top) or 1
    red = sum(colour[0] * entry[0] for colour, entry in zip(colours, top)) / total
    green = sum(colour[1] * entry[0] for colour, entry in zip(colours, top)) / total
    blue = sum(colour[2] * entry[0] for colour, entry in zip(colours, top)) / total
    if red > blue + 18:
        temperature = "warm"
    elif blue > red + 18:
        temperature = "cool"
    else:
        temperature = "neutral"
    saturation = 0.0
    for i in range(0, len(rgb), 3):
        high = max(rgb[i], rgb[i + 1], rgb[i + 2])
        low = min(rgb[i], rgb[i + 1], rgb[i + 2])
        saturation += (high - low) / high if high else 0.0
    return {
        "dominant": ["#%02x%02x%02x" % colour for colour in colours],
        "temperature": temperature,
        "saturation": _r(saturation / max(1, len(rgb) // 3), 3),
        "mean": {"r": _r(red, 1), "g": _r(green, 1), "b": _r(blue, 1)},
    }


def _palette(frames):
    if not frames:
        return {"dominant": [], "temperature": "neutral", "saturation": 0.0}
    middle = frames[len(frames) // 2]
    return _palette_frame(middle[1])


def _scene_profile(frames, work_width=32):
    small = []
    for at, rgb, w, h in frames:
        resized, w2, h2 = _resize(rgb, w, h, work_width)
        small.append((at, _grey(resized, w2, h2), w2, h2))
    rows = []
    for (t0, g0, w0, h0), (t1, g1, _, _) in zip(small, small[1:]):
        n = w0 * h0
        pixel = sum(abs(g0[i] - g1[i]) for i in range(n)) / n / 255.0
        hist_a, hist_b = [0] * 16, [0] * 16
        for value in g0:
            hist_a[value >> 4] += 1
        for value in g1:
            hist_b[value >> 4] += 1
        hist = 0.5 * sum(abs(hist_a[i] - hist_b[i]) for i in range(16)) / n
        rows.append({
            "at": _r((t0 + t1) / 2, 3),
            "change": _r(0.65 * pixel + 0.35 * hist),
            "pixel": _r(pixel),
            "hist": _r(hist),
        })
    if not rows:
        return {"cuts": [], "dissolves": [], "profile": [], "threshold": CUT_FLOOR}
    changes = [row["change"] for row in rows]
    threshold = max(CUT_FLOOR, 2.2 * _median(changes))
    cuts = []
    for index, row in enumerate(rows):
        if row["change"] < threshold:
            continue
        before = rows[index - 1]["change"] if index else -1.0
        after = rows[index + 1]["change"] if index + 1 < len(rows) else -1.0
        if row["change"] >= before and row["change"] >= after:
            cuts.append({"at": row["at"], "score": row["change"]})
    dissolves = []
    run = []
    for row in rows:
        if threshold * 0.45 <= row["change"] < threshold:
            run.append(row["at"])
        else:
            if len(run) >= 3:
                dissolves.append({"from": run[0], "to": run[-1]})
            run = []
    if len(run) >= 3:
        dissolves.append({"from": run[0], "to": run[-1]})
    return {
        "cuts": cuts,
        "dissolves": dissolves,
        "profile": rows,
        "threshold": _r(threshold, 3),
    }


def _shot_scale(frames, spec, work_width):
    boxes = spec.get("boxes")
    subject = spec.get("subject")
    areas, heights = [], []
    source = "subject"
    if boxes or subject:
        for index, frame in enumerate(frames):
            box = boxes[index] if boxes and index < len(boxes) else subject
            if not box:
                continue
            bw = float(box.get("w", box.get("width", 0.0)) or 0.0)
            bh = float(box.get("h", box.get("height", 0.0)) or 0.0)
            areas.append(bw * bh)
            heights.append(bh)
    else:
        source = "saliency"
        for _, rgb, w, h in frames:
            resized, w2, h2 = _resize(rgb, w, h, work_width)
            saliency = _saliency(_grey(resized, w2, h2), w2, h2)
            blob = saliency.get("blob")
            if blob:
                areas.append(blob["area"])
                heights.append(blob["h"])
    if not areas:
        return {"scale": "unknown", "subjectArea": 0.0,
                "subjectHeight": 0.0, "source": source, "confidence": 0.0}
    area = _median(areas)
    height = _median(heights)
    if area >= 0.30 or height >= 0.60:
        scale = "close_up"
    elif area >= 0.10 or height >= 0.25:
        scale = "medium"
    else:
        scale = "wide"
    def classify(a, hh):
        if a >= 0.30 or hh >= 0.60:
            return "close_up"
        if a >= 0.10 or hh >= 0.25:
            return "medium"
        return "wide"
    agree = sum(1 for a, hh in zip(areas, heights) if classify(a, hh) == scale)
    return {
        "scale": scale,
        "subjectArea": _r(area, 3),
        "subjectHeight": _r(height, 3),
        "source": source,
        "confidence": _r(agree / len(areas), 2),
    }


# ---------------------------------------------------------------------------
# The ONNX doors. Real doors: they run when the file, numpy and
# onnxruntime are all present, and refuse with the reason when not.
# ---------------------------------------------------------------------------
def _model_file(spec, default_id):
    named = spec.get("model")
    if named:
        path = Path(named)
        if path.is_file():
            return str(path), None
        return None, f"no model file at {named}"
    wanted = spec.get("modelId") or default_id
    dirs = []
    if spec.get("modelDir"):
        dirs.append(str(spec["modelDir"]))
    if os.environ.get("FUNCUT_VISUAL_MODELS"):
        dirs.append(os.environ["FUNCUT_VISUAL_MODELS"])
    try:
        dirs.append(str(Path(__file__).resolve().parents[4] / "models" / "visual"))
    except IndexError:
        pass
    for model in VISUAL_MODELS:
        if model["id"] != wanted:
            continue
        for directory in dirs:
            candidate = Path(directory) / model["file"]
            if candidate.is_file():
                return str(candidate), None
        return None, (
            f"the {wanted} model is not on disk: put {model['file']} in "
            f"{dirs[0] if dirs else 'a model directory'} or pass `model`"
        )
    return None, f"unknown model id {wanted!r}: one of {[m['id'] for m in VISUAL_MODELS]}"


def _onnx_session(path):
    if np is None:
        raise RuntimeError("numpy is not installed: the ONNX doors need numpy")
    try:
        import onnxruntime as ort
    except Exception as exc:  # pragma: no cover - depends on the machine
        raise RuntimeError(f"onnxruntime is not installed: {exc}")
    options = ort.SessionOptions()
    options.intra_op_num_threads = max(1, min(4, os.cpu_count() or 1))
    return ort.InferenceSession(path, sess_options=options,
                                providers=["CPUExecutionProvider"])


def _resize_square(rgb, w, h, size):
    out = bytearray(size * size * 3)
    for y in range(size):
        sy = y * h // size
        for x in range(size):
            sx = x * w // size
            i, j = (sy * w + sx) * 3, (y * size + x) * 3
            out[j : j + 3] = rgb[i : i + 3]
    return bytes(out)


def _tensor(frames, size, time=None, mean=0.0, std=255.0):
    """A float32 NCHW (or NTCHW) tensor from the sampled frames."""
    chosen = frames
    if time:
        if len(chosen) >= time:
            step = len(chosen) / time
            chosen = [chosen[int(i * step)] for i in range(time)]
        else:
            chosen = chosen + [chosen[-1]] * (time - len(chosen))
    else:
        chosen = [chosen[len(chosen) // 2]]
    planes = []
    for _, rgb, w, h in chosen:
        square = _resize_square(rgb, w, h, size)
        channel = [0.0] * (size * size * 3)
        for i in range(size * size):
            for c in range(3):
                channel[c * size * size + i] = (square[i * 3 + c] - mean) / std
        planes.append(channel)
    flat = [value for plane in planes for value in plane]
    if time:
        shape = (1, len(planes), 3, size, size)
    else:
        shape = (1, 3, size, size)
    return np.asarray(flat, dtype=np.float32).reshape(shape)


def _softmax(values):
    if not values:
        return []
    top = max(values)
    exps = [math.exp(value - top) for value in values]
    total = sum(exps) or 1.0
    return [value / total for value in exps]


def _labels(spec):
    source = spec.get("labels")
    if isinstance(source, list):
        return [str(item) for item in source]
    if isinstance(source, str) and Path(source).is_file():
        return [line.strip() for line in Path(source).read_text(
            encoding="utf-8", errors="replace").splitlines() if line.strip()]
    return []


def _shape_size(shape, fallback=224):
    if isinstance(shape, (list, tuple)):
        for value in reversed(shape):
            if isinstance(value, int) and value > 32:
                return value
    return fallback


def action_class(spec):
    frames = _load_frames(spec)
    path, why = _model_file(spec, "movenet-a0")
    if not path:
        return {"ok": False, "error": why}
    try:
        session = _onnx_session(path)
        layer = session.get_inputs()[0]
        shape = list(layer.shape)
        size = max(64, min(320, _int(spec, "size", _shape_size(shape))))
        temporal = len(shape) == 5
        time = _int(spec, "time", shape[1] if temporal and isinstance(shape[1], int) else 8)
        tensor = _tensor(frames, size, time if temporal else None)
        output = session.run(None, {layer.name: tensor})[0]
        logits = [float(value) for value in np.asarray(output).reshape(-1)]
        scores = _softmax(logits)
        top_k = max(1, min(10, _int(spec, "topK", 5)))
        order = sorted(range(len(scores)), key=lambda i: -scores[i])[:top_k]
        names = _labels(spec)
        return {
            "ok": True,
            "tool": "action_class",
            "model": Path(path).name,
            "temporal": temporal,
            "top": [
                {
                    "index": index,
                    "label": names[index] if index < len(names) else f"class {index}",
                    "score": _r(scores[index]),
                }
                for index in order
            ],
            "note": (
                "Indices are the model's own class order. Pass `labels` (a text "
                "file, one label per line) to name them; without it the reading "
                "is an index and a score, never a guessed action name."
            ),
        }
    except Exception as exc:
        return {"ok": False, "error": f"{type(exc).__name__}: {exc}"}


def depth_map(spec):
    frames = _load_frames(spec)
    path, why = _model_file(spec, "depth-anything-v2-small")
    if not path:
        return {"ok": False, "error": why}
    try:
        session = _onnx_session(path)
        layer = session.get_inputs()[0]
        size = max(64, min(640, _int(spec, "size", _shape_size(layer.shape, 518))))
        tensor = _tensor(frames, size, mean=0.5, std=0.5)
        output = session.run(None, {layer.name: tensor})[0]
        arr = np.asarray(output).squeeze().astype("float32")
        if arr.ndim > 2:
            arr = arr.reshape(arr.shape[-2], arr.shape[-1])
        low, high = float(arr.min()), float(arr.max())
        span = high - low or 1.0
        normalised = ((arr - low) / span * 255.0).clip(0, 255).astype("uint8")
        h, w = normalised.shape
        png = base64.b64encode(_png_grey(normalised.reshape(-1).tolist(), w, h)).decode()
        return {
            "ok": True,
            "tool": "depth_map",
            "model": Path(path).name,
            "pngB64": png,
            "width": w,
            "height": h,
            "min": _r(low, 3),
            "max": _r(high, 3),
            "nearFraction": _r(float((normalised > 200).mean())),
            "farFraction": _r(float((normalised < 55).mean())),
            "note": "Brighter is nearer in the usual inverse-depth export. The map is "
                    "relative, not metric: it orders layers, it does not measure metres.",
        }
    except Exception as exc:
        return {"ok": False, "error": f"{type(exc).__name__}: {exc}"}


def subject_mask(spec):
    frames = _load_frames(spec)
    path, why = _model_file(spec, "fastsam-s")
    if not path:
        return {"ok": False, "error": why}
    try:
        session = _onnx_session(path)
        inputs = session.get_inputs()
        if len(inputs) > 1:
            return {
                "ok": False,
                "error": (
                    "this door takes an end-to-end mask model; a prompted "
                    "encoder/decoder pair needs the brush path, not this one"
                ),
            }
        layer = inputs[0]
        size = max(64, min(640, _int(spec, "size", _shape_size(layer.shape))))
        tensor = _tensor(frames, size)
        output = session.run(None, {layer.name: tensor})[0]
        arr = np.asarray(output).squeeze().astype("float32")
        if arr.ndim == 3:
            arr = arr[0]
        mask = arr > _num(spec, "threshold", 0.5)
        area = float(mask.mean())
        ys, xs = np.nonzero(mask)
        centroid = (
            {"x": _r(float(xs.mean()) / max(1, mask.shape[1]), 3),
             "y": _r(float(ys.mean()) / max(1, mask.shape[0]), 3)}
            if len(xs) else None
        )
        grey = (mask.astype("uint8") * 255).reshape(-1).tolist()
        h, w = mask.shape
        png = base64.b64encode(_png_grey(grey, w, h)).decode()
        return {
            "ok": True,
            "tool": "subject_mask",
            "model": Path(path).name,
            "pngB64": png,
            "width": w,
            "height": h,
            "area": _r(area),
            "centroid": centroid,
            "note": "The mask is white where the subject is. It is the pixel-accurate "
                    "form of track_subject's box, and it is what caption avoidance needs.",
        }
    except Exception as exc:
        return {"ok": False, "error": f"{type(exc).__name__}: {exc}"}


def visual_embedding(spec):
    frames = _load_frames(spec)
    default = spec.get("modelId") or "dinov2-small"
    path, why = _model_file(spec, default)
    if not path:
        return {"ok": False, "error": why}
    try:
        session = _onnx_session(path)
        layer = session.get_inputs()[0]
        size = max(64, min(640, _int(spec, "size", _shape_size(layer.shape))))
        tensor = _tensor(frames, size, mean=0.5, std=0.5)
        output = session.run(None, {layer.name: tensor})[0]
        vector = np.asarray(output).reshape(-1).astype("float32")
        limit = max(16, min(1024, _int(spec, "dimensions", 512)))
        return {
            "ok": True,
            "tool": "visual_embedding",
            "model": Path(path).name,
            "dimensions": int(vector.shape[0]),
            "norm": _r(float(np.linalg.norm(vector)), 4),
            "embedding": [_r(float(value), 5) for value in vector[:limit]],
            "truncated": bool(vector.shape[0] > limit),
            "note": "Cosine distance between two of these is visual similarity. "
                    "DINOv2 is the strong visual one; CLIP is the text-matched one.",
        }
    except Exception as exc:
        return {"ok": False, "error": f"{type(exc).__name__}: {exc}"}


DESCRIBE_SCHEMA = {
    "type": "object",
    "properties": {
        "environment": {"type": "string"},
        "subjects": {"type": "array", "items": {"type": "string"}, "maxItems": 5},
        "action": {"type": "string"},
        "mood": {"type": "string"},
        "shotScale": {"type": "string", "enum": ["wide", "medium", "close_up"]},
    },
    "required": ["environment", "subjects", "action"],
    "additionalProperties": False,
}

DESCRIBE_PROMPT = (
    "Describe only what is visible in these frames. Do not infer a story, a "
    "brand, a place or a person's intent. Answer with JSON matching the schema: "
    "environment (where this is), subjects (what is in frame, at most five), "
    "action (what is happening), mood (a single word, optional) and shotScale "
    "(wide, medium or close_up). If a field cannot be seen, leave it out rather "
    "than guessing."
)


def describe_scene(spec):
    if _VLM is None or not hasattr(_VLM, "ask"):
        return {"ok": False,
                "error": "the local VLM helper is not in this runtime (sense/vlm.py)"}
    try:
        frames = _load_frames(spec)
        images = [
            base64.b64encode(_png_rgb(rgb, w, h)).decode()
            for _, rgb, w, h in frames[:4]
        ]
        text = _VLM.ask(
            spec.get("prompt") or DESCRIBE_PROMPT,
            images=images,
            json_schema=DESCRIBE_SCHEMA,
            max_tokens=max(80, min(500, _int(spec, "maxTokens", 300))),
        )
    except Exception as exc:
        return {"ok": False, "error": f"{type(exc).__name__}: {exc}"}
    description = text
    try:
        description = json.loads(text)
    except (TypeError, ValueError):
        pass
    return {
        "ok": True,
        "tool": "describe_scene",
        "frames": len(images),
        "description": description,
        "bounded": True,
        "note": "Schema-constrained on purpose: docs/VLM.md records a free-form read "
                "inventing a whole film from one blue frame, so this door asks for "
                "visible facts only.",
    }


# ---------------------------------------------------------------------------
# The tool handlers. Each is one JSON object out, `ok` always present.
# ---------------------------------------------------------------------------
def measure_motion(spec):
    frames = _load_frames(spec)
    width = max(16, min(240, _int(spec, "flowWidth", _int(spec, "width", WORK_WIDTH))))
    search = max(1, min(6, _int(spec, "search", 3)))
    motion = _motion_profile(frames, width, search)
    camera = _camera(motion["profile"], width)
    return {
        "ok": True,
        "tool": "measure_motion",
        "frames": len(frames),
        "width": width,
        "search": search,
        "peakAt": motion["peakAt"],
        "peak": motion["peak"],
        "mean": motion["mean"],
        "still": motion["still"],
        "onsets": motion["onsets"],
        "threshold": motion.get("threshold", STILL),
        "profile": motion["profile"],
        "camera": camera,
        "note": (
            "The motion peak is where a cut can land on movement; the onsets are "
            "the visual equivalent of the audio flux onset. `camera` separates a "
            "panning shot from a moving subject."
        ),
    }


def camera_motion(spec):
    frames = _load_frames(spec)
    width = max(16, min(240, _int(spec, "flowWidth", _int(spec, "width", WORK_WIDTH))))
    motion = _motion_profile(frames, width, max(1, min(6, _int(spec, "search", 3))))
    camera = _camera(motion["profile"], width)
    return {
        "ok": True,
        "tool": "camera_motion",
        "frames": len(frames),
        **camera,
        "peakAt": motion["peakAt"],
        "note": (
            "A shot already panning, tilting or zooming needs no Ken Burns push; a "
            "static shot with a moving subject is the one that does. Monocularly a "
            "dolly and a zoom are the same flow, so `zoom_in`/`zoom_out` names both."
        ),
    }


def shot_scale(spec):
    frames = _load_frames(spec)
    width = max(16, min(240, _int(spec, "width", WORK_WIDTH)))
    scale = _shot_scale(frames, spec, width)
    return {"ok": True, "tool": "shot_scale", "frames": len(frames), **scale,
            "note": "Wide / medium / close-up, from the subject's measured area. "
                    "Pass `subject` or `boxes` from track_subject for the exact "
                    "answer; without them the saliency blob stands in."}


def saliency(spec):
    frames = _load_frames(spec)
    width = max(16, min(240, _int(spec, "width", WORK_WIDTH)))
    readings, rows = [], []
    for at, rgb, w, h in frames:
        resized, w2, h2 = _resize(rgb, w, h, width)
        reading = _saliency(_grey(resized, w2, h2), w2, h2)
        readings.append(reading)
        rows.append({"at": _r(at, 3), "peak": reading["peak"],
                     "focus": reading["focus"], "blob": reading["blob"]})
    if readings:
        middle = readings[len(readings) // 2]
    else:
        middle = {"map": [], "cols": 32, "rows": 18,
                  "peak": {"x": 0.5, "y": 0.5}, "focus": "center", "blob": None}
    return {"ok": True, "tool": "saliency", "frames": len(frames),
            "map": middle["map"], "cols": middle["cols"], "rows": middle["rows"],
            "peak": middle["peak"], "focus": middle["focus"],
            "blob": middle["blob"], "perFrame": rows,
            "note": "Classical center-surround attention, not a learned saliency model: "
                    "a cut that lands on this peak lands where the eye already is."}


def visual_quality(spec):
    frames = _load_frames(spec)
    width = max(16, min(240, _int(spec, "width", WORK_WIDTH)))
    reading = _quality(frames, width)
    per_frame = reading.pop("frames", [])
    return {"ok": True, "tool": "visual_quality", "frames": len(frames),
            **reading, "perFrame": per_frame,
            "note": "The technical veto: soft, noisy, letterboxed or blown footage is "
                    "rejected before it reaches a semantic rank."}


def palette(spec):
    frames = _load_frames(spec)
    return {"ok": True, "tool": "palette", "frames": len(frames),
            **_palette(frames),
            "note": "Dominant colours and temperature. A palette shift at a seam is a "
                    "real continuity signal; a match is a style match."}


def scene_cuts(spec):
    frames = _load_frames(spec)
    width = max(16, min(120, _int(spec, "width", 32)))
    reading = _scene_profile(frames, width)
    provided = _scene_times(spec.get("scenes") or spec.get("sceneCuts"))
    detected = reading["cuts"]
    merged = sorted({row["at"] for row in detected} | set(provided))
    agreement = None
    if provided and detected:
        near = sum(
            1 for at in provided
            if any(abs(at - row["at"]) <= 0.5 for row in detected)
        )
        agreement = _r(near / len(provided), 2)
    return {
        "ok": True,
        "tool": "scene_cuts",
        "frames": len(frames),
        "detected": detected,
        "scenes": provided,
        "cuts": merged,
        "agreement": agreement,
        "dissolves": reading["dissolves"],
        "threshold": reading["threshold"],
        "profile": reading["profile"],
        "note": "Frame-change boundaries. When analyze_scenes' own `scenes` are given "
                "they are carried as authoritative and the detected cuts are the "
                "agreement check.",
    }


def _scene_times(value):
    if not value:
        return []
    if isinstance(value, dict):
        value = value.get("scenes") or []
    times = []
    for row in value:
        if isinstance(row, (int, float)):
            times.append(float(row))
        elif isinstance(row, dict) and row.get("start") is not None:
            try:
                times.append(float(row["start"]))
            except (TypeError, ValueError):
                continue
    return sorted({round(at, 3) for at in times if at > 0.05})


def visual_sync(spec):
    audio = spec.get("audioOnsets")
    if not isinstance(audio, list) or not audio:
        return {"ok": False, "error": "give `audioOnsets` (the flux onsets or beat times)"}
    audio = [float(at) for at in audio]
    motion = spec.get("motionOnsets")
    if motion is None:
        frames = _load_frames(spec)
        width = max(16, min(240, _int(spec, "width", WORK_WIDTH)))
        motion = _motion_profile(frames, width, 3)["onsets"]
    motion = [float(at) for at in motion]
    tolerance = _num(spec, "toleranceMs", 80.0) / 1000.0
    pairs, offsets, matched = [], [], 0
    for at in audio:
        if not motion:
            pairs.append({"audio": _r(at, 3), "motion": None,
                          "offsetMs": None, "matched": False})
            continue
        nearest = min(motion, key=lambda moment: abs(moment - at))
        offset = nearest - at
        ok = abs(offset) <= tolerance
        if ok:
            matched += 1
            offsets.append(abs(offset))
        pairs.append({"audio": _r(at, 3), "motion": _r(nearest, 3),
                      "offsetMs": _r(offset * 1000, 1), "matched": ok})
    score = matched / len(audio) if audio else 0.0
    verdict = "synced" if score >= 0.6 else ("loose" if score >= 0.3 else "apart")
    return {
        "ok": True,
        "tool": "visual_sync",
        "toleranceMs": _r(tolerance * 1000, 1),
        "audioOnsets": [_r(at, 3) for at in audio],
        "motionOnsets": [_r(at, 3) for at in motion],
        "matched": matched,
        "score": _r(score),
        "meanAbsOffsetMs": _r(sum(offsets) / len(offsets) * 1000, 1) if offsets else None,
        "pairs": pairs,
        "verdict": verdict,
        "note": "Does this sound land on that movement? A score near 1 is a sound and "
                "a motion that agree; a low score is a sound placed by the clock alone.",
    }


def _visual_score(motion, quality, intent, scene=None):
    sharp = min(1.0, quality.get("sharpness", 0.0) / 20.0)
    noise = max(0.0, 1.0 - min(1.0, quality.get("noise", 0.0) / 15.0))
    bars = max(0.0, 1.0 - quality.get("letterbox", 0.0))
    peak = min(1.0, motion.get("peak", 0.0) / 0.15)
    score = 0.30 * sharp + 0.20 * noise + 0.15 * bars + 0.35 * peak
    if quality.get("verdict") != "pass":
        score *= 0.45
    if intent == "cut" and motion.get("peakAt") is not None:
        score += 0.10 * max(0.0, 1.0 - min(1.0, motion["peakAt"] / 1.0))
    # A montage is not a shot: a candidate that cuts inside itself fights a
    # narration gap it can never fill whole, so it sinks behind a single
    # take even when its frames measure well.
    if scene and intent == "cut" and scene.get("cuts", 0) >= 2:
        score *= 0.6
    return _r(_clamp(score))


def _notes(motion, quality, camera, intent, scene=None):
    notes = []
    if quality.get("verdict") == "soft":
        notes.append("soft: the measured detail is too low to hold a full frame")
    if quality.get("verdict") == "noisy":
        notes.append("noisy: grain is above the technical floor")
    if quality.get("letterbox"):
        notes.append("letterboxed: the frame carries bars, so fill will crop twice")
    if motion.get("still"):
        notes.append("still: no measured movement, so it cannot carry a cut-in on motion")
    if intent == "cut" and motion.get("peakAt") is not None and motion["peakAt"] > 1.2:
        notes.append("the movement starts late; the cut-in would open on stillness")
    if camera.get("cameraMoving"):
        notes.append(f"camera already {camera.get('kind')}: no extra push needed")
    if scene and scene.get("cuts", 0) >= 2:
        notes.append(f"montage: {scene['cuts']} shot changes inside the clip, not one take")
    return notes


def measure_clips(spec):
    paths = spec.get("paths") or spec.get("clips")
    if not isinstance(paths, list) or not paths:
        return {"ok": False, "error": "give `paths` (the candidate media files)"}
    intent = str(spec.get("intent") or "stock")
    samples = max(2, min(12, _int(spec, "samples", 6)))
    width = max(16, min(160, _int(spec, "width", WORK_WIDTH)))
    limit = max(1, min(16, _int(spec, "maxClips", 8)))
    rows = []
    for path in paths[:limit]:
        if not isinstance(path, str) or not path.strip():
            rows.append({"path": str(path), "ok": False, "error": "not a path"})
            continue
        sub = {**spec, "path": path, "samples": samples, "width": width}
        try:
            frames = _load_frames(sub)
            motion = _motion_profile(frames, width, 3)
            camera = _camera(motion["profile"], width)
            quality = _quality(frames, width)
            per_frame = quality.pop("frames", [])
            pal = _palette(frames)
            middle = frames[len(frames) // 2]
            resized, w2, h2 = _resize(middle[1], middle[2], middle[3], width)
            sal = _saliency(_grey(resized, w2, h2), w2, h2)
            scale = _shot_scale(frames, sub, width)
            scene = _scene_profile(frames, 24)
            scene_row = {
                "cuts": len(scene["cuts"]),
                "dissolves": len(scene["dissolves"]),
                "firstCut": scene["cuts"][0]["at"] if scene["cuts"] else None,
            }
            motion_row = {
                "mean": motion["mean"],
                "peak": motion["peak"],
                "peakAt": motion["peakAt"],
                "onsets": motion["onsets"],
                "still": motion["still"],
            }
            score = _visual_score(motion_row, quality, intent, scene_row)
            rows.append({
                "path": path,
                "ok": True,
                "samples": len(frames),
                "motion": motion_row,
                "camera": camera,
                "shotScale": scale,
                "quality": quality,
                "palette": pal,
                "saliency": {"peak": sal["peak"], "focus": sal["focus"],
                             "blob": sal["blob"]},
                "scene": scene_row,
                "visualScore": score,
                "notes": _notes(motion_row, quality, camera, intent, scene_row),
                "perFrame": per_frame,
            })
        except Exception as exc:  # one bad file never sinks the batch
            rows.append({"path": path, "ok": False,
                         "error": f"{type(exc).__name__}: {exc}"})
    good = [row for row in rows if row.get("ok")]
    return {
        "ok": True,
        "tool": "measure_clips",
        "intent": intent,
        "clips": rows,
        "summary": {
            "clips": len(rows),
            "measured": len(good),
            "passed": sum(1 for row in good
                          if row["quality"]["verdict"] == "pass"),
            "moving": sum(1 for row in good if not row["motion"]["still"]),
            "closeUps": sum(1 for row in good
                            if row["shotScale"]["scale"] == "close_up"),
        },
        "note": "One compact read per candidate. The score is the sheet's ranking: "
                "sharpness, noise, bars and measured motion, with a technical veto.",
    }


def bench(spec):
    clips = spec.get("clips")
    if not isinstance(clips, list) or not clips:
        measured = measure_clips(spec)
        if not measured.get("ok"):
            return measured
        clips = measured["clips"]
    window = _num(spec, "window", 0.5)
    cuts = spec.get("cuts")
    if not isinstance(cuts, list) or not cuts:
        return {"ok": False,
                "error": "give `cuts` as [{path, at}] - the cut times to judge"}

    def rate(rows):
        hit, checked = 0, 0
        for cut in rows:
            path = cut.get("path")
            at = float(cut.get("at", 0.0))
            row = next((clip for clip in clips if clip.get("path") == path), None)
            if not row or not row.get("ok"):
                continue
            onsets = row["motion"].get("onsets") or []
            peak = row["motion"].get("peakAt")
            checked += 1
            if any(abs(onset - at) <= window for onset in onsets):
                hit += 1
            elif peak is not None and abs(peak - at) <= window:
                hit += 1
        return {"checked": checked, "onPeak": hit,
                "fraction": _r(hit / checked) if checked else None}

    on_motion = rate(cuts)
    pause_rows = spec.get("pauseCuts")
    on_pause = rate(pause_rows) if isinstance(pause_rows, list) and pause_rows else None
    delta = None
    if on_pause and on_motion["fraction"] is not None and on_pause["fraction"] is not None:
        delta = _r(on_motion["fraction"] - on_pause["fraction"])
    if delta is None:
        verdict = ("measured" if on_motion["fraction"] is not None else "unmeasured")
    elif delta > 0.05:
        verdict = "motion-peaks-win"
    elif delta < -0.05:
        verdict = "pauses-win"
    else:
        verdict = "no-difference"
    return {
        "ok": True,
        "tool": "bench",
        "windowSeconds": _r(window, 3),
        "onMotionPeak": on_motion,
        "onPause": on_pause,
        "delta": delta,
        "verdict": verdict,
        "note": "The bench question: do cuts that land on a measured motion peak score "
                "better than cuts on pauses alone? A null result is a finding too.",
    }


VISUAL_MODELS = [
    {
        "id": "movenet-a0",
        "file": "movenet-a0.onnx",
        "bytes": 15_000_000,
        "licence": "Apache-2.0",
        "tool": "action_class",
        "signal": "temporal action class per window (600 Kinetics actions)",
        "upstream": "https://www.kaggle.com/models/google/movenet",
    },
    {
        "id": "depth-anything-v2-small",
        "file": "depth-anything-v2-small.onnx",
        "bytes": 25_000_000,
        "licence": "Apache-2.0",
        "tool": "depth_map",
        "signal": "per-pixel relative depth",
        "upstream": "https://huggingface.co/depth-anything/Depth-Anything-V2-Small",
    },
    {
        "id": "fastsam-s",
        "file": "fastsam-s.onnx",
        "bytes": 40_000_000,
        "licence": "Apache-2.0",
        "tool": "subject_mask",
        "signal": "pixel-accurate object masks",
        "upstream": "https://huggingface.co/Ultralytics/FastSAM",
    },
    {
        "id": "dinov2-small",
        "file": "dinov2-small.onnx",
        "bytes": 90_000_000,
        "licence": "Apache-2.0",
        "tool": "visual_embedding",
        "signal": "384-d visual embedding",
        "upstream": "https://huggingface.co/facebook/dinov2-small",
    },
    {
        "id": "clip-vit-b32",
        "file": "clip-vit-b32-visual.onnx",
        "bytes": 150_000_000,
        "licence": "MIT",
        "tool": "visual_embedding",
        "signal": "image-text similarity embedding",
        "upstream": "https://huggingface.co/openai/clip-vit-base-patch32",
    },
]


def models(spec):
    dirs = []
    if spec.get("modelDir"):
        dirs.append(str(spec["modelDir"]))
    if os.environ.get("FUNCUT_VISUAL_MODELS"):
        dirs.append(os.environ["FUNCUT_VISUAL_MODELS"])
    try:
        dirs.append(str(Path(__file__).resolve().parents[4] / "models" / "visual"))
    except IndexError:
        pass
    rows = []
    for model in VISUAL_MODELS:
        found = None
        for directory in dirs:
            candidate = Path(directory) / model["file"]
            if candidate.is_file():
                found = str(candidate)
                break
        rows.append({
            **model,
            "present": found is not None,
            "path": found,
            "onDiskBytes": Path(found).stat().st_size if found else 0,
        })
    return {
        "ok": True,
        "tool": "models",
        "dirs": dirs,
        "models": rows,
        "note": "The visual doors run only when their file is present AND numpy + "
                "onnxruntime are installed; otherwise they refuse with the reason. "
                "No model is downloaded by this module.",
    }


TOOLS = {
    "measure_motion": measure_motion,
    "camera_motion": camera_motion,
    "shot_scale": shot_scale,
    "visual_sync": visual_sync,
    "scene_cuts": scene_cuts,
    "visual_quality": visual_quality,
    "palette": palette,
    "saliency": saliency,
    "measure_clips": measure_clips,
    "bench": bench,
    "models": models,
    "action_class": action_class,
    "depth_map": depth_map,
    "subject_mask": subject_mask,
    "visual_embedding": visual_embedding,
    "describe_scene": describe_scene,
}


def _solid(w, h, colour):
    return bytes(bytearray(bytes(colour) * (w * h)))


def _draw_square(rgb, w, h, colour, sx, sy, size):
    buf = bytearray(rgb)
    for y in range(max(0, sy), min(h, sy + size)):
        for x in range(max(0, sx), min(w, sx + size)):
            i = (y * w + x) * 3
            buf[i : i + 3] = bytes(colour)
    return bytes(buf)


def _square(w, h, background, colour, sx, sy, size):
    return _draw_square(_solid(w, h, background), w, h, colour, sx, sy, size)


def _pattern(w, h, dx=0, dy=0):
    buf = bytearray(w * h * 3)
    for y in range(h):
        for x in range(w):
            value = (x * 3 + y * 2 + dx * 3 + dy * 2) % 256
            i = (y * w + x) * 3
            buf[i] = value
            buf[i + 1] = (value * 2) % 256
            buf[i + 2] = (255 - value) % 256
    return bytes(buf)


def _blur(rgb, w, h):
    out = bytearray(w * h * 3)
    for y in range(h):
        for x in range(w):
            r = g = b = n = 0
            for yy in range(max(0, y - 1), min(h, y + 2)):
                for xx in range(max(0, x - 1), min(w, x + 2)):
                    i = (yy * w + xx) * 3
                    r += rgb[i]
                    g += rgb[i + 1]
                    b += rgb[i + 2]
                    n += 1
            j = (y * w + x) * 3
            out[j] = r // n
            out[j + 1] = g // n
            out[j + 2] = b // n
    return bytes(out)


def selftest(only=None):
    checks = []

    def check(name, ok, detail=""):
        checks.append({"check": name, "ok": bool(ok), "detail": str(detail)})

    w, h = 48, 27
    pattern = _pattern(w, h)
    decoded, dw, dh = png_decode(_png_rgb(pattern, w, h))
    check("png roundtrip", decoded == pattern and dw == w and dh == h, f"{dw}x{dh}")

    static = [(i / 30.0, pattern, w, h) for i in range(4)]
    still = _motion_profile(static, 32, 3)
    check("a still shot measures still", still["still"] and still["peak"] < STILL,
          f"peak={still['peak']}")

    moving = [
        (i / 30.0, _draw_square(pattern, w, h, (245, 245, 245), 3 + i * 5, 9, 10), w, h)
        for i in range(6)
    ]
    motion = _motion_profile(moving, 32, 3)
    camera = _camera(motion["profile"], 32)
    check("a moving subject measures motion", (not motion["still"]) and motion["peak"] > STILL,
          f"peak={motion['peak']}")
    check("a moving subject is not a camera move",
          camera["subjectMoving"] and not camera["cameraMoving"], camera)

    panning = [(i / 30.0, _pattern(w, h, dx=i * 4), w, h) for i in range(6)]
    pan_motion = _motion_profile(panning, 32, 3)
    pan_camera = _camera(pan_motion["profile"], 32)
    check("a pan is a camera move",
          pan_camera["kind"] in ("pan", "diagonal") and pan_camera["magnitude"] > 0.01,
          pan_camera)
    # The travel word is what the continuity gate compares: the kind names
    # the axis, the flow's sign names the way along it.
    still_camera = _camera([], 32)
    check("a still frame travels nowhere",
          still_camera["travel"] == "still", still_camera["travel"])
    check("the travel word names a direction the gate knows",
          pan_camera["travel"] in ("left", "right", "up", "down", "in", "out",
                                   "still")
          and (pan_camera["kind"] == "static"
               or pan_camera["travel"] != "still"),
          f"{pan_camera['kind']} travels {pan_camera['travel']}")

    cut_frames = [
        (i / 30.0, pattern if i < 3 else _pattern(w, h, dx=40, dy=30), w, h)
        for i in range(6)
    ]
    cuts = _scene_profile(cut_frames, 24)
    check("a hard cut is found",
          len(cuts["cuts"]) >= 1 and abs(cuts["cuts"][0]["at"] - 0.083) < 0.12,
          cuts["cuts"])

    sharp = _quality([(0.0, pattern, w, h)], 32)
    soft = _quality([(0.0, _blur(pattern, w, h), w, h)], 32)
    check("blur lowers sharpness", sharp["sharpness"] > soft["sharpness"],
          f"{sharp['sharpness']} > {soft['sharpness']}")

    warm = _palette([(0.0, _solid(w, h, (200, 30, 30)), w, h)])
    check("a red frame reads warm",
          warm["temperature"] == "warm" and warm["dominant"][0].startswith("#"), warm)

    bright = _grey(_square(w, h, (10, 10, 10), (250, 250, 250), 36, 8, 10), w, h)
    attention = _saliency(bright, w, h)
    check("saliency finds a bright subject on the right",
          attention["peak"]["x"] > 0.6 and attention["focus"] == "right",
          f"{attention['peak']} {attention['focus']}")

    close = _shot_scale([(0.0, pattern, w, h)], {"subject": {"w": 0.5, "h": 0.7}}, 32)
    wide = _shot_scale([(0.0, pattern, w, h)], {"subject": {"w": 0.1, "h": 0.1}}, 32)
    check("shot scale follows the subject's area",
          close["scale"] == "close_up" and wide["scale"] == "wide",
          f"{close['scale']}/{wide['scale']}")

    sync = visual_sync({"audioOnsets": [0.5, 1.5],
                        "motionOnsets": [0.52, 1.45], "toleranceMs": 80})
    check("an audio onset on a motion onset is synced",
          sync["matched"] == 2 and sync["score"] == 1.0, sync["score"])

    encoded = [
        {"pngB64": base64.b64encode(_png_rgb(
            pattern if i < 3 else _pattern(w, h, dx=40, dy=30), w, h)).decode(),
         "at": i / 30.0}
        for i in range(6)
    ]
    provided = scene_cuts({"frames": encoded, "width": 24,
                           "scenes": [{"start": 0.0}, {"start": 0.083},
                                      {"start": 0.2}]})
    check("analyze_scenes' own cuts are carried",
          provided["ok"] and provided["scenes"] == [0.083, 0.2], provided.get("scenes"))

    handler = measure_motion({
        "frames": [{"pngB64": base64.b64encode(_png_rgb(f[1], w, h)).decode(), "at": f[0]}
                   for f in moving],
        "flowWidth": 32,
    })
    check("the measure_motion door answers", handler["ok"] and handler["peak"] > STILL,
          handler.get("peak"))

    good = _visual_score({"peak": 0.2, "peakAt": 0.2},
                         {"sharpness": 25, "noise": 1, "letterbox": 0,
                          "verdict": "pass"}, "cut")
    bad = _visual_score({"peak": 0.0, "peakAt": None},
                        {"sharpness": 1, "noise": 14, "letterbox": 0.2,
                         "verdict": "soft"}, "cut")
    check("the score ranks a sharp moving shot over a soft still one", good > bad,
          f"{good} > {bad}")

    rows = [{"path": "a.mp4", "ok": True,
             "motion": {"onsets": [0.5], "peakAt": 0.5}}]
    bench_out = bench({"clips": rows, "cuts": [{"path": "a.mp4", "at": 0.52}],
                       "pauseCuts": [{"path": "a.mp4", "at": 0.9}], "window": 0.2})
    check("the bench prefers motion peaks",
          bench_out["verdict"] == "motion-peaks-win"
          and bench_out["onMotionPeak"]["fraction"] == 1.0,
          bench_out.get("verdict"))

    refusal = action_class({"frames": [{"pngB64": base64.b64encode(
        _png_rgb(pattern, w, h)).decode()}], "modelId": "no-such-model"})
    check("an absent model refuses with a reason",
          refusal["ok"] is False and "unknown model id" in refusal["error"],
          refusal.get("error", ""))

    table = models({})
    check("the model doors are listed", table["ok"] and len(table["models"]) == 5,
          len(table.get("models", [])))

    measured = measure_clips({"paths": ["synthetic"], "frames": encoded,
                              "width": 32, "intent": "cut"})
    row = measured["clips"][0] if measured.get("clips") else {}
    expected = {"motion", "camera", "shotScale", "quality", "palette",
                "saliency", "scene", "visualScore"}
    missing = expected - set(row)
    unnamed = [key for key in expected if key not in CONSUMERS]
    unnamed += [f"motion.{key}" for key in ("mean", "peak", "peakAt",
                                            "onsets", "still")
                if f"motion.{key}" not in CONSUMERS]
    unnamed += ["camera.travel"] if "camera.travel" not in CONSUMERS else []
    check("every visual reading names its consumer",
          not missing and not unnamed,
          f"missing={sorted(missing)} unnamed={unnamed}")

    montage_score = _visual_score({"peak": 0.2, "peakAt": 0.2},
                                  {"sharpness": 25, "noise": 1, "letterbox": 0,
                                   "verdict": "pass"}, "cut", {"cuts": 3})
    single_score = _visual_score({"peak": 0.2, "peakAt": 0.2},
                                 {"sharpness": 25, "noise": 1, "letterbox": 0,
                                  "verdict": "pass"}, "cut", {"cuts": 0})
    check("a montage sinks behind a single take", montage_score < single_score,
          f"{montage_score} < {single_score}")

    failed = [row for row in checks if not row["ok"]]
    return {
        "ok": not failed,
        "tool": "selftest",
        "only": only,
        "checks": len(checks),
        "passed": len(checks) - len(failed),
        "failed": failed,
        "results": checks,
    }


def command(spec):
    """The sense.py proxy entry: spec names the tool; action extract or
    selftest. Never raises - the contract is one JSON object out."""
    tool = str(spec.get("tool") or "")
    action = str(spec.get("action", "extract"))
    try:
        if action == "selftest":
            return selftest(tool if tool in TOOLS else None)
        handler = TOOLS.get(tool)
        if handler is None:
            return {"ok": False,
                    "error": f"unknown tool {tool!r}: one of {sorted(TOOLS)}"}
        return handler(spec)
    except Exception as exc:  # noqa: BLE001  the contract: never crash
        return {"ok": False, "error": f"{type(exc).__name__}: {exc}"}


def main():
    parser = argparse.ArgumentParser(description="What a shot does, measured.")
    parser.add_argument("cmd", choices=["analyse", "selftest"])
    parser.add_argument("--spec", default="{}")
    args = parser.parse_args()
    raw = sys.stdin.read() if args.spec == "-" else args.spec
    try:
        spec = json.loads(raw)
        if not isinstance(spec, dict):
            raise ValueError("spec must be a JSON object")
    except ValueError as exc:
        print(json.dumps({"ok": False, "error": f"bad spec: {exc}"}))
        sys.exit(2)
    spec.setdefault("action", args.cmd)
    out = command(spec)
    print(json.dumps(out))
    sys.exit(0 if out.get("ok") else 4)


if __name__ == "__main__":
    main()
