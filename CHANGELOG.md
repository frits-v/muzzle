# Changelog

All notable changes to muzzle will be documented in this file.

Format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.2.1](https://github.com/frits-v/muzzle/compare/muzzle-v0.2.0...muzzle-v0.2.1) (2026-03-18)


### Features

* actionable block messages + allow CLAUDE.md in worktrees ([#18](https://github.com/frits-v/muzzle/issues/18)) ([71a07bb](https://github.com/frits-v/muzzle/commit/71a07bb4e97618fe3b20e2f77580dec068545567))
* **build:** add deploy target to Makefile ([7c0e986](https://github.com/frits-v/muzzle/commit/7c0e9868f9de3e9b11cb9e82bbfb08e50e6886a7))
* initial session isolation system ([9c78646](https://github.com/frits-v/muzzle/commit/9c78646a2ec887d9b964e39d99eb8fe8ed27a16a))
* multi-workspace + XDG state directory separation ([#22](https://github.com/frits-v/muzzle/issues/22)) ([78637bb](https://github.com/frits-v/muzzle/commit/78637bbc5b88f58546a3ed9aa527de18c6b584b4))
* OpenSSF Scorecard hardening + test coverage expansion ([#16](https://github.com/frits-v/muzzle/issues/16)) ([c804a25](https://github.com/frits-v/muzzle/commit/c804a2562f6ac8d6f1b789f69424242d068d1200))
* structured logging, semver, fuzz, proptest, benchmarks, product docs ([#4](https://github.com/frits-v/muzzle/issues/4)) ([e1033cc](https://github.com/frits-v/muzzle/commit/e1033cc154eb49de7ffeff1935467872fbe2bdc7))


### Bug Fixes

* add catch_unwind to ensure-worktree binary ([3db088b](https://github.com/frits-v/muzzle/commit/3db088b4aed78b3ea783d87eb403c708849604b1))
* add flock advisory lock to append_spec_entry ([f11f92d](https://github.com/frits-v/muzzle/commit/f11f92d0cb76f97696058cc5c6e7d1c806105f2d))
* **ci:** badges, scorecard SARIF, release-please manifest ([#11](https://github.com/frits-v/muzzle/issues/11)) ([31c55f4](https://github.com/frits-v/muzzle/commit/31c55f4aa29f5fc90e0f3a6df28ffc3316c39de9))
* **ci:** remove broken GITHUB_TOKEN close+reopen from release-please ([#17](https://github.com/frits-v/muzzle/issues/17)) ([c72063f](https://github.com/frits-v/muzzle/commit/c72063f17985d3e0b851406b4ff6d2deb3c8feb4))
* pipe stdout in git commands and emit JSON from SessionStart ([e141b88](https://github.com/frits-v/muzzle/commit/e141b88869a91ea7c56aa938e82bb246e40f7d30))
* resolve clippy, rustfmt, and flaky rate-limit test ([ea04c70](https://github.com/frits-v/muzzle/commit/ea04c702e6f0a61151474104c97cf24dc974e162))
* stdout contamination + setup docs ([813151c](https://github.com/frits-v/muzzle/commit/813151c86eaf687f4641d6bd7d77bc130eda4a82))

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
