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

## Testing

103 unit tests across all modules. Run with `make test` or `cargo test`.

Test patterns:
- Session tests use `SESSION_LOCK` mutex to avoid PPID marker conflicts
- MCP rate limit tests use unique session IDs to avoid cross-test interference
- Sandbox tests construct paths from `config::workspace()` for portability

## Dependencies

Only 4 crates: `serde`, `serde_json`, `regex`, `flate2`. No async runtime, no network deps.
