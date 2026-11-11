#!/usr/bin/env bash
# Retry transient Ubuntu mirror/DNS failures without hiding a persistent error.
set -euo pipefail

if (($# == 0)); then
  echo "usage: apt-retry.sh update|install [apt-get arguments...]" >&2
  exit 2
fi

for attempt in 1 2 3; do
  if sudo apt-get \
    -o Acquire::Retries=3 \
    -o Acquire::http::Timeout=20 \
    -o Acquire::https::Timeout=20 \
    "$@"; then
    exit 0
  fi
  if ((attempt < 3)); then
    delay=$((attempt * 5))
    echo "apt-get $1 failed; retrying in ${delay}s (attempt $((attempt + 1))/3)" >&2
    sleep "$delay"
  fi
done

echo "apt-get $1 failed after 3 attempts" >&2
exit 1
