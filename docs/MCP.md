# Concat for AI agents (MCP)

Concat ships an [MCP](https://modelcontextprotocol.io) server inside the app. An agent - Claude Code, Claude Desktop, or any other MCP client - starts it as `concat --mcp` and can then create projects, import media, cut, add titles and captions, keyframe, apply effects, look at frames and export, with the same edit commands the window uses. Nothing extra to install.

### Stock clips (OpenVid)

The `openvid` tools search 937,957 stock clips (the OpenVid-1M dataset on
Hugging Face) by caption and quality numbers and fetch only what is used.
The first call on a machine is `openvid_prepare`, which downloads a 435 MB
local index once (poll `openvid_status` until `ready`); after that every
search is local and instant. A typical run: `openvid_search` with words
and quality floors ("sunset ocean", `minAesthetic: 5.2`) → `openvid_get`
to read the best candidates in full → `openvid_import` with `start: 0` to
lay them on a lane in one undoable step. Frame sizes are measured from
each clip's own MP4 header; the first search with an `orientation` filter
also takes a one-time census of every shelf (a few seconds), after which
`square` finds the three 512x512 shelves and `horizontal` the WebVid
shelf's 720p/1080p landscape footage - nothing vertical has been measured
in it, and an honest vertical search returns nothing rather than
everything.

### Sound effects

The `sounds` tools search the built-in CC0 sound library this repository
ships (see `docs/AUDIO_LIBRARY.md`) and place what they find:
`sounds_search` with words against the name, tags, use-for lines and
description ("cinematic impact", "click", "soft swish for a card
transition" - the little words are dropped, and where a word matched
ranks it: the name counts most, then the tags, then the description - or
a `category`: Whoosh, Impact, Riser, Motion, UI, Notification, Keyboard,
Digital, Foley, Writing, Ambience, Music) → `sounds_add` with `start: 0`
to lay the sound on a lane in one undoable step. Each match carries what
an agent needs to pick without hearing anything: a `description` of what
the sound is, `useFor` lines saying what it is for, and `syncAt`, seconds
in where its peak lands, for placing it by the moment it is for. The
first use of a sound downloads it (a few hundred kilobytes at most,
checked against its SHA-256) and keeps it in the app's cache; every sound
is CC0, so nothing asks for attribution.
### Background music (FreePD and MobyGratis)

The `bgm` tools search 1,191 whole CC0 music tracks on two shelves and
fetch only what is used. FreePD is the Internet Archive's mirror of Kevin
MacLeod's CC0 library (~1,000 tracks, grouped as freepd.com grouped them:
upbeat, epic, comedy, scoring, world…); Lo-Fi is Moby's MobyGratis
instrumentals as the OpenLo-Fi pack (166 tracks, 10 categories). Nothing
is bundled: `bgm_prepare` fetches both shelves' catalogs once (a few MB,
plus a one-time measuring of the Lo-Fi pack's zip, a few seconds; poll
`bgm_status` until both are `ready`), and a track's audio arrives only
when it is picked - FreePD as its own file, a Lo-Fi track as **one ranged
read of exactly that track's span out of the pack's 554 MB zip**, never
the whole zip. A typical run: `bgm_search` with words and a length
("city rain", `maxSeconds: 180`, `source: "lofi"`) → `bgm_fetch` to check
a candidate's `localPath` (or just listen: the same file previews in the
window) → `bgm_import` with `start: 0` to lay it on a lane in one
undoable step. `surprise` as the sort is the shuffle that picks when you
cannot; a track whose length is unknown is passed by a duration filter
rather than guessed at.

### Stickers (MAGICK and OpenMoji)

The `magick` tools search 149,758 transparent-PNG overlays on two
shelves and fetch only what is used. MAGICK is OneOverZero's dataset of
145,193 captioned AI-generated pictures with the backgrounds already
taken away (people, animals, objects, gestures); OpenMoji is the
project's colour emoji set (4,565, CC-BY 4.0, and its tags are searched:
"smile" finds the grinning face). Nothing is bundled: `magick_prepare`
fetches both indexes once (~18 MB; poll `magick_status` until both are
`ready`), and a picture's PNG arrives only when it is picked - one plain
GET, ~1 MB for a MAGICK picture, a few KB for an emoji, checked to be a
PNG that can hold transparency before it is kept. A typical run:
`magick_search` with words ("red balloon", `source: "magick"`) →
`magick_add` with `id`, `start` and a placement (`scale: 0.3,
offsetX: 0.2`, `duration: 3`), which lays the picture *over* the video -
the first free lane above the highest occupied, never beneath it - as
one undoable step beside the clip's own. MAGICK's pictures are
AI-generated and CC BY-NC 4.0: fine for personal video, not for
commercial use; OpenMoji is CC-BY 4.0. The window's Stickers page
searches the same engine, and its heart button keeps the favourites the
`favourites` filter narrows by.

### Analysis and speech (the agent's senses)

Six tools give an agent what a director has before touching a timeline: the
shot list, the beat grid, the dead air, the words - and a voice. Each runs
as a job (returns `job` at once) and is waited on with `analysis_wait`, the
same loop as `export_wait`, which returns the status; a finished analysis
carries its JSON result in the status's `output`:

- `analyze_scenes` - PySceneDetect's shot changes of a video, `{start,
  end}` seconds. Optional threshold/detector/minSceneLen.
- `analyze_beats` - librosa's bpm, beat grid and onsets of a piece of
  music, in milliseconds (`beatsMs`, `downbeatsMs`, `onsetsMs`).
- `analyze_silence` - the dead air of a recording, `spans` and their
  complement `speech` in seconds, the shape `edit.apply`'s `removeSpans`
  cuts by. Optional thresholdDb (35 default) and minSilenceMs (500).
- `transcribe` - whisper.cpp's words with their times, as caption segments;
  the model comes from Settings > Transcriber (`base.en` default, `small`
  for other languages).
- `tts_generate` - a voiceover spoken to a WAV in an open project's own
  audio folder; `output` is its path and length, ready for `media_import`.
  Voices are the same ids the Settings > Speech page shows (Kokoro 0-52);
  the model comes from Settings > Speech.
- `analysis_wait` - waits for any job (analysis, speech or export) up to
  `timeoutSeconds` (60 default, 600 at most) and returns its status.

The first three need a Python 3 with the extras named in the analysis
folder's `requirements.txt` (scenedetect, OpenCV headless, librosa) - a
one-time `pip install -r <app data>/analysis/requirements.txt`. When
something is missing the reply says the exact line to run instead of
failing silently; `analyze_silence` prefers an `ffmpeg` binary on PATH and
falls back to librosa when there is none.

## Set it up

Point your client at the Concat binary with the one argument `--mcp`.

| System | Command |
| --- | --- |
| Linux (.deb, .rpm, Arch) | `/usr/bin/concat` |
| Linux (AppImage) | the `.AppImage` file itself |

**Claude Code:**

```sh
claude mcp add concat -- /usr/bin/concat --mcp
```

**Claude Desktop** (Settings > Developer > Edit Config, `claude_desktop_config.json`), and most other clients, which take the same shape:

```json
{
  "mcpServers": {
    "concat": {
      "command": "/usr/bin/concat",
      "args": ["--mcp"]
    }
  }
}
```

## How it reaches Concat

`concat --mcp` works one of two ways, and the `concat_version` tool says which (`mcp.mode`):

- **Through the window** (`window`). When Concat is open with **Settings > Automation > Scripts and apps** on, the window writes where its server is, and its token, to `automation.json` in Concat's config folder (readable only by you). The MCP server finds it and goes through the window's server. This is the safer way while you are editing: the agent cannot open the project you have on screen and save over it, and exports take turns with yours.
- **On its own** (`headless`). Otherwise the MCP server runs Concat's engine itself, with no window. Nothing has to be open. Every Concat process locks the project folders it has open (`concat.lock`, let go on close or when the process dies), so a project open in the window is refused here with a note to turn on Automation and use `--mcp window`, and the window refuses a project an agent has open.

Choose explicitly with `--mcp window` (fail rather than run on its own) or `--mcp headless`; the default is `--mcp auto`. To reach a server elsewhere, set `CONCAT_API_ADDRESS` (e.g. `127.0.0.1:7420`) and `CONCAT_API_TOKEN` in the client's `env` for the server.

Either way the agent writes files - projects, exports, frames - only under your home folder. Set `CONCAT_MCP_ROOTS` (a path list, `:`-separated) to allow other folders in headless mode.

The agent edits projects of its own; it does not edit the project open in the window while you watch. That ("live mode") is planned in [#337](https://github.com/jub0t/Concat/issues/337).

## What the agent can do

Tools are named for what they do, and their argument schemas are generated from the Concat API's own types, so they carry the same limits and descriptions the engine does.

| Area | Tools |
| --- | --- |
| Projects | `project_list`, `project_create`, `project_open`, `project_save`, `project_close`, `project_set_video` |
| Reading | `timeline_read` (compact: lanes, clips in time order, media, ids), `project_get` (everything), `concat_version` |
| Media | `media_probe`, `media_import` |
| Stock clips | `openvid_status`, `openvid_prepare`, `openvid_search`, `openvid_get`, `openvid_fetch`, `openvid_import` |
| Sound effects | `sounds_search`, `sounds_add` |
| Music | `bgm_status`, `bgm_prepare`, `bgm_search`, `bgm_fetch`, `bgm_import` |
| Stickers | `magick_status`, `magick_prepare`, `magick_search`, `magick_fetch`, `magick_add` |
| Editing | `clip_add`, `clips_add_sequence`, `clip_trim`, `clips_split`, `clip_cut_spans`, `clips_move`, `clips_remove`, `clip_update`, `clip_transform`, `clip_speed`, `clip_frame`, `track_add` |
| Picture | `clip_mask`, `clip_stabilize`, `timeline_background` |
| Titles and captions | `title_add`, `captions_add` |
| Keyframes and effects | `keyframe_set`, `keyframe_clear`, `effects_list`, `effect_layer_add`, `effect_key_set` |
| Analysis | `analyze_scenes`, `analyze_beats`, `analyze_silence`, `analysis_wait` (results as JSON in the status's `output`) |
| Speech | `transcribe`, `tts_generate` |
| Anything else | `edit_apply` (any edit command, or a batch of them as one undo step), `edit_undo`, `edit_redo` |
| Templates | `template_list`, `template_instantiate` |
| Export | `export_start`, `export_status`, `export_wait` (with progress), `export_cancel` |
| Looking | `preview_frame` (the composited frame as an image) |

Edits stay in memory until `project_save`, as in the window. Times are seconds; a keyframe's position is a fraction of its clip (0 to 1); sizes and offsets are fractions of the frame.

## Protocol

MCP over stdio, newline-delimited JSON-RPC 2.0. Both protocol eras are served: the `initialize` handshake (2025-11-25, 2025-06-18, 2025-03-26, 2024-11-05) and the per-request `_meta` of 2026-07-28 with `server/discover`. `ping`, `tools/list`, `tools/call`, progress notifications for `export_wait`, and `notifications/cancelled` are supported. Standard output carries only protocol; the server logs to standard error.

For scripts that do not speak MCP, the same API is served as JSON-RPC lines on a socket from Settings > Automation; see [the developer docs](https://concatenate.pages.dev/docs).
