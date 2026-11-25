# FUNCUT in a sandbox

FUNCUT is a Linux video editor with a headless MCP server. A sandbox does
not need a desktop, a GPU, or a build toolchain to install a release and
make an edit: the release bundles carry FFmpeg, and the MCP server runs
without a window.

This page is the shortest complete path. The tool catalogue, safety
switches and the AUTO EDIT contract are in [MCP.md](MCP.md).

## 1. Install the release

The release AppImage is self-contained. The installer puts it in
`~/.local/bin`, checks its digest, and installs the core models the first
run needs.

```sh
git clone https://github.com/coreselfmastery/FUNCUT.git
cd FUNCUT
./install-linux.sh
export PATH="$HOME/.local/bin:$PATH"
concat --version
```

No root is needed. `--no-models` skips the model download, but
transcription, captions and AUTO EDIT need the core models, so leave it
out unless you already have them. `--with-openvid` adds the 435 MB
research index; it is not rights-cleared stock and AUTO EDIT never uses
it.

Check the binary before pointing an agent at it:

```sh
concat --version
concat --smoke
```

## 2. Start the headless server

```sh
export CONCAT_MCP_ALLOW=all
export CONCAT_MCP_ROOTS=/workspace/video
concat --mcp headless
```

`CONCAT_MCP_ALLOW=all` serves the whole 116-tool catalogue. Without it the
server serves the 106-tool director surface, which includes `auto_edit`.
`CONCAT_MCP_ROOTS` is the only tree the server may write to; point it at
the sandbox's work folder, not at the repository.

An MCP client configuration looks like this:

```json
{
  "mcpServers": {
    "funcut": {
      "command": "/home/sandbox/.local/bin/concat",
      "args": ["--mcp", "headless"],
      "env": {
        "CONCAT_MCP_ALLOW": "all",
        "CONCAT_MCP_ROOTS": "/workspace/video"
      }
    }
  }
}
```

The first call should be `concat_version`. It returns the mode
(`headless`), the version and the roots the server will write under.

## 3. Make an edit

A minimal edit is `project_create` -> `media_import` -> `edit_apply`.
Every edit is one undoable step; use an atomic `batch` for several.

When the timeline already has narration audio, the one-call edit is:

```json
{
  "name": "auto_edit",
  "arguments": {
    "path": "/workspace/video/MyReel",
    "copy": true,
    "stockProvider": "pexels"
  }
}
```

`auto_edit` copies the project to `MyReel edit`, repairs and transcribes
the voice, reads the delivery, cuts dead air, grades, adds captions,
shops licensed B-roll, adds music and accents, previews, revises, saves,
verifies and watches. It returns the edited project path, the terminal
state, `auto-edit-report.json` and, on success, `auto-edit-receipt.json`.
It does not export; pass the returned `project` to `export_start`.

Optional fields:

- `script` - authoritative wording. The transcript is aligned to it and
  the measured timings are kept.
- `stockProvider` - `pexels` (default) or `pixabay`. Without an API key
  the run fills the picture from the project's own footage and
  typography.
- `copy` - `true` edits the sibling; `false` edits `path` in place.

The call is budgeted at 1200 seconds because transcription, stock fetches,
motion bakes and the watch render are slow.

## 4. Export

```json
{
  "name": "export_start",
  "arguments": {
    "path": "/workspace/video/MyReel edit",
    "out": "/workspace/video/MyReel.mp4"
  }
}
```

Then poll `export_status` or call `export_wait`. The exporter writes H.264
by default; HEVC and AV1 are options on the same call.

## 5. What a sandbox needs

- Linux x86_64 or aarch64.
- About 2 GB for the app plus the core models; more if `--all-models`.
- No GPU. The editor uses a software renderer when no Vulkan device is
  present; install Mesa's `lavapipe` (`mesa-vulkan-drivers` on Debian) if
  preview renders need a Vulkan device.
- Network for the first model download and for live stock shelves.
- A writable `CONCAT_MCP_ROOTS` folder.

If you build from source instead of installing a release, run
`scripts/setup-build-env.sh` first and then
`cargo build --release -p concat`. The binary is
`src/target/release/concat`.

## 6. Troubleshooting

- **`no GPU or software renderer is available`** - install
  `mesa-vulkan-drivers`; the headless edit still runs, but preview sheets
  need a renderer.
- **`fontconfig` not found while building** - install the development
  package (`libfontconfig1-dev` on Debian).
- **`the project folder ... does not exist`** - `auto_edit` takes a
  project folder, not a `concat.json` file.
- **`no narration audio`** - `auto_edit` edits narration. Add an audio
  clip outside the project's `bgm/` folder; a talking-head video is the
  GUI AUTO EDIT path.
- **A stock search is refused** - save a Pexels or Pixabay key, or let the
  run use the project's own footage. AUTO EDIT never silently uses
  unlicensed stock.
- **The server refuses a path** - it writes only under
  `CONCAT_MCP_ROOTS`; move the project there or start the server in that
  directory.
