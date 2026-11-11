# Known issues and verified limitations

This file records limitations that remain on `main`. It is not a list of historical bugs that have already been fixed.

## Remaining limitations

1. **No Lottie/dotLottie playback engine or timeline Motion-clip creation command.** `EngineKind::Lottie` is reserved, not implemented. The Bodymovin adapter imports a supported subset into comp nodes, but conversion is not playback; native `NodeKind::Lottie` explicitly draws nothing and warns.

2. **Geneva sidecar is a pinned binary (v1.6.0, closed source).** Its limitations are documented in `MOTION.md`: no path drawing, no variable-font axes, no in-keyframe weights, keyframes in px/% only, and markup animations static per bake. Keep the documented guards and warnings aligned with the actual sidecar behavior.

3. **VAAPI hardware encode/decode is not wired into the application.** On this Linux-only fork, `HwDevice::platform_default()` returns `None` and `VideoCodec::encoders()` does not expose VAAPI. The Python motion-overlay sidecar has a VAAPI encode path, but that does not mean the native application uses it. Hardware-specific support must be implemented and tested on supported devices before being called complete.

4. **Timeline view state is not isolated per timeline.** Zoom, scroll, playhead, and selection state can reset or carry incorrectly when switching timeline tabs. This requires a GUI-level fix and regression coverage.

5. **Export remains single-slot and targets the active timeline.** There is no export queue or explicit timeline picker. The output is now read back and verified after export, but that verification does not add queueing or timeline selection.

6. **Motion comps cannot yet be created as timeline clips through a normal edit command.** The exporter can resolve hand-authored `ClipKind::Motion` documents and motion-agent output, but the editor currently lacks the command that creates those clips in ordinary timeline use.

7. **1080p proxy performance still needs target-hardware measurement.** CI cannot establish smooth scrubbing on the user's antiX laptop. Verify proxy generation, proxy selection while scrubbing, original-frame restoration after pause, memory use, and sustained playback on the actual machine. Do not publish an 8–12x speedup claim without repeatable before/after measurements.

8. **The complete AI Director workflow needs end-to-end acceptance testing.** Unit tests and individual tools do not prove that a real project can reliably go from brief/script through footage selection, voice, motion, music, and final export without manual recovery. Track failures and verify a representative real project.

## Previously reported issues now fixed on main

- **Native `CompEngine`: implemented.** PR #79 merged the in-process tiny-skia compositor, including paths, shapes, text, still images, nested comps, masks, mattes, and supported effects. Lottie and shader packages remain outside its supported render set.
- **E303 timeline collision: fixed with regression coverage.** PR #78 handles sub-millisecond key collisions.
- **Export output verification: implemented.** PR #78 reads back the exported file and checks that it opens, contains video, and is close to the requested timeline duration.

When a limitation is fixed, remove it from the remaining list only after the implementation and its regression evidence are present on `main`.
