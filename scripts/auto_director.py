#!/usr/bin/env python3
"""auto_director - the trigger the tool server does not have.

Dragging a voiceover into FUNCUT lands it in the project's bin and stops
there: the MCP server is a tool server, and nothing fires the director
pipeline on an import.  This script is that something.

THE FIRST QUESTION, ANSWERED: `media.import` is a REQUEST, not an EVENT.

docs/JSON-RPC.md does not exist in this repository, so the answer is read
from the protocol's own definition, `src/crates/concat-api/src/message.rs`:

- `media.import` is a variant of the `Request` enum
  (`Request::MediaImport { path, file }` - "Probes a file and adds it to
  a project's bin: what dropping a file on the window does").  A request
  is a method a client calls; the server never pushes it to anybody.
- The `Event` enum - what the server does push to every connected caller
  - carries job progress only: `cutout.progress`, `stabilize.progress`,
  `export.progress`, `export.done`, `export.failed`, `analysis.done`,
  `analysis.failed`.  There is no import event to subscribe to, and no
  event name is guessed here.

THE LIMITATION THAT FOLLOWS (the polling fallback): because there is no
`media.import` event, the script polls the project's media list -
`timeline_read`, whose reply carries the bin - every 2 seconds, and fires
the pipeline when a new audio file appears.  An import removed again
within one interval can be missed, and the pipeline starts up to one
interval late.  When an import event is added to the protocol, the loop
in `watch()` below is the piece to replace; everything after it stands.

The pipeline is the director skill's reading-and-plan loop over the MCP
server's own tools, in this order:

    transcribe
    voice_signals
    emotion            (action "extract")
    the plan           (an LLM call, injected - this script ships no model)
    clip_cut_spans     (only the pauses classified dead_air; a dramatic
                        hold, a scene turn, a breath and a beat are all
                        left in the take)
    plan_validate
    openvid_rank_for_slot   (once per broll slot, in plan order)
    footage_continuity
    motion_reel        (once per graphic slot)
    export             (export_start, then export_wait to the end)

Every phase is logged; the first failure stops the run.  The MCP server
is started as `concat --mcp` and spoken to over stdio - MCP is
newline-delimited JSON-RPC 2.0 (docs/MCP.md).

Usage:
    python3 scripts/auto_director.py watch --project /path/to/Cut
    python3 scripts/auto_director.py run --project /path/to/Cut \\
        --audio /path/to/voiceover.wav
    python3 scripts/auto_director.py selftest

`watch` is the drag-and-drop trigger: it needs the editor running with
Settings > Automation on, so `--mcp window` can reach the window's
server.  `run` fires the pipeline once on a file, no watching.  The plan
between the readings and `plan_validate` comes from `--plan FILE` (a
JSON edit plan per templates/edit_plan.schema.json) or the injected plan
writer; with neither, the run stops at the plan phase and says why
rather than inventing an edit.
"""

import argparse
import json
import os
import pathlib
import subprocess
import sys
import time

# What counts as a voiceover when a new file lands in the bin: the
# media kind the editor reports, or the file's own suffix.
AUDIO_EXTENSIONS = {
    ".wav", ".mp3", ".m4a", ".aac", ".flac", ".ogg", ".opus", ".wma",
    ".aiff", ".aif",
}

# How long one wait cycle asks a job about itself, and how long the whole
# wait may run before the phase is called failed (the tool's own ceiling
# is 600 s per call; the transcribe ceiling is 900 s per call).
WAIT_SLICE_SECONDS = 30
WAIT_CEILING_SECONDS = 900

# The MCP protocol version this client speaks; the server answers with
# it when it is one it knows (src/crates/concat-mcp/src/protocol.rs).
PROTOCOL_VERSION = "2025-11-25"


class DirectorError(RuntimeError):
    """A phase refused or failed; the run stops with this."""


class McpError(DirectorError):
    """A tool call answered with an error, or the transport broke."""


class McpClient:
    """A minimal MCP client over a `concat --mcp` subprocess's stdio.

    Newline-delimited JSON-RPC 2.0, the same wire docs/MCP.md describes:
    one `initialize`, one `notifications/initialized`, then `tools/call`
    per phase.  Notifications from the server (progress, cancellation)
    have no `id` and are skipped; responses are matched by `id`.
    """

    def __init__(self, command):
        self.command = command
        self.process = None
        self.next_id = 1

    def start(self):
        self.process = subprocess.Popen(
            self.command,
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=None,  # the server logs to stderr; let it show
            text=True,
            bufsize=1,
        )
        reply = self._request(
            "initialize",
            {
                "protocolVersion": PROTOCOL_VERSION,
                "capabilities": {},
                "clientInfo": {"name": "auto-director", "version": "1"},
            },
        )
        if "serverInfo" not in reply:
            raise McpError(f"the MCP server did not shake hands: {reply}")
        self._send(
            {"jsonrpc": "2.0", "method": "notifications/initialized"}
        )
        return reply["serverInfo"]

    def _send(self, message):
        if self.process is None or self.process.poll() is not None:
            raise McpError("the MCP server is not running")
        self.process.stdin.write(json.dumps(message) + "\n")
        self.process.stdin.flush()

    def _request(self, method, params):
        request_id = self.next_id
        self.next_id += 1
        self._send(
            {
                "jsonrpc": "2.0",
                "id": request_id,
                "method": method,
                "params": params,
            }
        )
        while True:
            line = self.process.stdout.readline()
            if not line:
                raise McpError("the MCP server closed its stdout")
            message = json.loads(line)
            if "id" not in message:
                continue  # a notification; not ours to answer
            if message["id"] != request_id:
                continue  # a response to a call we already finished
            if "error" in message:
                error = message["error"]
                raise McpError(
                    f"{method}: {error.get('code')}: "
                    f"{error.get('message')}"
                )
            return message.get("result", {})

    def call(self, name, arguments=None):
        """One `tools/call`; returns the tool's reply as received.

        A tool that answers with `isError` is a failed phase, not a
        failed transport - the text it carries is the reason.
        """
        result = self._request(
            "tools/call", {"name": name, "arguments": arguments or {}}
        )
        if result.get("isError"):
            text = " ".join(
                block.get("text", "")
                for block in result.get("content", [])
                if isinstance(block, dict)
            ).strip()
            raise McpError(f"{name}: {text or 'the tool reported an error'}")
        return result

    def close(self):
        if self.process is not None and self.process.poll() is not None:
            return
        if self.process is not None:
            self.process.stdin.close()
            self.process.wait(timeout=10)


def structured(reply):
    """A tool reply's structured half, or its text parsed, or {}."""
    if not isinstance(reply, dict):
        return {}
    value = reply.get("structuredContent")
    if isinstance(value, dict):
        return value
    for block in reply.get("content", []):
        if isinstance(block, dict) and "text" in block:
            try:
                parsed = json.loads(block["text"])
            except ValueError:
                continue
            if isinstance(parsed, dict):
                return parsed
    return {}


def wait_job(client, job, wait_tool, log):
    """Wait one job out through its wait tool, slice by slice.

    The reply's `output` is JSON text the job printed - a transcript, an
    export's file - and is parsed here; `running` means ask again.
    """
    waited = 0
    while True:
        status = structured(
            client.call(
                wait_tool,
                {"job": job, "timeoutSeconds": WAIT_SLICE_SECONDS},
            )
        )
        state = status.get("state", "")
        if state == "done":
            output = status.get("output")
            if isinstance(output, str) and output:
                try:
                    return json.loads(output)
                except ValueError:
                    return {"text": output}
            return status
        if state in ("failed", "cancelled"):
            raise DirectorError(
                f"job {job} ended {state}: {status.get('error', 'no reason')}"
            )
        waited += WAIT_SLICE_SECONDS
        if waited >= WAIT_CEILING_SECONDS:
            raise DirectorError(f"job {job} never finished in {waited}s")
        log(f"job {job} still {state} at {waited}s")


def is_audio(media):
    """A bin entry is a voiceover when the editor says so, or the file does."""
    kind = str(media.get("kind", "")).lower()
    if "audio" in kind:
        return True
    name = str(media.get("path") or media.get("name") or "")
    return pathlib.Path(name).suffix.lower() in AUDIO_EXTENSIONS


def dead_air_spans(signals):
    """The pauses voice_signals classified dead air, as [start, end] pairs.

    Only `dead_air` makes the cut.  A dramatic hold, a scene turn, a
    breath and a beat are delivery, not waste - the engine's own
    directorHints list the protected ones, and this respects them all.
    """
    spans = []
    for pause in signals.get("pauses") or []:
        if pause.get("class") == "dead_air":
            start, end = pause.get("start"), pause.get("end")
            if isinstance(start, (int, float)) and isinstance(
                end, (int, float)
            ):
                spans.append([float(start), float(end)])
    spans.sort()
    return spans


def plan_slots(plan):
    """The plan's slots, an empty list when the plan carries none."""
    slots = plan.get("slots")
    if not isinstance(slots, list):
        return []
    return [slot for slot in slots if isinstance(slot, dict)]


def storyboard_for(slot, plan):
    """One graphic slot as a one-scene storyboard motion_reel can render.

    The slot's own words become the comp's text; `at` and `hold` come
    from the slot's own timing, with the plan's canvas.
    """
    scene = {
        "preset": slot.get("preset", "funcut.lower-third.glass"),
        "at": slot.get("start", 0.0),
        "hold": slot.get("durationS", 2.5),
        "ops": {"text": slot.get("description", slot.get("slotId", ""))},
    }
    return {
        "scenes": [scene],
        "width": plan.get("width", 1080),
        "height": plan.get("height", 1920),
        "fps": plan.get("fps", 30),
    }


class Director:
    """The pipeline, one phase per method, in the prescribed order.

    `client` is anything with a `call(name, arguments)` that returns an
    MCP reply - the real stdio client, or the selftest's scripted one.
    `plan_writer` is the LLM call: (audio, transcript, signals, emotion)
    -> an edit-plan object per templates/edit_plan.schema.json.
    """

    def __init__(self, client, project, plan_writer, log=None):
        self.client = client
        self.project = project
        self.plan_writer = plan_writer
        self.log = log or (lambda line: print(f"auto-director: {line}"))

    def phase(self, name, line):
        self.log(f"[{name}] {line}")

    def tool(self, name, arguments):
        return structured(self.client.call(name, arguments))

    def run(self, audio):
        """Fire the whole pipeline on one voiceover; stop on any failure."""
        transcript = self._transcribe(audio)
        signals = self._voice_signals(audio, transcript)
        emotion = self._emotion(transcript)
        plan = self._plan(audio, transcript, signals, emotion)
        self._cut_dead_air(audio, signals)
        self._validate(plan)
        manifest = self._shop_broll(plan)
        self._continuity(manifest)
        self._render_graphics(plan)
        output = self._export(audio)
        return {"audio": audio, "output": output}

    # -- the phases, in order ------------------------------------------------

    def _transcribe(self, audio):
        self.phase("transcribe", audio)
        started = self.tool("transcribe", {"path": audio})
        job = started.get("job")
        if not job:
            return started if started else None
        return self._wait(job, "analysis_wait")

    def _wait(self, job, wait_tool):
        if not job:
            raise DirectorError("the tool started no job")
        result = wait_job(self.client, job, wait_tool, self.log)
        self.phase("wait", f"job {job} done")
        return result

    def _voice_signals(self, audio, transcript):
        self.phase("voice_signals", "the acoustic read of the take")
        return self.tool(
            "voice_signals",
            {"action": "extract", "audio": audio, "transcript": transcript},
        )

    def _emotion(self, transcript):
        self.phase("emotion", "the emotional read of the take")
        return self.tool(
            "emotion", {"action": "extract", "transcript": transcript}
        )

    def _plan(self, audio, transcript, signals, emotion):
        self.phase("plan", "drafting the edit plan (the LLM call)")
        if self.plan_writer is None:
            raise DirectorError(
                "no plan writer: pass --plan FILE with a JSON edit plan, "
                "or inject one - the script will not invent an edit"
            )
        plan = self.plan_writer(audio, transcript, signals, emotion)
        if not isinstance(plan, dict):
            raise DirectorError("the plan writer returned no plan object")
        return plan

    def _cut_dead_air(self, audio, signals):
        self.phase(
            "clip_cut_spans",
            "only the dead air; dramatic holds stay in the take",
        )
        clip_id = self._voiceover_clip(audio)
        spans = dead_air_spans(signals)
        if not spans:
            self.phase(
                "clip_cut_spans", "no dead air to cut; the take stays whole"
            )
            return
        self.tool(
            "clip_cut_spans",
            {
                "path": self.project,
                "clipId": clip_id,
                "spans": spans,
                "ripple": True,
            },
        )
        self.phase(
            "clip_cut_spans", f"cut {len(spans)} dead-air span(s), ripple on"
        )

    def _voiceover_clip(self, audio):
        """The voiceover's clip id, placing it first if the bin never did.

        Dragging a file onto the window lands it in the bin; the cut
        tools work on a clip, so the take is laid on the timeline at 0
        when it is not already there.
        """
        state = self.tool("timeline_read", {"path": self.project})
        project = state.get("project", state)
        media_id = None
        for media in project.get("media", []):
            path = str(media.get("path") or "")
            if path == audio or pathlib.Path(path).name == pathlib.Path(
                audio
            ).name:
                media_id = media.get("id")
                break
        if media_id is None:
            raise DirectorError(
                f"{audio} is not in the project's bin; import it first"
            )
        for clip in project.get("clips", []):
            if clip.get("mediaId") == media_id:
                return clip.get("id")
        placed = self.tool(
            "clip_add",
            {"path": self.project, "mediaId": media_id, "start": 0.0},
        )
        clip_id = placed.get("clipId") or placed.get("id")
        if not clip_id:
            raise DirectorError(
                f"the voiceover could not be placed: {placed}"
            )
        self.phase("clip_cut_spans", f"placed the take as clip {clip_id}")
        return clip_id

    def _validate(self, plan):
        self.phase("plan_validate", "the plan's schema and readiness")
        verdict = self.tool("plan_validate", {"plan": plan})
        if verdict.get("valid") is not True:
            raise DirectorError(
                f"the plan is not valid: {verdict.get('problems', verdict)}"
            )
        if verdict.get("creativeReady") is False:
            raise DirectorError(
                "the plan is not creativeReady: "
                f"{verdict.get('creativeBlockers', 'no blockers listed')}"
            )
        self.phase(
            "plan_validate",
            f"valid and creativeReady "
            f"(craft {verdict.get('craftScore', '?')})",
        )

    def _shop_broll(self, plan):
        self.phase(
            "openvid_rank_for_slot", "shopping every broll slot, in order"
        )
        manifest = []
        used = []
        for slot in plan_slots(plan):
            if slot.get("treatment") != "broll":
                continue
            description = slot.get("description", "").strip()
            if not description:
                raise DirectorError(
                    f"slot {slot.get('slotId', '?')} shops with no description"
                )
            arguments = {"description": description}
            if slot.get("queries"):
                arguments["queries"] = slot["queries"]
            if slot.get("hero") is not None:
                arguments["hero"] = slot["hero"]
            if slot.get("motionMin") is not None:
                arguments["motion_min"] = slot["motionMin"]
            if used:
                arguments["exclude_ids"] = list(used)
            page = self.tool("openvid_rank_for_slot", arguments)
            clips = page.get("clips") or []
            if not clips:
                raise DirectorError(
                    f"slot {slot.get('slotId', '?')} found nothing to show"
                )
            chosen = clips[0]
            used.append(chosen.get("id"))
            manifest.append(
                {
                    "slotId": slot.get("slotId", ""),
                    "clipId": chosen.get("id"),
                    "motion": chosen.get("motion"),
                    "motionMin": slot.get("motionMin"),
                }
            )
            self.phase(
                "openvid_rank_for_slot",
                f"slot {slot.get('slotId', '?')} -> clip "
                f"{chosen.get('id')} (slotRank {chosen.get('slotRank')})",
            )
        return manifest

    def _continuity(self, manifest):
        self.phase("footage_continuity", "the plan-level continuity gate")
        if not manifest:
            self.phase(
                "footage_continuity",
                "nothing was shopped; nothing to gate",
            )
            return
        verdict = self.tool("footage_continuity", {"clips": manifest})
        count = verdict.get("count", 0)
        if count:
            self.phase(
                "footage_continuity",
                f"{count} flag(s); the remedies are the director's next "
                f"pass: {json.dumps(verdict.get('flags', []))[:400]}",
            )
        else:
            self.phase("footage_continuity", "clean")

    def _render_graphics(self, plan):
        graphics = [
            slot
            for slot in plan_slots(plan)
            if slot.get("treatment") == "graphic"
        ]
        if not graphics:
            self.phase("motion_reel", "no graphic slots; nothing to render")
            return
        for slot in graphics:
            self.phase(
                "motion_reel",
                f"rendering slot {slot.get('slotId', '?')}",
            )
            self.tool(
                "motion_reel",
                {
                    "project": self.project,
                    "storyboard": storyboard_for(slot, plan),
                },
            )

    def _export(self, audio):
        self.phase("export", "the cut, written out")
        stem = pathlib.Path(audio).stem or "voiceover"
        output = os.path.join(self.project, f"{stem}-auto-director.mp4")
        started = self.tool(
            "export_start", {"path": self.project, "output": output}
        )
        job = started.get("job")
        if not job:
            raise DirectorError(f"the export started no job: {started}")
        done = self._wait(job, "export_wait")
        # A job's `output` is JSON text when the job printed JSON (a
        # transcript) and a bare path when it wrote a file (an export);
        # wait_job hands the bare string back as `text`.
        written = done.get("output") or done.get("text") or output
        self.phase("export", f"written: {written}")
        return written


def watch(client, project, plan_writer, interval=2.0, log=None):
    """Poll the project's media list; fire the pipeline on new audio.

    This is the REQUEST-only fallback: no media.import event exists to
    subscribe to (see the module docstring), so the bin is read every
    `interval` seconds.  Media already in the bin when the watch starts
    is not re-run.
    """
    director = Director(client, project, plan_writer, log=log)
    state = structured(client.call("timeline_read", {"path": project}))
    project_view = state.get("project", state)
    seen = {
        media.get("id")
        for media in project_view.get("media", [])
        if is_audio(media)
    }
    director.log(
        f"watching {project} every {interval}s "
        f"({len(seen)} audio file(s) already in the bin)"
    )
    while True:
        time.sleep(interval)
        state = structured(client.call("timeline_read", {"path": project}))
        project_view = state.get("project", state)
        for media in project_view.get("media", []):
            if media.get("id") in seen or not is_audio(media):
                continue
            seen.add(media.get("id"))
            audio = media.get("path") or media.get("name")
            director.log(f"new audio in the bin: {audio}")
            director.run(audio)


def plan_from_file(path):
    """A plan writer that reads the edit plan from a JSON file."""
    document = json.loads(pathlib.Path(path).read_text())

    def writer(audio, transcript, signals, emotion):
        return document

    return writer


# -- selftest -----------------------------------------------------------------
#
# The repo's own contract for a Python tool (every sense sidecar carries
# one): `selftest` proves the whole chain on fixtures, with nothing real
# - no MCP server, no model, no ffmpeg.  The MCP client is a scripted
# fake that records every call; the LLM call is a mock that returns a
# fixed plan.  What it proves: a synthetic audio path drives the phases
# in the prescribed order, the dead-air cut never touches a dramatic
# hold, the slots shop one search each with the variety law held, and a
# failure stops the run.


class ScriptedClient:
    """The MCP client as a script: every call answered from a queue and
    recorded, so the selftest can assert on both the replies' consumers
    and the exact calls the pipeline made."""

    def __init__(self, replies):
        self.replies = list(replies)
        self.calls = []

    def call(self, name, arguments=None):
        self.calls.append((name, arguments or {}))
        if not self.replies:
            raise AssertionError(
                f"the pipeline called {name} but the script has no reply"
            )
        reply = self.replies.pop(0)
        if isinstance(reply, Exception):
            raise reply
        return reply


def selftest():
    audio = "/tmp/funcut-selftest/voiceover.wav"
    project = "/tmp/funcut-selftest/Project"
    transcript = {
        "segments": [
            {"start": 0.0, "end": 2.4, "text": "The hook line."},
            {"start": 3.9, "end": 6.1, "text": "The body line."},
        ]
    }
    signals = {
        "pauses": [
            {"start": 2.4, "end": 3.9, "class": "dead_air"},
            {"start": 1.1, "end": 1.9, "class": "dramatic"},
            {"start": 0.4, "end": 0.7, "class": "breath"},
            {"start": 5.2, "end": 5.5, "class": "scene_turn"},
        ],
        "directorHints": {
            "protectedPauses": [
                {"class": "dramatic"},
                {"class": "scene_turn"},
            ]
        },
    }
    emotion = {"lines": [{"emotion": "curious", "act": "hook"}], "arc": {}}
    plan = {
        "aspect": "9:16",
        "mode": "reel",
        "sections": [{"role": "hook", "start": 0.0, "end": 6.1}],
        "segments": [
            {
                "start": 0.0,
                "end": 6.1,
                "visual": {"type": "broll", "query": "city skyline"},
            },
        ],
        "audio": {},
        "grade": {
            "filter": "concat.teal-orange",
            "intensityKey": "amount",
            "intensity": 65,
        },
        "slots": [
            {
                "slotId": "hook.1",
                "treatment": "broll",
                "durationS": 2.4,
                "description": "a slow aerial drift across the city "
                "skyline at dusk",
                "queries": ["city skyline aerial"],
                "motionMin": 1.0,
            },
            {
                "slotId": "body.1",
                "treatment": "broll",
                "durationS": 2.2,
                "description": "a close detail of rain on a dark window",
                "queries": ["rain window close"],
            },
            {
                "slotId": "title.1",
                "treatment": "graphic",
                "durationS": 2.0,
                "description": "THE SALT STAYS",
            },
        ],
    }
    timeline = {
        "project": {
            "media": [
                {
                    "id": "m-vo",
                    "kind": "Audio",
                    "path": audio,
                    "name": "voiceover.wav",
                }
            ],
            "clips": [],
        }
    }
    replies = [
        {"structuredContent": {"job": "j-transcribe"}},           # transcribe
        {
            "structuredContent": {
                "state": "done",
                "output": json.dumps(transcript),
            }
        },                                                        # wait
        {"structuredContent": signals},                    # voice_signals
        {"structuredContent": emotion},                          # emotion
        {"structuredContent": timeline},                   # timeline_read
        {"structuredContent": {"clipId": "c-vo"}},               # clip_add
        {"structuredContent": {"clips": []}},              # clip_cut_spans
        {
            "structuredContent": {
                "valid": True,
                "creativeReady": True,
                "craftScore": 7.5,
            }
        },                                               # plan_validate
        {
            "structuredContent": {
                "clips": [
                    {"id": 101, "slotRank": 1, "motion": 2.4},
                    {"id": 102, "slotRank": 2, "motion": 1.8},
                ]
            }
        },                                              # rank hook.1
        {
            "structuredContent": {
                "clips": [{"id": 205, "slotRank": 1, "motion": 3.1}]
            }
        },                                              # rank body.1
        {"structuredContent": {"count": 0, "flags": []}},  # continuity
        {"structuredContent": {"scenes": 1, "duration": 2.0}},  # motion_reel
        {"structuredContent": {"job": "j-export"}},        # export_start
        {
            "structuredContent": {
                "state": "done",
                "output": "/tmp/funcut-selftest/out.mp4",
            }
        },                                               # export_wait
    ]
    client = ScriptedClient(replies)
    logged = []

    def mock_plan_writer(audio_arg, transcript_arg, signals_arg, emotion_arg):
        logged.append("[plan] the mocked LLM call")
        assert transcript_arg == transcript, "the plan writer saw the reading"
        assert signals_arg == signals, "the plan writer saw the signals"
        return plan

    director = Director(
        client,
        project,
        plan_writer=mock_plan_writer,
        log=logged.append,
    )
    result = director.run(audio)

    # The phases, in the prescribed order, as the tools the pipeline
    # called them (the plan is the mocked LLM between emotion and the
    # cut; timeline_read and clip_add are the cut's own setup).
    order = [name for name, _ in client.calls]
    expected = [
        "transcribe",
        "analysis_wait",
        "voice_signals",
        "emotion",
        "timeline_read",
        "clip_add",
        "clip_cut_spans",
        "plan_validate",
        "openvid_rank_for_slot",
        "openvid_rank_for_slot",
        "footage_continuity",
        "motion_reel",
        "export_start",
        "export_wait",
    ]
    assert order == expected, (
        f"phases out of order:\n  ran: {order}\n  want: {expected}"
    )
    assert any("[plan] the mocked LLM call" in line for line in logged)

    # The cut took only the dead air: the dramatic hold, the breath and
    # the scene turn all stayed in the take.
    cut = client.calls[6][1]
    assert cut["spans"] == [[2.4, 3.9]], cut["spans"]
    assert cut["ripple"] is True and cut["clipId"] == "c-vo"

    # The slots shopped one search each, and the second carried the
    # first's chosen clip on its exclude list - the variety law.
    first, second = client.calls[8][1], client.calls[9][1]
    assert first["description"].startswith("a slow aerial drift")
    assert "exclude_ids" not in first
    assert second["exclude_ids"] == [101], second

    # The continuity gate read a manifest with both shopped slots.
    manifest = client.calls[10][1]["clips"]
    assert [entry["clipId"] for entry in manifest] == [101, 205], manifest

    # The graphic slot rendered one reel with its own words.
    board = client.calls[11][1]["storyboard"]
    assert board["scenes"][0]["ops"]["text"] == "THE SALT STAYS"

    # The export went to the project's own folder, and the run said so.
    assert client.calls[12][1]["output"].endswith(
        "voiceover-auto-director.mp4"
    )
    assert result["output"] == "/tmp/funcut-selftest/out.mp4"

    # A failure stops the run: a failed transcribe never reaches the
    # readings, the cut or the export.
    failing = ScriptedClient(
        [
            {"structuredContent": {"job": "j2"}},
            {
                "structuredContent": {
                    "state": "failed",
                    "error": "no whisper model",
                }
            },
        ]
    )
    stopped = Director(
        failing, project, plan_writer=mock_plan_writer, log=logged.append
    )
    try:
        stopped.run(audio)
        raise AssertionError("a failed transcribe must stop the run")
    except DirectorError as error:
        assert "whisper" in str(error), error
    assert [name for name, _ in failing.calls] == [
        "transcribe",
        "analysis_wait",
    ], "nothing may run after a failure"

    print(
        "auto_director selftest: phases in order, dead air only cut, "
        "slots shopped, graphics rendered, export waited, failure stops"
    )


def main(argv=None):
    parser = argparse.ArgumentParser(
        description="Fire the director pipeline when a voiceover lands."
    )
    parser.add_argument(
        "command",
        choices=["watch", "run", "selftest"],
        help="watch the bin for new audio, run the pipeline once, or "
        "prove the chain with selftest",
    )
    parser.add_argument("--project", help="the project folder to work in")
    parser.add_argument("--audio", help="the voiceover file (run)")
    parser.add_argument(
        "--plan",
        help="a JSON edit plan for the plan phase (no LLM is shipped)",
    )
    parser.add_argument(
        "--concat",
        default="concat",
        help="the concat binary to start the MCP server with",
    )
    parser.add_argument(
        "--mcp-mode",
        choices=["auto", "window", "headless"],
        default="auto",
        help="which MCP mode concat runs in",
    )
    parser.add_argument(
        "--interval",
        type=float,
        default=2.0,
        help="seconds between media-list polls (watch)",
    )
    args = parser.parse_args(argv)

    if args.command == "selftest":
        selftest()
        return 0

    if not args.project:
        parser.error("watch and run need --project")
    if args.command == "run" and not args.audio:
        parser.error("run needs --audio")

    plan_writer = plan_from_file(args.plan) if args.plan else None
    client = McpClient([args.concat, "--mcp", args.mcp_mode])
    try:
        server = client.start()
        print(
            f"auto-director: MCP server {server.get('name', '?')} "
            f"{server.get('version', '?')}"
        )
        if args.command == "run":
            Director(client, args.project, plan_writer).run(args.audio)
        else:
            watch(client, args.project, plan_writer, interval=args.interval)
    finally:
        client.close()
    return 0


if __name__ == "__main__":
    sys.exit(main())
