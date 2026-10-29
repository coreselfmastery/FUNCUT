"""concat-vlm: the FUNCUT editor's vision critic, local or hosted.

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

Three ways to reach a stronger critic, in the order `detect()` tries them:

1. `FUNCUT_VLM_ENDPOINT` (+ optional `FUNCUT_VLM_API_KEY`,
   `FUNCUT_VLM_MODEL`) pointing at any OpenAI-compatible server - Ollama,
   LM Studio, llama.cpp, or a hosted API.
2. The saved provider config (`vlm.json` in the data dir), written by
   `configure()` / the `local_vlm {action:"configure"}` MCP tool. The
   bundled PROVIDERS table below carries the free-tier hosted vision APIs,
   their base URLs, default model ids and where their keys usually live.
3. A local server already running on :8099 / :8080 / :1234, or Ollama on
   :11434.

Privacy: the local paths never send a byte off the machine. A hosted
provider sends the contact-sheet frames to that provider's API - `configure()`
says so in every reply, and docs/VLM.md says so in bold.
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
import urllib.parse
import urllib.request
import zipfile
from pathlib import Path

LLAMA_RELEASE = "b11930"
LLAMA_REPO = "ggml-org/llama.cpp"
DEFAULT_PORT = 8099

# The largest artifact this module will ever fetch. The pinned models and
# server are hundreds of MB; a mirror that lies about Content-Length - or
# an endless stream - must not be able to fill the disk.
MAX_DOWNLOAD_BYTES = 2 * 1024 * 1024 * 1024  # 2 GiB

# SHA-256 of every artifact setup() fetches, pinned so that "verified,
# public, pinned artifacts" in the module docstring is enforced by code
# and not by trust in a CDN: the llama.cpp b11930 release assets (as the
# GitHub release API reports them) and the GGUF model files (as
# HuggingFace's own LFS object ids). A download that does not match its
# pin is deleted and the setup fails - a substituted binary must never
# reach chmod +x, let alone run.
LLAMA_SHA256 = {
    "llama-b11930-bin-ubuntu-x64.tar.gz":
        "36a884c75ea47242f532484d09aaa9ecc8bccd3079f901f9c4a340f38303c4a5",
    "llama-b11930-bin-ubuntu-arm64.tar.gz":
        "8207aeb8e977f9eacf12cb20a65c8dc4100fbde628cf437be128a524b97748da",
    "llama-b11930-bin-macos-arm64.tar.gz":
        "66e0aade3643f10f76fa1ac67bbce59d2e44dcc7138a6bf6d2110d2746601567",
    "llama-b11930-bin-macos-x64.tar.gz":
        "e5625c0370d2a12de415ec717a524c8d2104c81ca6b0ea78965b668c3a7f9b10",
    "llama-b11930-bin-win-cpu-arm64.zip":
        "7299ad2cc9bc68b1601a4d05799f709abc358f004a42b446658e84f0590552c5",
    "llama-b11930-bin-win-cpu-x64.zip":
        "43273de703aaafc0eae7639c1cf71b0b5e8932283190f7aa93f684bafcd1d739",
}
MODEL_SHA256 = {
    "SmolVLM2-500M-Video-Instruct-Q8_0.gguf":
        "6f67b8036b2469fcd71728702720c6b51aebd759b78137a8120733b4d66438bc",
    "mmproj-SmolVLM2-500M-Video-Instruct-Q8_0.gguf":
        "921dc7e259f308e5b027111fa185efcbf33db13f6e35749ddf7f5cdb60ef520b",
    "SmolVLM-500M-Instruct-Q8_0.gguf":
        "9d4612de6a42214499e301494a3ecc2be0abdd9de44e663bda63f1152fad1bf4",
    "mmproj-SmolVLM-500M-Instruct-Q8_0.gguf":
        "d1eb8b6b23979205fdf63703ed10f788131a3f812c7b1f72e0119d5d81295150",
    "ggml-model-Q2_K.gguf":
        "d6c3584306a32ec56b6ca6cce21c05fd01db629ee4360fce665baaece15d2792",
    "mmproj-model-f16.gguf":
        "79611c59b5ad5b0547256602e3fb546a3041bcf6db5058091b6bcaa31f3a1c95",
}

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

# Free-tier hosted vision providers (facts checked 2026-10-28; every one
# speaks the OpenAI chat-completions dialect `ask()` already uses, so a
# provider is just a base URL, a default vision model, the env var its key
# usually lives in, and an honest line about what the free tier gives).
# Model ids drift - the `providers` action lists what an endpoint actually
# serves, and FUNCUT_VLM_MODEL or the config file overrides the default.
PROVIDERS = {
    "groq": {
        "base": "https://api.groq.com/openai/v1",
        "model": "meta-llama/llama-4-scout-17b-16e-instruct",
        "key_env": "GROQ_API_KEY",
        "free": "free tier, no card needed; LPU-fast; roughly 30 requests/minute, limits are org-wide (console.groq.com shows the live numbers)",
        "keys": "https://console.groq.com/keys",
    },
    "gemini": {
        "base": "https://generativelanguage.googleapis.com/v1beta/openai",
        "model": "gemini-2.5-flash-lite",
        "key_env": "GEMINI_API_KEY",
        "free": "free tier covers the Flash models (the Pro models left the free tier in April 2026); aistudio.google.com shows the live quotas",
        "keys": "https://aistudio.google.com/apikey",
    },
    "zai": {
        "base": "https://api.z.ai/api/paas/v4",
        "model": "glm-4v-flash",
        "key_env": "ZAI_API_KEY",
        "free": "the GLM vision flash models are served free; check z.ai/model-api for the current model id and quota before trusting the default",
        "keys": "https://z.ai/model-api",
    },
    "openrouter": {
        "base": "https://openrouter.ai/api/v1",
        "model": "qwen/qwen2.5-vl-72b-instruct:free",
        "key_env": "OPENROUTER_API_KEY",
        "free": "models suffixed :free cost nothing and rotate often; pick a vision one at openrouter.ai/models before trusting the default id",
        "keys": "https://openrouter.ai/keys",
    },
}

CONFIG_NAME = "vlm.json"


def config_path() -> Path:
    return data_dir() / CONFIG_NAME


def load_config() -> dict:
    try:
        d = json.loads(config_path().read_text(encoding="utf-8"))
        return d if isinstance(d, dict) else {}
    except Exception:
        return {}


def save_config(entry: dict) -> dict:
    config_path().parent.mkdir(parents=True, exist_ok=True)
    tmp = config_path().with_suffix(".json.part")
    tmp.write_text(json.dumps(entry, indent=2), encoding="utf-8")
    tmp.replace(config_path())
    return entry


def clear_config() -> bool:
    """Remove the saved provider config. True when one existed."""
    existed = config_path().exists()
    config_path().unlink(missing_ok=True)
    return existed


def _mask(key):
    if not key:
        return ""
    key = str(key)
    return f"{key[:4]}...{key[-4:]}" if len(key) > 12 else "****"


def _provider_env_key(name):
    p = PROVIDERS.get(name or "")
    return os.environ.get(p["key_env"]) if p else None


def _resolved_key(provider=None, config=None):
    """FUNCUT_VLM_API_KEY beats the saved config, which beats the
    provider's own exported env var (so a machine that already has
    GROQ_API_KEY in the environment just works)."""
    if os.environ.get("FUNCUT_VLM_API_KEY"):
        return os.environ["FUNCUT_VLM_API_KEY"]
    cfg = config if config is not None else load_config()
    if isinstance(cfg, dict) and cfg.get("apiKey"):
        return cfg["apiKey"]
    return _provider_env_key(provider)


def configure(provider=None, endpoint=None, api_key=None, model=None):
    """Save the hosted-critic config. A named provider fills in its base
    URL and default model; `custom` (or no provider) needs an endpoint.
    The key lands in vlm.json inside the app's own data dir and is never
    echoed back in full."""
    if provider and provider not in PROVIDERS and provider != "custom":
        raise RuntimeError(
            f"unknown provider {provider!r}; choose from {list(PROVIDERS)} or 'custom'"
        )
    previous = load_config()
    preset = PROVIDERS.get(provider or "", {})
    base = (endpoint or preset.get("base") or previous.get("endpoint") or "").rstrip("/")
    if not base:
        raise RuntimeError(
            "no endpoint to configure: name a provider or pass an endpoint URL"
        )
    key = api_key or _resolved_key(provider, previous) or ""
    chosen = model or preset.get("model") or previous.get("model") or ""
    entry = {
        "provider": provider or previous.get("provider") or "custom",
        "endpoint": base,
        "apiKey": key,
        "model": chosen,
    }
    save_config(entry)
    return {
        **{k: v for k, v in entry.items() if k != "apiKey"},
        "apiKey": _mask(key),
        "saved": True,
        "privacy": (
            "HOSTED: contact-sheet frames are sent to this provider's API."
            if not _is_local(base)
            else "local endpoint; frames stay on this machine."
        ),
        "note": "craft_critique now scores with the full six-axis model prompt; "
                "re-check the model id with local_vlm {action:'providers'} if scores look odd.",
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


def _http(url: str, timeout: float = 8.0, key: str = None):
    headers = {"User-Agent": "concat-editor-vlm/1.0"}
    if key:
        headers["Authorization"] = f"Bearer {key}"
    req = urllib.request.Request(url, headers=headers)
    return urllib.request.urlopen(req, timeout=timeout)


def _json(url: str, timeout: float = 8.0, key: str = None):
    with _http(url, timeout, key) as resp:
        return json.loads(resp.read().decode("utf-8", "replace"))


def _probe_openai(base: str, key: str = None):
    for path in ("/v1/models", "/models"):
        try:
            d = _json(base.rstrip("/") + path, 2.5, key)
            models = [m.get("id", "") for m in d.get("data", [])]
            return {"type": "openai", "base": base.rstrip("/"), "models": models}
        except Exception:
            continue
    return None


def _is_local(base: str):
    """A loopback endpoint never leaves the machine; everything else is
    hosted and owes the user a privacy note."""
    try:
        host = (urllib.parse.urlparse(base).hostname or "").lower()
    except Exception:
        return False
    return host in ("127.0.0.1", "localhost", "::1", "0.0.0.0") or host.endswith(".local")


def detect(timeout: float = 3.0):
    """Resolve a vision endpoint. Order: FUNCUT_VLM_ENDPOINT, the saved
    hosted-provider config, an already-running local server, Ollama.

    A keyless entry must answer its probe (the old behaviour: a dead
    endpoint falls through to the local candidates). An entry WITH a key
    is trusted even when the probe fails, so `ask()` can surface the
    provider's own error - a 401 from the provider is information a
    silent fall-back to pixels would hide. Hosted entries carry
    `hosted: True`, which sense.py reads as "strong enough for the
    six-axis prompt".
    """
    env = os.environ.get("FUNCUT_VLM_ENDPOINT")
    key = os.environ.get("FUNCUT_VLM_API_KEY")
    model = os.environ.get("FUNCUT_VLM_MODEL") or None
    if env:
        base = env.rstrip("/")
        got = _probe_openai(base, key)
        if got or key:
            models = (got or {}).get("models") or []
            entry = {
                "type": "openai",
                "base": base,
                "hosted": not _is_local(base),
                "verified": bool(got),
                "models": models,
                "model": model or (models[0] if models else "local-model"),
            }
            if key:
                entry["key"] = key
            return entry
        # a dead keyless endpoint keeps the old fall-through behaviour
    cfg = load_config()
    if cfg.get("endpoint"):
        base = str(cfg["endpoint"]).rstrip("/")
        cfg_key = key or cfg.get("apiKey") or _provider_env_key(cfg.get("provider"))
        got = _probe_openai(base, cfg_key)
        if got or cfg_key:
            models = (got or {}).get("models") or []
            entry = {
                "type": "openai",
                "base": base,
                "hosted": not _is_local(base),
                "provider": cfg.get("provider"),
                "verified": bool(got),
                "models": models,
                "model": model or cfg.get("model") or (models[0] if models else "local-model"),
            }
            if cfg_key:
                entry["key"] = cfg_key
            return entry
    candidates = [
        f"http://127.0.0.1:{DEFAULT_PORT}",
        "http://127.0.0.1:8080",
        "http://127.0.0.1:1234",
    ]
    for base in candidates:
        got = _probe_openai(base)
        if got:
            got["model"] = got["models"][0] if got["models"] else "local-model"
            got["hosted"] = False
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
                    "hosted": False,
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


def _sha256(path: Path) -> str:
    """Streaming SHA-256 of a file, as lowercase hex."""
    import hashlib
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def _download(url: str, dest: Path, label: str, progress=None, sha256: str = None):
    """Fetch `url` into `dest`. When `sha256` is pinned, the bytes must
    match it or the file is deleted and the fetch fails: an artifact that
    arrives wrong is never left where a later call would find and use it.
    The stream is also capped, so a mirror that never ends cannot fill
    the disk."""
    if dest.exists() and dest.stat().st_size > 1_000_000:
        if sha256 and _sha256(dest) != sha256:
            raise RuntimeError(
                f"{label} at {dest} is not the pinned file (sha256 mismatch) - "
                f"delete it and run setup again"
            )
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
            if total > MAX_DOWNLOAD_BYTES:
                raise RuntimeError(
                    f"{label} claims {total} bytes; the most this module fetches is "
                    f"{MAX_DOWNLOAD_BYTES}"
                )
            done, chunk = 0, 1 << 20
            last_report = 0.0
            while True:
                buf = resp.read(chunk)
                if not buf:
                    break
                out.write(buf)
                done += len(buf)
                if done > MAX_DOWNLOAD_BYTES:
                    raise RuntimeError(
                        f"{label} passed {MAX_DOWNLOAD_BYTES} bytes without ending"
                    )
                now = time.time()
                if progress and (now - last_report > 1.0 or done == total):
                    last_report = now
                    progress(label, done, total)
        if total and tmp.stat().st_size != total:
            raise RuntimeError(f"incomplete download for {label}: {tmp.stat().st_size}/{total} bytes")
        if sha256:
            actual = _sha256(tmp)
            if actual != sha256:
                raise RuntimeError(
                    f"{label} is not the file it should be: sha256 {actual}, pinned "
                    f"{sha256}. The download was discarded."
                )
        tmp.replace(dest)
    finally:
        tmp.unlink(missing_ok=True)
    return dest


def _safe_members(archive_names):
    """The members an archive may be extracted under a root: no absolute
    paths, no `..`, no drive letters - the zip-slip and tar-traversal
    rules, checked by us rather than left to whichever extraction helper
    a Python version happens to route through."""
    import ntpath
    keep = []
    for name in archive_names:
        pure = name.replace("\\", "/")
        if pure.startswith("/") or ntpath.splitdrive(pure)[0]:
            continue
        parts = [p for p in pure.split("/") if p not in ("", ".")]
        if not parts or ".." in parts:
            continue
        keep.append(name)
    return keep


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
    archive = _download(
        url, data_dir() / asset, "llama.cpp server", progress, LLAMA_SHA256.get(asset)
    )
    root.mkdir(parents=True, exist_ok=True)
    # Extraction takes only members that stay under the root: the archive
    # is digest-pinned now, but a pin checks the bytes it got, and defence
    # in depth costs five lines.
    if archive.suffix == ".zip":
        with zipfile.ZipFile(archive) as z:
            z.extractall(root, members=_safe_members(z.namelist()))
    else:
        with tarfile.open(archive) as t:
            names = [m.name for m in t.getmembers()]
            allowed = set(_safe_members(names))
            # No links out, no devices: files and directories only. The
            # list is ours, so the Python < 3.12 fallback (no `filter`
            # kwarg) is safe to run against it.
            safe = [m for m in t.getmembers() if m.name in allowed and (m.isreg() or m.isdir())]
            try:
                t.extractall(root, members=safe, filter="data")
            except TypeError:  # Python < 3.11.4 has no `filter`
                t.extractall(root, members=safe)
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
    model = _download(
        f"{base}/{p['model']}", target / p["model"], "vision model", progress,
        MODEL_SHA256.get(p["model"]),
    )
    mmproj = _download(
        f"{base}/{p['mmproj']}", target / p["mmproj"], "vision projector", progress,
        MODEL_SHA256.get(p["mmproj"]),
    )
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
    """Send images + prompt to a local or hosted OpenAI-compatible vision
    endpoint. A key on the endpoint entry (or FUNCUT_VLM_API_KEY) is sent
    as a Bearer token, which is what the free-tier hosted providers need.

    `json_schema` asks llama.cpp's server for grammar-constrained JSON. Not
    every OpenAI-compatible server accepts it, so a 400 with a schema set
    retries once without it rather than losing the whole critique.
    """
    ep = endpoint or detect()
    if not ep:
        raise RuntimeError("no local vision endpoint")
    base = ep["base"] if isinstance(ep, dict) else str(ep)
    key = None
    if isinstance(ep, dict) and ep.get("key"):
        key = ep["key"]
    elif os.environ.get("FUNCUT_VLM_API_KEY"):
        key = os.environ["FUNCUT_VLM_API_KEY"]
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
        headers = {"Content-Type": "application/json", "User-Agent": "concat-editor-vlm/1.0"}
        if key:
            headers["Authorization"] = f"Bearer {key}"
        req = urllib.request.Request(url, data=json.dumps(body).encode(), headers=headers)
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
    cfg = load_config()
    out = {
        "available": bool(ep),
        "endpoint": (ep or {}).get("base"),
        "model": (ep or {}).get("model"),
        "type": (ep or {}).get("type"),
        "hosted": (ep or {}).get("hosted", False),
        "provider": (ep or {}).get("provider"),
        "verified": (ep or {}).get("verified", False),
        "config": (
            {
                "provider": cfg.get("provider"),
                "endpoint": cfg.get("endpoint"),
                "model": cfg.get("model"),
                "apiKey": _mask(cfg.get("apiKey")),
            }
            if cfg
            else None
        ),
        "providers": {
            name: {
                "endpoint": p["base"],
                "model": p["model"],
                "keyEnv": p["key_env"],
                "free": p["free"],
                "keys": p["keys"],
            }
            for name, p in PROVIDERS.items()
        },
        "profiles": {k: {"params": v["params"], "mb": v["model_mb"] + v["mmproj_mb"]} for k, v in PROFILES.items()},
        "dir": str(data_dir()),
    }
    if out["hosted"]:
        out["privacy"] = "HOSTED critic active: contact-sheet frames are sent to this provider's API."
    return out


def _progress(label, done, total):
    pct = (100.0 * done / total) if total else 0.0
    print(f"[vlm] {label}: {done/1e6:.0f}/{total/1e6:.0f} MB ({pct:.0f}%)", file=sys.stderr, flush=True)


def selftest():
    """The fragile seams, checked with no network, no model, no download:
    key masking, loopback detection, archive traversal rules, the provider
    table's shape, and a config round-trip in a scratch data dir. The reply
    is the same shape sense.py's and emotion.py's selftests give."""
    import tempfile

    out = {}
    # Key masking: never echo a whole key back.
    assert _mask("") == ""
    assert _mask("short") == "****"
    assert _mask("sk-1234567890abcdef") == "sk-1...cdef"
    out["mask"] = "ok"
    # Loopback detection decides the privacy note every reply carries.
    assert _is_local("http://127.0.0.1:8099") is True
    assert _is_local("http://localhost:1234/v1") is True
    assert _is_local("http://[::1]:8099") is True
    assert _is_local("https://api.groq.com/openai/v1") is False
    out["localProbe"] = "ok"
    # The extraction allow-list: absolute paths, `..`, drive letters and
    # mixed separators must all fall away however they are spelled.
    keep = _safe_members(
        ["bin/llama-server", "/etc/passwd", "../evil", "C:\\x",
         "a/../../b", "./nested/ok", "..\\\\win"]
    )
    assert keep == ["bin/llama-server", "./nested/ok"], keep
    out["archiveGuards"] = "ok"
    # Every provider needs a base, a default model, a key env and a console
    # link, and the platform table needs an asset for this machine.
    for name, provider in PROVIDERS.items():
        assert provider["base"].startswith("http"), name
        assert provider["model"], name
        assert provider["key_env"], name
        assert provider["keys"].startswith("http"), name
    assert _asset_name() is not None
    out["providers"] = "ok"
    # Config round-trip in a scratch dir - the real data dir is never read
    # or written by a selftest.
    previous = os.environ.pop("FUNCUT_VLM_DIR", None)
    try:
        os.environ["FUNCUT_VLM_DIR"] = tempfile.mkdtemp(prefix="vlm-selftest-")
        entry = configure(provider="groq", api_key="sk-test-123456789")
        assert entry["saved"] is True
        assert entry["apiKey"] == "sk-t...6789", entry["apiKey"]  # masked, never whole
        cfg = load_config()
        assert cfg["provider"] == "groq"
        assert cfg["endpoint"].startswith("https://api.groq.com")
        assert cfg["apiKey"] == "sk-test-123456789"
        assert cfg["model"] == PROVIDERS["groq"]["model"]
        assert clear_config() is True
        assert not config_path().exists()
        # A provider-less configure with no endpoint and no history refuses
        # rather than saving an entry that can never be reached.
        try:
            configure()
        except RuntimeError:
            out["config"] = "ok"
        else:
            raise AssertionError("configure() saved an endpoint-less entry")
    finally:
        import shutil

        shutil.rmtree(os.environ["FUNCUT_VLM_DIR"], ignore_errors=True)
        if previous is None:
            os.environ.pop("FUNCUT_VLM_DIR", None)
        else:
            os.environ["FUNCUT_VLM_DIR"] = previous
    out["selftest"] = True
    return out


def main():
    import argparse
    ap = argparse.ArgumentParser()
    ap.add_argument(
        "cmd",
        choices=["status", "setup", "ask", "health", "providers", "configure", "clear", "selftest"],
    )
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
        elif args.cmd == "providers":
            out = {
                "ok": True,
                "providers": {
                    name: {
                        "endpoint": p["base"],
                        "model": p["model"],
                        "keyEnv": p["key_env"],
                        "free": p["free"],
                        "keys": p["keys"],
                    }
                    for name, p in PROVIDERS.items()
                },
            }
        elif args.cmd == "configure":
            spec = json.loads(args.spec)
            out = {
                "ok": True,
                "configured": configure(
                    provider=spec.get("provider"),
                    endpoint=spec.get("endpoint"),
                    api_key=spec.get("apiKey"),
                    model=spec.get("model"),
                ),
            }
        elif args.cmd == "clear":
            out = {"ok": True, "cleared": clear_config()}
        elif args.cmd == "selftest":
            out = {"ok": True, **selftest()}
        else:
            spec = json.loads(args.spec)
            text = ask(spec.get("prompt", ""), spec.get("images") or [], spec.get("endpoint"), spec.get("model"))
            out = {"ok": True, "text": text}
    except Exception as exc:
        out = {"ok": False, "error": f"{type(exc).__name__}: {exc}"}
    print(json.dumps(out, ensure_ascii=False), flush=True)


if __name__ == "__main__":
    main()
