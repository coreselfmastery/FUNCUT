# The Concat sound library

> **FUNCUT:** this fork ships the library itself - 430 CC0 sounds in
> [`audio/`](../audio) with the manifest beside them, mirrored onto this
> repository's `audio-v1` release - and the URLs the app reads point here
> (`raw.githubusercontent.com/coreselfmastery/FUNCUT/main/audio/…`) rather
> than at the upstream assets repository, which has nothing published yet.
> The manifest carries three fields this page's schema does not name:
> `tags`, a list of words the shelf's search and the API's `sounds.search`
> narrow by, beyond the name; `description`, a sentence saying what the
> sound is, and `useFor`, short lines saying what it is for, both searched
> after the name and the tags and both handed back to an agent by
> `sounds.search`; and `syncAt`, seconds into the file where the sound's
> peak lands, which the API hands back so a sound can be placed by the
> moment it is for. Everything else below is the schema as the app reads
> it. The making of this library is in
> [FUNCUT-CHANGES.md](FUNCUT-CHANGES.md), sections 5 and 7.

The Library's **Audio** page has a **Concat** shelf of built-in sounds and
music (#60). Nothing ships inside the app: the shelf reads a manifest from
the [`concatapp/assets`](https://github.com/concatapp/assets) repository the
first time someone asks for it (a "Load Concat sounds" button, never at
startup), and each sound is downloaded the first time it is played or used,
then checked against its SHA-256 and kept in the app's cache.

This page is what the asset curator produces for that to work.

## Layout in `concatapp/assets`

```text
audio/
  manifest.json
  whoosh-air-short.ogg
  impact-low-thud.ogg
  ...
```

The files sit flat in `audio/`. The app fetches the manifest from:

```text
https://raw.githubusercontent.com/concatapp/assets/main/audio/manifest.json
```

and each sound from, in order:

1. the release mirror: `https://github.com/concatapp/assets/releases/download/audio-v1/<file>`
2. the repository itself: `https://raw.githubusercontent.com/concatapp/assets/main/audio/<file>`

Upload every file to the `audio-v1` release as well. Then a file that moves in
the repository can't break the shelf. Whichever source the bytes come from,
they are checked against the manifest's `sha256`. The URLs are constants in
`src/crates/concat-host/src/models.rs`: `SOUNDS_MANIFEST`, `SOUNDS_REPO` and
`SOUNDS_RELEASE`.

Until `audio/manifest.json` exists (the request answers 404), the shelf says
"Sound library coming soon".

## `manifest.json`

```json
{
  "version": 1,
  "sounds": [
    {
      "id": "whoosh-air-short",
      "name": "Air whoosh, short",
      "category": "Whoosh",
      "file": "whoosh-air-short.ogg",
      "duration": 0.84,
      "bytes": 18234,
      "license": "CC0-1.0",
      "source": "https://freesound.org/s/123456/",
      "author": "someone",
      "tags": ["fast", "air", "transition"],
      "description": "A fast, crisp whip of air.",
      "useFor": ["Whip pans", "Fast cuts between scenes"],
      "syncAt": 0.092,
      "sha256": "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08"
    }
  ]
}
```

| Field | Required | What it is |
|---|---|---|
| `version` | yes | The manifest format: `1`. |
| `sounds[].id` | yes | Stable for ever, unique, `[a-z0-9-]` only. Projects don't store it, but the app's cache does. |
| `name` | yes | What the row shows. English. Keep it short and descriptive. |
| `category` | yes | One of `Whoosh`, `Impact`, `Riser`, `Motion`, `UI`, `Notification`, `Keyboard`, `Digital`, `Foley`, `Writing`, `Ambience`, `Music`. These are translated in the app. Any other value shows only under "All". |
| `file` | yes | The file name in `audio/`: a plain name, no folders. `.ogg` (Vorbis/Opus), `.mp3`, `.m4a`, `.flac` or `.wav`. |
| `duration` | yes | Seconds, as a number. The row shows it before the file is downloaded. |
| `bytes` | recommended | The file's size. It drives the progress bar. |
| `license` | yes | `CC0-1.0`. Only CC0 is accepted. An entry with any other licence is left off the shelf. |
| `source` | yes | Where the file was found (Freesound, Kenney, OpenGameArt…): the page a reviewer can check the licence on. |
| `author` | no | Who made it, when known. CC0 owes no attribution, but it is kind to keep. |
| `tags` | no | Words the search narrows by, beyond the name. |
| `description` | no | What the sound is, in a sentence. The search reads it after the name and the tags, and `sounds.search` hands it back so an agent can pick without hearing. |
| `useFor` | no | What the sound is for, as short lines. Searched like `description`, and handed back by `sounds.search`. |
| `syncAt` | no | Seconds into the file where the sound's peak lands, when the source says: where a move should meet it. Handed back by `sounds.search`, so a caller can place the file by the moment it is for rather than by its start. |
| `sha256` | yes | The SHA-256 of the file's bytes, lowercase hex, 64 characters. An entry without a valid digest is left off the shelf, and a download that doesn't match it is deleted. |

Compute a digest with `sha256sum <file>`.

## Rules for the selection

- **CC0 only**, so the sounds can be redistributed as a collection and used by
  anyone without owing attribution. Pixabay's and Mixkit's licences forbid
  passing their files on as a collection, so nothing from either.
- Good sources: Freesound (filtered to CC0), Kenney and OpenGameArt CC0 packs.
- Keep files small. Effects should be a few seconds long. Music beds should
  be compressed (Ogg Vorbis around q5, or AAC around 160 kb/s) and ideally
  under a few megabytes each.
- Trim silence at the head, so a sound placed at the playhead starts there.
  Normalise peaks to about −1 dBFS.
- No recognisable voices, no trademarks, nothing that imitates a known
  character or product sound.

## How the app treats it

- The manifest is cached at `<app data>/sounds/manifest.json`. Later sessions
  show the shelf from that cache without going online. The shelf's Refresh
  fetches it again.
- A sound is cached at `<app data>/sounds/<first 12 characters of sha256>-<file>`.
  It stays there until the cache is cleared. When a manifest changes a file's
  bytes, the new digest gets a new file name.
- Placing a sound copies it into the project's own `audio/` folder and
  imports it into the bin, the same as the user's own sounds. Projects
  never point into the cache.
