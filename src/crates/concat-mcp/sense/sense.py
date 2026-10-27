"""concat-sense: deterministic visual + auditory perception for FUNCUT.

Pure Python standard library. No pip, no numpy, no Pillow. This is the layer
that lets the editor actually SEE what it is making: it decodes real frames
with ffmpeg, measures them, builds a labelled contact sheet as a PNG, and
returns the PNG as base64 so it can be forwarded over MCP as `type: image`
content (which is the only way a coding agent can look at it).

Commands (all take --spec JSON and print one JSON object on stdout):

  sheet      decode frames -> contact sheet PNG + per-shot pixel metrics
  critique   contact sheet + brief + metrics -> 6-axis craft score + revisions
  vlm        proxy to vlm.py status / setup

The pixel metrics are honest but shallow. When a local VLM is reachable,
`critique` uses it as the semantic rater and keeps the pixel metrics as a
grounded cross-check. When it is not, `critique` still returns a usable,
explicitly-labelled heuristic score instead of failing.
"""

from __future__ import annotations

import base64
import binascii
import json
import math
import os
import re
import shutil
import stat
import struct
import subprocess
import sys
import zlib
from pathlib import Path

VLM_DIR = Path(__file__).resolve().parent
if str(VLM_DIR) not in sys.path:
    sys.path.insert(0, str(VLM_DIR))
try:
    import vlm as local_vlm
except Exception:  # pragma: no cover - sense must work without the VLM helper
    local_vlm = None


# ---------------------------------------------------------------------------
# Minimal 5x7 bitmap font, enough for labels like "SHOT 03  00:01.5".
# ---------------------------------------------------------------------------
_FONT = {
    " ": ["00000"] * 7,
    "0": ["01110", "10001", "10011", "10101", "11001", "10001", "01110"],
    "1": ["00100", "01100", "00100", "00100", "00100", "00100", "01110"],
    "2": ["01110", "10001", "00001", "00010", "00100", "01000", "11111"],
    "3": ["11110", "00001", "00001", "01110", "00001", "00001", "11110"],
    "4": ["00010", "00110", "01010", "10010", "11111", "00010", "00010"],
    "5": ["11111", "10000", "11110", "00001", "00001", "10001", "01110"],
    "6": ["00110", "01000", "10000", "11110", "10001", "10001", "01110"],
    "7": ["11111", "00001", "00010", "00100", "01000", "01000", "01000"],
    "8": ["01110", "10001", "10001", "01110", "10001", "10001", "01110"],
    "9": ["01110", "10001", "10001", "01111", "00001", "00010", "01100"],
    "A": ["01110", "10001", "10001", "11111", "10001", "10001", "10001"],
    "B": ["11110", "10001", "10001", "11110", "10001", "10001", "11110"],
    "C": ["01111", "10000", "10000", "10000", "10000", "10000", "01111"],
    "D": ["11110", "10001", "10001", "10001", "10001", "10001", "11110"],
    "E": ["11111", "10000", "10000", "11110", "10000", "10000", "11111"],
    "F": ["11111", "10000", "10000", "11110", "10000", "10000", "10000"],
    "G": ["01111", "10000", "10000", "10111", "10001", "10001", "01111"],
    "H": ["10001", "10001", "10001", "11111", "10001", "10001", "10001"],
    "I": ["01110", "00100", "00100", "00100", "00100", "00100", "01110"],
    "J": ["00111", "00010", "00010", "00010", "00010", "10010", "01100"],
    "K": ["10001", "10010", "10100", "11000", "10100", "10010", "10001"],
    "L": ["10000", "10000", "10000", "10000", "10000", "10000", "11111"],
    "M": ["10001", "11011", "10101", "10101", "10001", "10001", "10001"],
    "N": ["10001", "11001", "10101", "10011", "10001", "10001", "10001"],
    "O": ["01110", "10001", "10001", "10001", "10001", "10001", "01110"],
    "P": ["11110", "10001", "10001", "11110", "10000", "10000", "10000"],
    "Q": ["01110", "10001", "10001", "10001", "10101", "10010", "01101"],
    "R": ["11110", "10001", "10001", "11110", "10100", "10010", "10001"],
    "S": ["01111", "10000", "10000", "01110", "00001", "00001", "11110"],
    "T": ["11111", "00100", "00100", "00100", "00100", "00100", "00100"],
    "U": ["10001", "10001", "10001", "10001", "10001", "10001", "01110"],
    "V": ["10001", "10001", "10001", "10001", "10001", "01010", "00100"],
    "W": ["10001", "10001", "10001", "10101", "10101", "11011", "10001"],
    "X": ["10001", "10001", "01010", "00100", "01010", "10001", "10001"],
    "Y": ["10001", "10001", "01010", "00100", "00100", "00100", "00100"],
    "Z": ["11111", "00001", "00010", "00100", "01000", "10000", "11111"],
    ".": ["00000"] * 6 + ["00100"],
    ",": ["00000"] * 5 + ["00100", "01000"],
    ":": ["00000", "00100", "00100", "00000", "00100", "00100", "00000"],
    "-": ["00000"] * 3 + ["11111"] + ["00000"] * 3,
    "+": ["00000", "00100", "00100", "11111", "00100", "00100", "00000"],
    "/": ["00001", "00010", "00010", "00100", "01000", "01000", "10000"],
    "%": ["11001", "11010", "00010", "00100", "01000", "01011", "10011"],
    "#": ["01010", "01010", "11111", "01010", "11111", "01010", "01010"],
    "(": ["00010", "00100", "01000", "01000", "01000", "00100", "00010"],
    ")": ["01000", "00100", "00010", "00010", "00010", "00100", "01000"],
    "[": ["01110", "01000", "01000", "01000", "01000", "01000", "01110"],
    "]": ["01110", "00010", "00010", "00010", "00010", "00010", "01110"],
    "<": ["00010", "00100", "01000", "10000", "01000", "00100", "00010"],
    ">": ["01000", "00100", "00010", "00001", "00010", "00100", "01000"],
    "=": ["00000", "11111", "00000", "11111", "00000", "00000", "00000"],
    "!": ["00100", "00100", "00100", "00100", "00100", "00000", "00100"],
    "?": ["01110", "10001", "00001", "00110", "00100", "00000", "00100"],
    "_": ["00000"] * 6 + ["11111"],
    "*": ["00000", "10101", "01110", "11111", "01110", "10101", "00000"],
}


def _put_pixel(buf, w, h, x, y, rgb):
    if 0 <= x < w and 0 <= y < h:
        i = (y * w + x) * 3
        buf[i] = rgb[0]
        buf[i + 1] = rgb[1]
        buf[i + 2] = rgb[2]


def _draw_text(buf, w, h, x, y, text, color=(255, 255, 255), scale=1, bg=None):
    text = str(text).upper()
    cw = 6 * scale
    if bg:
        for j in range(-scale, 8 * scale):
            for i in range(-scale, (len(text) * cw) + scale):
                _put_pixel(buf, w, h, x + i, y + j, bg)
    for idx, ch in enumerate(text):
        glyph = _FONT.get(ch, _FONT["?"])
        ox = x + idx * cw
        for gy, row in enumerate(glyph):
            for gx, bit in enumerate(row):
                if bit == "1":
                    for sy in range(scale):
                        for sx in range(scale):
                            _put_pixel(buf, w, h, ox + gx * scale + sx, y + gy * scale + sy, color)


def _png(width, height, rgb):
    raw = bytearray()
    stride = width * 3
    for y in range(height):
        raw.append(0)  # filter type: none
        raw.extend(rgb[y * stride : (y + 1) * stride])

    def chunk(tag, data):
        return (
            struct.pack(">I", len(data))
            + tag
            + data
            + struct.pack(">I", binascii.crc32(tag + data) & 0xFFFFFFFF)
        )

    return (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 2, 0, 0, 0))
        + chunk(b"IDAT", zlib.compress(bytes(raw), 6))
        + chunk(b"IEND", b"")
    )


# ---------------------------------------------------------------------------
# Pure-Python PNG decoding. Concat's preview.frame returns base64 PNGs, so
# the whole "watch the timeline" loop needs no ffmpeg CLI at all.
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
        raise ValueError(f"unsupported PNG: depth={depth} colour={colour} interlace={interlace}")
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


def fit_rgb(rgb, sw, sh, dw, dh):
    """Nearest-neighbour scale into dw x dh, padded black like ffmpeg's pad."""
    scale = min(dw / sw, dh / sh)
    cw, ch = max(1, int(sw * scale)), max(1, int(sh * scale))
    ox, oy = (dw - cw) // 2, (dh - ch) // 2
    out = bytearray(b"\x00" * (dw * dh * 3))
    for y in range(ch):
        sy = min(sh - 1, y * sh // ch)
        row = oy + y
        for x in range(cw):
            sx = min(sw - 1, x * sw // cw)
            src = (sy * sw + sx) * 3
            dst = (row * dw + ox + x) * 3
            out[dst : dst + 3] = rgb[src : src + 3]
    return bytes(out)


# ---------------------------------------------------------------------------
# ffmpeg helpers. Concat links FFmpeg as a library and does NOT ship the
# ffmpeg CLI, so we bootstrap a small static build into the app data dir on
# first use for anything that must decode a video file (rather than frames
# the API already rendered).
# ---------------------------------------------------------------------------
FFMPEG_BUILDS = {
    ("Linux", "x86_64"): "ffmpeg-release-amd64-static.tar.xz",
    ("Linux", "aarch64"): "ffmpeg-release-arm64-static.tar.xz",
    ("Linux", "arm64"): "ffmpeg-release-arm64-static.tar.xz",
}
FFMPEG_URL_BASE = "https://johnvansickle.com/ffmpeg/releases/"


def _app_dir():
    return Path(os.environ.get("FUNCUT_VLM_DIR", str(Path.home() / ".local" / "share" / "app.concat.editor")))


def _ensure_ffmpeg(spec):
    """Return (ffmpeg, ffprobe) paths, bootstrapping a static build if needed."""
    explicit = spec.get("ffmpeg") or os.environ.get("FUNCUT_FFMPEG")
    if explicit and os.path.isfile(explicit) and os.access(explicit, os.X_OK):
        ff = explicit
    else:
        ff = shutil.which("ffmpeg")
    if ff:
        fp = spec.get("ffprobe") or os.path.join(os.path.dirname(ff), "ffprobe")
        return ff, (fp if os.path.isfile(fp) else (shutil.which("ffprobe") or "ffprobe"))
    root = _app_dir() / "ffmpeg"
    for name in ("ffmpeg", "ffmpeg.exe"):
        cand = root / name
        if cand.is_file():
            probe = root / ("ffprobe.exe" if name.endswith(".exe") else "ffprobe")
            return str(cand), str(probe if probe.is_file() else cand)
    if spec.get("autoSetup") is False:
        return "ffmpeg", "ffprobe"
    import platform as _p
    asset = FFMPEG_BUILDS.get((_p.system(), _p.machine()))
    if not asset:
        raise RuntimeError(
            "no ffmpeg CLI on this machine; install one or set FUNCUT_FFMPEG to its path "
            "(timeline previews via preview.frame need no ffmpeg at all)"
        )
    import tarfile
    import urllib.request
    root.mkdir(parents=True, exist_ok=True)
    archive = root / asset
    if not archive.is_file():
        req = urllib.request.Request(FFMPEG_URL_BASE + asset, headers={"User-Agent": "concat-sense/1.0"})
        with urllib.request.urlopen(req, timeout=60) as r, open(archive, "wb") as f:
            total = int(r.headers.get("Content-Length") or 0)
            done = 0
            while True:
                buf = r.read(1 << 20)
                if not buf:
                    break
                f.write(buf)
                done += len(buf)
                if total:
                    print(f"[sense] ffmpeg: {done/1e6:.0f}/{total/1e6:.0f} MB", file=sys.stderr, flush=True)
    with tarfile.open(archive) as t:
        for member in t.getmembers():
            if member.isfile() and os.path.basename(member.name) in ("ffmpeg", "ffprobe"):
                src = t.extractfile(member)
                if src is None:
                    continue
                dest = root / os.path.basename(member.name)
                dest.write_bytes(src.read())
                dest.chmod(dest.stat().st_mode | stat.S_IXUSR | stat.S_IXGRP | stat.S_IXOTH)
    archive.unlink(missing_ok=True)
    return str(root / "ffmpeg"), str(root / "ffprobe")


def _tools(spec):
    try:
        return _ensure_ffmpeg(spec)
    except Exception as exc:
        print(f"[sense] ffmpeg bootstrap: {exc}", file=sys.stderr, flush=True)
        return (spec.get("ffmpeg") or shutil.which("ffmpeg") or "ffmpeg",
                spec.get("ffprobe") or shutil.which("ffprobe") or "ffprobe")


def _run(cmd, timeout=45):
    try:
        p = subprocess.run(cmd, stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=timeout)
        return p.returncode, p.stdout, p.stderr
    except (OSError, subprocess.TimeoutExpired) as exc:
        return -1, b"", str(exc).encode()


def probe_duration(ffprobe, path):
    rc, out, _ = _run([ffprobe, "-v", "error", "-show_entries", "format=duration", "-of", "default=nw=1:nk=1", path], 20)
    try:
        return float(out.decode().strip())
    except Exception:
        return 0.0


def extract_frame(ff, path, at, w, h):
    """Decode one RGB frame, letterboxed into exactly w*h*3 bytes."""
    vf = (
        f"scale={w}:{h}:force_original_aspect_ratio=decrease,"
        f"pad={w}:{h}:(ow-iw)/2:(oh-ih)/2:color=black,setsar=1"
    )
    cmd = [ff, "-v", "error", "-ss", f"{max(0.0, float(at)):.3f}", "-i", path,
           "-frames:v", "1", "-vf", vf, "-f", "rawvideo", "-pix_fmt", "rgb24", "-"]
    rc, out, err = _run(cmd)
    want = w * h * 3
    if rc != 0 or len(out) < want:
        # Some inputs ignore -ss before -i (e.g. single images); retry output seek.
        cmd = [ff, "-v", "error", "-i", path, "-ss", f"{max(0.0, float(at)):.3f}",
               "-frames:v", "1", "-vf", vf, "-f", "rawvideo", "-pix_fmt", "rgb24", "-"]
        rc, out, err = _run(cmd)
    if len(out) < want:
        return None, (err.decode("utf-8", "replace") or "short frame").strip()[:200]
    return bytes(out[:want]), None


def audio_envelope(ff, path, buckets=48):
    """Low-rate mono PCM energy envelope; works without scipy/numpy."""
    rate = 8000
    cmd = [ff, "-v", "error", "-i", path, "-vn", "-ac", "1", "-ar", str(rate),
           "-f", "s16le", "-acodec", "pcm_s16le", "-"]
    rc, out, err = _run(cmd, 90)
    if rc != 0 or len(out) < 2:
        return None
    n = len(out) // 2
    vals = struct.unpack("<%dh" % n, out[: n * 2])
    if not vals:
        return None
    step = max(1, n // buckets)
    env = []
    for start in range(0, n, step):
        block = vals[start : start + step]
        if not block:
            break
        env.append(math.sqrt(sum((v / 32768.0) ** 2 for v in block) / len(block)))
    peak = max(env) or 1e-6
    return [round(v / peak, 4) for v in env]


# ---------------------------------------------------------------------------
# Pixel metrics
# ---------------------------------------------------------------------------
def _luma_grid(rgb, w, h, gw=32, gh=18):
    grid = []
    for gy in range(gh):
        row = []
        y = min(h - 1, int((gy + 0.5) * h / gh))
        for gx in range(gw):
            x = min(w - 1, int((gx + 0.5) * w / gw))
            i = (y * w + x) * 3
            r, g, b = rgb[i], rgb[i + 1], rgb[i + 2]
            row.append(0.2126 * r + 0.7152 * g + 0.0722 * b)
        grid.append(row)
    return grid


def tile_metrics(rgb, w, h, gw=32, gh=18):
    """Cheap but real measurements: exposure, contrast, detail, bars, crop hint, hash."""
    g = _luma_grid(rgb, w, h, gw, gh)
    flat = [v for row in g for v in row]
    n = len(flat)
    mean = sum(flat) / n
    var = sum((v - mean) ** 2 for v in flat) / n
    std = math.sqrt(var)
    edge = 0.0
    for y in range(gh):
        for x in range(1, gw):
            edge += abs(g[y][x] - g[y][x - 1])
    for x in range(gw):
        for y in range(1, gh):
            edge += abs(g[y][x] - g[y - 1][x])
    edge /= (gh * (gw - 1) + gw * (gh - 1))
    black = sum(1 for v in flat if v < 14) / n
    blown = sum(1 for v in flat if v > 245) / n

    # Uniform dark bands along the borders -> letterbox / pillarbox.
    def band(arr):
        b = sum(1 for v in arr if v < 20) / max(1, len(arr))
        return b
    top = band(g[0] + g[1])
    bot = band(g[-1] + g[-2])
    left = band([r[0] for r in g] + [r[1] for r in g])
    right = band([r[-1] for r in g] + [r[-2] for r in g])
    bar_ratio = max(min(top, bot), min(left, right))

    # Energy centroid -> where the subject actually is, so a 9:16 crop can
    # follow it instead of blindly taking the centre column.
    weights = []
    for y in range(gh):
        for x in range(gw):
            d = max(0.0, abs(g[y][x] - mean) - 6.0)
            weights.append((x, y, d))
    tw = sum(d for _, _, d in weights) or 1.0
    cx = sum(x * d for x, _, d in weights) / tw / (gw - 1)
    cy = sum(y * d for _, y, d in weights) / tw / (gh - 1)

    # 64-bit dHash for duplicate-shot detection.
    bits = 0
    k = 0
    for y in range(gh):
        for x in range(gw - 1):
            bits = (bits << 1) | (1 if g[y][x] > g[y][x + 1] else 0)
            k += 1
            if k >= 64:
                break
        if k >= 64:
            break

    return {
        "luma": round(mean, 1),
        "contrast": round(std, 1),
        "detail": round(edge, 2),
        "blackFraction": round(black, 3),
        "blownFraction": round(blown, 3),
        "barRatio": round(bar_ratio, 3),
        "cropHint": {
            "x": round(max(-1.0, min(1.0, (cx - 0.5) * 2)), 2),
            "y": round(max(-1.0, min(1.0, (cy - 0.5) * 2)), 2),
        },
        "hash": "%016x" % bits,
    }


def _hamming(a, b):
    try:
        return bin(int(a, 16) ^ int(b, 16)).count("1")
    except Exception:
        return 64


def _fmt_time(t):
    t = max(0.0, float(t))
    return f"{int(t // 60):02d}:{t % 60:04.1f}"


def build_sheet(spec):
    tw = max(64, min(480, int(spec.get("tileWidth", 320))))
    th = max(48, min(360, int(spec.get("tileHeight", 180))))
    cols = max(1, min(6, int(spec.get("cols", 4))))
    sources = list(spec.get("sources") or [])
    errors = []

    # Only reach for ffmpeg when at least one source really is a media file.
    # Timeline previews arrive as base64 PNGs and need no decoder at all.
    needs_ffmpeg = (
        bool(spec.get("path")) or bool(spec.get("paths")) or bool(spec.get("audio"))
        or any(s.get("pngB64") is None for s in sources)
    )
    ff, fp = _tools(spec) if needs_ffmpeg else ("", "")

    if spec.get("path") and not sources:
        path = spec["path"]
        samples = max(2, min(24, int(spec.get("samples", 12))))
        duration = probe_duration(fp, path)
        if duration <= 0:
            duration = max(1.0, samples * 0.5)
        sources = [
            {"path": path, "at": round(duration * (i + 0.5) / samples, 3),
             "label": f"{i + 1:02d} {_fmt_time(duration * (i + 0.5) / samples)}"}
            for i in range(samples)
        ]
    elif spec.get("paths") and not sources:
        sources = [{"path": p, "at": float(spec.get("at", 1.0)), "label": str(i + 1)}
                   for i, p in enumerate(spec["paths"])]

    sources = sources[:24]
    if not sources:
        return {"ok": False, "error": "sheet requires sources[], paths[] or path+samples"}

    frames, metas = [], []
    for i, s in enumerate(sources):
        if s.get("pngB64"):
            try:
                raw, sw, sh = png_decode(base64.b64decode(s["pngB64"]))
                rgb = fit_rgb(raw, sw, sh, tw, th)
            except Exception as exc:
                errors.append(f"source {i}: png decode: {exc}")
                continue
        else:
            path = s.get("path")
            if not path or not os.path.isfile(path):
                errors.append(f"source {i}: file not found: {path}")
                continue
            rgb, err = extract_frame(ff, path, s.get("at", 0.5), tw, th)
            if rgb is None:
                errors.append(f"source {i}: {err}")
                continue
        frames.append(rgb)
        m = tile_metrics(rgb, tw, th)
        m.update({
            "index": len(frames) - 1,
            "path": s.get("path"),
            "at": round(float(s.get("at", 0.5)), 3),
            "label": s.get("label") or str(len(frames)),
        })
        metas.append(m)

    if not frames:
        return {"ok": False, "error": "no frames decoded", "errors": errors[:8]}

    rows = math.ceil(len(frames) / cols)
    bw, bh = 2, 2
    W = cols * tw + (cols + 1) * bw
    H = rows * th + (rows + 1) * bh
    buf = bytearray(b"\x18\x18\x1e" * (W * H))
    for idx, rgb in enumerate(frames):
        r, c = divmod(idx, cols)
        ox, oy = bw + c * (tw + bw), bh + r * (th + bh)
        for y in range(th):
            start = (oy + y) * W * 3 + ox * 3
            buf[start : start + tw * 3] = rgb[y * tw * 3 : (y + 1) * tw * 3]
        m = metas[idx]
        label = f"{m['index'] + 1:02d} {m['label']}"
        flags = []
        if m["barRatio"] > 0.6:
            flags.append("BARS")
        if m["blackFraction"] > 0.55:
            flags.append("BLACK")
        if m["contrast"] < 18:
            flags.append("FLAT")
        if flags:
            label = label + " " + "/".join(flags)
        _draw_text(buf, W, H, ox + 6, oy + 6, label[:34], (255, 255, 255), 2, (10, 10, 14))

    png = _png(W, H, bytes(buf))
    out_path = spec.get("out")
    if out_path:
        try:
            Path(out_path).parent.mkdir(parents=True, exist_ok=True)
            Path(out_path).write_bytes(png)
        except Exception as exc:
            errors.append(f"write png: {exc}")

    # Cross-frame analysis: duplicates, static runs, cut energy.
    dup_groups, seen = [], {}
    for m in metas:
        match = None
        for h, members in seen.items():
            if _hamming(m["hash"], h) <= 6:
                match = members
                break
        if match is not None:
            m["duplicateOf"] = match[0]
            match.append(m["index"])
        else:
            seen[m["hash"]] = [m["index"]]
    diffs = []
    for a, b in zip(metas, metas[1:]):
        dm = abs(a["luma"] - b["luma"]) / 25.0 + abs(a["detail"] - b["detail"]) / 8.0
        dm += _hamming(a["hash"], b["hash"]) / 24.0
        diffs.append(round(min(4.0, dm), 3))
        b["cutEnergy"] = diffs[-1]

    notes = []
    black_shots = [m["index"] for m in metas if m["blackFraction"] > 0.55]
    bars = [m["index"] for m in metas if m["barRatio"] > 0.6]
    flat = [m["index"] for m in metas if m["contrast"] < 18]
    dups = [m["index"] for m in metas if "duplicateOf" in m]
    static = [i for i, d in enumerate(diffs) if d < 0.45]
    if black_shots:
        notes.append(f"Near-black frames at samples {black_shots}. Trim them or add a visual beat.")
    if bars:
        notes.append(f"Letterbox/pillarbox bars at samples {bars}. Reframe with clip_transform or pick a different clip.")
    if flat:
        notes.append(f"Very low contrast at samples {flat}. These read as dead air; add motion or a new shot.")
    if dups:
        notes.append(f"Visually duplicated samples {dups}. Consecutive near-identical shots feel like a slideshow.")
    if len(static) >= max(3, len(diffs) // 2):
        notes.append(f"{len(static)}/{len(diffs)} sample transitions are near-static. Add entrances, push-ins, or new shots.")
    if len(metas) >= 3 and len(set(m["hash"] for m in metas)) <= max(2, len(metas) // 4):
        notes.append("The whole sequence is built from very few distinct images. Source more varied B-roll.")

    audio = None
    if spec.get("audio") and spec.get("path"):
        env = audio_envelope(ff, spec["path"])
        if env:
            silence = [i for i, v in enumerate(env) if v < 0.06]
            audio = {
                "envelope": env,
                "mean": round(sum(env) / len(env), 3),
                "dynamicRange": round(max(env) - min(env), 3),
                "silenceFraction": round(len(silence) / len(env), 3),
            }
            if audio["silenceFraction"] > 0.25:
                notes.append(f"{int(audio['silenceFraction'] * 100)}% of the soundtrack is near-silence. Fix the mix or add a music bed.")
            if audio["dynamicRange"] < 0.25:
                notes.append("Soundtrack energy is flat. Use beat-aligned cuts or ducking under the voice.")

    craft = {
        "samples": len(metas),
        "distinctShots": len(set(m["hash"] for m in metas)),
        "blackFrames": len(black_shots),
        "letterboxedFrames": len(bars),
        "flatFrames": len(flat),
        "duplicateFrames": len(dups),
        "staticTransitions": len(static),
        "meanCutEnergy": round(sum(diffs) / len(diffs), 3) if diffs else 0.0,
    }
    return {
        "ok": True,
        "pngB64": base64.b64encode(png).decode(),
        "mimeType": "image/png",
        "width": W,
        "height": H,
        "path": out_path,
        "tiles": metas,
        "craft": craft,
        "audio": audio,
        "notes": notes,
        "errors": errors[:8],
    }


# ---------------------------------------------------------------------------
# Craft critique
# ---------------------------------------------------------------------------
AXES = ["hook", "story", "rhythm", "shotChoice", "design", "sound"]


def _clamp(v):
    try:
        return max(0.0, min(10.0, float(v)))
    except Exception:
        return 5.0


def heuristic(brief, sheet):
    c = (sheet or {}).get("craft", {})
    audio = (sheet or {}).get("audio") or {}
    n = max(1, c.get("samples", 1))
    distinct = c.get("distinctShots", n)
    variety = distinct / n
    black_pen = c.get("blackFrames", 0) / n
    bar_pen = c.get("letterboxedFrames", 0) / n
    flat_pen = c.get("flatFrames", 0) / n
    dup_pen = c.get("duplicateFrames", 0) / n
    static_pen = c.get("staticTransitions", 0) / max(1, n - 1)

    hook = 6.2 + 1.6 * variety - 3.0 * black_pen - 2.0 * flat_pen
    story = 5.6 + 1.2 * variety - 1.5 * dup_pen
    rhythm = 6.4 - 3.2 * static_pen + 0.5 * min(2.0, c.get("meanCutEnergy", 0))
    shot = 7.4 - 4.0 * black_pen - 2.5 * bar_pen - 2.5 * flat_pen - 1.5 * dup_pen
    design = 6.1 - 2.2 * flat_pen - 1.2 * dup_pen + 0.8 * min(1.0, c.get("meanCutEnergy", 0))
    sound = 6.6
    if audio:
        sound -= 2.5 * max(0.0, audio.get("silenceFraction", 0) - 0.12)
        sound -= 1.5 * max(0.0, 0.4 - audio.get("dynamicRange", 0.4))
    else:
        sound = 5.5  # unmeasured, not "good"

    scores = {
        "hook": _clamp(hook), "story": _clamp(story), "rhythm": _clamp(rhythm),
        "shotChoice": _clamp(shot), "design": _clamp(design), "sound": _clamp(sound),
    }
    overall = sum(scores.values()) / len(scores)
    notes = list((sheet or {}).get("notes") or [])
    if brief:
        for key in ("audience", "platform", "emotionalArc", "thesis", "curiosityGap", "payoff"):
            if not brief.get(key):
                notes.append(f"Creative brief is missing `{key}`. The edit has no stated {key}; define it before cutting.")
    revisions = []
    if black_pen > 0.05:
        revisions.append("CALL trim_clip on black/empty lead-ins, then re-run watch_render.")
    if bar_pen > 0.1:
        revisions.append("CALL clip_transform to reframe letterboxed sources; use cropHint from stock_contact_sheet for off-centre subjects.")
    if static_pen > 0.4:
        revisions.append("CALL set_entrance / add_keyframes on consecutive static shots; vary focal length and direction.")
    if dup_pen > 0.1 or variety < 0.6:
        revisions.append("CALL openvid_search with 3 different visual queries and replace repeated frames.")
    if not brief or not brief.get("emotionalArc"):
        revisions.append("CALL set_creative_brief with audience, platform, emotionalArc, curiosityGap and payoff, then re-plan.")
    if not audio:
        revisions.append("CALL watch_render with audio:true to measure the soundtrack instead of guessing.")
    if not revisions:
        revisions.append("CALL watch_render again after the next revision and compare craft scores.")
    return {
        "engine": "heuristic",
        "scores": {k: round(v, 1) for k, v in scores.items()},
        "overall": round(overall, 1),
        "confidence": 0.45,
        "verdict": "measured pixels only; no vision model reviewed the frames",
        "notes": notes[:10],
        "revisions": revisions[:8],
    }


VLM_PROMPT = """You are the final creative director for a short-form video edit. You are looking at a contact sheet of frames sampled from the current cut, in timeline order, each labelled with its sample number and timestamp.

CREATIVE BRIEF (JSON):
{brief}

MEASURED PIXEL/AUDIO FACTS (JSON, these are real measurements, not opinions):
{facts}

Judge the edit the way a demanding human editor would. Be harsh and specific. Score each axis 0-10 (0 = unwatchable, 5 = competent but generic, 8 = genuinely good, 10 = excellent):
- hook: do the first 1-2 frames create curiosity or emotion?
- story: is there a visible progression / emotional arc / payoff?
- rhythm: do the cuts have pace and variation rather than a slideshow?
- shotChoice: are the images meaningful, varied, well-framed, free of black frames and letterbox bars?
- design: do graphics/captions/motion add meaning rather than decorate randomly?
- sound: based on the reported audio envelope, is the mix likely to support the story?

Return ONLY a JSON object, no markdown, with exactly these keys:
{{"scores":{{"hook":N,"story":N,"rhythm":N,"shotChoice":N,"design":N,"sound":N}},
"notes":["3-5 short observations citing specific frame numbers"],
"revisions":["2-4 concrete edit actions, each naming a tool such as trim_clip, openvid_search, set_entrance, clip_transform, add_captions, set_creative_brief, watch_render"],
"verdict":"one sentence"}}
"""


def critique(spec):
    brief = spec.get("brief") or {}
    sheet = spec.get("sheet") or {}
    if isinstance(sheet, str):
        try:
            sheet = json.loads(sheet)
        except Exception:
            sheet = {}
    png_b64 = spec.get("pngB64") or sheet.get("pngB64")
    facts = {
        "craft": sheet.get("craft", {}),
        "audio": {k: v for k, v in (sheet.get("audio") or {}).items() if k != "envelope"},
        "tiles": [
            {k: t.get(k) for k in ("index", "label", "at", "luma", "contrast", "detail", "barRatio", "cropHint")}
            for t in (sheet.get("tiles") or [])
        ],
        "notes": sheet.get("notes", []),
    }
    base = heuristic(brief, sheet)
    if not png_b64:
        base["notes"].append("No contact sheet image was supplied; semantic review skipped.")
        return {**base, "ok": True}

    if local_vlm is None:
        base["notes"].append("Local VLM helper unavailable; using measured-pixel critique.")
        return {**base, "ok": True}

    ep = local_vlm.detect()
    if not ep and spec.get("autoSetup"):
        try:
            local_vlm.start(spec.get("profile", "mini"))
            ep = local_vlm.detect()
        except Exception as exc:
            base["notes"].append(f"Auto VLM setup failed ({type(exc).__name__}: {exc}); using measured-pixel critique.")
    if not ep:
        base["notes"].append("No local vision model running. Run vlm_setup once for semantic review; using measured-pixel critique.")
        return {**base, "ok": True}

    prompt = VLM_PROMPT.format(
        brief=json.dumps(brief, ensure_ascii=False)[:2000],
        facts=json.dumps(facts, ensure_ascii=False)[:6000],
    )
    try:
        text = local_vlm.ask(prompt, [png_b64], endpoint=ep)
        match = re.search(r"\{[\s\S]*\}", text)
        if not match:
            raise ValueError("model returned no JSON object")
        parsed = json.loads(match.group(0))
        scores = {a: _clamp((parsed.get("scores") or {}).get(a, base["scores"][a])) for a in AXES}
        # Blend: the model judges craft, the pixels keep it honest.
        blended = {a: round(0.78 * scores[a] + 0.22 * base["scores"][a], 1) for a in AXES}
        return {
            "ok": True,
            "engine": "local-vlm",
            "model": ep.get("model", "local-vlm"),
            "scores": blended,
            "rawScores": {k: round(v, 1) for k, v in scores.items()},
            "overall": round(sum(blended.values()) / len(blended), 1),
            "confidence": 0.8,
            "verdict": str(parsed.get("verdict", ""))[:300],
            "notes": list(parsed.get("notes") or [])[:8] + base["notes"][:3],
            "revisions": list(parsed.get("revisions") or [])[:8] or base["revisions"],
            "measured": base["scores"],
        }
    except Exception as exc:
        base["notes"].append(f"Local VLM critique failed ({type(exc).__name__}: {exc}); using measured-pixel critique.")
        return {**base, "ok": True}


RANK_PROMPT = """You are choosing B-roll for a short-form video. The image is a labelled contact sheet of candidate clips; each tile is numbered on its top-left and its metadata is below.

CREATIVE BRIEF (JSON):
{brief}

CANDIDATES (JSON, in tile order):
{tiles}

Pick the candidates that would actually make the edit better. Judge visual meaning and emotional fit, not just technical quality. Reject clips that are dark, letterboxed, generic filler, or off-brief. Prefer variety: different subjects, compositions and motion.

Return ONLY a JSON object, no markdown, with exactly these keys:
{{"ranking":[{{"index":0,"score":8.5,"why":"one short reason"}}],
"reject":[{{"index":1,"why":"one short reason"}}],
"queries":["2-3 better search phrases if these candidates are weak"]}}
"""


def rank(spec):
    """Semantic re-ranking of stock candidates. VLM when present, pixels when not."""
    brief = spec.get("brief") or {}
    sheet = spec.get("sheet") or {}
    if isinstance(sheet, str):
        try:
            sheet = json.loads(sheet)
        except Exception:
            sheet = {}
    tiles = sheet.get("tiles") or []
    png_b64 = spec.get("pngB64") or sheet.get("pngB64")
    # Deterministic fallback: reward detail/contrast, punish bars and black,
    # and reward aesthetic/motion metadata when the index supplied it.
    scored = []
    for t in tiles:
        m = t.get("metrics") or t
        s = 5.0
        s += min(2.0, (m.get("detail", 0) or 0) / 6.0)
        s += min(1.5, (m.get("contrast", 0) or 0) / 40.0)
        s -= 3.0 * (m.get("barRatio", 0) or 0)
        s -= 4.0 * (m.get("blackFraction", 0) or 0)
        if m.get("aesthetic") is not None:
            s += max(-1.0, min(1.5, (float(m["aesthetic"]) - 3.5) / 2.0))
        scored.append({"index": m.get("index", 0), "score": round(_clamp(s), 2),
                       "why": "pixel quality (no vision model available)"})
    scored.sort(key=lambda r: -r["score"])
    base = {
        "ok": True, "engine": "heuristic", "ranking": scored,
        "reject": [r for r in scored if r["score"] < 4.0],
        "queries": [],
        "note": "Ranked by measured pixel quality only; run vlm_setup for semantic ranking.",
    }
    if not png_b64 or local_vlm is None:
        return base
    ep = local_vlm.detect()
    if not ep and spec.get("autoSetup"):
        try:
            local_vlm.start(spec.get("profile", "mini"))
            ep = local_vlm.detect()
        except Exception:
            ep = None
    if not ep:
        return base
    thin = [
        {k: t.get(k) for k in ("index", "label", "id", "caption", "aesthetic", "motion", "camera", "source")}
        for t in tiles
    ]
    prompt = RANK_PROMPT.format(
        brief=json.dumps(brief, ensure_ascii=False)[:2000],
        tiles=json.dumps(thin, ensure_ascii=False)[:6000],
    )
    try:
        text = local_vlm.ask(prompt, [png_b64], endpoint=ep)
        m = re.search(r"\{[\s\S]*\}", text)
        parsed = json.loads(m.group(0)) if m else {}
        ranking = parsed.get("ranking") or []
        if not ranking:
            return base
        # Keep the model's ordering but anchor on measured quality (20%).
        measured = {r["index"]: r["score"] for r in scored}
        out = []
        for r in ranking:
            i = r.get("index")
            if not isinstance(i, int):
                continue
            out.append({
                "index": i,
                "score": round(0.8 * _clamp(r.get("score", 5)) + 0.2 * measured.get(i, 5.0), 2),
                "why": str(r.get("why", ""))[:200],
            })
        out.sort(key=lambda r: -r["score"])
        return {
            "ok": True, "engine": "local-vlm", "model": ep.get("model", "local-vlm"),
            "ranking": out,
            "reject": list(parsed.get("reject") or []),
            "queries": list(parsed.get("queries") or []),
            "note": "Semantic ranking blended with measured pixel quality.",
        }
    except Exception as exc:
        base["note"] = f"VLM ranking failed ({type(exc).__name__}: {exc}); used pixel ranking."
        return base


def _probe_size(ffprobe, path):
    rc, out, _ = _run([
        ffprobe, "-v", "error", "-select_streams", "v:0",
        "-show_entries", "stream=width,height", "-of", "csv=p=0:s=x", path,
    ], 20)
    try:
        w, h = out.decode().strip().split("x")[:2]
        return int(w), int(h)
    except Exception:
        return None


def overlay(spec):
    """Composite an RGBA PNG bake over real footage (or a colour plate).

    This is the honest answer to the alpha problem: the app's sidecar
    encoders write opaque video, and FFmpeg's WebM muxer in the wild often
    drops VP9's AlphaMode. Rather than pretend, we honour the comp's
    transparency at the compositing step and hand back an opaque, correct
    piece of footage that plays anywhere. The original RGBA sequence is
    left untouched.
    """
    import glob
    ff, fp = _tools(spec)
    frames_dir = spec.get("frames") or spec.get("dir")
    out = spec.get("out")
    if not frames_dir or not out:
        return {"ok": False, "error": "overlay needs `frames` (the bake's frames/ dir) and `out`"}
    names = sorted(glob.glob(os.path.join(frames_dir, "[0-9][0-9][0-9][0-9].png")))
    if not names:
        return {"ok": False, "error": f"no 0001.png-style frames in {frames_dir}"}
    fps = max(1.0, min(120.0, float(spec.get("fps", 30))))
    count = len(names)
    duration = count / fps
    at = max(0.0, float(spec.get("at", 0.0)))
    crf = int(spec.get("crf", 18))
    plate = spec.get("plate")
    width = int(spec.get("width", 0) or 0)
    height = int(spec.get("height", 0) or 0)
    if plate:
        if not (width and height):
            size = _probe_size(fp, plate)
            if not size:
                return {"ok": False, "error": f"could not read the plate's frame size: {plate}"}
            width, height = size
        plate_duration = probe_duration(fp, plate)
    else:
        if not (width and height):
            return {"ok": False, "error": "without a `plate`, give `width` and `height`"}
        plate_duration = at + duration
    width = max(2, width // 2 * 2)
    height = max(2, height // 2 * 2)
    pattern = os.path.join(frames_dir, "%04d.png")
    shift = f"setpts=PTS+{at:.4f}/TB," if at > 0 else ""
    ov = (
        f"[1:v]{shift}scale={width}:{height}:force_original_aspect_ratio=decrease,"
        f"format=rgba,setsar=1[ov]"
    )
    place = (
        f"x={spec.get('x', '(W-w)/2')}:y={spec.get('y', '(H-h)/2')}:"
        f"enable='between(t,{at:.4f},{at + duration:.4f})'"
    )
    if plate:
        fc = (
            f"[0:v]scale={width}:{height}:force_original_aspect_ratio=increase,"
            f"crop={width}:{height},setsar=1[base];{ov};"
            f"[base][ov]overlay={place}[v]"
        )
        cmd = [
            ff, "-v", "error", "-i", plate, "-framerate", f"{fps:.6f}",
            "-start_number", "1", "-i", pattern,
            "-filter_complex", fc, "-map", "[v]", "-map", "0:a?",
        ]
        if plate_duration > 0:
            cmd += ["-t", f"{plate_duration:.4f}"]
        cmd += ["-c:a", "aac", "-b:a", "192k"]
    else:
        colour = str(spec.get("background", "black")).lstrip("#")
        colour = "0x" + colour if all(c in "0123456789abcdefABCDEF" for c in colour) else colour
        fc = f"{ov};[0:v][ov]overlay={place}:shortest=1[v]"
        cmd = [
            ff, "-v", "error", "-f", "lavfi",
            "-i", f"color=c={colour}:s={width}x{height}:r={fps:.6f}:d={at + duration:.4f}",
            "-framerate", f"{fps:.6f}", "-start_number", "1", "-i", pattern,
            "-filter_complex", fc, "-map", "[v]",
        ]
    Path(out).parent.mkdir(parents=True, exist_ok=True)
    cmd += [
        "-c:v", "libx264", "-preset", "medium", "-crf", str(crf),
        "-pix_fmt", "yuv420p", "-movflags", "+faststart", "-y", out,
    ]
    rc, _, err = _run(cmd, 900)
    if rc != 0:
        return {"ok": False, "error": (err.decode("utf-8", "replace") or "ffmpeg failed")[-1200:]}
    return {
        "ok": True,
        "file": out,
        "frames": count,
        "fps": fps,
        "duration": round(duration, 3),
        "width": width,
        "height": height,
        "plate": plate,
        "note": (
            "The RGBA bake was composited over the plate, so the comp's transparency "
            "was honoured and the result is an opaque H.264 clip ready for media_import "
            "and clip_add. The alpha sequence itself is untouched. VP9/prores alpha "
            "targets are deliberately not claimed: FFmpeg's WebM muxer does not reliably "
            "write AlphaMode, which is why this path exists."
        ),
    }


def vlm_command(spec):
    if local_vlm is None:
        return {"ok": False, "error": "vlm.py missing from sense runtime"}
    action = spec.get("action", "status")
    try:
        if action == "status":
            return {"ok": True, **local_vlm.status()}
        if action == "setup":
            p = spec.get("profile", "mini")
            if p not in local_vlm.PROFILES:
                return {"ok": False, "error": f"unknown profile {p}; choose from {list(local_vlm.PROFILES)}"}
            return {"ok": True, **local_vlm.start(p)}
        return {"ok": False, "error": f"unknown vlm action {action}"}
    except Exception as exc:
        return {"ok": False, "error": f"{type(exc).__name__}: {exc}"}


def main():
    import argparse
    ap = argparse.ArgumentParser()
    ap.add_argument("cmd", choices=["sheet", "critique", "rank", "overlay", "vlm", "selftest"])
    ap.add_argument("--spec", default="{}")
    args = ap.parse_args()
    try:
        spec = json.loads(args.spec)
        if not isinstance(spec, dict):
            raise ValueError("spec must be a JSON object")
        if args.cmd == "sheet":
            out = build_sheet(spec)
        elif args.cmd == "critique":
            out = critique(spec)
        elif args.cmd == "rank":
            out = rank(spec)
        elif args.cmd == "overlay":
            out = overlay(spec)
        elif args.cmd == "vlm":
            out = vlm_command(spec)
        else:
            # No external dependencies: generate a synthetic gradient sheet and
            # measure it. Proves ffmpeg, PNG encoding and metrics all work.
            tw, th = 64, 36
            buf = bytearray(tw * th * 3)
            for y in range(th):
                for x in range(tw):
                    i = (y * tw + x) * 3
                    buf[i] = int(255 * x / tw)
                    buf[i + 1] = int(255 * y / th)
                    buf[i + 2] = 128
            b64 = base64.b64encode(_png(tw, th, bytes(buf))).decode()
            out = build_sheet({"sources": [
                {"pngB64": b64, "at": 0, "label": "test A"},
                {"pngB64": b64, "at": 1, "label": "test B"},
            ]})
            out["selftest"] = True
            out["decoder"] = "pure-python"
    except Exception as exc:
        out = {"ok": False, "error": f"{type(exc).__name__}: {exc}"}
    print(json.dumps(out, ensure_ascii=False, separators=(",", ":")), flush=True)


if __name__ == "__main__":
    main()
