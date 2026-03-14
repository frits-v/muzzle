# CLAUDE.md — muzzle

Session isolation hooks for Claude Code. Rust implementation producing 5 binaries
that enforce workspace sandboxing, git safety, and worktree-based session isolation.

## Architecture

```
src/
  lib.rs              # Library root (re-exports all modules)
  config.rs           # Constants, path helpers (MUZZLE_WORKSPACE or $HOME/src)
  session.rs          # Session ID resolution via PPID walk + spec file I/O
  sandbox.rs          # Path sandboxing (7 rules + worktree enforcement)
  gitcheck.rs         # 8 git safety regex patterns + worktree enforcement
  output.rs           # JSON response formatting for PreToolUse
  changelog.rs        # Audit log formatting + read-only detection
  log.rs              # Structured JSON logging to stderr
  mcp.rs              # MCP tool routing (GitHub, Atlassian, Datadog, etc.)
  worktree/
    mod.rs            # Worktree creation, restore, ensure_for_repo
    git.rs            # Git command helpers (fetch, branch resolution)
    cleanup.rs        # Worktree removal, pruning, rollback
  bin/
    session_start.rs  # SessionStart hook (creates worktrees, changelog)
    permissions.rs    # PreToolUse hook (sandbox + git safety checks)
    changelog_bin.rs  # PostToolUse hook (audit log entries)
    session_end.rs    # SessionEnd hook (cleanup worktrees, gzip logs)
    ensure_worktree.rs # On-demand worktree creation binary
```

## Commands

```bash
make build            # Dev build (fast)
make test             # Run all unit tests
make release          # Optimized + stripped release build
make install          # Build release and copy binaries to bin/
make lint             # clippy with -D warnings
make fmt              # Check formatting
make fmt-fix          # Auto-fix formatting
make sizes            # Show release binary sizes
make test-one NAME=x  # Run single test by name
```

## Key Design Decisions

- **Three-layer sandbox**: Session resolution -> context-aware path checking -> git safety regex
- **H-4 purity**: PreToolUse hook (`permissions`) NEVER writes files. Uses `resolve_readonly()`
- **Lazy worktrees**: `WORKTREE_MISSING:<repo>` denials trigger `ensure-worktree` on-demand
- **Config persistence**: `.agents/`, `CLAUDE.md`, `.claude/` always write to main checkout, never worktrees
- **Panic -> deny**: All hooks catch panics and deny rather than fail open

## Commit Convention

Use [Conventional Commits](https://www.conventionalcommits.org/) for all commits
and PR titles.

```
<type>(<scope>): <description>
```

| Type       | When                                          |
|------------|-----------------------------------------------|
| `feat`     | New functionality or capability                |
| `fix`      | Bug fix                                        |
| `docs`     | Documentation only                             |
| `chore`    | Build, deps, config, tooling                   |
| `ci`       | CI/CD changes                                  |
| `test`     | Adding or updating tests only                  |
| `refactor` | Code change that neither fixes nor adds        |
| `perf`     | Performance improvement                        |
| `evolve`   | Autonomous improvement cycle ledger entries     |

Optional scopes: `sandbox`, `gitcheck`, `worktree`, `session`, `permissions`,
`changelog`, `mcp`, `log`, `bench`, `fuzz`.

**PR titles** must also follow this format. Squash-merge PRs inherit the PR title
as the merge commit message.

**Before pushing a feature branch**, always rebase on `origin/main`:
```bash
git fetch origin main
git rebase origin/main
# resolve any conflicts
git push origin <branch> --force-with-lease
```
Never submit a PR with merge conflicts.

Examples:
```
feat(sandbox): add dot-dot normalization for path traversal
fix(worktree): handle dirty worktree on session end
docs: rewrite README with product-grade presentation
chore: bump to v0.2.0 with cargo-release
ci: add binary size gate to CI workflow
test(gitcheck): add property-based tests for git safety
evolve: cycle 13 -- directive-4-proptest improved
```

## Lint Suppression Policy

**Lint rule exclusion comments require human approval.** Never add `#[allow(...)]`,
`// nolint`, `# shellcheck disable=...`, or any lint suppression annotation without
explicit user sign-off. If a lint rule fires, fix the underlying issue instead.

The only pre-approved suppression is `SC2016` in `check-claude-md.sh` (regex
patterns in single quotes are intentional).

## Shell Style

All shell scripts follow the [Google Shell Style Guide](https://google.github.io/styleguide/shellguide.html):

- `[[ ]]` over `[ ]` for conditionals
- `(( ))` for arithmetic
- `mapfile` for array population (requires bash >= 4.0 — add version guard)
- 2-space indent, case indent, binary operator at start of continuation line
- Declare and assign separately (`var=""; var="$(cmd)"` not `readonly var="$(cmd)"`)
- Lint: `shellcheck scripts/*.sh`
- Format: `shfmt -d -i 2 -ci -bn scripts/*.sh`
- Run both: `make lint-sh`

## Testing

153 tests (130 unit + 13 integration + 10 proptest) plus 4 fuzz targets.
Run with `make test` or `cargo test`.

Test patterns:
- Session tests use `SESSION_LOCK` mutex to avoid PPID marker conflicts
- MCP rate limit tests use unique session IDs to avoid cross-test interference
- Sandbox tests construct paths from `config::workspace()` for portability
- Property tests use proptest strategies (256 cases each by default)
- Fuzz targets require nightly: `cargo +nightly fuzz run <target>`

## Dependencies

5 crates: `serde`, `serde_json`, `regex`, `flate2`, `libc`. No async runtime,
no network deps, no proc macros.
