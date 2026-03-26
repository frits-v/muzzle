#!/usr/bin/env bash
# Portable shellcheck wrapper for GOALS.md gate.
#
# Resolves shellcheck from: PATH, mise, or known mise install location.
# Usage: bash scripts/run-shellcheck.sh [files...]
#        bash scripts/run-shellcheck.sh scripts/*.sh

set -euo pipefail

find_shellcheck() {
  # 1. Already on PATH
  if command -v shellcheck &>/dev/null; then
    command -v shellcheck
    return
  fi

  # 2. mise shim
  local mise_path
  mise_path="$(mise which shellcheck 2>/dev/null)" && [[ -x "$mise_path" ]] && {
    echo "$mise_path"
    return
  }

  # 3. Known mise install locations (glob across versions)
  local candidate
  for candidate in ~/.local/share/mise/installs/shellcheck/*/shellcheck; do
    if [[ -x "$candidate" ]]; then
      echo "$candidate"
      return
    fi
  done

  echo "ERROR: shellcheck not found. Install via: mise install shellcheck" >&2
  return 1
}

SC="$(find_shellcheck)"

if (($# == 0)); then
  exec "$SC" scripts/*.sh
else
  exec "$SC" "$@"
fi
