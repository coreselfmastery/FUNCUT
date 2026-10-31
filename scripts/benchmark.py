#!/usr/bin/env python3
"""The small-machine benchmark: scrub, export and peak memory, measured.

What this measures, and how, so the numbers it prints can be read for
what they are:

- **Scrub FPS** - the JSON-RPC API's `preview.frame`, the same composite
  the paused monitor shows (decode, effects, blend, scale), asked for
  `--scrubs` random instants across the clip through one `concat-cli
  api` process. Random instants, not sequential ones, because scrubbing
  is random access; the first answers are the disk's, the rest the
  caches' - which is the machine's real scrubbing.
- **Export** - `concat-cli render`, the end-to-end slice the CLI itself
  is: decode, composite, encode, one process, `--render-frames` frames
  of it. Timed wall-clock, reported as seconds and as frames a second.
- **Peak RSS** - the high-water mark of each process's own memory
  (`VmHWM` from /proc), read while the process lives. What the caches
  sized to this machine actually took.

Usage:
    python3 scripts/benchmark.py [--video FILE] [--cli PATH]
                                 [--scrubs N] [--render-frames N]

Without `--video` a 60 s 1080p30 `testsrc2` clip is generated with
ffmpeg when there is ffmpeg - a synthetic source, and the page this
feeds says so. Without `--cli` a `concat-cli` on the PATH is used, else
a source checkout's `cargo run --release -p concat-cli --`.

Prints a markdown block ready for docs/PERFORMANCE.md. Nothing here
knows what machine it is on beyond what /proc says; the page's results
table is filled by running this on the machine being claimed about.
"""
import argparse
import json
import pathlib
import random
import shutil
import subprocess
import sys
import tempfile
import time

WIDTH = 1920
HEIGHT = 1080
RATE = 30


def machine_facts():
    mem = {}
    for line in pathlib.Path("/proc/meminfo").read_text().splitlines():
        parts = line.split(":", 1)
        if len(parts) == 2:
            mem[parts[0]] = parts[1].strip()
    cpu = ""
    for line in pathlib.Path("/proc/cpuinfo").read_text().splitlines():
        if line.startswith("model name"):
            cpu = line.split(":", 1)[1].strip()
            break
    total = mem.get("MemTotal", "?")
    kernel = subprocess.run(["uname", "-r"], capture_output=True, text=True).stdout.strip()
    return cpu, total, kernel


def find_cli(wanted):
    if wanted:
        return [wanted]
    if shutil.which("concat-cli"):
        return ["concat-cli"]
    return ["cargo", "run", "--release", "-p", "concat-cli", "--"]


def make_clip(where, ffmpeg):
    clip = where / "benchmark-source.mp4"
    subprocess.run(
        [
            ffmpeg,
            "-y",
            "-f",
            "lavfi",
            "-i",
            f"testsrc2=size={WIDTH}x{HEIGHT}:rate={RATE}:duration=60",
            "-c:v",
            "libx264",
            "-preset",
            "veryfast",
            "-pix_fmt",
            "yuv420p",
            str(clip),
        ],
        check=True,
        capture_output=True,
    )
    return clip


def vmhwm(pid):
    try:
        for line in pathlib.Path(f"/proc/{pid}/status").read_text().splitlines():
            if line.startswith("VmHWM:"):
                return int(line.split()[1]) * 1024
    except (OSError, ValueError):
        pass
    return None


def human(bytes_):
    if bytes_ is None:
        return "?"
    for unit in ("B", "KiB", "MiB", "GiB"):
        if bytes_ < 1024 or unit == "GiB":
            return f"{bytes_:.0f} {unit}" if unit == "B" else f"{bytes_ / 1024:.1f} {unit}"
        bytes_ /= 1024
    return f"{bytes_} B"


class Api:
    """One `concat-cli api` process: a request a line, a reply a line."""

    def __init__(self, cli):
        self.proc = subprocess.Popen(
            cli + ["api"],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL,
            text=True,
            bufsize=1,
        )
        self.next = 0

    def call(self, method, params):
        self.next += 1
        request = {"jsonrpc": "2.0", "id": self.next, "method": method}
        if params is not None:
            request["params"] = params
        self.proc.stdin.write(json.dumps(request) + "\n")
        self.proc.stdin.flush()
        for line in self.proc.stdout:
            reply = json.loads(line)
            if reply.get("id") == self.next:
                if "error" in reply:
                    raise RuntimeError(f"{method}: {reply['error']}")
                return reply.get("result")
        raise RuntimeError(f"{method}: the server closed its end")

    def close(self):
        try:
            self.proc.stdin.close()
            self.proc.wait(timeout=30)
        except OSError:
            pass
        return self.proc.pid


def find_project_path(reply, under):
    """The created project's folder, wherever the reply carries it."""
    stack = [reply]
    while stack:
        value = stack.pop()
        if isinstance(value, str) and value.startswith(str(under)):
            return value
        if isinstance(value, dict):
            stack.extend(value.values())
        if isinstance(value, list):
            stack.extend(value)
    raise RuntimeError("project.create's reply named no folder")


def scrub(cli, video, where, scrubs):
    api = Api(cli)
    try:
        api.call("version", None)
        api.call("project.create", {"location": str(where), "name": "benchmark"})
        project = find_project_path(api.call("project.list", None), str(where))
        api.call("media.import", {"path": project, "file": str(video)})
        document = api.call("project.document", {"path": project})
        media = document["media"][-1]
        track = document["tracks"][-1]["id"]
        api.call(
            "edit.apply",
            {
                "path": project,
                "command": {
                    "op": "addClip",
                    "mediaId": media["id"],
                    "trackId": track,
                    "start": 0,
                    "ripple": False,
                },
            },
        )
        duration = media.get("duration") or 60.0
        instants = [random.uniform(0, max(duration - 1, 1)) for _ in range(scrubs)]
        frame = where / "scrub-frame.png"
        began = time.perf_counter()
        for instant in instants:
            api.call(
                "preview.frame",
                {"path": project, "time": round(instant, 3), "output": str(frame)},
            )
        took = time.perf_counter() - began
        peak = vmhwm(api.proc.pid)
        api.call("project.close", {"path": project, "save": False})
        return scrubs / took, took, peak
    finally:
        api.close()


def render(cli, video, where, frames):
    out = where / "benchmark-render.mp4"
    began = time.perf_counter()
    proc = subprocess.Popen(
        cli + ["render", str(video), str(out), "--frames", str(frames), "--fade", "0"],
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )
    peak = 0
    while proc.poll() is None:
        peak = max(peak, vmhwm(proc.pid) or 0)
        time.sleep(0.05)
    if proc.returncode != 0:
        raise RuntimeError("render did not finish")
    return time.perf_counter() - began, peak


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--video", help="a source clip; generated when absent")
    parser.add_argument("--cli", help="the concat-cli to run; PATH or cargo when absent")
    parser.add_argument("--scrubs", type=int, default=120, help="preview.frame asks (default 120)")
    parser.add_argument("--render-frames", type=int, default=300, help="render frames (default 300)")
    args = parser.parse_args()

    cli = find_cli(args.cli)
    ffmpeg = shutil.which("ffmpeg")
    with tempfile.TemporaryDirectory(prefix="concat-benchmark-") as work:
        where = pathlib.Path(work)
        video = pathlib.Path(args.video) if args.video else None
        generated = video is None
        if generated:
            if not ffmpeg:
                sys.exit("benchmark: no --video and no ffmpeg to make one with")
            video = make_clip(where, ffmpeg)
        random.seed(20261031)

        fps, seconds, scrub_peak = scrub(cli, video, where, args.scrubs)
        render_seconds, render_peak = render(cli, video, where, args.render_frames)

        cpu, total, kernel = machine_facts()
        source = "a generated 60 s 1080p30 testsrc2 clip" if generated else str(video)
        print(
            f"""| Measurement | Result |
| --- | --- |
| Machine | {cpu}, {total} RAM, kernel {kernel} |
| Source | {source} |
| Scrub: {args.scrubs} random instants, `preview.frame` | **{fps:.1f} frames/s** ({seconds:.1f} s in all) |
| Export: `concat-cli render`, {args.render_frames} frames | **{render_seconds:.1f} s** ({args.render_frames / render_seconds:.1f} frames/s) |
| Peak RSS while scrubbing | **{human(scrub_peak)}** |
| Peak RSS while rendering | **{human(render_peak)}** |
"""
        )


if __name__ == "__main__":
    main()
