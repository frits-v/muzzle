# Changelog

All notable changes to muzzle will be documented in this file.

Format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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
