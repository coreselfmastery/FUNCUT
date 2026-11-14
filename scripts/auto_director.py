#!/usr/bin/env python3
"""The auto-director: watch for an imported voiceover, direct the edit.

    scripts/auto_director.py --self-test
    scripts/auto_director.py --project /edits/Reel --plan plan.json
    scripts/auto_director.py --project /edits/Reel --plan plan.json \\
        --audio take.wav                # run once, no watching

STEP 1 - is `media.import` an EVENT or a REQUEST?
--------------------------------------------------
A REQUEST (a method call), not an event. This repository has no
docs/JSON-RPC.md; the JSON-RPC protocol is documented in `src/README.md`
("Driving Concat without the window") and the wire itself is
`src/crates/concat-api/src/message.rs`, where `MediaImport` is a variant of
the **Request** enum (`#[serde(rename = "media.import")]`, taking `path` and
`file`). The **Event** enum beside it carries only `cutout.progress`,
`stabilize.progress`, `export.progress`, `export.done`, `export.failed`,
`analysis.done` and `analysis.failed` - no media event exists, so nothing is
ever pushed when a file lands in a project's bin.

THE LIMITATION (and why this script polls)
------------------------------------------
Because `media.import` is a request and no notification announces it, this
script cannot subscribe to imports. It polls the project's media list
(`project_get`) every 2 seconds and diffs the replies by media id, and a new
item whose kind is `audio` starts the pipeline below. Consequences worth
saying out loud:

- a file imported and removed again between two polls is never seen;
- several files landing inside one poll interval are run in bin order, one
  pipeline at a time, so the second waits for the first;
- the poll is the reaction time: an import answers in at most one interval.

THE PIPELINE (in order; each phase logged; any failure stops the run)
---------------------------------------------------------------------
0.  `concat_version` + the session's tool list: the handshake that says the
    server this plan needs is the server that answered
1.  `local_vlm` status: which critic tier will score the cut, said up front
2.  `enhance_speech` + `remove_hiss_hum_rumble`: the take is repaired before
    anything reads it, and every later phase runs on the repaired file
3.  `transcribe` words and timings from the repaired voiceover
4.  `voice_signals` measured pauses, emphasis and delivery
5.  `emotion` emotional read of the narration
6.  `footage_plan_suggest` dialogue gaps and monotony runs, cross-checked
    against the plan's B-roll coverage
7.  `detect_music_segments` the BGM gate: a take that already carries music
    gets no bed stacked under it (`identify_music` logged for the
    copyright record when it does)
8.  `plan_validate` schema, creative readiness and the signal-driven gate -
    including the runner's own evidence check that the signalLog cites
    signals the take actually measured
9.  `project_get` find or import the repaired voiceover, place it on its own
    lane, apply the plan's `speed`
10. `openvid_rank_for_slot` each timed B-roll segment, with duration checks
11. `stock_contact_sheet` the chosen candidates, seen before import
12. `footage_continuity` check the selected clips; replace flagged shots once
13. `openvid_import` import and place stock at the segment start; trim
14. `clip_update`/`keyframe_set` the plan's transitions and entrances
15. the plan's grade, captions, SFX and BGM, each only where the plan asks
    - captions word-timed and kinetic when the plan asks for them
    (`captions.wordTimed` or `captions.kinetic`): the measured word
    timings bake one reel through `captions_animate`, placed where the
    graphics are; the timeline's segment-timed captions do not double it
16. `collision_check` + `timeline_read` the timeline is sane before export
17. `export_start` + `export_wait` render and wait for a terminal job state
18. `transcribe` + `emotion (verify)` the export says the words it should
19. `measure_loudness` LUFS and true peak, measured and asserted - a plan's
    target is a wish, this is the measurement
20. `watch_render` + `craft_critique` inspect the actual output and score it

The run keeps its own three records beside the plan's: a signalLog of
what it measured and what it could not - the footage planner's
evidence for every B-roll slot, the degradations with their provenance,
fallback or measured - an audit of all nineteen plan fields, executed
or rejected or skipped, with nothing the runner did not consume dropped
silently, and the Verify stage's own report. All three ride the run
history the run writes.

Steps 18-19 are the Verify stage, and the stage keeps its own record
(VERIFY-01..05, 08..14): every check the finished export must answer -
black frames, captions present and on-frame, transcript sync, visual
quality, media availability, duplicate clips on a lane, codec, frame
size, frame rate, the true-peak ceiling, the post-final-edit timeline
hash - each one a structured entry, and the whole record written beside
the export as `verification-report.json` for the acceptance read. A
failed check stops the run naming its ids; a check the environment
cannot run is skipped with its reason written down.

The watcher is an execution layer, not a topic-to-plan author: the Director
agent must supply a validated canonical edit plan. A craft score below 7
runs the revision loop: exactly the slots the critic named go back for
another pass (a B-roll slot re-shops with the old clip excluded, a graphic
re-renders with the critic's timing fixes folded into its storyboard), at
most twice, and the edit then ships with the failures recorded per slot -
this script revises the plan it was given, it never invents a new one.

The calls go through the editor's MCP server over stdio (newline-delimited
JSON-RPC 2.0 - `tools/call`), the same surface the funcut-director skill
drives; `--mcp-command` says how it is started, and defaults to
`concat --mcp auto`.

The server is started with a minimal environment (see `_server_env`): a
caller's PYTHONPATH, LD_LIBRARY_PATH or proxy variables are not the server's
to inherit, and a missing HOME is given one that exists.
"""
from __future__ import annotations

import argparse
import copy
import hashlib
import json
import os
import re
import shlex
import shutil
import subprocess
import sys
import tempfile
import time
from fractions import Fraction
from typing import Any

POLL_SECONDS = 2.0

# The tools this pipeline cannot run without. The handshake fails fast,
# naming every one that is missing, instead of letting the run die three
# phases later inside a call the plan never could have made.
REQUIRED_TOOLS = frozenset({
    "transcribe", "analysis_wait", "voice_signals", "emotion",
    "plan_validate", "project_get", "track_add", "clip_add",
    "openvid_rank_for_slot", "footage_continuity", "openvid_import",
    "media_import", "clip_trim", "export_start", "export_wait",
    "measure_loudness", "watch_render", "craft_critique",
})

# The tools the enrichment phases call when the server has them: an older
# server skips the phase with a logged line, a newer one runs it. A plan
# that asks for one of these (captions, SFX, BGM, a grade, a speed change,
# a transition, an entrance) makes its phase's tool required instead - the
# export must not silently lack what the plan promised.
ENRICHMENT_TOOLS = frozenset({
    "concat_version", "local_vlm", "footage_plan_suggest",
    "stock_contact_sheet", "detect_music_segments", "identify_music",
    "enhance_speech", "remove_hiss_hum_rumble", "sounds_search",
    "sounds_add", "bgm_status", "bgm_search", "bgm_import", "bgm_prepare",
    "clip_speed", "clip_update", "keyframe_set", "captions_add",
    "clip_audio_levels_db", "collision_check", "timeline_read",
    "normalize_loudness", "effect_layer_add",
})

# Calls that may be retried when the failure sounds like the network's
# side of the contract, not the tool's: reads, searches and analyses, and
# the three repair writes (which target a fixed `out` path with overwrite,
# so a retry lands on the same file). Mutating timeline calls are never
# retried - an ambiguous clip_add must stop, not double-place.
RETRYABLE_TOOLS = frozenset({
    "transcribe", "analysis_wait", "project_get", "plan_validate",
    "voice_signals", "emotion", "concat_version", "local_vlm",
    "footage_plan_suggest", "stock_contact_sheet", "detect_music_segments",
    "identify_music", "measure_loudness", "watch_render", "craft_critique",
    "sounds_search", "bgm_status", "bgm_search", "bgm_fetch",
    "openvid_search", "openvid_search_batch", "openvid_rank_for_slot",
    "openvid_get", "openvid_status", "timeline_read", "collision_check",
    "effects_list", "enhance_speech", "remove_hiss_hum_rumble",
    "normalize_loudness",
})

_RETRYABLE_MARKERS = (
    "timeout", "timed out", "unreachable", "connection", "reset",
    "temporarily", "broken pipe", "502", "504", "try again",
)

# FALL-07/08/09, the defaults this runner stands on and the knobs that
# turn them. The transcriber is base.en (the fast English one) unless the
# plan names audio.voiceover.whisperModel or the operator pins
# FUNCUT_WHISPER_MODEL - a multilingual take on the default is exactly the
# wrong-language transcript the reply's languageMismatch flag names. The
# voice model is the server's kokoro-int8 default unless the plan names
# audio.voiceover.model (the int8 quantisation hiss is why the repair
# chain runs and is recorded, DEG-10). The bed is shopped with the plan's
# own query, falling back to its mood when the query is absent.


class PipelineError(RuntimeError):
    """A phase failed: the run stops here, later phases never start."""


def _is_retryable(message: str) -> bool:
    lowered = message.lower()
    return any(marker in lowered for marker in _RETRYABLE_MARKERS)


def log(message: str) -> None:
    """One line per event, stamped, flushed: the run is watchable live."""
    stamp = time.strftime("%Y-%m-%dT%H:%M:%S")
    print(f"[{stamp}] {message}", flush=True)


# ---------------------------------------------------------------- transports


def _server_env() -> dict[str, str]:
    """The environment the MCP server is started with: minimal on purpose.

    A server that inherits the caller's whole environment inherits the
    caller's accidents: a PYTHONPATH that finds different Python packages,
    an LD_LIBRARY_PATH that finds different FFmpeg libraries, proxy
    variables that make downloads fail differently on a machine that never
    needed them. What the server needs is a PATH to find python3 and
    ffmpeg, a HOME that exists (model discovery dies without one), a locale
    that Whisper's language detection can rely on, and every FUNCUT_*
    pin the operator set. FUNCUT_KEEP_ENV=1 keeps the caller's environment
    whole, for the operator who means it.
    """
    if os.environ.get("FUNCUT_KEEP_ENV"):
        return dict(os.environ)
    env: dict[str, str] = {
        "PATH": os.environ.get("PATH", "/usr/local/bin:/usr/bin:/bin"),
        "LANG": "C.UTF-8",
    }
    home = os.environ.get("HOME")
    if home and os.path.isdir(home):
        env["HOME"] = home
    else:
        # A HOME that does not exist is a server that cannot find its
        # models and cannot say why; give it one that exists.
        fallback = os.path.join(tempfile.gettempdir(), "funcut-home")
        os.makedirs(fallback, exist_ok=True)
        env["HOME"] = fallback
        log(f"server HOME {home!r} is unset or missing; using {fallback}")
    for name in ("TZ",):
        if name in os.environ:
            env[name] = os.environ[name]
    for name, value in os.environ.items():
        if name.startswith(("FUNCUT_", "CONCAT_")) or name == "OPENVID_INDEX_PATH":
            env[name] = value
    return env


class McpStdio:
    """The MCP server over stdio: newline-delimited JSON-RPC 2.0."""

    def __init__(self, command: str) -> None:
        self._next_id = 1
        self._child = subprocess.Popen(
            shlex.split(command),
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            text=True,
            bufsize=1,
            env=_server_env(),
        )

    def _send(self, payload: dict[str, Any]) -> None:
        assert self._child.stdin is not None
        self._child.stdin.write(json.dumps(payload) + "\n")
        self._child.stdin.flush()

    def _receive(self, want_id: int) -> dict[str, Any]:
        assert self._child.stdout is not None
        for line in self._child.stdout:
            line = line.strip()
            if not line:
                continue
            try:
                reply = json.loads(line)
            except ValueError:
                log(f"unparseable line from the server: {line[:120]}")
                continue
            if reply.get("id") == want_id:
                return reply
            # A notification (export progress, and friends) between the
            # replies: noted and stepped over, never mistaken for one.
            log(f"notification: {reply.get('method', line[:80])}")
        raise PipelineError("the MCP server closed its stdout")

    def start(self) -> None:
        """The initialize handshake every MCP session opens with."""
        reply_id = self._next_id
        self._next_id += 1
        self._send(
            {
                "jsonrpc": "2.0",
                "id": reply_id,
                "method": "initialize",
                "params": {
                    "protocolVersion": "2025-11-25",
                    "capabilities": {},
                    "clientInfo": {
                        "name": "funcut-auto-director",
                        "version": "1.0",
                    },
                },
            }
        )
        reply = self._receive(reply_id)
        if "error" in reply:
            raise PipelineError(f"initialize refused: {reply['error']}")
        self._send({"jsonrpc": "2.0", "method": "notifications/initialized"})

    def request(self, method: str, params: dict[str, Any]) -> Any:
        """One arbitrary JSON-RPC request, for the surfaces `tools/call`
        does not cover - `tools/list` above all."""
        reply_id = self._next_id
        self._next_id += 1
        self._send({"jsonrpc": "2.0", "id": reply_id, "method": method,
                    "params": params})
        reply = self._receive(reply_id)
        if "error" in reply:
            raise PipelineError(f"{method}: {reply['error'].get('message')}")
        return reply.get("result")

    def available_tools(self) -> frozenset[str]:
        """The names the server actually serves, straight from tools/list.

        The contract this pipeline runs on is the server's own list, not a
        count remembered from a build: a tool that is not served is a
        phase that is skipped with a logged line (or, where the plan needs
        it, a refusal that names it), never a call that fails three
        phases later for a reason the handshake could have said.
        """
        try:
            result = self.request("tools/list", {})
        except PipelineError as error:
            log(f"tools/list refused ({error}); assuming the full surface")
            return frozenset(REQUIRED_TOOLS | ENRICHMENT_TOOLS)
        names: set[str] = set()
        for tool in (result or {}).get("tools", []):
            if isinstance(tool, dict) and isinstance(tool.get("name"), str):
                names.add(tool["name"])
        return frozenset(names)

    def _call_once(self, tool: str, arguments: dict[str, Any]) -> Any:
        reply_id = self._next_id
        self._next_id += 1
        self._send(
            {
                "jsonrpc": "2.0",
                "id": reply_id,
                "method": "tools/call",
                "params": {"name": tool, "arguments": arguments},
            }
        )
        reply = self._receive(reply_id)
        if "error" in reply:
            raise PipelineError(f"{tool}: {reply['error'].get('message')}")
        result = reply.get("result", {})
        if result.get("isError"):
            text = " ".join(
                part.get("text", "") for part in result.get("content", [])
            )
            raise PipelineError(f"{tool}: {text or 'the tool failed'}")
        return _content_json(result)

    def call(self, tool: str, arguments: dict[str, Any]) -> Any:
        """One `tools/call`; a tool error is a failure, not a None.

        A failure that sounds like the network's side of the contract -
        the transport's own timeouts, a fetch the CDN refused - is retried
        twice with a little patience, for the calls where a retry is safe
        to make. Everything else stops the run with the tool's own words.
        """
        attempts = 3 if tool in RETRYABLE_TOOLS else 1
        for attempt in range(attempts):
            try:
                return self._call_once(tool, arguments)
            except PipelineError as error:
                if attempt + 1 >= attempts or not _is_retryable(str(error)):
                    raise
                log(f"{tool}: {error}; retrying ({attempt + 2}/{attempts})")
                time.sleep(2.0 * (attempt + 1))
        raise PipelineError(f"{tool}: the call did not answer")

    def close(self) -> None:
        if self._child.stdin is not None:
            self._child.stdin.close()
        self._child.terminate()
        try:
            self._child.wait(timeout=10)
        except subprocess.TimeoutExpired:
            self._child.kill()


def _content_json(result: dict[str, Any]) -> Any:
    """The reply's first text part, as JSON when it parses, else as text."""
    for part in result.get("content", []):
        if part.get("type") == "text":
            text = part.get("text", "")
            try:
                return json.loads(text)
            except ValueError:
                return text
    return None


class MockMcp:
    """A transport for tests: canned replies, every call recorded."""

    def __init__(self, replies: dict[str, Any], fail_on: str | None = None):
        self.replies = replies
        self.fail_on = fail_on
        self.calls: list[tuple[str, dict[str, Any]]] = []

    def start(self) -> None:
        pass

    def request(self, method: str, params: dict[str, Any]) -> Any:
        if method == "tools/list":
            return {"tools": [{"name": name} for name in self.replies]}
        return None

    def available_tools(self) -> frozenset[str]:
        return frozenset(self.replies)

    def call(self, tool: str, arguments: dict[str, Any]) -> Any:
        self.calls.append((tool, arguments))
        if self.fail_on is not None and tool == self.fail_on:
            raise PipelineError(f"{tool}: the tool failed (as arranged)")
        reply = self.replies.get(tool)
        if callable(reply):
            reply = reply(arguments)
        return reply

    def close(self) -> None:
        pass


# ------------------------------------------------------------------ the plan


def load_plan(path: str) -> dict[str, Any]:
    with open(path, encoding="utf-8") as handle:
        return json.load(handle)


def plan_segments(plan: dict[str, Any]) -> list[dict[str, Any]]:
    """The canonical edit-plan schema stores timed visuals in segments."""
    return [s for s in plan.get("segments", []) if isinstance(s, dict)]


def broll_segments(plan: dict[str, Any]) -> list[tuple[int, dict[str, Any]]]:
    return [
        (index, segment)
        for index, segment in enumerate(plan_segments(plan))
        if isinstance(segment.get("visual"), dict)
        and segment["visual"].get("type") == "broll"
    ]


def motion_segments(plan: dict[str, Any]) -> list[tuple[int, dict[str, Any]]]:
    return [
        (index, segment)
        for index, segment in enumerate(plan_segments(plan))
        if isinstance(segment.get("visual"), dict)
        and segment["visual"].get("type") in {"textCard", "titleCard"}
    ]


def existing_segments(plan: dict[str, Any]) -> list[tuple[int, dict[str, Any]]]:
    return [
        (index, segment)
        for index, segment in enumerate(plan_segments(plan))
        if isinstance(segment.get("visual"), dict)
        and segment["visual"].get("type") == "existing"
    ]


def _walk_dicts(value: Any):
    """Yield nested dictionaries without assuming one EditorView wrapper."""
    if isinstance(value, dict):
        yield value
        for child in value.values():
            yield from _walk_dicts(child)
    elif isinstance(value, list):
        for child in value:
            yield from _walk_dicts(child)


def _clip_for_media(view: Any, media_id: str, start: float) -> dict[str, Any] | None:
    """The clip a placement made, found by its media and where it starts.

    A clip whose start is the placement's own start wins outright - the
    media may sit on the timeline several times, and the one that begins
    exactly here is the one this import just placed. Among the rest, the
    nearest start wins, and equal distances break on the clip id, so the
    answer never depends on the order the view happened to list them in.
    """
    candidates = [
        item for item in _walk_dicts(view)
        if (item.get("mediaId") or item.get("media")) == media_id and isinstance(item.get("id"), str)
        and isinstance(item.get("duration"), (int, float))
        and isinstance(item.get("start"), (int, float))
    ]
    if not candidates:
        return None
    # SRC-10: exact placement, not the nearest - a clip found by proximity
    # is a guess about which import landed where, and a wrong guess
    # places somebody else's media on the beat. Only a clip whose start
    # IS the ask (to the millisecond) answers; none answering is the
    # caller's to handle, loudly.
    exact = [
        item for item in candidates
        if abs(float(item["start"]) - start) <= 0.001
    ]
    if not exact:
        return None
    return min(exact, key=lambda item: str(item.get("id", "")))


def _direction(camera: Any) -> str:
    value = str(camera or "").lower()
    for needle, direction in (
        ("left", "left"), ("right", "right"), ("up", "up"), ("down", "down"),
        ("zoom_in", "zoom_in"), ("zoom-in", "zoom_in"),
        ("zoom_out", "zoom_out"), ("zoom-out", "zoom_out"),
    ):
        if needle in value:
            return direction
    return "static"


def _clip_int_id(value: Any, slot: str) -> int:
    """A clip id as the importer takes it: an integer, or a string of
    digits - never a guess, never a silent skip."""
    if isinstance(value, bool):
        raise PipelineError(f"{slot}: clip id {value!r} is not an integer id")
    if isinstance(value, int):
        return value
    if isinstance(value, str) and value.strip().lstrip("-").isdigit():
        return int(value.strip())
    raise PipelineError(f"{slot}: clip id {value!r} is not an integer id the importer takes")


def _reply_file(reply: Any) -> str | None:
    """The file a repair wrote, wherever the reply carries it."""
    if not isinstance(reply, dict):
        return None
    for key in ("file", "out", "path", "output"):
        value = reply.get(key)
        if isinstance(value, str) and value.strip():
            return value
    return None


def _signal_family(name: str) -> str:
    """A cited signal's family: `pause.class=dead_air` names `pause`."""
    return str(name).split(".")[0].split("[")[0].strip()


_SIGNAL_ALIASES = {
    # The confidence map spells some families the reply's own way.
    "pauses": "pause",
    "sentencetype": "sentence",
    "wordconfidence": "words",
}


def _measured_families(voice: Any) -> set[str]:
    """The signal families the take actually measured, from the
    voice_signals reply's own confidence map: a family the reading
    answered with confidence is a family the plan may cite."""
    families: set[str] = set()
    if not isinstance(voice, dict):
        return families
    per_signal = (voice.get("signalConfidence") or {})
    per_signal = per_signal.get("perSignal", per_signal) if isinstance(per_signal, dict) else []
    if isinstance(per_signal, list):
        for entry in per_signal:
            if not isinstance(entry, dict):
                continue
            if float(entry.get("confidence") or 0.0) > 0.0 and entry.get("name"):
                family = _signal_family(str(entry["name"])).lower()
                families.add(_SIGNAL_ALIASES.get(family, family))
    return families


# ------------------------------------------------------- the plan field audit
#
# The plan's own vocabulary, the nineteen fields this runner knows how to
# read (P1-005): the audit answers for every one of them - executed,
# rejected with its reason, or skipped because the plan does not carry it
# - and names any top-level key it does not know, so a plan is never
# silently half-read: a field the run did not consume is seen as one.

#: field -> what the runner reads off it. The order is the plan's own
#: reading order, top to bottom.
PLAN_FIELDS: dict[str, str] = {
    "aspect": "the frame the plan promises; the render is measured against it",
    "mode": "reel or longform; the shape the plan is cut to",
    "sections": "the acts and their spans; validated whole",
    "slots": "the shopping list the B-roll segments draw their ids from",
    "segments": "the timed beats: placements, trims, spoken lines",
    "segments.visual": "each beat's picture: broll shopping, graphics, existing media",
    "segments.transitionIn": "the transition into each cut",
    "segments.in/out": "the entrance and exit animations",
    "segments.sfx": "the sound effects, shopped and peaked on the beat",
    "segments.captions": "the per-segment caption flags",
    "audio.voiceover": "the take: its repair, its speed, its glossary",
    "audio.bgm": "the music bed: its query or track, its duck",
    "audioClipId": "an existing voiceover clip reused in place",
    "grade": "the look, layered or per-clip",
    "captions": "the words on screen: style, offset, or the kinetic reel",
    "signalLog": "the decisions and the signals that drove them",
    "creativeBrief": "the brief the creative gate and the emotion read answer to",
    "export": "where the render lands and the profile it carries",
    "segments.visual.existing": "the plan's own media: mediaId, file or clipId",
}


def plan_field_audit(
    plan: dict[str, Any], consumed: dict[str, dict[str, Any]]
) -> dict[str, Any]:
    """The audit of every field the runner knows (P1-005): each of the
    nineteen answers for itself - the status the run recorded, or
    `skipped` when the plan does not carry it - and a top-level key the
    registry has never met is named `unrecognized`, never dropped. The
    plan's half-read is the bug this refuses to ship silently."""
    audit: dict[str, Any] = {}
    for field in PLAN_FIELDS:
        recorded = consumed.get(field)
        if recorded is not None:
            audit[field] = recorded
            continue
        key = field.split(".", 1)[0]
        present = key in plan
        if field == "segments.visual.existing":
            present = any(existing_segments(plan))
        elif field == "segments.visual":
            present = bool(plan_segments(plan))
        elif field == "segments.transitionIn":
            present = any(
                isinstance(segment.get("transitionIn"), dict)
                for segment in plan_segments(plan)
            )
        elif field == "segments.in/out":
            present = any(
                isinstance(segment.get(side), dict)
                for segment in plan_segments(plan) for side in ("in", "out")
            )
        elif field == "segments.sfx":
            present = any(
                isinstance(segment.get("sfx"), dict)
                for segment in plan_segments(plan)
            )
        elif field == "segments.captions":
            present = any(segment.get("captions") for segment in plan_segments(plan))
        elif field == "audio.voiceover":
            present = isinstance((plan.get("audio") or {}).get("voiceover"), dict)
        elif field == "audio.bgm":
            present = isinstance((plan.get("audio") or {}).get("bgm"), dict)
        elif field == "audioClipId":
            present = plan.get("audioClipId") is not None
        elif field == "captions":
            present = isinstance(plan.get("captions"), dict)
        elif field == "signalLog":
            present = bool(plan.get("signalLog"))
        elif field == "creativeBrief":
            present = isinstance(plan.get("creativeBrief"), dict)
        elif field == "export":
            present = isinstance(plan.get("export"), dict)
        elif field == "grade":
            present = isinstance(plan.get("grade"), dict)
        elif field == "slots":
            present = isinstance(plan.get("slots"), list) and bool(plan.get("slots"))
        audit[field] = {
            "status": "skipped" if not present else "unconsumed",
            "note": "the plan does not carry it" if not present
            else "the run ended before this field's phase recorded it",
        }
    known_top = {field.split(".", 1)[0] for field in PLAN_FIELDS}
    unknown = sorted(key for key in plan if key not in known_top)
    return {"fields": audit, "unrecognized": unknown}


# --------------------------------------------------------------- the pipeline


def phase_plan(plan: dict[str, Any]) -> list[tuple[str, str]]:
    """Describe the real segment-based execution order for the run log."""
    phases = [
        ("concat_version", "the session's own version, and its tool list"),
        ("local_vlm", "which critic tier will score the cut"),
        ("enhance_speech", "repair the take before anything reads it"),
        ("remove_hiss_hum_rumble", "clean the repaired take's floor"),
        ("transcribe", "start source transcription"),
        ("analysis_wait", "wait for source transcription"),
        ("voice_signals", "voice_signals"),
        ("emotion", "emotion (extract)"),
        ("footage_plan_suggest", "cross-check B-roll coverage against the gaps"),
        ("detect_music_segments", "the BGM gate: does the take carry music"),
        ("plan_validate", "plan_validate (before timeline edits)"),
        ("project_get", "read the project bin and active timeline"),
        ("track_add", "create a dedicated voiceover lane when needed"),
        ("clip_add", "place the voiceover when it is not already on the timeline"),
    ]
    voiceover = (plan.get("audio") or {}).get("voiceover") or {}
    if isinstance(voiceover, dict) and isinstance(voiceover.get("speed"), (int, float)) \
            and float(voiceover["speed"]) != 1.0:
        phases.append(("clip_speed", "apply the plan's voiceover speed"))
    phases.extend(
        ("openvid_rank_for_slot", f"rank B-roll segment {index + 1}")
        for index, _ in broll_segments(plan)
    )
    if broll_segments(plan):
        phases.append(("stock_contact_sheet", "see the chosen B-roll candidates"))
        phases.append(("footage_continuity", "footage_continuity and repair"))
        phases.extend(
            ("openvid_import", f"import and place B-roll segment {index + 1}")
            for index, _ in broll_segments(plan)
        )
        phases.extend(
            ("clip_trim", f"trim B-roll segment {index + 1}")
            for index, _ in broll_segments(plan)
        )
        phases.extend(
            ("clip_update", f"the transition into segment {index + 1}")
            for index, segment in broll_segments(plan)
            if isinstance(segment.get("transitionIn"), dict)
        )
        phases.extend(
            ("keyframe_set", f"the entrance of segment {index + 1}")
            for index, segment in broll_segments(plan)
            if isinstance(segment.get("in"), dict)
        )
    placed_existing = [
        (index, segment) for index, segment in existing_segments(plan)
        if not segment["visual"].get("clipId")
    ]
    if motion_segments(plan) or placed_existing:
        phases.append(("track_add", "create a dedicated top video lane"))
    for index, _ in motion_segments(plan):
        phases.extend([
            ("motion_reel", f"render motion segment {index + 1}"),
            ("media_import", f"import motion segment {index + 1}"),
            ("clip_add", f"place motion segment {index + 1}"),
            ("clip_trim", f"trim motion segment {index + 1}"),
        ])
    for index, segment in placed_existing:
        if segment["visual"].get("file"):
            phases.extend([
                ("media_import", f"import existing source segment {index + 1}"),
                ("project_get", f"read imported existing source {index + 1}"),
            ])
        phases.extend([
            ("clip_add", f"place existing source segment {index + 1}"),
            ("clip_trim", f"trim existing source segment {index + 1}"),
        ])
    if isinstance(plan.get("grade"), dict) and plan["grade"].get("filter"):
        phases.append(("effect_layer_add", "the plan's grade"))
    captions_wanted = isinstance(plan.get("captions"), dict) or any(
        segment.get("captions") for _, segment in enumerate(plan_segments(plan))
    )
    if captions_wanted:
        phases.append(("captions_add", "the plan's captions"))
    for index, segment in enumerate(plan_segments(plan)):
        if isinstance(segment.get("sfx"), dict):
            phases.extend([
                ("sounds_search", f"shop SFX for segment {index + 1}"),
                ("sounds_add", f"place SFX for segment {index + 1}"),
                ("clip_audio_levels_db", f"gain the SFX of segment {index + 1}"),
            ])
    if isinstance((plan.get("audio") or {}).get("bgm"), dict):
        phases.extend([
            ("bgm_status", "the music library's state"),
            ("bgm_search", "shop the music bed"),
            ("bgm_import", "place the music bed"),
            ("clip_audio_levels_db", "duck the bed under the words"),
        ])
    phases.extend([
        ("collision_check", "no two clips share a lane's same seconds"),
        ("timeline_read", "the timeline as it stands, before export"),
        ("export_start", "export"),
        ("export_wait", "wait for completed export"),
        ("transcribe", "start exported speech transcription"),
        ("analysis_wait", "wait for exported speech transcription"),
        ("emotion", "compare source and rendered speech timing"),
        ("timeline_read", "the timeline re-read, as the export left it"),
        ("project_get", "the bin re-read, every clip's source accounted for"),
        ("measure_loudness", "the export's LUFS and true peak, measured"),
        ("watch_render", "inspect rendered frames and audio"),
        ("craft_critique", "score the rendered cut"),
    ])
    return phases


# ----------------------------------------------------------- the verification
#
# The Verify stage's own record: the checks the finished export must
# answer before the run may call itself complete (VERIFY-01..05,
# 08..14; 06 and 07 are the transcript-similarity and loudness gates the
# stage has always run, now folded into the same record). Every check
# emits one structured entry - id, check, status, reason, evidence -
# and the entries are written beside the export as
# verification-report.json, so an acceptance read does not have to
# replay the log to know what was proven. A failed entry stops the run
# naming its ids; a check the environment cannot run is skipped with
# its reason written down, never silently dropped.

#: The report's own name for its shape, the way auto-edit-report names its.
VERIFICATION_REPORT_SCHEMA = "funcut.verification-report/1"
#: The run record's own name for its shape (DATA-04): the handoff a later
#: session or a resume reads knows which record it is holding.
RUN_RECORD_SCHEMA = "funcut.auto-director-run/1"
#: How many frames the black-frame and visual-quality scans sample.
VERIFICATION_SAMPLES = 10
#: A sampled frame whose pixels move less than this (variance, 0-255
#: gray) and whose mean sits under it is a black frame.
VERIFICATION_BLACK = 16.0
#: Ten samples whose brightness spread and per-frame contrast all sit
#: inside this are a still field, not a cut.
VERIFICATION_STILL = 2.0
#: How far a word may start from where the edit placed it (VERIFY-03).
VERIFICATION_SYNC_TOLERANCE_MS = 60
#: The absolute ceiling true peak may touch (VERIFY-13; the loudness
#: gate's own -1 dBTP pairing is asserted beside it, unchanged).
VERIFICATION_TRUE_PEAK_DBFS = 0.0


def _verification_entry(
    check_id: str, check: str, status: str, reason: str | None = None, **evidence: Any
) -> dict[str, Any]:
    """One check's structured record: what ran, what it found, why."""
    entry: dict[str, Any] = {"id": check_id, "check": check, "status": status}
    if reason:
        entry["reason"] = reason
    if evidence:
        entry["evidence"] = evidence
    return entry


def _ffprobe(path: str) -> dict[str, Any] | None:
    """The render's own video stream as ffprobe reads it: codec, size,
    frame rate, duration. None when ffprobe is absent, the file is not
    there, or it does not answer - the caller decides what that means,
    because a probe that never ran must not be read as a pass."""
    if not shutil.which("ffprobe") or not os.path.isfile(path):
        return None
    try:
        result = subprocess.run(
            [
                "ffprobe", "-v", "error", "-select_streams", "v:0",
                "-show_entries", "stream=codec_name,width,height,avg_frame_rate",
                "-show_entries", "format=duration", "-of", "json", path,
            ],
            capture_output=True, text=True, timeout=60, check=True,
        )
        probe = json.loads(result.stdout)
    except (OSError, subprocess.SubprocessError, ValueError):
        return None
    streams = probe.get("streams") if isinstance(probe, dict) else None
    if not isinstance(streams, list) or not streams:
        return None
    stream = streams[0] if isinstance(streams[0], dict) else {}
    rate = stream.get("avg_frame_rate") or "0/1"
    try:
        fps = float(Fraction(str(rate)))
    except (ValueError, ZeroDivisionError):
        fps = None
    try:
        duration = float(probe.get("format", {}).get("duration"))
    except (TypeError, ValueError):
        duration = None
    return {
        "codec": stream.get("codec_name"),
        "width": stream.get("width"),
        "height": stream.get("height"),
        "fps": fps,
        "duration": duration,
    }


def _sample_frames(
    path: str, duration: float | None, samples: int = VERIFICATION_SAMPLES
) -> list[dict[str, Any]] | None:
    """Evenly spaced frames from the render, each measured as a small
    gray raster: its mean (brightness) and its variance (contrast). The
    two verify scans ride on these numbers - a black frame is a frame
    whose mean and variance both sit under the floor; a still render is
    ten samples that never moved. None when ffmpeg is absent or no
    frame could be read - an unmeasured render is never a passed one."""
    if not shutil.which("ffmpeg") or not os.path.isfile(path):
        return None
    if not isinstance(duration, (int, float)) or duration <= 0:
        probe = _ffprobe(path)
        duration = probe.get("duration") if probe else None
    if not isinstance(duration, (int, float)) or duration <= 0:
        return None
    frames: list[dict[str, Any]] = []
    for index in range(samples):
        # Midpoints, not edges: a render that leads or trails with black
        # gets its black counted where it is, not sampled into it by
        # construction.
        at = duration * (index + 0.5) / samples
        try:
            result = subprocess.run(
                [
                    "ffmpeg", "-nostdin", "-v", "error", "-ss", f"{at:.6f}", "-i", path,
                    "-frames:v", "1", "-vf", "scale=64:36", "-f", "rawvideo",
                    "-pix_fmt", "gray", "pipe:1",
                ],
                capture_output=True, timeout=60, check=True,
            )
        except (OSError, subprocess.SubprocessError):
            return None
        pixels = result.stdout
        if len(pixels) < 64 * 36:
            return None
        mean = sum(pixels) / len(pixels)
        variance = sum((value - mean) ** 2 for value in pixels) / len(pixels)
        frames.append({
            "t": round(at, 3), "mean": round(mean, 2),
            "variance": round(variance, 2),
        })
    return frames


def _codec_family(codec: str) -> str:
    """The one word a plan's codec and a probe's codec agree by: h265 is
    hevc's other name, and both spellings mean the same encoder."""
    lowered = str(codec or "").strip().lower()
    return "hevc" if lowered in ("h265", "hevc") else lowered or "unknown"


def _probe_checks(
    rendered: str,
    plan: dict[str, Any],
    export: dict[str, Any],
    probe: dict[str, Any] | None,
    frames: list[dict[str, Any]] | None,
) -> list[dict[str, Any]]:
    """The checks that read the render itself (VERIFY-01, 04, 09, 10,
    11): what the file actually carries, against what the plan promised
    and what a finished cut must look like. A render that cannot be
    probed is skipped with the reason - the export tools already proved
    the file exists; what could not be measured is said, not guessed."""
    entries: list[dict[str, Any]] = []
    if probe is None:
        skipped = _verification_entry(
            "VERIFY-01", "black-frame-scan", "skipped",
            "the render was not probed (ffprobe is absent or did not answer); "
            "the black-frame scan did not run",
        )
        entries.extend([
            skipped,
            _verification_entry(
                "VERIFY-04", "visual-quality", "skipped",
                "the render's frames were not sampled; the visual-quality scan did not run",
            ),
            _verification_entry(
                "VERIFY-09", "codec-match", "skipped",
                "the render's codec was not measured",
            ),
            _verification_entry(
                "VERIFY-10", "resolution-match", "skipped",
                "the render's frame size was not measured",
            ),
            _verification_entry(
                "VERIFY-11", "frame-rate-match", "skipped",
                "the render's frame rate was not measured",
            ),
        ])
        return entries

    # VERIFY-09: the codec the file carries against the one the plan named.
    wanted_codec = _codec_family(export.get("codec") or "h264")
    actual_codec = _codec_family(probe.get("codec"))
    if actual_codec == "unknown":
        entries.append(_verification_entry(
            "VERIFY-09", "codec-match", "skipped",
            "the probe carried no codec name",
            probed=rendered,
        ))
    elif actual_codec != wanted_codec:
        entries.append(_verification_entry(
            "VERIFY-09", "codec-match", "fail",
            f"the render carries {actual_codec} where the plan promised {wanted_codec}",
            planCodec=wanted_codec, renderCodec=actual_codec, probed=rendered,
        ))
    else:
        entries.append(_verification_entry(
            "VERIFY-09", "codec-match", "pass",
            evidence={"codec": actual_codec, "probed": rendered},
        ))

    # VERIFY-10: the frame the plan named (or its aspect) against the
    # frame the render carries.
    wanted_width, wanted_height = export.get("width"), export.get("height")
    actual_width, actual_height = probe.get("width"), probe.get("height")
    if isinstance(wanted_width, int) and isinstance(wanted_height, int):
        if (actual_width, actual_height) != (wanted_width, wanted_height):
            entries.append(_verification_entry(
                "VERIFY-10", "resolution-match", "fail",
                f"the render is {actual_width}x{actual_height} where the plan "
                f"promised {wanted_width}x{wanted_height}",
                planSize=[wanted_width, wanted_height],
                renderSize=[actual_width, actual_height],
                probed=rendered,
            ))
        else:
            entries.append(_verification_entry(
                "VERIFY-10", "resolution-match", "pass",
                evidence={"size": [actual_width, actual_height], "probed": rendered},
            ))
    elif isinstance(actual_width, int) and isinstance(actual_height, int) and actual_height > 0:
        ratios = {"16:9": 16.0 / 9.0, "9:16": 9.0 / 16.0, "1:1": 1.0, "4:5": 4.0 / 5.0}
        aspect = plan.get("aspect")
        if aspect in ratios:
            actual_ratio = actual_width / actual_height
            wanted_ratio = ratios[aspect]
            if abs(actual_ratio - wanted_ratio) / wanted_ratio > 0.02:
                entries.append(_verification_entry(
                    "VERIFY-10", "resolution-match", "fail",
                    f"the render's frame is {actual_width}x{actual_height} "
                    f"({actual_ratio:.3f}), not the {aspect} the plan promised",
                    planAspect=aspect, renderSize=[actual_width, actual_height],
                ))
            else:
                entries.append(_verification_entry(
                    "VERIFY-10", "resolution-match", "pass",
                    evidence={"size": [actual_width, actual_height], "aspect": aspect},
                ))
        else:
            entries.append(_verification_entry(
                "VERIFY-10", "resolution-match", "skipped",
                "the plan names no frame size or aspect to match",
                renderSize=[actual_width, actual_height],
            ))
    else:
        entries.append(_verification_entry(
            "VERIFY-10", "resolution-match", "skipped",
            "the render's frame size was not measured",
        ))

    # VERIFY-11: the frame rate the plan named against the one measured.
    wanted_fps = export.get("fps")
    actual_fps = probe.get("fps")
    if not isinstance(wanted_fps, (int, float)) or wanted_fps <= 0:
        entries.append(_verification_entry(
            "VERIFY-11", "frame-rate-match", "skipped",
            "the plan names no frame rate to match",
            renderFps=actual_fps,
        ))
    elif not isinstance(actual_fps, (int, float)):
        entries.append(_verification_entry(
            "VERIFY-11", "frame-rate-match", "skipped",
            "the render's frame rate was not measured",
        ))
    elif abs(actual_fps - float(wanted_fps)) > max(0.01, float(wanted_fps) * 0.0005):
        entries.append(_verification_entry(
            "VERIFY-11", "frame-rate-match", "fail",
            f"the render carries {actual_fps:g} fps where the plan promised "
            f"{float(wanted_fps):g} fps",
            planFps=float(wanted_fps), renderFps=actual_fps,
        ))
    else:
        entries.append(_verification_entry(
            "VERIFY-11", "frame-rate-match", "pass",
            evidence={"fps": actual_fps},
        ))

    # VERIFY-01 and VERIFY-04: the ten sampled frames, read twice.
    if frames is None:
        entries.extend([
            _verification_entry(
                "VERIFY-01", "black-frame-scan", "skipped",
                "the render's frames were not sampled (ffmpeg is absent or "
                "did not answer); the black-frame scan did not run",
            ),
            _verification_entry(
                "VERIFY-04", "visual-quality", "skipped",
                "the render's frames were not sampled; the visual-quality scan did not run",
            ),
        ])
        return entries
    black = [
        frame for frame in frames
        if frame["mean"] <= VERIFICATION_BLACK and frame["variance"] <= VERIFICATION_BLACK
    ]
    if black:
        entries.append(_verification_entry(
            "VERIFY-01", "black-frame-scan", "fail",
            "the render carries black frames: "
            + ", ".join(f"{frame['t']}s" for frame in black[:3])
            + (" and more" if len(black) > 3 else ""),
            samples=len(frames),
            blackAt=[frame["t"] for frame in black],
        ))
    else:
        entries.append(_verification_entry(
            "VERIFY-01", "black-frame-scan", "pass",
            evidence={"samples": len(frames)},
        ))
    brightness = [frame["mean"] for frame in frames]
    contrast = [max(frame["variance"], 0.0) ** 0.5 for frame in frames]
    spread = max(brightness) - min(brightness) if brightness else 0.0
    mean_contrast = sum(contrast) / len(contrast) if contrast else 0.0
    if spread <= VERIFICATION_STILL and mean_contrast <= VERIFICATION_STILL:
        entries.append(_verification_entry(
            "VERIFY-04", "visual-quality", "fail",
            "the render is a still field: ten samples that never moved and "
            "carried no contrast - the cut the plan describes is not on the screen",
            brightnessSpread=round(spread, 2),
            meanContrast=round(mean_contrast, 2),
        ))
    else:
        entries.append(_verification_entry(
            "VERIFY-04", "visual-quality", "pass",
            evidence={
                "brightnessSpread": round(spread, 2),
                "meanContrast": round(mean_contrast, 2),
            },
        ))
    return entries


def _sync_entry(verification: Any) -> dict[str, Any]:
    """VERIFY-03: the words the export says, against the words the edit
    placed - the gate the stage has always run, one structured entry
    now. matchRate and maxDriftMs are the evidence the tool measured."""
    if isinstance(verification, dict) and verification.get("verdict") == "pass":
        return _verification_entry(
            "VERIFY-03", "transcript-sync", "pass",
            evidence={
                "matchRate": verification.get("matchRate"),
                "maxDriftMs": verification.get("maxDriftMs"),
                "toleranceMs": VERIFICATION_SYNC_TOLERANCE_MS,
            },
        )
    return _verification_entry(
        "VERIFY-03", "transcript-sync", "fail",
        "the exported speech failed transcript/timing verification: "
        + json.dumps(verification, ensure_ascii=False)[:400],
        matchRate=verification.get("matchRate") if isinstance(verification, dict) else None,
        maxDriftMs=verification.get("maxDriftMs") if isinstance(verification, dict) else None,
        toleranceMs=VERIFICATION_SYNC_TOLERANCE_MS,
    )


def _timeline_clips(view: Any) -> list[dict[str, Any]]:
    if isinstance(view, dict) and isinstance(view.get("clips"), list):
        return [clip for clip in view["clips"] if isinstance(clip, dict)]
    return []


def _timeline_hash(view: Any) -> str | None:
    """The timeline's own fingerprint: every clip's placement - id,
    media, lane, start, length - as one canonical string hashed. Two
    reads that agree here agree placement-for-placement; two that do
    not mean something edited the project between them."""
    clips = _timeline_clips(view)
    if not clips:
        return None
    placed = [
        {
            "id": str(clip.get("id")),
            "mediaId": str(clip.get("mediaId")),
            "trackId": str(clip.get("trackId")),
            "start": round(float(clip.get("start") or 0.0), 3),
            "duration": round(float(clip.get("duration") or 0.0), 3),
        }
        for clip in clips
    ]
    placed.sort(key=lambda clip: (clip["id"], clip["start"]))
    canonical = json.dumps(placed, sort_keys=True, separators=(",", ":"))
    return hashlib.sha256(canonical.encode("utf-8")).hexdigest()


def _timeline_checks(
    timeline_then: Any, timeline_now: Any, media_view: Any
) -> list[dict[str, Any]]:
    """The checks that read the project's own state at verify time
    (VERIFY-05, 08, 14): the timeline the export rendered is the
    timeline the run finished with, no two clips are the same source
    stacked on one lane's same seconds, and every clip's media is still
    in the bin - and still on the disk it claims."""
    entries: list[dict[str, Any]] = []
    then_hash = _timeline_hash(timeline_then)
    now_hash = _timeline_hash(timeline_now)
    if timeline_then is None:
        entries.append(_verification_entry(
            "VERIFY-14", "post-final-edit-hash", "skipped",
            "the timeline was never read back before the export; there is "
            "no pre-export state to hash against",
        ))
    elif now_hash is None:
        entries.append(_verification_entry(
            "VERIFY-14", "post-final-edit-hash", "skipped",
            "the timeline re-read carried no clips to hash",
        ))
    elif then_hash != now_hash:
        entries.append(_verification_entry(
            "VERIFY-14", "post-final-edit-hash", "fail",
            "the timeline changed after the export's edit was final: the "
            "project the run verified is not the project the run exported",
            preExportSha=then_hash, atVerifySha=now_hash,
        ))
    else:
        entries.append(_verification_entry(
            "VERIFY-14", "post-final-edit-hash", "pass",
            evidence={"sha256": now_hash},
        ))

    clips = _timeline_clips(timeline_now)
    lanes: dict[tuple[str, str], list[dict[str, Any]]] = {}
    unplaced = 0
    for clip in clips:
        track = clip.get("trackId")
        media = clip.get("mediaId")
        if not isinstance(track, str) or not isinstance(media, str):
            unplaced += 1
            continue
        lanes.setdefault((track, media), []).append(clip)
    duplicates: list[str] = []
    for (track, media), group in sorted(lanes.items()):
        spans = sorted(
            (float(clip.get("start") or 0.0), float(clip.get("start") or 0.0)
             + float(clip.get("duration") or 0.0), str(clip.get("id")))
            for clip in group
        )
        for (start, end, clip_id), later in zip(spans, spans[1:]):
            if later[0] < end - 0.001:
                duplicates.append(
                    f"clips {clip_id} and {later[2]} overlap at {later[0]:.2f}s "
                    f"on lane {track} (source {media})"
                )
    if timeline_now is None:
        entries.append(_verification_entry(
            "VERIFY-08", "duplicate-clip-same-lane", "skipped",
            "the timeline was not re-read; duplicate clips were not checked",
        ))
    elif duplicates:
        entries.append(_verification_entry(
            "VERIFY-08", "duplicate-clip-same-lane", "fail",
            "; ".join(duplicates[:3]) + (" and more" if len(duplicates) > 3 else ""),
            duplicates=duplicates[:10],
        ))
    elif unplaced:
        entries.append(_verification_entry(
            "VERIFY-08", "duplicate-clip-same-lane", "pass",
            evidence={"clips": len(clips), "unlaneChecked": unplaced},
        ))
    else:
        entries.append(_verification_entry(
            "VERIFY-08", "duplicate-clip-same-lane", "pass",
            evidence={"clips": len(clips)},
        ))

    media = media_view.get("media", []) if isinstance(media_view, dict) else []
    media = [item for item in media if isinstance(item, dict)]
    bin_ids = {str(item.get("id")) for item in media if item.get("id") is not None}
    referenced = {
        str(clip.get("mediaId")) for clip in clips if clip.get("mediaId") is not None
    }
    missing_from_bin = sorted(referenced - bin_ids)
    missing_on_disk = [
        str(item.get("path")) for item in media
        if isinstance(item.get("path"), str) and not os.path.isfile(item["path"])
    ]
    if media_view is None:
        entries.append(_verification_entry(
            "VERIFY-05", "media-availability", "skipped",
            "the project's bin was not re-read; media availability was not checked",
        ))
    elif missing_from_bin:
        entries.append(_verification_entry(
            "VERIFY-05", "media-availability", "fail",
            "the timeline references media the bin no longer holds: "
            + ", ".join(missing_from_bin[:5]),
            missingFromBin=missing_from_bin[:10],
        ))
    elif missing_on_disk:
        entries.append(_verification_entry(
            "VERIFY-05", "media-availability", "fail",
            "the bin's media is not on the disk it names: "
            + ", ".join(missing_on_disk[:3]),
            missingOnDisk=missing_on_disk[:10],
        ))
    else:
        entries.append(_verification_entry(
            "VERIFY-05", "media-availability", "pass",
            evidence={"binMedia": len(bin_ids), "referenced": len(referenced)},
        ))
    return entries


def _caption_checks(
    captions_wanted: bool,
    placed_captions: list[dict[str, Any]],
    captions_plan: dict[str, Any] | None,
    duration: float | None,
) -> list[dict[str, Any]]:
    """The caption checks (VERIFY-02, 12): a plan that asks for
    captions gets captions that exist, land inside the render's own
    runtime, and sit inside the frame - a caption the viewer cannot
    see is a promise the export did not keep. The band a caption
    occupies is its anchor (offsetY) plus its line height (fontSize
    as a share of frame height, 0.04 the tool's own default)."""
    entries: list[dict[str, Any]] = []
    if not captions_wanted:
        entries.extend([
            _verification_entry(
                "VERIFY-02", "caption-present", "skipped",
                "the plan asks for no captions",
            ),
            _verification_entry(
                "VERIFY-12", "caption-on-frame", "skipped",
                "the plan asks for no captions",
            ),
        ])
        return entries
    if not placed_captions:
        entries.append(_verification_entry(
            "VERIFY-02", "caption-present", "fail",
            "the plan asks for captions and none were placed",
        ))
    elif isinstance(duration, (int, float)) and duration > 0:
        off_render = [
            caption for caption in placed_captions
            if float(caption.get("end", 0.0)) <= 0.0
            or float(caption.get("start", 0.0)) >= duration
        ]
        if off_render:
            entries.append(_verification_entry(
                "VERIFY-02", "caption-present", "fail",
                "captions land outside the render's own runtime and are "
                "never on screen: "
                + ", ".join(
                    f"[{caption.get('start')}-{caption.get('end')}]"
                    for caption in off_render[:3]
                ),
                placed=len(placed_captions), renderDuration=round(duration, 3),
            ))
        else:
            entries.append(_verification_entry(
                "VERIFY-02", "caption-present", "pass",
                evidence={"placed": len(placed_captions),
                          "renderDuration": round(duration, 3)},
            ))
    else:
        entries.append(_verification_entry(
            "VERIFY-02", "caption-present", "pass",
            evidence={"placed": len(placed_captions)},
        ))

    offset_y = (captions_plan or {}).get("offsetY", 0.35)
    offset_y = float(offset_y) if isinstance(offset_y, (int, float)) else 0.35
    font_size = (captions_plan or {}).get("fontSize", 0.04)
    font_size = float(font_size) if isinstance(font_size, (int, float)) else 0.04
    half_band = 0.7 * font_size
    top, bottom = offset_y - half_band, offset_y + half_band
    if top < 0.0 or bottom > 1.0:
        entries.append(_verification_entry(
            "VERIFY-12", "caption-on-frame", "fail",
            f"the caption band sits at offsetY {offset_y:g} with fontSize "
            f"{font_size:g} - the band [{top:.3f}, {bottom:.3f}] crosses the "
            "frame edge and the viewer cannot see all of the caption",
            offsetY=offset_y, fontSize=font_size,
            band=[round(top, 4), round(bottom, 4)],
        ))
    else:
        entries.append(_verification_entry(
            "VERIFY-12", "caption-on-frame", "pass",
            evidence={"offsetY": offset_y, "fontSize": font_size,
                      "band": [round(top, 4), round(bottom, 4)]},
        ))
    return entries


def _true_peak_entry(true_peak: float | None) -> dict[str, Any]:
    """VERIFY-13: the absolute ceiling - a true peak above 0 dBFS is
    digital clipping, whatever the loudness target allowed. A reply
    that carried no number is said, not guessed."""
    if true_peak is None:
        return _verification_entry(
            "VERIFY-13", "true-peak-ceiling", "skipped",
            "measure_loudness carried no true peak to assert",
        )
    if true_peak > VERIFICATION_TRUE_PEAK_DBFS:
        return _verification_entry(
            "VERIFY-13", "true-peak-ceiling", "fail",
            f"the render's true peak is {true_peak:g} dBTP, over the "
            f"{VERIFICATION_TRUE_PEAK_DBFS:g} dBFS ceiling - the export clips",
            truePeakDbtp=true_peak, ceilingDbfs=VERIFICATION_TRUE_PEAK_DBFS,
        )
    return _verification_entry(
        "VERIFY-13", "true-peak-ceiling", "pass",
        evidence={"truePeakDbtp": true_peak, "ceilingDbfs": VERIFICATION_TRUE_PEAK_DBFS},
    )


def _verification_summary(entries: list[dict[str, Any]]) -> dict[str, Any]:
    counts: dict[str, int] = {"pass": 0, "fail": 0, "skipped": 0}
    for entry in entries:
        status = entry.get("status")
        if status in counts:
            counts[status] += 1
    return {
        "total": len(entries), "passed": counts["pass"],
        "failed": counts["fail"], "skipped": counts["skipped"],
    }


def _verification_report(rendered: str, probed: str, entries: list[dict[str, Any]]) -> dict[str, Any]:
    """The whole record, one JSON object: the schema name, the file the
    checks ran against, every entry, and the summary an acceptance read
    starts from."""
    return {
        "schema": VERIFICATION_REPORT_SCHEMA,
        "at": time.strftime("%Y-%m-%dT%H:%M:%S"),
        "output": rendered,
        "probed": probed,
        "checks": entries,
        "summary": _verification_summary(entries),
    }


def _write_verification_report(project: str, report: dict[str, Any]) -> str | None:
    """The record beside the export, where the acceptance read looks:
    <project>/verification-report.json. A report that cannot be written
    is logged, not fatal - the entries already crossed as log lines."""
    try:
        path = os.path.join(project, "verification-report.json")
        with open(path, "w", encoding="utf-8") as handle:
            handle.write(json.dumps(report, ensure_ascii=False, indent=2) + "\n")
        return path
    except OSError as error:
        log(f"verification report not written ({error})")
        return None


def _verification_failed(entries: list[dict[str, Any]]) -> None:
    """The one refusal the whole record ends on, naming every check that
    failed and why - the run stops here, nothing later runs."""
    failed = [entry for entry in entries if entry.get("status") == "fail"]
    if not failed:
        return
    sentences = "; ".join(
        f"{entry['id']} ({entry['check']}): {entry.get('reason')}" for entry in failed
    )
    raise PipelineError(f"the export failed verification: {sentences}")


# ------------------------------------------------------------- run identity
#
# The idempotency contract (IDEM): a run is resumable and non-duplicating.
# Its durable half is <project>/.auto-director-state.json - the stages a
# run completed, with the evidence each one's reuse must re-verify - and
# its per-run product lives under <project>/runs/<run-id>/ (the export
# and report.json, never over a prior run's). A stage is skipped only on
# its own evidence: the prior record exists, its input hashes still
# match, and what it claims to have placed is still on the timeline.

RUN_STATE_SCHEMA = "funcut.auto-director-state/1"
RUN_STATE_NAME = ".auto-director-state.json"
RUNS_DIR_NAME = "runs"


def _new_run_id() -> str:
    """A run's own name, one per run, down to the second."""
    return time.strftime("%Y%m%dT%H%M%SZ")


def _file_sha256(path: str) -> str | None:
    """A file's own digest, or None when it cannot be read."""
    try:
        digest = hashlib.sha256()
        with open(path, "rb") as handle:
            for block in iter(lambda: handle.read(1 << 16), b""):
                digest.update(block)
        return digest.hexdigest()
    except OSError:
        return None


def _json_sha256(value: Any) -> str:
    """A JSON value's digest over its canonical form."""
    canonical = json.dumps(value, ensure_ascii=False, sort_keys=True)
    return hashlib.sha256(canonical.encode("utf-8")).hexdigest()


def _run_state_path(project: str) -> str:
    return os.path.join(project, RUN_STATE_NAME)


def _load_run_state(project: str) -> dict[str, Any]:
    """The last run's stage record, or an empty record. A state that
    cannot be parsed is a fresh start, never a crash (IDEM)."""
    try:
        with open(_run_state_path(project), encoding="utf-8") as handle:
            state = json.load(handle)
        return state if isinstance(state, dict) else {}
    except (OSError, ValueError):
        return {}


def _write_run_state(project: str, state: dict[str, Any]) -> None:
    """The stage record, written whole each time a stage completes: a
    failed run leaves exactly the stages it finished, which is what the
    next run resumes from."""
    try:
        os.makedirs(project, exist_ok=True)
        path = _run_state_path(project)
        temporary = path + ".tmp"
        with open(temporary, "w", encoding="utf-8") as handle:
            handle.write(json.dumps(state, ensure_ascii=False, indent=2) + "\n")
        os.replace(temporary, path)
    except OSError as error:
        log(f"run state not written ({error}); the run continues, resume will not")


def _run_report_dir(project: str, run_id: str) -> str:
    return os.path.join(project, RUNS_DIR_NAME, run_id)


def _stage_failure_note(project: str) -> dict[str, Any]:
    """What a failure record can say about resume: the run id and the
    stages the state file says completed before the stop."""
    state = _load_run_state(project)
    stages = state.get("stages") if isinstance(state.get("stages"), dict) else {}
    return {
        "runId": state.get("runId"),
        "stagesDone": sorted(stages) if stages else [],
    }


def _scenario_state_clear(project: str) -> None:
    """Stand the run state down for the self-test's scenarios: each is
    its own world, not a resume of the one before it. The resume itself
    is tested where a resume is the point (IDEM)."""
    try:
        os.remove(os.path.join(project, RUN_STATE_NAME))
    except OSError:
        pass


def run_pipeline(
    transport: Any,
    project: str,
    audio: str,
    plan: dict[str, Any],
    audio_media_id: str | None = None,
) -> list[tuple[str, dict[str, Any]]]:
    """Validate, source, repair, place, export and inspect a real segment plan."""
    phases = phase_plan(plan)
    total = len(phases)
    done: list[tuple[str, dict[str, Any]]] = []
    step = 0

    def advance(tool: str, arguments: dict[str, Any]) -> Any:
        nonlocal step
        step += 1
        log(f"step {step}: {tool}")
        reply = transport.call(tool, arguments)
        done.append((tool, arguments))
        return reply

    # The run's own two records (P1-004, P1-005): the signalLog the run
    # keeps - what it measured, what it could not, per slot - and the
    # audit of every plan field it consumed. Both ride the run record;
    # neither is the plan's own signalLog, which is the Director's.
    run_signals: list[dict[str, Any]] = []
    plan_audit: dict[str, dict[str, Any]] = {}

    def _audit(field: str, status: str, note: str) -> None:
        """One field's answer, recorded where the field is consumed: the
        status the run gave it and the sentence that says why. A field
        the run refused or degraded records `rejected` here - a logged
        absence is still an answer, never a silent drop (P1-005)."""
        plan_audit[field] = {"status": status, "note": note}

    def _signal(at: float, decision: str, **entry: Any) -> None:
        """One signalLog entry in the run's own log: the decision, the
        signals or degradation behind it, the provenance - measured when
        a reading answered, fallback when one did not (P1-004)."""
        record = {"at": round(float(at), 3), "decision": decision}
        record.update(entry)
        run_signals.append(record)

    def record_fallback(
        stage: str, decision: str, signal_name: str, note: str,
        confidence: float | None = None,
    ) -> None:
        """One degradation in the same run signalLog the planner evidence
        rides (DEG): which stage fell back, which signal it fell back
        from, when both halves happened, and provenance=fallback - the
        shape the plan's signalLog entries will be held to. The console
        hears it as it happens; the run record carries it with the rest."""
        run_signals.append({
            "decision": decision,
            "stage": stage,
            "signalName": signal_name,
            "signalTs": time.strftime("%Y-%m-%dT%H:%M:%S"),
            "decisionTs": time.strftime("%Y-%m-%dT%H:%M:%S"),
            "confidence": confidence,
            "provenance": "fallback",
            "note": note,
        })
        log(f"degraded: {stage}/{decision} <- {signal_name}: {note}")

    # ---- The run's own identity and resume record (IDEM). The run id
    # names the run-scoped directory its export and report land under;
    # the state is the durable half of idempotency - what a prior run
    # completed, reused only when its own evidence still stands. A
    # different take or a different plan is a different edit: the stage
    # record starts over rather than resuming half of the wrong run.
    run_id = _new_run_id()
    source_audio_sha = _file_sha256(audio) or ""
    plan_sha = _json_sha256(plan)
    _prior_state = _load_run_state(project)
    _prior_stages: dict[str, Any] = {}
    if (
        isinstance(_prior_state, dict)
        and _prior_state.get("audioSha256") == source_audio_sha
        and _prior_state.get("planSha256") == plan_sha
    ):
        stages_read = _prior_state.get("stages")
        if isinstance(stages_read, dict):
            _prior_stages = stages_read
    state_stages: dict[str, Any] = {}
    skipped_stages: list[str] = []

    def _state_write() -> None:
        _write_run_state(project, {
            "schemaVersion": RUN_STATE_SCHEMA,
            "runId": run_id,
            "audioSha256": source_audio_sha,
            "planSha256": plan_sha,
            "stages": state_stages,
        })

    def _stage_mark(stage: str, **record: Any) -> None:
        """A stage completed: its evidence recorded, the state written
        before the run goes on - a crash one stage later still resumes
        from here."""
        state_stages[stage] = record
        _state_write()

    def _stage_prior(stage: str) -> dict[str, Any] | None:
        record = _prior_stages.get(stage)
        return record if isinstance(record, dict) else None

    def _stage_skip(stage: str, why: str) -> None:
        skipped_stages.append(stage)
        log(f"skip {stage}: {why} (reused from a prior run)")

    # ---- 0. The handshake: the server says what it is and what it serves.
    served = transport.available_tools()
    missing = REQUIRED_TOOLS - served
    if missing:
        raise PipelineError(
            "the server does not serve tools this pipeline cannot run "
            f"without: {', '.join(sorted(missing))}"
        )
    if "concat_version" in served:
        version = advance("concat_version", {})
        if isinstance(version, dict):
            log(f"server: {json.dumps(version, ensure_ascii=False)[:200]}")

    # ---- 1. Which critic will score the cut, said before the cut exists.
    if "local_vlm" in served:
        vlm = advance("local_vlm", {"action": "status"})
        if isinstance(vlm, dict):
            tier = vlm.get("tier") or vlm.get("status") or vlm.get("state")
            log(f"critic tier: {tier} (ranking and critique lean on this)")

    # ---- 2. The take is repaired before anything reads it: every later
    # reading, and the timeline's own voiceover, run on the repaired file.
    # The original path is remembered: a take the project's own audio/
    # folder produced is a synthesized one, and the default voice model
    # is the int8 Kokoro (DEG-10).
    source_audio = audio
    repaired = audio
    hiss_cleaned = False
    # IDEM: a repair this same take already received is reused when the
    # repaired file still stands on disk - the repair is a function of
    # the take, and the same take needs the same repair, not a second
    # one layered over the first.
    _repair_prior = _stage_prior("audio-repair")
    if (
        isinstance(_repair_prior, dict)
        and _repair_prior.get("sourceSha256") == source_audio_sha
        and isinstance(_repair_prior.get("file"), str)
        and os.path.isfile(_repair_prior["file"])
    ):
        repaired = _repair_prior["file"]
        hiss_cleaned = bool(_repair_prior.get("hissCleaned"))
        _stage_mark(
            "audio-repair", sourceSha256=source_audio_sha,
            file=repaired, hissCleaned=hiss_cleaned,
        )
        _stage_skip(
            "audio-repair",
            f"the repaired take {_repair_prior['file']} is still on disk",
        )
    else:
        if "enhance_speech" in served:
            enhanced_out = os.path.join(project, "voiceover-enhanced.wav")
            reply = advance("enhance_speech", {
                "audio": audio, "out": enhanced_out, "overwrite": True,
            })
            enhanced_file = _reply_file(reply)
            if enhanced_file:
                repaired = enhanced_file
                if isinstance(reply, dict):
                    measured = {
                        key: reply[key] for key in
                        ("noiseFloorDbfs", "snrDb", "integratedLufs", "before", "after")
                        if reply.get(key) is not None
                    }
                    if measured:
                        log(f"enhance_speech: {json.dumps(measured, ensure_ascii=False)}")
            else:
                log("enhance_speech returned no file; analysis continues on the take as recorded")
        if "remove_hiss_hum_rumble" in served and repaired:
            cleaned_out = os.path.join(project, "voiceover-cleaned.wav")
            reply = advance("remove_hiss_hum_rumble", {
                "audio": repaired, "out": cleaned_out, "overwrite": True,
            })
            cleaned_file = _reply_file(reply)
            if cleaned_file:
                repaired = cleaned_file
                hiss_cleaned = True
            else:
                log("remove_hiss_hum_rumble returned no file; the enhanced take ships as cleaned")
        _stage_mark(
            "audio-repair", sourceSha256=source_audio_sha,
            file=repaired, hissCleaned=hiss_cleaned,
        )
    if repaired != audio:
        log(f"voiceover repaired: {repaired} (the bin's original stays untouched)")
    audio = repaired

    # DEG-10: the int8 Kokoro is the server's default voice, and its
    # quantisation adds a hiss that sits above the noise floor #111's
    # window fix stopped the classifier flagging - the cure is still owed,
    # and its use must be recorded, not implied. A take the project's own
    # audio/ folder produced is synthesized; a plan that names a model says
    # which; a plan that names none got the int8 default.
    voiceover_plan = (plan.get("audio") or {}).get("voiceover") or {}
    voiceover_model = ""
    if isinstance(voiceover_plan, dict):
        named = voiceover_plan.get("model")
        if isinstance(named, str) and named.strip():
            voiceover_model = named.strip()
    synthesized = os.path.normpath(source_audio).startswith(
        os.path.join(os.path.normpath(project), "audio") + os.sep
    )
    int8_take = "int8" in voiceover_model.lower() or (
        not voiceover_model and synthesized
    )
    if int8_take:
        if hiss_cleaned:
            record_fallback(
                "voiceover-repair", "denoise", "remove_hiss_hum_rumble",
                "Kokoro int8 take"
                + (f" ({voiceover_model})" if voiceover_model else " (the server's default model)")
                + ": quantisation hiss expected above the floor; "
                "remove_hiss_hum_rumble ran on the take",
            )
        elif "remove_hiss_hum_rumble" in served:
            record_fallback(
                "voiceover-repair", "denoise", "remove_hiss_hum_rumble",
                "Kokoro int8 take, but remove_hiss_hum_rumble returned no "
                "file: the take ships with whatever hiss it carries",
            )
        else:
            record_fallback(
                "voiceover-repair", "denoise", "remove_hiss_hum_rumble",
                "Kokoro int8 take and the server does not serve "
                "remove_hiss_hum_rumble: the take ships with its "
                "quantisation hiss, uncleaned",
            )

    # ---- 3-5. The readings, on the repaired take.
    transcribe_args: dict[str, Any] = {"path": audio}
    # IDEM: the transcript is a function of the take's bytes AND the
    # recogniser's ask - the same repaired file under the same ask needs
    # its words read once, not again per run; a different ask (model,
    # glossary) is a different reading. The reply is recorded whole, and
    # reuse re-derives every check the fresh path makes (DEG-09's
    # mismatch flag included) from the recorded reply.
    _transcribe_prior = _stage_prior("transcribe")
    _transcribe_sha = _file_sha256(audio) or ""
    _transcribe_ask: dict[str, Any] = {}
    if isinstance(voiceover_plan, dict):
        _named_model = voiceover_plan.get("whisperModel")
        if isinstance(_named_model, str) and _named_model.strip():
            _transcribe_ask["model"] = _named_model.strip()
    _env_model = os.environ.get("FUNCUT_WHISPER_MODEL")
    if _env_model:
        _transcribe_ask["envModel"] = _env_model
    _glossary_ask = ((plan.get("audio") or {}).get("voiceover") or {}).get("glossary")
    if isinstance(_glossary_ask, str) and _glossary_ask.strip():
        _transcribe_ask["glossary"] = _glossary_ask.strip()[:200]
    if (
        isinstance(_transcribe_prior, dict)
        and _transcribe_prior.get("audioSha256") == _transcribe_sha
        and _transcribe_prior.get("ask") == _transcribe_ask
        and isinstance(_transcribe_prior.get("transcript"), dict)
        and isinstance(_transcribe_prior["transcript"].get("segments"), list)
    ):
        transcript = _transcribe_prior["transcript"]
        _stage_mark(
            "transcribe", audioSha256=_transcribe_sha,
            ask=_transcribe_ask, transcript=transcript,
        )
        _stage_skip("transcribe", "the recorded transcript is this take's words")
    else:
        # FALL-07: the recogniser is a knob, not a constant - the plan names a
        # model, the operator pins one, and only when neither speaks does the
        # server's base.en default answer (fast, English-only; the wrong
        # choice for any other language, which DEG-09's flag will name).
        whisper_model = None
        if isinstance(voiceover_plan, dict):
            named_model = voiceover_plan.get("whisperModel")
            if isinstance(named_model, str) and named_model.strip():
                whisper_model = named_model.strip()
        whisper_model = whisper_model or os.environ.get("FUNCUT_WHISPER_MODEL") or None
        if whisper_model:
            transcribe_args["model"] = whisper_model
            log(f"transcriber: {whisper_model} (named; base.en is the server's default)")
        glossary = ((plan.get("audio") or {}).get("voiceover") or {}).get("glossary")
        if isinstance(glossary, str) and glossary.strip():
            # The spelling the whole run keeps: the proper nouns and jargon
            # the recogniser mishears, named before the first window.
            transcribe_args["initialPrompt"] = glossary.strip()[:200]
        source_job = advance("transcribe", transcribe_args)
        source_job_id = source_job.get("job") if isinstance(source_job, dict) else None
        if not source_job_id:
            raise PipelineError(f"transcribe did not start a source job: {source_job}")
        source_status: Any = None
        for _ in range(4):
            source_status = advance(
                "analysis_wait", {"job": source_job_id, "timeoutSeconds": 590}
            )
            if not isinstance(source_status, dict) or source_status.get("state") != "running":
                break
        if not isinstance(source_status, dict) or source_status.get("state") != "done":
            raise PipelineError(f"source transcription did not finish: {source_status}")
        try:
            transcript = json.loads(source_status.get("output", ""))
        except (TypeError, json.JSONDecodeError) as error:
            raise PipelineError(f"source transcription returned invalid JSON: {error}") from error
        if not isinstance(transcript, dict) or not isinstance(transcript.get("segments"), list):
            raise PipelineError("source transcription returned no timed segments")
        _stage_mark(
            "transcribe", audioSha256=_transcribe_sha,
            ask=_transcribe_ask, transcript=transcript,
        )
    # DEG-09: the transcribe reply carries what ran and what script the
    # words came back in; a wrong-language transcript is flagged here, at
    # the reading, not discovered three phases later in the captions.
    if transcript.get("languageMismatch") is True:
        record_fallback(
            "transcribe", "transcript", "transcribe.language",
            "the transcription's script disagrees with the ask: model "
            f"{transcript.get('model') or 'unknown'}, script "
            f"{transcript.get('scriptDetected') or 'unknown'}, hint "
            f"{transcript.get('language') or 'none'} - the words may not "
            "be the take's words",
        )
    # IDEM: the voice reading is a function of the same take's bytes -
    # its record is reused whole, and the degradation checks below the
    # call re-run on the recorded reply (DEG-01's flags included).
    _signals_prior = _stage_prior("voice-signals")
    if (
        isinstance(_signals_prior, dict)
        and _signals_prior.get("audioSha256") == _transcribe_sha
        and isinstance(_signals_prior.get("signals"), dict)
    ):
        voice = _signals_prior["signals"]
        _stage_mark(
            "voice-signals", audioSha256=_transcribe_sha, signals=voice,
        )
        _stage_skip("voice-signals", "the recorded reading is this take's reading")
    else:
        voice = advance("voice_signals", {"audio": audio, "transcript": transcript})
        _stage_mark(
            "voice-signals", audioSha256=_transcribe_sha, signals=voice,
        )
    if isinstance(voice, dict):
        if voice.get("status") == "degraded":
            for note in voice.get("degradations") or []:
                log(f"voice_signals degraded: {note}")
                record_fallback("sense", "reading", "voice_signals", str(note))
        elif voice.get("status"):
            log("voice_signals: every ear that could run did")
        # DEG-01: "no entities found" and "the NER missing" are different
        # facts; the reply's flag says which, and a missing NER is a
        # recorded fallback, never a quiet empty list.
        if voice.get("entitiesAvailable") is False:
            record_fallback(
                "sense", "entities", "entities",
                "voice_signals NER absent: entity-driven decisions stand "
                "on the regex complement alone",
            )
        language_read = voice.get("language")
        if (
            isinstance(language_read, dict)
            and language_read.get("matchesHint") is False
        ):
            record_fallback(
                "sense", "language", "voice_signals.language",
                "the voice reading's script "
                f"({language_read.get('script')}) disagrees with the hint "
                f"({language_read.get('hint')})",
            )
    emotion = advance(
        "emotion", {"action": "extract", "transcript": transcript, "project": project}
    )
    if isinstance(emotion, dict) and emotion.get("arc") is not None:
        log(f"voice emotion arc: {json.dumps(emotion['arc'], ensure_ascii=False)}")
        # FALL-06: the emotion read on the English-only stem lexicon is a
        # recorded fallback - the hosted multilingual model is absent, and
        # an arc over another language's words reads as neutral line after
        # line. The reply's own note carries the remedy (a free key).
        if emotion.get("engine") == "lexicon":
            record_fallback(
                "emotion", "arc", "emotion.model",
                "the emotion read ran on the English-only stem lexicon (the "
                "hosted multilingual model is absent; FUNCUT_EMOTION_API_KEY "
                "is the free key that unlocks it)",
            )

    # ---- 6. The footage planner's own read: the gaps and monotony runs
    # the plan's B-roll should answer, cross-checked against its coverage
    # - and recorded per slot, in the run's own signalLog: every B-roll
    # slot carries the planner's evidence for its span, or the
    # degradation of not having one (P1-004). A slot the planner's read
    # says nothing about is a slot the plan cut on its own authority -
    # said plainly, not lost in a log line.
    planner_spans: list[tuple[float, float, str]] = []
    if "footage_plan_suggest" in served:
        footage_plan = advance("footage_plan_suggest", {
            "transcript": {"segments": transcript.get("segments", [])},
        })
        if isinstance(footage_plan, dict):
            gaps = footage_plan.get("gaps") or []
            suggestions = footage_plan.get("suggestions") or []
            log(
                f"footage plan: {len(gaps)} gap(s), {len(suggestions)} span(s) "
                f"wanting B-roll"
            )
            for gap in gaps:
                if not isinstance(gap, dict):
                    continue
                start = gap.get("startMs")
                end = gap.get("endMs")
                if not isinstance(start, (int, float)) or not isinstance(end, (int, float)):
                    continue
                planner_spans.append((start / 1000.0, end / 1000.0, "dialogue_gap"))
            for suggestion in suggestions:
                if not isinstance(suggestion, dict):
                    continue
                start = suggestion.get("startS")
                end = suggestion.get("endS")
                kind = str(suggestion.get("kind") or "")
                if not isinstance(start, (int, float)) or not isinstance(end, (int, float)):
                    continue
                planner_spans.append((float(start), float(end), kind or "suggestion"))
            covered = [
                (float(segment["start"]), float(segment["end"]))
                for _, segment in broll_segments(plan)
            ]
            for gap in gaps:
                if not isinstance(gap, dict):
                    continue
                start = gap.get("startMs")
                end = gap.get("endMs")
                if not isinstance(start, (int, float)) or not isinstance(end, (int, float)):
                    continue
                if (end - start) < 2000:
                    continue
                answered = any(
                    b_start <= start / 1000.0 + 0.25 and b_end >= end / 1000.0 - 0.25
                    for b_start, b_end in covered
                )
                if not answered:
                    log(
                        f"footage plan: a {int(end - start)} ms gap at "
                        f"{start / 1000.0:.2f}s carries no B-roll segment"
                    )
        else:
            log("footage plan: the planner's reply was not a reading; coverage is unmeasured")
    else:
        log("footage plan: the server does not serve footage_plan_suggest; coverage is unmeasured")

    # The per-slot record: what the planner's read says about each
    # B-roll slot's own seconds (P1-004). A span the planner asked to
    # cover is evidence the cut is signal-driven; a span it said nothing
    # about is the plan's own choice, named as one; a planner that never
    # answered is a degradation every slot carries.
    for index, segment in broll_segments(plan):
        slot_id = str(segment.get("slotId") or f"segment-{index + 1}")
        slot_start = float(segment["start"])
        slot_end = float(segment["end"])
        evidence = sorted({
            kind for start, end, kind in planner_spans
            if start < slot_end + 0.25 and end > slot_start - 0.25
        })
        if not planner_spans and "footage_plan_suggest" not in served:
            _signal(
                slot_start, "broll-coverage", slotId=slot_id,
                signals=[], provenance="fallback",
                note="the footage planner was not served; this slot's coverage is unmeasured",
            )
        elif not planner_spans:
            _signal(
                slot_start, "broll-coverage", slotId=slot_id,
                signals=[], provenance="fallback",
                note="the footage planner did not answer; this slot's coverage is unmeasured",
            )
        elif evidence:
            _signal(
                slot_start, "broll-coverage", slotId=slot_id,
                signals=evidence, provenance="measured",
                note="the planner's read asked for this span",
            )
        else:
            _signal(
                slot_start, "broll-coverage", slotId=slot_id,
                signals=[], provenance="measured",
                note="the planner's read named no gap or run inside this span; the plan cut it on its own authority",
            )
    _audit(
        "segments.visual", "executed",
        f"{len(broll_segments(plan))} B-roll segment(s) shopped with the planner's per-slot record",
    )
    _audit(
        "slots", "executed" if isinstance(plan.get("slots"), list) and plan.get("slots")
        else "skipped",
        "the shopping list the B-roll segments draw their ids from"
        if isinstance(plan.get("slots"), list) and plan.get("slots")
        else "the plan does not carry it",
    )

    # ---- 7. The BGM gate, before any bed is shopped: a take that already
    # carries music gets no bed stacked under it. detect_music_segments is
    # the segment map; identify_music is the authority (FALL-10) - where
    # both readings exist, the identifier's own pulse measurement settles
    # the disagreement, and BOTH readings are recorded. Where an ear cannot
    # speak, the gate says so as a recorded fallback, never as a quiet
    # negative (DEG-05).
    bgm_allowed = True
    if "detect_music_segments" not in served:
        record_fallback(
            "music-gate", "bgm", "detect_music_segments",
            "detect_music_segments is not served: the music gate stands "
            "unmeasured, and a bed may stack under take music",
        )
    else:
        music = advance("detect_music_segments", {"audio": audio})
        present = music.get("musicPresent") if isinstance(music, dict) else None
        if present is None:
            # The tool answered without a verdict: a degradation with the
            # reply's own words, not a silent "must be speech".
            record_fallback(
                "music-gate", "bgm", "detect_music_segments",
                "detect_music_segments answered without a verdict"
                + (f" ({json.dumps(music, ensure_ascii=False)[:160]})"
                   if isinstance(music, dict) else "")
                + "; the take is treated as speech on that absence of "
                "evidence, and the bed decision stands unmeasured",
            )
        identified: Any = None
        identified_present: bool | None = None
        if "identify_music" in served:
            # DEG-05 + FALL-10: the identification runs whenever it can,
            # not only when the gate already said music - its pulse
            # measurement is the authority the gate's verdict is settled
            # by, and a failure is a recorded absence, never a "no music"
            # reading and never a dead run.
            try:
                identified = advance("identify_music", {"audio": audio})
            except PipelineError as error:
                record_fallback(
                    "music-gate", "identify", "identify_music",
                    f"identify_music failed: {error} - this is a missing "
                    "identification, not a negative one; the gate's own "
                    "reading stands alone",
                )
            else:
                if isinstance(identified, dict) and identified.get("ok") is not False:
                    log(
                        "identify_music: "
                        + json.dumps(identified, ensure_ascii=False)[:300]
                    )
                    strength = (identified.get("tempo") or {}).get("strength")
                    if isinstance(strength, (int, float)) and not isinstance(strength, bool):
                        # The same pulse line detect_music_segments itself
                        # uses: music holds a beat the autocorrelation can
                        # find (>= 0.1); dense speech does not.
                        identified_present = float(strength) >= 0.1
        if identified_present is not None:
            # FALL-10: both readings recorded; the identifier decides.
            _signal(0.0, "bgm", provenance="measured",
                    detectMusic=present is True, identifyMusic=identified_present,
                    note="both music readings recorded; identify_music is authoritative")
            if identified_present != (present is True):
                bgm_allowed = not identified_present
                record_fallback(
                    "music-gate", "bgm", "identify_music",
                    "the readings disagree - detect_music_segments said "
                    f"{'music' if present is True else 'no music'}, "
                    "identify_music's pulse said "
                    f"{'music' if identified_present else 'no music'} - "
                    "identify_music is authoritative",
                )
                log("music gate: the readings disagree; identify_music's pulse decides")
            elif present is True:
                bgm_allowed = False
                log("music gate: the take already carries music - no bed will be stacked")
        elif present is True:
            bgm_allowed = False
            log("music gate: the take already carries music - no bed will be stacked")

    # ---- 8. Gate all plan and voice-signal readiness before changing the
    # timeline - and prove the log's citations against the measurement.
    # IDEM: the gate is a function of the plan and the measured voice -
    # both reused unchanged, the gate's answer is reused with them.
    _plan_prior = _stage_prior("plan")
    if (
        isinstance(_plan_prior, dict)
        and _plan_prior.get("planSha256") == plan_sha
        and _plan_prior.get("signalsSha256") == _json_sha256(voice)
        and isinstance(_plan_prior.get("validation"), dict)
    ):
        validation = _plan_prior["validation"]
        _stage_mark(
            "plan", planSha256=plan_sha,
            signalsSha256=_json_sha256(voice), validation=validation,
        )
        _stage_skip("plan", "the plan's gate answer is this plan's, on this voice")
    else:
        validation = advance("plan_validate", {"plan": plan})
        _stage_mark(
            "plan", planSha256=plan_sha,
            signalsSha256=_json_sha256(voice), validation=validation
            if isinstance(validation, dict) else {},
        )
    if not isinstance(validation, dict) or validation.get("valid") is not True:
        raise PipelineError(
            "plan_validate rejected the plan: "
            + json.dumps(validation, ensure_ascii=False)
        )
    if validation.get("creativeReady") is not True:
        raise PipelineError(
            "plan is not creatively ready: "
            + json.dumps(validation.get("creativeBlockers", []), ensure_ascii=False)
        )
    signal_log = validation.get("signalLog")
    if isinstance(signal_log, dict) and signal_log.get("driven") is False:
        raise PipelineError(
            "the plan's signalLog is not driven by the measured voice: "
            + json.dumps(signal_log.get("blockers", signal_log.get("problems", [])), ensure_ascii=False)
        )
    if isinstance(signal_log, dict) and len(plan_segments(plan)) > 1:
        coverage = signal_log.get("coverage")
        if isinstance(coverage, (int, float)) and float(coverage) < 0.5:
            raise PipelineError(
                f"the plan's signalLog covers {float(coverage):.0%} of its cuts; "
                "a log that thin is a decoration, not a decision record"
            )
    cited: set[str] = set()
    for entry in plan.get("signalLog") or []:
        if isinstance(entry, dict):
            for name in entry.get("signals") or []:
                if isinstance(name, str):
                    cited.add(_signal_family(name).lower())
    measured = _measured_families(voice)
    if cited and measured and not (cited & measured):
        raise PipelineError(
            "the plan's signalLog cites " + ", ".join(sorted(cited))
            + ", but the measured take answered none of them: "
            "the log was written against a reading that never happened"
        )
    _audit("signalLog", "executed",
           "validated: driven, covered, and every citation answered by the measured take")
    _audit("creativeBrief", "executed",
           "validated: the creative gate answered ready against it")
    _audit("aspect", "executed", "validated: the frame the plan promises")
    _audit("mode", "executed", "validated: the shape the plan is cut to")
    _audit("sections", "executed", "validated: the acts and their spans")
    _audit("segments", "executed",
           f"{len(plan_segments(plan))} timed segment(s) placed, trimmed and read")

    # Fail on incomplete visual contracts before the first timeline edit.
    for index, segment in enumerate(plan_segments(plan)):
        visual = segment.get("visual")
        if not isinstance(visual, dict):
            raise PipelineError(f"segment-{index + 1}: visual must be an object")
        kind = visual.get("type")
        if kind == "broll":
            query = str(visual.get("query") or "").strip()
            description = str(
                visual.get("description") or segment.get("spoken") or ""
            ).strip()
            if not query or len(description) < 40:
                raise PipelineError(
                    f"segment-{index + 1}: B-roll needs a query and a 40+ character semantic description"
                )
        elif kind in {"textCard", "titleCard"}:
            board = visual.get("storyboard")
            if not isinstance(board, dict) or not isinstance(board.get("scenes"), list) or not board["scenes"]:
                raise PipelineError(
                    f"segment-{index + 1}: {kind} needs a complete visual.storyboard"
                )
        elif kind == "existing":
            if not any(visual.get(key) for key in ("clipId", "mediaId", "file")):
                raise PipelineError(
                    f"segment-{index + 1}: existing visual needs a real clipId, mediaId or file"
                )
        else:
            raise PipelineError(f"segment-{index + 1}: unsupported visual type {kind!r}")

    # The voiceover is a real timeline asset, not just an analysis input.
    # Reuse its imported media/clip when present; otherwise import it and
    # place it on a dedicated bottom lane before cutting measured dead air.
    # The repaired file is what the timeline carries: the export must not
    # ship the noise the repair removed.
    view = advance("project_get", {"path": project})
    if not isinstance(view, dict):
        raise PipelineError("project_get returned no project view")
    media = view.get("media", [])
    media = media if isinstance(media, list) else []
    if audio_media_id:
        audio_item = next(
            (item for item in media if isinstance(item, dict) and item.get("id") == audio_media_id),
            None,
        )
        if audio_item is None:
            raise PipelineError(f"the watched audio media id {audio_media_id} is no longer in the project bin")
        # The bin's original is the analysis input; the timeline still
        # carries the repaired take when the repair produced one.
        audio_media_id = None
    if not audio_media_id:
        audio_item = next(
            (
                item for item in media
                if isinstance(item, dict)
                and item.get("kind") == "audio"
                and os.path.realpath(str(item.get("path", ""))) == os.path.realpath(audio)
            ),
            None,
        )
        if audio_item is None:
            imported_audio = advance("media_import", {"path": project, "file": audio})
            new_media_id = imported_audio.get("createdId") if isinstance(imported_audio, dict) else None
            if not isinstance(new_media_id, str) or not new_media_id:
                raise PipelineError("media_import did not return the voiceover media id")
            audio_media_id = new_media_id
        else:
            audio_media_id = str(audio_item.get("id", ""))

    timeline = view.get("timeline", {})
    timeline = timeline if isinstance(timeline, dict) else {}
    clips = timeline.get("clips", [])
    clips = clips if isinstance(clips, list) else []
    # Resolve existing visuals against the real project view before edits.
    existing_clip_refs: dict[int, dict[str, Any]] = {}
    existing_media_refs: dict[int, dict[str, Any]] = {}
    for index, segment in existing_segments(plan):
        visual = segment["visual"]
        if visual.get("clipId"):
            clip = next(
                (item for item in clips if isinstance(item, dict) and item.get("id") == visual["clipId"]),
                None,
            )
            if clip is None or clip.get("kind") != "video":
                raise PipelineError(
                    f"segment-{index + 1}: existing clipId is not a video clip on the active timeline"
                )
            start_at = float(segment["start"])
            target_duration = float(segment["end"]) - start_at
            if abs(float(clip.get("start", -999.0)) - start_at) > 0.05:
                raise PipelineError(
                    f"segment-{index + 1}: existing clip is not at its planned start; use mediaId to place it"
                )
            if float(clip.get("duration", 0.0)) + 0.05 < target_duration:
                raise PipelineError(
                    f"segment-{index + 1}: existing clip is shorter than its planned segment"
                )
            existing_clip_refs[index] = clip
        elif visual.get("mediaId"):
            media_item = next(
                (item for item in media if isinstance(item, dict) and item.get("id") == visual["mediaId"]),
                None,
            )
            if media_item is None or media_item.get("kind") != "video":
                raise PipelineError(
                    f"segment-{index + 1}: existing mediaId is not a video item in the project bin"
                )
            existing_media_refs[index] = media_item
        else:
            source_file = str(visual.get("file", ""))
            if not source_file or not os.path.isfile(source_file):
                raise PipelineError(
                    f"segment-{index + 1}: existing visual file does not exist: {source_file}"
                )

    requested_audio_clip = plan.get("audioClipId")
    if requested_audio_clip:
        audio_clip = next(
            (clip for clip in clips if isinstance(clip, dict) and clip.get("id") == requested_audio_clip),
            None,
        )
        if audio_clip is None or audio_clip.get("kind") != "audio":
            raise PipelineError(
                f"audioClipId {requested_audio_clip!r} is not an audio clip in the active timeline"
            )
        if audio_clip.get("media") != audio_media_id:
            raise PipelineError(
                f"audioClipId {requested_audio_clip!r} does not reference the supplied voiceover media"
            )
        audio_clip_id = str(requested_audio_clip)
    else:
        audio_clip = next(
            (
                clip for clip in clips
                if isinstance(clip, dict)
                and clip.get("kind") == "audio"
                and clip.get("media") == audio_media_id
            ),
            None,
        )
        if audio_clip is not None:
            audio_clip_id = str(audio_clip.get("id"))
        else:
            audio_track = advance("track_add", {"path": project, "bottom": True})
            audio_track_id = audio_track.get("createdId") if isinstance(audio_track, dict) else None
            if not isinstance(audio_track_id, str) or not audio_track_id:
                raise PipelineError("track_add did not return a voiceover track id")
            placed_audio = advance(
                "clip_add",
                {
                    "path": project,
                    "mediaId": audio_media_id,
                    "trackId": audio_track_id,
                    "start": 0.0,
                },
            )
            audio_clip_id = placed_audio.get("createdId") if isinstance(placed_audio, dict) else None
            if not isinstance(audio_clip_id, str) or not audio_clip_id:
                raise PipelineError("clip_add did not return the placed voiceover clip id")

    # The plan's voiceover speed, applied to the placed clip: the plan's
    # segment times are the final (sped) timebase, so the placements that
    # follow are unaffected - only the voiceover's own length changes.
    # IDEM: a second run must not compound the speed - the clip the plan
    # already sped is left at its speed, and the handoff's record says
    # which clip and which speed so the reuse is a fact, not a guess.
    voiceover = (plan.get("audio") or {}).get("voiceover") or {}
    _handoff_prior = _stage_prior("handoff")
    _handoff_speed = None
    if isinstance(voiceover, dict):
        _asked_speed = voiceover.get("speed")
        if isinstance(_asked_speed, (int, float)) and not isinstance(_asked_speed, bool):
            _handoff_speed = float(_asked_speed)
    _speed_reusable = (
        isinstance(_handoff_prior, dict)
        and _handoff_prior.get("planSha256") == plan_sha
        and _handoff_prior.get("clipId") == str(audio_clip_id)
        and _handoff_prior.get("speed") == _handoff_speed
    )
    if isinstance(voiceover, dict) and "clip_speed" in served:
        speed = voiceover.get("speed")
        if isinstance(speed, (int, float)) and not isinstance(speed, bool) and float(speed) != 1.0:
            if _speed_reusable:
                _stage_skip("handoff", f"clip {audio_clip_id} already at {float(speed):g}x")
            else:
                advance("clip_speed", {"path": project, "clipId": audio_clip_id, "speed": float(speed)})
                log(f"voiceover speed {float(speed):g}x applied to clip {audio_clip_id}")
    _stage_mark(
        "handoff", planSha256=plan_sha,
        mediaId=str(audio_media_id), clipId=str(audio_clip_id),
        speed=_handoff_speed,
    )

    log(f"voiceover ready on timeline: clip {audio_clip_id}; timing remains unchanged by the runner")
    _audit(
        "audio.voiceover", "executed",
        "the take repaired, its glossary handed to the recogniser, its speed applied",
    )
    _audit(
        "audioClipId",
        "executed" if requested_audio_clip else "skipped",
        f"the existing voiceover clip {requested_audio_clip} reused in place"
        if requested_audio_clip else "the plan does not carry it",
    )

    # Rank one unique candidate per actual timed B-roll segment.
    # IDEM: a slot this plan already filled is reused when its clip is
    # still on the timeline - the slot is not re-shopped, re-imported or
    # re-trimmed, so a resumed run places no duplicate B-roll. A slot
    # continuity downgraded to its storyboard in a prior run is
    # downgraded again here, so the graphics pass owns it as before.
    _gather_prior = _stage_prior("gather")
    _reusable_slots: dict[str, str] = {}
    _live_clip_ids = {
        str(clip.get("id")) for clip in clips if isinstance(clip, dict)
    }
    if (
        isinstance(_gather_prior, dict)
        and _gather_prior.get("planSha256") == plan_sha
        and isinstance(_gather_prior.get("slots"), dict)
    ):
        for _slot_id, _entry in _gather_prior["slots"].items():
            if (
                isinstance(_entry, dict)
                and isinstance(_entry.get("clipId"), str)
                and _entry["clipId"] in _live_clip_ids
            ):
                _reusable_slots[_slot_id] = _entry["clipId"]
        for _slot_id in _gather_prior.get("downgraded") or []:
            if not isinstance(_slot_id, str):
                continue
            for _index, _segment in broll_segments(plan):
                if str(_segment.get("slotId") or f"segment-{_index + 1}") == _slot_id:
                    if isinstance(_segment["visual"].get("storyboard"), dict):
                        _segment["visual"]["type"] = "textCard"
                    break
    excluded: list[int] = []
    chosen: dict[str, dict[str, Any]] = {}
    slot_segments: dict[str, tuple[int, dict[str, Any]]] = {}
    # DEG-03: a keyword-tier ranking is recorded once per run, not once
    # per slot - the first reply that says the vision rerank did not run
    # speaks for every search after it.
    rerank_fallback_recorded = False
    # IDEM: the B-roll slots as they stand entering the ranking - a slot
    # continuity later downgrades to its storyboard leaves this list's
    # `chosen` without ever being re-shopped, and the gather record must
    # name it so the resume downgrades it the same way.
    _broll_slots_at_ranking = [
        str(segment.get("slotId") or f"segment-{index + 1}")
        for index, segment in broll_segments(plan)
    ]
    for index, segment in broll_segments(plan):
        visual = segment["visual"]
        slot_id = str(segment.get("slotId") or f"segment-{index + 1}")
        if slot_id in _reusable_slots:
            _stage_skip(
                "gather",
                f"{slot_id}'s clip {_reusable_slots[slot_id]} is still on the timeline",
            )
            continue
        query = str(visual.get("query") or "").strip()
        if not query:
            raise PipelineError(f"{slot_id}: B-roll segment has no real visual.query")
        description = str(
            visual.get("description") or segment.get("spoken") or query
        ).strip()
        queries = visual.get("queries")
        if not isinstance(queries, list) or not queries:
            queries = [query]
        target_duration = float(segment["end"]) - float(segment["start"])
        candidate = None
        # Do not place a stock shot that ends before its story beat. Search
        # again with the rejected id excluded; at most three candidates per
        # slot, then stop before any timeline mutation.
        for attempt in range(3):
            reply = advance(
                "openvid_rank_for_slot",
                {
                    "description": description,
                    "queries": queries,
                    "hero": bool(visual.get("hero", False)),
                    "motion_min": visual.get("motionMin"),
                    "exclude_ids": list(excluded),
                },
            )
            rerank = reply.get("rerank") if isinstance(reply, dict) else None
            if (
                not rerank_fallback_recorded
                and isinstance(rerank, dict)
                and rerank.get("applied") is False
            ):
                rerank_fallback_recorded = True
                record_fallback(
                    "broll", "ranking", "openvid_search.rerank",
                    "stock ranking fell back to keyword-tier (BM25 order "
                    "stands): "
                    + str(rerank.get("reason") or "the vision rerank did not run")
                    + " - stock_contact_sheet is the way to look at the page",
                )
            clips_found = reply.get("clips", []) if isinstance(reply, dict) else []
            if not clips_found:
                break
            candidate = clips_found[0]
            clip_id = candidate.get("id")
            seconds = candidate.get("seconds")
            if isinstance(seconds, (int, float)) and seconds + 0.05 < target_duration:
                log(f"{slot_id}: rejecting {candidate.get('name', clip_id)}; {seconds:.2f}s is shorter than the {target_duration:.2f}s beat")
                if isinstance(clip_id, (int, str)) and not isinstance(clip_id, bool):
                    excluded.append(_clip_int_id(clip_id, slot_id))
                candidate = None
                continue
            break
        if candidate is None:
            raise PipelineError(
                f"{slot_id}: no candidate is long enough for its {target_duration:.2f}s story beat"
            )
        chosen[slot_id] = candidate
        slot_segments[slot_id] = (index, segment)
        clip_id = candidate.get("id")
        if isinstance(clip_id, (int, str)) and not isinstance(clip_id, bool):
            excluded.append(_clip_int_id(clip_id, slot_id))

    # See the chosen candidates before anything is fetched: the contact
    # sheet is the approval record, and its path is logged for review.
    if chosen and "stock_contact_sheet" in served:
        ids = []
        for slot_id, clip in chosen.items():
            try:
                ids.append(_clip_int_id(clip.get("id"), slot_id))
            except PipelineError as error:
                log(f"stock_contact_sheet skipped: {error}")
                ids = []
                break
        if ids:
            sheet = advance("stock_contact_sheet", {"ids": ids, "rank": True})
            if isinstance(sheet, dict) and sheet.get("pngPath"):
                log(f"B-roll contact sheet: {sheet['pngPath']}")
            else:
                log("stock_contact_sheet returned no sheet; importing without a visual approval record")

    # Continuity reads measured library metadata. Replace flagged shots once
    # with the remedy's avoid hint; never silently pretend unknown metadata
    # (colour/scale) was measured - an unmeasured reading is omitted, and
    # the gate stands its rules down for it rather than judge a fake 0.0.
    if chosen:
        def continuity_manifest() -> list[dict[str, Any]]:
            result = []
            for slot_id, clip in chosen.items():
                index, segment = slot_segments[slot_id]
                visual = segment["visual"]
                entry: dict[str, Any] = {
                    "slotId": slot_id,
                    "clipId": str(clip.get("id", clip.get("name", slot_id))),
                    "dominantDirection": _direction(clip.get("camera")),
                    "shotScale": visual.get("shotScale", "medium"),
                    "category": str(visual.get("category") or clip.get("category") or clip.get("source") or "unknown"),
                    "motionMin": float(visual.get("motionMin") or 0.0),
                }
                # Only a measured reading is a reading: colour and motion
                # the search did not carry are left out entirely, never
                # written as a 0.0 that continuity would read as neutral.
                for key in ("colorTemperature", "motion"):
                    value = clip.get(key)
                    if isinstance(value, (int, float)) and not isinstance(value, bool):
                        entry[key] = float(value)
                result.append(entry)
            return result

        def attempt_replace(slot_id: str, avoid: str) -> None:
            index, segment = slot_segments[slot_id]
            visual = segment["visual"]
            old_id = chosen[slot_id].get("id")
            old_int = _clip_int_id(old_id, slot_id) if old_id is not None else None
            retry = advance(
                "openvid_rank_for_slot",
                {
                    "description": f"{visual.get('description') or segment.get('spoken') or visual.get('query', '')}; avoid {avoid}",
                    "queries": visual.get("queries") or [visual.get("query")],
                    "hero": bool(visual.get("hero", False)),
                    "motion_min": visual.get("motionMin"),
                    "exclude_ids": list(dict.fromkeys(excluded + ([old_int] if old_int is not None else []))),
                },
            )
            alternatives = retry.get("clips", []) if isinstance(retry, dict) else []
            required_duration = float(segment["end"]) - float(segment["start"])
            alternatives = [
                candidate for candidate in alternatives
                if candidate.get("id") != old_id
                and isinstance(candidate.get("seconds"), (int, float))
                and float(candidate["seconds"]) + 0.05 >= required_duration
            ]
            if not alternatives:
                raise PipelineError(
                    f"continuity could not find a replacement long enough for {slot_id}: {avoid}"
                )
            chosen[slot_id] = alternatives[0]
            new_id = alternatives[0].get("id")
            if new_id is not None:
                excluded.append(_clip_int_id(new_id, slot_id))

        continuity = advance("footage_continuity", {"clips": continuity_manifest()})
        flags = continuity.get("flags", []) if isinstance(continuity, dict) else []
        for flag in flags:
            slot_id = flag.get("slotId")
            remedy = flag.get("remedy", {})
            replace = remedy.get("replace") if isinstance(remedy, dict) else None
            if not isinstance(slot_id, str) or slot_id not in chosen:
                continue
            if isinstance(replace, dict):
                avoid = str(replace.get("avoid") or "the previous shot's look")
                attempt_replace(slot_id, avoid)
                continue
            # A downgrade the plan can honour: an authored storyboard
            # becomes the graphic, whatever the remedy's exact flavour.
            if isinstance(remedy, dict) and (
                "downgrade_to_graphic" in remedy or "downgrade_to_hold" in remedy
            ):
                index, segment = slot_segments[slot_id]
                if isinstance(segment["visual"].get("storyboard"), dict):
                    segment["visual"]["type"] = "textCard"
                    chosen.pop(slot_id, None)
                    slot_segments.pop(slot_id, None)
                    log(f"{slot_id}: continuity downgraded the shot to its authored motion storyboard")
                    continue
                log(
                    f"{slot_id}: continuity asked for a downgrade with no authored "
                    "storyboard to downgrade to; replacing the shot instead"
                )
                attempt_replace(slot_id, str(flag.get("detail") or "the previous shot's look"))
                continue
            # A remedy this runner has never met: the honest move is a
            # replacement away from what the flag describes, not a crash
            # that throws away a fixable cut.
            log(
                f"{slot_id}: continuity remedy "
                f"{json.dumps(remedy, ensure_ascii=False)} is not one this runner "
                "knows by name; replacing the flagged shot"
            )
            attempt_replace(slot_id, str(flag.get("detail") or "the previous shot's look"))
        if flags:
            second = advance("footage_continuity", {"clips": continuity_manifest()})
            remaining = second.get("flags", []) if isinstance(second, dict) else []
            if remaining:
                raise PipelineError(
                    "continuity still has unresolved flags after one repair pass: "
                    + json.dumps(remaining, ensure_ascii=False)
                )

    # Import and place each chosen source at its segment's real timeline
    # start, with the plan's own trim honoured beside the segment's length.
    # IDEM: the walk is the plan's own B-roll order, so a reused slot's
    # recorded clip rides the same order the fresh placements do - the
    # transition and keyframe passes below index by position, and the
    # order must not shift between a first run and its resume.
    placed_clip_ids: list[str] = []
    _gather_slots: dict[str, dict[str, Any]] = {
        slot_id: {"clipId": clip_id} for slot_id, clip_id in _reusable_slots.items()
    }
    for index, segment in broll_segments(plan):
        slot_id = str(segment.get("slotId") or f"segment-{index + 1}")
        if slot_id in _reusable_slots:
            placed_clip_ids.append(_reusable_slots[slot_id])
            slot_segments[slot_id] = (index, segment)
            continue
        if slot_id not in chosen:
            continue
        clip = chosen[slot_id]
        start_at = float(segment["start"])
        target_duration = float(segment["end"]) - start_at
        imported = advance(
            "openvid_import",
            {"path": project, "ids": [_clip_int_id(clip["id"], slot_id)], "start": start_at},
        )
        media_ids = imported.get("mediaIds", []) if isinstance(imported, dict) else []
        if not media_ids:
            raise PipelineError(f"{slot_id}: OpenVid import did not return a media id")
        media_id = str(media_ids[0])
        view = imported.get("view") if isinstance(imported, dict) else None
        placed = _clip_for_media(view, media_id, start_at)
        if placed is None:
            view = transport.call("project_get", {"path": project})
            placed = _clip_for_media(view, media_id, start_at)
        if placed is None:
            raise PipelineError(f"{slot_id}: imported media is not present on the timeline")
        actual_duration = float(placed["duration"])
        if actual_duration + 0.05 < target_duration:
            raise PipelineError(
                f"{slot_id}: imported clip duration {actual_duration:.2f}s is shorter "
                f"than the required {target_duration:.2f}s segment"
            )
        if actual_duration > target_duration + 0.05:
            delta = target_duration - actual_duration
            advance(
                "clip_trim",
                {
                    "path": project,
                    "clipId": placed["id"],
                    "edge": "end",
                    "delta": delta,
                    "ripple": False,
                },
            )
        placed_clip_ids.append(str(placed["id"]))
        _gather_slots[slot_id] = {"clipId": str(placed["id"])}
        log(
            f"{slot_id}: imported {clip.get('name', clip['id'])} at {start_at:.2f}s "
            f"for {min(actual_duration, target_duration):.2f}s"
        )
    # IDEM: the gather's own record - the clip each slot owns, and the
    # slots continuity turned into graphics (a resume downgrades them
    # the same way instead of re-shopping what was replaced).
    _prior_downgraded = (
        [slot for slot in (_gather_prior.get("downgraded") or []) if isinstance(slot, str)]
        if isinstance(_gather_prior, dict) else []
    )
    _downgraded_this_run = [
        slot_id for slot_id in _broll_slots_at_ranking
        if slot_id not in chosen and slot_id not in _reusable_slots
    ]
    _stage_mark(
        "gather", planSha256=plan_sha, slots=_gather_slots,
        downgraded=list(dict.fromkeys(_prior_downgraded + _downgraded_this_run)),
    )

    # Motion reels and imported existing media share one dedicated top
    # video lane; already-placed existing clips are verified in place.
    # IDEM: a graphic this plan already rendered and placed is reused
    # when its clip still stands on the timeline - and the lane is only
    # added when something is actually left to place, so a resumed run
    # opens no second lane beside the first.
    graphics = motion_segments(plan)
    # The placed clip each graphic segment owns, for the revision loop's
    # re-render to swap in place.
    graphic_clip_ids: dict[int, str] = {}
    existing_to_place = [
        (index, segment) for index, segment in existing_segments(plan)
        if not segment["visual"].get("clipId")
    ]
    _graphics_prior = _stage_prior("graphics")
    _reusable_graphics: dict[int, str] = {}
    if (
        isinstance(_graphics_prior, dict)
        and _graphics_prior.get("planSha256") == plan_sha
        and isinstance(_graphics_prior.get("segments"), dict)
    ):
        for _index_text, _clip_id in _graphics_prior["segments"].items():
            try:
                _index = int(_index_text)
            except (TypeError, ValueError):
                continue
            if isinstance(_clip_id, str) and _clip_id in _live_clip_ids:
                _reusable_graphics[_index] = _clip_id
    _graphics_slots: dict[str, str] = {
        str(index): clip_id for index, clip_id in _reusable_graphics.items()
    }
    _reusable_existing: dict[int, str] = {}
    if (
        isinstance(_graphics_prior, dict)
        and _graphics_prior.get("planSha256") == plan_sha
        and isinstance(_graphics_prior.get("existing"), dict)
    ):
        for _index_text, _clip_id in _graphics_prior["existing"].items():
            try:
                _index = int(_index_text)
            except (TypeError, ValueError):
                continue
            if isinstance(_clip_id, str) and _clip_id in _live_clip_ids:
                _reusable_existing[_index] = _clip_id
    _existing_slots: dict[str, str] = {
        str(index): clip_id for index, clip_id in _reusable_existing.items()
    }
    track_id = None
    _graphics_to_place = [
        (index, segment) for index, segment in graphics
        if index not in _reusable_graphics
    ]
    _existing_to_place = [
        (index, segment) for index, segment in existing_to_place
        if index not in _reusable_existing
    ]
    if _graphics_to_place or _existing_to_place:
        track_reply = advance("track_add", {"path": project, "bottom": False})
        track_id = track_reply.get("createdId") if isinstance(track_reply, dict) else None
        if not isinstance(track_id, str) or not track_id:
            raise PipelineError("track_add did not return the dedicated top video track id")
    for index, clip in existing_clip_refs.items():
        segment = next(s for i, s in existing_segments(plan) if i == index)
        target_duration = float(segment["end"]) - float(segment["start"])
        actual_duration = float(clip["duration"])
        if actual_duration > target_duration + 0.05:
            advance(
                "clip_trim",
                {
                    "path": project,
                    "clipId": clip["id"],
                    "edge": "end",
                    "delta": target_duration - actual_duration,
                    "ripple": False,
                },
            )
    for index, segment in graphics:
        # IDEM: the graphic this segment already owns is reused in place.
        if index in _reusable_graphics:
            graphic_clip_ids[index] = _reusable_graphics[index]
            placed_clip_ids.append(_reusable_graphics[index])
            _stage_skip(
                "graphics",
                f"segment-{index + 1}'s clip {_reusable_graphics[index]} still stands",
            )
            continue
        visual = segment["visual"]
        storyboard = visual.get("storyboard")
        if not isinstance(storyboard, dict) or not isinstance(storyboard.get("scenes"), list):
            raise PipelineError(
                f"segment-{index + 1}: {visual.get('type')} needs an authored storyboard; "
                "refusing to export an empty graphic beat"
            )
        start_at = float(segment["start"])
        target_duration = float(segment["end"]) - start_at
        reel = advance(
            "motion_reel",
            {
                "project": project,
                "storyboard": storyboard,
                "name": f"director-segment-{index + 1}.mp4",
                "crf": 18,
            },
        )
        motion_file = reel.get("file") if isinstance(reel, dict) else None
        if not isinstance(motion_file, str) or not motion_file:
            raise PipelineError(f"segment-{index + 1}: motion_reel returned no rendered file")
        rendered_duration = float(reel.get("duration", target_duration))
        if rendered_duration + 0.05 < target_duration:
            raise PipelineError(
                f"segment-{index + 1}: rendered motion is {rendered_duration:.2f}s, "
                f"shorter than the {target_duration:.2f}s story beat"
            )
        imported = advance("media_import", {"path": project, "file": motion_file})
        media_id = imported.get("createdId") if isinstance(imported, dict) else None
        if not isinstance(media_id, str) or not media_id:
            raise PipelineError(f"segment-{index + 1}: media_import returned no media id")
        placed = advance(
            "clip_add",
            {
                "path": project,
                "mediaId": media_id,
                "trackId": track_id,
                "start": start_at,
            },
        )
        clip_id = placed.get("createdId") if isinstance(placed, dict) else None
        if not isinstance(clip_id, str) or not clip_id:
            raise PipelineError(f"segment-{index + 1}: clip_add returned no timeline clip id")
        if rendered_duration > target_duration + 0.05:
            advance(
                "clip_trim",
                {
                    "path": project,
                    "clipId": clip_id,
                    "edge": "end",
                    "delta": target_duration - rendered_duration,
                    "ripple": False,
                },
            )
        graphic_clip_ids[index] = clip_id
        placed_clip_ids.append(clip_id)
        _graphics_slots[str(index)] = clip_id
        log(f"segment-{index + 1}: rendered and placed motion at {start_at:.2f}s")
    _stage_mark(
        "graphics", planSha256=plan_sha, segments=_graphics_slots,
    )

    # Place imported existing media on the same overlay lane. A supplied
    # clipId was already verified to exist at the correct time above.
    for index, segment in existing_to_place:
        # IDEM: an existing-media placement this plan already made rides
        # its recorded clip, not a second copy beside it.
        if index in _reusable_existing:
            placed_clip_ids.append(_reusable_existing[index])
            _stage_skip(
                "graphics",
                f"segment-{index + 1}'s existing clip {_reusable_existing[index]} still stands",
            )
            continue
        visual = segment["visual"]
        start_at = float(segment["start"])
        target_duration = float(segment["end"]) - start_at
        media_id = visual.get("mediaId")
        media_item = existing_media_refs.get(index)
        if not media_id:
            imported = advance("media_import", {"path": project, "file": visual["file"]})
            media_id = imported.get("createdId") if isinstance(imported, dict) else None
            imported_media = imported.get("media", []) if isinstance(imported, dict) else []
            media_item = next(
                (
                    item for item in imported_media
                    if isinstance(item, dict) and item.get("id") == media_id
                ),
                None,
            )
            if media_item is None:
                refreshed = advance("project_get", {"path": project})
                refreshed_media = refreshed.get("media", []) if isinstance(refreshed, dict) else []
                media_item = next(
                    (
                        item for item in refreshed_media
                        if isinstance(item, dict) and item.get("id") == media_id
                    ),
                    None,
                )
        if not isinstance(media_id, str) or not media_id:
            raise PipelineError(f"segment-{index + 1}: existing source import returned no media id")
        if not isinstance(media_item, dict) or not isinstance(media_item.get("duration"), (int, float)):
            raise PipelineError(f"segment-{index + 1}: cannot verify the existing source duration")
        placed = advance(
            "clip_add",
            {
                "path": project,
                "mediaId": media_id,
                "trackId": track_id,
                "start": start_at,
            },
        )
        clip_id = placed.get("createdId") if isinstance(placed, dict) else None
        if not isinstance(clip_id, str) or not clip_id:
            raise PipelineError(f"segment-{index + 1}: existing media placement returned no clip id")
        actual_duration = float(media_item["duration"])
        if actual_duration + 0.05 < target_duration:
            raise PipelineError(
                f"segment-{index + 1}: existing media is shorter than its planned segment"
            )
        if actual_duration > target_duration + 0.05:
            advance(
                "clip_trim",
                {
                    "path": project,
                    "clipId": clip_id,
                    "edge": "end",
                    "delta": target_duration - actual_duration,
                    "ripple": False,
                },
            )
        placed_clip_ids.append(clip_id)
        _existing_slots[str(index)] = clip_id
        log(f"segment-{index + 1}: existing media placed at {start_at:.2f}s")
    _stage_mark(
        "graphics", planSha256=plan_sha,
        segments=_graphics_slots, existing=_existing_slots,
    )
    if existing_to_place or any(existing_segments(plan)):
        _audit(
            "segments.visual.existing", "executed",
            f"{len(existing_segments(plan))} existing-media segment(s) placed and trimmed to their beats",
        )

    # The plan's cut vocabulary, applied to the placed clips: the
    # transition into each segment, and the entrance/exit animations. The
    # transition's duration is clamped to half the segment it enters, so
    # a two-second beat never spends a full second dissolving.
    runtime = max(
        (float(segment["end"]) for segment in plan_segments(plan)), default=0.0
    )
    # The placed clip each B-roll segment owns, in the order the
    # placements ran: the chosen list is slot-ordered, and the placements
    # walked it in exactly that order.
    slot_order: dict[str, int] = {
        slot_id: order for order, slot_id in enumerate(slot_segments)
    }
    segment_clip_ids: dict[int, str] = {}
    for index, segment in broll_segments(plan):
        slot_id = str(segment.get("slotId") or f"segment-{index + 1}")
        order = slot_order.get(slot_id)
        if order is not None and order < len(placed_clip_ids):
            segment_clip_ids[index] = placed_clip_ids[order]

    def transition_arguments(segment: dict[str, Any]) -> dict[str, Any] | None:
        transition = segment.get("transitionIn")
        if not isinstance(transition, dict):
            return None
        effect = str(transition.get("effect") or "").strip()
        if not effect:
            return None
        duration = float(segment["end"]) - float(segment["start"])
        seconds = float(transition.get("seconds") or 0.0)
        patch: dict[str, Any] = {"id": effect}
        if seconds:
            patch["duration"] = round(min(seconds, max(0.2, duration / 2.0)), 3)
        return {"path": project, "patch": {"transitionIn": patch}}

    def entrance_arguments(
        clip_id: str, segment: dict[str, Any], duration: float
    ) -> list[dict[str, Any]]:
        """The keyframes an entrance/exit asks for, as clip-relative
        (0-1) keys the engine understands. `static` is deliberate
        stillness: it animates nothing, which is the point."""
        keys: list[dict[str, Any]] = []

        def key(property_name: str, at: float, value: float, seconds: float) -> None:
            keys.append({
                "path": project, "clipId": clip_id, "property": property_name,
                "at": round(max(0.0, min(1.0, at)), 4),
                "value": round(value, 4),
            })

        entrance = segment.get("in") if isinstance(segment.get("in"), dict) else None
        if entrance and entrance.get("anim") not in (None, "static"):
            seconds = float(entrance.get("seconds") or 0.25)
            fraction = max(0.08, min(0.6, seconds / duration)) if duration > 0 else 0.25
            anim = entrance.get("anim")
            if anim == "pop":
                key("scale", 0.0, 0.6, seconds); key("scale", fraction, 1.0, seconds)
            elif anim == "punch":
                key("scale", 0.0, 1.18, seconds); key("scale", fraction, 1.0, seconds)
            elif anim == "slide":
                key("offsetY", 0.0, 0.12, seconds); key("offsetY", fraction, 0.0, seconds)
            elif anim == "dissolve":
                key("opacity", 0.0, 0.0, seconds); key("opacity", fraction, 1.0, seconds)
            elif anim == "whip":
                key("opacity", 0.0, 0.0, seconds); key("opacity", fraction * 0.6, 1.0, seconds)
        exit_read = segment.get("out") if isinstance(segment.get("out"), dict) else None
        if exit_read and exit_read.get("anim") not in (None, "static"):
            seconds = float(exit_read.get("seconds") or 0.25)
            fraction = max(0.08, min(0.6, seconds / duration)) if duration > 0 else 0.25
            anim = exit_read.get("anim")
            if anim == "pop":
                key("scale", 1.0 - fraction, 1.0, seconds); key("scale", 1.0, 0.6, seconds)
            elif anim == "punch":
                key("scale", 1.0 - fraction, 1.0, seconds); key("scale", 1.0, 1.18, seconds)
            elif anim == "slide":
                key("offsetY", 1.0 - fraction, 0.0, seconds); key("offsetY", 1.0, -0.12, seconds)
            elif anim in ("dissolve", "whip"):
                key("opacity", 1.0 - fraction, 1.0, seconds); key("opacity", 1.0, 0.0, seconds)
        return keys

    # IDEM: the timeline passes - transitions, entrance keys, the grade -
    # are a function of the plan over the placed clips. When every clip
    # the record names still stands, the passes already ran over exactly
    # these clips, and a resume re-keys nothing (a second keyframe set on
    # the same clip would compound; a second grade layer would stack).
    _timeline_prior = _stage_prior("timeline")
    _timeline_reusable = (
        isinstance(_timeline_prior, dict)
        and _timeline_prior.get("planSha256") == plan_sha
        and isinstance(_timeline_prior.get("clipIds"), list)
        and bool(_timeline_prior["clipIds"]) is bool(placed_clip_ids)
        and all(
            isinstance(clip_id, str) and clip_id in _live_clip_ids
            for clip_id in _timeline_prior["clipIds"]
        )
    )
    if _timeline_reusable:
        _stage_skip(
            "timeline",
            "the transitions, keys and grade already ran over these clips",
        )
    else:
        if "clip_update" in served:
            applied_transitions = 0
            for index, segment in broll_segments(plan):
                arguments = transition_arguments(segment)
                if arguments is None:
                    continue
                clip_id = segment_clip_ids.get(index)
                if clip_id is None:
                    continue
                arguments["clipId"] = clip_id
                advance("clip_update", arguments)
                applied_transitions += 1
                log(f"segment-{index + 1}: transition into the cut applied to clip {clip_id}")
            _audit(
                "segments.transitionIn", "executed" if applied_transitions else "skipped",
                f"{applied_transitions} transition(s) applied on the cut"
                if applied_transitions else "the plan does not carry it",
            )
        if "keyframe_set" in served:
            for index, segment in broll_segments(plan):
                clip_id = segment_clip_ids.get(index)
                if clip_id is None:
                    continue
                duration = float(segment["end"]) - float(segment["start"])
                for arguments in entrance_arguments(clip_id, segment, duration):
                    advance("keyframe_set", arguments)
            keyed = sum(
                1
                for segment in plan_segments(plan)
                for side in ("in", "out")
                if isinstance(segment.get(side), dict)
                and segment[side].get("anim") not in (None, "static")
            )
            _audit(
                "segments.in/out", "executed" if keyed else "skipped",
                f"{keyed} entrance/exit animation(s) keyed on the placed clips"
                if keyed else "the plan does not carry it",
            )

        # The plan's grade. `layer` is one effect layer over the whole runtime
        # - with the duration written, because absent means five seconds and
        # five seconds is the trap. `perClip` is the same chain on every
        # placed clip. Either way the grade is applied, not admired.
        grade = plan.get("grade") if isinstance(plan.get("grade"), dict) else None
        if grade and grade.get("filter"):
            apply_mode = str(grade.get("apply") or "layer")
            if apply_mode == "layer" and "effect_layer_add" in served:
                advance("effect_layer_add", {
                    "path": project,
                    "start": 0.0,
                    "duration": max(runtime, 0.1),
                    "effectId": str(grade["filter"]),
                    "name": "director-grade",
                    "trackId": None,
                })
                log(f"grade {grade['filter']} applied as one layer over {runtime:.2f}s")
                _audit("grade", "executed",
                       f"{grade['filter']} applied as one layer over {runtime:.2f}s")
            elif "clip_update" in served and placed_clip_ids:
                params = {}
                if grade.get("intensity") is not None and grade.get("intensityKey"):
                    params[str(grade["intensityKey"])] = grade["intensity"]
                patch = {"videoEffects": [{"id": str(grade["filter"]), "params": params}]}
                for clip_id in placed_clip_ids:
                    advance("clip_update", {"path": project, "clipId": clip_id, "patch": patch})
                log(f"grade {grade['filter']} applied to {len(placed_clip_ids)} placed clip(s)")
                _audit("grade", "executed",
                       f"{grade['filter']} applied per clip over {len(placed_clip_ids)} placed clip(s)")
            else:
                log("the plan's grade cannot be applied: the server serves neither effect layers nor clip patches")
                _audit("grade", "rejected",
                       "the server serves neither effect layers nor clip patches")
        elif isinstance(plan.get("grade"), dict):
            _audit("grade", "rejected", "the plan's grade names no filter")
    _stage_mark(
        "timeline", planSha256=plan_sha,
        clipIds=[str(clip_id) for clip_id in placed_clip_ids],
    )

    # The plan's captions, segment-timed from the transcript the runner
    # measured: one caption a transcript segment, inside the plan's
    # runtime, styled the way the plan asked. Word-synced kinetic
    # captions are captions_animate's to bake - and when the plan asks
    # for them (captions.wordTimed, or a kinetic setting), the runner
    # wires the call: the measured word timings are the timing it was
    # told never to invent, the bake rides the plan's own preset and
    # kinetic settings, and the reel lands on the timeline where the
    # graphics do (P1-010). The timeline's segment-timed captions are
    # the kinetic reel's to replace, not double: the words appear once.
    captions_plan = plan.get("captions") if isinstance(plan.get("captions"), dict) else None
    captions_wanted = captions_plan is not None or any(
        segment.get("captions") for segment in plan_segments(plan)
    )
    kinetic_setting = None
    words: list[list[Any]] = []
    if captions_plan is not None:
        if captions_plan.get("wordTimed") is True or captions_plan.get("kinetic") is True:
            kinetic_setting = True
        elif isinstance(captions_plan.get("kinetic"), dict):
            kinetic_setting = captions_plan["kinetic"]
    # The captions the run actually placed, kept for the Verify stage's
    # own record: a plan that asks for captions is measured for having
    # them, not assumed (VERIFY-02, 12).
    placed_captions: list[dict[str, Any]] = []
    # IDEM: the captions are a function of the plan's ask and the
    # measured transcript - when both are the prior run's own, the
    # captions already on the timeline are the ones this plan asked for,
    # and the placement (kinetic bake or segment-timed) is not doubled.
    _captions_prior = _stage_prior("captions")
    _captions_sha = _json_sha256(transcript)
    # IDEM: only a placement stands as a placement - a record that placed
    # nothing is a refusal the run owes again, not a fact to reuse.
    _captions_reusable = (
        isinstance(_captions_prior, dict)
        and _captions_prior.get("planSha256") == plan_sha
        and _captions_prior.get("transcriptSha256") == _captions_sha
        and isinstance(_captions_prior.get("placed"), list)
        and bool(_captions_prior["placed"])
    )
    if captions_wanted and _captions_reusable:
        placed_captions = [
            entry for entry in _captions_prior["placed"] if isinstance(entry, dict)
        ]
        _stage_mark(
            "captions", planSha256=plan_sha,
            transcriptSha256=_captions_sha, placed=placed_captions,
        )
        _stage_skip("captions", "this transcript's captions are already placed")
    if captions_wanted and not _captions_reusable:
        if kinetic_setting is not None:
            # The word-timed path: the recogniser's own word times, the
            # plan's kinetic hand, one baked reel.
            if "captions_animate" not in served:
                raise PipelineError(
                    "the plan asks for word-timed kinetic captions and the server does "
                    "not serve captions_animate; the export would silently lack them"
                )
            words = []
            for segment in transcript.get("segments", []):
                if not isinstance(segment, dict):
                    continue
                for word in segment.get("words") or []:
                    if not isinstance(word, dict):
                        continue
                    text, start, end = word.get("text"), word.get("start"), word.get("end")
                    if isinstance(text, str) and text.strip() and isinstance(start, (int, float)) \
                            and isinstance(end, (int, float)) and float(end) > float(start):
                        words.append([text, float(start), float(end)])
            if not words:
                log(
                    "kinetic captions wanted, but the transcript carried no word "
                    "timings; segment-timed captions placed instead"
                )
                _signal(0.0, "captions", provenance="fallback",
                        note="kinetic captions wanted and no word timings answered; "
                             "segment-timed captions placed instead")
            else:
                animate: dict[str, Any] = {
                    "project": project,
                    "words": words,
                    "kinetic": kinetic_setting,
                }
                if isinstance(captions_plan.get("preset"), str) and captions_plan["preset"].strip():
                    animate["preset"] = captions_plan["preset"]
                if captions_plan.get("curtain") is False:
                    animate["curtain"] = False
                baked = advance("captions_animate", animate)
                baked_file = _reply_file(baked)
                baked_duration = (
                    float(baked.get("duration")) if isinstance(baked, dict)
                    and isinstance(baked.get("duration"), (int, float)) else runtime
                )
                if isinstance(baked, dict):
                    log(
                        f"kinetic captions baked: {baked.get('lines')} line(s) over "
                        f"{len(words)} word(s), {baked.get('width')}x{baked.get('height')}"
                        f" at {baked.get('fps'):g} fps"
                        if isinstance(baked.get("fps"), (int, float))
                        else f"kinetic captions baked: {baked.get('lines')} line(s) over {len(words)} word(s)"
                    )
                _signal(0.0, "captions", provenance="measured",
                        words=len(words),
                        note="word-timed kinetic captions baked from the recogniser's own word times")
                # The words the reel carries are the captions the run
                # placed: the Verify stage measures them for being on the
                # screen (VERIFY-02), the same as it measures the
                # timeline's own.
                placed_captions = [
                    {"start": start, "end": end, "text": text}
                    for text, start, end in words
                ]
                if baked_file:
                    # The reel lands where the graphics do: the overlay
                    # lane, its own lane made when no graphic came first.
                    # The encoded reel is opaque - it is the picture for
                    # its span, not a burn-in over the B-roll; the PNG
                    # sequence is the path that keeps a transparent
                    # comp's alpha. Said out loud, never silently.
                    imported = advance("media_import", {"path": project, "file": baked_file})
                    kinetic_media = imported.get("createdId") if isinstance(imported, dict) else None
                    if not isinstance(kinetic_media, str) or not kinetic_media:
                        raise PipelineError("the kinetic caption reel was baked but not imported")
                    if not isinstance(track_id, str) or not track_id:
                        track_id = advance("track_add", {"path": project})
                        if not isinstance(track_id, str) or not track_id:
                            raise PipelineError("track_add did not return the overlay lane for the captions")
                    placed = advance("clip_add", {
                        "path": project,
                        "mediaId": kinetic_media,
                        "trackId": track_id,
                        "start": 0.0,
                    })
                    kinetic_clip = placed.get("createdId") if isinstance(placed, dict) else None
                    if not isinstance(kinetic_clip, str) or not kinetic_clip:
                        raise PipelineError("the kinetic caption reel was imported but never placed")
                    log(
                        f"kinetic caption reel placed on the overlay lane: clip {kinetic_clip}, "
                        f"{baked_duration:.2f}s (the encoded reel is opaque: it is the "
                        "picture for its span, not a burn-in over the B-roll)"
                    )
                else:
                    log("kinetic captions baked, but the reply named no file to import")
                _audit(
                    "captions", "executed",
                    "the kinetic reel baked from the measured word timings and placed on the overlay lane",
                )
        elif "captions_add" not in served:
            raise PipelineError(
                "the plan asks for captions and the server does not serve captions_add; "
                "the export would silently lack them"
            )
        if kinetic_setting is None or not words:
            wanted_segments = [
                segment for segment in plan_segments(plan)
                if segment.get("captions") or (captions_plan is not None and not any(
                    s.get("captions") is False for s in [segment]
                ))
            ] if any(segment.get("captions") for segment in plan_segments(plan)) else plan_segments(plan)
            timed = [
                {"start": float(s["start"]), "end": float(s["end"]), "text": str(s.get("text") or "")}
                for s in transcript.get("segments", [])
                if isinstance(s, dict) and isinstance(s.get("start"), (int, float))
                and isinstance(s.get("end"), (int, float))
                and float(s["end"]) > float(s["start"])
                and float(s["start"]) < runtime
            ]
            for caption in timed:
                caption["end"] = min(caption["end"], runtime)
            if timed:
                style: dict[str, Any] = {"fontFamily": "Hanken Grotesk"}
                if captions_plan is not None:
                    for key in ("fontSize", "fontWeight", "color", "background"):
                        if captions_plan.get(key) is not None:
                            style[key] = captions_plan[key]
                offset_y = (captions_plan or {}).get("offsetY", 0.35)
                advance("captions_add", {
                    "path": project,
                    "captions": timed,
                    "style": style,
                    "offsetY": float(offset_y) if isinstance(offset_y, (int, float)) else 0.35,
                })
                placed_captions = timed
                log(f"{len(timed)} segment-timed caption(s) placed")
                _audit(
                    "captions", "executed",
                    f"{len(timed)} segment-timed caption(s) placed from the measured transcript",
                )
            else:
                log("captions wanted, but the transcript carried no timed segments to caption")
                _audit(
                    "captions", "rejected",
                    "captions wanted and the transcript carried no timed segments to caption",
                )
        _stage_mark(
            "captions", planSha256=plan_sha,
            transcriptSha256=_captions_sha, placed=placed_captions,
        )

    # The plan's SFX: shopped from the CC0 shelf, placed so the sound's
    # own peak lands on the planned beat (syncAt says where inside the
    # file the peak is), and gained to the plan's dB.
    # IDEM: the SFX beats are the plan's, placed once - their clips
    # recorded, and reused only while every one still stands.
    _sfx_prior = _stage_prior("sfx")
    _sfx_reusable = (
        isinstance(_sfx_prior, dict)
        and _sfx_prior.get("planSha256") == plan_sha
        and isinstance(_sfx_prior.get("clipIds"), list)
        and all(
            isinstance(clip_id, str) and clip_id in _live_clip_ids
            for clip_id in _sfx_prior["clipIds"]
        )
    )
    _sfx_clips: list[str] = []
    if _sfx_reusable:
        _sfx_clips = [str(clip_id) for clip_id in _sfx_prior["clipIds"]]
        if _sfx_clips:
            _stage_skip("sfx", "every beat's sound is still on the timeline")
    for index, segment in enumerate(plan_segments(plan)):
        if _sfx_reusable:
            break
        sfx = segment.get("sfx")
        if not isinstance(sfx, dict) or not str(sfx.get("query") or "").strip():
            continue
        if "sounds_search" not in served or "sounds_add" not in served:
            raise PipelineError(
                f"segment-{index + 1} asks for SFX and the server does not serve the "
                "sound shelf; the export would silently lack them"
            )
        found = advance("sounds_search", {"query": str(sfx["query"]), "limit": 5})
        rows = found.get("sounds", []) if isinstance(found, dict) else []
        if not rows:
            log(f"segment-{index + 1}: SFX {sfx['query']!r} found nothing on the shelf")
            continue
        sound = rows[0]
        peak_at = sfx.get("peakAt")
        sync_at = sound.get("syncAt")
        start = float(segment["start"])
        if isinstance(peak_at, (int, float)):
            # The peak lands where the plan says: back the file's own
            # lead-up out of the placement time.
            start = max(0.0, float(peak_at) - float(sync_at if isinstance(sync_at, (int, float)) else 0.0))
        added = advance("sounds_add", {"path": project, "id": sound.get("id"), "start": start})
        sfx_clip = added.get("clipId") if isinstance(added, dict) else None
        if not isinstance(sfx_clip, str) or not sfx_clip:
            log(f"segment-{index + 1}: the sound was imported but not placed; no gain to set")
            continue
        gain = sfx.get("gainDb")
        if isinstance(gain, (int, float)) and "clip_audio_levels_db" in served:
            advance("clip_audio_levels_db", {
                "path": project, "clipId": sfx_clip, "baseDb": float(gain),
            })
        _sfx_clips.append(str(sfx_clip))
        log(f"segment-{index + 1}: SFX {sound.get('name', sound.get('id'))} placed at {start:.2f}s")
    _stage_mark("sfx", planSha256=plan_sha, clipIds=_sfx_clips)

    sfx_asked = sum(
        1 for segment in plan_segments(plan) if isinstance(segment.get("sfx"), dict)
    )
    if sfx_asked:
        _audit(
            "segments.sfx", "executed",
            f"{sfx_asked} SFX beat(s) shopped, peaked and gained",
        )

    # The plan's music bed, under the words and ducked to them: only when
    # the take itself carries no music, only from the libraries the plan
    # named, and only when the library is actually present - a missing
    # library is a logged absence, never a silent one.
    # IDEM: the bed is placed once - its clip recorded, reused while it
    # still plays under the words. A bed that was refused or warned
    # absent stays unmarked, so a resume re-attempts it.
    bgm_plan = (plan.get("audio") or {}).get("bgm")
    _bgm_prior = _stage_prior("bgm")
    _bgm_reusable = (
        isinstance(_bgm_prior, dict)
        and _bgm_prior.get("planSha256") == plan_sha
        and isinstance(_bgm_prior.get("clipId"), str)
        and _bgm_prior["clipId"] in _live_clip_ids
    )
    if _bgm_reusable:
        _stage_mark("bgm", planSha256=plan_sha, clipId=_bgm_prior["clipId"])
        _stage_skip("bgm", f"the bed {_bgm_prior['clipId']} still plays under the words")
    elif isinstance(bgm_plan, dict) and str(
        bgm_plan.get("query") or bgm_plan.get("trackId") or bgm_plan.get("mood") or ""
    ).strip():
        if not bgm_allowed:
            log("BGM skipped: the take already carries music (detect_music_segments said so)")
            _audit("audio.bgm", "rejected",
                   "the take already carries music; no bed stacked (the music gate's own rule)")
            _signal(0.0, "bgm", provenance="measured",
                    note="detect_music_segments found music in the take; the bed was refused")
        elif "bgm_status" not in served or "bgm_search" not in served or "bgm_import" not in served:
            raise PipelineError(
                "the plan asks for a music bed and the server does not serve the "
                "music library; the export would silently lack it"
            )
        else:
            status = advance("bgm_status", {})
            state = status.get("state") if isinstance(status, dict) else None
            if state != "ready" and "bgm_prepare" in served:
                # DEG-07: a library that is not ready is usually "nobody
                # started it" or "the last server died mid-prepare" (the
                # prepare runs on a thread that goes with its process), so
                # this session starts it - the call is idempotent, a no-op
                # while it already runs - and gives it a bounded minute to
                # come ready before the bed is given up on.
                try:
                    advance("bgm_prepare", {})
                except PipelineError as error:
                    raise PipelineError(
                        "the music library could not be prepared for the "
                        f"bed the plan promised: {error}"
                    ) from error
                deadline = time.monotonic() + 60.0
                while time.monotonic() < deadline:
                    time.sleep(5.0)
                    status = advance("bgm_status", {})
                    state = status.get("state") if isinstance(status, dict) else None
                    if state in ("ready", "failed"):
                        break
                if state == "ready":
                    log("BGM: the music library came ready after bgm_prepare started it this session")
            if state != "ready":
                # DEG-06: a missing library is a warned, recorded absence -
                # never a silent one - and the export ships loudly without
                # the bed the plan promised.
                _audit("audio.bgm", "rejected",
                       f"the music library is {state or 'unavailable'}; no bed was shopped")
                record_fallback(
                    "bgm", "bed", "bgm_status",
                    "the music library is "
                    f"{state or 'unavailable'} after a prepare attempt this "
                    "session: the bed the plan promised is missing from the "
                    "export",
                )
            if state == "ready":
                track_id = bgm_plan.get("trackId")
                if not isinstance(track_id, str) or not track_id.strip():
                    # FALL-09: the bed is shopped with the plan's own
                    # query, and a plan that named only a mood shops with
                    # that - said, not silently defaulted.
                    bed_query = str(bgm_plan.get("query") or bgm_plan.get("mood") or "")
                    if not bgm_plan.get("query") and bgm_plan.get("mood"):
                        log(f"BGM: shopping by mood ({bgm_plan.get('mood')}) - the plan named no query")
                    search = advance("bgm_search", {"query": bed_query, "limit": 5})
                    tracks = search.get("tracks", []) if isinstance(search, dict) else []
                    if not tracks:
                        log(f"BGM skipped: no track answered {bgm_plan.get('query')!r}")
                        _audit("audio.bgm", "rejected",
                               f"no track answered {bgm_plan.get('query')!r}")
                        _signal(0.0, "bgm", provenance="fallback",
                                note=f"no track answered {bgm_plan.get('query')!r}")
                        tracks = []
                    if tracks:
                        track_id = str(tracks[0].get("id") or "")
                        log(f"BGM: {tracks[0].get('title', track_id)}")
                if track_id:
                    imported = advance("bgm_import", {"path": project, "ids": [track_id], "start": 0.0})
                    media_ids = imported.get("mediaIds", []) if isinstance(imported, dict) else []
                    if not media_ids:
                        raise PipelineError("bgm_import returned no media id for the bed")
                    bed_view = imported.get("view") if isinstance(imported, dict) else None
                    bed_clip = _clip_for_media(bed_view, str(media_ids[0]), 0.0)
                    if bed_clip is None:
                        bed_view = transport.call("project_get", {"path": project})
                        bed_clip = _clip_for_media(bed_view, str(media_ids[0]), 0.0)
                    if bed_clip is None:
                        raise PipelineError("the music bed was imported but never placed on the timeline")
                    bed_duration = max(float(bed_clip.get("duration") or runtime), 0.1)
                    duck_db = float(bgm_plan.get("duckDb", -12.0))
                    if "clip_audio_levels_db" in served:
                        # The duck rides the measured speech spans: down
                        # to duckDb under the words, back to unity between
                        # them - keyframes at each boundary, at fractions
                        # of the bed clip's own length.
                        spans = [
                            (float(s["start"]), float(s["end"]))
                            for s in transcript.get("segments", [])
                            if isinstance(s, dict)
                            and isinstance(s.get("start"), (int, float))
                            and isinstance(s.get("end"), (int, float))
                        ]
                        keyframes: list[dict[str, Any]] = []
                        previous_end = 0.0
                        for span_start, span_end in spans:
                            if span_start > previous_end + 0.05:
                                keyframes.append({"at": round(previous_end / bed_duration, 4), "gainDb": 0.0})
                                keyframes.append({"at": round(min(span_start / bed_duration, 1.0), 4), "gainDb": duck_db})
                            else:
                                keyframes.append({"at": round(min(span_start / bed_duration, 1.0), 4), "gainDb": duck_db})
                            previous_end = span_end
                        if previous_end < runtime:
                            keyframes.append({"at": round(min(previous_end / bed_duration, 1.0), 4), "gainDb": duck_db})
                            keyframes.append({"at": 1.0, "gainDb": 0.0})
                        if keyframes:
                            advance("clip_audio_levels_db", {
                                "path": project, "clipId": str(bed_clip["id"]),
                                "baseDb": 0.0, "keyframes": keyframes,
                                "replaceAutomation": True,
                            })
                            log(f"BGM ducked {duck_db:g} dB under {len(spans)} speech span(s)")
                            _audit("audio.bgm", "executed",
                                   "the bed imported, placed and ducked under the measured speech spans")
                    _stage_mark(
                        "bgm", planSha256=plan_sha, clipId=str(bed_clip["id"]),
                    )

    # The timeline as it stands, before the render pays for it: no two
    # clips may share a lane's same seconds, and the read-back is the
    # record of what is about to be exported - the state the Verify
    # stage hashes the project against (VERIFY-14).
    read_back: Any = None
    if "collision_check" in served:
        collisions = advance("collision_check", {"path": project})
        if isinstance(collisions, dict) and collisions.get("clean") is False:
            raise PipelineError(
                "the timeline has collisions: "
                + json.dumps(collisions.get("collisions", []), ensure_ascii=False)[:500]
            )
    if "timeline_read" in served:
        read_back = advance("timeline_read", {"path": project})
        if isinstance(read_back, dict):
            read_clips = read_back.get("clips", [])
            if isinstance(read_clips, list):
                log(f"timeline read-back: {len(read_clips)} clip(s) before export")

    # A render is not complete until the export job reports a terminal
    # state. A previous good export is never overwritten: it is moved
    # aside, stamped, before the new one writes.
    export = plan.get("export") if isinstance(plan.get("export"), dict) else {}

    def export_arguments(destination: str) -> dict[str, Any]:
        """The export call's own arguments, the plan's profile carried
        whole: width, height and frame rate included when the plan named
        them - VERIFY-10 and 11's promise starts here, the export asked
        for what the plan says, then measured for having it."""
        arguments: dict[str, Any] = {
            "path": project,
            "output": destination,
            "crf": int(export.get("crf", 20)),
            "codec": export.get("codec", "h264"),
        }
        for field in ("width", "height", "preset"):
            if export.get(field) is not None:
                arguments[field] = export[field]
        frame_rate = export.get("fps")
        if (
            isinstance(frame_rate, (int, float)) and not isinstance(frame_rate, bool)
            and float(frame_rate) > 0
        ):
            rate = Fraction(float(frame_rate)).limit_denominator(1001)
            arguments["rateNum"] = rate.numerator
            arguments["rateDen"] = rate.denominator
        return arguments

    # IDEM: the export lands under the run's own directory - a prior
    # run's render is never overwritten, because no two runs share a
    # path. A plan that names its own output still gets it (the plan is
    # the promise); the run's own default is run-scoped.
    run_dir = _run_report_dir(project, run_id)
    output = export.get("output")
    if not isinstance(output, str) or not output.strip():
        try:
            os.makedirs(run_dir, exist_ok=True)
            output = os.path.join(run_dir, "output.mp4")
        except OSError:
            output = os.path.join(project, f"funcut-final-{time.strftime('%Y%m%d-%H%M%S')}.mp4")
    if os.path.exists(output):
        backup = f"{output}.bak-{time.strftime('%Y%m%d-%H%M%S')}"
        try:
            os.replace(output, backup)
            log(f"previous export moved aside: {backup}")
        except OSError as error:
            log(f"could not back up the previous export ({error}); the new one will overwrite it")
    started = advance("export_start", export_arguments(output))
    job = started.get("job") if isinstance(started, dict) else None
    if not job:
        raise PipelineError("export_start did not return a job id")
    status: Any = None
    for attempt in range(4):
        status = advance("export_wait", {"job": job, "timeoutSeconds": 590})
        if not isinstance(status, dict) or status.get("state") != "running":
            break
    if not isinstance(status, dict) or status.get("state") == "running":
        raise PipelineError(f"export did not finish within the wait budget: {status}")
    if status.get("state") not in {"done", "completed", "succeeded"}:
        raise PipelineError(f"export finished unsuccessfully: {status}")
    rendered = status.get("output") or output

    export_transcript_job = advance("transcribe", {"path": rendered})
    export_transcript_job_id = (
        export_transcript_job.get("job")
        if isinstance(export_transcript_job, dict) else None
    )
    if not export_transcript_job_id:
        raise PipelineError(
            f"transcribe did not start the exported-file verification job: {export_transcript_job}"
        )
    export_transcript_status: Any = None
    for _ in range(4):
        export_transcript_status = advance(
            "analysis_wait",
            {"job": export_transcript_job_id, "timeoutSeconds": 590},
        )
        if (
            not isinstance(export_transcript_status, dict)
            or export_transcript_status.get("state") != "running"
        ):
            break
    if (
        not isinstance(export_transcript_status, dict)
        or export_transcript_status.get("state") != "done"
    ):
        raise PipelineError(
            f"exported-file transcription did not finish: {export_transcript_status}"
        )
    try:
        exported_transcript = json.loads(export_transcript_status.get("output", ""))
    except (TypeError, json.JSONDecodeError) as error:
        raise PipelineError(f"exported transcription returned invalid JSON: {error}") from error

    # ---- 18/19. The Verify stage's own record: every check the finished
    # export must answer, each one a structured entry that rides the
    # report written beside the export (VERIFY-01..05, 08..14). The sync
    # gate the stage has always run is one entry now; the rest are the
    # render probed and sampled, the project re-read, the captions'
    # placement measured - and the refusal, when one fails, names every
    # id that did.
    verify_entries: list[dict[str, Any]] = []
    probed_path = rendered
    probe = _ffprobe(rendered)
    frames = _sample_frames(rendered, (probe or {}).get("duration"))
    verify_entries.extend(_probe_checks(rendered, plan, export, probe, frames))

    verification = advance(
        "emotion",
        {
            "action": "verify",
            "expected": transcript,
            "actual": exported_transcript,
            "toleranceMs": VERIFICATION_SYNC_TOLERANCE_MS,
        },
    )
    verify_entries.append(_sync_entry(verification))
    if isinstance(verification, dict) and verification.get("verdict") == "pass":
        log(
            f"speech verification passed: matchRate={verification.get('matchRate')} "
            f"maxDriftMs={verification.get('maxDriftMs')} "
            f"(tolerance {VERIFICATION_SYNC_TOLERANCE_MS} ms)"
        )

    # The project as the export left it: the timeline re-read and hashed
    # against the read-back before the render (VERIFY-14), the same
    # source never twice on one lane's same seconds (VERIFY-08), and
    # every clip's media still in the bin and on the disk it names
    # (VERIFY-05).
    timeline_now = (
        advance("timeline_read", {"path": project})
        if "timeline_read" in served else None
    )
    media_now = (
        advance("project_get", {"path": project})
        if "project_get" in served else None
    )
    verify_entries.extend(_timeline_checks(read_back, timeline_now, media_now))
    verify_entries.extend(
        _caption_checks(
            captions_wanted, placed_captions, captions_plan,
            (probe or {}).get("duration"),
        )
    )

    # The loudness the export actually carries, measured and asserted.
    # The platform sets the target (short-form and YouTube sit at -14
    # LUFS, broadcast at -23); a plan's number is a wish, this is the
    # measurement. One normalize pass is attempted before refusing.
    platform = str(export.get("platform") or "").lower()
    target_lufs = -23.0 if platform in ("broadcast", "ebu") else -14.0
    measured_loudness = advance(
        "measure_loudness", {"path": rendered, "targetLufs": target_lufs}
    )
    if not isinstance(measured_loudness, dict) or measured_loudness.get("integratedLufs") is None:
        raise PipelineError(
            "measure_loudness returned no integrated LUFS; the export's loudness "
            "is unproven: " + json.dumps(measured_loudness, ensure_ascii=False)
        )
    integrated = float(measured_loudness["integratedLufs"])
    true_peak = measured_loudness.get("truePeakDbtp")
    true_peak = float(true_peak) if isinstance(true_peak, (int, float)) else None
    log(
        f"loudness: {integrated:.1f} LUFS (target {target_lufs:g}), "
        f"true peak {true_peak if true_peak is None else round(true_peak, 1)} dBTP"
    )

    def loudness_ok() -> bool:
        return abs(integrated - target_lufs) <= 1.0 and (
            true_peak is None or true_peak <= -1.0
        )

    if not loudness_ok() and "normalize_loudness" in served:
        normalized_out = output[: -len(".mp4")] + "-normalized.mp4" if output.endswith(".mp4") else output + "-normalized"
        normalized = advance("normalize_loudness", {
            "audio": rendered, "out": normalized_out,
            "overwrite": True, "targetLufs": target_lufs,
        })
        normalized_file = _reply_file(normalized)
        if normalized_file:
            rendered = normalized_file
            measured_loudness = advance(
                "measure_loudness", {"path": rendered, "targetLufs": target_lufs}
            )
            if isinstance(measured_loudness, dict) and measured_loudness.get("integratedLufs") is not None:
                integrated = float(measured_loudness["integratedLufs"])
                true_peak = measured_loudness.get("truePeakDbtp")
                true_peak = float(true_peak) if isinstance(true_peak, (int, float)) else None
                log(f"loudness after normalize: {integrated:.1f} LUFS, true peak "
                    f"{true_peak if true_peak is None else round(true_peak, 1)} dBTP")

    # VERIFY-13: the absolute ceiling. The loudness gate's own pairing
    # (the -1 dBTP inside loudness_ok) asserts beside it, unchanged.
    verify_entries.append(_true_peak_entry(true_peak))

    # VERIFY-15 (FAIL-03): the export's own speech, read for synthetic-
    # voice artifacts - a disclosure reading, never a proof (the tool's
    # own words). A synthetic band on a take the plan promised as a file
    # is a possible swapped voice, recorded as such; on a tts take it is
    # the expected answer, recorded for the disclosure duty.
    if "detect_ai_voice" not in served:
        verify_entries.append(_verification_entry(
            "VERIFY-15", "ai-voice-disclosure", "skipped",
            "detect_ai_voice is not served; the synthetic-voice read did not run",
        ))
    else:
        try:
            ai_voice = advance("detect_ai_voice", {
                "audio": rendered, "transcript": transcript,
            })
        except PipelineError as error:
            verify_entries.append(_verification_entry(
                "VERIFY-15", "ai-voice-disclosure", "skipped",
                f"detect_ai_voice failed: {error}",
            ))
        else:
            likeness = (
                ai_voice.get("aiLikeness") if isinstance(ai_voice, dict) else None
            )
            band = likeness.get("band") if isinstance(likeness, dict) else None
            score = likeness.get("score") if isinstance(likeness, dict) else None
            voiceover_source = str(
                ((plan.get("audio") or {}).get("voiceover") or {}).get("source") or ""
            ).lower()
            if band is None:
                verify_entries.append(_verification_entry(
                    "VERIFY-15", "ai-voice-disclosure", "skipped",
                    "detect_ai_voice answered without an aiLikeness band",
                ))
            else:
                note = (
                    "the take reads likely synthetic while the plan promised a "
                    "file recording - a swapped voice is worth a human look"
                    if band == "likely synthetic" and voiceover_source == "file"
                    else "recorded for the disclosure duty"
                )
                verify_entries.append(_verification_entry(
                    "VERIFY-15", "ai-voice-disclosure", "pass",
                    note,
                    band=band, likenessScore=score,
                    voiceoverSource=voiceover_source or None,
                ))
                if band == "likely synthetic" and voiceover_source == "file":
                    record_fallback(
                        "verify", "voice", "detect_ai_voice",
                        "the export's speech reads likely synthetic although the "
                        "plan's voiceover source is a file recording; the reading "
                        "informs a look, it never proves a swap",
                    )

    # The whole record, written beside the export before anything may
    # refuse: an acceptance read reads the file, not the log. Then the
    # per-check lines cross, and a failed entry stops the run naming
    # every id that failed - the export did not keep the plan's promise,
    # and the run says so before the craft gate scores anything.
    verify_summary = _verification_summary(verify_entries)
    verify_report = _verification_report(rendered, probed_path, verify_entries)
    verify_report_path = _write_verification_report(project, verify_report)
    for entry in verify_entries:
        log(
            f"verify {entry['id']} {entry['check']}: {entry['status']}"
            + (f" - {entry['reason']}" if entry.get("reason") else "")
        )
    if verify_report_path:
        log(
            f"verification report: {verify_report_path} "
            f"({verify_summary['passed']} passed, {verify_summary['failed']} failed, "
            f"{verify_summary['skipped']} skipped)"
        )
    _verification_failed(verify_entries)

    if not loudness_ok():
        raise PipelineError(
            f"the export carries {integrated:.1f} LUFS against the {target_lufs:g} target "
            f"(true peak {true_peak if true_peak is None else round(true_peak, 1)} dBTP); "
            "fix the mix with clip_audio_levels_db and measure again"
        )

    # The craft gate, run as the loop the motion layer's revision.rs owns
    # (P1-006/007, INT-04/06, PLAN-05): below the ship line exactly the
    # slots the critic named go back for another pass - a B-roll slot
    # re-shops with the old clip excluded, a graphic re-renders with the
    # critic's timing fixes folded into its storyboard - and after two
    # revisions the edit ships with the failures recorded per slot.
    # Shipping with a note beats not shipping.

    def watch_and_critique(path: str, attempt: int) -> tuple[Any, Any, float]:
        """One look at one render: the sheet, then the score. Every
        attempt is watched with the same eyes, and its evidence - score,
        tier, confidence - rides the run record (the plan's signalLog is
        the planning decision log; what a run MEASURED belongs to the
        run, and the craft journal the server keeps carries the rest)."""
        sheet_path = os.path.join(project, f"director-review-v{attempt}.png")
        def inspect(path: str) -> Any:
            return advance(
                "watch_render",
                {
                    "path": path,
                    "samples": 12,
                    "audio": True,
                    "targetLufs": target_lufs,
                    "out": sheet_path,
                },
            )
        try:
            inspection = inspect(path)
        except PipelineError as error:
            # FAIL-02: an empty or corrupt render is a retryable Watch
            # failure, not a low craft score and not a dead critique -
            # the export is re-made once, to the same profile, before
            # anything gives up on it. A render that probed healthy is
            # the tool's own refusal, and stops the run with its words.
            healthy = (
                os.path.isfile(path) and os.path.getsize(path) > 0
                and _ffprobe(path) is not None
            )
            if not healthy:
                log(
                    f"watch_render failed on an unreadable render ({error}); "
                    "re-exporting once before the critique gives up (FAIL-02)"
                )
                started = advance("export_start", export_arguments(path))
                job = started.get("job") if isinstance(started, dict) else None
                if not job:
                    raise PipelineError(
                        f"the render was unreadable and the re-export did not "
                        f"start: {error}"
                    ) from error
                status: Any = None
                for _ in range(4):
                    status = advance("export_wait", {"job": job, "timeoutSeconds": 590})
                    if not isinstance(status, dict) or status.get("state") != "running":
                        break
                if not isinstance(status, dict) or status.get("state") not in {"done", "completed", "succeeded"}:
                    raise PipelineError(
                        f"the render was unreadable and the re-export did not "
                        f"finish: {status}"
                    ) from error
                inspection = inspect(path)
            else:
                raise
        if not isinstance(inspection, dict) or not inspection.get("pngPath"):
            raise PipelineError(
                f"render inspection did not produce a contact sheet: {inspection}"
            )
        for note in (inspection.get("notes") or [])[:5]:
            if isinstance(note, str):
                log(f"watch_render: {note}")
        critique = advance(
            "craft_critique",
            {"sheet": inspection, "project": project},
        )
        score = None
        if isinstance(critique, dict):
            score = critique.get("overall", critique.get("score", critique.get("craftScore")))
        if not isinstance(score, (int, float)):
            raise PipelineError(
                "craft_critique returned no numeric overall score; the render cannot be certified: "
                + json.dumps(critique, ensure_ascii=False)
            )
        if isinstance(critique, dict):
            tier = critique.get("tier") or critique.get("engine")
            confidence = critique.get("confidence")
            log(
                f"craft critique: {score}/10"
                + (f" ({tier}, confidence {confidence})" if tier else "")
                + f" [pass {attempt}]"
            )
            # DEG-04: the tier names who scored the cut; this is what the
            # pairing of tier and confidence MEANS. Below its tier's line
            # the critique is a recorded fallback, not a certificate - the
            # ship line does not move, but the run record says the pass
            # stands on a critic that was not sure of itself.
            if not tier:
                record_fallback(
                    "craft", "critique", "craft_critique",
                    "the critic named no tier: the score has no provenance "
                    "to interpret",
                )
            else:
                threshold = CRAFT_TIER_CONFIDENCE.get(
                    str(tier).strip().lower(), CRAFT_TIER_DEFAULT
                )
                if isinstance(confidence, (int, float)) and not isinstance(confidence, bool):
                    if float(confidence) < threshold:
                        record_fallback(
                            "craft", "critique", "craft_critique",
                            f"critic tier {tier} scored {score}/10 at "
                            f"confidence {float(confidence):.2f}, under "
                            f"the {threshold:g} its tier needs: the pass "
                            "stands, but it is not a certificate",
                            confidence=float(confidence),
                        )
                else:
                    record_fallback(
                        "craft", "critique", "craft_critique",
                        f"critic tier {tier} scored {score}/10 with no "
                        "confidence: the pairing cannot be interpreted, "
                        "only recorded",
                    )
        return inspection, critique, float(score)

    def render_attempt(attempt: int) -> str:
        """A revision renders to its own file - edit_v2, edit_v3 - because
        an attempt nobody can diff against the one before it is an attempt
        nobody learns from (revision.rs: attempt_path). It renders with
        the plan's own export profile, the same arguments the first
        export carried. IDEM: the attempt files live under the run's own
        directory, so no prior run's revisions are overwritten either."""
        try:
            os.makedirs(run_dir, exist_ok=True)
            output = os.path.join(run_dir, f"edit_v{attempt}.mp4")
        except OSError:
            output = os.path.join(project, f"edit_v{attempt}.mp4")
        started = advance("export_start", export_arguments(output))
        job = started.get("job") if isinstance(started, dict) else None
        if not job:
            raise PipelineError("export_start did not return a job id")
        status: Any = None
        for _ in range(4):
            status = advance("export_wait", {"job": job, "timeoutSeconds": 590})
            if not isinstance(status, dict) or status.get("state") != "running":
                break
        if not isinstance(status, dict) or status.get("state") == "running":
            raise PipelineError(f"export did not finish within the wait budget: {status}")
        if status.get("state") not in {"done", "completed", "succeeded"}:
            raise PipelineError(f"revision render finished unsuccessfully: {status}")
        return str(status.get("output") or output)

    def replace_broll_clip(slot_id: str, avoid: str) -> None:
        """One B-roll slot re-shopped and re-placed: the candidates move
        away from what the critic saw (exclude_ids carries the old clip
        and everything before it), the old clip leaves the timeline, and
        the new one inherits the beat's own trim, transition and entrance
        keys - a replacement that lost the beat's craft is no repair."""
        attempt_replace(slot_id, avoid)
        index, segment = slot_segments[slot_id]
        start_at = float(segment["start"])
        target_duration = float(segment["end"]) - start_at
        clip = chosen[slot_id]
        imported = advance(
            "openvid_import",
            {"path": project, "ids": [_clip_int_id(clip["id"], slot_id)], "start": start_at},
        )
        media_ids = imported.get("mediaIds", []) if isinstance(imported, dict) else []
        if not media_ids:
            raise PipelineError(f"{slot_id}: OpenVid import did not return a media id")
        media_id = str(media_ids[0])
        view = imported.get("view") if isinstance(imported, dict) else None
        placed = _clip_for_media(view, media_id, start_at)
        if placed is None:
            view = transport.call("project_get", {"path": project})
            placed = _clip_for_media(view, media_id, start_at)
        if placed is None:
            raise PipelineError(f"{slot_id}: replacement media is not present on the timeline")
        new_clip_id = str(placed["id"])
        actual = float(placed["duration"])
        if actual + 0.05 < target_duration:
            raise PipelineError(
                f"{slot_id}: replacement is {actual:.2f}s, shorter than the "
                f"{target_duration:.2f}s beat"
            )
        if actual > target_duration + 0.05:
            advance(
                "clip_trim",
                {
                    "path": project,
                    "clipId": new_clip_id,
                    "edge": "end",
                    "delta": target_duration - actual,
                    "ripple": False,
                },
            )
        old_clip_id = segment_clip_ids.get(index)
        if (
            old_clip_id
            and old_clip_id != new_clip_id
            and "clips_remove" in served
        ):
            advance(
                "clips_remove",
                {"path": project, "clipIds": [old_clip_id], "ripple": False},
            )
        segment_clip_ids[index] = new_clip_id
        if old_clip_id in placed_clip_ids:
            placed_clip_ids[placed_clip_ids.index(old_clip_id)] = new_clip_id
        if "clip_update" in served:
            arguments = transition_arguments(segment)
            if arguments is not None:
                arguments["clipId"] = new_clip_id
                advance("clip_update", arguments)
        if "keyframe_set" in served:
            for arguments in entrance_arguments(new_clip_id, segment, target_duration):
                advance("keyframe_set", arguments)
        log(
            f"{slot_id}: re-shopped away from {avoid[:60]!r} "
            f"-> clip {new_clip_id}"
        )

    def revise_graphic(
        index: int, key: str, sentences: list[str], attempt: int
    ) -> bool:
        """A graphic beat re-rendered with the critic's timing fixes folded
        into its storyboard: a retime moves the scenes, and anything the
        storyboard cannot carry is said, not pretended - the scene
        builder's own fix applier is not on this wire yet, so the loop
        applies what a storyboard can hold and reports the rest."""
        segment = plan_segments(plan)[index]
        storyboard = segment["visual"].get("storyboard")
        scenes = storyboard.get("scenes") if isinstance(storyboard, dict) else None
        if not isinstance(scenes, list) or not scenes:
            return False
        applied = 0
        for fix in _fixes_for_slot(critique, sentences):
            op = str(fix.get("op") or "").strip().lower()
            patch = fix.get("patch") if isinstance(fix.get("patch"), dict) else {}
            if op != "retime":
                unaddressed.append(
                    f"{key}: the critic's {op or 'fix'} "
                    f"({str(fix.get('change'))[:80]!r}) needs the scene builder's "
                    "applier; recorded, not applied"
                )
                continue
            seconds = None
            if isinstance(patch.get("shift"), (int, float)):
                seconds = float(patch["shift"])
            elif isinstance(patch.get("start"), (int, float)):
                base = scenes[0].get("at") if isinstance(scenes[0], dict) else None
                seconds = float(patch["start"]) - float(base or 0.0)
            else:
                match = _CRITIC_SECONDS.search(str(fix.get("change") or ""))
                seconds = float(match.group(1)) if match else None
            if seconds is None:
                unaddressed.append(
                    f"{key}: a retime with no time with a unit; recorded, not applied"
                )
                continue
            for scene in scenes:
                if isinstance(scene, dict) and isinstance(scene.get("at"), (int, float)):
                    scene["at"] = max(0.0, float(scene["at"]) + seconds)
            applied += 1
        if not applied:
            return False
        start_at = float(segment["start"])
        target_duration = float(segment["end"]) - start_at
        reel = advance(
            "motion_reel",
            {
                "project": project,
                "storyboard": storyboard,
                "name": f"director-segment-{index + 1}-v{attempt}.mp4",
                "crf": 18,
            },
        )
        motion_file = reel.get("file") if isinstance(reel, dict) else None
        rendered_duration = (
            float(reel.get("duration", target_duration)) if isinstance(reel, dict) else target_duration
        )
        if not isinstance(motion_file, str) or not motion_file:
            raise PipelineError(f"{key}: motion_reel returned no rendered file for the revision")
        if rendered_duration + 0.05 < target_duration:
            raise PipelineError(
                f"{key}: revised motion is {rendered_duration:.2f}s, shorter than "
                f"the {target_duration:.2f}s beat"
            )
        imported = advance("media_import", {"path": project, "file": motion_file})
        media_id = imported.get("createdId") if isinstance(imported, dict) else None
        if not isinstance(media_id, str) or not media_id:
            raise PipelineError(f"{key}: media_import returned no media id for the revision")
        placed = advance(
            "clip_add",
            {"path": project, "mediaId": media_id, "trackId": track_id, "start": start_at},
        )
        new_clip_id = placed.get("createdId") if isinstance(placed, dict) else None
        if not isinstance(new_clip_id, str) or not new_clip_id:
            raise PipelineError(f"{key}: clip_add returned no clip id for the revision")
        if rendered_duration > target_duration + 0.05:
            advance(
                "clip_trim",
                {
                    "path": project,
                    "clipId": new_clip_id,
                    "edge": "end",
                    "delta": target_duration - rendered_duration,
                    "ripple": False,
                },
            )
        old_clip_id = graphic_clip_ids.get(index)
        if (
            old_clip_id
            and old_clip_id != new_clip_id
            and "clips_remove" in served
        ):
            advance(
                "clips_remove",
                {"path": project, "clipIds": [old_clip_id], "ripple": False},
            )
        graphic_clip_ids[index] = new_clip_id
        if old_clip_id in placed_clip_ids:
            placed_clip_ids[placed_clip_ids.index(old_clip_id)] = new_clip_id
        log(f"{key}: graphic re-rendered with {applied} retime fix(es) -> clip {new_clip_id}")
        return True

    unaddressed: list[str] = []
    attempt = 1
    inspection, critique, score = watch_and_critique(rendered, 1)
    while score < CRAFT_SHIP_SCORE and attempt < 1 + CRAFT_MAX_REVISIONS:
        named = _critic_slot_map(critique, plan)
        if not named:
            unaddressed.append(
                "the critic scored below the ship line but named no slot this runner can revise"
            )
            break
        attempt += 1
        log(
            f"craft gate: revision pass {attempt - 1} of {CRAFT_MAX_REVISIONS}, "
            f"slot(s) {', '.join(named)}"
        )
        revised = 0
        for key, sentences in named.items():
            if key in slot_segments:
                try:
                    replace_broll_clip(key, "; ".join(sentences)[:120])
                    revised += 1
                except PipelineError as error:
                    unaddressed.append(f"{key}: {error}")
                continue
            index = key.rsplit("-", 1)[-1]
            index = int(index) - 1 if index.isdigit() else -1
            if index in graphic_clip_ids:
                try:
                    if revise_graphic(index, key, sentences, attempt):
                        revised += 1
                    else:
                        unaddressed.append(f"{key}: {' '.join(sentences)[:160]}")
                except PipelineError as error:
                    unaddressed.append(f"{key}: {error}")
            else:
                unaddressed.append(f"{key}: {' '.join(sentences)[:160]}")
        if not revised:
            # Nothing the critic named could be revised; another render
            # would produce the same frames and say so.
            break
        rendered = render_attempt(attempt)
        inspection, critique, score = watch_and_critique(rendered, attempt)

    status = "completed" if score >= CRAFT_SHIP_SCORE else "completed-with-warnings"
    for note in unaddressed:
        log(f"craft warning: {note}")
    degraded_entries = [
        entry for entry in run_signals if entry.get("provenance") == "fallback"
    ]
    log(
        f"pipeline complete: {len(done)} call(s); rendered {rendered}; "
        f"craft score {score}"
        + ("" if status == "completed" else f" ({status})")
        + (f"; {len(degraded_entries)} recorded degradation(s)" if degraded_entries else "")
    )
    # DATA-01: the run's own signalLog is held to its schema before it is
    # written - every entry carries a decision and a provenance, and the
    # provenance is one of the two words it may be. An entry that cannot
    # say what decided or on what footing it stands is not a record.
    for entry in run_signals:
        if not isinstance(entry, dict) or not entry.get("decision"):
            raise PipelineError(
                f"a run signalLog entry carries no decision: {entry!r}"
            )
        if entry.get("provenance") not in ("measured", "fallback"):
            raise PipelineError(
                f"a run signalLog entry's provenance is neither measured nor "
                f"fallback: {entry!r}"
            )
    # The audit's own last word: every plan field accounted for, and the
    # one the runner did not consume named as one (P1-005). The run's
    # signalLog rides the record with it (P1-004).
    _audit("export", "executed",
           f"rendered to {rendered} with the plan's own profile and awaited to done")
    audit = plan_field_audit(plan, plan_audit)
    audit_counts: dict[str, int] = {}
    for entry in audit["fields"].values():
        audit_counts[entry["status"]] = audit_counts.get(entry["status"], 0) + 1
    if audit["unrecognized"]:
        log(
            "plan audit: unrecognized plan field(s) the runner did not consume: "
            + ", ".join(audit["unrecognized"])
        )
    log(
        "plan audit: "
        + ", ".join(
            f"{count} {name}"
            for name, count in sorted(audit_counts.items())
        )
        + (f"; {len(audit['unrecognized'])} unrecognized" if audit["unrecognized"] else "")
        + "; nothing dropped silently"
    )
    # IDEM: the run's own record carries its identity - which run, what
    # it skipped, what its state says stands - and lands whole as
    # runs/<run-id>/report.json, the per-run record two runs can be
    # diffed against each other by.
    run_record = {
        # DATA-04: the run record's own name for its shape, the way the
        # verification report names its - a handoff a later session (or a
        # resume) reads knows which record it is holding.
        "schemaVersion": RUN_RECORD_SCHEMA,
        "runId": run_id,
        "at": time.strftime("%Y-%m-%dT%H:%M:%S"),
        "status": status,
        "calls": len(done),
        "output": rendered,
        "skippedStages": list(skipped_stages),
        "stagesDone": sorted(state_stages),
        "signalLog": run_signals,
        "planAudit": audit,
        "verification": {
            "summary": verify_summary,
            "report": verify_report_path,
            "checks": [entry["id"] for entry in verify_entries],
        },
        "craftScore": score,
        "craftTier": (
            critique.get("tier") or critique.get("engine")
            if isinstance(critique, dict) else None
        ),
        "craftConfidence": (
            critique.get("confidence") if isinstance(critique, dict) else None
        ),
        "craftNotes": _critic_slot_map(critique, plan) if score < CRAFT_SHIP_SCORE else {},
        "unaddressed": unaddressed,
        "lufs": integrated,
    }
    _record_run(project, run_record)
    _stage_mark("export", planSha256=plan_sha, output=rendered)
    try:
        os.makedirs(run_dir, exist_ok=True)
        report_path = os.path.join(run_dir, "report.json")
        with open(report_path, "w", encoding="utf-8") as handle:
            handle.write(json.dumps(run_record, ensure_ascii=False, indent=2) + "\n")
    except OSError as error:
        log(f"run report not written ({error}); the run history line is the record")
    return done


# ------------------------------------------------------ the revision gate

# The ship line the docs promise, said once (SKILL.md: "Craft score below
# 7 means rewrite the plan, not proceed") - the one boundary the runner,
# the skill and the agent's own reading agree on (FALL-04).
CRAFT_SHIP_SCORE = 7.0
# Two revisions, three renders in all - the motion layer's own gate
# (concat-motion/src/revision.rs: MAX_RETRIES), mirrored here so the
# runner and the reel loop mean the same thing by "one more pass".
CRAFT_MAX_REVISIONS = 2

# The least confidence a critique needs before its score decides the run
# (DEG-04): #116 records the critic's tier and confidence, and this table
# is the interpretation - the weaker the critic, the more sure it must be
# before a pass on its word ships. Below the line the critique is a
# recorded fallback, not a certificate; the ship line itself never moves,
# because a weak critic that said 8 is still an 8 that was said.
CRAFT_TIER_CONFIDENCE = {
    "hosted": 0.5,
    "local_vlm": 0.6,
    "local": 0.6,
    "heuristic": 0.75,
}
#: The line for a tier the table does not name: the local tier's own.
CRAFT_TIER_DEFAULT = 0.6

# A time with a unit in a critic's sentence: "the hook at 0.4s", "the
# beat at 12 s", "by 1.5 seconds" - the seconds a note carries are the
# strongest anchor a slot map has.
_CRITIC_SECONDS = re.compile(r"(\d+(?:\.\d+)?)\s*(?:s\b|sec|second)", re.IGNORECASE)


def _critic_sentences(critique: Any) -> list[str]:
    """Every sentence the critic said: the notes, the revisions, and each
    fix's where/problem/change read as one - the slot mapping reads them
    all, because which field a tier fills is the tier's business, not the
    loop's."""
    texts: list[str] = []
    if not isinstance(critique, dict):
        return texts
    for key in ("notes", "revisions"):
        for note in critique.get(key) or []:
            if isinstance(note, str) and note.strip():
                texts.append(note.strip())
    for fix in critique.get("fixes") or []:
        if isinstance(fix, dict):
            text = " ".join(
                str(fix.get(part) or "").strip()
                for part in ("where", "problem", "change")
            ).strip()
            if text:
                texts.append(text)
    return texts


def _critic_slot_map(critique: Any, plan: dict[str, Any]) -> dict[str, list[str]]:
    """The slots the critic's words name, revision.rs's own rule: below
    the ship line exactly the named slots go back for another pass, and a
    note that names no slot is a revision round with nothing named, seen
    plainly rather than the gate inventing work.

    A sentence names a slot three ways, strongest first: by the slot's
    own id ("hook.1 enters late", "segment-3 is black"), by a time the
    segment covers ("the beat at 4.5s"), or by the slot's own words (the
    description, queries or spoken line a sentence shares distinctive
    words with). Each sentence lands on at most one slot."""
    segments = plan_segments(plan)
    broll_keys: dict[int, str] = {}
    for index, segment in broll_segments(plan):
        broll_keys[index] = str(segment.get("slotId") or f"segment-{index + 1}")
    graphic_keys = {
        index: f"segment-{index + 1}" for index, _ in motion_segments(plan)
    }
    key_by_index = {**broll_keys, **graphic_keys}
    explicit_ids: dict[str, int] = {}
    for index, key in key_by_index.items():
        explicit_ids[key.lower()] = index
        slot_id = str(segments[index].get("slotId") or "").strip()
        if slot_id:
            explicit_ids[slot_id.lower()] = index
    named: dict[str, list[str]] = {}
    for sentence in _critic_sentences(critique):
        target: int | None = None
        # The slot's own id, as a whole word: "hook.1", "segment-3".
        for slot_id, index in explicit_ids.items():
            if re.search(rf"(?<![\w.]){re.escape(slot_id)}(?![\w.])", sentence, re.IGNORECASE):
                target = index
                break
        # A time the segment covers: the strongest anchor a sentence
        # without an id carries.
        if target is None:
            for match in _CRITIC_SECONDS.finditer(sentence):
                seconds = float(match.group(1))
                for index, segment in enumerate(segments):
                    if float(segment.get("start", -1)) <= seconds < float(segment.get("end", -1)):
                        if index in key_by_index:
                            target = index
                            break
                if target is not None:
                    break
        # The slot's own words: distinctive shared words, never "the"
        # or "frame" - a description's nouns are its fingerprint.
        if target is None:
            words = {
                word.strip(".,:;!?\"'()").lower()
                for word in sentence.split()
                if len(word) >= 5
            }
            best: tuple[int, int] | None = None
            for index, segment in enumerate(segments):
                if index not in key_by_index:
                    continue
                visual = segment.get("visual") or {}
                vocabulary = " ".join(
                    str(part)
                    for part in (
                        visual.get("description"),
                        segment.get("spoken"),
                        visual.get("query"),
                        *(visual.get("queries") or []),
                    )
                    if part
                ).lower()
                shared = sum(1 for word in words if word in vocabulary)
                if shared >= 2 and (best is None or shared > best[1]):
                    best = (index, shared)
            if best is not None:
                target = best[0]
        if target is not None:
            key = key_by_index[target]
            named.setdefault(key, []).append(sentence)
    return named


def _fixes_for_slot(critique: Any, sentences: list[str]) -> list[dict[str, Any]]:
    """The structured fixes whose own sentence a slot's sentences carry:
    the same where/problem/change join built the sentence, so the exact
    match is the fix's fingerprint."""
    wanted = {sentence.lower() for sentence in sentences}
    fixes: list[dict[str, Any]] = []
    if not isinstance(critique, dict):
        return fixes
    for fix in critique.get("fixes") or []:
        if not isinstance(fix, dict):
            continue
        text = " ".join(
            str(fix.get(part) or "").strip()
            for part in ("where", "problem", "change")
        ).strip()
        if text and text.lower() in wanted:
            fixes.append(fix)
    return fixes


def _record_run(project: str, entry: dict[str, Any]) -> None:
    """The run history, one JSON line a run: what ran, where it stopped,
    what it produced. The re-run question - did this already happen, and
    what did it make - is answered from the file, not from memory."""
    try:
        path = os.path.join(project, ".auto-director-runs.jsonl")
        with open(path, "a", encoding="utf-8") as handle:
            handle.write(json.dumps(entry, ensure_ascii=False) + "\n")
    except OSError as error:
        log(f"run history not written ({error})")


# ---------------------------------------------------------------- the watcher


def media_ids(view: Any) -> set[str]:
    if not isinstance(view, dict):
        return set()
    return {
        item.get("id", item.get("path", ""))
        for item in view.get("media", [])
        if isinstance(item, dict)
    }


def new_audio(view: Any, known: set[str]) -> list[dict[str, Any]]:
    if not isinstance(view, dict):
        return []
    return [
        item
        for item in view.get("media", [])
        if isinstance(item, dict)
        and item.get("id", item.get("path", "")) not in known
        and item.get("kind") == "audio"
    ]


def watch(transport: Any, project: str, plan: dict[str, Any], poll: float) -> None:
    """Poll the project's media list; every new audio file is directed."""
    log(f"watching {project} (media.import is a request: polling every {poll:g}s)")
    view = transport.call("project_get", {"path": project})
    known = media_ids(view)
    log(f"baseline: {len(known)} media item(s) already in the bin")
    try:
        while True:
            time.sleep(poll)
            view = transport.call("project_get", {"path": project})
            arrivals = new_audio(view, known)
            known = media_ids(view)
            for item in arrivals:
                audio = item.get("path", "")
                log(f"new audio: {audio}")
                try:
                    run_pipeline(transport, project, audio, plan, str(item.get("id", "")) or None)
                except PipelineError as error:
                    log(f"STOP: {error}")
                    log("the run stopped at its failure; nothing later ran")
                    _record_run(project, {
                        "at": time.strftime("%Y-%m-%dT%H:%M:%S"),
                        "status": "failed",
                        "audio": audio,
                        "stop": str(error),
                        **_stage_failure_note(project),
                    })
    except KeyboardInterrupt:
        # A cancelled run leaves its partial state named, not guessed at:
        # the log above says exactly which steps ran and which never did.
        log("interrupted: the run stopped where it stood; the run log is the record")
        _record_run(project, {
            "at": time.strftime("%Y-%m-%dT%H:%M:%S"),
            "status": "cancelled",
        })


# ----------------------------------------------------------------- self-test


SELF_TEST_PLAN = {
    "aspect": "16:9",
    "mode": "longform",
    "sections": [
        {"role": "hook", "start": 0.0, "end": 4.0},
        {"role": "body", "start": 4.0, "end": 12.0},
    ],
    "segments": [
        {
            "start": 0.0,
            "end": 4.0,
            "spoken": "The city changes after dark",
            "visual": {
                "type": "broll",
                "query": "cinematic city skyline at dusk",
                "description": "A slow aerial drift across a dense city skyline at dusk",
                "motionMin": 1.0,
            },
            "transitionIn": {"effect": "cross-fade", "seconds": 0.5},
            "in": {"anim": "pop", "seconds": 0.3},
            "sfx": {"query": "cinematic impact", "peakAt": 0.0, "gainDb": -6.0},
            "captions": True,
        },
        {
            "start": 4.0,
            "end": 8.0,
            "spoken": "The evidence becomes clear",
            "visual": {
                "type": "textCard",
                "text": "THE EVIDENCE",
                "storyboard": {
                    "width": 1920,
                    "height": 1080,
                    "fps": 30,
                    "scenes": [{"preset": "funcut.chapter.chip", "hold": 4.0, "at": 0.0}],
                },
            },
        },
        {
            "start": 8.0,
            "end": 12.0,
            "spoken": "The final proof is visible",
            "visual": {"type": "existing", "mediaId": "m-existing"},
        },
    ],
    "audio": {
        "voiceover": {"source": "file", "speed": 1.2,
                      "glossary": "FUNCUT, Kokoro, OpenVid"},
        "bgm": {"query": "city night pulse", "duckDb": -14.0},
    },
    "grade": {"filter": "concat.warm", "intensityKey": "amount", "intensity": 35},
    "captions": {"fontSize": 0.05, "fontWeight": 700, "offsetY": 0.35},
    "creativeBrief": {
        "audience": "curious beginners",
        "platform": "youtube",
        "goal": "understand the story",
        "thesis": "The evidence changes the common explanation",
        "curiosityGap": "What actually happened?",
        "payoff": "The evidence reveals the cause",
        "emotionalArc": ["curiosity", "tension", "clarity"],
    },
    "signalLog": [
        {"at": 0.0, "decision": "cut", "signals": ["emphasisWords"]},
    ],
    "export": {"output": "/tmp/funcut-selftest/final.mp4", "crf": 20, "codec": "h264",
               "width": 320, "height": 180, "fps": 30},
}


SELF_TEST_REPLIES = {
    "concat_version": {"version": "0.2.8-funcut.12", "tools": 110},
    "local_vlm": {"status": "heuristic", "installed": False},
    "enhance_speech": {"ok": True, "file": "/tmp/funcut-selftest/voiceover-enhanced.wav",
                       "noiseFloorDbfs": -22.7, "snrDb": 5.9},
    "remove_hiss_hum_rumble": {"ok": True, "file": "/tmp/funcut-selftest/voiceover-cleaned.wav",
                               "noiseFloorDbfs": -34.1},
    "transcribe": {"job": "speech-job"},
    "analysis_wait": {
        "state": "done",
        'output': '{"segments":[{"text":"one","start":0.0,"end":1.0,'
                  '"words":[{"text":"The","start":0.0,"end":0.4},'
                  '{"text":"city","start":0.4,"end":1.0}]}]}',
    },
    "voice_signals": {
        "status": "ok",
        "degradations": [],
        "signals": {
            "pauses": [
                {"class": "dead_air", "start": 1.2, "end": 2.4},
                {"class": "dramatic_hold", "start": 4.0, "end": 5.1},
                {"class": "dead_air", "start": 6.0, "end": 6.5},
            ]
        },
        "signalConfidence": {"perSignal": [
            {"name": "pauses", "confidence": 0.75, "basis": "silencedetect"},
            {"name": "emphasisWords", "confidence": 0.8, "basis": "token timestamps"},
            {"name": "loudness", "confidence": 0.9, "basis": "ffmpeg ebur128"},
        ]},
    },
    "emotion": lambda args: (
        {"verdict": "pass", "matchRate": 1.0, "maxDriftMs": 0.0}
        if args.get("action") == "verify"
        else {"lines": [{"text": "one", "emotion": "curious"}], "arc": ["curiosity", "clarity"]}
    ),
    "footage_plan_suggest": {
        "gapCount": 1,
        "suggestionCount": 1,
        "cutCount": 4,
        "gaps": [{"startMs": 1000, "endMs": 4000, "topicTags": ["city"]}],
        "suggestions": [{"kind": "dialogue_gap", "needBRoll": True}],
        "cuts": [],
    },
    "detect_music_segments": {"musicPresent": False, "segments": []},
    "identify_music": {"musicPresent": False},
    "plan_validate": {
        "valid": True,
        "creativeReady": True,
        "creativeBlockers": [],
        "signalLog": {"driven": True, "coverage": 1.0, "blockers": [], "problems": []},
        "craftScore": 8.0,
    },
    "openvid_rank_for_slot": {
        "clips": [{
            "id": 7, "name": "city", "motion": 2.0, "camera": "pan_left",
            "source": "pexels", "seconds": 8.0, "slotRank": 1,
        }]
    },
    "stock_contact_sheet": {"pngPath": "/tmp/funcut-selftest/broll.png", "samples": 1},
    "footage_continuity": {"count": 0, "flags": []},
    "openvid_import": {
        "mediaIds": ["m-stock-7"],
        "view": {
            "timeline": {
                "clips": [{
                    "id": "c-stock-7", "mediaId": "m-stock-7",
                    "start": 0.0, "duration": 8.0,
                }]
            }
        },
    },
    "clip_trim": {"ok": True},
    "clip_update": {"ok": True},
    "keyframe_set": {"ok": True},
    "clip_speed": {"ok": True},
    "effect_layer_add": {"ok": True, "createdId": "l-grade"},
    "captions_add": {"ok": True, "count": 1},
    "sounds_search": {
        "sounds": [{
            "id": "impact-glass-heavy", "name": "Glass, heavy impact",
            "category": "Impact", "duration": 1.4, "syncAt": 0.12,
        }],
        "total": 1,
    },
    "sounds_add": {"mediaId": "m-sfx", "clipId": "c-sfx"},
    "clip_audio_levels_db": {"ok": True},
    "bgm_status": {"state": "ready"},
    "bgm_search": {
        "tracks": [{"id": "lofi-0123456789ab", "title": "Night Pulse", "duration": 180.0}],
        "total": 1,
    },
    "bgm_import": {
        "mediaIds": ["m-bgm"],
        "view": {
            "timeline": {
                "clips": [{
                    "id": "c-bgm", "mediaId": "m-bgm",
                    "start": 0.0, "duration": 180.0,
                }]
            }
        },
    },
    "collision_check": {"count": 0, "clean": True, "collisions": []},
    "timeline_read": {
        "clips": [
            {"id": "c-voice", "mediaId": "m-voice-cleaned", "trackId": "t-voice",
             "start": 0.0, "duration": 10.0},
            {"id": "c-stock-7", "mediaId": "m-stock-7", "trackId": "t-video",
             "start": 0.0, "duration": 4.0},
            {"id": "c-motion", "mediaId": "m-motion", "trackId": "t-motion",
             "start": 4.0, "duration": 4.0},
            {"id": "c-existing", "mediaId": "m-existing", "trackId": "t-video",
             "start": 8.0, "duration": 4.0},
            {"id": "c-bgm", "mediaId": "m-bgm", "trackId": "t-bgm",
             "start": 0.0, "duration": 12.0},
            {"id": "c-sfx", "mediaId": "m-sfx", "trackId": "t-sfx",
             "start": 0.0, "duration": 1.4},
        ],
    },
    "track_add": lambda args: {"createdId": "t-voice" if args.get("bottom") else "t-motion"},
    "motion_reel": {"file": "/tmp/funcut-selftest/motion.mp4", "duration": 4.0},
    "media_import": lambda args: {
        "createdId": (
            "m-voice-cleaned" if "voiceover-cleaned" in str(args.get("file", ""))
            else "m-kinetic" if "captions-" in str(args.get("file", ""))
            else "m-motion"
        )
    },
    "clip_add": lambda args: {
        "createdId": (
            "c-voice" if args.get("mediaId") == "m-voice-cleaned"
            else "c-existing" if args.get("mediaId") == "m-existing"
            else "c-kinetic" if args.get("mediaId") == "m-kinetic"
            else "c-motion"
        )
    },
    "captions_animate": {
        "file": "/tmp/funcut-selftest/project/motion/captions-selftest.mp4",
        "duration": 12.0, "lines": 1, "words": 2,
        "width": 1920, "height": 1080, "fps": 30,
        "preset": "funcut.caption.kinetic", "kinetic": True, "warnings": [],
    },
    "export_start": {"job": "j1"},
    "export_wait": {"state": "done", "output": "/tmp/funcut-selftest/final.mp4"},
    "detect_ai_voice": {"ok": True, "aiLikeness": {"score": 12, "band": "likely human"}},
    "measure_loudness": {
        "integratedLufs": -14.2, "truePeakDbtp": -1.4, "loudnessRangeLu": 6.0,
        "targetLufs": -14.0, "lufsError": -0.2, "platformTargetMet": True,
        "notes": [],
    },
    "watch_render": {"pngPath": "/tmp/funcut-selftest/contact.png", "samples": 12, "notes": []},
    "craft_critique": {"overall": 8.5, "scores": {}, "notes": [], "tier": "heuristic",
                       "confidence": 0.5},
}


#: The bin as the run first reads it: the voiceover take and the one
#: existing visual, before anything the run itself imports.
SELF_TEST_BASELINE_BIN = {
    "media": [
        {
            "id": "m-voice",
            "path": "/tmp/funcut-selftest/voiceover.wav",
            "kind": "audio",
        },
        {
            "id": "m-existing",
            "path": "/tmp/funcut-selftest/existing.mp4",
            "kind": "video",
            "duration": 8.0,
        },
    ],
    "timeline": {"tracks": [], "clips": []},
}

#: The bin as the Verify stage re-reads it: everything the run imported -
#: the repaired take, the stock it shopped, the reel it baked, the bed and
#: the SFX - beside the two it started from. The items the mocks have no
#: real file for carry no path; media availability checks exactly what
#: the bin claims, not what a mock pretends.
SELF_TEST_GROWN_BIN = {
    "media": [
        {
            "id": "m-voice",
            "path": "/tmp/funcut-selftest/voiceover.wav",
            "kind": "audio",
        },
        {
            "id": "m-existing",
            "path": "/tmp/funcut-selftest/existing.mp4",
            "kind": "video",
            "duration": 8.0,
        },
        {"id": "m-voice-cleaned", "kind": "audio"},
        {"id": "m-stock-7", "kind": "video"},
        {"id": "m-stock-8", "kind": "video"},
        {"id": "m-stock-9", "kind": "video"},
        {"id": "m-motion", "kind": "video"},
        {"id": "m-bgm", "kind": "audio"},
        {"id": "m-sfx", "kind": "audio"},
    ],
    "timeline": {"tracks": [], "clips": []},
}


def selftest_world() -> dict[str, Any]:
    """A fresh mock world, one per run: the stateful replies start from
    scratch, so two runs never share a call count. The bin reads its
    baseline first - the handoff reads the take it was handed - and every
    later read sees the bin the run's imports grew, the state the Verify
    stage's media-availability check reads. The export answers like a
    renderer does: export_start remembers where the render will land,
    and export_wait, answering done, has written the master render there
    - a file on the disk the verify checks can actually read."""
    world = dict(SELF_TEST_REPLIES)
    reads = {"project_get": 0}
    renders = {"output": None}

    def project_get(_args: dict[str, Any]) -> dict[str, Any]:
        reads["project_get"] += 1
        if reads["project_get"] > 1:
            return SELF_TEST_GROWN_BIN
        return SELF_TEST_BASELINE_BIN

    def export_start(args: dict[str, Any]) -> dict[str, Any]:
        renders["output"] = args.get("output")
        return {"job": "j1"}

    def export_wait(_args: dict[str, Any]) -> dict[str, Any]:
        output = renders["output"]
        master = "/tmp/funcut-selftest/master.mp4"
        if output and os.path.isfile(master):
            try:
                shutil.copyfile(master, output)
            except OSError:
                pass
        return {"state": "done", "output": output}

    world["project_get"] = project_get
    world["export_start"] = export_start
    world["export_wait"] = export_wait
    return world


def selftest_media() -> bool:
    """The self-test's own little world, on real files where the machine
    has ffmpeg: a rendered export with moving frames at the plan's own
    size and rate, a black render for the scan that must catch one, a
    voiceover take and an existing visual the bin can name, and a real
    project folder for the records the run writes. Without ffmpeg the
    files still exist, empty: the probe checks answer `skipped` for them,
    and the run proves the honest skip instead of a pretend pass."""
    root = "/tmp/funcut-selftest"
    os.makedirs(os.path.join(root, "project"), exist_ok=True)
    renders = [
        ["ffmpeg", "-y", "-v", "error",
         "-f", "lavfi", "-i", "testsrc2=size=320x180:rate=30:duration=2",
         "-f", "lavfi", "-i", "sine=frequency=440:duration=2",
         "-af", "volume=-18dB", "-c:v", "libx264", "-pix_fmt", "yuv420p",
         "-c:a", "aac", f"{root}/master.mp4"],
        ["ffmpeg", "-y", "-v", "error",
         "-f", "lavfi", "-i", "color=black:size=320x180:rate=30:duration=2",
         "-f", "lavfi", "-i", "sine=frequency=440:duration=2",
         "-af", "volume=-18dB", "-c:v", "libx264", "-pix_fmt", "yuv420p",
         "-c:a", "aac", f"{root}/final-black.mp4"],
        ["ffmpeg", "-y", "-v", "error",
         "-f", "lavfi", "-i", "sine=frequency=300:duration=12",
         "-ac", "1", f"{root}/voiceover.wav"],
        ["ffmpeg", "-y", "-v", "error",
         "-f", "lavfi", "-i", "testsrc2=size=320x180:rate=30:duration=4",
         "-c:v", "libx264", "-pix_fmt", "yuv420p", f"{root}/existing.mp4"],
    ]
    rendered = bool(shutil.which("ffmpeg"))
    if rendered:
        for command in renders:
            try:
                if subprocess.run(
                    command, capture_output=True, timeout=120
                ).returncode != 0:
                    rendered = False
                    break
            except (OSError, subprocess.SubprocessError):
                rendered = False
                break
    for name in ("master.mp4", "final-black.mp4", "voiceover.wav", "existing.mp4"):
        path = os.path.join(root, name)
        if not rendered and os.path.exists(path):
            os.remove(path)
        if not os.path.exists(path):
            with open(path, "wb"):
                pass
    if rendered:
        log("self-test media rendered with ffmpeg: the verify scans run for real")
    else:
        log("self-test media is empty: the probe checks answer skipped, not passed")
    return rendered


def self_test() -> int:
    """A synthetic canonical plan exercises the real execution contract."""
    have_media = selftest_media()
    project = "/tmp/funcut-selftest/project"
    audio = "/tmp/funcut-selftest/voiceover.wav"
    # A self-test starts as a first run, not a resume of whatever the
    # last invocation's final scenario left in the shared project dir.
    _scenario_state_clear(project)
    transport = MockMcp(selftest_world())
    calls = run_pipeline(transport, project, audio, SELF_TEST_PLAN)
    by_tool = [(tool, args) for tool, args in calls]
    order = [tool for tool, _ in by_tool]
    expected = [
        "concat_version", "local_vlm",
        "enhance_speech", "remove_hiss_hum_rumble",
        "transcribe", "analysis_wait", "voice_signals", "emotion",
        "footage_plan_suggest", "detect_music_segments", "identify_music",
        "plan_validate",
        "project_get", "media_import", "track_add", "clip_add", "clip_speed",
        "openvid_rank_for_slot", "stock_contact_sheet", "footage_continuity",
        "openvid_import", "clip_trim",
        "track_add", "motion_reel", "media_import", "clip_add", "clip_add", "clip_trim",
        "clip_update", "keyframe_set", "keyframe_set",
        "effect_layer_add",
        "captions_add",
        "sounds_search", "sounds_add", "clip_audio_levels_db",
        "bgm_status", "bgm_search", "bgm_import", "clip_audio_levels_db",
        "collision_check", "timeline_read",
        "export_start", "export_wait", "transcribe", "analysis_wait", "emotion",
        "timeline_read", "project_get",
        "measure_loudness", "detect_ai_voice", "watch_render", "craft_critique",
    ]
    if order != expected:
        print(f"FAIL: phase order\n  want: {expected}\n  got:  {order}")
        return 1
    print(f"order ok: {' -> '.join(order)}")

    voice_track = next(
        args for tool, args in by_tool
        if tool == "track_add" and args.get("bottom") is True
    )
    voice_placement = next(
        args for tool, args in by_tool
        if tool == "clip_add" and args.get("mediaId") == "m-voice-cleaned"
    )
    if voice_track.get("path") != project or voice_placement.get("trackId") != "t-voice" or voice_placement.get("start") != 0.0:
        print(f"FAIL: voiceover was not placed on its own bottom lane: {voice_track}, {voice_placement}")
        return 1
    if "clip_cut_spans" in order:
        print("FAIL: the runner changed voiceover timing behind the plan's back")
        return 1
    speed = next(args for tool, args in by_tool if tool == "clip_speed")
    if speed.get("clipId") != "c-voice" or speed.get("speed") != 1.2:
        print(f"FAIL: the plan's voiceover speed was not applied: {speed}")
        return 1
    print("voiceover ok: repaired take placed on its own lane, speed 1.2x applied, plan timing untouched")

    transcribed = next(args for tool, args in by_tool if tool == "transcribe")
    if transcribed.get("path") != "/tmp/funcut-selftest/voiceover-cleaned.wav":
        print(f"FAIL: transcription did not read the repaired take: {transcribed}")
        return 1
    if transcribed.get("initialPrompt") != "FUNCUT, Kokoro, OpenVid":
        print(f"FAIL: the glossary was not handed to the recogniser: {transcribed}")
        return 1
    print("repair ok: the readings and the timeline both run on the repaired, cleaned take")

    rank = next(args for tool, args in by_tool if tool == "openvid_rank_for_slot")
    if rank["description"] != "A slow aerial drift across a dense city skyline at dusk":
        print(f"FAIL: rank did not use the segment's creative description: {rank}")
        return 1
    imported = next(args for tool, args in by_tool if tool == "openvid_import")
    if imported.get("ids") != [7] or imported.get("start") != 0.0:
        print(f"FAIL: selected footage was not imported at its segment start: {imported}")
        return 1
    sheet = next(args for tool, args in by_tool if tool == "stock_contact_sheet")
    if sheet.get("ids") != [7] or sheet.get("rank") is not True:
        print(f"FAIL: the chosen candidates were not seen before import: {sheet}")
        return 1
    print("stock ok: ranked, seen on a contact sheet, and imported at 0.0s")

    trim = next(args for tool, args in by_tool if tool == "clip_trim")
    if trim.get("clipId") != "c-stock-7" or trim.get("delta") != -4.0:
        print(f"FAIL: footage was not trimmed to the segment duration: {trim}")
        return 1
    transition = next(
        args for tool, args in by_tool
        if tool == "clip_update" and "transitionIn" in (args.get("patch") or {})
    )
    if transition.get("clipId") != "c-stock-7" or transition["patch"]["transitionIn"]["id"] != "cross-fade":
        print(f"FAIL: the plan's transition was not applied to the segment's clip: {transition}")
        return 1
    if transition["patch"]["transitionIn"].get("duration") != 0.5:
        print(f"FAIL: the transition's duration was not the plan's own: {transition}")
        return 1
    keys = [args for tool, args in by_tool if tool == "keyframe_set"]
    if len(keys) != 2 or keys[0].get("property") != "scale" or keys[0].get("value") != 0.6:
        print(f"FAIL: the entrance was not animated on the placed clip: {keys}")
        return 1
    print("timing ok: trimmed to the beat, transition on the cut, entrance keyed")

    motion_call = next(args for tool, args in by_tool if tool == "motion_reel")
    if motion_call.get("storyboard", {}).get("scenes", [{}])[0].get("preset") != "funcut.chapter.chip":
        print(f"FAIL: motion storyboard was not passed to the renderer: {motion_call}")
        return 1
    placement = next(
        args for tool, args in by_tool
        if tool == "clip_add" and args.get("mediaId") == "m-motion"
    )
    if placement.get("mediaId") != "m-motion" or placement.get("trackId") != "t-motion" or placement.get("start") != 4.0:
        print(f"FAIL: rendered motion was not placed on the dedicated lane: {placement}")
        return 1
    print("motion ok: storyboard rendered, imported, and placed on a top overlay track")

    existing_place = next(
        args for tool, args in by_tool
        if tool == "clip_add" and args.get("mediaId") == "m-existing"
    )
    existing_trim = next(
        args for tool, args in by_tool
        if tool == "clip_trim" and args.get("clipId") == "c-existing"
    )
    if existing_place.get("trackId") != "t-motion" or existing_place.get("start") != 8.0:
        print(f"FAIL: existing media was not placed on the overlay lane: {existing_place}")
        return 1
    if existing_trim.get("delta") != -4.0:
        print(f"FAIL: existing media was not trimmed to its segment: {existing_trim}")
        return 1
    print("existing visual ok: project media placed and trimmed to its timed segment")

    grade = next(args for tool, args in by_tool if tool == "effect_layer_add")
    if grade.get("effectId") != "concat.warm" or grade.get("duration") != 12.0 or grade.get("start") != 0.0:
        print(f"FAIL: the plan's grade was not layered over the whole runtime: {grade}")
        return 1
    captions = next(args for tool, args in by_tool if tool == "captions_add")
    if captions.get("captions") != [{"start": 0.0, "end": 1.0, "text": "one"}]:
        print(f"FAIL: captions were not placed from the measured transcript: {captions}")
        return 1
    sfx_added = next(args for tool, args in by_tool if tool == "sounds_add")
    if sfx_added.get("id") != "impact-glass-heavy" or sfx_added.get("start") != 0.0:
        print(f"FAIL: the SFX was not placed with its peak on the beat: {sfx_added}")
        return 1
    sfx_gain = next(
        args for tool, args in by_tool
        if tool == "clip_audio_levels_db" and args.get("clipId") == "c-sfx"
    )
    if sfx_gain.get("baseDb") != -6.0:
        print(f"FAIL: the SFX gain was not the plan's own: {sfx_gain}")
        return 1
    duck = next(
        args for tool, args in by_tool
        if tool == "clip_audio_levels_db" and args.get("clipId") == "c-bgm"
    )
    frames = duck.get("keyframes") or []
    if not frames or frames[0].get("gainDb") != -14.0 or frames[-1].get("gainDb") != 0.0:
        print(f"FAIL: the music bed was not ducked under the words: {duck}")
        return 1
    print("enrichment ok: grade layered, captions placed, SFX peaked on beat, bed ducked")

    collisions = next(args for tool, args in by_tool if tool == "collision_check")
    if collisions.get("path") != project:
        print(f"FAIL: the timeline was not collision-checked: {collisions}")
        return 1
    print("pre-export ok: collisions checked, timeline read back")

    waited = next(args for tool, args in by_tool if tool == "export_wait")
    if waited.get("job") != "j1":
        print(f"FAIL: export job was not awaited: {waited}")
        return 1
    verified = next(args for tool, args in by_tool if tool == "emotion" and args.get("action") == "verify")
    if verified.get("action") != "verify" or verified.get("toleranceMs") != 60:
        print(f"FAIL: exported speech was not verified against the source: {verified}")
        return 1
    loudness = next(args for tool, args in by_tool if tool == "measure_loudness")
    if loudness.get("path") != "/tmp/funcut-selftest/final.mp4" or loudness.get("targetLufs") != -14.0:
        print(f"FAIL: the export's loudness was not measured against the platform target: {loudness}")
        return 1
    print("export ok: awaited, speech verified to 60 ms, loudness measured and in target")

    # The run's own records (P1-004, P1-005): the signalLog the run kept
    # and the audit of every plan field it consumed, both read back from
    # the run history the run wrote.
    try:
        with open(os.path.join(project, ".auto-director-runs.jsonl"), encoding="utf-8") as handle:
            recorded = [json.loads(line) for line in handle if line.strip()]
    except (OSError, ValueError) as error:
        print(f"FAIL: the run history was not written: {error}")
        return 1
    if not recorded or recorded[-1].get("status") != "completed":
        print(f"FAIL: the run's own record is missing or wrong: {recorded[-1:] }")
        return 1
    run_record = recorded[-1]
    signals = run_record.get("signalLog") or []
    coverage = [entry for entry in signals if entry.get("decision") == "broll-coverage"]
    if len(coverage) != 1 or coverage[0].get("slotId") != "segment-1":
        print(f"FAIL: the per-slot planner record is missing: {signals}")
        return 1
    if coverage[0].get("signals") != ["dialogue_gap"] or coverage[0].get("provenance") != "measured":
        print(f"FAIL: the slot's planner evidence is wrong: {coverage[0]}")
        return 1
    audit = run_record.get("planAudit") or {}
    audited = audit.get("fields") or {}
    if len(audited) != len(PLAN_FIELDS):
        print(f"FAIL: the audit does not answer for every field: {sorted(audited)}")
        return 1
    if audit.get("unrecognized"):
        print(f"FAIL: fields the runner does not know went unnamed: {audit['unrecognized']}")
        return 1
    expected_executed = {
        "aspect", "mode", "sections", "segments", "segments.visual",
        "segments.visual.existing", "segments.transitionIn", "segments.in/out",
        "segments.sfx", "audio.voiceover", "audio.bgm", "grade", "captions",
        "signalLog", "creativeBrief", "export",
    }
    executed = {field for field, entry in audited.items() if entry.get("status") == "executed"}
    if not expected_executed <= executed:
        print(f"FAIL: fields the run consumed were not audited as executed: "
              f"{sorted(expected_executed - executed)}")
        return 1
    if audited.get("audioClipId", {}).get("status") != "skipped":
        print(f"FAIL: the absent audioClipId was not audited as skipped: {audited.get('audioClipId')}")
        return 1
    print(
        "run records ok: the slot's planner evidence and the 19-field audit "
        f"rode the run ({len(executed)} executed, "
        f"{sum(1 for e in audited.values() if e.get('status') == 'skipped')} skipped)"
    )

    # ---- IDEM: the run twice, and the resume after a failed stage
    # (IDEM-01..10). The plan's export is unpinned so each run's render
    # is run-scoped; the second run's world is the project as the first
    # run left it - the placements the first run made are on the
    # timeline, exactly what a real resume reads back.
    twice_plan = copy.deepcopy(SELF_TEST_PLAN)
    twice_plan.pop("export", None)
    # The repaired take the first run's repair step claims to have
    # written: the mock replies with the path without writing the file,
    # so the test writes it - the resumed run's bin, the repair's reuse
    # check and VERIFY-05 all read the disk, and the world must not name
    # a file it never made. The path is the mock's own claimed one.
    cleaned_path = "/tmp/funcut-selftest/voiceover-cleaned.wav"
    try:
        os.makedirs(os.path.dirname(cleaned_path), exist_ok=True)
        shutil.copyfile(audio, cleaned_path)
    except OSError:
        pass
    _scenario_state_clear(project)
    first_world = selftest_world()
    first_calls = run_pipeline(MockMcp(first_world), project, audio, twice_plan)
    first_record = None
    with open(os.path.join(project, ".auto-director-runs.jsonl"), encoding="utf-8") as handle:
        first_record = [json.loads(line) for line in handle if line.strip()][-1]
    first_output = first_record.get("output") or ""
    if os.sep + RUNS_DIR_NAME + os.sep not in first_output:
        print(f"FAIL: the unpinned export is not run-scoped: {first_output}")
        return 1
    first_report = os.path.join(
        _run_report_dir(project, str(first_record.get("runId"))), "report.json"
    )
    if not os.path.isfile(first_report):
        print(f"FAIL: the run wrote no runs/<id>/report.json: {first_report}")
        return 1

    resumed_media = [dict(item) for item in SELF_TEST_GROWN_BIN["media"]]
    for item in resumed_media:
        if item["id"] == "m-voice-cleaned":
            item["path"] = cleaned_path
    # The clip ids are the ones the mock's own placements answered with:
    # the resume re-reads a project the first run actually built.
    resumed_view = {
        "media": resumed_media,
        "timeline": {"clips": [
            {"id": "c-voice", "kind": "audio", "media": "m-voice-cleaned",
             "start": 0.0, "duration": 10.0},
            {"id": "c-stock-7", "kind": "video", "media": "m-stock-7",
             "start": 0.0, "duration": 8.0},
            {"id": "c-motion", "kind": "video", "media": "m-motion",
             "start": 8.0, "duration": 4.0},
            {"id": "c-existing", "kind": "video", "media": "m-existing",
             "start": 4.0, "duration": 4.0},
            {"id": "c-sfx", "kind": "audio", "media": "m-sfx",
             "start": 1.0, "duration": 1.4},
            {"id": "c-bgm", "kind": "audio", "media": "m-bgm",
             "start": 0.0, "duration": 180.0},
        ]},
    }
    resumed_world = selftest_world()
    resumed_world["project_get"] = lambda _args: resumed_view
    second_calls = run_pipeline(MockMcp(resumed_world), project, audio, twice_plan)
    second_tools = [tool for tool, _ in second_calls]
    duplicated = {
        "openvid_import": "the B-roll re-imported",
        "track_add": "a second lane opened",
        "motion_reel": "the graphic re-rendered",
        "captions_add": "the captions doubled",
        "sounds_add": "the SFX re-placed",
        "bgm_import": "the bed re-imported",
        "clip_speed": "the speed compounded",
        "keyframe_set": "the keys re-set",
        "clip_update": "the transition re-patched",
        "effect_layer_add": "the grade stacked a second layer",
    }
    for tool, complaint in duplicated.items():
        if tool in second_tools:
            print(f"FAIL: the resumed run duplicated work - {complaint} ({tool})")
            return 1
    with open(os.path.join(project, ".auto-director-runs.jsonl"), encoding="utf-8") as handle:
        second_record = [json.loads(line) for line in handle if line.strip()][-1]
    if not second_record.get("skippedStages"):
        print(f"FAIL: the resumed run recorded no skipped stages: {second_record}")
        return 1
    second_output = second_record.get("output") or ""
    if second_output == first_output:
        print(f"FAIL: the second run overwrote the first's export: {second_output}")
        return 1
    second_report = os.path.join(
        _run_report_dir(project, str(second_record.get("runId"))), "report.json"
    )
    if not os.path.isfile(second_report):
        print(f"FAIL: the resumed run wrote no report: {second_report}")
        return 1
    print(
        f"run twice ok: {len(first_calls)} calls then {len(second_calls)}, "
        f"{len(second_record['skippedStages'])} stage(s) reused, two exports, two reports"
    )

    # The resume after a failed stage: the captions tool fails once, the
    # run stops with its completed stages recorded, and the next run
    # skips exactly those and places what failed.
    _scenario_state_clear(project)
    caption_fails = {"count": 0}

    def failing_captions(_args: dict[str, Any]) -> dict[str, Any]:
        caption_fails["count"] += 1
        if caption_fails["count"] == 1:
            raise PipelineError("captions_add: the tool failed (as arranged)")
        return {"ok": True, "count": 1}

    failed_world = selftest_world()
    failed_world["captions_add"] = failing_captions
    try:
        run_pipeline(MockMcp(failed_world), project, audio, twice_plan)
        print("FAIL: the arranged captions failure did not stop the run")
        return 1
    except PipelineError as error:
        if "as arranged" not in str(error):
            print(f"FAIL: the run stopped for the wrong reason: {error}")
            return 1
    failed_state = _load_run_state(project)
    failed_stages = set(failed_state.get("stages") or {})
    for wanted in ("transcribe", "voice-signals", "gather", "timeline"):
        if wanted not in failed_stages:
            print(f"FAIL: the failed run's state lacks the completed {wanted}: {sorted(failed_stages)}")
            return 1
    if "captions" in failed_stages:
        print("FAIL: the failed stage was recorded as complete")
        return 1
    resume_calls = run_pipeline(MockMcp(resumed_world), project, audio, twice_plan)
    resume_tools = [tool for tool, _ in resume_calls]
    for tool in ("openvid_import", "track_add", "motion_reel", "keyframe_set"):
        if tool in resume_tools:
            print(f"FAIL: the resume re-ran a completed stage's work ({tool})")
            return 1
    if "captions_add" not in resume_tools:
        print("FAIL: the resume did not place the captions the failure ate")
        return 1
    with open(os.path.join(project, ".auto-director-runs.jsonl"), encoding="utf-8") as handle:
        resume_record = [json.loads(line) for line in handle if line.strip()][-1]
    if resume_record.get("status") != "completed":
        print(f"FAIL: the resumed run did not complete: {resume_record.get('status')}")
        return 1
    print("resume ok: the failed stage's work was placed, the completed stages were not re-run")

    # A planner the server does not serve: every slot carries the
    # degradation, provenance fallback, in the run's signalLog (P1-004).
    unplanned = selftest_world()
    unplanned.pop("footage_plan_suggest", None)
    _scenario_state_clear(project)
    unplanned_run = run_pipeline(MockMcp(unplanned), project, audio, SELF_TEST_PLAN)
    if not unplanned_run:
        print("FAIL: the run without a planner did not complete")
        return 1
    with open(os.path.join(project, ".auto-director-runs.jsonl"), encoding="utf-8") as handle:
        degraded = [json.loads(line) for line in handle if line.strip()][-1]
    degraded_coverage = [
        entry for entry in degraded.get("signalLog") or []
        if entry.get("decision") == "broll-coverage"
    ]
    if len(degraded_coverage) != 1 or degraded_coverage[0].get("provenance") != "fallback":
        print(f"FAIL: the unserved planner's degradation was not recorded per slot: {degraded_coverage}")
        return 1
    print("planner degradation ok: an unserved planner is recorded per slot, provenance fallback")

    # A plan field the runner has never met: named, never dropped (P1-005).
    strange = copy.deepcopy(SELF_TEST_PLAN)
    strange["mysteryField"] = {"anything": True}
    strange_audit = plan_field_audit(strange, {})
    if strange_audit["unrecognized"] != ["mysteryField"]:
        print(f"FAIL: an unknown plan field went unnamed: {strange_audit['unrecognized']}")
        return 1
    print("audit ok: an unknown plan field is named unrecognized, never dropped")

    # Word-timed kinetic captions, asked for by the plan (P1-010): the
    # measured word timings are the words, the plan's kinetic setting
    # rides along, the reel is baked, imported and placed on the overlay
    # lane, and the timeline's segment-timed captions do not double it.
    kinetic_plan = copy.deepcopy(SELF_TEST_PLAN)
    kinetic_plan["captions"]["wordTimed"] = True
    kinetic_world = selftest_world()
    kinetic_transport = MockMcp(kinetic_world)
    _scenario_state_clear(project)
    kinetic_calls = run_pipeline(kinetic_transport, project, audio, kinetic_plan)
    animate_call = next(
        (args for tool, args in kinetic_calls if tool == "captions_animate"), None,
    )
    if animate_call is None:
        print("FAIL: the plan's kinetic captions were never baked")
        return 1
    if animate_call.get("words") != [["The", 0.0, 0.4], ["city", 0.4, 1.0]]:
        print(f"FAIL: the bake did not carry the measured word timings: {animate_call}")
        return 1
    if animate_call.get("kinetic") is not True or animate_call.get("project") != project:
        print(f"FAIL: the bake did not carry the plan's kinetic setting and project: {animate_call}")
        return 1
    if any(tool == "captions_add" for tool, _ in kinetic_calls):
        print("FAIL: segment-timed captions doubled the kinetic reel")
        return 1
    kinetic_placement = next(
        (
            args for tool, args in kinetic_calls
            if tool == "clip_add" and args.get("mediaId") == "m-kinetic"
        ),
        None,
    )
    if kinetic_placement is None or kinetic_placement.get("trackId") != "t-motion" \
            or kinetic_placement.get("start") != 0.0:
        print(f"FAIL: the kinetic reel was not placed on the overlay lane: {kinetic_placement}")
        return 1
    kinetic_import = next(
        (
            args for tool, args in kinetic_calls
            if tool == "media_import" and "captions-" in str(args.get("file", ""))
        ),
        None,
    )
    if kinetic_import is None:
        print("FAIL: the baked reel was never imported")
        return 1
    print("kinetic captions ok: baked from the measured words, placed once, never doubled")

    # Kinetic captions asked for, word timings absent: the honest
    # fallback - segment-timed captions placed, the degradation recorded
    # in the run's signalLog with provenance fallback (P1-010). The
    # scenario's take is the same plan over the same audio as the bake
    # above, so the state that bake wrote is stood down first: this
    # scenario tests the wordless transcript itself, not the resume.
    wordless = copy.deepcopy(SELF_TEST_PLAN)
    wordless["captions"]["wordTimed"] = True
    _scenario_state_clear(project)
    wordless_world = selftest_world()
    wordless_world["analysis_wait"] = {
        "state": "done",
        'output': '{"segments":[{"text":"one","start":0.0,"end":1.0}]}',
    }
    wordless_calls = run_pipeline(MockMcp(wordless_world), project, audio, wordless)
    if not any(tool == "captions_add" for tool, _ in wordless_calls):
        print("FAIL: the wordless kinetic fallback placed no captions at all")
        return 1
    with open(os.path.join(project, ".auto-director-runs.jsonl"), encoding="utf-8") as handle:
        wordless_record = [json.loads(line) for line in handle if line.strip()][-1]
    wordless_signal = [
        entry for entry in wordless_record.get("signalLog") or []
        if entry.get("decision") == "captions"
    ]
    if len(wordless_signal) != 1 or wordless_signal[0].get("provenance") != "fallback":
        print(f"FAIL: the wordless kinetic degradation was not recorded: {wordless_signal}")
        return 1
    print("kinetic fallback ok: no word timings, segment-timed captions placed, the degradation recorded")

    # The Verify stage's own record: twelve structured entries, the report
    # written beside the export, every entry's status the truth the
    # environment could measure (VERIFY-01..05, 08..14).
    try:
        with open(os.path.join(project, "verification-report.json"), encoding="utf-8") as handle:
            report = json.load(handle)
    except (OSError, ValueError) as error:
        print(f"FAIL: the verification report was not written: {error}")
        return 1
    if report.get("schema") != "funcut.verification-report/1":
        print(f"FAIL: the report's schema is wrong: {report.get('schema')}")
        return 1
    entries = report.get("checks", [])
    by_check = {entry.get("id"): entry for entry in entries}
    wanted_ids = [f"VERIFY-{number:02d}" for number in (1, 2, 3, 4, 5, 8, 9, 10, 11, 12, 13, 14, 15)]
    if sorted(entry.get("id") for entry in entries) != wanted_ids:
        print(f"FAIL: the report's checks are not the thirteen the stage owes: "
              f"{[entry.get('id') for entry in entries]}")
        return 1
    probe_ids = ("VERIFY-01", "VERIFY-04", "VERIFY-09", "VERIFY-10", "VERIFY-11")
    for check_id in wanted_ids:
        status = by_check[check_id].get("status")
        expected_status = "pass" if have_media or check_id not in probe_ids else "skipped"
        if status != expected_status:
            print(f"FAIL: {check_id} answered {status!r}, not {expected_status!r}: "
                  f"{by_check[check_id]}")
            return 1
    summary = report.get("summary", {})
    if summary.get("total") != 13 or summary.get("failed") != 0:
        print(f"FAIL: the report's summary is wrong: {summary}")
        return 1
    print(
        "verify record ok: 13 entries, report written, "
        + f"{summary.get('passed')} passed / {summary.get('skipped')} skipped "
        + "(probe checks skipped: no ffmpeg)" * (not have_media)
    )

    # A caption the frame cannot hold: the plan asks for the bottom edge,
    # the entry names the band that crosses it, the run stops before the
    # craft gate scores anything.
    off_frame_plan = copy.deepcopy(SELF_TEST_PLAN)
    off_frame_plan["captions"]["offsetY"] = 1.2
    _scenario_state_clear(project)
    try:
        run_pipeline(MockMcp(selftest_world()), project, audio, off_frame_plan)
        print("FAIL: an off-frame caption shipped without a refusal")
        return 1
    except PipelineError as error:
        if "VERIFY-12" not in str(error):
            print(f"FAIL: the off-frame caption was refused for the wrong reason: {error}")
            return 1
    print("caption frame ok: an off-frame band is refused as VERIFY-12, naming the band")

    # A plan that asks for captions but has none to place: the entry says
    # so, and the refusal names it.
    _scenario_state_clear(project)
    no_caption_transcript = selftest_world()
    no_caption_transcript["analysis_wait"] = {
        "state": "done",
        'output': '{"segments":[]}',
    }
    try:
        run_pipeline(MockMcp(no_caption_transcript), project, audio, SELF_TEST_PLAN)
        print("FAIL: a captionless export shipped without a refusal")
        return 1
    except PipelineError as error:
        if "VERIFY-02" not in str(error):
            print(f"FAIL: the missing captions were refused for the wrong reason: {error}")
            return 1
    print("caption presence ok: captions wanted and none placed is refused as VERIFY-02")

    # A drifted timeline: something edited the project after the export's
    # edit was final; the hash names it, the run refuses.
    _scenario_state_clear(project)
    drifted_world = selftest_world()
    timeline_reads = {"count": 0}

    def drifting_timeline(_args: dict[str, Any]) -> dict[str, Any]:
        timeline_reads["count"] += 1
        view = {"clips": [dict(clip) for clip in SELF_TEST_REPLIES["timeline_read"]["clips"]]}
        if timeline_reads["count"] > 1:
            view["clips"][1]["start"] = 0.5
        return view

    drifted_world["timeline_read"] = drifting_timeline
    try:
        run_pipeline(MockMcp(drifted_world), project, audio, SELF_TEST_PLAN)
        print("FAIL: a timeline that drifted after the final edit shipped without a refusal")
        return 1
    except PipelineError as error:
        if "VERIFY-14" not in str(error):
            print(f"FAIL: the drifted timeline was refused for the wrong reason: {error}")
            return 1
    print("post-edit hash ok: a timeline that moved after the final edit is refused as VERIFY-14")

    # The same source twice on one lane's same seconds: the entry names
    # the pair and the lane, the run refuses.
    _scenario_state_clear(project)
    duplicated_world = selftest_world()
    duplicated_world["timeline_read"] = {
        "clips": SELF_TEST_REPLIES["timeline_read"]["clips"] + [
            {"id": "c-stock-7-again", "mediaId": "m-stock-7", "trackId": "t-video",
             "start": 1.0, "duration": 3.0},
        ],
    }
    try:
        run_pipeline(MockMcp(duplicated_world), project, audio, SELF_TEST_PLAN)
        print("FAIL: the same clip twice on one lane shipped without a refusal")
        return 1
    except PipelineError as error:
        if "VERIFY-08" not in str(error):
            print(f"FAIL: the duplicated clip was refused for the wrong reason: {error}")
            return 1
    print("duplicate lane ok: the same source twice on one lane is refused as VERIFY-08")

    # Media the bin no longer holds: the clip references an id the re-read
    # does not answer, and the entry names the id.
    _scenario_state_clear(project)
    vanished_world = selftest_world()
    vanished_world["project_get"] = lambda _args: {
        "media": [
            item for item in SELF_TEST_GROWN_BIN["media"]
            if item["id"] != "m-stock-7"
        ],
        "timeline": {"tracks": [], "clips": []},
    }
    try:
        run_pipeline(MockMcp(vanished_world), project, audio, SELF_TEST_PLAN)
        print("FAIL: media that vanished from the bin shipped without a refusal")
        return 1
    except PipelineError as error:
        if "VERIFY-05" not in str(error):
            print(f"FAIL: the vanished media was refused for the wrong reason: {error}")
            return 1
    print("media availability ok: a clip whose source left the bin is refused as VERIFY-05")

    # A sync refusal that is the entry's, not a crash's: the verdict fails,
    # the entry carries the tool's own numbers, the refusal names VERIFY-03.
    _scenario_state_clear(project)
    unsynced_world = selftest_world()
    unsynced_world["emotion"] = lambda args: (
        {"verdict": "fail", "matchRate": 0.62, "maxDriftMs": 340.0}
        if args.get("action") == "verify"
        else {"lines": [{"text": "one", "emotion": "curious"}], "arc": ["curiosity", "clarity"]}
    )
    try:
        run_pipeline(MockMcp(unsynced_world), project, audio, SELF_TEST_PLAN)
        print("FAIL: an export whose speech drifted shipped without a refusal")
        return 1
    except PipelineError as error:
        if "VERIFY-03" not in str(error):
            print(f"FAIL: the drifting speech was refused for the wrong reason: {error}")
            return 1
    print("sync entry ok: a failed verdict is refused as VERIFY-03 with the tool's numbers")

    # A clipping export: the true peak over the 0 dBFS ceiling, measured
    # and named, even with the loudness target itself met.
    _scenario_state_clear(project)
    clipping_world = selftest_world()
    clipping_world["measure_loudness"] = {
        "integratedLufs": -14.0, "truePeakDbtp": 0.8,
        "loudnessRangeLu": 6.0, "targetLufs": -14.0,
        "lufsError": 0.0, "platformTargetMet": True, "notes": [],
    }
    try:
        run_pipeline(MockMcp(clipping_world), project, audio, SELF_TEST_PLAN)
        print("FAIL: a clipping export shipped without a refusal")
        return 1
    except PipelineError as error:
        if "VERIFY-13" not in str(error):
            print(f"FAIL: the clipping export was refused for the wrong reason: {error}")
            return 1
    print("true peak ok: a peak over the 0 dBFS ceiling is refused as VERIFY-13")

    # The checks on their own inputs, where the machine has ffmpeg to
    # render them: a black render fails the black-frame scan and the
    # visual-quality scan; the plan's codec, size and rate are measured
    # against what the render carries; the caption band that fits says so.
    if have_media:
        render = "/tmp/funcut-selftest/final.mp4"
        probe = _ffprobe(render)
        frames = _sample_frames(render, (probe or {}).get("duration"))
        checks = {
            entry["id"]: entry
            for entry in _probe_checks(render, SELF_TEST_PLAN, SELF_TEST_PLAN["export"], probe, frames)
        }
        for check_id in ("VERIFY-01", "VERIFY-04", "VERIFY-09", "VERIFY-10", "VERIFY-11"):
            if checks[check_id]["status"] != "pass":
                print(f"FAIL: {check_id} did not pass on a clean render: {checks[check_id]}")
                return 1
        black = "/tmp/funcut-selftest/final-black.mp4"
        black_probe = _ffprobe(black)
        black_frames = _sample_frames(black, (black_probe or {}).get("duration"))
        black_checks = {
            entry["id"]: entry
            for entry in _probe_checks(black, SELF_TEST_PLAN, SELF_TEST_PLAN["export"], black_probe, black_frames)
        }
        for check_id in ("VERIFY-01", "VERIFY-04"):
            if black_checks[check_id]["status"] != "fail":
                print(f"FAIL: {check_id} did not fail on a black render: {black_checks[check_id]}")
                return 1
        mismatch = _probe_checks(
            render, SELF_TEST_PLAN,
            {"codec": "hevc", "width": 640, "height": 360, "fps": 24},
            probe, frames,
        )
        for entry in mismatch:
            if entry["id"] in ("VERIFY-09", "VERIFY-10", "VERIFY-11") and entry["status"] != "fail":
                print(f"FAIL: {entry['id']} did not fail on a mismatched plan: {entry}")
                return 1
        print("probe checks ok: clean render passes, black render fails, mismatches name themselves")
    else:
        print("probe checks: skipped - no ffmpeg on this machine; the skips were proven above")

    # A caption that lands outside the render's own runtime: present, on
    # the frame, and never on screen.
    late = _caption_checks(
        True, [{"start": 3.0, "end": 4.0}], {"offsetY": 0.35, "fontSize": 0.05}, 2.0
    )
    if late[0]["status"] != "fail" or late[1]["status"] != "pass":
        print(f"FAIL: a caption outside the render's runtime was not caught: {late}")
        return 1
    print("caption runtime ok: a caption that never shows is caught by VERIFY-02")

    # A continuity replacement must exclude the rejected shot and place the
    # replacement, then pass the second continuity read before importing.
    repair_rank_calls: list[dict[str, Any]] = []
    continuity_calls = [0]

    def repair_rank(args: dict[str, Any]) -> dict[str, Any]:
        repair_rank_calls.append(args)
        return {
            "clips": [{
                "id": 7 if len(repair_rank_calls) == 1 else 8,
                "name": "first" if len(repair_rank_calls) == 1 else "replacement",
                "motion": 2.0,
                "camera": "pan_left" if len(repair_rank_calls) == 1 else "pan_right",
                "source": "pexels",
                "seconds": 8.0,
            }]
        }

    def repair_continuity(args: dict[str, Any]) -> dict[str, Any]:
        continuity_calls[0] += 1
        if continuity_calls[0] == 1:
            return {
                "count": 1,
                "flags": [{
                    "slotId": "segment-1",
                    "remedy": {"replace": {"avoid": "a leftward camera pan"}},
                }],
            }
        return {"count": 0, "flags": []}

    repair_replies = selftest_world()
    repair_replies["openvid_rank_for_slot"] = repair_rank
    repair_replies["footage_continuity"] = repair_continuity
    repair_replies["openvid_import"] = lambda args: {
        "mediaIds": ["m-stock-8"],
        "view": {
            "timeline": {
                "clips": [{
                    "id": "c-stock-8", "mediaId": "m-stock-8",
                    "start": 0.0, "duration": 8.0,
                }]
            }
        },
    }
    repaired = MockMcp(repair_replies)
    repaired_calls = run_pipeline(
        repaired, project, audio, SELF_TEST_PLAN
    )
    replacement_rank = repair_rank_calls[1]
    imported_choice = next(
        args for tool, args in repaired_calls if tool == "openvid_import"
    )
    if 7 not in replacement_rank.get("exclude_ids", []) or imported_choice.get("ids") != [8]:
        print(f"FAIL: continuity did not replace the rejected clip: {replacement_rank}, {imported_choice}")
        return 1
    if continuity_calls[0] != 2:
        print(f"FAIL: continuity was not rechecked after replacement: {continuity_calls}")
        return 1
    print("continuity ok: rejected footage excluded, replacement placed, second gate clean")

    # An unknown remedy is replaced away from, not crashed on.
    remedy_replies = selftest_world()
    remedy_replies["footage_continuity"] = lambda args: {
        "count": 1,
        "flags": [{
            "slotId": "segment-1",
            "detail": "a jump nobody measured",
            "remedy": {"regrade": {"towards": "warmer"}},
        }],
    }
    remedy = MockMcp(remedy_replies)
    _scenario_state_clear(project)
    try:
        run_pipeline(remedy, project, audio, SELF_TEST_PLAN)
    except PipelineError as error:
        if "regrade" in str(error):
            print(f"FAIL: an unknown continuity remedy still crashes the run: {error}")
            return 1
    print("remedy ok: a flag this runner never met is replaced away from, not crashed on")

    # A music-carrying take gets no bed stacked under it.
    stacked_replies = selftest_world()
    stacked_replies["detect_music_segments"] = {"musicPresent": True, "segments": [{"start": 0.0, "end": 12.0}]}
    stacked = MockMcp(stacked_replies)
    _scenario_state_clear(project)
    stacked_calls = run_pipeline(stacked, project, audio, SELF_TEST_PLAN)
    if any(tool == "bgm_import" for tool, _ in stacked_calls):
        print("FAIL: a bed was stacked under a take that already carries music")
        return 1
    if not any(tool == "identify_music" for tool, _ in stacked_calls):
        print("FAIL: the music-carrying take was not identified for the record")
        return 1
    print("music gate ok: a take that carries music gets no bed, and the music is identified")

    # A forged signalLog - citations no measurement answered - is refused.
    forged_replies = selftest_world()
    forged_replies["voice_signals"] = {
        "status": "ok",
        "degradations": [],
        "signalConfidence": {"perSignal": [
            {"name": "loudness", "confidence": 0.9, "basis": "ffmpeg ebur128"},
        ]},
    }
    forged = MockMcp(forged_replies)
    _scenario_state_clear(project)
    try:
        run_pipeline(forged, project, audio, SELF_TEST_PLAN)
        print("FAIL: a signalLog citing unmeasured signals was executed")
        return 1
    except PipelineError as error:
        if "never happened" not in str(error):
            print(f"FAIL: the forged signalLog was refused for the wrong reason: {error}")
            return 1
    print("evidence ok: citations the take never answered refuse the plan")

    # A failure stops the run: no timeline or export work follows it.
    broken = MockMcp(selftest_world(), fail_on="emotion")
    _scenario_state_clear(project)
    try:
        run_pipeline(broken, project, audio, SELF_TEST_PLAN)
        print("FAIL: a failing phase did not stop the pipeline")
        return 1
    except PipelineError:
        later = [tool for tool, _ in broken.calls if tool in {
            "plan_validate", "openvid_import", "clip_trim",
            "export_start", "export_wait", "watch_render",
        }]
        if later:
            print(f"FAIL: phases ran past the failure: {later}")
            return 1
    print("failure ok: a failed analysis stops before any timeline mutation")

    # A server that cannot serve the core tools is refused at the handshake.
    thin = MockMcp({"project_get": lambda _args: SELF_TEST_BASELINE_BIN})
    _scenario_state_clear(project)
    try:
        run_pipeline(thin, project, audio, SELF_TEST_PLAN)
        print("FAIL: a server without the core tools was run against")
        return 1
    except PipelineError as error:
        if "does not serve" not in str(error):
            print(f"FAIL: the thin server was refused for the wrong reason: {error}")
            return 1
    print("handshake ok: a server that cannot serve the pipeline is refused up front")

    unready_replies = selftest_world()
    unready_replies["plan_validate"] = {
        "valid": True,
        "creativeReady": False,
        "creativeBlockers": ["missing signal log"],
        "signalLog": {"driven": False, "coverage": 0.0, "blockers": ["missing signal log"]},
    }
    unready = MockMcp(unready_replies)
    _scenario_state_clear(project)
    try:
        run_pipeline(unready, project, audio, SELF_TEST_PLAN)
        print("FAIL: a creatively unready plan was executed")
        return 1
    except PipelineError:
        mutated = [
            tool for tool, _ in unready.calls
            if tool in {"project_get", "track_add", "media_import", "openvid_import", "clip_add", "edit_apply", "export_start"}
        ]
        if mutated:
            print(f"FAIL: timeline mutation preceded readiness: {mutated}")
            return 1
    print("readiness gate ok: no edit or export before creativeReady")

    # ---- the degradation record (DEG): every fallback the run survived
    # lands in its own signalLog with provenance=fallback - one entry per
    # degradation, the shape the plan's signalLog entries will be held to.

    def deg_project(name: str) -> str:
        folder = os.path.join("/tmp/funcut-selftest", name)
        os.makedirs(folder, exist_ok=True)
        runs = os.path.join(folder, ".auto-director-runs.jsonl")
        if os.path.exists(runs):
            os.remove(runs)
        _scenario_state_clear(folder)
        return folder

    def recorded_fallbacks(folder: str) -> list[dict[str, Any]]:
        with open(os.path.join(folder, ".auto-director-runs.jsonl"), encoding="utf-8") as handle:
            lines = [line for line in handle if line.strip()]
        return (json.loads(lines[-1]) if lines else {}).get("signalLog") or []

    def has_entry(entries: list[dict[str, Any]], **wanted: str) -> bool:
        return any(
            all(entry.get(key) == value for key, value in wanted.items())
            and entry.get("provenance") == "fallback"
            for entry in entries
        )

    # DEG-01: the NER ear missing is a recorded fallback, not an empty list.
    deg01_world = selftest_world()
    deg01_world["voice_signals"] = dict(SELF_TEST_REPLIES["voice_signals"]) | {
        "entitiesAvailable": False,
    }
    deg01 = deg_project("deg-01")
    run_pipeline(MockMcp(deg01_world), deg01, audio, SELF_TEST_PLAN)
    entries = recorded_fallbacks(deg01)
    if not has_entry(entries, stage="sense", decision="entities", signalName="entities"):
        print(f"FAIL: a missing NER was not recorded as a fallback: {entries}")
        return 1
    print("DEG-01 ok: entitiesAvailable=false is recorded with provenance=fallback")

    # DEG-03: a keyword-tier ranking is named in the run's own log.
    deg03_world = selftest_world()
    deg03_world["openvid_rank_for_slot"] = lambda args: dict(
        SELF_TEST_REPLIES["openvid_rank_for_slot"]
    ) | {
        "rerank": {
            "applied": False,
            "ranking": "keyword-tier",
            "reason": "the vision rerank model is not installed",
        },
    }
    deg03 = deg_project("deg-03")
    run_pipeline(MockMcp(deg03_world), deg03, audio, SELF_TEST_PLAN)
    entries = recorded_fallbacks(deg03)
    if not has_entry(entries, stage="broll", decision="ranking", signalName="openvid_search.rerank"):
        print(f"FAIL: a keyword-tier ranking was not recorded: {entries}")
        return 1
    print("DEG-03 ok: keyword-tier stock ranking is recorded with provenance=fallback")

    # DEG-04: a critique under its tier's confidence line is a recorded
    # fallback; a confident hosted critique is not.
    deg04 = deg_project("deg-04")
    run_pipeline(MockMcp(selftest_world()), deg04, audio, SELF_TEST_PLAN)
    entries = recorded_fallbacks(deg04)
    if not has_entry(entries, stage="craft", decision="critique", signalName="craft_critique"):
        print(f"FAIL: a heuristic 0.5-confidence critique was not recorded: {entries}")
        return 1
    deg04b_world = selftest_world()
    deg04b_world["craft_critique"] = {
        "overall": 8.5, "scores": {}, "notes": [], "tier": "hosted", "confidence": 0.9,
    }
    deg04b = deg_project("deg-04b")
    run_pipeline(MockMcp(deg04b_world), deg04b, audio, SELF_TEST_PLAN)
    if has_entry(recorded_fallbacks(deg04b), stage="craft"):
        print("FAIL: a confident hosted critique was recorded as a fallback")
        return 1
    print("DEG-04 ok: the tier/confidence line is interpreted, not just recorded")

    # DEG-05: a failing identification is a missing identification, not a
    # "no music" reading and not a dead run.
    deg05_world = selftest_world()
    deg05_world["detect_music_segments"] = {
        "musicPresent": True, "segments": [{"start": 0.0, "end": 12.0}],
    }
    deg05 = deg_project("deg-05")
    deg05_calls = run_pipeline(
        MockMcp(deg05_world, fail_on="identify_music"), deg05, audio, SELF_TEST_PLAN
    )
    entries = recorded_fallbacks(deg05)
    if not has_entry(entries, stage="music-gate", decision="identify", signalName="identify_music"):
        print(f"FAIL: a failing identify_music was not recorded as missing: {entries}")
        return 1
    if any(tool == "bgm_import" for tool, _ in deg05_calls):
        print("FAIL: a bed was stacked under a take whose music was identified as present")
        return 1
    print("DEG-05 ok: identify_music failure is a recorded absence, not 'no music'")

    # DEG-06: a library that cannot be made ready is a warned, recorded
    # absence - the export ships loudly without the bed.
    deg06_world = selftest_world()
    deg06_world["bgm_status"] = {"state": "unavailable"}
    deg06 = deg_project("deg-06")
    deg06_calls = run_pipeline(MockMcp(deg06_world), deg06, audio, SELF_TEST_PLAN)
    entries = recorded_fallbacks(deg06)
    if not has_entry(entries, stage="bgm", decision="bed", signalName="bgm_status"):
        print(f"FAIL: a missing music library was not recorded: {entries}")
        return 1
    if any(tool == "bgm_import" for tool, _ in deg06_calls):
        print("FAIL: a bed was placed against a library that was never ready")
        return 1
    print("DEG-06 ok: the missing library is a recorded absence, never a silent skip")

    # DEG-07: a prepare that dies with the server fails with a clear error;
    # a prepare that comes ready places the bed.
    deg07_world = selftest_world()
    deg07_world["bgm_status"] = {"state": "loading"}
    deg07_world["bgm_prepare"] = {"ok": True}
    deg07 = deg_project("deg-07")
    try:
        run_pipeline(MockMcp(deg07_world, fail_on="bgm_prepare"), deg07, audio, SELF_TEST_PLAN)
        print("FAIL: a bgm_prepare that died did not stop the run with a clear error")
        return 1
    except PipelineError as error:
        if "could not be prepared" not in str(error):
            print(f"FAIL: the dead bgm_prepare failed for the wrong reason: {error}")
            return 1
    bgm_reads = {"count": 0}

    def warming_library(_args: dict[str, Any]) -> dict[str, Any]:
        bgm_reads["count"] += 1
        return {"state": "ready"} if bgm_reads["count"] > 1 else {"state": "missing"}

    deg07b_world = selftest_world()
    deg07b_world["bgm_status"] = warming_library
    deg07b_world["bgm_prepare"] = {"ok": True}
    deg07b = deg_project("deg-07b")
    deg07b_calls = run_pipeline(MockMcp(deg07b_world), deg07b, audio, SELF_TEST_PLAN)
    if not any(tool == "bgm_import" for tool, _ in deg07b_calls):
        print("FAIL: a library that came ready after bgm_prepare still placed no bed")
        return 1
    if has_entry(recorded_fallbacks(deg07b), stage="bgm"):
        print("FAIL: a library that came ready was still recorded as a fallback")
        return 1
    print("DEG-07 ok: the dead prepare fails clearly; the live one recovers the bed")

    # DEG-09: a wrong-language transcript is flagged at the reading.
    deg09_world = selftest_world()
    deg09_world["analysis_wait"] = {
        "state": "done",
        "output": '{"segments":[{"text":"one","start":0.0,"end":1.0}],'
                  '"model":"base.en","scriptDetected":"cyrillic",'
                  '"language":null,"languageMismatch":true}',
    }
    deg09 = deg_project("deg-09")
    run_pipeline(MockMcp(deg09_world), deg09, audio, SELF_TEST_PLAN)
    entries = recorded_fallbacks(deg09)
    if not has_entry(entries, stage="transcribe", decision="transcript", signalName="transcribe.language"):
        print(f"FAIL: a wrong-language transcript was not recorded: {entries}")
        return 1
    print("DEG-09 ok: the script mismatch is flagged in the run's own log")

    # DEG-10: an int8 Kokoro take has its hiss cure recorded, chained or not.
    deg10_plan = copy.deepcopy(SELF_TEST_PLAN)
    deg10_plan.setdefault("audio", {}).setdefault("voiceover", {})[
        "model"
    ] = "kokoro-int8-multi-lang-v1_0"
    deg10 = deg_project("deg-10")
    run_pipeline(MockMcp(selftest_world()), deg10, audio, deg10_plan)
    entries = recorded_fallbacks(deg10)
    if not has_entry(
        entries, stage="voiceover-repair", decision="denoise",
        signalName="remove_hiss_hum_rumble",
    ):
        print(f"FAIL: an int8 take's denoise chain was not recorded: {entries}")
        return 1
    print("DEG-10 ok: the int8 hiss cure is recorded with provenance=fallback")

    # FALL-06: the emotion read on the English-only lexicon is recorded.
    fall06_world = selftest_world()
    fall06_world["emotion"] = lambda args: (
        {"verdict": "pass", "matchRate": 1.0, "maxDriftMs": 0.0}
        if args.get("action") == "verify"
        else {"engine": "lexicon",
              "lines": [{"text": "one", "emotion": "curious"}],
              "arc": ["curiosity", "clarity"]}
    )
    fall06 = deg_project("fall-06")
    run_pipeline(MockMcp(fall06_world), fall06, audio, SELF_TEST_PLAN)
    entries = recorded_fallbacks(fall06)
    if not has_entry(entries, stage="emotion", decision="arc", signalName="emotion.model"):
        print(f"FAIL: the lexicon emotion read was not recorded: {entries}")
        return 1
    print("FALL-06 ok: the English-lexicon emotion read is a recorded fallback")

    # FALL-07: a plan that names a whisper model hands it to the server.
    fall07_plan = copy.deepcopy(SELF_TEST_PLAN)
    fall07_plan.setdefault("audio", {}).setdefault("voiceover", {})[
        "whisperModel"
    ] = "small"
    fall07 = deg_project("fall-07")
    fall07_calls = run_pipeline(MockMcp(selftest_world()), fall07, audio, fall07_plan)
    asked = next(args for tool, args in fall07_calls if tool == "transcribe")
    if asked.get("model") != "small":
        print(f"FAIL: the plan's whisperModel did not reach the transcriber: {asked}")
        return 1
    print("FALL-07 ok: the plan's whisper model is the transcriber's ask")

    # FALL-09: a plan that names only a mood shops its bed with the mood.
    fall09_plan = copy.deepcopy(SELF_TEST_PLAN)
    fall09_plan["audio"]["bgm"] = {"mood": "warm lofi", "duckDb": -12.0}
    fall09 = deg_project("fall-09")
    fall09_calls = run_pipeline(MockMcp(selftest_world()), fall09, audio, fall09_plan)
    shopped = next(args for tool, args in fall09_calls if tool == "bgm_search")
    if shopped.get("query") != "warm lofi":
        print(f"FAIL: the mood did not stand in for the query: {shopped}")
        return 1
    print("FALL-09 ok: a mood-only bed is shopped with the mood, said aloud")

    # FALL-10: where the two music readings disagree, identify_music decides
    # and both readings are recorded.
    fall10_world = selftest_world()
    fall10_world["detect_music_segments"] = {"musicPresent": False, "segments": []}
    fall10_world["identify_music"] = {"ok": True, "tempo": {"bpm": 124.0, "strength": 0.42}}
    fall10 = deg_project("fall-10")
    fall10_calls = run_pipeline(MockMcp(fall10_world), fall10, audio, SELF_TEST_PLAN)
    if any(tool == "bgm_import" for tool, _ in fall10_calls):
        print("FAIL: a bed was stacked although identify_music heard the pulse")
        return 1
    entries = recorded_fallbacks(fall10)
    if not has_entry(entries, stage="music-gate", decision="bgm", signalName="identify_music"):
        print(f"FAIL: the music disagreement was not recorded: {entries}")
        return 1
    with open(os.path.join(fall10, ".auto-director-runs.jsonl"), encoding="utf-8") as handle:
        record = json.loads([line for line in handle if line.strip()][-1])
    both = [
        entry for entry in record.get("signalLog", [])
        if entry.get("decision") == "bgm" and entry.get("provenance") == "measured"
    ]
    if not both or both[0].get("detectMusic") is not False or both[0].get("identifyMusic") is not True:
        print(f"FAIL: both music readings were not recorded as measured: {both}")
        return 1
    print("FALL-10 ok: the identifier decides the disagreement, both readings recorded")

    print("SELF-TEST PASS")
    return 0


def revision_self_test() -> int:
    """The craft gate's own loop, exercised against mocks that score the
    first render low and name the B-roll slot: the named slot re-shops
    with the old clip excluded, the old clip leaves the timeline, the
    revision renders to its own file, and the second score ships. A
    critic that never clears the line ships the edit anyway after two
    passes, with the failures recorded - never a crash that throws away
    a fixable cut (P1-006/007, the revision.rs contract)."""
    audio = "/tmp/funcut-selftest/voiceover.wav"
    project = "/tmp/funcut-selftest/project"

    def replies_always_low(scores: list[float]) -> dict[str, Any]:
        """One mock world: the shelf offers a fresh candidate every time,
        the critic speaks in the documented reply shape, and the scores
        come from the list, one per pass."""
        rank_calls: list[dict[str, Any]] = []
        passes: list[dict[str, Any]] = []

        def rank(args: dict[str, Any]) -> dict[str, Any]:
            rank_calls.append(args)
            clip_id = 6 + len(rank_calls)  # 7, 8, 9, ... - always fresh
            return {"clips": [{
                "id": clip_id, "name": f"city-{clip_id}", "motion": 2.0,
                "camera": "pan_left", "source": "pexels", "seconds": 8.0,
                "slotRank": 1,
            }]}

        def critique(_args: dict[str, Any]) -> dict[str, Any]:
            index = len(passes)
            passes.append(_args)
            score = scores[min(index, len(scores) - 1)]
            return {
                "ok": True, "engine": "heuristic", "tier": "heuristic",
                "overall": score, "scores": {}, "confidence": 0.5,
                "notes": (
                    ["segment-1's opening frame is near-black; the hook never lands"]
                    if score < CRAFT_SHIP_SCORE else []
                ),
                "revisions": (
                    ["replace segment-1 with a brighter establishing shot"]
                    if score < CRAFT_SHIP_SCORE else []
                ),
                "fixes": [],
            }

        def importer(args: dict[str, Any]) -> dict[str, Any]:
            clip_id = (args.get("ids") or [7])[0]
            return {
                "mediaIds": [f"m-stock-{clip_id}"],
                "view": {"timeline": {"clips": [{
                    "id": f"c-stock-{clip_id}", "mediaId": f"m-stock-{clip_id}",
                    "start": 0.0, "duration": 8.0,
                }]}},
            }

        world = selftest_world()
        world["openvid_rank_for_slot"] = rank
        world["openvid_import"] = importer
        world["craft_critique"] = critique
        world["clips_remove"] = {"ok": True}
        return world

    # One revision, then the score clears the line and the edit ships.
    _scenario_state_clear(project)
    transport = MockMcp(replies_always_low([5.5, 7.5]))
    calls = run_pipeline(transport, project, audio, SELF_TEST_PLAN)
    by_tool = [(tool, args) for tool, args in calls]
    ranks = [args for tool, args in by_tool if tool == "openvid_rank_for_slot"]
    if len(ranks) != 2:
        print(f"FAIL: expected the shop plus one re-shop, got {len(ranks)}")
        return 1
    if ranks[1].get("exclude_ids") != [7]:
        print(f"FAIL: the re-shop did not exclude the clip the critic saw: {ranks[1]}")
        return 1
    removals = [args for tool, args in by_tool if tool == "clips_remove"]
    if not removals or removals[0].get("clipIds") != ["c-stock-7"]:
        print(f"FAIL: the replaced clip never left the timeline: {removals}")
        return 1
    exports = [args for tool, args in by_tool if tool == "export_start"]
    if len(exports) != 2 or not str(exports[1].get("output", "")).endswith("edit_v2.mp4"):
        print(f"FAIL: the revision did not render to its own file: {exports}")
        return 1
    if len([args for tool, args in by_tool if tool == "watch_render"]) != 2:
        print("FAIL: the revision was not watched with the same eyes")
        return 1
    reimport = [args for tool, args in by_tool if tool == "openvid_import"]
    if reimport[-1].get("ids") != [8]:
        print(f"FAIL: the replacement candidate was not imported: {reimport[-1]}")
        return 1
    print("revision ok: the named slot re-shopped, the old clip left, edit_v2 shipped at 7.5")

    # A critic that never clears the line: two passes, then the edit
    # ships with the failures recorded - never a crash.
    _scenario_state_clear(project)
    transport = MockMcp(replies_always_low([5.5, 5.5, 5.5, 5.5]))
    calls = run_pipeline(transport, project, audio, SELF_TEST_PLAN)
    by_tool = [(tool, args) for tool, args in calls]
    exports = [args for tool, args in by_tool if tool == "export_start"]
    if len(exports) != 3:
        print(f"FAIL: expected three renders (the edit plus two revisions), got {len(exports)}")
        return 1
    if not str(exports[2].get("output", "")).endswith("edit_v3.mp4"):
        print(f"FAIL: the second revision did not render to its own file: {exports}")
        return 1
    if len([args for tool, args in by_tool if tool == "watch_render"]) != 3:
        print("FAIL: every attempt must be watched and scored")
        return 1
    print("exhaustion ok: two revisions, three renders, the edit shipped with its failures recorded")

    # A graphic beat the critic re-times: the fix folds into the
    # storyboard, the reel re-renders, and the second score ships.
    world = replies_always_low([5.5, 7.5])
    plain_critique = world["craft_critique"]

    def critique_with_fix(args: dict[str, Any]) -> dict[str, Any]:
        reply = plain_critique(args)
        if reply.get("overall", 10.0) < CRAFT_SHIP_SCORE:
            reply["notes"] = ["segment-2's chapter title enters before the beat carries it"]
            reply["revisions"] = []
            reply["fixes"] = [{
                "where": "segment-2's chapter title",
                "problem": "the title enters 0.5s before the beat carries it",
                "change": "hold it back by 0.5 seconds",
                "op": "retime",
                "patch": {"shift": 0.5},
            }]
        return reply

    world["craft_critique"] = critique_with_fix
    _scenario_state_clear(project)
    transport = MockMcp(world)
    calls = run_pipeline(transport, project, audio, SELF_TEST_PLAN)
    by_tool = [(tool, args) for tool, args in calls]
    reels = [args for tool, args in by_tool if tool == "motion_reel"]
    if len(reels) != 2:
        print(f"FAIL: the graphic was not re-rendered: {len(reels)} reel call(s)")
        return 1
    retimed = reels[1]["storyboard"]["scenes"][0].get("at")
    if retimed != 0.5:
        print(f"FAIL: the retime fix did not fold into the storyboard: {reels[1]['storyboard']}")
        return 1
    exports = [args for tool, args in by_tool if tool == "export_start"]
    if len(exports) != 2:
        print(f"FAIL: the re-rendered graphic never reached a new render: {len(exports)}")
        return 1
    print("graphic ok: the critic's retime folded into the storyboard, the reel re-rendered")
    return 0


# ---------------------------------------------------------------------- main


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Watch for an imported voiceover and direct the edit."
    )
    parser.add_argument("--self-test", action="store_true",
                        help="run the pipeline against mocks; no server, no model")
    parser.add_argument("--project", help="the project folder to watch")
    parser.add_argument("--plan", help="the edit plan JSON (plan_validate's input)")
    parser.add_argument("--audio", help="run the pipeline once on this file")
    parser.add_argument("--poll", type=float, default=POLL_SECONDS,
                        help="seconds between media-list polls (default 2)")
    parser.add_argument(
        "--mcp-command",
        default=None,
        help="how the MCP server is started (default: concat --mcp auto, "
             "or $FUNCUT_MCP_COMMAND)",
    )
    args = parser.parse_args()

    if args.self_test:
        status = self_test()
        if status:
            return status
        return revision_self_test()
    if not args.project or not args.plan:
        parser.error("--project and --plan are required unless --self-test")

    command = args.mcp_command or os.environ.get("FUNCUT_MCP_COMMAND") or "concat --mcp auto"
    transport = McpStdio(command)
    transport.start()
    try:
        plan = load_plan(args.plan)
        if args.audio:
            try:
                run_pipeline(transport, args.project, args.audio, plan)
            except PipelineError as error:
                log(f"STOP: {error}")
                _record_run(args.project, {
                    "at": time.strftime("%Y-%m-%dT%H:%M:%S"),
                    "status": "failed",
                    "audio": args.audio,
                    "stop": str(error),
                    **_stage_failure_note(args.project),
                })
                return 1
            return 0
        watch(transport, args.project, plan, args.poll)
    except KeyboardInterrupt:
        log("interrupted: the run stopped where it stood; the run log is the record")
        return 130
    finally:
        transport.close()
    return 0


if __name__ == "__main__":
    sys.exit(main())
