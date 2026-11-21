#!/usr/bin/env bash
# Downloads one of the prebuilt dependencies scripts/ci/deps.lock names and
# checks it against the digest written there, so a build only ever links
# or ships bytes somebody has looked at once. A file whose digest does not
# match is deleted and the script fails, naming both digests: a release
# that was replaced upstream, or a download that went wrong, stops the
# build rather than ending up in a bundle.
#
#   scripts/ci/fetch.sh NAME OUT     download NAME to the file OUT
#   scripts/ci/fetch.sh --url NAME   print the URL deps.lock has for NAME
#
# OUT already there with the right digest is kept, so a rerun on a warm
# machine downloads nothing. Runs on Linux, macOS and Git's bash on
# Windows: sha256sum where there is one, shasum where there is not.
#
# To bump a dependency, change its row and put the new file's digest in
# it; scripts/ci/fetch.sh prints the digest it got when they differ.
#
# The release-asset CDN returns intermittent 500s and 504s for minutes
# at a stretch -- BtbN's FFmpeg, the Geneva sidecar and ONNX Runtime all
# went dark in one afternoon, and curl's own --retry backoff is spent
# in half a minute. So each asset is fetched up to eight times with an
# exponential backoff (2s, 4s, 8s, 16s, 32s, then 60s a time, slept
# before the first through the eighth attempt). Transient failures -- any HTTP 5xx, a
# timeout (curl exit 28), a connection that died mid-answer (exit 18
# with a truncated body, 52 with no reply at all, 55 on the send side,
# 56 on the receive side, 92 on an HTTP/2 stream) -- ride the backoff; an
# HTTP 4xx such as a 404 or 403 fails at once, because retrying those
# cannot help. The digest is verified on every attempt and a mismatch is
# never retried: a file that does not match deps.lock is a wrong release
# or a broken download, not a flaky CDN.
set -euo pipefail

here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
lock="$here/deps.lock"

usage() {
  echo "usage: $0 NAME OUT | $0 --url NAME" >&2
  exit 2
}

# The row for a name: its URL and digest, space-separated.
row() {
  awk -v name="$1" '
    /^[[:space:]]*(#|$)/ { next }
    $1 == name { print $2, $3; found = 1; exit }
    END { exit !found }
  ' "$lock"
}

digest() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{ print $1 }'
  else
    shasum -a 256 "$1" | awk '{ print $1 }'
  fi
}

[ $# -eq 2 ] || usage

if [ "$1" = "--url" ]; then
  found=$(row "$2") || { echo "fetch.sh: no row named $2 in $lock" >&2; exit 1; }
  echo "${found%% *}"
  exit 0
fi

name=$1
out=$2
found=$(row "$name") || { echo "fetch.sh: no row named $name in $lock" >&2; exit 1; }
url=${found%% *}
want=${found##* }

if [ -f "$out" ] && [ "$(digest "$out")" = "$want" ]; then
  echo "fetch.sh: $name is already at $out"
  exit 0
fi

echo "fetch.sh: $name from $url"
mkdir -p "$(dirname "$out")"

attempts=8

# The wait before attempt N: 2s, 4s, 8s, 16s, 32s, then 60s a time, so a
# CDN outage of a few minutes is ridden out rather than failing the job.
backoff_for() {
  case "$1" in
    1) echo 2 ;;
    2) echo 4 ;;
    3) echo 8 ;;
    4) echo 16 ;;
    5) echo 32 ;;
    *) echo 60 ;;
  esac
}

is_2xx() {
  case "$1" in 2??) return 0 ;; esac
  return 1
}

is_5xx() {
  case "$1" in 5??) return 0 ;; esac
  return 1
}

err=$(mktemp)
trap 'rm -f "$err"' EXIT

attempt=1
while :; do
  sleep "$(backoff_for "$attempt")"

  rc=0
  # A server that accepts the connection and then stalls the body never
  # trips --connect-timeout; --speed-limit/-time aborts it (curl exit 28,
  # the timeout the backoff below was written for) once less than a
  # kilobyte a second has arrived for a straight minute.
  code=$(curl -sS -L --connect-timeout 30 --speed-limit 1024 --speed-time 60 \
    -o "$out" -w '%{http_code}' "$url" 2>"$err") || rc=$?

  if [ "$rc" -eq 0 ] && is_2xx "$code"; then
    got=$(digest "$out")
    if [ "$got" != "$want" ]; then
      rm -f "$out"
      {
        echo "fetch.sh: $name does not match deps.lock"
        echo "  url:      $url"
        echo "  expected: $want"
        echo "  got:      $got"
      } >&2
      exit 1
    fi
    echo "fetch.sh: $name attempt $attempt/$attempts: HTTP $code"
    echo "fetch.sh: $name verified (sha256 $got)"
    exit 0
  fi

  reason=
  if { [ "$rc" -eq 0 ] || [ "$rc" -eq 22 ]; } && is_5xx "$code"; then
    reason="HTTP $code"
  elif [ "$rc" -eq 28 ]; then
    reason="HTTP $code, curl exit 28 (timeout)"
  elif [ "$rc" -eq 18 ]; then
    reason="HTTP $code, curl exit 18 (partial file)"
  elif [ "$rc" -eq 52 ]; then
    reason="HTTP $code, curl exit 52 (empty reply)"
  elif [ "$rc" -eq 55 ]; then
    reason="HTTP $code, curl exit 55 (send failure)"
  elif [ "$rc" -eq 56 ]; then
    reason="HTTP $code, curl exit 56 (recv failure)"
  elif [ "$rc" -eq 92 ]; then
    reason="HTTP $code, curl exit 92 (HTTP/2 stream)"
  fi

  if [ -n "$reason" ] && [ "$attempt" -lt "$attempts" ]; then
    echo "fetch.sh: $name attempt $attempt/$attempts: $reason, retrying in $(backoff_for "$((attempt + 1))")s"
    attempt=$((attempt + 1))
    continue
  fi

  rm -f "$out"
  {
    if [ -n "$reason" ]; then
      echo "fetch.sh: $name failed after $attempt attempts: $reason"
    else
      echo "fetch.sh: $name failed: HTTP $code, curl exit $rc"
    fi
    echo "  url: $url"
    if [ -s "$err" ]; then
      echo "  curl: $(tail -n 1 "$err")"
    fi
  } >&2
  exit 1
done
