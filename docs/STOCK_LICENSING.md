# Stock footage, provenance and licensing

This note exists because FUNCUT's B-roll library is a **research dataset**,
not a rights-cleared commercial stock subscription. An edit can be
technically excellent and still be unusable. Know what you are shipping.

## What OpenVid actually is

`openvid_search` / `openvid_fetch` read a local index of the
[OpenVid-1M](https://huggingface.co/datasets/nkp37/OpenVid-1M) dataset
(937,957 clips in this build) served as a Lance table at
`lance-format/openvid-lance`. OpenVid-1M is a **text-video research
dataset**, assembled from several pre-existing sources. Its own card
describes it as research data; it is not a stock licence and it is not a
warranty of title.

The shelves the index names, and what is known about each:

| `source` | What it is | Practical risk |
| --- | --- | --- |
| `pixabay` | Clips attributed to Pixabay | Pixabay's content licence is generally permissive, but the dataset does not carry per-clip licence metadata or model/property releases. |
| `pexels` | Clips attributed to Pexels | Pexels licence permits commercial use; again, no per-clip releases or attribution record in the index. |
| `celebv` | CelebV-HQ-derived person/face clips | **Identity and likeness rights.** Highest-risk shelf for commercial or political content. Avoid for ads, endorsements or anything implying a real person said or did something. |
| `other` / WebVid | Web-crawled video with scraped alt-text | Unknown provenance. Treat as research-only unless you clear it yourself. |

Additional facts from the code, not guesses:

- The index stores captions, aesthetic/motion/temporal scores, camera
  motion, fps, duration, bytes and a source label. **It stores no licence
  field, no author, no release information and no attribution string.**
- Caption text is the dataset's own description. It is not a rights record.
- The clips are cut for training: Pixabay/Pexels/CelebV shelves are
  typically **512×512 squares**, WebVid is 720p/1080p landscape. There is
  **no native vertical footage** in the library. A 9:16 edit is always a
  crop. `stock_contact_sheet`'s `cropHint` tells you where the subject is;
  use it rather than centre-cropping blindly.
- Search is SQLite FTS5 BM25 over a prefix of the caption, not a visual
  embedding. That is another reason to *look* at the frames before import.

## Rules for shipping an edit

1. **Internal / research / prototype** — OpenVid is fine. Say so in the
   deliverable.
2. **Monetised or client work** — verify each clip. Prefer `source:
   "pixabay"` or `"pexels"` and re-find the original on the source site to
   confirm the current licence and any required attribution. Keep a record
   of the clip `id`, `name`, `caption`, `source` and the date checked.
3. **Never** use `celebv` clips to imply endorsement, identity, or a real
   person's speech or actions.
4. When in doubt, replace the clip with footage you shot, footage from a
   paid library, or a generated B-roll asset whose licence you hold.

`stock_contact_sheet` returns each candidate's `id`, `name`, `caption`,
`source`, `aesthetic` and `localPath`. Save that list with the project: it
is the closest thing this build has to a provenance log. A future version
should add a `licence` column to the index and refuse to import
unlicensed shelves in a commercial export profile; **that is not
implemented here**.

## The other shelves

- `sounds_search` / `bgm_search` serve the repository's own CC0 sound and
  music collections. CC0 means no attribution is required, but check the
  individual asset's metadata if you redistribute the raw files.
- `magick_search` overlays are CC BY-NC in the source catalogue. **NC is
  not commercial use.** The audit's advice stands: for commercial work,
  prefer CC0/CC BY shelves or your own graphics.
- Anything produced by `motion_bake`, `captions_animate` or
  `motion_overlay` is derived from comps in this repository (AGPL-3.0)
  plus whatever assets you supplied. The rendered output is yours; the
  comp documents are not a substitute for clearing the underlying plate.

## Bottom line

FUNCUT can find a clip; it cannot grant you the rights to it. The tool
returns provenance hints so a person can make the call — it does not make
the call for you.
