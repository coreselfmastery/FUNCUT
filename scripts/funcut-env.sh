#!/usr/bin/env bash
# The environment a local build shares with the CI engine job: the
# FFmpeg, ONNX Runtime and sherpa-onnx that scripts/setup-build-env.sh
# fetched and unpacked, exported under the same five names
# .github/workflows/ci.yml's engine job sets - FFMPEG_DIR,
# ORT_LIB_LOCATION, ORT_PREFER_DYNAMIC_LINK, LD_LIBRARY_PATH and
# SHERPA_ONNX_LIB_DIR - plus the memory floor a small machine needs.
#
#   source scripts/funcut-env.sh
#
# Sourced, never run: it only exports, and every path is derived from
# the script's own place in the checkout, so it works from any clone.
# The versions come from scripts/ci/deps.lock's URLs - the one source
# of truth, the same way setup-build-env.sh reads them.
#
# CARGO_PROFILE_DEV_DEBUG=0 is the 4 GB machine's floor ([profile.dev]
# in src/Cargo.toml says the whole story): it overrides the profile's
# line tables for a build that fits, and a debugging session on a
# machine that can afford more sets the variable back -
# CARGO_PROFILE_DEV_DEBUG=2 cargo test - the documented way.
#
# No `set` of its own: a sourced script that sets the caller's shell
# options hands every build script that sources this one an exit-on-error
# and a nounset it never asked for. The guards below fail loudly on
# their own.

_funcut_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
_fetch="$_funcut_root/scripts/ci/fetch.sh"

case "$(uname -m)" in
  x86_64)
    _funcut_ort_row=onnxruntime-linux-x64
    _funcut_sherpa_row=sherpa-onnx-linux-x64-static-lib
    _funcut_multiarch=x86_64-linux-gnu
    ;;
  aarch64 | arm64)
    _funcut_ort_row=onnxruntime-linux-aarch64
    _funcut_sherpa_row=sherpa-onnx-linux-aarch64-static-lib
    _funcut_multiarch=aarch64-linux-gnu
    ;;
  *)
    echo "funcut-env.sh: no deps.lock rows for $(uname -m)" >&2
    return 1
    ;;
esac

# A SourceForge mirror URL carries "/download" after the filename (the
# same shape check-ort-pins.py pins); strip it before the basename, or
# the stem comes back empty and the guard below refuses to source.
_funcut_stem_from_url() {
  _funcut_path=$1
  case "$_funcut_path" in
    */download) _funcut_path="${_funcut_path%/download}" ;;
  esac
  basename "$_funcut_path" | sed -n -e 's/^\(.*\)\.tar\.[a-z0-9]*$/\1/p' -e 's/^\(.*\)\.tgz$/\1/p'
}
_funcut_ort_stem=$(_funcut_stem_from_url "$("$_fetch" --url "$_funcut_ort_row")")
_funcut_sherpa_stem=$(_funcut_stem_from_url "$("$_fetch" --url "$_funcut_sherpa_row")")
test -n "$_funcut_ort_stem" || { echo "funcut-env.sh: no ONNX Runtime version in deps.lock" >&2; return 1; }
test -n "$_funcut_sherpa_stem" || { echo "funcut-env.sh: no sherpa-onnx version in deps.lock" >&2; return 1; }

for _funcut_want in \
  "ffmpeg-dev/include" \
  "$_funcut_ort_stem/lib" \
  "sherpa/$_funcut_sherpa_stem/lib"; do
  if [ ! -d "$_funcut_root/$_funcut_want" ]; then
    echo "funcut-env.sh: $_funcut_want is missing - run 'bash scripts/setup-build-env.sh' first" >&2
    return 1
  fi
done

export FFMPEG_DIR="$_funcut_root/ffmpeg-dev"
export ORT_LIB_LOCATION="$_funcut_root/$_funcut_ort_stem/lib"
export ORT_PREFER_DYNAMIC_LINK=1
export LD_LIBRARY_PATH="$_funcut_root/ffmpeg-dev/lib:$_funcut_root/$_funcut_ort_stem/lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
export SHERPA_ONNX_LIB_DIR="$_funcut_root/sherpa/$_funcut_sherpa_stem/lib"

# The no-root clang a --debs setup unpacked, when it is there: the
# bindgen crates find libclang through LIBCLANG_PATH, and its builtin
# headers (stddef.h and family, which the runtime packages do not
# carry) ride along as an extra include. An already-set
# BINDGEN_EXTRA_CLANG_ARGS keeps what it had.
_funcut_sysroot_lib="$_funcut_root/sysroot/usr/lib/$_funcut_multiarch"
if [ -d "$_funcut_sysroot_lib" ]; then
  export LIBCLANG_PATH="${LIBCLANG_PATH:-$_funcut_sysroot_lib}"
  # The same directory on the linker's and the loader's paths: the -sys
  # crates bind against the unpacked ALSA/fontconfig/freetype/xkbcommon
  # without an install, and the test binaries they link need their .so
  # at run time too.
  case ":${RUSTFLAGS:-}:" in
    *"-L $_funcut_sysroot_lib"*) ;;
    *) export RUSTFLAGS="${RUSTFLAGS:+$RUSTFLAGS }-L $_funcut_sysroot_lib" ;;
  esac
  case ":${LD_LIBRARY_PATH:-}:" in
    *":$_funcut_sysroot_lib:"*) ;;
    *) export LD_LIBRARY_PATH="$_funcut_sysroot_lib:${LD_LIBRARY_PATH:-}" ;;
  esac
  case ":${LD_LIBRARY_PATH:-}:" in
    *":$_funcut_sysroot_lib:"*) ;;
    *) export LD_LIBRARY_PATH="$_funcut_sysroot_lib:${LD_LIBRARY_PATH:-}" ;;
  esac
  _funcut_clang_include=$(find "$_funcut_root/sysroot/usr/lib" -type d -path '*lib/clang/*/include' 2>/dev/null | head -1)
  if [ -n "$_funcut_clang_include" ]; then
    case " ${BINDGEN_EXTRA_CLANG_ARGS:-} " in
      *"-I$_funcut_clang_include"*) ;;
      *) export BINDGEN_EXTRA_CLANG_ARGS="${BINDGEN_EXTRA_CLANG_ARGS:-}${BINDGEN_EXTRA_CLANG_ARGS:+ }-I$_funcut_clang_include" ;;
    esac
  fi
fi

export CARGO_PROFILE_DEV_DEBUG="${CARGO_PROFILE_DEV_DEBUG:-0}"
export CARGO_INCREMENTAL="${CARGO_INCREMENTAL:-0}"

# The no-root sysroot's pkg-config entries, when a --debs (or fuller)
# setup unpacked them: the -sys crates that bind against ALSA,
# fontconfig, freetype and xkbcommon find them here without an install.
_funcut_pkgconfig="$_funcut_root/sysroot/usr/lib/$_funcut_multiarch/pkgconfig"
if [ -d "$_funcut_pkgconfig" ]; then
  case ":${PKG_CONFIG_PATH:-}:" in
    *":$_funcut_pkgconfig:"*) ;;
    *) export PKG_CONFIG_PATH="$_funcut_pkgconfig${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}" ;;
  esac
fi

echo "funcut-env.sh: FFMPEG_DIR, ORT_LIB_LOCATION, SHERPA_ONNX_LIB_DIR set for $_funcut_sherpa_stem" >&2

unset _funcut_root _funcut_ort_row _funcut_sherpa_row _funcut_ort_stem \
  _funcut_sherpa_stem _funcut_want _funcut_multiarch _funcut_sysroot_lib \
  _funcut_clang_include
