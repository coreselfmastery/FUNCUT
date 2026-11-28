# Network and update policy

The Linux packages are distributed through GitHub Releases with a
`SHA256SUMS` checksum file; verify a download against it before installing
one by hand (`sha256sum --check SHA256SUMS --ignore-missing`).

## How a release is made

- Every package is built by GitHub Actions from a tagged commit of
  <https://github.com/coreselfmastery/FUNCUT>, a fork of
  <https://github.com/jub0t/Concat> whose tags are re-cuts of the
  upstream 0.2.8 line (`v0.2.8-funcut.15` is the current one), by the
  workflows in [`.github/workflows`](.github/workflows) (`release.yml`,
  which calls `build-app.yml`).
- A release is built only after the tests pass on the tagged commit, and each
  bundle is launched before it is uploaded (see
  [`scripts/ci`](scripts/ci)).
- The workflows and build scripts are part of the source and are reviewed like
  any other change.

## The data releases and their digests

The app's own downloads are mirrored on this repository's data releases
rather than fetched from whoever published them: `models-v1` (the AI
models), `audio-v1` (the bundled CC0 sounds), `openvid-index-v1` (the
stock index) and `fluent-emoji-index-v1` (the sticker index). These
carry no `SHA256SUMS` beside them; their digests are pinned where the
app reads them - the models' in `models/manifest.toml`, the sounds' in
`audio/manifest.json`, and the two indexes' in the code constants beside
the URLs that fetch them (`openvid.rs` and `magick.rs`, each an
`INDEX_SHA256`). Every download is checked against its digest before it
is installed, and a mismatch is deleted, never used.

## Team and roles

| Role | Members |
|---|---|
| Committers and reviewers | the FUNCUT fork's maintainer |
| Approvers | the FUNCUT fork's maintainer |

Committers may change the source without further review. Every change from
anyone else, such as a pull request, is reviewed by a reviewer before it is
merged. Every member uses multi-factor authentication on GitHub.

## Privacy policy

This program will not transfer any information to other networked systems
unless specifically requested by the user or the person installing or operating
it. Concat reaches the network only when you ask it to:

- **Settings › Version** asks GitHub for the list of releases when you open
  it or press **Check for updates**, and installing one downloads it from
  GitHub. The three-day automatic check is opt-in (off by default): set
  `"checkUpdatesAutomatically": true` in the app's `settings.json` and the
  background clock runs - a fresh install never asks about releases on its
  own.
- **Downloading a model** (transcription, speech, cutout) fetches it from the
  source picked under **Downloads** in Settings › General, and tries the other
  sources listed there if that one fails.
- **The Remote API** is off by default. Turned on, it listens on this computer
  (`127.0.0.1`) unless you give it another address, and answers only callers
  that hold its token.

Nothing is sent about you, your projects or your media, and there is no
telemetry.
