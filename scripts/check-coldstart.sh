#!/usr/bin/env bash
# Gate: verify permissions binary cold-start stays under threshold.
#
# Usage: bash scripts/check-coldstart.sh
# Exit 0 if p99 < 15ms, exit 1 otherwise.
#
# Uses hyperfine --export-json to extract max latency as a p99 proxy.

set -euo pipefail

BIN="target/release/permissions"
THRESHOLD_MS=15
TMP=$(mktemp)

if [ ! -x "$BIN" ]; then
  echo "SKIP: $BIN not found (run cargo build --release first)" >&2
  exit 0
fi

if ! command -v hyperfine &>/dev/null; then
  echo "SKIP: hyperfine not installed" >&2
  exit 0
fi

PAYLOAD='{"tool_name":"Bash","tool_input":{"command":"echo hello","description":"test"}}'

hyperfine \
  --warmup 3 \
  --min-runs 30 \
  --shell none \
  --export-json "$TMP" \
  "echo '${PAYLOAD}' | ${BIN}" \
  2>/dev/null

MAX_MS=$(jq '.results[0].max * 1000 | floor' "$TMP")
MEAN_MS=$(jq '.results[0].mean * 1000 * 10 | floor / 10' "$TMP")

rm -f "$TMP"

echo "Cold-start: mean=${MEAN_MS}ms, max=${MAX_MS}ms (threshold=${THRESHOLD_MS}ms)"

if [ "$MAX_MS" -gt "$THRESHOLD_MS" ]; then
  echo "FAIL: max ${MAX_MS}ms exceeds ${THRESHOLD_MS}ms threshold" >&2
  exit 1
fi

echo "PASS"
