# The background-music library

The library's **Music** page (#71) puts two shelves of whole CC0 tracks
beside the stock clips: **FreePD** - the Internet Archive's mirror of
Kevin MacLeod's CC0 library, ~1,000 tracks grouped the way freepd.com
grouped them (upbeat, epic, romantic, comedy, electronic, horror,
scoring, world, misc, page2) - and **Lo-Fi** - Moby's MobyGratis
instrumentals, republished as the
[OpenLo-Fi](https://github.com/btahir/open-lofi) pack (166 tracks, 10
categories).

Nothing ships inside the app and nothing ships inside the repository. A
shelf's catalog arrives the first time someone asks for it (a "Load the
music library" button, never at startup), a few MB, and each track is
fetched the first time it is played or used, then kept in the app's
cache. **A track costs its own size in bandwidth and nothing else** - and
the Lo-Fi pack's 554 MB zip is never downloaded at all; read on.

## Where it lives

| | |
| --- | --- |
| Catalogs | `https://archive.org/metadata/freepd` and `https://raw.githubusercontent.com/btahir/open-lofi/v1.0.0/catalog.json` |
| FreePD tracks | `https://archive.org/download/freepd/<file>`, percent-encoded per segment |
| Lo-Fi tracks | one ranged read out of `https://github.com/btahir/open-lofi/releases/download/v1.0.0/openlofi.zip` |
| The app's cache | `<data>/bgm/` - `freepd.json`, `lofi.json`, `lofi-zip.json`, `tracks/<id>.mp3` |

The URLs are constants in `src/crates/concat-host/src/bgm.rs`
(`FREEPD_METADATA`, `FREEPD_DOWNLOAD`, `LOFI_CATALOG`, `LOFI_ZIP`). The
Lo-Fi URLs are pinned to the release this build reads - the catalog and
the zip cannot drift apart, because the byte ranges the zip index holds
are only honest for the exact zip they were measured from. A kept index
is believed only while the live zip still says the same total size; a
zip that disagrees is read again.

## The Lo-Fi ranged-read trick

The pack's whole audio sits in one 554 MB zip on a GitHub release, and
that zip honours byte ranges (GitHub's release assets are served from
storage that answers `Range` with `206`). So the library reads:

1. **The zip's own directory**, once (~80 KB of ranged reads): the
   end-of-central-directory record out of the last 64 KB, then the
   central directory itself - every member's name, method, sizes, CRC
   and offset.
2. **The census**, once per member (one ~4 KB ranged read each, five at
   a time): each member's local file header, for where its data really
   starts, and each track's own first MPEG frames, for its playing time
   - a Xing header's frame count exactly, a plain frame's bitrate over
   the track's own size as the estimate a CBR track deserves.
3. **A track**, whenever it is picked: one ranged read of exactly that
   track's span - inflated when the zip deflated it (the members are
   deflate-compressed), and checked, size and CRC-32 both, against what
   the zip's own directory says they are.

The other 550 MB never moves. A kept census is kept for good
(`<data>/bgm/lofi-zip.json`), so the second track is one ranged read,
and so is the hundredth.

## The page

The page owns what is the window's about the library and not the
library's own: the current results and where the next page starts, the
per-card fetches, and the one-time catalog load's progress. The search
itself, the catalogs and the ranged-read trick are
`concat_host::bgm`'s.

- A search box, a shelf filter (All / FreePD / Lo-Fi), a category filter
  (both shelves' own groupings, title-cased), and an order (Relevance,
  Shortest, Longest, Surprise me). Words are ANDed and prefix-matched
  over a track's title, artist and category.
- A click previews a track - fetched first if it is not on this machine
  yet, then played through the source preview, as a sound in the bin
  plays. A plus lays it at the playhead on the first lane with room;
  both fetch first.
- A track put on the timeline is copied into that project's own `bgm`
  folder and imported into the bin first, as a sound or a stock clip is,
  so the project owns its files and the cache can be emptied whenever.

## For agents

The same library is served through the same API the window uses, so an
agent - Claude, or anything else that speaks MCP (`concat --mcp`, see
[MCP.md](MCP.md)) or JSON-RPC - can pick the music for an edit:

| Method | Tool | What it does |
| --- | --- | --- |
| `bgm.status` | `bgm_status` | Both shelves' state, and how many tracks each holds |
| `bgm.prepare` | `bgm_prepare` | The one-time catalog fetches, started |
| `bgm.search` | `bgm_search` | Words, shelf, category, length, order; pages of results |
| `bgm.fetch` | `bgm_fetch` | One track's bytes into the cache, `localPath` back |
| `bgm.import` | `bgm_import` | Tracks copied into a project's bin, and laid on a lane with `start` |

Nothing is added to any project by the first four; `bgm.import` is what
puts music in an edit, as one undoable step. A track already on this
machine is returned as it stands - tracks are immutable, so there is
nothing to refresh.

## Licences

Everything on both shelves is **CC0 1.0** (public domain). FreePD's
artists are named on each card (Kevin MacLeod, Frank Nora, Alexander
Nakarada, Rafael Krux, Bryan Teoh, Komiku, Monplaisir, and others) - CC0
owes no attribution, but it is kind to keep one, and the Lo-Fi pack's
tracks are credited to Moby (MobyGratis) in the same place. See
`THIRD_PARTY_NOTICES.md` for the full notice.
