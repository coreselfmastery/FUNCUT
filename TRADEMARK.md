# Concat trademark policy, as this fork reads it

The Concat **source code** is free software under AGPL-3.0-or-later. The Concat
**name and logo** are not part of that grant.

This is deliberate and it is normal: the AGPL, like every GNU licence, conveys
no trademark rights (see AGPL-3.0 section 7(e)). Mozilla, Docker and the Rust
project all separate the two the same way. The code is yours to fork; the name
identifies builds that come from the Concat project, so that a user downloading
"Concat" knows what they are getting.

## The marks

- The word mark **Concat**, as the name of the video editor this repository
  is a fork of.
- The Concat logo and its variants, in [`assets/`](assets/), which belong to
  upstream's repository.

Copyright and marks are held by Jareer Abdullah (GitHub: [@jub0t](https://github.com/jub0t)).
**This repository holds no claim to them.**

## Where this fork stands

FUNCUT is a modified Concat — different defaults, different cache sizing,
libraries and tooling added — which is exactly the case upstream's policy
puts under "needs a different name", and this repository honours it as
follows:

- The repository, its releases' fork tags (`v0.2.8-funcut.5`) and the
  launcher the installer adds beside the binary are named **FUNCUT**;
  bug reports about this fork's tuning belong in
  [this repository's issues](https://github.com/coreselfmastery/FUNCUT/issues),
  not upstream's.
- The binary, the desktop entry and the project format keep the `concat`
  name and icon unchanged, so projects and muscle memory move between
  this fork and upstream without friction, and this README says plainly
  what is whose: nominative use, which upstream's policy allows without
  asking. Where this README shows upstream's stars, history or
  contributors, they are upstream's, shown as attribution.

If upstream asks this fork to carry its own name end to end, that is
upstream's right under the policy above, and the ask will be taken.

## What you may do without asking

- **Say what your software is.** Nominative use is always fine: "a plugin for
  Concat", "compatible with Concat", "a fork of Concat", "imports Concat
  projects". Describe the relationship accurately and you need no permission.
- **Redistribute official builds** under the name Concat, unmodified.
- **Package Concat for a distribution** (Nix, Homebrew, AUR, Debian, Flatpak…)
  from unmodified upstream source, keeping the name. Patches confined to
  packaging, paths and build flags are fine.
- **Write about Concat.** Reviews, tutorials, videos, screenshots, comparisons,
  academic work. No permission needed, no approval sought.
- **Use the logo** to link to or refer to the Concat project.

## What is not allowed

- Presenting a modified build as official Concat.
- Using **Concat** or a confusingly similar name in your product name, company
  name, app-store listing, or domain in a way that implies the Concat project
  endorses or maintains it.
- Using the logo as your own product's icon or branding.
- Registering the Concat mark, or a confusingly similar one, in any
  jurisdiction.

## Asking

Anything not covered above, or a use you want blessed explicitly, is
upstream's to answer: **jub0trd@gmail.com**. Reasonable requests get a yes.

*This policy may be updated. Changes are not retroactive against uses that were
permitted when made.*
