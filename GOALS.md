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

Current tests are example-based. Add proptest strategies that generate random
paths, tool contexts, and session states to verify sandbox invariants hold
under arbitrary input combinations.

**Steer:** increase

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

Current: 140 tests (127 unit + 13 integration). Do not regress.

**Steer:** increase

## Gates

| ID              | Check                                            | Weight | Description                       |
|-----------------|--------------------------------------------------|--------|-----------------------------------|
| cargo-build     | `cargo build`                                    | 8      | Release-ready Rust build          |
| cargo-test      | `cargo test`                                     | 8      | All unit tests pass               |
| cargo-clippy    | `cargo clippy --all-targets -- -D warnings`      | 5      | No clippy warnings                |
| cargo-fmt       | `cargo fmt -- --check`                           | 3      | Code formatted per rustfmt.toml   |
| integration     | `cargo build && cargo test --test integration`   | 5      | Integration tests pass            |
| five-binaries   | `cargo build --release && test -f target/release/session-start && test -f target/release/permissions && test -f target/release/changelog && test -f target/release/session-end && test -f target/release/ensure-worktree` | 5 | All 5 binaries produced |
| rustdoc         | `RUSTDOCFLAGS="-D warnings" cargo doc --no-deps`         | 3      | No rustdoc warnings             |
| binary-size     | `cargo build --release && test $(stat -f%z target/release/permissions) -lt 5242880` | 2 | Each binary stays under 5 MB |
| license-exists  | `test -f LICENSE`                                        | 1      | MIT license file present        |
