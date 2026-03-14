#!/usr/bin/env bash
# Benchmark cold-start latency of the permissions binary.
#
# Usage: bash scripts/bench-coldstart.sh [--export-json FILE]
#
# Sends a minimal Bash-allow payload via stdin and measures wall-clock time.
# Target: < 10ms p99 on macOS ARM.

set -euo pipefail

BIN="${1:-target/release/permissions}"
EXPORT_FLAG=""
EXPORT_FILE=""

# Parse --export-json flag
for arg in "$@"; do
  case "$arg" in
    --export-json=*)
      EXPORT_FILE="${arg#*=}"
      EXPORT_FLAG="--export-json=${EXPORT_FILE}"
      ;;
  esac
done

if [ ! -x "$BIN" ]; then
  echo "ERROR: $BIN not found or not executable. Run: cargo build --release" >&2
  exit 1
fi

if ! command -v hyperfine &>/dev/null; then
  echo "ERROR: hyperfine not found. Install: brew install hyperfine" >&2
  exit 1
fi

# Minimal stdin payload — a safe Bash command that the permissions hook should allow
PAYLOAD='{"tool_name":"Bash","tool_input":{"command":"echo hello","description":"test"}}'

echo "Benchmarking: $BIN"
echo "Payload: Bash allow (fast path)"
echo ""

# shellcheck disable=SC2086
hyperfine \
  --warmup 3 \
  --min-runs 50 \
  --shell none \
  "echo '${PAYLOAD}' | ${BIN}" \
  ${EXPORT_FLAG:+"$EXPORT_FLAG"}
