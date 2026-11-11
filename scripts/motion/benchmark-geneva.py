#!/usr/bin/env python3
"""Measure a real 1080p30 Geneva render on the machine running this script.

The benchmark builds the existing 10-second, 1920x1080, 30 fps multi-layer
example, renders it through the installed Geneva sidecar, and records wall
time, output size, machine details and sampled process-tree RSS. It does not
pretend hosted-CI numbers are measurements of a user's laptop.

Usage:
  python3 scripts/motion/benchmark-geneva.py
  python3 scripts/motion/benchmark-geneva.py --output-dir /tmp/funcut-geneva
  python3 scripts/motion/benchmark-geneva.py --skip-build
"""

from __future__ import annotations

import argparse
import json
import os
import platform
import shutil
import subprocess
import sys
import time
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parents[2]
EXAMPLE = ROOT / "src" / "target" / "release" / "examples" / "layers"
ASSETS_SCRIPT = ROOT / "scripts" / "motion" / "layers-assets.py"


def geneva_binary() -> Path:
    candidates = [
        os.environ.get("CONCAT_GENEVA_BIN"),
        shutil.which("geneva"),
        str(Path.home() / ".local" / "bin" / "geneva"),
        "/opt/concat/geneva",
    ]
    for candidate in candidates:
        if candidate:
            path = Path(candidate).expanduser()
            if path.is_file() and os.access(path, os.X_OK):
                return path.resolve()
    raise SystemExit(
        "Geneva was not found. Install the pinned sidecar or set CONCAT_GENEVA_BIN."
    )


def process_tree_rss_kib(root_pid: int) -> int | None:
    """Return summed RSS for a process and descendants, or None off Linux."""
    proc = Path("/proc")
    if not proc.is_dir():
        return None
    parents: dict[int, int] = {}
    rss: dict[int, int] = {}
    page_kib = os.sysconf("SC_PAGE_SIZE") // 1024
    try:
        entries = list(proc.iterdir())
    except OSError:
        return None
    for entry in entries:
        if not entry.name.isdigit():
            continue
        try:
            stat = (entry / "stat").read_text()
            fields = stat[stat.rfind(")") + 2 :].split()
            pid = int(entry.name)
            parents[pid] = int(fields[1])
            rss[pid] = max(0, int(fields[21])) * page_kib
        except (OSError, ValueError, IndexError):
            continue
    children: dict[int, list[int]] = {}
    for pid, parent in parents.items():
        children.setdefault(parent, []).append(pid)
    pending = [root_pid]
    descendants: set[int] = set()
    while pending:
        pid = pending.pop()
        if pid in descendants:
            continue
        descendants.add(pid)
        pending.extend(children.get(pid, []))
    if root_pid not in rss:
        return None
    return sum(rss.get(pid, 0) for pid in descendants)


def parse_json_documents(stream: str) -> list[dict[str, Any]]:
    decoder = json.JSONDecoder()
    documents: list[dict[str, Any]] = []
    offset = 0
    while offset < len(stream):
        remaining = stream[offset:].lstrip()
        if not remaining:
            break
        offset = len(stream) - len(remaining)
        value, end = decoder.raw_decode(stream, offset)
        if isinstance(value, dict):
            documents.append(value)
        offset = end
    return documents


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--output-dir",
        type=Path,
        default=Path("geneva-benchmark"),
        help="where generated assets, logs, report and MP4 are written",
    )
    parser.add_argument(
        "--skip-build",
        action="store_true",
        help="use an already-built src/target/release/examples/layers binary",
    )
    args = parser.parse_args()

    geneva = geneva_binary()
    version = subprocess.run(
        [str(geneva), "--version"],
        check=True,
        capture_output=True,
        text=True,
        timeout=15,
    ).stdout.strip()

    output_dir = args.output_dir.expanduser().resolve()
    output_dir.mkdir(parents=True, exist_ok=True)
    if not args.skip_build:
        subprocess.run(
            [
                "cargo",
                "build",
                "--release",
                "-p",
                "concat-motion",
                "--example",
                "layers",
            ],
            cwd=ROOT / "src",
            check=True,
        )
    if not EXAMPLE.is_file():
        raise SystemExit(f"benchmark executable not found: {EXAMPLE}")

    subprocess.run(
        [sys.executable, str(ASSETS_SCRIPT), str(output_dir)],
        cwd=ROOT,
        check=True,
    )
    video = output_dir / "geneva-1080p30.mp4"
    cache = output_dir / "cache"
    stdout_path = output_dir / "render.stdout.json"
    stderr_path = output_dir / "render.stderr.log"
    # Always measure a cold render: stale output or a previous cache hit
    # would make a repeat run look faster than the work it is meant to test.
    video.unlink(missing_ok=True)
    shutil.rmtree(cache, ignore_errors=True)
    command = [
        str(EXAMPLE),
        "-o",
        str(video),
        "--assets",
        str(output_dir),
        "--cache",
        str(cache),
    ]

    peak_rss_kib = 0
    started = time.perf_counter()
    with stdout_path.open("w", encoding="utf-8") as stdout, stderr_path.open(
        "w", encoding="utf-8"
    ) as stderr:
        process = subprocess.Popen(command, cwd=ROOT, stdout=stdout, stderr=stderr)
        while process.poll() is None:
            measured = process_tree_rss_kib(process.pid)
            if measured is not None:
                peak_rss_kib = max(peak_rss_kib, measured)
            time.sleep(0.05)
        return_code = process.wait()
    elapsed = time.perf_counter() - started

    if return_code != 0:
        tail = stderr_path.read_text(encoding="utf-8", errors="replace")[-4000:]
        raise SystemExit(f"Geneva render failed (exit {return_code}):\n{tail}")

    documents = parse_json_documents(
        stdout_path.read_text(encoding="utf-8", errors="replace")
    )
    if len(documents) < 2:
        raise SystemExit(
            f"expected plan summary and render report; found {len(documents)} JSON document(s)"
        )
    plan_summary, render_report = documents[0], documents[-1]
    if not plan_summary.get("ok") or not render_report.get("ok"):
        raise SystemExit(f"render did not report success: {render_report}")
    if not video.is_file() or video.stat().st_size == 0:
        raise SystemExit("Geneva reported success but did not produce a non-empty MP4")

    memory_kib = peak_rss_kib or None
    report = {
        "schema": "funcut.geneva-benchmark/1",
        "workload": {
            "example": "concat-motion/examples/layers.rs",
            "width": 1920,
            "height": 1080,
            "fps": 30,
            "planned_scenes": plan_summary.get("scenes"),
            "planned_layers": plan_summary.get("layers"),
            "duration_seconds": plan_summary.get("duration"),
        },
        "machine": {
            "platform": platform.platform(),
            "architecture": platform.machine(),
            "processor": platform.processor(),
            "logical_cpus": os.cpu_count(),
        },
        "geneva": {"binary": str(geneva), "version": version},
        "measurement": {
            "render_wall_seconds": round(elapsed, 3),
            "cache_policy": "cold; previous cache removed before render",
            "peak_process_tree_rss_mib": (
                round(memory_kib / 1024, 1) if memory_kib is not None else None
            ),
            "rss_method": (
                "sampled summed VmRSS for the renderer and descendants every 50 ms; "
                "shared pages may be counted more than once"
            ),
            "output_bytes": video.stat().st_size,
        },
        "artifacts": {
            "video": str(video),
            "stdout": str(stdout_path),
            "stderr": str(stderr_path),
        },
        "render_report": render_report,
    }
    report_path = output_dir / "benchmark.json"
    report_path.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(report, indent=2))
    print(f"\nSaved benchmark report: {report_path}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
