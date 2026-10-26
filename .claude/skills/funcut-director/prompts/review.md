# Review: the gate before export

Nothing exports until this passes AFTER THE LAST EDIT. Work the checklist in
order; fix; re-check the touched items; then export.

## 1. Structure (no tools needed)

- [ ] Runtime within ±10 % of the brief; plan segments all executed.
- [ ] Cuts/min matches the niche playbook; an interrupt every 2–4 s in
  short-form.
- [ ] Hook is inside the first 3 s; the payoff loops the hook's image or word.

## 2. Timeline integrity (timeline_read)

- [ ] No unintended gaps: every `start` equals the previous `end`.
- [ ] No overlaps on the same track; the BGM/SFX tracks sit where planned.
- [ ] Every segment's visual exists (no placeholder, no missing media).
- [ ] VO total length matches the tightened transcript (silence pass done).

## 3. Look (preview_frame — start / middle / end, ≤5 calls/minute budget)

- [ ] First frame: hook visual + title present, grade visible, not black.
- [ ] Middle frame: B-roll + captions composed inside the safe area;
  captions readable against the actual footage (stroke/plate working).
- [ ] Last frame: payoff card or loop frame held; grade consistent.
- [ ] One filter, same intensity everywhere (spot-check two frames).

## 4. Audio (from the plan's numbers)

- [ ] BGM present, ducked −12 dB under every spoken segment, riding up in
  ≥1.5 s gaps.
- [ ] SFX peaks on their cuts (spot-check the hook and one section change).
- [ ] No SFX louder than the VO at its moment (−8 dB hook max).
- [ ] Sections land on downbeats or speech won the conflict (±300 ms).

## 5. Rights and hygiene

- [ ] MAGICK stickers absent from monetized work (openmoji is the safe shelf).
- [ ] No invented ids anywhere in what was placed (all came from tool results).
- [ ] Music from FreePD/MobyGratis (both cleared for this use).

## 6. Export and the definition of done

- [ ] `export_start` with the plan's output/crf/codec (h264 crf 20 default;
  platform aspect honored: 9:16 for shorts/reels/tiktok).
- [ ] `export_wait` until `state: done` — THEN check the status's output path
  is reported. **"Done" means the file was written, not that you feel
  finished.** If the job failed, read the error, fix the cause (not the
  symptom), re-verify the touched checklist items, and export again — max 3
  attempts, then report the failure honestly.
- [ ] Report to the user: the output path, runtime, cuts/min, the look and
  voice used, and anything you could not do (missing library results,
  fallbacks used). An honest gap report is professional; a hidden one is not.

## Failure quick reference

| Symptom | First suspect | Fix |
| --- | --- | --- |
| Black frame in preview | segment without a visual / background none | give it a visual; `timeline_background blur` |
| Captions unreadable | no stroke/plate over busy footage | strokeWidth or background plate; darken offset |
| Audio missing in export | VO clip muted / detached track | re-check clip volume keys; re-attach |
| Export fails fast | unsupported size/codec combo | check width/height ≤ 8192, h264+HDR → hevc |
| "not downloaded" error | model not local | surface the Settings instruction to the user |
| Analysis pip error | python extras missing | surface the pip install hint exactly |
| SFX feels late | peak placed after the cut | re-place the sound so its peak lands on the cut (`start = cut - syncAt`), 1 frame early if unsure |
