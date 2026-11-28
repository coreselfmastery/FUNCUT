#!/usr/bin/env bash
# Retry transient Ubuntu mirror/DNS failures without hiding a persistent error.
set -euo pipefail

if (($# == 0)); then
  echo "usage: apt-retry.sh update|install [apt-get arguments...]" >&2
  exit 2
fi

# The runner image ships Microsoft's own apt shelf preinstalled, and its
# InRelease has taken to answering GitHub's runners with 403 Forbidden:
# the Ubuntu archives answer fine, but one broken shelf fails the whole
# `apt-get update`. Nothing this repository installs comes from
# Microsoft's shelf, so on a failed update it is dropped before the next
# attempt instead of failing the run for a shelf the run never reads.
drop_broken_microsoft_shelf() {
  local dropped=0
  local file
  for file in \
    /etc/apt/sources.list.d/microsoft-prod.list \
    /etc/apt/sources.list.d/microsoft-prod.sources; do
    if [[ -f $file ]]; then
      echo "dropping Microsoft's preinstalled shelf (403 on runners): $file" >&2
      sudo rm -f "$file"
      dropped=1
    fi
  done
  ((dropped)) || echo "no Microsoft shelf to drop; the failure is elsewhere" >&2
}

for attempt in 1 2 3; do
  if sudo apt-get \
    -o Acquire::Retries=3 \
    -o Acquire::http::Timeout=20 \
    -o Acquire::https::Timeout=20 \
    "$@"; then
    exit 0
  fi
  if [[ $1 == update ]]; then
    drop_broken_microsoft_shelf
  fi
  if ((attempt < 3)); then
    delay=$((attempt * 5))
    echo "apt-get $1 failed; retrying in ${delay}s (attempt $((attempt + 1))/3)" >&2
    sleep "$delay"
  fi
done

echo "apt-get $1 failed after 3 attempts" >&2
exit 1
