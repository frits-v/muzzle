#!/usr/bin/env bash
# shellcheck disable=SC2016
# Validate CLAUDE.md claims against the actual codebase.
# Exits 0 if all checks pass, 1 on any mismatch.
#
# Follows Google Shell Style Guide:
#   https://google.github.io/styleguide/shellguide.html
#
# Requires: bash >= 4.0 (mapfile), cargo, awk, sed, grep
set -euo pipefail

# --- Version guard ---
if ((BASH_VERSINFO[0] < 4)); then
  echo >&2 "ERROR: bash >= 4.0 required (have ${BASH_VERSION}). Install via: brew install bash"
  exit 1
fi

readonly CLAUDE_MD="CLAUDE.md"
FAIL=0

if [[ ! -f "${CLAUDE_MD}" ]]; then
  echo "FAIL: ${CLAUDE_MD} not found"
  exit 1
fi

# --- Helpers ---

fail() {
  echo "FAIL: $1"
  FAIL=1
}

pass() {
  echo "PASS: $1"
}

# --- 1. Binary count ---

claimed_binaries=""
claimed_binaries="$(grep -oE 'producing [0-9]+ binaries' "${CLAUDE_MD}" | grep -oE '[0-9]+')"

actual_binaries=""
actual_binaries="$(grep -c '^\[\[bin\]\]' Cargo.toml)"

if ((claimed_binaries == actual_binaries)); then
  pass "binary count (${actual_binaries})"
else
  fail "binary count: CLAUDE.md says ${claimed_binaries}, Cargo.toml has ${actual_binaries}"
fi

# --- 2. Architecture tree: every listed .rs file exists ---

mapfile -t arch_files < <(
  sed -n '/^```$/,/^```$/p' "${CLAUDE_MD}" \
    | head -30 \
    | grep -oE '[a-z_]+\.rs' \
    | sort -u
)

for f in "${arch_files[@]}"; do
  found="$(find src -name "${f}" -type f 2>/dev/null | head -1)"
  if [[ -z "${found}" ]]; then
    fail "architecture tree lists ${f} but file not found in src/"
  fi
done

# Check for .rs files in src/ not listed in the architecture tree.
mapfile -t actual_files < <(
  find src -name '*.rs' -type f \
    | sed 's|.*/||' \
    | sort -u
)

missing=()
for f in "${actual_files[@]}"; do
  found=false
  for a in "${arch_files[@]}"; do
    if [[ "${f}" == "${a}" ]]; then
      found=true
      break
    fi
  done
  if [[ "${found}" == "false" ]]; then
    missing+=("${f}")
  fi
done

if ((${#missing[@]} == 0)); then
  pass "architecture tree completeness"
else
  fail "source files not in architecture tree: ${missing[*]}"
fi

# --- 3. Dependency count ---

claimed_deps=""
claimed_deps="$(grep -oE '^[0-9]+ crates' "${CLAUDE_MD}" | grep -oE '^[0-9]+')"

actual_deps=""
actual_deps="$(
  sed -n '/^\[dependencies\]/,/^\[/p' Cargo.toml \
    | grep -cE '^[a-z]'
)"

if ((claimed_deps == actual_deps)); then
  pass "dependency count (${actual_deps})"
else
  fail "dependency count: CLAUDE.md says ${claimed_deps}, Cargo.toml has ${actual_deps}"
fi

# --- 4. Named dependencies match ---

mapfile -t claimed_dep_names < <(
  grep -oE '`[a-z_][a-z0-9_-]*`' "${CLAUDE_MD}" | tr -d '`' | sort -u
)

mapfile -t actual_dep_names < <(
  sed -n '/^\[dependencies\]/,/^\[/p' Cargo.toml \
    | grep -oE '^[a-z_][a-z0-9_-]*' \
    | sort -u
)

for dep in "${actual_dep_names[@]}"; do
  found=false
  for claimed in "${claimed_dep_names[@]}"; do
    if [[ "${dep}" == "${claimed}" ]]; then
      found=true
      break
    fi
  done
  if [[ "${found}" == "false" ]]; then
    fail "dependency '${dep}' in Cargo.toml but not mentioned in CLAUDE.md"
  fi
done

# --- 5. Test count ---
# Sum all "N passed" lines across all test binaries.

claimed_tests=""
claimed_tests="$(grep -oE '^[0-9]+ tests' "${CLAUDE_MD}" | grep -oE '^[0-9]+' | head -1)"

if [[ -n "${claimed_tests}" ]]; then
  actual_tests=""
  actual_tests="$(
    cargo test 2>&1 \
      | grep -oE '[0-9]+ passed' \
      | grep -oE '[0-9]+' \
      | awk '{s+=$1} END{print s}'
  )"

  if [[ -n "${actual_tests}" ]] && ((actual_tests > 0)); then
    if ((actual_tests < claimed_tests)); then
      fail "test count: CLAUDE.md says ${claimed_tests}, cargo test found ${actual_tests}"
    elif ((actual_tests > claimed_tests)); then
      fail "test count stale: CLAUDE.md says ${claimed_tests} but ${actual_tests} now pass (update CLAUDE.md)"
    else
      pass "test count (${actual_tests})"
    fi
  fi
fi

# --- 6. Make targets listed actually exist ---

mapfile -t make_targets < <(
  grep -oE 'make [a-z_-]+' "${CLAUDE_MD}" | awk '{print $2}' | sort -u
)

if [[ -f Makefile ]]; then
  for target in "${make_targets[@]}"; do
    if ! grep -qE "^${target}:" Makefile; then
      fail "make target '${target}' listed in CLAUDE.md but not in Makefile"
    fi
  done
  pass "make targets"
fi

# --- Summary ---

if ((FAIL == 0)); then
  echo "---"
  echo "All CLAUDE.md checks passed."
  exit 0
else
  echo "---"
  echo "CLAUDE.md has stale or incorrect claims. Update it."
  exit 1
fi
