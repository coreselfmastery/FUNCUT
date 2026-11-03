#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-or-later
# SPDX-FileCopyrightText: 2026 Jareer Abdullah and Concat contributors
"""farm-sync.py - the cloud farm's legs: a farm directory, staged to S3.

The resumable farm (`concat_motion::farm`) renders a long reel in parts
that survive their own failure, and its render directory is self-contained
by design - the unit of work a worker on another machine is handed whole.
What it does not have is the *transport*: the folder still had to reach
the worker by NFS, SSHFS or a synced copy. This script is the transport,
on object storage, with nothing but the standard library:

    farm-sync.py push   FARM [--staged DIR]  upload the farm (and the staged
                                             render directory) under the prefix
    farm-sync.py pull   FARM [--staged DIR]  download them back
    farm-sync.py status FARM                  what is remote, what is local
    farm-sync.py work   FARM --staged DIR     a worker's whole day: pull,
                                             render one missing part through
                                             the sidecar, mark it done, push
                                             it back
    farm-sync.py selftest                      the client against a mock S3,
                                             in-process, no network

Everything the storage needs comes from the environment the OPERATOR set -
the same posture the VLM endpoint allow-list takes - and never from a
tool's arguments, so a prompted agent cannot point a render (or the
credentials beside it) at a host the operator did not choose:

    FUNCUT_FARM_BUCKET          the bucket (required)
    FUNCUT_FARM_PREFIX          the key prefix (default: farms/<farm name>/)
    FUNCUT_FARM_ENDPOINT        an S3-compatible endpoint - MinIO, R2, a
                                self-hosted one; default: AWS's own
    FUNCUT_FARM_REGION          the signing region (default: us-east-1)
    AWS_ACCESS_KEY_ID           the access key
    AWS_SECRET_ACCESS_KEY       the secret key

The wire is SigV4 over HTTPS: every request is signed with the standard
AWS HMAC-SHA256 chain, the payload hash travelled as
x-amz-content-sha256, and an upload streams its bytes a block at a time
(the file is hashed once, uploaded once, and never held whole in memory -
the same megabyte-at-a-time rule the cache's own hashes follow). A
push HEADs each file first and skips the ones whose stored sha256
metadata already matches, so a re-push after a dead worker costs what
changed, not what existed.

Commands take `--spec -` (JSON on standard input) and print one JSON
object, the same contract sense.py set:

    {"action": "push", "farm": "farm/", "staged": "cache/render"}

`work` finds its part ranges the way the farm itself does: the plan
event in progress.jsonl says how many parts to ask for, the sidecar's
own `plan` verb names the boundaries, and a part is missing when its
file or its `.done` marker is - or when the marker names another key,
which is the farm's own rule for "the plan changed". The key it writes
is the same FNV-1a of the staged timeline the Rust side computes,
byte for byte, or the part would be re-rendered by the joiner for
nothing.
"""

from __future__ import annotations

import hashlib
import hmac
import http.server
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
import threading
import time
import urllib.error
import urllib.parse
import urllib.request
import xml.etree.ElementTree as ET
from pathlib import Path

# How big a block the hashes read and the uploads send: a megabyte, the
# cache's own number, so nothing ever holds a file whole.
BLOCK = 1024 * 1024

# FNV-1a 64: the offset basis and the prime, the numbers the cache's own
# stream_hash uses, so a key computed here is the key computed there.
FNV_OFFSET = 0xCBF29CE484222325
FNV_PRIME = 0x00000100000001B3
MASK64 = 0xFFFFFFFFFFFFFFFF


def fnv1a64(data: bytes, seed: int = FNV_OFFSET) -> int:
    """FNV-1a 64 over `data`, continuing from `seed`."""
    hash_ = seed
    for byte in data:
        hash_ = ((hash_ ^ byte) * FNV_PRIME) & MASK64
    return hash_


def stream_fnv(domain: str, path: Path) -> int:
    """The cache's own stream hash: the domain's bytes, then the file's,
    a block at a time. The same number `cache::stream_hash` computes, or
    the part keys here and there disagree and the farm re-renders for
    nothing."""
    hash_ = FNV_OFFSET
    hash_ = fnv1a64(domain.encode(), hash_)
    with open(path, "rb") as handle:
        while True:
            block = handle.read(BLOCK)
            if not block:
                return hash_
            hash_ = fnv1a64(block, hash_)


def stream_sha256(path: Path) -> str:
    """A file's sha256, a block at a time - the metadata a push compares
    so 'already there' means the same bytes, not the same size."""
    digest = hashlib.sha256()
    with open(path, "rb") as handle:
        while True:
            block = handle.read(BLOCK)
            if not block:
                return digest.hexdigest()
            digest.update(block)


def part_key(timeline: int, start: int, end: int) -> str:
    """The farm's part key, exactly as `farm::part_key` writes it:
    ``{timeline:016x}.{from:016x}.{to:016x}``."""
    return f"{timeline:016x}.{start:016x}.{end:016x}"


def amz_timestamp() -> str:
    """The x-amz-date SigV4 speaks: YYYYMMDDTHHMMSSZ, UTC."""
    return time.strftime("%Y%m%dT%H%M%SZ", time.gmtime())


def _uri_encode(value: str, keep_slash: bool = False) -> str:
    """The URI encoding SigV4's canonical request wants: everything
    percent-encoded in upper case hex, `/` kept only where the caller
    said (the path keeps its slashes, the query keeps none)."""
    safe = "/" if keep_slash else ""
    return urllib.parse.quote(value, safe=safe)


class S3:
    """A minimal S3 client: PUT, GET, HEAD and LIST, SigV4-signed, over
    the standard library's own HTTP. Nothing here retries a semantic
    failure or hides an error body - S3's XML refusals are parsed and
    raised as they are, and a transport failure is retried the once a
    flaky link deserves."""

    def __init__(self, bucket: str, prefix: str, region: str,
                 access_key: str, secret_key: str, endpoint: str | None):
        self.bucket = bucket
        # The prefix is the farm's own name in the bucket, with the
        # slashes it needs on both sides.
        self.prefix = prefix.strip("/") + "/" if prefix.strip("/") else ""
        self.region = region
        self.access_key = access_key
        self.secret_key = secret_key
        self.endpoint = (endpoint or f"https://s3.{region}.amazonaws.com").rstrip("/")
        self.host = urllib.parse.urlparse(self.endpoint).netloc

    # ── Signing ────────────────────────────────────────────────────────

    def _sign(self, method: str, key: str, query: str,
              payload_hash: str, extra_headers: dict[str, str]) -> dict[str, str]:
        """The signed headers for one request. `key` is the object key
        (already the bucket-relative path), `query` the already-encoded
        query string without its '?'."""
        now = amz_timestamp()
        date = now[:8]
        # The path the request will carry, built the one way `_url` builds
        # it, because a signature over a path the wire never sees is a
        # signature over nothing.
        path = "/" + self.bucket + ("/" + _uri_encode(key, keep_slash=True) if key else "")
        headers = {
            "host": self.host,
            "x-amz-content-sha256": payload_hash,
            "x-amz-date": now,
        }
        headers.update({name.lower(): value for name, value in extra_headers.items()})
        signed = ";".join(sorted(headers))
        canonical_headers = "".join(
            f"{name}:{headers[name].strip()}\n" for name in sorted(headers)
        )
        canonical = "\n".join([method, path, query, canonical_headers, signed, payload_hash])
        scope = f"{date}/{self.region}/s3/aws4_request"
        string_to_sign = "\n".join([
            "AWS4-HMAC-SHA256", now, scope,
            hashlib.sha256(canonical.encode()).hexdigest(),
        ])
        key_chain = ("AWS4" + self.secret_key).encode()
        for part in (date, self.region, "s3", "aws4_request"):
            key_chain = hmac.new(key_chain, part.encode(), hashlib.sha256).digest()
        signature = hmac.new(key_chain, string_to_sign.encode(), hashlib.sha256).hexdigest()
        authorization = (
            f"AWS4-HMAC-SHA256 Credential={self.access_key}/{scope}, "
            f"SignedHeaders={signed}, Signature={signature}"
        )
        out = {name: headers[name] for name in headers if name != "host"}
        out["Authorization"] = authorization
        return out

    def _url(self, key: str, query: str = "") -> str:
        path = "/" + self.bucket + ("/" + _uri_encode(key, keep_slash=True) if key else "")
        return self.endpoint + path + (("?" + query) if query else "")

    def _call(self, method: str, key: str, query: str = "",
              data=None, content_length: int | None = None,
              payload_hash: str | None = None,
              extra_headers: dict[str, str] | None = None,
              stream_to: Path | None = None, return_body: bool = False,
              attempts: int = 3):
        """One signed HTTP call. `data` may be a file object, which the
        standard library sends a block at a time when Content-Length is
        named - the whole point of the streaming upload - and a file
        body MUST name its payload hash (the sha256 the caller computed
        for the metadata), because signing a body nobody read would be
        signing a lie."""
        if data is None or isinstance(data, (bytes, bytearray)):
            payload = hashlib.sha256(bytes(data or b"")).hexdigest()
        else:
            payload = payload_hash or ""
            if not payload:
                raise ValueError("a file body must name its payload hash")
        headers = self._sign(method, key, query, payload,
                             dict(extra_headers or {}))
        if content_length is not None:
            headers["Content-Length"] = str(content_length)
        last: Exception | None = None
        for attempt in range(attempts):
            if data is not None and hasattr(data, "seek"):
                data.seek(0)
            request = urllib.request.Request(
                self._url(key, query), data=data, method=method, headers=headers)
            try:
                with urllib.request.urlopen(request, timeout=120) as response:
                    body = response.read() if return_body else None
                    if stream_to is not None:
                        part = stream_to.parent / (stream_to.name + ".part")
                        part.parent.mkdir(parents=True, exist_ok=True)
                        with open(part, "wb") as out:
                            while True:
                                block = response.read(BLOCK)
                                if not block:
                                    break
                                out.write(block)
                        part.replace(stream_to)
                    return response.headers, body
            except urllib.error.HTTPError as error:
                raw = error.read().decode("utf-8", "replace")
                message = raw
                try:
                    message = ET.fromstring(raw).findtext("Message") or raw
                except ET.ParseError:
                    pass
                raise S3Error(f"{method} {key}: {error.code} {message}") from error
            except (urllib.error.URLError, OSError) as error:
                last = error
                if attempt + 1 < attempts:
                    time.sleep(1.0 + attempt)
        raise S3Error(f"{method} {key}: {last}")

    # ── The four verbs ────────────────────────────────────────────────

    def put(self, key: str, path: Path) -> str:
        """Upload a file, streaming it: hash once for the signature and
        the metadata, send once a block at a time. Returns the sha256
        stored beside the object."""
        digest = stream_sha256(path)
        size = path.stat().st_size
        with open(path, "rb") as handle:
            self._call("PUT", key, "", data=handle, content_length=size,
                       payload_hash=digest,
                       extra_headers={
                           "x-amz-meta-sha256": digest,
                           "x-amz-meta-mtime": str(int(path.stat().st_mtime)),
                       })
        return digest

    def head(self, key: str) -> dict[str, str] | None:
        """An object's metadata, or None when it is not there."""
        try:
            headers, _ = self._call("HEAD", key, "", attempts=1)
        except S3Error as error:
            if "404" in str(error):
                return None
            raise
        return {name.lower(): value for name, value in headers.items()}

    def get(self, key: str, to: Path) -> None:
        """Download an object to a path, atomically: the bytes land in a
        `.part` beside the destination and are renamed in, so a pull
        interrupted mid-file never leaves a whole-looking half."""
        self._call("GET", key, "", stream_to=to)

    def list(self, prefix: str) -> list[tuple[str, int]]:
        """Every key under `prefix` with its size, across the
        continuation-token pagination S3 pages long listings by."""
        found: list[tuple[str, int]] = []
        token = ""
        while True:
            query_parts = [
                ("list-type", "2"),
                ("prefix", prefix),
            ]
            if token:
                query_parts.append(("continuation-token", token))
            # The canonical query is sorted by name, encoded once, and
            # sent exactly as it was signed.
            pairs = sorted(query_parts)
            query = "&".join(f"{_uri_encode(k)}={_uri_encode(v)}" for k, v in pairs)
            _, body = self._call("GET", "", query, return_body=True)
            root = ET.fromstring(body)
            namespace = root.tag.partition("}")[0] + "}" if root.tag.startswith("{") else ""
            for contents in root.findall(f"{namespace}Contents"):
                key = contents.findtext(f"{namespace}Key") or ""
                size = int(contents.findtext(f"{namespace}Size") or "0")
                found.append((key, size))
            token = root.findtext(f"{namespace}NextContinuationToken") or ""
            if not token:
                return found


class S3Error(RuntimeError):
    """A refusal from the wire, parsed and said plainly."""


# ── The farm's own shapes ─────────────────────────────────────────────

def progress_plan(farm: Path) -> dict | None:
    """The plan event in a farm's progress.jsonl: how many parts the
    orchestrator asked for, and whether the audio part is among them.
    A torn tail is ignored, the same as the Rust reader."""
    plan = None
    path = farm / "progress.jsonl"
    if not path.is_file():
        return None
    for line in path.read_text(encoding="utf-8", errors="replace").splitlines():
        try:
            event = json.loads(line)
        except ValueError:
            continue
        if event.get("event") == "plan":
            plan = event
    return plan


def marker_key(text: str) -> str | None:
    """The key a `.done` marker names: its first field, whatever wrote
    it - the same read `farm::marker_key` makes."""
    return text.split()[0] if text.split() else None


def find_geneva(named: str | None) -> str:
    """The sidecar: the one the caller named, the env's own, or one on
    PATH - the same order the agent binary discovers it."""
    if named:
        return named
    env = os.environ.get("CONCAT_GENEVA_BIN")
    if env:
        return env
    found = shutil.which("geneva")
    if found:
        return found
    raise S3Error("no geneva binary: name it with `geneva`, set "
                  "CONCAT_GENEVA_BIN, or put one on PATH")


def ask_geneva_for_parts(geneva: str, staged: Path, parts: int) -> tuple[list[tuple[int, int]], bool]:
    """The engine's own plan verb: where the good part boundaries are.
    The same call `farm::ask_for_parts` makes, answered the same way."""
    command = [
        geneva, "--format", "json", "plan", "timeline.json",
        "--parts", str(max(1, parts)), "--output", "reel.mp4",
    ]
    done = subprocess.run(command, cwd=staged, capture_output=True, text=True,
                          timeout=120)
    if done.returncode != 0:
        raise S3Error(f"the sidecar's plan verb failed: {done.stderr.strip()[:400]}")
    report = json.loads(done.stdout or "{}")
    ranges: list[tuple[int, int]] = []
    for part in report.get("parts", []):
        frames = part.get("frames")
        if isinstance(frames, list) and len(frames) == 2:
            ranges.append((int(frames[0]), int(frames[1])))
    has_audio = isinstance(report.get("audio"), dict) and "output" in report.get("audio", {})
    if not ranges:
        raise S3Error("the sidecar's plan named no parts; a timeline too short "
                      "to split is not a cloud farm's job")
    return ranges, has_audio


def render_one_part(geneva: str, staged: Path, farm: Path, index: int,
                    start: int, end: int, audio_only: bool,
                    timeline: int, timeout: float) -> str:
    """Render one part into the farm, atomically, and mark it done with
    the key the joiner will compute - the worker's half of
    `farm::render`, in the same shapes."""
    name = "audio" if audio_only else f"part-{index:03d}"
    final = farm / f"{name}.mp4"
    temp = final.with_suffix(final.suffix + ".part")
    args = [geneva, "--format", "json", "render", "timeline.json"]
    if audio_only:
        args.append("--audio-only")
    else:
        args += ["--frames", f"{start}..{end}"]
    args += ["-o", str(temp)]
    done = subprocess.run(args, cwd=staged, capture_output=True, text=True,
                          timeout=max(60.0, timeout))
    if done.returncode != 0 or not temp.is_file():
        raise S3Error(f"the sidecar refused part {index}: "
                      f"{(done.stderr or done.stdout).strip()[:400]}")
    final.parent.mkdir(parents=True, exist_ok=True)
    temp.replace(final)
    key = part_key(timeline, start, end)
    marker = final.with_suffix(".done")
    marker_temp = marker.with_suffix(".done.part")
    marker_temp.write_text(f"{key} 0.000\n", encoding="utf-8")
    marker_temp.replace(marker)
    return key


# ── push, pull, status, work ──────────────────────────────────────────

def _walk(root: Path) -> list[Path]:
    """Every file under `root`, relative paths included, skipping the
    `.part` temporaries an in-flight write leaves beside its destination."""
    if not root.is_dir():
        return []
    found = []
    for path in sorted(root.rglob("*")):
        if path.is_file() and path.suffix != ".part":
            found.append(path)
    return found


def _client(spec: dict) -> S3:
    """The client, from the environment the operator set - never from
    the spec: the same posture the VLM endpoint allow-list takes, so a
    prompted agent cannot aim the render at a host of its choosing."""
    bucket = os.environ.get("FUNCUT_FARM_BUCKET", "")
    if not bucket:
        raise S3Error("no cloud farm: set FUNCUT_FARM_BUCKET (and the "
                      "AWS_ACCESS_KEY_ID / AWS_SECRET_ACCESS_KEY beside it)")
    prefix = spec.get("prefix") or os.environ.get("FUNCUT_FARM_PREFIX", "")
    if not prefix:
        prefix = "farms/" + Path(spec.get("farm", "farm")).name
    region = os.environ.get("FUNCUT_FARM_REGION",
                            os.environ.get("AWS_REGION", "us-east-1"))
    access = os.environ.get("AWS_ACCESS_KEY_ID", "")
    secret = os.environ.get("AWS_SECRET_ACCESS_KEY", "")
    if not access or not secret:
        raise S3Error("no cloud farm credentials: set AWS_ACCESS_KEY_ID and "
                      "AWS_SECRET_ACCESS_KEY in the operator's environment")
    endpoint = os.environ.get("FUNCUT_FARM_ENDPOINT", "") or None
    return S3(bucket, prefix, region, access, secret, endpoint)


def _trees(spec: dict) -> tuple[Path, Path | None, S3]:
    """The farm directory, the staged render directory beside it in the
    spec, and the client. The staged tree lands under `staged/` in the
    prefix, the farm tree at the prefix's own root, so one prefix holds
    the whole unit of work."""
    farm = Path(spec.get("farm", "")).expanduser()
    if not spec.get("farm"):
        raise S3Error("`farm` is required: the farm directory itself")
    staged = Path(spec["staged"]).expanduser() if spec.get("staged") else None
    return farm, staged, _client(spec)


def cmd_push(spec: dict) -> dict:
    """Upload the farm (and the staged directory when named) under the
    prefix, skipping the files whose stored sha256 already matches. A
    dead worker's re-push costs what changed, not what existed."""
    farm, staged, client = _trees(spec)
    uploaded, skipped, bytes_up = [], [], 0
    for path in _walk(farm):
        key = client.prefix + path.relative_to(farm).as_posix()
        remote = client.head(key)
        digest = stream_sha256(path)
        if remote and remote.get("x-amz-meta-sha256") == digest:
            skipped.append(key)
            continue
        client.put(key, path)
        uploaded.append(key)
        bytes_up += path.stat().st_size
    staged_keys: list[str] = []
    if staged is not None:
        for path in _walk(staged):
            key = client.prefix + "staged/" + path.relative_to(staged).as_posix()
            remote = client.head(key)
            digest = stream_sha256(path)
            if remote and remote.get("x-amz-meta-sha256") == digest:
                skipped.append(key)
                continue
            client.put(key, path)
            staged_keys.append(key)
            bytes_up += path.stat().st_size
    return {
        "ok": True,
        "prefix": client.prefix,
        "uploaded": len(uploaded) + len(staged_keys),
        "skipped": len(skipped),
        "bytes": bytes_up,
        "note": "Files whose stored sha256 matched were left alone; nothing "
                "was deleted, and .part temporaries never travelled.",
    }


def cmd_pull(spec: dict) -> dict:
    """Download the prefix's tree back: the farm files to the farm
    directory, the `staged/` subtree to the staged directory. A file is
    skipped when its local size already matches - pull makes the tree
    whole; the farm's own key discipline is what catches a stale part."""
    farm, staged, client = _trees(spec)
    pulled, skipped, bytes_down = [], [], 0
    remote = client.list(client.prefix)
    for key, size in remote:
        rel = key[len(client.prefix):]
        if rel.startswith("staged/"):
            if staged is None:
                continue
            to = staged / rel[len("staged/"):]
        else:
            to = farm / rel
        if to.is_file() and to.stat().st_size == size:
            skipped.append(key)
            continue
        client.get(key, to)
        pulled.append(key)
        bytes_down += size
    return {
        "ok": True,
        "prefix": client.prefix,
        "pulled": len(pulled),
        "skipped": len(skipped),
        "bytes": bytes_down,
        "staged": str(staged) if staged else None,
    }


def cmd_status(spec: dict) -> dict:
    """What is remote, what is local, and what differs - the answer an
    orchestrator reads before it decides to pull and join."""
    farm, staged, client = _trees(spec)
    remote = dict(client.list(client.prefix))
    local: dict[str, int] = {}
    for path in _walk(farm):
        local[client.prefix + path.relative_to(farm).as_posix()] = path.stat().st_size
    if staged is not None:
        for path in _walk(staged):
            local[client.prefix + "staged/" + path.relative_to(staged).as_posix()] = \
                path.stat().st_size
    both = {k: (v, local.get(k)) for k, v in remote.items()}
    missing_local = [k for k in remote if k not in local]
    missing_remote = [k for k in local if k not in remote]
    differs = [k for k, (remote_size, local_size) in both.items()
               if local_size is not None and remote_size is not None
               and remote_size != local_size]
    return {
        "ok": True,
        "prefix": client.prefix,
        "remoteObjects": len(remote),
        "remoteBytes": sum(remote.values()),
        "localObjects": len(local),
        "missingLocal": len(missing_local),
        "missingRemote": len(missing_remote),
        "differing": len(differs),
        "note": "The counts, not the names: pull and push name what moves.",
    }


def cmd_work(spec: dict) -> dict:
    """A worker's whole day: pull the farm and its staged directory,
    render one missing part through the sidecar with the farm's own
    atomic shapes and keys, push the part and its marker back. The
    worker never touches progress.jsonl - that stream is the
    orchestrator's story, and two writers would be a torn one."""
    farm, staged, client = _trees(spec)
    if staged is None:
        raise S3Error("`work` needs `staged`: the render directory the "
                      "timeline was staged into (the --cache of the render)")
    cmd_pull({"farm": str(farm), "staged": str(staged), "prefix": client.prefix})
    timeline_path = staged / "timeline.json"
    if not timeline_path.is_file():
        raise S3Error("the staged directory holds no timeline.json; pull the "
                      "staged tree the orchestrator pushed")
    timeline = stream_fnv("concat-motion/farm/1", timeline_path)
    plan = progress_plan(farm)
    parts = int(spec.get("parts") or (plan or {}).get("parts") or 0)
    if parts <= 0:
        raise S3Error("`work` needs `parts`, or a progress.jsonl whose plan "
                      "event says the number the orchestrator asked for")
    geneva = find_geneva(spec.get("geneva"))
    ranges, has_audio = ask_geneva_for_parts(geneva, staged, parts)
    timeout = float(spec.get("timeoutSeconds") or 3600.0)
    wanted = int(spec["part"]) if spec.get("part") is not None else None

    def done(index: int, start: int, end: int, audio: bool) -> bool:
        name = "audio" if audio else f"part-{index:03d}"
        final = farm / f"{name}.mp4"
        marker = final.with_suffix(".done")
        if not final.is_file() or not marker.is_file():
            return False
        return marker_key(marker.read_text(encoding="utf-8")) == \
            part_key(timeline, start, end)

    candidates: list[tuple[int, int, int, bool]] = []
    for index, (start, end) in enumerate(ranges):
        candidates.append((index, start, end, False))
    if has_audio:
        candidates.append((len(ranges), 0, 0, True))
    if wanted is not None and not (0 <= wanted < len(candidates)):
        raise S3Error(f"`part` {wanted} is not one of the {len(candidates)} "
                      "parts the plan named")
    missing = [entry for entry in candidates if not done(*entry[0:3], entry[3])]
    if not missing:
        return {
            "ok": True, "done": True, "farm": str(farm), "parts": len(candidates),
            "note": "Every part the plan named is done; pull and join from "
                    "the orchestrator, or ask for `status`.",
        }
    chosen = missing[0]
    if wanted is not None:
        chosen = next((entry for entry in missing if entry[0] == wanted), chosen)
    index, start, end, audio = chosen
    key = render_one_part(geneva, staged, farm, index, start, end, audio,
                          timeline, timeout)
    pushed = []
    name = "audio" if audio else f"part-{index:03d}"
    for suffix in (".mp4", ".done"):
        path = farm / f"{name}{suffix}"
        push_key = client.prefix + path.relative_to(farm).as_posix()
        client.put(push_key, path)
        pushed.append(push_key)
    return {
        "ok": True, "done": False, "part": index, "from": start, "to": end,
        "audio": audio, "key": key, "pushed": pushed,
        "remaining": len(missing) - 1, "parts": len(candidates),
        "geneva": geneva,
    }


# ── The selftest: a mock S3 that verifies signatures ─────────────────

class MockS3Handler(http.server.BaseHTTPRequestHandler):
    """An S3-shaped store in memory that VERIFIES the SigV4 signature of
    every request it serves - the same chain the client computes, run
    again from the request's own bytes, so a signature that does not
    prove the request fails here, not in production."""

    store: dict[str, tuple[bytes, dict[str, str]]] = {}
    access_key = "AKIAFARMSYNCSELFTEST"
    secret_key = "wJalrXUtnFEMI/K7MDENG/bPxRfiCYSELFTESTKEY"
    region = "us-east-1"

    def log_message(self, *_args):
        pass

    def _verify(self, body: bytes) -> None:
        auth = self.headers.get("Authorization", "")
        if not auth.startswith("AWS4-HMAC-SHA256"):
            raise AssertionError("the request carries no SigV4 authorization")
        credential = re.search(r"Credential=([^/]+)/", auth)
        signed = re.search(r"SignedHeaders=([^,]+),", auth)
        signature = re.search(r"Signature=([0-9a-f]+)", auth)
        if not (credential and signed and signature):
            raise AssertionError(f"a malformed authorization: {auth}")
        if credential.group(1) != self.access_key:
            raise AssertionError("the request is signed by another key")
        path = urllib.parse.urlparse(self.path).path
        query = urllib.parse.urlparse(self.path).query
        # The canonical query is the raw, already-sorted string.
        now = self.headers.get("x-amz-date", "")
        date = now[:8]
        headers = {"host": self.headers.get("Host", ""), }
        for name in signed.group(1).split(";"):
            if name != "host":
                headers[name] = self.headers.get(name, "")
        canonical_headers = "".join(f"{n}:{headers[n].strip()}\n" for n in sorted(headers))
        payload = self.headers.get("x-amz-content-sha256",
                                   hashlib.sha256(b"").hexdigest())
        canonical = "\n".join([
            self.command, path, query, canonical_headers,
            signed.group(1), payload,
        ])
        scope = f"{date}/{self.region}/s3/aws4_request"
        string_to_sign = "\n".join([
            "AWS4-HMAC-SHA256", now, scope,
            hashlib.sha256(canonical.encode()).hexdigest(),
        ])
        chain = ("AWS4" + self.secret_key).encode()
        for part in (date, self.region, "s3", "aws4_request"):
            chain = hmac.new(chain, part.encode(), hashlib.sha256).digest()
        expected = hmac.new(chain, string_to_sign.encode(), hashlib.sha256).hexdigest()
        if expected != signature.group(1):
            raise AssertionError(
                f"the signature does not verify: expected {expected}, "
                f"got {signature.group(1)}")

    def do_PUT(self):
        length = int(self.headers.get("Content-Length", "0"))
        body = self.rfile.read(length) if length else b""
        self._verify(body)
        key = urllib.parse.urlparse(self.path).path.lstrip("/").split("/", 1)[1]
        meta = {
            "x-amz-meta-sha256": self.headers.get("x-amz-meta-sha256", ""),
            "content-length": str(length),
        }
        MockS3Handler.store[key] = (body, meta)
        self.send_response(200)
        self.send_header("ETag", f"\"{hashlib.md5(body).hexdigest()}\"")
        self.end_headers()

    def do_HEAD(self):
        self._verify(b"")
        key = urllib.parse.urlparse(self.path).path.lstrip("/").split("/", 1)[1]
        if key not in MockS3Handler.store:
            self.send_response(404)
            body = ("<Error><Code>NoSuchKey</Code>"
                    "<Message>The specified key does not exist.</Message></Error>")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body.encode())
            return
        body, meta = MockS3Handler.store[key]
        self.send_response(200)
        self.send_header("Content-Length", meta["content-length"])
        self.send_header("x-amz-meta-sha256", meta["x-amz-meta-sha256"])
        self.end_headers()

    def do_GET(self):
        self._verify(b"")
        parsed = urllib.parse.urlparse(self.path)
        if parsed.query.startswith("list-type=2") or "list-type=2" in parsed.query:
            prefix = urllib.parse.parse_qs(parsed.query).get("prefix", [""])[0]
            keys = sorted(k for k in MockS3Handler.store if k.startswith(prefix))
            entries = "".join(
                f"<Contents><Key>{k}</Key><Size>{len(MockS3Handler.store[k][0])}"
                "</Size></Contents>" for k in keys)
            body = (f"<ListBucketResult>{entries}"
                    "<IsTruncated>false</IsTruncated></ListBucketResult>")
        else:
            key = parsed.path.lstrip("/").split("/", 1)[1]
            if key not in MockS3Handler.store:
                self.send_response(404)
                err = ("<Error><Code>NoSuchKey</Code>"
                       "<Message>The specified key does not exist.</Message></Error>")
                self.send_header("Content-Length", str(len(err)))
                self.end_headers()
                self.wfile.write(err.encode())
                return
            body_bytes, meta = MockS3Handler.store[key]
            self.send_response(200)
            self.send_header("Content-Length", meta["content-length"])
            self.send_header("x-amz-meta-sha256", meta["x-amz-meta-sha256"])
            self.end_headers()
            self.wfile.write(body_bytes)
            return
        self.send_response(200)
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body.encode())


def _stub_geneva(dirpath: Path) -> Path:
    """A stand-in sidecar that speaks the two verbs the farm drives:
    `plan` names three 30-frame parts, `render` writes the bytes it was
    asked for. The real one is pinned, downloaded and hashed; this one
    exists so the selftest proves the WORKFLOW, not the engine."""
    path = dirpath / "geneva-stub"
    path.write_text(
        "#!/usr/bin/env python3\n"
        "import json, sys\n"
        "args = sys.argv[1:]\n"
        "if 'plan' in args:\n"
        "    parts = int(args[args.index('--parts') + 1])\n"
        "    spans = [[i * 30, (i + 1) * 30] for i in range(parts)]\n"
        "    print(json.dumps({'parts': [{'frames': s} for s in spans], "
        "'audio': None}))\n"
        "elif 'render' in args:\n"
        "    out = args[args.index('-o') + 1]\n"
        "    frames = args[args.index('--frames') + 1] if '--frames' in args "
        "else 'audio'\n"
        "    open(out, 'wb').write(frames.encode() + b'-' + out.encode())\n"
        "    print(json.dumps({'ok': True}))\n",
        encoding="utf-8")
    path.chmod(0o755)
    return path


def cmd_selftest(_spec: dict) -> dict:
    """The client against a mock S3 in-process: signatures verified, a
    push/pull round trip, the sha256 skip, the FNV vectors, the part
    key's shape, and the whole `work` loop over a stub sidecar."""
    checks: list[str] = []

    # The FNV-1a vectors the standard owns, so the key parity here is
    # the key parity there.
    assert fnv1a64(b"") == 0xCBF29CE484222325
    assert fnv1a64(b"a") == 0xAF63DC4C8601EC8C
    assert fnv1a64(b"foobar") == 0x85944171F73967E8
    checks.append("fnv1a64: the three published vectors hold")

    key = part_key(1, 2, 3)
    assert key == "0" * 15 + "1." + "0" * 15 + "2." + "0" * 15 + "3", key
    assert marker_key("0123456789abcdef. 0.000") == "0123456789abcdef."
    checks.append("part_key and marker_key: the farm's own shapes")

    MockS3Handler.store = {}
    server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), MockS3Handler)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    port = server.server_address[1]
    try:
        root = Path(tempfile.mkdtemp(prefix="farm-sync-selftest-"))
        farm = root / "farm"
        staged = root / "staged"
        farm.mkdir()
        staged.mkdir()
        # A farm worth syncing: a done part with its marker, a missing
        # one, and a progress stream that names a three-part plan.
        (farm / "part-000.mp4").write_bytes(b"part-zero-bytes")
        (farm / "part-000.done").write_text(
            part_key(1, 0, 30) + " 0.500\n", encoding="utf-8")
        (farm / "progress.jsonl").write_text(
            json.dumps({"event": "plan", "parts": 3, "frames": 90, "audio": False})
            + "\n", encoding="utf-8")
        (staged / "timeline.json").write_text(
            json.dumps({"format": 1, "width": 480, "height": 270,
                        "fps": 30, "duration": 3.0, "nodes": []}),
            encoding="utf-8")
        (staged / "assets").mkdir()
        (staged / "assets" / "plate.png").write_bytes(b"pngpngpng")

        client = S3("bucket", "farms/selftest/", MockS3Handler.region,
                    MockS3Handler.access_key, MockS3Handler.secret_key,
                    f"http://127.0.0.1:{port}")
        # The environment the client normally reads, set here so _trees
        # walks the same door the tool does.
        os.environ["FUNCUT_FARM_BUCKET"] = "bucket"
        os.environ["FUNCUT_FARM_PREFIX"] = "farms/selftest/"
        os.environ["AWS_ACCESS_KEY_ID"] = MockS3Handler.access_key
        os.environ["AWS_SECRET_ACCESS_KEY"] = MockS3Handler.secret_key
        os.environ["FUNCUT_FARM_REGION"] = MockS3Handler.region
        os.environ["FUNCUT_FARM_ENDPOINT"] = f"http://127.0.0.1:{port}"

        reply = cmd_push({"farm": str(farm), "staged": str(staged)})
        assert reply["ok"], reply
        assert len(MockS3Handler.store) == 5, sorted(MockS3Handler.store)
        checks.append("push: five objects, every signature verified by the mock")

        # The second push of the same bytes skips them all.
        reply = cmd_push({"farm": str(farm), "staged": str(staged)})
        assert reply["skipped"] == 5 and not reply["uploaded"], reply
        checks.append("push again: the sha256 metadata skips what is already there")

        status = cmd_status({"farm": str(farm), "staged": str(staged)})
        assert status["remoteObjects"] == 5, status
        assert status["missingLocal"] == 0 and status["missingRemote"] == 0, status
        checks.append("status: the remote and the local agree")

        # A worker elsewhere: a fresh directory pair, the whole `work`
        # loop against the stub sidecar, and the marker the joiner
        # will believe.
        worker_root = Path(tempfile.mkdtemp(prefix="farm-sync-worker-"))
        worker_farm = worker_root / "farm"
        worker_staged = worker_root / "staged"
        geneva = _stub_geneva(worker_root)
        work = cmd_work({"farm": str(worker_farm), "staged": str(worker_staged),
                         "parts": 3, "geneva": str(geneva), "prefix": "farms/selftest/"})
        assert work["ok"] and work["done"] is False, work
        assert work["part"] == 0 and work["from"] == 0 and work["to"] == 30, work
        marker = (worker_farm / "part-000.done").read_text(encoding="utf-8")
        timeline_hash = stream_fnv("concat-motion/farm/1", worker_staged / "timeline.json")
        assert marker.startswith(part_key(timeline_hash, 0, 30) + " "), marker
        checks.append("work: pulled, rendered part 0 through the stub, marked "
                      "with the joiner's own key")

        work = cmd_work({"farm": str(worker_farm), "staged": str(worker_staged),
                         "parts": 3, "geneva": str(geneva), "prefix": "farms/selftest/"})
        assert work["done"] is False and work["part"] == 1, work
        work = cmd_work({"farm": str(worker_farm), "staged": str(worker_staged),
                         "parts": 3, "geneva": str(geneva), "prefix": "farms/selftest/"})
        assert work["done"] is False and work["part"] == 2, work
        work = cmd_work({"farm": str(worker_farm), "staged": str(worker_staged),
                         "parts": 3, "geneva": str(geneva), "prefix": "farms/selftest/"})
        assert work["done"] is True, work
        checks.append("work: parts 1 and 2 drawn, the fourth call serves")

        # And the orchestrator's side: a fresh pull lands every part.
        join_farm = root / "joined"
        reply = cmd_pull({"farm": str(join_farm), "staged": str(root / "joined-staged"),
                          "prefix": "farms/selftest/"})
        assert reply["ok"], reply
        assert (join_farm / "part-000.mp4").is_file()
        assert (join_farm / "part-002.done").is_file()
        checks.append("pull: the whole farm lands on the joiner's side")

        shutil.rmtree(root, ignore_errors=True)
        shutil.rmtree(worker_root, ignore_errors=True)
    finally:
        server.shutdown()
        server.server_close()

    return {
        "ok": True,
        "checks": checks,
        "note": "No network left this process: the mock S3 verified every "
                "signature, the stub sidecar answered the farm's two verbs, "
                "and the keys were the joiner's own.",
    }


def main() -> None:
    import argparse
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("cmd", choices=["push", "pull", "status", "work", "selftest"])
    parser.add_argument("--spec", default="{}")
    args = parser.parse_args()
    try:
        raw = sys.stdin.read() if args.spec == "-" else args.spec
        spec = json.loads(raw or "{}")
        if not isinstance(spec, dict):
            raise ValueError("spec must be a JSON object")
        if args.cmd == "push":
            out = cmd_push(spec)
        elif args.cmd == "pull":
            out = cmd_pull(spec)
        elif args.cmd == "status":
            out = cmd_status(spec)
        elif args.cmd == "work":
            out = cmd_work(spec)
        else:
            out = cmd_selftest(spec)
    except (S3Error, ValueError, OSError, subprocess.SubprocessError) as error:
        out = {"ok": False, "error": str(error)}
    json.dump(out, sys.stdout, indent=1)
    sys.stdout.write("\n")


if __name__ == "__main__":
    main()
