# The sticker shelf

The library's **Stickers** page puts one shelf beside the stock clips and
the music: **Fluent Emoji** - Microsoft's fluentui-emoji set, 1,285
rendered 3D transparent PNGs, **MIT licence**, free for commercial and
personal use alike. Every one of them is a picture with its background
already taken away, ready to sit over video.

Nothing ships inside the app and nothing ships inside the repository.
The index arrives the first time someone asks for it (a "Load the sticker
library" button, never at startup), ~135 KB, and each picture is fetched
the first time it is previewed or used - one plain HTTPS GET of its own
PNG, a few hundred KB of rendered 3D - then kept in the app's cache.
**A picture costs its own size in bandwidth and nothing else**: the
repository's other gigabytes never move.

## Where it lives

| | |
| --- | --- |
| The index (this repository's release) | `https://github.com/coreselfmastery/FUNCUT/releases/download/fluent-emoji-index-v1/fluent_index.tsv` |
| A picture | `https://raw.githubusercontent.com/microsoft/fluentui-emoji/main/<path from the index>` |
| A picture, mirrored | `https://cdn.jsdelivr.net/gh/microsoft/fluentui-emoji@main/<path from the index>` |
| The app's cache | `<data>/magick/` - `fluent_index.tsv`, `images/<id>.png`, `favourites.json` |

The URLs are constants in `src/crates/concat-host/src/magick.rs`
(`INDEX_URL`, `ASSET_URL`, `ASSET_MIRROR`). The index download comes from
this repository's release and the bytes are checked against a SHA-256
baked into the build - a frozen set, so a new index is a new release tag
and a new digest. The pictures come from Microsoft's repository, with
jsDelivr's mirror behind it for the places raw.githubusercontent.com does
not reach.

## The index

The index is built by `scripts/fluent-emoji-index.py` from the
repository's own tree: one row an emoji, `unicode` (the codepoints,
hyphen-joined for a sequence), `cldr` (the name a card shows),
`keywords` (the words a search reads beside the name), `group`
(Smileys & Emotion, People & Body, ...), `license` (`MIT` - read from
the row, not assumed of the shelf) and `path` (where the PNG lives in
the repository). The name and the keywords are folded together and
lowercased at parse, so "smile" finds the grinning face by a word the
name alone does not say, and no search allocates per row.

The 3D style is the one the shelf shows; the set carries the same 1,285
emoji in Color, Flat and High Contrast too, and a future index could
offer them the same way this one offers 3D.

## The search

One engine, in `Library::search`. The query's words are lowercased, cut
at anything that is not a letter or a digit, and the glue that only
holds a sentence together ("a", "an", "the", "for", "with", "and",
"or", "of", "to", "that", "on", "in") is dropped. The words are ANDed -
every word must land somewhere in the folded name-and-keywords, as a
whole word (+20) or as the front of one (+12, so "partying" finds
"party").

Where a word landed is only half of what it is worth; the other half is
how rare it is. Before the scoring pass, one pass counts how many rows
each word lands in, and each word's points are scaled by
`ln((N+1)/(df+1))` turned into half again to half as much again: a word
every row holds is still worth half its points, a word two rows share
about half again. The whole phrase said as one run of words counts
+100, and a row pays a fifth of a point a character for its sprawl, so
a terse name that says the thing outranks a sprawling one that says it
among other things.

The same engine answers the window's page and the API's `magick.search`
- one function, so a person and an agent are answered by the same list.

## What happens on use

A click on a tile previews the picture through the source preview,
fetching it first if need be; a tile's plus lays it on the timeline at
the playhead; a tile's heart keeps the picture on the favourites list
(`favourites.json`, newest first), and the sidebar's Favourites row
narrows the search to that list.

Placement is the titles' walk, not the bin's: a sticker is cut onto the
first free lane *above* the highest one occupied over its span
(`AddClipAtFirstFree { above: true }`, the flag the titles'
lane-picking already used), so an overlay never hides beneath the video.
The picture is first copied into the project's own `magick/` folder as
`{name}-{id}.png` and imported into the bin - once; a second use finds
the copy already there - so the project owns its files and a cleared
cache breaks nothing.

Every picture's bytes are checked before they are kept: the PNG
signature, a header whose width and height are real, and either colour
type 6 (truecolour with an alpha channel, what the 3D renders are) or
colour type 3 with a `tRNS` chunk (a palette PNG that says which of its
colours are see-through). Anything else - a JPEG wearing the name, an
opaque palette, a truncated file - is deleted, not kept.

## The agent's tools

Five MCP tools, thin over the API, documented in `docs/MCP.md`:
`magick_status`, `magick_prepare`, `magick_search`, `magick_fetch` and
`magick_add` - the last with optional `duration`, `track`, `scale`,
`offsetX`/`offsetY` and `rotation`, landing as one undoable step beside
the clip's own. The names predate the shelf they serve - they stayed
when MAGICK and OpenMoji left, so a script that knew them still works;
`source` accepts the old shelf names and points them at this one.

## Honest words

* **The licence is MIT**: free for commercial and personal use, no
  attribution required. Every result carries its `license` field, and
  the tile's corner shows it, so the answer to "can this ship" is on
  the page rather than in a document.
* The emoji are Microsoft's renders of the Unicode set: real in the
  sense that a font is real, but a picture of a person is still an
  emoji of a person - for real footage, the stock library or your own
  files.
* Every picture is square at 1024x1024; orientation is the transform's
  business (`scale`, `offsetX`/`offsetY`), not the library's.
* The first "Load the sticker library" downloads ~135 KB of index;
  after that, nothing but the pictures you actually pick.

## The shelves that were

Two shelves served here before this one, and both were removed rather
than kept with a warning: **MAGICK** (OneOverZero's CVPR 2024 dataset,
145,193 AI-generated cutouts) was **CC BY-NC 4.0** - unusable in a
monetized video - and **OpenMoji** (the HfG Schwäbisch Gmünd colour
set, 4,565 emoji) was **CC-BY-SA 4.0**, whose share-alike term reaches
into whatever it is layered on. A sticker library that has to be
explained before it can be used is a liability wearing a search box;
the swap to a permissively-licensed set was the fix.
