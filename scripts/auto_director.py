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
-------------------------------------------
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
1.  `transcribe`            words and timings from the voiceover
2.  `voice_signals`         measured pauses, emphasis and delivery
3.  `emotion`               emotional read of the narration
4.  `plan_validate`         schema, creative readiness and signal-driven gate
5.  `project_get`           find or import the voiceover and place it on its own lane
6.  `openvid_rank_for_slot` each timed B-roll segment, with duration checks
7.  `footage_continuity`    check the selected clips; replace flagged shots once
8.  `openvid_import`        import and place stock at the segment start; trim
9.  `motion_reel`           render supplied text/title storyboards
10. `media_import` + `clip_add` place motion movies on a dedicated top lane
11. `export_start` + `export_wait` render and wait for a terminal job state
12. `watch_render` + `craft_critique` inspect the actual output and score it

The watcher is an execution layer, not a topic-to-plan author: the Director
agent must supply a validated canonical edit plan. A craft score below 7
stops the run for creative revision; this script does not invent a new plan.

The calls go through the editor's MCP server over stdio (newline-delimited
JSON-RPC 2.0 - `tools/call`), the same surface the funcut-director skill
drives; `--mcp-command` says how it is started, and defaults to
`concat --mcp auto`.
"""
from __future__ import annotations

import argparse
import json
import os
import shlex
import subprocess
import sys
import time
from typing import Any

POLL_SECONDS = 2.0


class PipelineError(RuntimeError):
    """A phase failed: the run stops here, later phases never start."""


def log(message: str) -> None:
    """One line per event, stamped, flushed: the run is watchable live."""
    stamp = time.strftime("%Y-%m-%dT%H:%M:%S")
    print(f"[{stamp}] {message}", flush=True)


# ---------------------------------------------------------------- transports


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

    def call(self, tool: str, arguments: dict[str, Any]) -> Any:
        """One `tools/call`; a tool error is a failure, not a None."""
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
    candidates = [
        item for item in _walk_dicts(view)
        if (item.get("mediaId") or item.get("media")) == media_id and isinstance(item.get("id"), str)
        and isinstance(item.get("duration"), (int, float))
        and isinstance(item.get("start"), (int, float))
    ]
    return min(candidates, key=lambda item: abs(float(item["start"]) - start), default=None)


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


# --------------------------------------------------------------- the pipeline


def phase_plan(plan: dict[str, Any]) -> list[tuple[str, str]]:
    """Describe the real segment-based execution order for the run log."""
    phases = [
        ("transcribe", "start source transcription"),
        ("analysis_wait", "wait for source transcription"),
        ("voice_signals", "voice_signals"),
        ("emotion", "emotion (extract)"),
        ("plan_validate", "plan_validate (before timeline edits)"),
        ("project_get", "read the project bin and active timeline"),
        ("track_add", "create a dedicated voiceover lane when needed"),
        ("clip_add", "place the voiceover when it is not already on the timeline"),
    ]
    phases.extend(
        ("openvid_rank_for_slot", f"rank B-roll segment {index + 1}")
        for index, _ in broll_segments(plan)
    )
    if broll_segments(plan):
        phases.append(("footage_continuity", "footage_continuity and repair"))
        phases.extend(
            ("openvid_import", f"import and place B-roll segment {index + 1}")
            for index, _ in broll_segments(plan)
        )
        phases.extend(
            ("clip_trim", f"trim B-roll segment {index + 1}")
            for index, _ in broll_segments(plan)
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
            phases.append(("media_import", f"import existing source segment {index + 1}"))
        phases.extend([
            ("clip_add", f"place existing source segment {index + 1}"),
            ("clip_trim", f"trim existing source segment {index + 1}"),
        ])
    phases.extend([
        ("export_start", "export"),
        ("export_wait", "wait for completed export"),
        ("transcribe", "start exported speech transcription"),
        ("analysis_wait", "wait for exported speech transcription"),
        ("emotion", "compare source and rendered speech timing"),
        ("watch_render", "inspect rendered frames and audio"),
        ("craft_critique", "score the rendered cut"),
    ])
    return phases


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

    source_job = advance("transcribe", {"path": audio})
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
    voice = advance("voice_signals", {"audio": audio, "transcript": transcript})
    emotion = advance(
        "emotion", {"action": "extract", "transcript": transcript, "project": project}
    )
    if isinstance(emotion, dict) and emotion.get("arc") is not None:
        log(f"voice emotion arc: {json.dumps(emotion['arc'], ensure_ascii=False)}")

    # Gate all plan and voice-signal readiness before changing the timeline.
    validation = advance("plan_validate", {"plan": plan})
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
            + json.dumps(signal_log.get("blockers", []), ensure_ascii=False)
        )

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
    else:
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

    log(f"voiceover ready on timeline: clip {audio_clip_id}; timing remains unchanged by the runner")

    # Rank one unique candidate per actual timed B-roll segment.
    excluded: list[int] = []
    chosen: dict[str, dict[str, Any]] = {}
    slot_segments: dict[str, tuple[int, dict[str, Any]]] = {}
    for index, segment in broll_segments(plan):
        visual = segment["visual"]
        slot_id = str(segment.get("slotId") or f"segment-{index + 1}")
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
            clips = reply.get("clips", []) if isinstance(reply, dict) else []
            if not clips:
                break
            candidate = clips[0]
            clip_id = candidate.get("id")
            seconds = candidate.get("seconds")
            if isinstance(seconds, (int, float)) and seconds + 0.05 < target_duration:
                log(f"{slot_id}: rejecting {candidate.get('name', clip_id)}; {seconds:.2f}s is shorter than the {target_duration:.2f}s beat")
                if isinstance(clip_id, int):
                    excluded.append(clip_id)
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
        if isinstance(clip_id, int):
            excluded.append(clip_id)

    # Continuity reads measured library metadata. Replace flagged shots once
    # with the remedy's avoid hint; never silently pretend unknown metadata
    # (colour/scale) was measured.
    if chosen:
        def continuity_manifest() -> list[dict[str, Any]]:
            result = []
            for slot_id, clip in chosen.items():
                index, segment = slot_segments[slot_id]
                visual = segment["visual"]
                result.append({
                    "slotId": slot_id,
                    "clipId": str(clip.get("id", clip.get("name", slot_id))),
                    "colorTemperature": float(clip.get("colorTemperature", 0.0)),
                    "motion": float(clip.get("motion", 0.0)),
                    "dominantDirection": _direction(clip.get("camera")),
                    "shotScale": visual.get("shotScale", "medium"),
                    "category": str(visual.get("category") or clip.get("category") or clip.get("source") or "unknown"),
                    "motionMin": float(visual.get("motionMin") or 0.0),
                })
            return result

        continuity = advance("footage_continuity", {"clips": continuity_manifest()})
        flags = continuity.get("flags", []) if isinstance(continuity, dict) else []
        for flag in flags:
            slot_id = flag.get("slotId")
            remedy = flag.get("remedy", {})
            replace = remedy.get("replace") if isinstance(remedy, dict) else None
            if not isinstance(slot_id, str) or slot_id not in chosen:
                continue
            if not isinstance(replace, dict):
                if isinstance(remedy, dict) and "downgrade_to_graphic" in remedy:
                    index, segment = slot_segments[slot_id]
                    if isinstance(segment["visual"].get("storyboard"), dict):
                        segment["visual"]["type"] = "textCard"
                        chosen.pop(slot_id, None)
                        slot_segments.pop(slot_id, None)
                        log(f"{slot_id}: continuity downgraded the shot to its authored motion storyboard")
                        continue
                raise PipelineError(
                    f"continuity requires a treatment this runner cannot safely execute for {slot_id}: "
                    + json.dumps(flag, ensure_ascii=False)
                )
            index, segment = slot_segments[slot_id]
            visual = segment["visual"]
            avoid = str(replace.get("avoid") or "the previous shot's look")
            old_id = chosen[slot_id].get("id")
            retry = advance(
                "openvid_rank_for_slot",
                {
                    "description": f"{visual.get('description') or segment.get('spoken') or visual.get('query', '')}; avoid {avoid}",
                    "queries": visual.get("queries") or [visual.get("query")],
                    "hero": bool(visual.get("hero", False)),
                    "motion_min": visual.get("motionMin"),
                    "exclude_ids": list(set(excluded + ([old_id] if isinstance(old_id, int) else []))),
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
            if isinstance(new_id, int):
                excluded.append(new_id)
        if flags:
            second = advance("footage_continuity", {"clips": continuity_manifest()})
            remaining = second.get("flags", []) if isinstance(second, dict) else []
            if remaining:
                raise PipelineError(
                    "continuity still has unresolved flags after one repair pass: "
                    + json.dumps(remaining, ensure_ascii=False)
                )

    # Import and place each chosen source at its segment's real timeline start.
    for slot_id, clip in chosen.items():
        index, segment = slot_segments[slot_id]
        start_at = float(segment["start"])
        target_duration = float(segment["end"]) - start_at
        imported = advance(
            "openvid_import",
            {"path": project, "ids": [int(clip["id"])], "start": start_at},
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
        log(
            f"{slot_id}: imported {clip.get('name', clip['id'])} at {start_at:.2f}s "
            f"for {min(actual_duration, target_duration):.2f}s"
        )

    # Motion reels and imported existing media share one dedicated top
    # video lane; already-placed existing clips are verified in place.
    graphics = motion_segments(plan)
    existing_to_place = [
        (index, segment) for index, segment in existing_segments(plan)
        if not segment["visual"].get("clipId")
    ]
    track_id = None
    if graphics or existing_to_place:
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
            log(f"segment-{index + 1}: rendered and placed motion at {start_at:.2f}s")

    # Place imported existing media on the same overlay lane. A supplied
    # clipId was already verified to exist at the correct time above.
    for index, segment in existing_to_place:
        visual = segment["visual"]
        start_at = float(segment["start"])
        target_duration = float(segment["end"]) - start_at
        media_id = visual.get("mediaId")
        if not media_id:
            imported = advance("media_import", {"path": project, "file": visual["file"]})
            media_id = imported.get("createdId") if isinstance(imported, dict) else None
        if not isinstance(media_id, str) or not media_id:
            raise PipelineError(f"segment-{index + 1}: existing source import returned no media id")
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
        media_item = next(
            (item for item in media if isinstance(item, dict) and item.get("id") == media_id),
            None,
        )
        actual_duration = float(media_item.get("duration", target_duration)) if media_item else target_duration
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
        log(f"segment-{index + 1}: existing media placed at {start_at:.2f}s")

    # A render is not complete until the export job reports a terminal state.
    export = plan.get("export") if isinstance(plan.get("export"), dict) else {}
    output = export.get("output")
    if not isinstance(output, str) or not output.strip():
        output = os.path.join(project, f"funcut-final-{time.strftime('%Y%m%d-%H%M%S')}.mp4")
    started = advance(
        "export_start",
        {
            "path": project,
            "output": output,
            "crf": int(export.get("crf", 20)),
            "codec": export.get("codec", "h264"),
        },
    )
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
    verification = advance(
        "emotion",
        {
            "action": "verify",
            "expected": transcript,
            "actual": exported_transcript,
            "toleranceMs": 150,
        },
    )
    if not isinstance(verification, dict) or verification.get("verdict") != "pass":
        raise PipelineError(
            "exported speech failed transcript/timing verification: "
            + json.dumps(verification, ensure_ascii=False)
        )
    log(
        f"speech verification passed: matchRate={verification.get('matchRate')} "
        f"maxDriftMs={verification.get('maxDriftMs')}"
    )

    inspection = advance(
        "watch_render",
        {
            "path": rendered,
            "samples": 12,
            "audio": True,
            "out": os.path.join(project, "director-review.png"),
        },
    )
    if not isinstance(inspection, dict) or not inspection.get("pngPath"):
        raise PipelineError(f"render inspection did not produce a contact sheet: {inspection}")
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
    if score < 7:
        raise PipelineError(
            f"rendered cut scored {score}/10; revision is required before publishing: "
            + json.dumps(critique, ensure_ascii=False)
        )
    log(f"pipeline complete: {len(done)} call(s); rendered {rendered}; craft score {score}")
    return done


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


# ----------------------------------------------------------------- self-test


SELF_TEST_PLAN = {
    "aspect": "16:9",
    "mode": "longform",
    "sections": [
        {"role": "hook", "start": 0.0, "end": 4.0},
        {"role": "body", "start": 4.0, "end": 8.0},
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
                    "scenes": [{"preset": "funcut.chapter.chip", "hold": 4.0}],
                },
            },
        },
    ],
    "audio": {"voiceover": {"source": "file"}},
    "grade": {"filter": "concat.warm", "intensityKey": "amount", "intensity": 35},
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
    "export": {"output": "/tmp/funcut-selftest/final.mp4", "crf": 20, "codec": "h264"},
}


SELF_TEST_REPLIES = {
    "transcribe": {"job": "speech-job"},
    "project_get": {
        "media": [{
            "id": "m-voice",
            "path": "/tmp/funcut-selftest/voiceover.wav",
            "kind": "audio",
        }],
        "timeline": {"tracks": [], "clips": []},
    },
    "analysis_wait": {
        "state": "done",
        'output': '{"segments":[{"text":"one","start":0.0,"end":1.0}]}',
    },
    "voice_signals": {
        "signals": {
            "pauses": [
                {"class": "dead_air", "start": 1.2, "end": 2.4},
                {"class": "dramatic_hold", "start": 4.0, "end": 5.1},
                {"class": "dead_air", "start": 6.0, "end": 6.5},
            ]
        }
    },
    "emotion": lambda args: (
        {"verdict": "pass", "matchRate": 1.0, "maxDriftMs": 0.0}
        if args.get("action") == "verify"
        else {"lines": [{"text": "one", "emotion": "curious"}], "arc": ["curiosity", "clarity"]}
    ),
    "plan_validate": {
        "valid": True,
        "creativeReady": True,
        "creativeBlockers": [],
        "signalLog": {"driven": True, "blockers": []},
        "craftScore": 8.0,
    },
    "openvid_rank_for_slot": {
        "clips": [{
            "id": 7, "name": "city", "motion": 2.0, "camera": "pan_left",
            "source": "pexels", "seconds": 8.0, "slotRank": 1,
        }]
    },
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
    "track_add": lambda args: {"createdId": "t-voice" if args.get("bottom") else "t-motion"},
    "motion_reel": {"file": "/tmp/funcut-selftest/motion.mp4", "duration": 4.0},
    "media_import": {"createdId": "m-motion"},
    "clip_add": lambda args: {"createdId": "c-voice" if args.get("mediaId") == "m-voice" else "c-motion"},
    "export_start": {"job": "j1"},
    "export_wait": {"state": "done", "output": "/tmp/funcut-selftest/final.mp4"},
    "watch_render": {"pngPath": "/tmp/funcut-selftest/contact.png", "samples": 12},
    "craft_critique": {"overall": 8.5, "scores": {}, "notes": []},
}


def self_test() -> int:
    """A synthetic canonical plan exercises the real execution contract."""
    audio = "/tmp/funcut-selftest/voiceover.wav"
    transport = MockMcp(dict(SELF_TEST_REPLIES))
    calls = run_pipeline(transport, "/edits/SelfTest", audio, SELF_TEST_PLAN)
    by_tool = [(tool, args) for tool, args in calls]
    order = [tool for tool, _ in by_tool]
    expected = [
        "transcribe", "analysis_wait", "voice_signals", "emotion", "plan_validate",
        "project_get", "track_add", "clip_add",
        "openvid_rank_for_slot", "footage_continuity", "openvid_import",
        "clip_trim", "track_add", "motion_reel", "media_import", "clip_add",
        "export_start", "export_wait", "transcribe", "analysis_wait", "emotion",
        "watch_render", "craft_critique",
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
        if tool == "clip_add" and args.get("mediaId") == "m-voice"
    )
    if voice_track.get("path") != "/edits/SelfTest" or voice_placement.get("trackId") != "t-voice" or voice_placement.get("start") != 0.0:
        print(f"FAIL: voiceover was not placed on its own bottom lane: {voice_track}, {voice_placement}")
        return 1
    if "clip_cut_spans" in order:
        print("FAIL: the runner changed voiceover timing behind the plan's back")
        return 1
    print("voiceover ok: placed on its own lane without silently changing plan timing")

    rank = next(args for tool, args in by_tool if tool == "openvid_rank_for_slot")
    if rank["description"] != "A slow aerial drift across a dense city skyline at dusk":
        print(f"FAIL: rank did not use the segment's creative description: {rank}")
        return 1
    imported = next(args for tool, args in by_tool if tool == "openvid_import")
    if imported.get("ids") != [7] or imported.get("start") != 0.0:
        print(f"FAIL: selected footage was not imported at its segment start: {imported}")
        return 1
    print("stock ok: ranked result 7 imported and placed at 0.0s")

    trim = next(args for tool, args in by_tool if tool == "clip_trim")
    if trim.get("clipId") != "c-stock-7" or trim.get("delta") != -4.0:
        print(f"FAIL: footage was not trimmed to the segment duration: {trim}")
        return 1
    print("timing ok: eight-second source trimmed to the four-second story beat")

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

    waited = next(args for tool, args in by_tool if tool == "export_wait")
    if waited.get("job") != "j1":
        print(f"FAIL: export job was not awaited: {waited}")
        return 1
    verified = next(args for tool, args in by_tool if tool == "emotion" and args.get("action") == "verify")
    if verified.get("action") != "verify" or verified.get("toleranceMs") != 150:
        print(f"FAIL: exported speech was not verified against the source: {verified}")
        return 1
    print("export ok: job awaited, speech timing verified, render inspected and critiqued")

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

    repair_replies = dict(SELF_TEST_REPLIES)
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
        repaired, "/edits/SelfTest", audio, SELF_TEST_PLAN
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

    # A failure stops the run: no timeline or export work follows it.
    broken = MockMcp(dict(SELF_TEST_REPLIES), fail_on="emotion")
    try:
        run_pipeline(broken, "/edits/SelfTest", audio, SELF_TEST_PLAN)
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

    unready_replies = dict(SELF_TEST_REPLIES)
    unready_replies["plan_validate"] = {
        "valid": True,
        "creativeReady": False,
        "creativeBlockers": ["missing signal log"],
        "signalLog": {"driven": False, "blockers": ["missing signal log"]},
    }
    unready = MockMcp(unready_replies)
    try:
        run_pipeline(unready, "/edits/SelfTest", audio, SELF_TEST_PLAN)
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
    print("SELF-TEST PASS")
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
        return self_test()
    if not args.project or not args.plan:
        parser.error("--project and --plan are required unless --self-test")

    command = args.mcp_command or "concat --mcp auto"
    transport = McpStdio(command)
    transport.start()
    try:
        plan = load_plan(args.plan)
        if args.audio:
            try:
                run_pipeline(transport, args.project, args.audio, plan)
            except PipelineError as error:
                log(f"STOP: {error}")
                return 1
            return 0
        watch(transport, args.project, plan, args.poll)
    finally:
        transport.close()
    return 0


if __name__ == "__main__":
    sys.exit(main())
