# muzzle

Session isolation hooks for [Claude Code](https://claude.ai/code). Enforces
workspace sandboxing, git safety, and worktree-based session isolation so
concurrent Claude sessions never clobber each other.

## What It Does

Five Rust binaries that plug into Claude Code's hook system:

| Binary            | Hook Event   | Purpose                                   |
|-------------------|--------------|-------------------------------------------|
| `session-start`   | SessionStart | Create worktrees, changelog, register PID |
| `permissions`     | PreToolUse   | Sandbox writes, block unsafe git ops      |
| `changelog`       | PostToolUse  | Audit log (commits, pushes, file edits)   |
| `session-end`     | SessionEnd   | Remove worktrees, gzip logs, clean up     |
| `ensure-worktree` | (on-demand)  | Lazily create worktrees mid-session       |

## Setup

### Prerequisites

```bash
# Install Rust via mise (or any method that gives you cargo)
mise use -g rust@latest
```

### 1. Build and deploy

```bash
cd ~/src/muzzle
make deploy
```

This builds release binaries and installs them to `~/.local/share/muzzle/bin/`.
To deploy elsewhere: `make deploy DEPLOY_TARGET=/your/path`.

### 2. Configure workspace

Create `~/.config/muzzle/config`:

```bash
mkdir -p ~/.config/muzzle
cat > ~/.config/muzzle/config << 'EOF'
# Directory containing your git repos (each repo is a direct child).
# Also: MUZZLE_WORKSPACE env var, or defaults to $HOME/src.
workspace = /path/to/your/workspace
EOF
```

The workspace is the parent directory that holds your git repos. For example,
if your repos live at `~/src/myorg/repo-a/` and `~/src/myorg/repo-b/`, your
workspace is `~/src/myorg`.

### 3. Register hooks in Claude Code

Add the following to `~/.claude/settings.json` (merge into existing config):

```jsonc
{
  "hooks": {
    "SessionStart": [
      {
        "matcher": "",
        "hooks": [
          { "type": "command", "command": "~/.local/share/muzzle/bin/session-start", "timeout": 30 }
        ]
      }
    ],
    "PreToolUse": [
      {
        "matcher": "",
        "hooks": [
          { "type": "command", "command": "~/.local/share/muzzle/bin/permissions", "timeout": 5 }
        ]
      }
    ],
    "PostToolUse": [
      {
        "matcher": "",
        "hooks": [
          { "type": "command", "command": "~/.local/share/muzzle/bin/changelog", "timeout": 10 }
        ]
      }
    ],
    "SessionEnd": [
      {
        "matcher": "",
        "hooks": [
          { "type": "command", "command": "~/.local/share/muzzle/bin/session-end", "timeout": 10 }
        ]
      }
    ]
  }
}
```

> **Note**: `~` expansion depends on your shell. If hooks fail to launch, use
> the full absolute path (e.g., `/Users/yourname/.local/share/muzzle/bin/...`).

### 4. (Recommended) Add deny rules

Defense-in-depth fallback in case a hook fails to load:

```jsonc
{
  "permissions": {
    "deny": [
      "Bash(rm -rf /*)",
      "Bash(rm -rf ~*)",
      "Bash(rm -rf $HOME*)",
      "Bash(mkfs *)",
      "Bash(dd if=*)",
      "Bash(chmod -R 777 /*)",
      "Bash(> /dev/sd*)"
    ]
  }
}
```

### 5. Add worktree instructions to CLAUDE.md

Claude needs to know how to work with worktrees. Add something like this to
your project's `CLAUDE.md`:

```markdown
## Git Worktrees (Session Isolation)

Each session gets an isolated git worktree. Use worktree paths for ALL
file operations — never modify files in the main checkout directly.

Worktree paths are printed at session start:
  <repo>/.worktrees/<short-id>/

When the permissions hook denies a write with WORKTREE_MISSING:<repo>,
run `ensure-worktree <repo>` to create a worktree on-demand, then retry.
```

### Verify

Start a new Claude Code session inside your workspace. You should see:

```
Active worktrees for this session:
  <repo>: /path/to/repo/.worktrees/<short-id>/ (branch: wt/<short-id>)
```

If you start from the workspace root (not inside any repo), no worktrees are
created at startup — they'll be created lazily on first write.

## How Worktrees Work

```
Session Start
  │
  ├─ Inside a git repo?
  │   └─ YES → create worktree immediately (eager)
  │   └─ NO  → no worktrees yet
  │
  ▼
During Session
  │
  ├─ Claude writes to repo without worktree
  │   └─ permissions hook denies: WORKTREE_MISSING:<repo>
  │   └─ Claude runs: ensure-worktree <repo>
  │   └─ Retries write using .worktrees/<short-id>/ path (lazy)
  │
  ▼
Session End
  │
  └─ Worktrees removed, logs gzipped, PID markers cleaned
```

**Eager path**: `session-start` detects the git repo under PWD and creates a
worktree from `origin/<default-branch>`. Alternatively, set `CLAUDE_WORKTREES`
env var to pre-specify repos: `CLAUDE_WORKTREES=repo-a:main,repo-b:develop`.

**Lazy path**: `permissions` denies writes to repos without worktrees.
`ensure-worktree` creates one on-demand (idempotent — safe to call twice).

## Safety Guarantees

- **8 git safety patterns**: Force push, push to main, delete tags, hard reset, etc.
- **Path sandboxing**: System paths blocked, dangerous dotfiles require confirmation
- **Worktree isolation**: Each session gets its own worktree per repo
- **Panic = deny**: Hooks never fail open on crash
- **Config persistence**: `.agents/`, `CLAUDE.md` always go to main checkout

## Development

```bash
make build            # Dev build (fast)
make test             # Run all unit tests
make release          # Optimized + stripped release build
make install          # Build release and copy binaries to bin/
make deploy           # Build release and deploy to ~/.local/share/muzzle/
make lint             # clippy with -D warnings
make fmt              # Check formatting
make fmt-fix          # Auto-fix formatting
make sizes            # Show release binary sizes
make test-one NAME=x  # Run single test by name
```

## Architecture

```
src/
  lib.rs              # Library root (re-exports all modules)
  config.rs           # Constants, path helpers (workspace resolution)
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
    session_start.rs  # SessionStart hook
    permissions.rs    # PreToolUse hook
    changelog_bin.rs  # PostToolUse hook
    session_end.rs    # SessionEnd hook
    ensure_worktree.rs # On-demand worktree creation binary
```

## Dependencies

Only 4 crates: `serde`, `serde_json`, `regex`, `flate2`. No async runtime,
no network deps.

## License

Internal tooling. Not published as a crate.
