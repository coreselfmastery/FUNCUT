#!/usr/bin/env bash
# Proves scripts/ci/fetch.sh rides the backoff for a body that stops short
# and still refuses a wrong digest at once. A local server truncates its
# first response and answers in full on the second; the fetch must retry,
# verify the digest on attempt 2 and never treat a mismatch as flaky.
#
#   scripts/ci/test-fetch.sh          run the test, print ok, exit 0/1
#
# Needs python3 and curl, nothing else: no network, no release CDN.
set -euo pipefail

here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
work=$(mktemp -d)
server=
cleanup() {
  [ -n "$server" ] && kill "$server" 2>/dev/null || true
  rm -rf "$work"
}
trap cleanup EXIT

mkdir -p "$work/scripts/ci"
cp "$here/fetch.sh" "$work/scripts/ci/fetch.sh"

body='funcut-fetch-selftest-'
for _ in $(seq 1 2000); do body=$body'funcut-fetch-selftest-'; done
if command -v sha256sum >/dev/null 2>&1; then
  digest=$(printf '%s' "$body" | sha256sum | awk '{ print $1 }')
else
  digest=$(printf '%s' "$body" | shasum -a 256 | awk '{ print $1 }')
fi

# The server: request 1 answers 200 with a body that stops 1000 bytes in,
# every later request answers 200 in full, and /missing answers 404.
cat >"$work/server.py" <<'PY'
import http.server, sys

BODY = sys.argv[2].encode()
hits = 0

class H(http.server.BaseHTTPRequestHandler):
    def do_GET(self):
        global hits
        if self.path == "/missing":
            self.send_error(404)
            return
        hits += 1
        self.send_response(200)
        self.send_header("Content-Length", str(len(BODY)))
        self.end_headers()
        if hits == 1:
            self.wfile.write(BODY[:1000])
            self.wfile.flush()
            self.close_connection = True
            return
        self.wfile.write(BODY)

    def log_message(self, *a):
        pass

http.server.HTTPServer(("127.0.0.1", int(sys.argv[1])), H).serve_forever()
PY

port=18731
for _ in $(seq 1 20); do
  if ! (exec 3<>"/dev/tcp/127.0.0.1/$port") 2>/dev/null; then break; fi
  port=$((port + 1))
done

python3 "$work/server.py" "$port" "$body" &
server=$!
for _ in $(seq 1 50); do
  if (exec 3<>"/dev/tcp/127.0.0.1/$port") 2>/dev/null; then break; fi
  sleep 0.1
done

url="http://127.0.0.1:$port/asset.bin"
failures=()

# 1. A truncated body is retried, and the digest verified on the second try.
printf 'flaky %s %s\n' "$url" "$digest" >"$work/scripts/ci/deps.lock"
if ! out=$(cd "$work" && bash scripts/ci/fetch.sh flaky out.bin 2>&1); then
  failures+=("a truncated download was not retried to success: $out")
else
  case "$out" in
    *"curl exit 18"*"retrying"*) ;;
    *) failures+=("the truncated attempt was not named as retried: $out") ;;
  esac
  case "$out" in
    *verified*) ;;
    *) failures+=("the retried download was not verified: $out") ;;
  esac
fi

# 2. A digest that does not match is fatal on the first attempt.
printf 'wrong %s %s\n' "$url" \
  "0000000000000000000000000000000000000000000000000000000000000000" \
  >"$work/scripts/ci/deps.lock"
if out=$(cd "$work" && bash scripts/ci/fetch.sh wrong out.bin 2>&1); then
  failures+=("a digest mismatch was accepted: $out")
else
  case "$out" in
    *"does not match deps.lock"*) ;;
    *) failures+=("a digest mismatch was not named: $out") ;;
  esac
fi

# 3. A 4xx is fatal at once, never retried.
printf 'gone http://127.0.0.1:%s/missing %s\n' "$port" "$digest" \
  >"$work/scripts/ci/deps.lock"
if out=$(cd "$work" && bash scripts/ci/fetch.sh gone out.bin 2>&1); then
  failures+=("a 404 was accepted: $out")
else
  case "$out" in
    *"HTTP 404"*) ;;
    *) failures+=("a 404 was not named: $out") ;;
  esac
  case "$out" in
    *retrying*) failures+=("a 404 was retried: $out") ;;
  esac
fi

if [ "${#failures[@]}" -gt 0 ]; then
  printf 'test-fetch: FAIL\n' >&2
  printf '  %s\n' "${failures[@]}" >&2
  exit 1
fi

echo "test-fetch: ok - a truncated body retried to a verified digest, a mismatch and a 404 fatal at once"
