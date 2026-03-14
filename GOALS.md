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

**Steer:** increase

### 3. Fuzz regex-heavy path parsing

Set up `cargo-fuzz` targets for the regex-heavy modules (gitcheck, sandbox,
config). Regex + user-controlled paths = classic attack surface. Goal: zero
panics/crashes on arbitrary input.

**Steer:** increase

### 4. Add property-based tests for sandbox decisions

Current tests are example-based. Add proptest strategies that generate random
paths, tool contexts, and session states to verify sandbox invariants hold
under arbitrary input combinations.

**Steer:** increase

### 5. Structured JSON logging from all binaries

Replace ad-hoc `eprintln!` calls with structured JSON output to stderr. This
enables log aggregation, filtering, and correlation across session lifecycle
events.

**Steer:** increase

### 6. Semantic versioning with cargo-release

Set up `cargo-release` for proper release workflow. Tag releases, generate
changelogs, publish binary artifacts. Currently stuck at `0.1.0` with no
release process.

**Steer:** increase

### 7. Improve cargo doc coverage

Ensure > 80% of public items have doc comments. Several public functions and
types lack documentation. Add `#![warn(missing_docs)]` to lib.rs.

**Steer:** increase

### 8. Benchmark permissions binary cold-start latency

Every tool call pays the permissions binary startup cost. Add a benchmark
(criterion or hyperfine) to track cold-start latency and catch regressions.
Target: < 10ms p99.

**Steer:** increase

### 9. Graceful degradation when workspace is missing

Handle edge case where workspace dir (`~/src/cn/`) doesn't exist. Currently
untested. Should fail with a clear error, not panic or produce confusing
messages.

**Steer:** increase

### 10. Maintain test coverage above 100 tests

Current: 116 tests (103 unit + 13 integration). Do not regress.

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
