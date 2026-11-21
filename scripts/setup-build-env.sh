#!/usr/bin/env bash
# The local build environment, reproduced from what CI itself uses: the
# digest-pinned FFmpeg, ONNX Runtime and sherpa-onnx that scripts/ci/
# deps.lock names, fetched by scripts/ci/fetch.sh (which refuses any file
# whose sha256 is not the one written there), unpacked at the repo root
# under the same names the CI engine job uses - ffmpeg-dev/, onnxruntime-
# linux-<arch>-<version>/ and sherpa/ - and read back by scripts/
# funcut-env.sh. A clean clone can follow the reproduction in
# TEST_EVIDENCE.md: no machine-local paths, no root, nothing outside the
# checkout.
#
#   bash scripts/setup-build-env.sh
#
# Idempotent: a warm checkout downloads nothing (fetch.sh keeps an
# archive whose digest already matches) and an extract that is already
# in place is left alone, so the script can be re-run before every work
# session the way CI's cache is.
#
# The system packages the engine's native crates want (libasound2-dev
# for cpal, libclang-dev for the bindgen crates, cmake for whisper.cpp)
# are a distribution's to give, not this script's: it checks for the
# tools and prints the one apt line the CI engine job runs, so a box
# with root installs them in one paste and a box without knows at once
# which of its checks can never pass.
#
#   bash scripts/setup-build-env.sh --debs
#
# adds the no-root path the environment notes record: the clang 19
# runtime and its builtin headers, downloaded as Debian packages with
# apt-get download (which needs no root and installs nothing) and
# unpacked with dpkg-deb -x under sysroot/ beside the extracts above,
# where scripts/funcut-env.sh finds them (LIBCLANG_PATH and the clang
# resource include). These bytes are the distribution's, pinned by its
# own archive, not by deps.lock - which is why they are a flag and not
# the default. A box that is not Debian or Ubuntu installs its own
# libclang the distribution's way.
#
# The GUI `concat` crate wants more than the engine's set (fontconfig,
# GTK, Vulkan); see the "System libraries" step of the engine job in
# .github/workflows/ci.yml for that list. On a small machine the crate
# is CI's to build, not the local box's - recorded in TEST_EVIDENCE.md.
set -euo pipefail

root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
cd "$root"
fetch="$root/scripts/ci/fetch.sh"
debs=false
if [ "${1:-}" = "--debs" ]; then
  debs=true
elif [ $# -gt 0 ]; then
  echo "usage: $0 [--debs]" >&2
  exit 2
fi

case "$(uname -m)" in
  x86_64)
    ffmpeg_row=ffmpeg-linux64
    ort_row=onnxruntime-linux-x64
    sherpa_row=sherpa-onnx-linux-x64-static-lib
    multiarch=x86_64-linux-gnu
    ;;
  aarch64 | arm64)
    ffmpeg_row=ffmpeg-linuxarm64
    ort_row=onnxruntime-linux-aarch64
    sherpa_row=sherpa-onnx-linux-aarch64-static-lib
    multiarch=aarch64-linux-gnu
    ;;
  *)
    echo "setup-build-env.sh: no deps.lock rows for $(uname -m)" >&2
    exit 1
    ;;
esac

# The version a deps.lock URL carries, so no second copy of it lives
# here: the row's own filename names the extract. deps.lock is the one
# source of truth - bump it there and this script follows. A SourceForge
# mirror URL (the ONNX rows since the 1.30.1 re-pin) ends in /download -
# the archive name is the segment before that suffix, so it is stripped
# before basename runs.
version_from_url() {
  local path=${1%/download}
  basename "$path" | sed -n -e 's/^\(.*\)\.tar\.[a-z0-9]*$/\1/p' -e 's/^\(.*\)\.tgz$/\1/p'
}

ort_url=$("$fetch" --url "$ort_row")
sherpa_url=$("$fetch" --url "$sherpa_row")
ort_stem=$(version_from_url "$ort_url")
sherpa_stem=$(version_from_url "$sherpa_url")
test -n "$ort_stem" || { echo "setup-build-env.sh: no version in $ort_url" >&2; exit 1; }
test -n "$sherpa_stem" || { echo "setup-build-env.sh: no version in $sherpa_url" >&2; exit 1; }

# The sherpa row must pin the version src/Cargo.lock resolves for
# sherpa-onnx-sys - the same cross-check the CI engine job runs, because
# the prebuilt library only fits that one version.
sherpa_lock_version=$(sed -n '/^name = "sherpa-onnx-sys"$/{n;s/^version = "\(.*\)"/\1/p;}' src/Cargo.lock)
case "$sherpa_url" in
  */v"$sherpa_lock_version"/*) ;;
  *)
    echo "setup-build-env.sh: deps.lock pins $sherpa_row for another version than Cargo.lock's $sherpa_lock_version" >&2
    exit 1
    ;;
esac

echo "setup-build-env.sh: $ffmpeg_row, $ort_stem, $sherpa_stem"

# FFmpeg development libraries: headers and shared libs in one tree,
# what FFMPEG_DIR points the bindings at. Same extract the engine job
# makes, --strip-components=1 over ffmpeg-dev/.
if [ ! -d ffmpeg-dev/include ]; then
  "$fetch" "$ffmpeg_row" ffmpeg.tar.xz
  rm -rf ffmpeg-dev
  mkdir -p ffmpeg-dev
  tar -xf ffmpeg.tar.xz -C ffmpeg-dev --strip-components=1
else
  echo "setup-build-env.sh: ffmpeg-dev/ already in place, kept"
fi

# ONNX Runtime, linked dynamically: the shared library the tests find
# through LD_LIBRARY_PATH. The .so symlink is made beside the versioned
# library exactly as the engine job makes it.
ort_version=$(printf '%s' "$ort_stem" | sed -n 's/^onnxruntime-linux-[a-z0-9]*-//p')
test -n "$ort_version" || { echo "setup-build-env.sh: no version in $ort_stem" >&2; exit 1; }
if [ ! -f "$ort_stem/lib/libonnxruntime.so" ] || [ ! -f "$ort_stem/lib/libonnxruntime.so.$ort_version" ]; then
  "$fetch" "$ort_row" onnxruntime.tgz
  rm -rf "$ort_stem"
  tar -xzf onnxruntime.tgz
  test -f "$ort_stem/lib/libonnxruntime.so.$ort_version"
  ln -sf "libonnxruntime.so.$ort_version" "$ort_stem/lib/libonnxruntime.so"
else
  echo "setup-build-env.sh: $ort_stem in place, kept"
fi

# sherpa-onnx's prebuilt static library, unpacked under sherpa/ the way
# the engine job unpacks it, so SHERPA_ONNX_LIB_DIR finds the same dir.
if [ ! -d "sherpa/$sherpa_stem/lib" ]; then
  "$fetch" "$sherpa_row" sherpa.tar.bz2
  rm -rf sherpa
  mkdir -p sherpa
  tar -xjf sherpa.tar.bz2 -C sherpa
  test -d "sherpa/$sherpa_stem/lib"
else
  echo "setup-build-env.sh: sherpa/$sherpa_stem in place, kept"
fi

# The tools the native crates drive. Missing ones are named, with the
# CI engine job's own apt line for a box that has root - this script
# installs nothing itself.
missing=
for tool in pkg-config cmake; do
  command -v "$tool" >/dev/null 2>&1 || missing="$missing $tool"
done
command -v cc >/dev/null 2>&1 || missing="$missing cc"
if [ -n "$missing" ]; then
  echo "setup-build-env.sh: missing tools:$missing" >&2
  echo "  the engine's system packages, the CI line:" >&2
  echo "  apt install -y libasound2-dev libclang-dev cmake" >&2
else
  echo "setup-build-env.sh: pkg-config, cmake and a C compiler present"
fi

# The no-root clang: the bindgen crates (ffmpeg-sys among them) drive
# libclang, whose runtime and builtin headers unpack cleanly out of the
# distribution's own packages. Nothing is installed - apt-get download
# fetches, dpkg-deb -x unpacks, sysroot/ holds the result and
# funcut-env.sh points LIBCLANG_PATH at it. A sysroot that is already
# in place is kept, like the extracts above.
if $debs; then
  clang_major="${FUNCUT_CLANG_VERSION:-19}"
  if [ -f "sysroot/usr/lib/$multiarch/libclang-${clang_major}.so.1" ]; then
    echo "setup-build-env.sh: sysroot/ clang ${clang_major} already in place, kept"
  else
    if ! command -v apt-get >/dev/null 2>&1; then
      echo "setup-build-env.sh: --debs needs apt-get (Debian or Ubuntu); install libclang the distribution's way instead" >&2
      exit 1
    fi
    rm -rf sysroot
    mkdir -p sysroot
    (
      cd sysroot
      apt-get download \
        "libclang1-${clang_major}" \
        "libclang-cpp${clang_major}" \
        "libllvm${clang_major}" \
        "libclang-common-${clang_major}-dev"
      for deb in ./*.deb; do
        dpkg-deb -x "$deb" .
      done
      rm -f ./*.deb
    )
    echo "setup-build-env.sh: clang ${clang_major} unpacked under sysroot/"
  fi
fi

echo "setup-build-env.sh: done - 'source scripts/funcut-env.sh' before any cargo command in src/"
