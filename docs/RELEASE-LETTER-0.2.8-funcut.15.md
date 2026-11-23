# A letter about Concat 0.2.8-funcut.15

To whoever installs this build,

This is the note that accompanies `v0.2.8-funcut.15`, cut on 23 November
2026. It carries everything `.14` carries - the headless AUTO EDIT tool,
the sandbox install path, the one-Geneva-surface work, the creative
contract, the signal consumers, the stock-redirect hardening and the
`.14` CI fixes - plus the one piece of Geneva work that was still on a
side branch when `.14` was tagged: the native 3D scene layer.

If you are here for the short version: `.15` is `.14` with a real 3D
scene node. The rest of the editor is unchanged.

## What this release is

`v0.2.8-funcut.15` is a re-cut of the 0.2.8 line, the same convention as
every fork tag. The installer, the updater and GitHub's "latest" pointer
read these tags, so `.15` is a full release even though the version
string inside the app still reads `0.2.8`.

## The 3D scene layer

A comp can now carry a real 3D scene, drawn by the shipped native
compositor. The shape is deliberately three.js-like without a JavaScript
runtime: `NodeKind::Scene3D` holds a scene document of `box`, `plane`,
`sphere`, `cylinder` and `torus` meshes, with per-object position,
rotation, scale, spin, colour, metalness, roughness and emissive;
ambient, directional and point lights; and a perspective camera that
reuses the comp's own `Camera3D` flight path (`at`, `x`, `y`, `z`, plus
optional pitch, yaw, roll and fov stops). Colours read as `#rrggbb` /
`#rrggbbaa` strings or as `{r,g,b,a}` objects.

Two authoring ops open it: `add-scene3d` (a named `showcase` or
`product` scene, or a complete scene) and `set-scene3d` (replace the
scene on an existing node). The preset library grew from 32 to 33 with
`funcut.scene3d.showcase`, and the authoring vocabulary from 28 to 30
ops. `dump_presets` writes the new comp, and
`examples/comps/funcut.scene3d.showcase.json` is checked in.

The renderer is a CPU rasteriser with a z-buffer and per-pixel Phong
shading; there is no GPU path and no browser. A triangle crossing the
near plane is clipped in homogeneous space, so a camera inside a mesh
does not fold the frame; point lights work at the origin; specular
highlights use the actual eye, not a fixed +z. Semi-transparent objects
are drawn in document order and do not write depth, which is the one
place order matters. The cost is measured, not guessed: the four-object
showcase renders a 1920x1080 frame in about 0.7 s on this CPU-only box,
so an 8 s / 30 fps scene is roughly 2.8 minutes; `samples: 2` costs
about four times that. Keep a scene to a few dozen objects at 1080p.

Engine routing is explicit. A comp whose only engine-specific feature is
a `scene3d` node resolves to native even when the Geneva sidecar is
installed. A comp that also carries markup keeps Geneva, and the bake
warns that Geneva cannot draw the scene rather than dropping it
silently.

## Everything `.14` carries

`.15` is a superset of `.14`; the `.14` letter is the detailed note for
the rest of the tree. In short:

- **Headless AUTO EDIT.** `auto_edit` runs the window's fixed spine as
  one MCP call, copies the project to a sibling by default, returns the
  stage receipt and leaves an editable timeline. `docs/SANDBOX.md` is
  the install-and-run recipe.
- **One Geneva surface.** A comp keeps the Geneva name and delegates to
  the native rasteriser when it carries paths, mattes, 3D, rigs,
  adjustment layers or a supported effect; every motion reply says the
  route.
- **Remap, cached frames, parallel preview and timeline placement.**
  The programmatic-video primitive, cached `motion_frame` /
  `motion_preview` PNGs, a four-core preview split and
  `motion_bake {place: true}`.
- **The creative contract.** `plan_validate` enforces a style bible and
  a retention contract, and the deterministic AUTO EDIT fallback writes
  both.
- **The signal work.** The fifty-signal vocabulary is parsed, spent and
  logged; every reading has a reader, a consumer and a decision.
- **The AUTO EDIT picture work.** Carried A-roll, the talking-head door,
  the picture floor, the beat grid, honest plans and the live timeline.
- **The security and CI work.** Stock redirect validation, the URL
  RUSTSEC-2026-0592 upgrade, release audits gated on Nix and RustSec,
  and the non-cancelling workflow queue.

## What this release does not prove

- **No target-hardware acceptance.** The AUTO EDIT button, the 3D
  renderer and the director's Verify record are unit- and CI-tested, but
  no real brief-to-export run on the intended low-memory Linux laptop has
  been recorded.
- **The 3D renderer is CPU-only.** It is not a GPU path, and the frame
  cost above is the measured one on this box, not a promise for every
  machine.
- **VAAPI hardware encode is opt-in and unvalidated on real GPUs.**
- **The Geneva sidecar is a pinned closed-source binary (1.6.0).** Its
  limits are documented in `docs/MOTION.md`: no path drawing, no
  variable-font axes, keyframes in px and % only.
- **The BGM library's FreePD shelf depends on archive.org.**

`KNOWN_ISSUES.md` is the authoritative list; read it before you trust a
feature on your own machine.

## Getting it

Every architecture Concat ships on gets a self-contained bundle, and
they are attached to the `v0.2.8-funcut.15` release:

| | x86_64 | arm64 |
|---|---|---|
| Linux | `linux-x86_64.deb`, `.rpm`, `.pkg.tar.zst`, `x86_64.AppImage` | `linux-aarch64.deb`, `.rpm`, `.pkg.tar.zst`, `aarch64.AppImage` |

`SHA256SUMS` lists each file's checksum, and `manifest.json` names the
bundle set. The `.deb` and `.rpm` carry the FFmpeg runtime libraries
beside the binary; the AppImage needs no `libfuse2`.

The per-commit list is generated by `.github/release-notes.sh` from the
range `v0.2.8-funcut.14..v0.2.8-funcut.15`, and the release workflow
attaches it to the GitHub release when the tag builds. This letter is the
prose around that list.

---

This letter was written by an AI agent (OpenHands) on behalf of the
maintainer, from the repository's own commit history, tests and measured
render timings.
