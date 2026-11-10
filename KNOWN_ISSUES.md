# KNOWN_ISSUES.md — open at last session end

None of these are hidden; each is a recorded limitation with its evidence.

1. **The native `CompEngine` does not exist** (G6). `EngineKind::Native`
   and its fallback plumbing are real and tested, but no `impl
   CompEngine` backs them, so `NodeKind::Path`, `Effect::Shadow` and
   `Effect::Shader` render nowhere today (Geneva drops each with a
   warning; the docs say the native engine draws them). The `lib.rs:19`
   "already shipped" line overstates this — being corrected in the docs
   pass. Implementation of the engine itself is the ledger's stretch unit.

2. **No Lottie player** (part of G6). The adapter *converts* Lottie to
   comps; `NodeKind::Lottie` renders nothing. Known, documented, and the
   geneva-adapter's "plays as an asset" claim is being fixed in the docs
   pass.

3. **Geneva sidecar is a pinned binary** (v1.6.0, closed source). Its
   measured limitations (no path drawing, no variable-font axes, no
   in-keyframe weights, keyframes in px/% only, markup animations static
   per bake) are enumerated in MOTION.md with the guards that catch each
   one. This is the repo's own honest table; nothing new found this
   session contradicts it.

4. **Latent E303 collision** (G1) — unit A in flight; will be closed with
   a regression test before this file next ships.

5. **Export does not verify its output** (G4) — unit F in flight.

6. **VAAPI hardware encode/decode is unreachable in the app** on this
   Linux-only fork (`HwDevice::platform_default()` returns `None`;
   `VideoCodec::encoders()` lists no vaapi). Deliberately NOT implemented
   this session: no GPU is available to validate honestly. The Python
   motion-overlay sidecar carries a working VAAPI encode path as the
   reference implementation (`sense/sense.py:1889-1971`).

7. **No per-timeline view state** (zoom/scroll/playhead reset on tab
   switch; selection cleared). Real UX gap in the multi-timeline surface;
   requires GUI-crate changes this box cannot compile — left to a session
   with a bigger machine or CI-driven iteration.

8. **Export is single-slot** (no queue) and always exports the *active*
   timeline (no picker). Same constraint class as 7.
