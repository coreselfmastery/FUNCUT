"""concat-vlm: zero-install local vision critic for the FUNCUT editor.

The editor ships *no* model weights. `setup()` downloads a sub-1B vision model
plus a llama.cpp server binary into the app data dir on first use, then starts
an OpenAI-compatible endpoint on localhost. Nothing is installed system-wide,
no Python packages, no pip, no admin rights. If any step fails the caller gets
a structured error and the editor falls back to the deterministic pixel-based
craft scorer in sense.py.

Verified, public, pinned artifacts (checked 2026-10-27):

  ggml-org/SmolVLM2-500M-Video-Instruct-GGUF
    SmolVLM2-500M-Video-Instruct-Q8_0.gguf          436.8 MB
    mmproj-SmolVLM2-500M-Video-Instruct-Q8_0.gguf   108.8 MB
    -> 0.5B parameters, ~546 MB on disk, CPU-only, 4 GB RAM is enough,
       and it natively understands sequences of video frames.

  ggml-org/llama.cpp release b11930 prebuilt llama-server binaries.

Set FUNCUT_VLM_ENDPOINT to reuse an already-running Ollama / LM Studio /
llama.cpp server and skip the download entirely.
"""

from __future__ import annotations

import json
import os
import platform
import shutil
import stat
import subprocess
import sys
import tarfile
import time
import urllib.error
import urllib.request
import zipfile
from pathlib import Path

LLAMA_RELEASE = "b11930"
LLAMA_REPO = "ggml-org/llama.cpp"
DEFAULT_PORT = 8099

PROFILES = {
    # Every-PC default. 0.5B params, ~546 MB. No GPU, no AVX-512 requirement.
    "mini": {
        "repo": "ggml-org/SmolVLM2-500M-Video-Instruct-GGUF",
        "model": "SmolVLM2-500M-Video-Instruct-Q8_0.gguf",
        "mmproj": "mmproj-SmolVLM2-500M-Video-Instruct-Q8_0.gguf",
        "model_mb": 437,
        "mmproj_mb": 109,
        "params": "0.5B",
    },
    # Slightly better single-image reasoning, same weight class.
    "image": {
        "repo": "ggml-org/SmolVLM-500M-Instruct-GGUF",
        "model": "SmolVLM-500M-Instruct-Q8_0.gguf",
        "mmproj": "mmproj-SmolVLM-500M-Instruct-Q8_0.gguf",
        "model_mb": 437,
        "mmproj_mb": 109,
        "params": "0.5B",
    },
    # "MiniCPM" class. Heavier but stronger on dense text / charts.
    "minicpm": {
        "repo": "openbmb/MiniCPM-V-2-gguf",
        "model": "ggml-model-Q2_K.gguf",
        "mmproj": "mmproj-model-f16.gguf",
        "model_mb": 1298,
        "mmproj_mb": 867,
        "params": "2.8B",
    },
}


def data_dir() -> Path:
    override = os.environ.get("FUNCUT_VLM_DIR")
    if override:
        return Path(override).expanduser()
    if sys.platform == "darwin":
        return Path.home() / "Library" / "Application Support" / "app.concat.editor" / "vlm"
    if os.name == "nt":
        return Path(os.environ.get("APPDATA", Path.home())) / "app.concat.editor" / "vlm"
    return Path(
        os.environ.get("XDG_DATA_HOME", str(Path.home() / ".local" / "share"))
    ) / "app.concat.editor" / "vlm"


def _http(url: str, timeout: float = 8.0):
    req = urllib.request.Request(url, headers={"User-Agent": "concat-editor-vlm/1.0"})
    return urllib.request.urlopen(req, timeout=timeout)


def _json(url: str, timeout: float = 8.0):
    with _http(url, timeout) as resp:
        return json.loads(resp.read().decode("utf-8", "replace"))


def _probe_openai(base: str):
    for path in ("/v1/models", "/models"):
        try:
            d = _json(base.rstrip("/") + path, 2.5)
            models = [m.get("id", "") for m in d.get("data", [])]
            return {"type": "openai", "base": base.rstrip("/"), "models": models}
        except Exception:
            continue
    return None


def detect(timeout: float = 3.0):
    """Find any already-running local vision-capable server. No downloads."""
    candidates = []
    env = os.environ.get("FUNCUT_VLM_ENDPOINT")
    if env:
        candidates.append(env.rstrip("/"))
    candidates += [
        f"http://127.0.0.1:{DEFAULT_PORT}",
        "http://127.0.0.1:8080",
        "http://127.0.0.1:1234",
    ]
    for base in candidates:
        got = _probe_openai(base)
        if got:
            got["model"] = got["models"][0] if got["models"] else "local-model"
            return got
    # Ollama has a different API shape.
    for base in ("http://127.0.0.1:11434",):
        try:
            d = _json(base + "/api/tags", 2.5)
            names = [m.get("name", "") for m in d.get("models", [])]
            vision = [n for n in names if any(k in n for k in ("vl", "vision", "llava", "minicpm", "smol"))]
            if names:
                return {
                    "type": "ollama",
                    "base": base,
                    "models": names,
                    "model": vision[0] if vision else names[0],
                }
        except Exception:
            pass
    return None


def _asset_name():
    system, machine = platform.system().lower(), platform.machine().lower()
    if system == "linux":
        if machine in ("aarch64", "arm64"):
            return f"llama-{LLAMA_RELEASE}-bin-ubuntu-arm64.tar.gz"
        return f"llama-{LLAMA_RELEASE}-bin-ubuntu-x64.tar.gz"
    if system == "darwin":
        if machine in ("arm64", "aarch64"):
            return f"llama-{LLAMA_RELEASE}-bin-macos-arm64.tar.gz"
        return f"llama-{LLAMA_RELEASE}-bin-macos-x64.tar.gz"
    if system == "windows":
        if machine in ("arm64", "aarch64"):
            return f"llama-{LLAMA_RELEASE}-bin-win-cpu-arm64.zip"
        return f"llama-{LLAMA_RELEASE}-bin-win-cpu-x64.zip"
    return None


def _download(url: str, dest: Path, label: str, progress=None):
    if dest.exists() and dest.stat().st_size > 1_000_000:
        return dest
    # data_dir() is a pure path helper, so the very first download on a
    # fresh machine has NO parent directory yet. Every caller used to rely
    # on some earlier download having created it (or failed outright).
    dest.parent.mkdir(parents=True, exist_ok=True)
    tmp = dest.with_suffix(dest.suffix + ".part")
    tmp.unlink(missing_ok=True)
    try:
        with _http(url, 30.0) as resp, open(tmp, "wb") as out:
            total = int(resp.headers.get("Content-Length") or 0)
            done, chunk = 0, 1 << 20
            last_report = 0.0
            while True:
                buf = resp.read(chunk)
                if not buf:
                    break
                out.write(buf)
                done += len(buf)
                now = time.time()
                if progress and (now - last_report > 1.0 or done == total):
                    last_report = now
                    progress(label, done, total)
        if total and tmp.stat().st_size != total:
            raise RuntimeError(f"incomplete download for {label}: {tmp.stat().st_size}/{total} bytes")
        tmp.replace(dest)
    finally:
        tmp.unlink(missing_ok=True)
    return dest


def _find_server(root: Path):
    for p in root.rglob("*"):
        if p.is_file() and p.name in ("llama-server", "llama-server.exe", "server"):
            return p
    return None


def server_binary(progress=None):
    root = data_dir() / "server"
    existing = _find_server(root)
    if existing:
        return existing
    asset = _asset_name()
    if not asset:
        raise RuntimeError(f"unsupported platform {platform.system()}/{platform.machine()}")
    url = f"https://github.com/{LLAMA_REPO}/releases/download/{LLAMA_RELEASE}/{asset}"
    archive = _download(url, data_dir() / asset, "llama.cpp server", progress)
    root.mkdir(parents=True, exist_ok=True)
    if archive.suffix == ".zip":
        with zipfile.ZipFile(archive) as z:
            z.extractall(root)
    else:
        with tarfile.open(archive) as t:
            try:
                t.extractall(root, filter="data")
            except TypeError:  # Python < 3.12
                t.extractall(root)
    found = _find_server(root)
    if not found:
        raise RuntimeError("llama-server binary missing from downloaded archive")
    found.chmod(found.stat().st_mode | stat.S_IXUSR | stat.S_IXGRP | stat.S_IXOTH)
    archive.unlink(missing_ok=True)
    return found


def model_files(profile: str, progress=None):
    p = PROFILES[profile]
    base = f"https://huggingface.co/{p['repo']}/resolve/main"
    target = data_dir() / "models" / profile
    target.mkdir(parents=True, exist_ok=True)
    model = _download(f"{base}/{p['model']}", target / p["model"], "vision model", progress)
    mmproj = _download(f"{base}/{p['mmproj']}", target / p["mmproj"], "vision projector", progress)
    return model, mmproj


def _server_alive(base: str):
    try:
        with _http(base + "/health", 1.5) as r:
            return r.status == 200
    except Exception:
        return _probe_openai(base) is not None


def start(profile: str = "mini", port: int = DEFAULT_PORT, progress=None):
    existing = detect()
    if existing:
        return {**existing, "reused": True}
    binary = server_binary(progress)
    model, mmproj = model_files(profile, progress)
    log_path = data_dir() / "llama-server.log"
    log_path.parent.mkdir(parents=True, exist_ok=True)
    cmd = [
        str(binary), "-m", str(model), "--mmproj", str(mmproj),
        "--host", "127.0.0.1", "--port", str(port),
        # 6144, not the 2048 a hello-world demo uses: the strong six-axis
        # prompt (brief + measured facts + image tokens + the reply) runs
        # past 4k tokens, and a server that truncates its input answers
        # from half a question.
        "-c", "6144", "-ngl", "0", "-t", str(max(2, min(8, os.cpu_count() or 4))),
    ]
    with open(log_path, "ab") as log:
        kwargs = {"stdout": log, "stderr": subprocess.STDOUT, "stdin": subprocess.DEVNULL}
        if os.name == "nt":
            kwargs["creationflags"] = subprocess.CREATE_NO_WINDOW
        subprocess.Popen(cmd, **kwargs)
    base = f"http://127.0.0.1:{port}"
    for _ in range(90):
        if _server_alive(base):
            got = _probe_openai(base) or {"type": "openai", "models": []}
            return {
                "type": "openai", "base": base, "models": got.get("models", []),
                "model": (got.get("models") or ["local-model"])[0],
                "profile": profile, "reused": False, "log": str(log_path),
            }
        time.sleep(1.0)
    tail = ""
    try:
        tail = log_path.read_text(errors="replace")[-1200:]
    except Exception:
        pass
    raise RuntimeError("llama-server did not become healthy in 90s. " + tail)


def health(base: str = None):
    base = (base or f"http://127.0.0.1:{DEFAULT_PORT}").rstrip("/")
    try:
        with _http(base + "/health", 1.5) as r:
            return r.status == 200
    except Exception:
        return False


def ask(prompt: str, images=None, endpoint=None, model=None, timeout=90.0,
        max_tokens=600, json_schema=None, temperature=0.2):
    """Send images + prompt to a local OpenAI-compatible vision endpoint.

    `json_schema` asks llama.cpp's server for grammar-constrained JSON. Not
    every OpenAI-compatible server accepts it, so a 400 with a schema set
    retries once without it rather than losing the whole critique.
    """
    ep = endpoint or detect()
    if not ep:
        raise RuntimeError("no local vision endpoint")
    base = ep["base"] if isinstance(ep, dict) else str(ep)
    chosen = model or (ep.get("model") if isinstance(ep, dict) else None) or "local-model"
    content = [{"type": "text", "text": prompt}]
    for b64 in images or []:
        content.append({
            "type": "image_url",
            "image_url": {"url": "data:image/png;base64," + b64},
        })
    if ep.get("type") == "ollama":
        payload = {
            "model": chosen, "stream": False,
            "messages": [{"role": "user", "content": prompt, "images": list(images or [])}],
            "format": "json",
        }
        url = base + "/api/chat"
        if json_schema:
            payload["format"] = json_schema
    else:
        payload = {
            "model": chosen, "stream": False, "max_tokens": max_tokens,
            "temperature": temperature,
            "messages": [{"role": "user", "content": content}],
        }
        url = base + "/v1/chat/completions"
        if json_schema:
            payload["json_schema"] = json_schema
            payload["response_format"] = {"type": "json_object", "schema": json_schema}

    def send(body):
        req = urllib.request.Request(
            url, data=json.dumps(body).encode(),
            headers={"Content-Type": "application/json", "User-Agent": "concat-editor-vlm/1.0"},
        )
        with urllib.request.urlopen(req, timeout=timeout) as resp:
            return json.loads(resp.read().decode("utf-8", "replace"))

    try:
        d = send(payload)
    except urllib.error.HTTPError as exc:
        if exc.code not in (400, 404, 422) or not json_schema:
            raise
        payload.pop("json_schema", None)
        payload.pop("response_format", None)
        if ep.get("type") == "ollama":
            payload["format"] = "json"
        d = send(payload)
    if ep.get("type") == "ollama":
        return d.get("message", {}).get("content", "")
    return d["choices"][0]["message"]["content"]


def status():
    ep = detect()
    out = {"available": bool(ep), "profiles": {k: {"params": v["params"], "mb": v["model_mb"] + v["mmproj_mb"]} for k, v in PROFILES.items()}, "dir": str(data_dir())}
    if ep:
        out.update({"endpoint": ep["base"], "model": ep.get("model"), "type": ep["type"]})
    return out


def _progress(label, done, total):
    pct = (100.0 * done / total) if total else 0.0
    print(f"[vlm] {label}: {done/1e6:.0f}/{total/1e6:.0f} MB ({pct:.0f}%)", file=sys.stderr, flush=True)


def main():
    import argparse
    ap = argparse.ArgumentParser()
    ap.add_argument("cmd", choices=["status", "setup", "ask", "health"])
    ap.add_argument("--profile", default="mini", choices=list(PROFILES))
    ap.add_argument("--port", type=int, default=DEFAULT_PORT)
    ap.add_argument("--spec", default="{}")
    args = ap.parse_args()
    try:
        if args.cmd == "status":
            out = {"ok": True, **status()}
        elif args.cmd == "setup":
            out = {"ok": True, "setup": True, **start(args.profile, args.port, _progress)}
        elif args.cmd == "health":
            out = {"ok": True, "healthy": health()}
        else:
            spec = json.loads(args.spec)
            text = ask(spec.get("prompt", ""), spec.get("images") or [], spec.get("endpoint"), spec.get("model"))
            out = {"ok": True, "text": text}
    except Exception as exc:
        out = {"ok": False, "error": f"{type(exc).__name__}: {exc}"}
    print(json.dumps(out, ensure_ascii=False), flush=True)


if __name__ == "__main__":
    main()
