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
1.  `transcribe`            the voiceover's words and timings
2.  `voice_signals`         the take read as a score (pauses, emphasis)
3.  `emotion` (extract)     what every line feels
4.  `clip_cut_spans`        cut ONLY dead air - pauses the reading
                            classified `dead_air`; dramatic holds are
                            protected and never cut
5.  `plan_validate`         the edit plan (from `--plan`) against the
                            schema and the creative floors
6.  `openvid_rank_for_slot` once per `broll` slot in the plan
7.  `footage_continuity`    the filled slots read the way an editor scrubs
8.  `motion_reel`           once per `graphic` slot in the plan
9.  `export_start`          the finished timeline to a movie

The calls go through the editor's MCP server over stdio (newline-delimited
JSON-RPC 2.0 - `tools/call`), the same surface the funcut-director skill
drives; `--mcp-command` says how it is started, and defaults to
`concat --mcp auto`.
"""
from __future__ import annotations

import argparse
import json
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


def broll_slots(plan: dict[str, Any]) -> list[dict[str, Any]]:
    return [s for s in plan.get("slots", []) if s.get("treatment") == "broll"]


def graphic_slots(plan: dict[str, Any]) -> list[dict[str, Any]]:
    return [s for s in plan.get("slots", []) if s.get("treatment") == "graphic"]


def dead_air_spans(voice_reply: Any) -> tuple[list[list[float]], int]:
    """The spans `clip_cut_spans` may cut: `dead_air` pauses only.

    A dramatic hold is a pause the reading kept on purpose - the beat the
    edit breathes through - and protecting it is the difference between a
    tight take and a flattened one. Returns the cut spans and how many
    holds were protected, for the log.
    """
    if not isinstance(voice_reply, dict):
        return [], 0
    pauses = voice_reply.get("pauses")
    if pauses is None:
        signals = voice_reply.get("signals")
        pauses = signals.get("pauses") if isinstance(signals, dict) else None
    if not isinstance(pauses, list):
        return [], 0
    spans: list[list[float]] = []
    holds = 0
    for pause in pauses:
        if not isinstance(pause, dict):
            continue
        if pause.get("class") == "dead_air":
            spans.append([float(pause["start"]), float(pause["end"])])
        else:
            holds += 1
    return spans, holds


# --------------------------------------------------------------- the pipeline


def phase_plan(plan: dict[str, Any]) -> list[tuple[str, str]]:
    """The concrete phase list for one plan: the nine phases in the fixed
    order, with the per-slot phases repeated once per slot they serve -
    so the run's numbering and the log name what actually happens."""
    phases = [
        ("transcribe", "transcribe"),
        ("voice_signals", "voice_signals"),
        ("emotion", "emotion (extract)"),
        ("plan_validate", "plan_validate (before timeline edits)"),
        ("clip_cut_spans", "clip_cut_spans (dead air only)"),
    ]
    for slot in broll_slots(plan):
        phases.append(
            (
                "openvid_rank_for_slot",
                f"openvid_rank_for_slot ({slot.get('slotId', '?')})",
            )
        )
    phases.append(("footage_continuity", "footage_continuity"))
    for slot in graphic_slots(plan):
        phases.append(
            ("motion_reel", f"motion_reel ({slot.get('slotId', '?')})")
        )
    phases.append(("export_start", "export"))
    return phases


def run_pipeline(
    transport: Any,
    project: str,
    audio: str,
    plan: dict[str, Any],
) -> list[tuple[str, dict[str, Any]]]:
    """The phases, in order, logged; the first failure stops the run.

    Returns the calls that were made - the self-test's evidence, and the
    honest count when a run stops partway.
    """
    phases = phase_plan(plan)
    total = len(phases)
    done: list[tuple[str, dict[str, Any]]] = []
    step = 0

    def advance(tool: str, arguments: dict[str, Any]) -> Any:
        nonlocal step
        step += 1
        log(f"phase {step}/{total}: {phases[step - 1][1]}")
        reply = transport.call(tool, arguments)
        done.append((tool, arguments))
        return reply

    # 1-2: the words, then the score they are spoken to.
    transcript = advance("transcribe", {"path": audio})
    voice = advance(
        "voice_signals",
        {"audio": audio, "transcript": transcript},
    )

    # 3: what every line feels - retain the reading for the run report.
    emotion = advance(
        "emotion",
        {"action": "extract", "transcript": transcript, "project": project},
    )
    if isinstance(emotion, dict):
        arc = emotion.get("arc")
        if arc is not None:
            log(f"voice emotion arc: {json.dumps(arc, ensure_ascii=False)}")

    # 4: validate before any timeline mutation. A structurally valid but
    # creatively unready plan is not safe to execute.
    validation = advance("plan_validate", {"plan": plan})
    if not isinstance(validation, dict):
        raise PipelineError("plan_validate returned no structured result")
    if validation.get("valid") is not True:
        raise PipelineError(
            "plan_validate refused the plan: "
            + json.dumps(validation.get("errors", validation), ensure_ascii=False)
        )
    if validation.get("creativeReady") is not True:
        raise PipelineError(
            "plan_validate says creativeReady is not true: "
            + json.dumps(
                validation.get("creativeBlockers", validation.get("blockers", [])),
                ensure_ascii=False,
            )
        )

    # 5: cut the dead air only; the dramatic holds ride untouched.
    spans, holds = dead_air_spans(voice)
    log(f"dead air: {len(spans)} span(s) to cut, {holds} dramatic hold(s) protected")
    advance(
        "clip_cut_spans",
        {
            "path": project,
            "clipId": plan.get("audioClipId", ""),
            "spans": spans,
            "ripple": True,
        },
    )

    # 6: shop every broll slot; each chosen clip is excluded from the next.
    excluded: list[int] = []
    chosen: dict[str, Any] = {}
    for slot in broll_slots(plan):
        reply = advance(
            "openvid_rank_for_slot",
            {
                "description": slot.get("description", ""),
                "queries": slot.get("queries", []),
                "hero": bool(slot.get("hero", False)),
                "motion_min": slot.get("motionMin"),
                "exclude_ids": list(excluded),
            },
        )
        clips = reply.get("clips", []) if isinstance(reply, dict) else []
        if clips:
            chosen[slot.get("slotId", "?")] = clips[0]
            clip_id = clips[0].get("id")
            if isinstance(clip_id, int):
                excluded.append(clip_id)

    # 7: the filled manifest, read the way an editor scrubs it.
    manifest = [
        {
            "slotId": slot_id,
            "clipId": clip.get("name", clip.get("id")),
            "motion": clip.get("motion"),
            "shotScale": clip.get("shotScale"),
            "category": clip.get("category"),
        }
        for slot_id, clip in chosen.items()
    ]
    advance("footage_continuity", {"clips": manifest})

    # 8: a reel for every graphic slot the plan carries.
    for slot in graphic_slots(plan):
        advance(
            "motion_reel",
            {
                "project": project,
                "storyboard": slot.get("storyboard", {}),
                "name": slot.get("slotId", "reel"),
            },
        )

    # 9: the finished timeline, out.
    advance("export_start", {"path": project})
    log(f"pipeline complete: {len(done)} call(s) over {total} phases")
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
                run_pipeline(transport, project, audio, plan)
            except PipelineError as error:
                log(f"STOP: {error}")
                log("the run stopped at its failure; nothing later ran")


# ----------------------------------------------------------------- self-test


SELF_TEST_PLAN = {
    "aspect": "9:16",
    "mode": "reel",
    "sections": [{"role": "hook", "start": 0.0, "end": 4.0}],
    "segments": [{"start": 0.0, "end": 4.0, "cutType": "hard"}],
    "audio": {"clipId": "c1"},
    "grade": {"filter": "warm", "intensityKey": "medium", "intensity": 0.5},
    "audioClipId": "c1",
    "slots": [
        {
            "slotId": "hook.1",
            "treatment": "broll",
            "description": "a slow aerial drift across the city skyline at dusk",
            "queries": ["city skyline aerial"],
            "motionMin": 1.0,
        },
        {
            "slotId": "reveal.hero",
            "treatment": "broll",
            "hero": True,
            "description": "a decision landing on one worried face, close up",
            "queries": ["worried face closeup"],
        },
        {
            "slotId": "title.card",
            "treatment": "graphic",
            "storyboard": {"scenes": [{"id": "s1", "kind": "title"}]},
        },
    ],
}

SELF_TEST_REPLIES = {
    "project_get": {"project": {}, "timeline": {}, "media": []},
    "transcribe": {"segments": [{"text": "one", "start": 0.0, "end": 1.0}]},
    "voice_signals": {
        "signals": {
            "pauses": [
                {"class": "dead_air", "start": 1.2, "end": 2.4},
                {"class": "dramatic_hold", "start": 4.0, "end": 5.1},
                {"class": "dead_air", "start": 6.0, "end": 6.5},
            ]
        }
    },
    "emotion": {"lines": [{"text": "one", "emotion": "curious"}]},
    "clip_cut_spans": {"ok": True},
    "plan_validate": {"valid": True, "creativeReady": True},
    "openvid_rank_for_slot": {
        "clips": [{"id": 7, "name": "city", "motion": 2.0, "slotRank": 1}]
    },
    "footage_continuity": {"count": 0, "flags": []},
    "motion_reel": {"path": "/tmp/reel.mp4"},
    "export_start": {"job": "j1"},
}


def self_test() -> int:
    """A synthetic audio path through a mocked server: the order is the test."""
    audio = "/tmp/funcut-selftest/voiceover.wav"
    transport = MockMcp(dict(SELF_TEST_REPLIES))

    calls = run_pipeline(transport, "/edits/SelfTest", audio, SELF_TEST_PLAN)
    order = [tool for tool, _ in calls]

    expected = [
        "transcribe",
        "voice_signals",
        "emotion",
        "plan_validate",
        "clip_cut_spans",
        "openvid_rank_for_slot",
        "openvid_rank_for_slot",  # once per broll slot: two
        "footage_continuity",
        "motion_reel",
        "export_start",
    ]
    if order != expected:
        print(f"FAIL: phase order\n  want: {expected}\n  got:  {order}")
        return 1
    print(f"order ok: {' -> '.join(order)}")

    # The dead air is cut, the dramatic hold is not.
    cut = dict(calls)["clip_cut_spans"]["spans"]
    if cut != [[1.2, 2.4], [6.0, 6.5]]:
        print(f"FAIL: dead-air spans {cut} - the dramatic hold must survive")
        return 1
    print("dead air ok: 2 spans cut, the dramatic hold protected")

    # The second broll slot shopped with the first's choice excluded.
    ranks = [args for tool, args in calls if tool == "openvid_rank_for_slot"]
    if ranks[1].get("exclude_ids") != [7]:
        print(f"FAIL: the second slot did not exclude the first's clip: {ranks[1]}")
        return 1
    print("slots ok: the hero slot shopped past the first choice")

    # A failure stops the run: nothing later than the failing phase runs.
    broken = MockMcp(dict(SELF_TEST_REPLIES), fail_on="emotion")
    try:
        run_pipeline(broken, "/edits/SelfTest", audio, SELF_TEST_PLAN)
        print("FAIL: a failing phase did not stop the pipeline")
        return 1
    except PipelineError:
        later = [tool for tool, _ in broken.calls if tool in {
            "plan_validate", "clip_cut_spans", "footage_continuity",
            "motion_reel", "export_start",
        } or tool == "openvid_rank_for_slot"]
        if later:
            print(f"FAIL: phases ran past the failure: {later}")
            return 1
    print("failure ok: the run stopped at emotion, nothing later ran")

    # A plan that is not creatively ready must stop BEFORE the first edit.
    unready_replies = dict(SELF_TEST_REPLIES)
    unready_replies["plan_validate"] = {
        "valid": True,
        "creativeReady": False,
        "creativeBlockers": ["missing signal log"],
    }
    unready = MockMcp(unready_replies)
    try:
        run_pipeline(unready, "/edits/SelfTest", audio, SELF_TEST_PLAN)
        print("FAIL: a creatively unready plan was executed")
        return 1
    except PipelineError:
        mutated = [
            tool for tool, _ in unready.calls
            if tool in {"clip_cut_spans", "openvid_import", "clip_add", "edit_apply"}
        ]
        if mutated:
            print(f"FAIL: timeline mutation preceded readiness: {mutated}")
            return 1
    print("readiness gate ok: no timeline mutation before creativeReady")

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
