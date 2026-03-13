# muzzle

Session isolation hooks for [Claude Code](https://claude.ai/code). Enforces
workspace sandboxing, git safety, and worktree-based session isolation so
concurrent Claude sessions never clobber each other.

## What It Does

Five Rust binaries that plug into Claude Code's hook system:

| Binary             | Hook Event     | Purpose                                    |
|--------------------|----------------|--------------------------------------------|
| `session-start`    | SessionStart   | Create worktrees, changelog, register PID  |
| `permissions`      | PreToolUse     | Sandbox writes, block unsafe git ops       |
| `changelog`        | PostToolUse    | Audit log (commits, pushes, file edits)    |
| `session-end`      | SessionEnd     | Remove worktrees, gzip logs, clean markers |
| `ensure-worktree`  | (on-demand)    | Lazily create worktrees mid-session        |

## Safety Guarantees

- **8 git safety patterns**: Force push, push to main, delete tags, hard reset, etc.
- **Path sandboxing**: System paths blocked, dangerous dotfiles require confirmation
- **Worktree isolation**: Each session gets its own worktree per repo
- **Panic = deny**: Hooks never fail open on crash
- **Config persistence**: `.agents/`, `CLAUDE.md` always go to main checkout

## Prerequisites

```bash
# Install Rust via mise
mise use -g rust@latest
```

## Quick Start

```bash
# Build and install
make install

# Run tests
make test

# Check binary sizes
make sizes
```

## Configuration

Hooks are configured in Claude Code's `settings.json` or `.claude/settings.json`.
See [Claude Code hooks documentation](https://docs.anthropic.com/en/docs/claude-code/hooks)
for the hook registration format.

Example hook configuration:

```json
{
  "hooks": {
    "PreToolUse": [
      { "matcher": "", "hooks": ["/path/to/bin/permissions"] }
    ],
    "PostToolUse": [
      { "matcher": "", "hooks": ["/path/to/bin/changelog"] }
    ],
    "SessionStart": [
      { "matcher": "", "hooks": ["/path/to/bin/session-start"] }
    ],
    "SessionEnd": [
      { "matcher": "", "hooks": ["/path/to/bin/session-end"] }
    ]
  }
}
```

## Lazy Worktree Creation

When the permissions hook denies a write with `WORKTREE_MISSING:<repo>`, the
target repo doesn't have a worktree yet. Create one on-demand:

```bash
# Create worktree (prints path to stdout)
bin/ensure-worktree <repo-name>

# Retry original command with worktree path
git -C <workspace>/<repo>/.worktrees/<short-id>/ status
```

## Development

```bash
make build            # Dev build
make test             # Unit tests (103 tests)
make lint             # clippy
make fmt              # Format check
make fmt-fix          # Auto-format
```

## License

Internal tooling. Not published as a crate.
