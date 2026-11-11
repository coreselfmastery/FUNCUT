# Known issues and verified limitations

This file records limitations that remain on `main`. It is not a list of historical bugs that have already been fixed.

## Remaining limitations

1. **No Lottie/dotLottie playback on `main` yet.** `EngineKind::Lottie` remains reserved. PR #91 is open with rasterlottie-backed `NodeKind::Lottie` playback, bounded v1/v2 dotLottie loading, image resolution and support diagnostics; its full CI/Geneva/Nix matrix must pass before merge. The Bodymovin adapter's conversion alone is not playback.

2. **Geneva sidecar is a pinned binary (v1.6.0, closed source).** Its limitations are documented in `docs/MOTION.md`: no path drawing, no variable-font axes, no in-keyframe weights, keyframes in px/% only, and markup animations static per bake. Keep the documented guards and warnings aligned with the actual sidecar behavior.

3. **Linux hardware acceleration is incomplete, not wholly absent.** VAAPI decode is implemented in `concat-media::hardware` and can be selected explicitly, but Linux has no automatic platform default (`HwDevice::platform_default()` returns `None`). The normal export encoder list does not include VAAPI encoders, so native VAAPI hardware encoding is still missing. Device availability, driver compatibility, fallback behavior and performance need testing on real Intel/AMD Linux hardware before enabling it by default.

4. **Timeline view-state fix is merged; final test and merge-commit CI confirmation remains.** PR #88 (`56bde7c85e9fedc817092e2fdad59a3c19aeb09f`) now caches/restores zoom, horizontal and vertical scroll, playhead, selection, tool, snap and lane sizing per timeline. Regression tests cover switching, restoration and removal. PR-head Format/locales/models, real-fixture analysis, Clippy, build and workspace tests passed; both Geneva jobs passed. Documentation examples and gRPC Clippy/tests were still running at the last poll. Keep this item open until those finish and fresh exact-merge-commit CI is green.

5. **Export remains single-slot and targets the active timeline.** There is no export queue or explicit timeline picker. The output is now read back and verified after export, but that verification does not add queueing or timeline selection.

6. **Motion-clip workflow is not complete on `main`.** PR #89 adds a compact project command for a five-second native Motion clip with comp/hash validation and automatic lane placement; it is under CI/Geneva review and is not merged. A visual comp picker, metadata editing, save/reopen, preview and final-export acceptance remain missing.

7. **1080p proxy performance still needs target-hardware measurement and a user-default decision.** Proxy scrubbing defaults on, but the codec defaults to H.264; MJPEG is available as an optional codec and trades substantially larger files for easier random-frame decoding. If the product goal is maximum scrub responsiveness, decide whether MJPEG should be the default or remain an explicit disk-space trade-off. CI cannot establish smooth scrubbing on the user's antiX laptop. Verify proxy generation, proxy selection while scrubbing, original-frame restoration after pause, memory use, and sustained playback on the actual machine. Do not publish an 8–12x speedup claim without repeatable before/after measurements.

8. **The complete AI Director workflow needs end-to-end acceptance testing.** Unit tests and individual tools do not prove that a real project can reliably go from brief/script through footage selection, voice, motion, music, and final export without manual recovery. Track failures and verify a representative real project.

## Previously reported issues now fixed on main

- **Native `CompEngine`: implemented.** PR #79 merged the in-process tiny-skia compositor, including paths, shapes, text, still images, nested comps, masks, mattes, and supported effects. Lottie and shader packages remain outside its supported render set.
- **E303 timeline collision: fixed with regression coverage.** PR #78 handles sub-millisecond key collisions.
- **Export output verification: implemented.** PR #78 reads back the exported file and checks that it opens, contains video, and is close to the requested timeline duration.

When a limitation is fixed, remove it from the remaining list only after the implementation and its regression evidence are present on `main`.
