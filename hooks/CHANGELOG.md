# Changelog

All notable changes to muzzle will be documented in this file.

Format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.2.1](https://github.com/frits-v/muzzle/compare/v0.2.0...v0.2.1) (2026-07-02)


### Features

* add WHAT/FIX/REF remediation format to all denial messages ([#44](https://github.com/frits-v/muzzle/issues/44)) ([86578eb](https://github.com/frits-v/muzzle/commit/86578eb781b73e487382f788d73cb9d31ae0a491))
* **config:** resolve bin_dir dynamically instead of hardcoded path ([#33](https://github.com/frits-v/muzzle/issues/33)) ([aa6463e](https://github.com/frits-v/muzzle/commit/aa6463ec4e2033a3afbc33df672d97058fafcbea))
* **mcp,gitcheck:** block server-side commits that bypass signing ([#30](https://github.com/frits-v/muzzle/issues/30)) ([33179aa](https://github.com/frits-v/muzzle/commit/33179aa13de04cb4bbcb4d3f3ee3b7f4c2f6c8cd))
* **mcp:** auto-allow read-only MCP servers (FR-MR-8) ([#32](https://github.com/frits-v/muzzle/issues/32)) ([10de29b](https://github.com/frits-v/muzzle/commit/10de29bcdc163b27e69f98da4c79e59c13704e26)), closes [#50](https://github.com/frits-v/muzzle/issues/50)
* muzzle-memory v0.1 — persistent cross-project memory ([#19](https://github.com/frits-v/muzzle/issues/19)) ([7c6ddd0](https://github.com/frits-v/muzzle/commit/7c6ddd0620bcad3d9c507606cc75029db5ad7e4f))
* **sandbox:** block dangerouslyDisableSandbox in PreToolUse hook ([#39](https://github.com/frits-v/muzzle/issues/39)) ([1312071](https://github.com/frits-v/muzzle/commit/13120716ff56ee99ed187190acee280b4eac5613))
* **sandbox:** detect file-mutating Bash commands as write-path bypasses ([#24](https://github.com/frits-v/muzzle/issues/24)) ([9752a17](https://github.com/frits-v/muzzle/commit/9752a172a9660c496804755354c9285589bda262))


### Bug Fixes

* **gitcheck:** block bare mutating git commands when worktrees active ([#25](https://github.com/frits-v/muzzle/issues/25)) ([adc82fd](https://github.com/frits-v/muzzle/commit/adc82fd8671ffe1af18d8ab2683356f78b208294))
* **gitcheck:** tokenize Bash commands for write-path scanning ([#55](https://github.com/frits-v/muzzle/issues/55)) ([2c4707b](https://github.com/frits-v/muzzle/commit/2c4707b8afc1e2655a67b11af21da45f42856818))
* **sandbox:** break Guard A/Guard B redirect loop for gitignored repo files ([#70](https://github.com/frits-v/muzzle/issues/70)) ([e089252](https://github.com/frits-v/muzzle/commit/e089252fd6ee2d38c03ccdf74e1453097a70ee2e))
* **sandbox:** recognize CC-native agent worktrees as worktree paths ([#73](https://github.com/frits-v/muzzle/issues/73)) ([e37ace6](https://github.com/frits-v/muzzle/commit/e37ace613ae19c0ef422815dd4a28b33c8b9fadf))
* **sandbox:** redirect gitignored worktree writes by ignore status ([#63](https://github.com/frits-v/muzzle/issues/63)) ([e2b9a35](https://github.com/frits-v/muzzle/commit/e2b9a3552291b2b489ea2e7c056650a816f29980))

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
