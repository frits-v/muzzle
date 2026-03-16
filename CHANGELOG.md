# Changelog

All notable changes to muzzle will be documented in this file.

Format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## 0.1.0 (2026-03-16)


### Features

* **build:** add deploy target to Makefile ([f71538a](https://github.com/frits-v/muzzle/commit/f71538a36701cb8af64cc4fd76137e70ba22678e))
* initial session isolation system ([08f80cc](https://github.com/frits-v/muzzle/commit/08f80ccc8c9a6f6a41ec349706d2015af3421e1e))
* structured logging, semver, fuzz, proptest, benchmarks, product docs ([#4](https://github.com/frits-v/muzzle/issues/4)) ([5b510ca](https://github.com/frits-v/muzzle/commit/5b510ca71fcdd472e17149e8f63d7b74d13f82f8))


### Bug Fixes

* add catch_unwind to ensure-worktree binary ([7ba1c77](https://github.com/frits-v/muzzle/commit/7ba1c77785df9692ad4a9cbdc096dbf7b086dae6))
* add flock advisory lock to append_spec_entry ([d8954c2](https://github.com/frits-v/muzzle/commit/d8954c26ff29cb96a9158d88e234b4173f08459b))
* pipe stdout in git commands and emit JSON from SessionStart ([94e8f6f](https://github.com/frits-v/muzzle/commit/94e8f6f3f26f9b9b45f889631020f1464006d6b8))
* resolve clippy, rustfmt, and flaky rate-limit test ([bc536a0](https://github.com/frits-v/muzzle/commit/bc536a0ee12620202d9c9498e4d893708db39a7b))
* stdout contamination + setup docs ([dd5ef74](https://github.com/frits-v/muzzle/commit/dd5ef742b1b0b8f42ae268b165dc139810deac81))

## [Unreleased]

## [0.2.0] — 2026-03-13

### Added
- Structured JSON logging module (`src/log.rs`) replacing all ad-hoc `eprintln!` calls
- On-demand worktree creation via `ensure-worktree` binary
- `WORKTREE_MISSING:<repo>` denial pattern for lazy worktree creation
- `normalize_dot_segments()` defense-in-depth for path canonicalization
- `config::validate_workspace()` for graceful degradation when workspace is missing
- `session::append_spec_entry()` with file-locking for concurrent safety
- 22 sandbox edge-case tests (symlink traversal, Unicode, spaces, dot-dot escape)
- `#![warn(missing_docs)]` with full public API documentation
- GitHub Actions CI covering all 9 gates
- `release.toml` for cargo-release workflow

### Fixed
- Dot-dot traversal bypass in `resolve_path()` when path doesn't exist on disk
- `catch_unwind` safety wrapper in `ensure-worktree` binary

### Changed
- All 17 `eprintln!` calls across 4 binaries converted to structured JSON logging

## [0.1.0] — 2026-03-10

### Added
- Initial release: 5 hook binaries (session-start, session-end, permissions, changelog, ensure-worktree)
- Three-layer architecture: session resolution → context-aware sandbox → git safety
- Worktree isolation for concurrent AI agent sessions
- PPID-walk session resolution
- Path-based permission enforcement with regex git safety checks
