# Goals

Fitness goals for muzzle — session isolation hooks for Claude Code.

## North Stars

- Every session gets isolated worktrees; concurrent sessions never clobber each other
- Hooks never fail open on crash (panic = deny)
- All checks pass on every commit

## Anti Stars

- Untested changes reaching main
- Writes to main checkouts bypassing worktree isolation
- Silent data loss in spec files or worktree state

## Directives

### 1. Add CI via GitHub Actions

All 9 gates covered in `.github/workflows/ci.yml`:
- [x] check & lint: fmt, clippy --all-targets, rustdoc -D warnings, license
- [x] test: unit + integration on macOS
- [x] build: release build, 5-binary verify, binary size check

**Steer:** complete

### 2. Harden sandbox edge cases

Fuzz and stress-test the permission boundaries: symlink traversal, path
canonicalization, race conditions in worktree creation, Unicode filenames,
paths with spaces. Each edge case gets a test.

Added `normalize_dot_segments()` to fix dot-dot traversal bypass in
`resolve_path()`. 22 new tests covering: symlink traversal (2), path
traversal (2), spaces (2), Unicode (1), double-slash (1), `/private/`
prefix (1), empty/long paths (2), trailing slashes (1), case sensitivity
(1), null session (1), worktree escapes (2), `normalize_dot_segments`
unit tests (5), WORKTREE_MISSING (1), `/dev/fd` range (1).

**Steer:** increase

### 3. Fuzz regex-heavy path parsing

Added 4 fuzz targets via `cargo-fuzz`: `fuzz_git_safety`, `fuzz_sandbox`,
`fuzz_bash_write_paths`, `fuzz_extract_repo`. Initial run: ~4M iterations
across all targets, zero crashes. Run with nightly:
`cargo +nightly fuzz run <target> -- -max_total_time=60`

**Steer:** complete

### 4. Add property-based tests for sandbox decisions

Added 14 property-based tests via proptest covering sandbox, gitcheck,
and MCP routing invariants: system paths always denied, no panics on
arbitrary input, /tmp paths via Bash allowed, force-push always blocked,
safe git never blocked, unknown MCP always Ask, non-MCP always Allow,
MCP route never panics, MCP route never Deny. Each property runs 256
cases by default.

**Steer:** complete

### 5. Structured JSON logging from all binaries

Added `src/log.rs` module with `emit()`, `emit_full()`, `error()`, `warn()`
functions producing JSON lines to stderr. Converted all 17 `eprintln!` calls
across 4 files (session_start, session_end, ensure_worktree, worktree/mod).
Each log entry includes: `ts`, `level`, `bin`, `msg`, optional `session` and
`detail` fields. 3 unit tests for format, JSON validity, and escaping.

**Steer:** complete

### 6. Semantic versioning with cargo-release

Added `release.toml` for cargo-release with changelog replacement rules,
tag format (`v{{version}}`), and crates.io publish disabled. Created
`CHANGELOG.md` with v0.1.0 and v0.2.0 entries. Bumped to v0.2.0 and tagged.
Usage: `cargo release patch|minor|major [--execute]`.

**Steer:** complete

### 7. Improve cargo doc coverage

Added `#![warn(missing_docs)]` to lib.rs with crate-level documentation.
Documented all public structs, enums, variants, fields, constants, and
associated functions across all modules. Zero rustdoc warnings with `-D warnings`.

**Steer:** complete

### 8. Benchmark permissions binary cold-start latency

Added `scripts/bench-coldstart.sh` (hyperfine, 50+ runs) and
`scripts/check-coldstart.sh` (gate: fail if max > 15ms). Baseline:
3.4ms mean, 9.4ms max on macOS ARM — well under 10ms target.

**Steer:** complete

### 9. Graceful degradation when workspace is missing

Added `config::validate_workspace()` with early validation in `ensure-worktree`.
Tests cover both existing and missing workspace paths.
`session-start` already exits cleanly via `is_in_workspace()` check.

**Steer:** complete

### 10. Maintain test coverage above 100 tests

Current: 269 tests (211 hooks unit + 5 doc + 13 hooks integration + 14 proptest + 22 memory unit + 4 memory integration). Do not regress.

**Steer:** increase

### 11. Enforce conventional commit format

All commits and PR titles must follow Conventional Commits (`type(scope): description`).
Valid types: `feat`, `fix`, `docs`, `chore`, `ci`, `test`, `refactor`, `perf`, `evolve`.
Scopes are optional. See CLAUDE.md for full spec.

**Steer:** increase

### 12. Shell scripts pass shellcheck + shfmt

All `.sh` files must pass `shellcheck` (no warnings) and `shfmt -d -i 2 -ci -bn`
(Google Shell Style: 2-space indent, case indent, binary operator newline).
Scripts must guard `bash >= 4.0` when using `mapfile` or associative arrays.

**Steer:** increase

### 13. CLAUDE.md stays in sync with codebase

`tests/claude_md.rs` validates that CLAUDE.md claims (binary count,
architecture tree, dependency count/names, make targets) match the actual
codebase. Runs as a Rust integration test — portable, no shell dependency.

**Steer:** increase

### 14. PRs must rebase cleanly before push

Always rebase on `origin/<default-branch>` before pushing a feature branch.
PRs with merge conflicts must never be submitted. This is a process gate, not
an automated check — enforced by convention and CLAUDE.md instructions.

**Steer:** increase

### 15. PR review comments must be addressed

All review comments on PRs (human or bot) must be resolved before merge.
For each comment: either fix the issue, or if `/council` disagrees with the
suggestion, respond to the comment with a reasoned explanation. No comment
should be left unaddressed.

For each addressed comment: reply to the thread explaining what was fixed
(or why you disagree), then resolve the conversation. This creates a clear
audit trail and marks the thread as done in GitHub's UI.

After pushing fixes, wait 5-10 minutes for automated reviewers (Greptile, etc.)
to re-review, then poll for new comments. Repeat until no new comments appear.
The review loop is: push → wait → check → address → push → ... → converge.

**Steer:** increase

### 16. CI must be green before merge

PRs must not be merged with failing CI checks. If CI fails after push,
diagnose and fix the failure before requesting review or merge. This
includes lint, fmt, test, build, and all custom gates.

**Steer:** increase

### 17. GitHub Actions SHA-pinned with supply chain lint

All `uses:` references in `.github/workflows/` must be pinned to full
40-char commit SHAs with a version comment (e.g.
`actions/checkout@11bd7190... # v4.2.2`). No rolling tags (`@v4`, `@main`).

Every workflow change must pass `actionlint` and `zizmor --pedantic` in CI.

**Steer:** increase

### 18. Automated releases via release-please + cosign

Conventional commits on `main` trigger release-please to open a Release PR.
Merging that PR creates a GitHub Release. The release workflow builds macOS
binaries (arm64 + x86_64), signs with cosign (keyless OIDC), and uploads
tarballs + `.sigstore.json` bundles + `SHA256SUMS.txt` to the release.

**Steer:** increase

### 19. Maximize OpenSSF Scorecard

Current score: 6.1/10. Eight checks already at 10/10 (Dangerous-Workflow,
Dependency-Update-Tool, Token-Permissions, Binary-Artifacts, Pinned-Dependencies,
License, Vulnerabilities, Fuzzing). Target: 8.0+/10.

Actionable improvements (ordered by effort):
- [x] Add `SECURITY.md` with vulnerability reporting policy (Security-Policy: 0→10)
- [x] Enable branch protection: require status checks, dismiss stale reviews (Branch-Protection: 3→8+)
- [x] Add CodeQL analysis workflow for SAST (`.github/workflows/codeql.yml`)
- [x] Ensure all PRs go through CI before merge (CI-Tests: 6→10) — branch protection requires 4 status checks
- [ ] Trigger first signed release via release-please (Signed-Releases: -1→10)

Not actionable (time/structural):
- Maintained (0/10) — repo < 90 days old, self-resolves
- Contributors (0/10) — single-org project
- CII-Best-Practices (0/10) — requires manual registration at bestpractices.coreinfrastructure.org

**Steer:** increase

### 20. muzzle-memory v0.1 quality and coverage

SQLite + FTS5 memory crate shipped in PR #19. Track deferred review findings
and ensure the memory crate meets the same quality bar as hooks.

Deferred items:
- [ ] `stats()` counts empty sessions without observations (#20)
- [ ] Add ONNX vector embeddings behind `semantic` feature flag (v0.2)
- [ ] Wire SessionStart inject + SessionEnd capture hooks into settings.json
- [ ] Import existing MEMORY.md entries as observations before switching to auto-gen

Current: 26 memory tests (22 unit + 4 integration). Binary: 1.4MB.

**Steer:** increase

### 21. Fix `is_worktree_management_op` false positive

`is_worktree_management_op()` used `contains("worktree")` which matched
any command mentioning the word "worktree" (e.g. `echo worktree`). Fixed
by replacing with `RE_GIT_WORKTREE` regex requiring `\bgit\b.*\bworktree\s+`
followed by a valid subcommand (`add|list|remove|prune|move|repair|lock|unlock`).
Unified with the `check_worktree_enforcement` regex (removed duplicate).
Test updated: 9 positive cases, 6 negative cases including the previously-broken
`echo worktree`.

**Steer:** complete

### 22. Golden fixtures for hook JSON payloads

Integration tests hardcode JSON strings that mirror the implementation's
expected format. No captured real Claude Code hook payloads exist as
committed fixtures. If Claude Code changes the payload envelope, tests
would still pass against the stale format.

Capture real `SessionStart`, `PreToolUse`, and `PostToolUse` payloads
from a live session and commit as `tests/fixtures/*.json`. Use these as
the primary input source for integration tests.

**Steer:** increase

### 23. Property-based tests for MCP routing

Added 4 property-based tests for MCP routing invariants: unknown MCP
providers always get Ask (never Allow), non-MCP tools always get Allow,
route never panics on arbitrary input, and route never returns Deny.
Each property runs 256 cases by default.

**Steer:** complete

### 24. Establish mutation testing baseline

Baseline established via `cargo mutants` on `sandbox.rs`, `gitcheck.rs`,
`mcp.rs`: **180 mutants tested, 121 caught, 45 missed, 14 unviable**
(67% kill rate on critical modules).

Missed mutants by function (security-critical first):

| Function                       | Missed | Root cause                                           |
|--------------------------------|--------|------------------------------------------------------|
| `check_worktree_enforcement`   | 11     | Boolean logic (`\|\|`↔`&&`, `!` delete) not isolated |
| `extract_repo_from_git_op`     | 7      | Multi-workspace path splitting under-tested           |
| `check_bash_write_paths`       | 5      | Redirect operators (`>`, `>>`, `2>`) not distinct     |
| `is_system_path_resolved`      | 3      | Symlink resolution paths not all exercised            |
| `check_path_with_context`      | 6      | Context-dependent branches (`&&`↔`\|\|`) under-tested |
| `route_github`                 | 2      | Ask arms delete to fallthrough Ask (equivalent)       |
| `route_atlassian`              | 3      | `\|\|` chains in read-only prefix matching            |
| `route_slack`                  | 2      | `\|\|` chains in prefix matching                      |
| `check_atlassian_rate_limit`   | 2      | Off-by-one: `<` vs `<=`, `>` vs `>=`                 |
| `extract_repo` / `extract_rel` | 3      | Return value mutations (empty vs "xyzzy")             |
| `resolve_path`                 | 1      | `!` deletion in existence check                       |

Target: kill all missed mutants in security-critical functions
(`check_worktree_enforcement`, `check_path_with_context`,
`is_system_path_resolved`, `check_bash_write_paths`). MCP route arm
deletions that fall through to the same `Ask` variant are arguably
equivalent and acceptable.

**Steer:** increase

### 25. Strengthen changelog format tests with invariant properties

Added 4 property tests for `format_entry` structural invariants:
output is always non-empty, always contains a bold marker (`**...**`),
always starts with a backtick timestamp, and Bash entries with long
commands (>200 chars) are always truncated. Uses generated tool names
and input fields. Each property runs 256 cases by default.

**Steer:** complete
## Gates

| ID              | Check                                            | Weight | Description                       |
|-----------------|--------------------------------------------------|--------|-----------------------------------|
| cargo-build     | `cargo build`                                    | 8      | Release-ready Rust build          |
| cargo-test      | `cargo test`                                     | 8      | All unit tests pass               |
| cargo-clippy    | `cargo clippy --all-targets -- -D warnings`      | 5      | No clippy warnings                |
| cargo-fmt       | `cargo fmt -- --check`                           | 3      | Code formatted per rustfmt.toml   |
| integration     | `cargo build && cargo test --test integration`   | 5      | Integration tests pass            |
| six-binaries    | `cargo build --release && test -f target/release/session-start && test -f target/release/permissions && test -f target/release/changelog && test -f target/release/session-end && test -f target/release/ensure-worktree && test -f target/release/memory` | 5 | All 6 binaries produced |
| rustdoc         | `RUSTDOCFLAGS="-D warnings" cargo doc --no-deps`         | 3      | No rustdoc warnings             |
| binary-size     | `cargo build --release && test $(stat -f%z target/release/permissions) -lt 5242880` | 2 | Each binary stays under 5 MB |
| claude-md-valid | `cargo test --test claude_md`                            | 3      | CLAUDE.md matches codebase      |
| shellcheck      | `shellcheck scripts/*.sh`                                | 3      | Shell scripts pass shellcheck   |
| shfmt           | `shfmt -d -i 2 -ci -bn scripts/*.sh`                    | 2      | Shell scripts formatted (Google)|
| license-exists  | `test -f LICENSE`                                        | 1      | MIT license file present        |
| ci-green        | `gh pr checks <pr-number>`                               | 5      | All CI checks pass before merge |
| pr-comments     | `gh api repos/{owner}/{repo}/pulls/{pr}/comments --jq 'map(select(.created_at > "{last_push_time}")) | length'` → 0 after 5-10 min wait | 3 | No new review comments after last push |
| actionlint      | `actionlint .github/workflows/*.yml`                         | 3      | Workflow files pass actionlint    |
| zizmor          | `zizmor --pedantic .github/workflows/`                       | 3      | Workflow files pass zizmor        |
| sha-pinned      | `grep -rE 'uses:.*@[a-z][^#]*$' .github/workflows/` returns 0 lines          | 3 | All actions SHA-pinned |
| security-policy | `test -f SECURITY.md`                                                         | 3 | Security policy published |
| branch-protect  | `gh api repos/{owner}/{repo}/branches/main/protection --jq '.required_status_checks.strict'` returns `true` | 5 | Branch protection enabled |
