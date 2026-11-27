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
  rank_frames      the stock gather's CLIP rerank: fetched candidates
                   scored against the slot's words, and each winner's
                   vision embedding handed back for near-duplicate
                   suppression (the openvid pair, no fetching)
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
    "composition": "the stock ranking's taste term and the caption band: a "
                   "subject on a third holds a caption without a fight",
    "blur": "the stock veto: motion that is a pan's smear is not an entrance",
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
def _model_candidates(spec):
    """Where a visual model may already be, in the order to try.

    The app keeps its models in `<data>/app.concat.editor/cutout-models`
    (see `concat-host/src/dirs.rs`), the same folder the settings sheet
    downloads into and the sound reader reads. Reading it here is what
    makes "download it in the model page and the picture reader uses it"
    true without either side knowing about the other. `modelDir` in the
    spec still wins, which is what the bench and the tests use.
    """
    if spec.get("modelDir"):
        return [Path(str(spec["modelDir"]))]
    if os.environ.get("FUNCUT_VISUAL_MODELS"):
        return [Path(os.environ["FUNCUT_VISUAL_MODELS"])]
    data = Path(os.environ.get("XDG_DATA_HOME") or (Path.home() / ".local" / "share"))
    dirs = []
    if os.environ.get("FUNCUT_APP_DIR"):
        dirs.append(Path(os.environ["FUNCUT_APP_DIR"]) / "portable" / "cutout-models")
    dirs.append(data / "app.concat.editor" / "cutout-models")
    for folder in ("funcut", "FUNCUT", "concat", "Concat"):
        dirs.append(data / folder / "cutout-models")
        dirs.append(data / folder / "models")
    try:
        dirs.append(Path(__file__).resolve().parents[4] / "models" / "visual")
    except IndexError:
        pass
    return dirs


def _model_file(spec, default_id):
    named = spec.get("model")
    if named:
        path = Path(named)
        if path.is_file():
            return str(path), None
        return None, f"no model file at {named}"
    wanted = spec.get("modelId") or default_id
    dirs = [str(candidate) for candidate in _model_candidates(spec)]
    for model in VISUAL_MODELS:
        if model["id"] != wanted:
            continue
        for directory in dirs:
            candidate = Path(directory) / model["file"]
            if candidate.is_file():
                return str(candidate), None
        # A file fetched by name (a Hub repo with no row of its own) is
        # recorded in index.json: the door follows the record.
        pinned = (_pins(spec).get(wanted) or {}).get("file", "")
        if pinned:
            for directory in dirs:
                candidate = Path(directory) / pinned
                if candidate.is_file():
                    return str(candidate), None
        return None, (
            f"the {wanted} model is not on disk: put {model['file']} in "
            f"{dirs[0] if dirs else 'a model directory'} or pass `model`"
        )
    return None, f"unknown model id {wanted!r}: one of {[m['id'] for m in VISUAL_MODELS]}"


_SESSIONS = {}


def _onnx_session(path):
    """One session per file per process: loading a model is the slow part.

    A door that runs twice on the same file (a clip measured for stock and
    again for the bench) pays the load once. The key is the path and its
    mtime, so a replaced checkpoint is a new session.
    """
    if np is None:
        raise RuntimeError("numpy is not installed: the ONNX doors need numpy")
    try:
        stamp = os.stat(path).st_mtime_ns
    except OSError:
        stamp = 0
    key = (os.path.abspath(path), stamp)
    cached = _SESSIONS.get(key)
    if cached is not None:
        return cached
    try:
        import onnxruntime as ort
    except Exception as exc:  # pragma: no cover - depends on the machine
        raise RuntimeError(f"onnxruntime is not installed: {exc}")
    options = ort.SessionOptions()
    options.intra_op_num_threads = max(1, min(4, os.cpu_count() or 1))
    options.graph_optimization_level = ort.GraphOptimizationLevel.ORT_ENABLE_ALL
    session = ort.InferenceSession(path, sess_options=options,
                                   providers=["CPUExecutionProvider"])
    if len(_SESSIONS) >= 4:
        _SESSIONS.clear()
    _SESSIONS[key] = session
    return session


def _resize_square(rgb, w, h, size, height=None):
    """Nearest-neighbour to (height or size) x size; the odd sizes are real:
    TransNetV2's input is 27 high and 48 wide."""
    out_h = height or size
    out = bytearray(size * out_h * 3)
    for y in range(out_h):
        sy = y * h // out_h
        for x in range(size):
            sx = x * w // size
            i, j = (sy * w + sx) * 3, (y * size + x) * 3
            out[j : j + 3] = rgb[i : i + 3]
    return bytes(out)


def _tensor(frames, size, time=None, mean=0.0, std=255.0, height=None):
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
    out_h = height or size
    planes = []
    for _, rgb, w, h in chosen:
        square = _resize_square(rgb, w, h, size, out_h)
        channel = [0.0] * (size * out_h * 3)
        for i in range(size * out_h):
            for c in range(3):
                channel[c * size * out_h + i] = (square[i * 3 + c] - mean) / std
        planes.append(channel)
    flat = [value for plane in planes for value in plane]
    if time:
        shape = (1, len(planes), 3, out_h, size)
    else:
        shape = (1, 3, out_h, size)
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
    path, why = _model_file(spec, "movinet-a0")
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


def rank_frames(spec):
    """The stock gather's semantic look at its own fetched candidates: the
    same MobileCLIP2-S0 pair the openvid shelf's search reranks by, pointed
    at files that are already on disk. `query` is the slot's words (its
    shelf query and the register the emotional read named); `paths` are the
    candidates the fetch cached. The reply is rerank.py's own - `order` by
    text-image cosine, `scores`, and `vectors`, the winning look's vision
    embedding per clip, which the gather spends on near-duplicate
    suppression. Nothing is fetched and nothing is downloaded here: a
    missing pair is an honest refusal the caller keeps its sheet order
    through, never a zero-filled score pretending to be one."""
    query = str(spec.get("query") or "").strip()
    if not query:
        return {"ok": False, "error": "`query` is the words the candidates are ranked against"}
    paths = spec.get("paths") or []
    if not isinstance(paths, list) or not paths:
        return {"ok": False, "error": "`paths` is the fetched candidate files"}
    try:
        import rerank  # the runtime dir's own: the pair, the tokenizer, the looks
    except Exception as exc:
        return {"ok": False, "error": f"the rerank half is absent: {exc}"}
    frames = []
    for index, path in enumerate(paths):
        if not isinstance(path, str) or not path.strip():
            continue
        duration = _probe_duration(path)
        if duration and duration > 1.2:
            # Two looks, the same timing the openvid rerank gives its own
            # shortlist: a clip whose subject only appears mid-clip is
            # ranked on what it shows there.
            frames.append({"id": index, "path": path, "at": round(duration * 0.35, 3)})
            frames.append({"id": index, "path": path, "at": round(duration * 0.65, 3)})
        else:
            frames.append({"id": index, "path": path, "at": 0.0})
    if not frames:
        return {"ok": False, "error": "no candidate path could be asked for"}
    reply = rerank.rerank({
        "query": query,
        "frames": frames,
        "candidates": [{"id": frame["id"]} for frame in frames],
        **{key: spec[key] for key in ("ffmpeg", "autoSetup") if key in spec},
    })
    reply["tool"] = "rank_frames"
    return reply


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
        pooled, tokens = _pooled(output)
        vector = np.asarray(pooled, dtype="float32").reshape(-1)
        limit = max(16, min(1024, _int(spec, "dimensions", 512)))
        return {
            "ok": True,
            "tool": "visual_embedding",
            "model": Path(path).name,
            "dimensions": int(vector.shape[0]),
            "tokens": tokens,
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


def _visual_score(motion, quality, intent, scene=None, comp=None, blur=None,
                  aesthetic=None):
    sharp = min(1.0, quality.get("sharpness", 0.0) / 20.0)
    noise = max(0.0, 1.0 - min(1.0, quality.get("noise", 0.0) / 15.0))
    bars = max(0.0, 1.0 - quality.get("letterbox", 0.0))
    peak = min(1.0, motion.get("peak", 0.0) / 0.15)
    taste = min(1.0, float((comp or {}).get("score", 0.5)))
    score = 0.26 * sharp + 0.17 * noise + 0.13 * bars + 0.32 * peak + 0.12 * taste
    # A measured taste prior, when the aesthetic door is installed, blends
    # a tenth of the score: it orders candidates, it never vetoes one.
    if isinstance(aesthetic, (int, float)):
        score = 0.9 * score + 0.1 * max(0.0, min(1.0, float(aesthetic) / 10.0))
    if quality.get("verdict") != "pass":
        score *= 0.45
    # A pan's smear reads as motion and is not one: the movement is the
    # camera losing detail, so the clip sinks the way a soft one does.
    if (blur or {}).get("verdict") == "directional blur":
        score *= 0.7
    if intent == "cut" and motion.get("peakAt") is not None:
        score += 0.10 * max(0.0, 1.0 - min(1.0, motion["peakAt"] / 1.0))
    # A montage is not a shot: a candidate that cuts inside itself fights a
    # narration gap it can never fill whole, so it sinks behind a single
    # take even when its frames measure well.
    if scene and intent == "cut" and scene.get("cuts", 0) >= 2:
        score *= 0.6
    return _r(_clamp(score))


def _notes(motion, quality, camera, intent, scene=None, comp=None, blur=None):
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
    if (blur or {}).get("verdict") == "directional blur":
        notes.append("blurred: the motion is a pan's smear, not an entrance")
    if comp and comp.get("placement") == "centre":
        notes.append("centred: the subject sits on the axis, so a caption "
                     "has nowhere to go that misses it")
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
            comp_rows = _composition_rows(frames, width)
            blur_rows = _blur_rows(frames, width)
            comp = max(comp_rows, key=lambda row: row["score"]) if comp_rows else None
            blur = {
                "verdict": "directional blur"
                if any(row["blurred"] for row in blur_rows) else "clean",
                "blurredShare": _r(sum(1 for row in blur_rows if row["blurred"])
                                   / max(1, len(blur_rows))),
            }
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
            doors = _enrich(sub, frames) if _enrich_on(spec) else {}
            door_report = (_enrich_doors(sub, _enrich_budget(sub))
                           if _enrich_on(spec)
                           else {"ready": [], "absent": [], "budget": 0})
            door_report["answered"] = sorted(doors)
            score = _visual_score(motion_row, quality, intent, scene_row, comp, blur,
                                  aesthetic=doors.get("aesthetic"))
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
                "composition": comp,
                "blur": blur,
                "models": doors,
                "doors": door_report,
                "visualScore": score,
                "notes": _notes(motion_row, quality, camera, intent, scene_row,
                                comp, blur),
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
        "enriched": sum(1 for row in good if row.get("models")),
        "note": "One compact read per candidate. The score is the sheet's ranking: "
                "sharpness, noise, bars, measured motion, composition and - when "
                "the shelf's doors are installed - aesthetic taste, with two "
                "vetoes: a technical one (soft, noisy, letterboxed) and a blur "
                "one (a pan's smear is not an entrance). `enrich: false` keeps "
                "the sheet to the classical reads.",
    }


ENRICH_DOORS = ("dover-mobile", "film-shot-classifier",
                "depth-anything-v2-small", "movinet-a0")


def _enrich_on(spec):
    flag = spec.get("enrich", True)
    if isinstance(flag, str):
        return flag.strip().lower() not in ("0", "false", "no", "off")
    return bool(flag)


def _ready_door(spec, model_id):
    """Whether this door would run right now: a file on disk and a runtime."""
    path, _ = _model_file(spec, model_id)
    return bool(path) and np is not None and _has_onnxruntime()


def _enrich_budget(spec):
    """How many doors one clip may open: all four listed by default."""
    return max(0, min(6, _int(spec, "enrichDoors", len(ENRICH_DOORS))))


def _enrich_doors(spec, budget):
    """Which enrich doors were installed, which were absent.

    The coverage ledger needs the difference between a reading that could
    not run (no checkpoint on disk) and one nobody asked for.
    """
    ready, absent = [], []
    for model_id in ENRICH_DOORS:
        (ready if _ready_door(spec, model_id) else absent).append(model_id)
    return {"ready": ready, "absent": absent, "budget": budget}


def _enrich(spec, frames):
    """The shelf's cheap reads on a clip the sheet already measured.

    Only doors that are installed run, at most `enrichDoors` of them, and
    each sees one frame - the middle one - so a stock sheet stays a sheet
    rather than a batch of inferences. With nothing installed this is an
    empty dict and costs a stat() per door.
    """
    budget = _enrich_budget(spec)
    if budget == 0:
        return {}
    middle = [frames[len(frames) // 2]]
    out = {}
    for model_id in ENRICH_DOORS:
        if budget <= 0:
            break
        if not _ready_door(spec, model_id):
            continue
        budget -= 1
        reading = _generic_door({**spec, "samples": 1}, model_id, frames=middle)
        if not reading.get("ok"):
            continue
        model = _model_row(model_id)
        decoded = reading.get("decoded") or {}
        if model["tool"] == "aesthetic":
            out["aesthetic"] = decoded.get("score")
        elif model["tool"] == "film_shot_type":
            top = (decoded.get("top") or [{}])[0]
            out["shotType"] = {"label": top.get("label"), "p": top.get("p")}
        elif model["tool"] == "depth_map":
            out["depth"] = {"nearShare": decoded.get("nearShare"),
                            "verdict": decoded.get("verdict")}
        elif model["tool"] == "action_class":
            top = (decoded.get("top") or [{}])[0]
            out["action"] = {"label": top.get("label"), "score": top.get("score")}
    return out


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
    # Tier 1 - action, scene and temporal understanding
    {
        "id": "movinet-a0",
        "tier": 1,
        "file": "movinet-a0.onnx",
        "bytes": 15_000_000,
        "licence": "Apache-2.0",
        "tool": "action_class",
        "signal": "temporal action class per window (600 Kinetics actions)",
        "upstream": "https://tfhub.dev/google/movinet/a0/base/kinetics-600",
        "door": {"decoder": "logits", "time": 8, "size": 172},
    },
    {
        "id": "transnetv2",
        "tier": 1,
        "file": "transnetv2.onnx",
        "bytes": 30_000_000,
        "licence": "MIT",
        "tool": "shot_boundaries",
        "signal": "shot-boundary probability per frame",
        "upstream": "https://github.com/soCzech/TransNetV2",
        "hfRepo": "elya5/transnetv2",
        "hfFile": "transnetv2.onnx",
        "door": {"decoder": "prob", "size": 48, "height": 27, "time": 100,
                 "layout": "nhwc", "std": 1.0,
                 "note": "the cut stage's own read: a boundary is where a cut "
                         "can land, a pause is only where it must"},
    },
    {
        "id": "smolvlm2-500m",
        "tier": 1,
        "file": "smolvlm2-500m.onnx",
        "bytes": 546_000_000,
        "licence": "Apache-2.0",
        "tool": "describe_scene",
        "signal": "bounded scene description: environment, objects, activity",
        "upstream": "https://huggingface.co/HuggingFaceTB/SmolVLM2-500M-Video-Instruct",
        "vlm": True,
        "door": {"decoder": "logits", "size": 384,
                 "note": "the repo's own local_vlm; schema-constrained, never "
                         "free creative judgment"},
    },
    {
        "id": "vjepa2-vitl",
        "tier": 1,
        "file": "vjepa2-vitl.onnx",
        "bytes": 1_130_000_000,
        "licence": "MIT",
        "tool": "video_embedding",
        "signal": "self-supervised video embedding + action head",
        "upstream": "https://github.com/facebookresearch/jepa",
        "door": {"decoder": "embedding", "size": 256, "time": 16,
                 "note": "a stronger temporal encoder than MoViNet and 75x the "
                         "bytes; the bench decides"},
    },
    {
        "id": "film-shot-classifier",
        "tier": 1,
        "file": "film-shot-classifier.onnx",
        "bytes": 90_000_000,
        "licence": "Apache-2.0",
        "tool": "film_shot_type",
        "signal": "shot type: wide, medium, close, extreme close, insert, two-shot",
        "upstream": "https://huggingface.co/models?search=shot%20type%20dinov2",
        "door": {"decoder": "logits", "size": 224,
                 "labels": ["wide", "medium", "close_up", "extreme_close_up",
                            "insert", "two_shot"]},
    },
    {
        "id": "scenegraph-vlm",
        "tier": 1,
        "file": "scenegraph-vlm.onnx",
        "bytes": 800_000_000,
        "licence": "CC-BY-4.0",
        "tool": "scene_graph",
        "signal": "subject-predicate-object graph per scene",
        "upstream": "https://arxiv.org/abs/2305.14623",
        "door": {"decoder": "logits", "size": 384,
                 "note": "relationships CLIP similarity cannot express; the "
                         "bounded-schema rule applies"},
    },
    # Tier 2 - depth, masks and subject geometry
    {
        "id": "depth-anything-v2-small",
        "tier": 2,
        "file": "depth-anything-v2-small.onnx",
        "bytes": 27_300_000,
        "licence": "Apache-2.0",
        "tool": "depth_map",
        "signal": "per-pixel relative depth",
        "upstream": "https://huggingface.co/depth-anything/Depth-Anything-V2-Small",
        "hfRepo": "onnx-community/depth-anything-v2-small",
        "hfFile": "onnx/model_quantized.onnx",
        "door": {"decoder": "depth", "size": 518},
    },
    {
        "id": "midas-v21-small",
        "tier": 2,
        "file": "midas-v21-small.onnx",
        "bytes": 80_000_000,
        "licence": "MIT",
        "tool": "depth_map",
        "signal": "monocular relative depth (the alternative to Depth Anything)",
        "upstream": "https://huggingface.co/Intel/dpt-hybrid-midas",
        "door": {"decoder": "depth", "size": 384},
    },
    {
        "id": "fastsam-s",
        "tier": 2,
        "file": "fastsam-s.onnx",
        "bytes": 40_000_000,
        "licence": "Apache-2.0",
        "tool": "subject_mask",
        "signal": "pixel-accurate object masks",
        "upstream": "https://huggingface.co/Ultralytics/FastSAM",
        "door": {"decoder": "masks", "size": 1024},
    },
    {
        "id": "segformer-b0",
        "tier": 2,
        "file": "segformer-b0.onnx",
        "bytes": 15_000_000,
        "licence": "Apache-2.0",
        "tool": "semantic_segments",
        "signal": "semantic segmentation (150 ADE20K classes)",
        "upstream": "https://huggingface.co/nvidia/segformer-b0-finetuned-ade-512-512",
        "door": {"decoder": "segments", "size": 512},
    },
    {
        "id": "bytetrack-yolo",
        "tier": 2,
        "file": "bytetrack-yolo.onnx",
        "bytes": 15_000_000,
        "licence": "MIT",
        "tool": "detect_objects",
        "signal": "persistent object tracks across frames",
        "upstream": "https://github.com/ifzhang/ByteTrack",
        "door": {"decoder": "boxes", "size": 640,
                 "note": "the detector's boxes feed `tracks`, which links them "
                         "into identities"},
    },
    # Tier 3 - semantic and visual similarity
    {
        "id": "dinov2-small",
        "tier": 3,
        "file": "dinov2-small.onnx",
        "bytes": 84_400_000,
        "licence": "Apache-2.0",
        "tool": "visual_embedding",
        "signal": "384-d visual embedding per frame",
        "upstream": "https://huggingface.co/facebook/dinov2-small",
        "hfRepo": "onnx-community/dinov2-small",
        "hfFile": "onnx/model_quantized.onnx",
        "door": {"decoder": "embedding", "size": 224},
    },
    {
        "id": "clip-vit-b32",
        "tier": 3,
        "file": "clip-vit-b32-visual.onnx",
        "bytes": 150_000_000,
        "licence": "MIT",
        "tool": "visual_embedding",
        "signal": "image-text similarity embedding",
        "upstream": "https://huggingface.co/openai/clip-vit-base-patch32",
        "hfRepo": "Xenova/clip-vit-base-patch32",
        "hfFile": "onnx/vision_model_quantized.onnx",
        "door": {"decoder": "embedding", "size": 224},
    },
    {
        "id": "siglip2-base",
        "tier": 3,
        "file": "siglip2-base.onnx",
        "bytes": 400_000_000,
        "licence": "Apache-2.0",
        "tool": "visual_embedding",
        "signal": "vision-language embedding (the CLIP successor to bench)",
        "upstream": "https://huggingface.co/google/siglip2-base-patch16-224",
        "door": {"decoder": "embedding", "size": 224},
    },
    {
        "id": "raft-small",
        "tier": 3,
        "file": "raft-small.onnx",
        "bytes": 3_800_000,
        "licence": "BSD-3-Clause",
        "tool": "optical_flow",
        "signal": "learned optical flow (better on complex scenes)",
        "upstream": "https://github.com/princeton-vl/RAFT",
        "door": {"decoder": "flow", "size": 256, "time": 2},
    },
    # Tier 4 - aesthetic, quality and attention
    {
        "id": "dover-mobile",
        "tier": 4,
        "file": "dover-mobile.onnx",
        "bytes": 100_000_000,
        "licence": "Apache-2.0",
        "tool": "aesthetic",
        "signal": "aesthetic + technical quality score per video",
        "upstream": "https://github.com/QualityAssessment/DOVER",
        "door": {"decoder": "scalar", "size": 224, "time": 8},
    },
    {
        "id": "laion-aesthetic",
        "tier": 4,
        "file": "laion-aesthetic.onnx",
        "bytes": 3_700_000,
        "licence": "MIT",
        "tool": "aesthetic",
        "signal": "aesthetic score 0-10 per frame",
        "upstream": "https://github.com/christophschuhmann/improved-aesthetic-predictor",
        "door": {"decoder": "scalar", "size": 224},
    },
    {
        "id": "vinet-s",
        "tier": 4,
        "file": "vinet-s.onnx",
        "bytes": 36_000_000,
        "licence": "Apache-2.0",
        "tool": "video_saliency",
        "signal": "video saliency map",
        "upstream": "https://github.com/remega/UNISAL",
        "door": {"decoder": "depth", "size": 224, "time": 8,
                 "note": "the map's peak is where the eye already is, so a cut "
                         "there lands on attention"},
    },
    {
        "id": "smblurdetect",
        "tier": 4,
        "file": "smblurdetect.onnx",
        "bytes": 8_000_000,
        "licence": "Apache-2.0",
        "tool": "blur_mask",
        "signal": "motion blur probability / mask",
        "upstream": "https://github.com/limacv/DeblurGANv2",
        "door": {"decoder": "masks", "size": 256},
    },
    # Tier 5 - human-specific signals
    {
        "id": "movenet-pose",
        "tier": 5,
        "file": "movenet-pose.onnx",
        "bytes": 5_000_000,
        "licence": "Apache-2.0",
        "tool": "pose",
        "signal": "17 body keypoints per person",
        "upstream": "https://www.kaggle.com/models/google/movenet",
        "door": {"decoder": "keypoints", "size": 192},
    },
    {
        "id": "mediapipe-hands",
        "tier": 5,
        "file": "mediapipe-hands.onnx",
        "bytes": 7_000_000,
        "licence": "Apache-2.0",
        "tool": "hands",
        "signal": "21 hand landmarks + gesture class",
        "upstream": "https://developers.google.com/mediapipe/solutions/vision/hand_landmarker",
        "door": {"decoder": "keypoints", "size": 224},
    },
    {
        "id": "eyegaze",
        "tier": 5,
        "file": "eyegaze.onnx",
        "bytes": 9_600_000,
        "licence": "research-only (Gaze360)",
        "tool": "gaze",
        "signal": "gaze pitch/yaw per face",
        "upstream": "https://github.com/ck-ll/EyeGaze",
        "door": {"decoder": "keypoints", "size": 224,
                 "note": "licence blocker: research-only, never a commercial path"},
    },
    {
        "id": "mobilegaze",
        "tier": 5,
        "file": "mobilegaze.onnx",
        "bytes": 5_000_000,
        "licence": "research-only (Gaze360)",
        "tool": "gaze",
        "signal": "gaze pitch/yaw per face (mobile variant)",
        "upstream": "https://github.com/yakhyo/gaze-estimation",
        "door": {"decoder": "keypoints", "size": 448},
    },
    {
        "id": "microexpnet",
        "tier": 5,
        "file": "microexpnet.onnx",
        "bytes": 900_000,
        "licence": "MIT",
        "tool": "face_emotion",
        "signal": "emotion class per face",
        "upstream": "https://github.com/citiususc/MicroExpNet",
        "door": {"decoder": "logits", "size": 84,
                 "labels": ["neutral", "happiness", "surprise", "sadness",
                            "anger", "disgust", "fear", "contempt"]},
    },
    # Tier 6 - specialised and deferred
    {
        "id": "syncnet",
        "tier": 6,
        "file": "syncnet.onnx",
        "bytes": 20_000_000,
        "licence": "MIT",
        "tool": "lip_sync",
        "signal": "lip-sync score",
        "upstream": "https://github.com/joonson/syncnet_python",
        "door": {"decoder": "scalar", "size": 224, "time": 5},
    },
    {
        "id": "aqa-transformer",
        "tier": 6,
        "file": "aqa-transformer.onnx",
        "bytes": 60_000_000,
        "licence": "research",
        "tool": "action_quality",
        "signal": "how well an action was performed",
        "upstream": "https://github.com/Finspire13/AQA-ANet",
        "door": {"decoder": "scalar", "size": 224, "time": 16},
    },
    {
        "id": "florence-2-base",
        "tier": 6,
        "file": "florence-2-base.onnx",
        "bytes": 230_000_000,
        "licence": "MIT",
        "tool": "describe_scene",
        "signal": "lightweight captioning / VQA",
        "upstream": "https://huggingface.co/microsoft/Florence-2-base",
        "vlm": True,
        "door": {"decoder": "logits", "size": 384},
    },
    {
        "id": "uform-gen2",
        "tier": 6,
        "file": "uform-gen2.onnx",
        "bytes": 400_000_000,
        "licence": "Apache-2.0",
        "tool": "describe_scene",
        "signal": "lightweight captioning / VQA (UForm)",
        "upstream": "https://huggingface.co/unum-cloud/uform-gen2-qwen-500m",
        "vlm": True,
        "door": {"decoder": "logits", "size": 384},
    },
    {
        "id": "grounding-dino-tiny",
        "tier": 6,
        "file": "grounding-dino-tiny.onnx",
        "bytes": 628_000_000,
        "licence": "Apache-2.0",
        "tool": "detect_objects",
        "signal": "open-vocabulary object detection by text query",
        "upstream": "https://huggingface.co/IDEA-Research/grounding-dino-tiny",
        "door": {"decoder": "boxes", "size": 800,
                 "note": "pass `labels` (the phrase list) with the call"},
    },
    {
        "id": "smolvlm2-256m",
        "tier": 6,
        "file": "smolvlm2-256m.onnx",
        "bytes": 256_000_000,
        "licence": "Apache-2.0",
        "tool": "describe_scene",
        "signal": "the small VLM variant, for 4 GB machines",
        "upstream": "https://huggingface.co/HuggingFaceTB/SmolVLM2-256M-Video-Instruct",
        "vlm": True,
        "door": {"decoder": "logits", "size": 384},
    },
]


def _np():
    if np is None:
        raise RuntimeError("numpy is not installed: the ONNX doors need numpy")
    return np


def _squeeze2(raw):
    """Drop leading/trailing singleton axes until two remain."""
    a = _np().asarray(raw, dtype=_np().float32)
    while a.ndim > 2 and a.shape[0] == 1:
        a = a[0]
    while a.ndim > 2 and a.shape[-1] == 1:
        a = a[..., 0]
    if a.ndim > 2:
        a = a[0]
    return a


def _norm_map(raw):
    a = _squeeze2(raw)
    lo, hi = float(a.min()), float(a.max())
    if hi - lo < 1e-6:
        return _np().zeros_like(a), lo, hi
    return (a - lo) / (hi - lo), lo, hi


def _decode_depth(raw):
    """A relative depth map: which half of the frame is nearer, and how much."""
    norm, lo, hi = _norm_map(raw)
    h, w = norm.shape[-2], norm.shape[-1]
    top = norm[: max(1, h // 2)]
    bottom = norm[max(1, h // 2):]
    centre = norm[h // 4 : max(h // 4 + 1, 3 * h // 4), w // 4 : max(w // 4 + 1, 3 * w // 4)]
    near = float((norm > 0.66).mean())
    return {
        "shape": [h, w],
        "nearShare": _r(near),
        "farShare": _r(float((norm < 0.33).mean())),
        "topMean": _r(float(top.mean())),
        "bottomMean": _r(float(bottom.mean())),
        "centreMean": _r(float(centre.mean())),
        "range": [_r(lo), _r(hi)],
        "verdict": "nearer at the bottom" if bottom.mean() > top.mean()
                   else "nearer at the top",
    }


def _pooled(raw):
    """One vector from a transformer's output: the CLS row, or the mean.

    DINOv2 and CLIP export `(1, 1 + patches, dim)`, not a vector; flattening
    that would mix every patch into the "embedding". The CLS token is the
    row the models are trained to pool from, so it is the row taken.
    """
    a = _np().asarray(raw, dtype=_np().float32)
    while a.ndim > 2 and a.shape[0] == 1:
        a = a[0]
    if a.ndim == 2 and a.shape[0] > 1:
        return a[0], int(a.shape[0])
    return a.reshape(-1), 1


def _decode_embedding(raw):
    pooled, tokens = _pooled(raw)
    norm = float(_np().linalg.norm(pooled))
    if norm < 1e-6:
        raise RuntimeError("the model returned an all-zero embedding")
    unit = pooled / norm
    return {
        "dim": int(unit.size),
        "tokens": tokens,
        "norm": _r(norm),
        "vector": [_r(float(value)) for value in unit],
    }


def _decode_logits(raw, labels=None, top=3):
    a = _np().asarray(raw, dtype=_np().float32).reshape(-1)
    exps = _np().exp(a - a.max())
    probs = exps / (float(exps.sum()) or 1.0)
    order = list(_np().argsort(probs)[::-1])[:top]
    names = labels or []
    return {
        "classes": int(probs.size),
        "top": [
            {
                "index": int(i),
                "label": names[int(i)] if int(i) < len(names) else f"class {int(i)}",
                "p": _r(float(probs[int(i)])),
            }
            for i in order
        ],
    }


def _decode_prob(raw, times=None, floor=0.5):
    a = _np().asarray(raw, dtype=_np().float32)
    if a.ndim >= 3 and a.shape[0] == 1:
        a = a[0]
    if a.ndim == 2 and a.shape[0] < a.shape[1]:
        a = a[:, 1] if a.shape[1] == 2 else a.max(axis=1)
    a = a.reshape(-1)
    if a.max() > 1.0 or a.min() < 0.0:
        a = 1.0 / (1.0 + _np().exp(-a))
    peak_index = int(_np().argmax(a))
    boundaries = [int(i) for i in _np().nonzero(a >= floor)[0]]
    span = float(times[-1] - times[0]) if times and len(times) > 1 else 1.0
    per = span / max(1, a.size - 1) if times else None
    return {
        "frames": int(a.size),
        "peak": _r(float(a[peak_index])),
        "peakAt": _r((times[0] + peak_index * per) if times and per else
                     peak_index / max(1, a.size - 1)),
        "boundaries": [int(i) for i in boundaries[:32]],
        "boundaryCount": len(boundaries),
    }


def _decode_masks(raw, floor=0.5):
    a = _np().asarray(raw, dtype=_np().float32)
    if a.ndim >= 3 and a.shape[0] == 1:
        a = a[0]
    if a.ndim > 2:
        a = a.max(axis=0) if a.shape[0] < a.shape[-1] else a[..., 0]
    if a.max() > 1.0 or a.min() < 0.0:
        a = 1.0 / (1.0 + _np().exp(-a))
    mask = a >= floor
    h, w = mask.shape[-2], mask.shape[-1]
    area = float(mask.mean())
    ys, xs = _np().nonzero(mask)
    if not len(ys):
        return {"shape": [h, w], "areaShare": 0.0, "present": False}
    return {
        "shape": [h, w],
        "areaShare": _r(area),
        "present": area > 0.002,
        "centroid": [_r(float(xs.mean()) / max(1, w - 1)),
                     _r(float(ys.mean()) / max(1, h - 1))],
        "bbox": [_r(float(xs.min()) / max(1, w - 1)),
                 _r(float(ys.min()) / max(1, h - 1)),
                 _r(float(xs.max()) / max(1, w - 1)),
                 _r(float(ys.max()) / max(1, h - 1))],
    }


def _decode_boxes(raw, labels=None, top=8, floor=0.25):
    a = _np().asarray(raw, dtype=_np().float32)
    while a.ndim > 2 and a.shape[0] == 1:
        a = a[0]
    if a.ndim != 2:
        raise RuntimeError(f"expected an (N, 4 or 6) box array, got {list(a.shape)}")
    rows = []
    for row in a:
        if a.shape[1] >= 6:
            x1, y1, x2, y2, conf, cls = row[:6]
            name = labels[int(cls)] if labels and int(cls) < len(labels) else f"class {int(cls)}"
        else:
            x1, y1, x2, y2 = row[:4]
            conf, name = 1.0, "object"
        if float(conf) < floor:
            continue
        rows.append({
            "label": name,
            "confidence": _r(float(conf)),
            "box": [_r(float(x1)), _r(float(y1)), _r(float(x2)), _r(float(y2))],
            "area": _r(max(0.0, float(x2) - float(x1)) * max(0.0, float(y2) - float(y1))),
        })
    rows.sort(key=lambda row: row["area"], reverse=True)
    return {"count": len(rows), "objects": rows[:top]}


def _decode_keypoints(raw, min_confidence=0.2):
    a = _np().asarray(raw, dtype=_np().float32)
    while a.ndim > 2 and a.shape[0] == 1:
        a = a[0]
    if a.ndim == 1:
        a = a.reshape(-1, 3) if a.size % 3 == 0 else a.reshape(-1, 2)
    if a.shape[-1] not in (2, 3):
        a = a.reshape(-1, a.shape[-1])
    scale = 1.0 if float(a[:, :2].max()) <= 1.5 else max(float(a[:, 0].max()),
                                                         float(a[:, 1].max()), 1.0)
    conf = a[:, 2] if a.shape[-1] == 3 else _np().ones(a.shape[0])
    keep = [i for i in range(a.shape[0]) if float(conf[i]) >= min_confidence]
    if not keep:
        return {"keypoints": 0, "present": False}
    xs = a[keep, 0] / scale
    ys = a[keep, 1] / scale
    return {
        "keypoints": int(len(keep)),
        "present": True,
        "meanConfidence": _r(float(conf[keep].mean())),
        "bbox": [_r(float(xs.min())), _r(float(ys.min())),
                 _r(float(xs.max())), _r(float(ys.max()))],
        "centre": [_r(float(xs.mean())), _r(float(ys.mean()))],
        "points": [[_r(float(a[i, 0] / scale)), _r(float(a[i, 1] / scale))]
                   for i in keep[:33]],
    }


def _decode_segments(raw, labels=None, top=3):
    a = _np().asarray(raw, dtype=_np().float32)
    while a.ndim > 3 and a.shape[0] == 1:
        a = a[0]
    if a.ndim == 3:
        classes = int(_np().argmax(a, axis=0).max()) + 1
        counts = _np().bincount(_np().argmax(a, axis=0).reshape(-1),
                                minlength=max(1, a.shape[0]))
    else:
        idx = _np().argmax(a, axis=-1).reshape(-1)
        classes = int(idx.max()) + 1
        counts = _np().bincount(idx, minlength=max(1, int(a.shape[-1])))
    total = float(counts.sum()) or 1.0
    order = list(_np().argsort(counts)[::-1])[:top]
    names = labels or []
    return {
        "classes": classes,
        "shares": [
            {
                "index": int(i),
                "label": names[int(i)] if int(i) < len(names) else f"class {int(i)}",
                "share": _r(float(counts[int(i)]) / total),
            }
            for i in order
        ],
    }


def _decode_scalar(raw, low=0.0, high=10.0):
    a = _np().asarray(raw, dtype=_np().float32).reshape(-1)
    value = float(a.mean())
    if high == 10.0 and 0.0 <= value <= 1.0:
        value *= 10.0
    return {"score": _r(_clamp((value - low) / max(1e-6, high - low)) * 10.0),
            "raw": _r(value)}


def _decode_flow(raw):
    a = _np().asarray(raw, dtype=_np().float32)
    while a.ndim > 3 and a.shape[0] == 1:
        a = a[0]
    if a.ndim != 3 or a.shape[0] not in (2, 3):
        raise RuntimeError(f"expected a (2, H, W) flow field, got {list(a.shape)}")
    dx, dy = a[0], a[1]
    mag = _np().sqrt(dx * dx + dy * dy)
    mean = float(mag.mean())
    mx, my = float(dx.mean()), float(dy.mean())
    if mean <= STILL:
        direction = "still"
    elif abs(mx) >= abs(my):
        direction = "right" if mx > 0 else "left"
    else:
        direction = "down" if my > 0 else "up"
    return {
        "shape": [int(a.shape[1]), int(a.shape[2])],
        "mean": _r(mean),
        "peak": _r(float(mag.max())),
        # The pixels' own direction; the camera travelled the other way.
        "direction": direction,
        "travel": _travel(mx, my, "pan") if mean > STILL else "still",
    }


DECODERS = {
    "depth": _decode_depth,
    "embedding": _decode_embedding,
    "logits": _decode_logits,
    "prob": _decode_prob,
    "masks": _decode_masks,
    "boxes": _decode_boxes,
    "keypoints": _decode_keypoints,
    "segments": _decode_segments,
    "scalar": _decode_scalar,
    "flow": _decode_flow,
}


def _model_row(model_id):
    for model in VISUAL_MODELS:
        if model["id"] == model_id:
            return model
    return None


def _generic_door(spec, model_id, frames=None):
    """One ONNX door, driven by the registry row: preprocess, run, decode.

    The decoder is chosen by the row, so a model's file is the only thing
    that changes when the door is pointed at a better checkpoint.
    """
    model = _model_row(model_id)
    if model is None:
        return {"ok": False, "error": f"unknown model id {model_id!r}"}
    if model.get("vlm"):
        return {"ok": False, "model": model_id, "tool": model["tool"],
                "error": ("this door is a vision-language model, not an ONNX "
                          "graph: call `describe_scene`, which asks the local "
                          "VLM helper with a bounded schema")}
    path, why = _model_file(spec, model_id)
    if not path:
        return {"ok": False, "model": model_id, "tool": model["tool"],
                "error": why, "licence": model["licence"],
                "upstream": model["upstream"]}
    try:
        session = _onnx_session(path)
    except Exception as exc:  # pragma: no cover - depends on the machine
        return {"ok": False, "model": model_id, "error": f"{model_id}: {exc}"}
    door = model.get("door") or {}
    try:
        if frames is None:
            frames = _load_frames(spec)
        layer = session.get_inputs()[0]
        shape = list(layer.shape)
        temporal = len(shape) == 5
        size = max(32, min(1024, _int(spec, "size",
                                     door.get("size") or _shape_size(shape, 224))))
        time = max(2, min(128, _int(spec, "time", door.get("time") or 8))) if temporal else None
        tensor = _tensor(frames, size, time,
                         door.get("mean", 0.0), door.get("std", 255.0),
                         door.get("height"))
        if door.get("layout") == "nhwc":
            tensor = tensor.transpose(0, 2, 3, 1) if not temporal else \
                tensor.transpose(0, 1, 3, 4, 2)
        raw = session.run(None, {layer.name: tensor})
        decoder = DECODERS.get(door.get("decoder", "logits"))
        if decoder is None:
            return {"ok": False, "model": model_id,
                    "error": f"unknown decoder {door.get('decoder')!r}"}
        kwargs = {}
        if door.get("decoder") in ("logits", "boxes", "segments"):
            kwargs["labels"] = _labels(spec) or door.get("labels") or None
        decoded = decoder(raw[0], **kwargs) if raw else {}
        return {
            "ok": True,
            "tool": model["tool"],
            "model": model_id,
            "licence": model["licence"],
            "decoded": decoded,
            "outputs": [list(o.shape) for o in session.get_outputs()],
            "note": door.get("note", model["signal"]),
        }
    except Exception as exc:
        return {"ok": False, "model": model_id,
                "error": f"{type(exc).__name__}: {exc}"}


def _door(spec, default_id):
    """A specialised door when one exists, the generic runner otherwise.

    `spec["modelId"]` picks an alternate from the registry (CLIP beside
    DINOv2, MiDaS beside Depth Anything, Grounding DINO beside ByteTrack),
    so a bench swaps the file and nothing else.
    """
    model_id = spec.get("modelId") or default_id
    model = _model_row(model_id)
    if model is None:
        return {"ok": False, "error": f"unknown model id {model_id!r}"}
    special = {
        "action_class": action_class,
        "depth_map": depth_map,
        "subject_mask": subject_mask,
        "visual_embedding": visual_embedding,
        "describe_scene": describe_scene,
    }
    handler = special.get(model["tool"])
    if handler is not None and not spec.get("generic"):
        return handler({**spec, "modelId": model_id})
    return _generic_door(spec, model_id)


def shot_boundaries(spec):
    return _door(spec, "transnetv2")


def film_shot_type(spec):
    return _door(spec, "film-shot-classifier")


def video_embedding(spec):
    return _door(spec, "vjepa2-vitl")


def scene_graph(spec):
    return _door(spec, "scenegraph-vlm")


def semantic_segments(spec):
    return _door(spec, "segformer-b0")


def detect_objects(spec):
    return _door(spec, "bytetrack-yolo")


def optical_flow(spec):
    return _door(spec, "raft-small")


def aesthetic(spec):
    return _door(spec, "dover-mobile")


def video_saliency(spec):
    return _door(spec, "vinet-s")


def blur_mask(spec):
    return _door(spec, "smblurdetect")


def pose(spec):
    return _door(spec, "movenet-pose")


def hands(spec):
    return _door(spec, "mediapipe-hands")


def gaze(spec):
    return _door(spec, "eyegaze")


def face_emotion(spec):
    return _door(spec, "microexpnet")


def lip_sync(spec):
    return _door(spec, "syncnet")


def action_quality(spec):
    return _door(spec, "aqa-transformer")


def _thirds_distance(x, y):
    """Distance from the nearest third-line intersection, normalised 0-1."""
    best = 1.0
    for tx in (1 / 3, 2 / 3):
        for ty in (1 / 3, 2 / 3):
            best = min(best, math.hypot(x - tx, y - ty))
    return best


def composition(spec):
    """Where the subject sits in the frame, and whether the frame is balanced."""
    frames = _load_frames(spec)
    width = max(16, min(160, _int(spec, "width", WORK_WIDTH)))
    rows = _composition_rows(frames, width)
    best = max(rows, key=lambda row: row["score"]) if rows else None
    return {
        "ok": True,
        "tool": "composition",
        "frames": rows,
        "score": best["score"] if best else 0.0,
        "placement": best["placement"] if best else None,
        "note": "A classical rule-of-thirds read of where the eye lands: the "
                "distance from the nearest third intersection, the left-right "
                "balance of the salient cells, and the headroom above the "
                "subject. No model, no download.",
    }


def _composition_rows(frames, width):
    rows = []
    for index, (_, rgb, w, h) in enumerate(frames):
        resized, w2, h2 = _resize(rgb, w, h, width)
        grey = _grey(resized, w2, h2)
        sal = _saliency(grey, w2, h2)
        blob = sal.get("blob") or {}
        peak = sal.get("peak") or {"x": 0.5, "y": 0.5}
        x = float(peak.get("x", 0.5))
        y = float(peak.get("y", 0.5))
        off = _thirds_distance(x, y)
        grid = sal.get("map") or []
        cols = max(1, int(sal.get("cols") or 1))
        left = right = 0.0
        for row in grid:
            for c, value in enumerate(row):
                if c < cols / 2:
                    left += value
                else:
                    right += value
        balance = 1.0 - abs(left - right) / max(1e-6, left + right)
        headroom = 1.0 - min(1.0, max(0.0, 0.14 - y) / 0.14)
        score = _clamp(0.55 * (1.0 - min(1.0, off / 0.45))
                       + 0.25 * balance + 0.20 * headroom)
        rows.append({
            "at": _r(index / max(1, len(frames) - 1)),
            "peak": {"x": _r(x), "y": _r(y)},
            "thirdsOffset": _r(off),
            "balance": _r(balance),
            "headroom": _r(headroom),
            "score": _r(score),
            "placement": ("centre" if off > 0.3 else
                          ("left third" if x < 0.5 else "right third")),
            "blobArea": _r(float(blob.get("area", 0.0))),
        })
    return rows


def _crispness(grey, w, h):
    """How step-like the transitions are, per axis: 2 is a step, 0 a ramp."""
    lap_x = grad_x = lap_y = grad_y = 0.0
    for y in range(1, h - 1):
        row = y * w
        for x in range(1, w - 1):
            i = row + x
            lap_x += abs(grey[i - 1] - 2 * grey[i] + grey[i + 1])
            grad_x += abs(grey[i + 1] - grey[i - 1])
            lap_y += abs(grey[i - w] - 2 * grey[i] + grey[i + w])
            grad_y += abs(grey[i + w] - grey[i - w])
    return (lap_x / max(grad_x, 1e-6), lap_y / max(grad_y, 1e-6),
            (grad_x + grad_y) / max(1, (w - 2) * (h - 2)))


def _blur_rows(frames, width):
    """A smear turns a step into a ramp: the axis it ran along loses crispness."""
    rows = []
    for index, (_, rgb, w, h) in enumerate(frames):
        resized, w2, h2 = _resize(rgb, w, h, width)
        grey = _grey(resized, w2, h2)
        crisp_x, crisp_y, detail = _crispness(grey, w2, h2)
        small, large = min(crisp_x, crisp_y), max(crisp_x, crisp_y)
        ratio = large / small if small > 1e-6 else 99.0
        rows.append({
            "at": _r(index / max(1, len(frames) - 1)),
            "crispX": _r(crisp_x, 3),
            "crispY": _r(crisp_y, 3),
            "ratio": _r(min(99.0, ratio), 2),
            "detail": _r(detail, 2),
            "axis": "x" if crisp_x < crisp_y else "y",
            "blurred": ratio >= 2.5 and small < 0.9,
        })
    return rows


def motion_blur(spec):
    """Directional smearing: is the frame's detail lost along one axis?"""
    frames = _load_frames(spec)
    width = max(16, min(160, _int(spec, "width", WORK_WIDTH)))
    rows = _blur_rows(frames, width)
    worst = max(rows, key=lambda row: row["ratio"]) if rows else None
    return {
        "ok": True,
        "tool": "motion_blur",
        "frames": rows,
        "worst": worst,
        "blurredShare": _r(sum(1 for row in rows if row["blurred"]) / max(1, len(rows))),
        "verdict": "directional blur" if worst and worst["blurred"] else "clean",
        "note": "A smear turns a step edge into a ramp, and only along the axis "
                "it ran: each frame's transitions are compared with themselves, "
                "so no model, no reference frame and no download. A clip whose "
                "motion is a pan's smear is vetoed the way a soft one is - the "
                "movement is an artefact, not an entrance.",
    }


def _iou(a, b):
    ax1, ay1, ax2, ay2 = a
    bx1, by1, bx2, by2 = b
    ix1, iy1 = max(ax1, bx1), max(ay1, by1)
    ix2, iy2 = min(ax2, bx2), min(ay2, by2)
    inter = max(0.0, ix2 - ix1) * max(0.0, iy2 - iy1)
    area_a = max(0.0, ax2 - ax1) * max(0.0, ay2 - ay1)
    area_b = max(0.0, bx2 - bx1) * max(0.0, by2 - by1)
    union = area_a + area_b - inter
    return inter / union if union > 1e-9 else 0.0


def tracks(spec):
    """Link per-frame boxes into identities: does the subject stay one subject?

    The detector's boxes come from `track_subject` (or any caller); this
    links them by overlap and velocity, which is what tells a cut whether
    the subject it framed is the subject the next shot inherits.
    """
    frames = spec.get("frames") or spec.get("boxes")
    if not isinstance(frames, list) or not frames:
        return {"ok": False,
                "error": "give `frames`: one list of [x1,y1,x2,y2] boxes per frame"}
    iou_floor = _num(spec, "iou", 0.25)
    active = {}
    finished = []
    next_id = 1
    for index, boxes in enumerate(frames):
        seen = set()
        for box in boxes or []:
            if isinstance(box, dict):
                # track_subject's own shape: fractions of the frame.
                x = float(box.get("x", 0.0))
                y = float(box.get("y", 0.0))
                box = [x, y, x + float(box.get("w", 0.0)),
                       y + float(box.get("h", 0.0))]
            if not isinstance(box, (list, tuple)) or len(box) < 4:
                continue
            best, best_iou = None, iou_floor
            for track_id, track in active.items():
                if track_id in seen:
                    continue
                predicted = track["box"]
                if track.get("velocity"):
                    predicted = [predicted[i] + track["velocity"][i]
                                 for i in range(4)]
                score = _iou(predicted, box[:4])
                if score >= best_iou:
                    best, best_iou = track_id, score
            if best is None:
                active[next_id] = {"id": next_id, "box": list(box[:4]),
                                   "first": index, "last": index, "boxes": 1,
                                   "velocity": None}
                seen.add(next_id)
                next_id += 1
            else:
                track = active[best]
                track["velocity"] = [box[i] - track["box"][i] for i in range(4)]
                track["box"] = list(box[:4])
                track["last"] = index
                track["boxes"] += 1
                seen.add(best)
        for track_id in list(active):
            if track_id not in seen:
                finished.append(active.pop(track_id))
    finished.extend(active.values())
    finished.sort(key=lambda track: track["boxes"], reverse=True)
    span = max(1, len(frames))
    rows = [{
        "id": track["id"],
        "first": track["first"],
        "last": track["last"],
        "boxes": track["boxes"],
        "coverage": _r(track["boxes"] / span),
        "box": [_r(float(value)) for value in track["box"]],
    } for track in finished]
    main = rows[0] if rows else None
    return {
        "ok": True,
        "tool": "tracks",
        "frames": len(frames),
        "tracks": rows[:16],
        "count": len(rows),
        "main": main,
        "continuity": _r(main["coverage"]) if main else 0.0,
        "note": "Overlap-and-velocity association over the boxes a detector "
                "gives. A subject that keeps one id across the clip is a "
                "subject a cut can follow; one that changes id mid-clip is a "
                "subject a cut would jump.",
    }


def tracks_from_file(spec):
    """The same linking, with `track_subject`'s own reply as the input."""
    reply = spec.get("trackSubject") or spec.get("track") or spec.get("reply")
    if not isinstance(reply, dict):
        return tracks(spec)
    boxes = reply.get("boxes") or reply.get("frames") or reply.get("points")
    if not isinstance(boxes, list):
        return {"ok": False, "error": "the track_subject reply carries no `boxes`"}
    frames = []
    for row in boxes:
        if isinstance(row, dict):
            box = row.get("box") or row.get("bbox") or row.get("rect")
            frames.append([box] if box else [])
        elif isinstance(row, list) and row and isinstance(row[0], list):
            frames.append(row)
        else:
            frames.append([row] if len(row or []) >= 4 else [])
    return tracks({**spec, "frames": frames})

# The six readings that need no model and no download, and the door each
# one answers. `models` lists them beside the ONNX rows so an operator can
# see the whole shelf: what runs on any machine, and what a file adds.
CLASSICAL = [
    {"id": "optical-flow", "tool": "measure_motion",
     "signal": "per-frame motion vectors: direction, magnitude, onset",
     "licence": "OpenCV (Apache-2.0)", "bytes": 0,
     "spends": "cut timing, motion sync, SFX sync"},
    {"id": "homography", "tool": "camera_motion",
     "signal": "global camera motion: pan / tilt / zoom / static",
     "licence": "OpenCV (Apache-2.0)", "bytes": 0,
     "spends": "cut continuity, motion preset, stock query"},
    {"id": "derived-shot-scale", "tool": "shot_scale",
     "signal": "wide / medium / close-up from the subject's area",
     "licence": "-", "bytes": 0,
     "spends": "cut rhythm, motion design, caption placement"},
    {"id": "audio-visual-sync", "tool": "visual_sync",
     "signal": "sync score correlating audio onset with flow onset",
     "licence": "-", "bytes": 0,
     "spends": "sound design, cut timing"},
    {"id": "scene-cuts", "tool": "scene_cuts",
     "signal": "shot changes inside a clip: cuts and dissolves, from frame difference",
     "licence": "-", "bytes": 0,
     "spends": "the cut stage: a montage clip is not one take, and a seam is a cut site"},
    {"id": "saliency-contrast", "tool": "saliency",
     "signal": "where the eye lands, as a grid and a focus box",
     "licence": "-", "bytes": 0,
     "spends": "the caption band, which moves away from it"},
    {"id": "palette-kmeans", "tool": "palette",
     "signal": "dominant colours per scene",
     "licence": "-", "bytes": 0,
     "spends": "motion style, continuity, stock similarity"},
    {"id": "noise-estimator", "tool": "visual_quality",
     "signal": "noise variance per clip",
     "licence": "-", "bytes": 0,
     "spends": "stock filter, quality gate"},
    {"id": "rule-of-thirds", "tool": "composition",
     "signal": "composition score and subject placement",
     "licence": "-", "bytes": 0,
     "spends": "stock ranking, caption placement"},
    {"id": "motion-blur", "tool": "motion_blur",
     "signal": "directional blur probability",
     "licence": "-", "bytes": 0,
     "spends": "stock filter, cut veto"},
    {"id": "box-tracks", "tool": "tracks",
     "signal": "persistent identities from per-frame boxes",
     "licence": "-", "bytes": 0,
     "spends": "subject continuity, cut continuity"},
]


def models(spec):
    dirs = [str(candidate) for candidate in _model_candidates(spec)]
    rows = []
    for model in VISUAL_MODELS:
        found = None
        for directory in dirs:
            candidate = Path(directory) / model["file"]
            if candidate.is_file():
                found = str(candidate)
                break
        pin = _pins(spec).get(model["id"]) or {}
        rows.append({
            **model,
            "present": found is not None,
            "path": found,
            "onDiskBytes": Path(found).stat().st_size if found else 0,
            "pinned": pin.get("sha256"),
            # The same readiness `_ready_door` gates every door on: a file
            # on disk, numpy for the tensors AND onnxruntime to run them -
            # a file with no runtime is listed, not ready.
            "ready": found is not None and np is not None and _has_onnxruntime(),
        })
    present = [row["id"] for row in rows if row["present"]]
    missing = [row["id"] for row in rows if not row["present"]]
    return {
        "ok": True,
        "tool": "models",
        "dirs": dirs,
        "numpy": np is not None,
        "onnxruntime": _has_onnxruntime(),
        "models": rows,
        "present": present,
        "missing": missing,
        "ready": len(present),
        "catalogue": len(rows),
        "classical": CLASSICAL,
        "note": "Every door runs when its file is present AND numpy + onnxruntime "
                "are installed; otherwise it refuses with the reason. The "
                "classical readings need neither a file nor a runtime. `download` "
                "is the only thing that fetches: it uses a row's own Hub repo "
                "when the row has one, or `url`/`hfRepo`+`file`, and pins what "
                "arrived.",
    }


def _has_onnxruntime():
    try:
        import onnxruntime  # noqa: F401
        return True
    except Exception:
        return False


HF_API = "https://huggingface.co/api"


def _stamp():
    import time
    return time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())


def urllib_quote(text):
    from urllib.parse import quote
    return quote(str(text), safe="")


def _model_dir(spec, create=True):
    """Where a download lands: the folder the reader reads.

    `modelDir`, then `FUNCUT_VISUAL_MODELS`, then the first app folder that
    already exists, then the repo's own `models/visual` - so a download
    from the model page and a download from the tool land in one place.
    """
    directory = _model_candidates(spec)[0]
    if create:
        directory.mkdir(parents=True, exist_ok=True)
    return directory


def _pin_path(spec):
    return _model_dir(spec, create=False) / "index.json"


def _pins(spec):
    try:
        return json.loads(_pin_path(spec).read_text(encoding="utf-8"))
    except Exception:  # noqa: BLE001 - a missing or broken index is no pins
        return {}


def _pin(spec, model_id, record):
    pins = _pins(spec)
    pins[model_id] = record
    try:
        _pin_path(spec).write_text(json.dumps(pins, indent=2, sort_keys=True),
                                   encoding="utf-8")
    except OSError:
        pass


def _hf_token(spec):
    """The token, from the call or the environment; never echoed back."""
    token = spec.get("token") or os.environ.get("HF_TOKEN") or ""
    return str(token)


def _hf_url(repo, file, revision="main"):
    repo = str(repo or "").strip().strip("/")
    file = str(file or "").strip().lstrip("/")
    if not repo or not file:
        return ""
    if not repo.startswith("http"):
        repo = f"https://huggingface.co/{repo}"
    return f"{repo}/resolve/{revision or 'main'}/{file}"


def _hf_get(path, token="", timeout=30):
    import urllib.request
    request = urllib.request.Request(f"{HF_API}{path}",
                                     headers={"User-Agent": "funcut-visual-sense"})
    if token:
        request.add_header("Authorization", f"Bearer {token}")
    with urllib.request.urlopen(request, timeout=timeout) as reply:
        return json.loads(reply.read().decode("utf-8", "replace"))


def hf_search(spec):
    """Find ONNX repositories on the Hugging Face Hub by name.

    The Hub is the only place this module will look: a repo name and a file
    inside it are what a download needs, and guessing either is how a
    door ends up pointed at the wrong weights.
    """
    query = str(spec.get("query") or spec.get("q") or "").strip()
    if not query:
        return {"ok": False, "error": "give `query`, e.g. \"transnetv2\""}
    limit = max(1, min(50, _int(spec, "limit", 10)))
    try:
        rows = _hf_get(f"/models?search={urllib_quote(query)}&filter=onnx"
                       f"&limit={limit}&full=false", _hf_token(spec))
    except Exception as exc:  # noqa: BLE001 - the network's answer, or none
        return {"ok": False, "error": f"{type(exc).__name__}: {exc}",
                "note": "the Hub is unreachable; pass `hfRepo` and `file` "
                        "directly if you know them"}
    found = []
    for row in rows if isinstance(rows, list) else []:
        found.append({
            "repo": row.get("id") or row.get("modelId"),
            "downloads": row.get("downloads"),
            "likes": row.get("likes"),
            "updated": row.get("lastModified"),
            "gated": bool(row.get("gated")),
            "licence": ((row.get("cardData") or {}).get("license")
                        if isinstance(row.get("cardData"), dict) else None),
        })
    return {"ok": True, "tool": "hf_search", "query": query, "repos": found,
            "note": "Then `hf_files` for one repo: the ONNX files with their "
                    "sizes, and `download` with the repo and the file you pick."}


def hf_files(spec):
    """The ONNX files inside one Hub repo, with sizes when the Hub says."""
    repo = str(spec.get("hfRepo") or spec.get("repo") or "").strip()
    if not repo:
        return {"ok": False, "error": "give `hfRepo`, e.g. \"Xenova/clip-vit-base-patch32\""}
    try:
        row = _hf_get(f"/models/{repo}?blobs=true", _hf_token(spec))
    except Exception as exc:  # noqa: BLE001
        return {"ok": False, "error": f"{type(exc).__name__}: {exc}"}
    files = []
    for item in (row.get("siblings") or []):
        name = item.get("rfilename") or ""
        if not name.lower().endswith(".onnx"):
            continue
        files.append({"file": name, "bytes": item.get("size"),
                      "lfs": bool(item.get("lfs"))})
    files.sort(key=lambda item: item["bytes"] or 0)
    return {"ok": True, "tool": "hf_files", "repo": repo, "files": files[:40],
            "licence": (row.get("cardData") or {}).get("license")
            if isinstance(row.get("cardData"), dict) else None,
            "note": "Pass `hfRepo` and `file` to `download`; a quantized file is "
                    "usually the one a laptop wants."}


def download(spec):
    """Fetch one model file from an address the caller names, and pin it.

    Three ways to name the file, in this order: `url` (any address),
    `hfRepo` + `file` (the Hub's own resolve URL, with `HF_TOKEN` when the
    repo is gated), or the registry's `download` when a row carries one.
    With `sha256` the digest is checked before anything is kept. Without
    it the digest of what arrived is **pinned** in the folder's
    `index.json` and checked on every later `download` of the same file:
    trust on first use, recorded, not silent.
    """
    wanted = spec.get("modelId") or spec.get("id")
    model = _model_row(wanted) if wanted else None
    if model is None and not (spec.get("url") or spec.get("hfRepo")):
        return {"ok": False, "error": f"unknown model id {wanted!r}: one of "
                                      f"{[row['id'] for row in VISUAL_MODELS]}"}
    url = str(spec.get("url") or "").strip()
    if not url and spec.get("hfRepo"):
        url = _hf_url(spec.get("hfRepo"), spec.get("file"), spec.get("revision"))
    if not url and model:
        if model.get("hfRepo") and model.get("hfFile"):
            url = _hf_url(model["hfRepo"], model["hfFile"], spec.get("revision"))
        else:
            url = model.get("download") or ""
    if not url:
        return {"ok": False, "model": model["id"] if model else None,
                "error": "give `url`, or `hfRepo` + `file` (find them with "
                         "`hf_search` and `hf_files`): this module does not "
                         "invent download links, and an upstream page is not "
                         "the file",
                "upstream": model["upstream"] if model else None,
                "file": model["file"] if model else None}
    # A registry row names its own file: the door looks for that name, so a
    # Hub file called `model_quantized.onnx` is saved as the door's file.
    # Only a repo with no row keeps the Hub's own basename.
    if model:
        name = model["file"]
    else:
        name = str(spec.get("file") or url.rsplit("/", 1)[-1].split("?")[0]
                   or "model.onnx")
        if spec.get("hfRepo") and spec.get("file"):
            name = str(spec["file"]).rsplit("/", 1)[-1]
    directory = _model_dir(spec)
    target = directory / name
    partial = target.with_suffix(target.suffix + ".part")
    key = model["id"] if model else name
    pinned = (_pins(spec).get(key) or {}).get("sha256", "")
    try:
        import hashlib
        import urllib.request
        request = urllib.request.Request(url, headers={"User-Agent": "funcut-visual-sense"})
        token = _hf_token(spec)
        if token and "huggingface.co" in url:
            request.add_header("Authorization", f"Bearer {token}")
        digest = hashlib.sha256()
        written = 0
        with urllib.request.urlopen(request, timeout=180) as reply, open(partial, "wb") as sink:
            while True:
                chunk = reply.read(1 << 20)
                if not chunk:
                    break
                digest.update(chunk)
                sink.write(chunk)
                written += len(chunk)
        actual = digest.hexdigest()
        expected = str(spec.get("sha256") or pinned or "").lower().strip()
        if expected and actual != expected:
            partial.unlink(missing_ok=True)
            return {"ok": False, "model": model["id"] if model else name,
                    "error": f"digest mismatch: got {actual}, expected {expected}; "
                             f"nothing was kept"}
        partial.replace(target)
        _pin(spec, key, {"sha256": actual, "bytes": written, "url": url,
                         "file": name, "at": _stamp()})
        return {"ok": True, "tool": "download", "model": model["id"] if model else name,
                "path": str(target), "bytes": written, "sha256": actual,
                "verified": bool(expected), "pinned": not bool(expected),
                "licence": model["licence"] if model else None,
                "note": ("Digest matched the one given." if expected else
                         "No digest was given, so the one that arrived is now "
                         "pinned in index.json and checked on every later "
                         "download of this file. Re-run `models` to see the "
                         "door ready.")}
    except Exception as exc:  # noqa: BLE001 - the network's answer, or none
        partial.unlink(missing_ok=True)
        return {"ok": False, "model": model["id"] if model else name,
                "error": f"{type(exc).__name__}: {exc}"}


TOOLS = {
    "measure_motion": measure_motion,
    "camera_motion": camera_motion,
    "shot_scale": shot_scale,
    "visual_sync": visual_sync,
    "scene_cuts": scene_cuts,
    "visual_quality": visual_quality,
    "palette": palette,
    "saliency": saliency,
    "composition": composition,
    "motion_blur": motion_blur,
    "tracks": tracks_from_file,
    "measure_clips": measure_clips,
    "rank_frames": rank_frames,
    "bench": bench,
    "models": models,
    "download": download,
    "hf_search": hf_search,
    "hf_files": hf_files,
    "action_class": action_class,
    "shot_boundaries": shot_boundaries,
    "film_shot_type": film_shot_type,
    "video_embedding": video_embedding,
    "scene_graph": scene_graph,
    "depth_map": depth_map,
    "subject_mask": subject_mask,
    "semantic_segments": semantic_segments,
    "detect_objects": detect_objects,
    "optical_flow": optical_flow,
    "visual_embedding": visual_embedding,
    "aesthetic": aesthetic,
    "video_saliency": video_saliency,
    "blur_mask": blur_mask,
    "pose": pose,
    "hands": hands,
    "gaze": gaze,
    "face_emotion": face_emotion,
    "lip_sync": lip_sync,
    "action_quality": action_quality,
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


def _smear_x(rgb, w, h, taps=17):
    """A horizontal smear: what a pan does to a frame in one exposure."""
    out = bytearray(len(rgb))
    half = taps // 2
    for y in range(h):
        for x in range(w):
            acc = [0, 0, 0]
            for t in range(-half, half + 1):
                sx = min(w - 1, max(0, x + t))
                i = (y * w + sx) * 3
                for c in range(3):
                    acc[c] += rgb[i + c]
            j = (y * w + x) * 3
            for c in range(3):
                out[j + c] = acc[c] // taps
    return bytes(out)


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
    check("the model shelf lists every door",
          table["ok"] and len(table["models"]) == len(VISUAL_MODELS)
          and len(table["classical"]) >= 6,
          f"{len(table.get('models', []))} models, "
          f"{len(table.get('classical', []))} classical")
    rows_ok = all(
        row.get("id") and row.get("file") and isinstance(row.get("bytes"), int)
        and row.get("licence") and row.get("tool") in TOOLS
        and row.get("signal") and row.get("upstream") and row.get("tier")
        and row.get("door", {}).get("decoder") in DECODERS
        for row in VISUAL_MODELS)
    check("every model row names a file, a licence, a tool and a decoder", rows_ok,
          [row["id"] for row in VISUAL_MODELS
           if row.get("tool") not in TOOLS or
           row.get("door", {}).get("decoder") not in DECODERS])
    check("the classical shelf names the readings that need no file",
          {row["tool"] for row in CLASSICAL} <= set(TOOLS),
          sorted(row["tool"] for row in CLASSICAL if row["tool"] not in TOOLS))
    fetched = download({"modelId": "film-shot-classifier"})
    check("a download with no address refuses and names the file",
          (not fetched["ok"]) and fetched["file"] == "film-shot-classifier.onnx",
          fetched.get("error", "")[:60])

    if np is not None:
        depth = _decode_depth(np.linspace(0, 1, 16 * 16).reshape(16, 16))
        check("a depth map reads near and far",
              depth["nearShare"] > 0 and depth["farShare"] > 0
              and depth["verdict"].startswith("nearer"), depth["verdict"])
        unit = _decode_embedding(np.arange(8, dtype=np.float32))
        check("an embedding is unit length",
              unit["dim"] == 8 and abs(sum(v * v for v in unit["vector"]) - 1) < 1e-3,
              unit["dim"])
        top = _decode_logits(np.array([0.1, 2.0, 0.3], dtype=np.float32),
                             ["a", "b", "c"])
        check("logits name their winner", top["top"][0]["label"] == "b"
              and top["top"][0]["p"] > 0.5, top["top"][0])
        curve = _decode_prob(np.array([0.0, 0.1, 0.9, 0.05], dtype=np.float32),
                             times=[0.0, 0.5, 1.0, 1.5])
        check("a boundary curve names the frame it peaks on",
              curve["boundaryCount"] == 1 and abs(curve["peakAt"] - 1.0) < 1e-6,
              curve)
        mask = _decode_masks(np.pad(np.ones((4, 4), dtype=np.float32), 2))
        check("a mask reports the area it covers",
              0.1 < mask["areaShare"] < 0.9 and mask["present"], mask["areaShare"])
        boxes = _decode_boxes(np.array([[0.1, 0.1, 0.4, 0.4, 0.9, 0],
                                        [0.0, 0.0, 0.1, 0.1, 0.1, 1]], dtype=np.float32),
                              ["person", "laptop"])
        check("boxes keep the confident one and drop the weak one",
              boxes["count"] == 1 and boxes["objects"][0]["label"] == "person", boxes)
        keys = _decode_keypoints(np.array([[0.2, 0.3, 0.9], [0.4, 0.5, 0.8]],
                                          dtype=np.float32))
        check("keypoints report where the body is",
              keys["keypoints"] == 2 and 0.2 <= keys["centre"][0] <= 0.4, keys["bbox"])
        seg = _decode_segments(np.stack([np.zeros((4, 4)), np.ones((4, 4))]) * 3)
        check("segments report the dominant class", seg["shares"][0]["index"] == 1, seg)
        score = _decode_scalar(np.array([0.72], dtype=np.float32))
        check("a scalar aesthetic reads 0-10", abs(score["score"] - 7.2) < 0.01, score)
        flow = _decode_flow(np.stack([np.ones((4, 4)), np.zeros((4, 4))]) * 0.5)
        check("a flow field reads its own magnitude and direction",
              flow["mean"] > 0 and flow["direction"] == "right", flow["direction"])
        check("every decoder in the shelf has a proof above",
              set(DECODERS) <= {"depth", "embedding", "logits", "prob", "masks",
                                "boxes", "keypoints", "segments", "scalar", "flow"},
              sorted(DECODERS))

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

    composed = _composition_rows(
        [(0.0, _draw_square(pattern, w, h, (250, 250, 250), 14, 9, 6), w, h)],
        32)[0]
    centred = _composition_rows(
        [(0.0, _draw_square(pattern, w, h, (250, 250, 250), 21, 12, 6), w, h)],
        32)[0]
    check("composition tells a third from the centre",
          composed["score"] > centred["score"],
          f"third={composed['score']} centre={centred['score']}")
    subject = _draw_square(pattern, w, h, (250, 250, 250), 20, 9, 6)
    sharp_row = _blur_rows([(0.0, subject, w, h)], 32)[0]
    blur_row = _blur_rows([(0.0, _smear_x(subject, w, h), w, h)], 32)[0]
    check("a smeared frame reads as blur, a sharp one does not",
          (not sharp_row["blurred"]) and blur_row["blurred"]
          and blur_row["ratio"] > sharp_row["ratio"],
          f"ratio {blur_row['ratio']} vs {sharp_row['ratio']}")
    linked = tracks({"frames": [[[0.1, 0.1, 0.3, 0.3]], [[0.12, 0.1, 0.32, 0.3]],
                                 [[0.14, 0.1, 0.34, 0.3]]]})
    jumped = tracks({"frames": [[[0.1, 0.1, 0.3, 0.3]], [[0.7, 0.6, 0.9, 0.8]]]})
    check("a subject that stays put keeps one id; a jump starts a second",
          linked["count"] == 1 and jumped["count"] == 2,
          f"{linked['count']} / {jumped['count']}")
    check("a track's coverage is its share of the clip",
          abs(linked["main"]["coverage"] - 1.0) < 1e-6, linked["main"])
    vetted = _visual_score({"peak": 0.2, "peakAt": 0.2},
                           {"sharpness": 25, "noise": 1, "letterbox": 0,
                            "verdict": "pass"}, "cut", {"cuts": 0},
                           {"score": 0.9}, {"verdict": "directional blur"})
    clean = _visual_score({"peak": 0.2, "peakAt": 0.2},
                          {"sharpness": 25, "noise": 1, "letterbox": 0,
                           "verdict": "pass"}, "cut", {"cuts": 0},
                          {"score": 0.9}, {"verdict": "clean"})
    check("a blurred candidate sinks behind a clean one", vetted < clean,
          f"{vetted} < {clean}")

    # Every door on the shelf, through the real runner, with a stub session
    # standing in for the weights: the row's preprocessing, the session call
    # and the row's decoder all run, so a door that cannot decode its own
    # output shape fails here rather than on a user's clip.
    if np is not None:
        shapes = {
            "depth": np.linspace(0, 1, 32 * 32).reshape(1, 32, 32),
            "embedding": np.arange(40, dtype=np.float32).reshape(1, 5, 8),
            "logits": np.array([[0.1, 1.4, 0.3, 0.2]], dtype=np.float32),
            "prob": np.array([[[0.1], [0.8], [0.2]]], dtype=np.float32),
            "masks": np.ones((1, 1, 16, 16), dtype=np.float32),
            "boxes": np.array([[0.1, 0.1, 0.5, 0.5, 0.9, 0],
                               [0.2, 0.2, 0.3, 0.3, 0.2, 1]], dtype=np.float32),
            "keypoints": np.array([[0.2, 0.3, 0.9], [0.4, 0.5, 0.8]], dtype=np.float32),
            "segments": np.ones((1, 3, 16, 16), dtype=np.float32),
            "scalar": np.array([[0.7]], dtype=np.float32),
            "flow": np.ones((1, 2, 8, 8), dtype=np.float32) * 0.5,
        }

        class _Stub:
            def __init__(self, array):
                self.array = array

            def get_inputs(self):
                return [type("In", (), {"name": "x", "shape": [1, 3, 32, 32]})()]

            def get_outputs(self):
                return [type("Out", (), {"name": "y", "shape": list(self.array.shape)})()]

            def run(self, _names, _feeds):
                return [self.array]

        import tempfile
        handle = tempfile.NamedTemporaryFile(suffix=".onnx", delete=False)
        handle.write(b"stub")
        handle.close()
        original = _onnx_session
        stub_frames = [(0.0, pattern, w, h)]
        failures = []
        for row in VISUAL_MODELS:
            decoder = row["door"]["decoder"]
            if row.get("vlm"):
                # A VLM row is refused by the runner on purpose: it is a
                # chat model, not a graph, and `describe_scene` is its door.
                said = _generic_door({"model": handle.name, "frames": None},
                                     row["id"], frames=stub_frames)
                if said.get("ok") or "describe_scene" not in said.get("error", ""):
                    failures.append(f"{row['id']}: the VLM row was not refused")
                continue
            _SESSIONS.clear()
            globals()["_onnx_session"] = lambda _path, _a=shapes[decoder]: _Stub(_a)
            try:
                said = _generic_door({"model": handle.name, "size": 32, "time": 4},
                                     row["id"], frames=stub_frames)
            finally:
                globals()["_onnx_session"] = original
            if not said.get("ok") or not said.get("decoded"):
                failures.append(f"{row['id']}: {said.get('error', 'no reading')}")
        os.unlink(handle.name)
        check("every door on the shelf runs its own row end to end", not failures,
              failures)
        check("the shelf names a decoder every row uses",
              all(row["door"]["decoder"] in DECODERS for row in VISUAL_MODELS),
              [row["id"] for row in VISUAL_MODELS
               if row["door"]["decoder"] not in DECODERS])

        # A Hub repo is what the model page needs, and the URL is the Hub's
        # own resolve path - never a guessed file.
        hub = _hf_url("onnx-community/dinov2-small", "onnx/model_quantized.onnx")
        check("a Hub row builds its own resolve url",
              hub == ("https://huggingface.co/onnx-community/dinov2-small/"
                      "resolve/main/onnx/model_quantized.onnx"), hub)
        check("a row with a verified Hub repo can be fetched with one call",
              all(row.get("hfRepo") and row.get("hfFile")
                  for row in VISUAL_MODELS if row["id"] in
                  ("transnetv2", "depth-anything-v2-small", "dinov2-small",
                   "clip-vit-b32")),
              [row["id"] for row in VISUAL_MODELS if row.get("hfRepo")
               and not row.get("hfFile")])
        # A local file:// address, so the check never reaches the network:
        # this selftest runs in a CI job with none.
        said = download({"modelId": "transnetv2", "modelDir": "/tmp/none",
                         "url": "file:///nonexistent/weights.onnx"})
        check("a download that cannot reach its file keeps nothing",
              not said["ok"] and "sha256" not in said, said.get("error", "")[:40])

        # The pin is what makes a digest-less download checkable next time.
        pin_dir = Path("/tmp/funcut-pin-selftest")
        pin_dir.mkdir(parents=True, exist_ok=True)
        _pin({"modelDir": str(pin_dir)}, "probe", {"sha256": "abc", "file": "x.onnx"})
        check("a pin is recorded and read back",
              (_pins({"modelDir": str(pin_dir)}).get("probe") or {}).get("sha256")
              == "abc", _pins({"modelDir": str(pin_dir)}))
        import shutil
        shutil.rmtree(pin_dir, ignore_errors=True)

        check("the sheet enriches nothing when no door is installed",
              _enrich({"modelDir": "/tmp/no-doors-here"}, stub_frames) == {},
              _enrich({"modelDir": "/tmp/no-doors-here"}, stub_frames))
        tuned = _visual_score({"peak": 0.2, "peakAt": 0.2},
                              {"sharpness": 25, "noise": 1, "letterbox": 0,
                               "verdict": "pass"}, "stock", {"cuts": 0},
                              {"score": 0.5}, {"verdict": "clean"}, aesthetic=9.5)
        plain = _visual_score({"peak": 0.2, "peakAt": 0.2},
                              {"sharpness": 25, "noise": 1, "letterbox": 0,
                               "verdict": "pass"}, "stock", {"cuts": 0},
                              {"score": 0.5}, {"verdict": "clean"})
        check("a measured taste prior lifts a candidate, and never vetoes",
              tuned > plain and plain > 0, f"{tuned} > {plain}")

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
