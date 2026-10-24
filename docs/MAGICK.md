# The sticker library

The library's **Stickers** page puts two shelves of transparent PNGs
beside the stock clips and the music: **MAGICK** - OneOverZero's MAGICK
dataset (CVPR 2024), 145,193 captioned single-subject 1024x1024 RGBA
images, chroma-keyed out of their backgrounds: people, animals, objects,
gestures - and **OpenMoji** - the OpenMoji project's colour set, 4,565
transparent 618x618 emoji, CC-BY 4.0. 149,758 overlays in all, every one
of them a picture with its background already taken away, ready to sit
over video.

Nothing ships inside the app and nothing ships inside the repository.
The shelves' indexes arrive the first time someone asks for them (a
"Load the sticker library" button, never at startup), ~18 MB in all, and
each picture is fetched the first time it is previewed or used - one
plain HTTPS GET of its own PNG, ~1 MB for a MAGICK picture, a few KB for
an emoji - then kept in the app's cache. **A picture costs its own size
in bandwidth and nothing else**: the dataset's other 200 GB never moves.

## Where it lives

| | |
| --- | --- |
| The MAGICK index (this repository's release) | `https://github.com/coreselfmastery/FUNCUT/releases/download/magick-index-v1/magick_index.tsv` |
| The MAGICK index (the Hub, and hf-mirror.com behind it) | `https://huggingface.co/datasets/OneOverZero/MAGICK/resolve/main/magick_index.tsv` |
| A MAGICK picture | `https://huggingface.co/datasets/OneOverZero/MAGICK/resolve/main/images/<first two of the id>/<id>.png` |
| The OpenMoji index | `https://raw.githubusercontent.com/hfg-gmuend/openmoji/master/data/openmoji.json` |
| An OpenMoji picture | `https://raw.githubusercontent.com/hfg-gmuend/openmoji/master/color/618x618/<hexcode>.png` |
| The app's cache | `<data>/magick/` - `magick_index.tsv`, `openmoji.json`, `images/<id>.png`, `favourites.json` |

The URLs are constants in `src/crates/concat-host/src/magick.rs`
(`INDEX_URL`, `INDEX_URL_HF`, `DATASET_URL`, `OPENMOJI_INDEX`,
`OPENMOJI_PNG`). The MAGICK index download tries this repository's
release first, then the Hub, then hf-mirror.com, and the bytes are
checked against a SHA-256 baked into the build whichever answers - a
frozen dataset, so a new index is a new release tag and a new digest.
OpenMoji is a living upstream rather than a frozen one, so its index is
cached rather than digest-pinned, the way the sound library's manifest
is.

## The index, and what it leaves out

The MAGICK index is the dataset's own TSV: one row a picture, `page_id`,
`sim_score`, `color`, `nsfw`, `subject`, `tag_idk`, `picked`. The parse
reads `page_id` and `subject` and drops two kinds of row outright: the
3,000 the index itself marks not-safe-for-work (a sticker shelf is not
where they belong), and any row without an id or a caption. What remains
is 145,193 pictures, each one's caption lowercased beside the original
so the search never allocates per row.

The OpenMoji index is the project's own JSON: one entry an emoji,
`hexcode`, `annotation`, `tags`. The caption a card shows is the
annotation - "grinning face" - and the words a search reads are the
annotation and the tags together, so "smile" finds the grinning face by
a word the annotation alone does not say.

Both shelves' ids never overlap (MAGICK's are ten mixed-case
alphanumerics, OpenMoji's are upper-case hex), so one id names one
picture without saying its shelf.

## The search

One engine over both shelves, in `Library::search`. The query's words
are lowercased, cut at anything that is not a letter or a digit, and the
glue that only holds a sentence together ("a", "an", "the", "for",
"with", "and", "or", "of", "to", "that", "on", "in") is dropped. The
words are ANDed - every word must land somewhere in a caption, as a
whole word (+20) or as the front of one (+12, so "waving" finds
"waves").

Where a word landed is only half of what it is worth; the other half is
how rare it is. Before the scoring pass, one pass counts how many
captions each word lands in, and each word's points are scaled by
`ln((N+1)/(df+1))` turned into half again to half as much again: a word
every caption holds is still worth half its points, a word two captions
share about half again. The whole phrase said as one run of words in a
caption counts +100, and a caption pays a fifth of a point a character
for its sprawl, so a terse caption that says the thing outranks a
sprawling one that says it among other things.

The same engine answers the window's page and the API's `magick.search`
- one function, so a person and an agent are answered by the same list.

## What happens on use

A click on a tile previews the picture through the source preview,
fetching it first if need be; a tile's plus lays it on the timeline at
the playhead; a tile's heart keeps the picture on the favourites list
(`favourites.json`, ids of either shelf, newest first), and the sidebar's
Favourites row narrows the search to that list.

Placement is the titles' walk, not the bin's: a sticker is cut onto the
first free lane *above* the highest one occupied over its span
(`AddClipAtFirstFree { above: true }`, the flag the titles'
lane-picking already used), so an overlay never hides beneath the video.
The picture is first copied into the project's own `magick/` folder as
`{caption}-{id}.png` and imported into the bin - once; a second use
finds the copy already there - so the project owns its files and a
cleared cache breaks nothing.

Every picture's bytes are checked before they are kept: the PNG
signature, a header whose width and height are real, and either colour
type 6 (truecolour with an alpha channel, what MAGICK serves) or colour
type 3 with a `tRNS` chunk (a palette PNG that says which of its colours
are see-through, what OpenMoji serves). Anything else - a JPEG wearing
the name, an opaque palette, a truncated file - is deleted, not kept.

## The agent's tools

Five MCP tools, thin over the API, documented in `docs/MCP.md`:
`magick_status`, `magick_prepare`, `magick_search`, `magick_fetch` and
`magick_add` - the last with optional `duration`, `track`, `scale`,
`offsetX`/`offsetY` and `rotation`, landing as one undoable step beside
the clip's own.

## Honest words

* **MAGICK's pictures are AI-generated** (chroma-keyed composites of
  generated subjects on green/blue/violet/red/yellow/orange), and the
  captions describe what the generator meant. A person, a car, a real
  landmark: OpenMoji is real, MAGICK is not - for real things, look at
  the stock library or your own files.
* **MAGICK is CC BY-NC 4.0** (the project page's own licence link):
  fine for personal and non-commercial video, *not* for commercial use.
  OpenMoji is CC-BY 4.0, whose one condition is attribution (see
  `THIRD_PARTY_NOTICES.md`). A video you publish commercially should
  not carry MAGICK stickers.
* Every MAGICK picture is 1024x1024 and square; orientation is the
  transform's business (`scale`, `offsetX`/`offsetY`), not the
  library's.
* The first "Load the sticker library" downloads ~18 MB of indexes;
  after that, nothing but the pictures you actually pick.

## Other transparent-PNG datasets considered

* **OpenMoji** - added, as above: living upstream, permissive licence,
  small, real.
* **PurePNG** (purepng.com) - a CC0 transparent-PNG library in the
  MAGICK mould; the site is defunct (it answers 403 and the project has
  not been maintained), so there is nothing stable to fetch. Not added.
* **Twemoji** (Twitter's emoji set, CC-BY 4.0) - subsumed by OpenMoji,
  which is the same idea with tags the search can read; not added
  beside it.
* **LaFleur and the "sticker" cutouts of Open Images** - human-segment
  photo cutouts, which is what MAGICK already is, with no captions to
  search by and a licence (CC-BY 2.0 on the images) that carries the
  photographer's attribution requirement per image; not added.
