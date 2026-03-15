<p align="center">
  <img src="docs/logo.png" alt="muzzle logo" width="200">
</p>

<h1 align="center">muzzle</h1>

<p align="center">
  <strong>Session isolation for AI coding agents.</strong><br>
  Keep your repos safe when multiple Claude Code sessions run side by side.
</p>

<p align="center">
  <a href="https://github.com/frits-v/muzzle/actions/workflows/ci.yml"><img src="https://github.com/frits-v/muzzle/actions/workflows/ci.yml/badge.svg" alt="CI"></a>
  <a href="https://github.com/frits-v/muzzle/releases/latest"><img src="https://img.shields.io/github/v/tag/frits-v/muzzle?label=version&sort=semver" alt="Version"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-MIT-blue.svg" alt="MIT License"></a>
  <img src="https://img.shields.io/badge/rust-stable-orange.svg" alt="Rust">
  <img src="https://img.shields.io/badge/tests-158-brightgreen.svg" alt="Tests">
</p>

---

## The Problem

You ask Claude Code to refactor your auth module. Meanwhile, another session is fixing a bug in the same file. One overwrites the other. Your `git push --force` nukes a teammate's branch. A stray `rm -rf` targets `/usr/`. Fun times.

AI coding agents operate with broad filesystem and git access. Without guardrails:

- **Concurrent sessions clobber each other** — two agents editing the same files
- **Dangerous git ops slip through** — force pushes, pushes to main, tag deletions
- **Writes escape the workspace** — system paths, dotfiles, config directories
- **Crashes fail open** — a hook panic means no protection at all

## The Solution

Muzzle is a set of Rust binaries that plug into Claude Code's [hook system](https://docs.anthropic.com/en/docs/claude-code/hooks). Each session gets its own git worktree, writes are sandboxed to the workspace, and dangerous operations are blocked before they execute.

```
                   ┌─────────────────────┐
                   │   Claude Code        │
                   │   Session A          │
                   └──────────┬──────────┘
                              │
              ┌───────────────┼───────────────┐
              ▼               ▼               ▼
        SessionStart     PreToolUse      SessionEnd
              │               │               │
              ▼               ▼               ▼
     ┌────────────────┐ ┌──────────┐ ┌──────────────┐
     │ Create worktree│ │ Sandbox  │ │ Remove       │
     │ from origin/   │ │ check    │ │ worktrees    │
     │ default branch │ │ path +   │ │ gzip logs    │
     │                │ │ git ops  │ │ clean PIDs   │
     └────────────────┘ └──────────┘ └──────────────┘
              │               │
              ▼               ▼
     repo/.worktrees/    ALLOW / DENY / ASK
     <short-id>/
```

**3.4ms** mean latency per permission check. You won't notice it.

## Quick Start

### Prerequisites

- [Rust](https://rustup.rs/) (stable), or via mise: `mise use -g rust@latest`
- [Claude Code](https://claude.ai/code) with hooks support
- A workspace directory containing your git repos

### Install from Release (recommended)

Download the latest signed release from [GitHub Releases](https://github.com/frits-v/muzzle/releases):

```bash
# macOS ARM (Apple Silicon)
curl -sSLO https://github.com/frits-v/muzzle/releases/latest/download/muzzle-aarch64-apple-darwin.tar.gz
mkdir -p ~/.local/share/muzzle/bin
tar xzf muzzle-aarch64-apple-darwin.tar.gz -C ~/.local/share/muzzle/bin

# Verify signature (optional, requires cosign)
curl -sSLO https://github.com/frits-v/muzzle/releases/latest/download/muzzle-aarch64-apple-darwin.tar.gz.sigstore.json
cosign verify-blob muzzle-aarch64-apple-darwin.tar.gz \
  --bundle muzzle-aarch64-apple-darwin.tar.gz.sigstore.json \
  --certificate-oidc-issuer="https://token.actions.githubusercontent.com" \
  --certificate-identity-regexp="https://github.com/frits-v/muzzle/"
```

### Install from Source

```bash
git clone https://github.com/frits-v/muzzle.git
cd muzzle
make deploy
```

This builds optimized binaries (~1.4 MB each) and installs them to `~/.local/share/muzzle/bin/`. Custom path: `make deploy DEPLOY_TARGET=/your/path`.

### Configure

**1. Set your workspace** — the parent directory that holds your git repos:

```bash
mkdir -p ~/.config/muzzle
echo 'workspace = /path/to/your/repos' > ~/.config/muzzle/config
```

**2. Register hooks** — add to `~/.claude/settings.json`:

```jsonc
{
  "hooks": {
    "SessionStart": [{
      "matcher": "",
      "hooks": [{ "type": "command", "command": "~/.local/share/muzzle/bin/session-start", "timeout": 30 }]
    }],
    "PreToolUse": [{
      "matcher": "",
      "hooks": [{ "type": "command", "command": "~/.local/share/muzzle/bin/permissions", "timeout": 5 }]
    }],
    "PostToolUse": [{
      "matcher": "",
      "hooks": [{ "type": "command", "command": "~/.local/share/muzzle/bin/changelog", "timeout": 10 }]
    }],
    "SessionEnd": [{
      "matcher": "",
      "hooks": [{ "type": "command", "command": "~/.local/share/muzzle/bin/session-end", "timeout": 10 }]
    }]
  }
}
```

> If hooks fail to launch, use absolute paths instead of `~`.

**3. Verify** — start a Claude Code session inside your workspace:

```
Active worktrees for this session:
  my-repo: /path/to/my-repo/.worktrees/a1b2c3/ (branch: wt/a1b2c3)
```

That's it. Your session is now isolated.

## How It Works

### Worktree Isolation

Every session gets its own [git worktree](https://git-scm.com/docs/git-worktree) per repo. Session A and Session B edit the same repository but in completely separate working directories, each branched from `origin/<default-branch>`.

```
my-repo/
├── .git/                          # shared git database
├── .worktrees/
│   ├── a1b2c3/                    # Session A's workspace
│   │   ├── src/
│   │   └── ...
│   └── d4e5f6/                    # Session B's workspace
│       ├── src/
│       └── ...
├── src/                           # main checkout (protected)
└── ...
```

**Eager creation**: If you start Claude Code inside a git repo, a worktree is created immediately.

**Lazy creation**: If you start outside a repo (e.g., your workspace root), worktrees are created on-demand when you first touch a repo. The permissions hook denies the write with `WORKTREE_MISSING:<repo>`, Claude runs `ensure-worktree <repo>`, and retries.

Pre-specify repos with `CLAUDE_WORKTREES=repo-a:main,repo-b:develop`.

### Permission Enforcement

Every file write and Bash command passes through the `permissions` binary. Three layers of defense:

| Layer               | What it checks                                                    |
|---------------------|-------------------------------------------------------------------|
| **Path sandbox**    | System paths (`/etc`, `/usr`, `/System`) always blocked. Dangerous dotfiles prompt. Writes redirected to worktree paths. |
| **Git safety**      | 8 regex patterns: force push, push to main, delete tags, hard reset, `--no-verify`, `--follow-tags`, delete main/master, rebase onto main. |
| **Worktree guard**  | Writes to main checkout blocked when worktrees are active. `WORKTREE_MISSING` for repos without a worktree yet. |

Every layer returns `ALLOW`, `DENY`, or `ASK` (prompt the user). Panics always deny — hooks never fail open.

### Structured Logging

All 5 binaries emit JSON lines to stderr for machine-parseable log aggregation:

```json
{"ts":"2026-03-13T12:00:00Z","level":"WARN","bin":"permissions","msg":"worktree has uncommitted changes","detail":"/path/to/.worktrees/a1b2c3"}
```

### Session Lifecycle

| Event        | Binary            | What happens                                          |
|--------------|-------------------|-------------------------------------------------------|
| Session start | `session-start`  | Resolve session ID via PPID walk, create worktrees, start changelog, register PID marker |
| Tool use     | `permissions`     | Sandbox path + git safety checks, return ALLOW/DENY/ASK |
| After tool   | `changelog`       | Append mutation to session audit log (skips read-only ops) |
| Session end  | `session-end`     | Remove worktrees (warn on dirty), gzip logs, clean PID markers |
| On-demand    | `ensure-worktree` | Create worktree lazily for a repo not covered at startup |

## Safety Guarantees

### What's Blocked

| Pattern                     | Why                                          | What to do instead                           |
|-----------------------------|----------------------------------------------|----------------------------------------------|
| `git push --force`          | Overwrites remote history                    | `git push --force-with-lease origin <branch>` |
| `git push origin main`      | Bypasses PR review                           | Push a feature branch, open a PR             |
| `git push --follow-tags`    | Pushes ALL local tags (dangerous)            | `git push origin <specific-tag>`             |
| `git push --no-verify`      | Skips pre-push hooks                         | Fix the hook failures                        |
| `git tag -d v1.2.3`         | Deletes semver tags (breaks consumers)       | Release a new patch version                  |
| `git reset --hard origin/*` | Destroys local work                          | `git stash` or `git reset --soft`            |
| Writes to `/etc`, `/usr`    | System path modification                     | Stay within your workspace                   |
| Writes to main checkout     | Bypasses worktree isolation                  | Use the `.worktrees/<id>/` path              |

### What's Allowed

- `--force-with-lease` (safe force push — fails if remote changed)
- Writes to worktree paths
- Writes to `.claude-tmp/`, `.claude-changelog*`, `CLAUDE.md`
- Bash writes to `/tmp` (compilers, pip, etc.)
- All read operations (no permission check needed)

## Development

```bash
mise run ci           # Run all CI gates locally (preferred)
mise run lint         # All lints (Rust + shell + workflows)
mise run test:all     # All tests (unit + integration + claude_md)
mise run workflow-lint # actionlint + zizmor pedantic

make build            # Dev build (fast)
make test             # All tests (158 passing)
make release          # Optimized + LTO + stripped
make deploy           # Build and install to ~/.local/share/muzzle/
make lint             # clippy -D warnings
make fmt              # Check formatting
make sizes            # Show release binary sizes

# Advanced
make test-one NAME=test_sandbox_system_paths   # Single test
cargo +nightly fuzz run fuzz_git_safety        # Fuzz testing
bash scripts/bench-coldstart.sh                # Benchmark permissions latency
```

### Test Suite

| Category     | Count | Framework |
|--------------|------:|-----------|
| Unit         |   130 | `#[test]` |
| Integration  |    18 | `#[test]` |
| Property     |    10 | proptest  |
| Fuzz targets |     4 | cargo-fuzz |
| **Total**    | **158+4** |       |

### Architecture

```
src/
  lib.rs              # Library root — re-exports all modules
  config.rs           # Workspace resolution, path constants
  session.rs          # Session ID via PPID walk, spec file I/O (flock)
  sandbox.rs          # Path sandboxing (7 rules + dot-dot normalization)
  gitcheck.rs         # 8 git safety regex patterns + repo extraction
  output.rs           # JSON response formatting for PreToolUse
  changelog.rs        # Audit log formatting + read-only detection
  log.rs              # Structured JSON logging to stderr
  mcp.rs              # MCP tool routing (GitHub, Atlassian, Datadog)
  worktree/
    mod.rs            # Creation, restore, ensure_for_repo (with retry)
    git.rs            # Git command helpers (fetch, branch resolution)
    cleanup.rs        # Removal, pruning, rollback
  bin/
    session_start.rs  # SessionStart hook entry point
    permissions.rs    # PreToolUse hook entry point
    changelog_bin.rs  # PostToolUse hook entry point
    session_end.rs    # SessionEnd hook entry point
    ensure_worktree.rs # On-demand worktree creation
```

### Dependencies

Just 5 crates. No async runtime, no network dependencies, no proc macros:

| Crate       | Purpose                      |
|-------------|------------------------------|
| serde       | JSON deserialization (hooks)  |
| serde_json  | JSON serialization (output)   |
| regex       | Git safety pattern matching   |
| flate2      | Gzip compression (log archival) |
| libc        | POSIX flock (concurrent safety) |

### Binary Sizes (release, LTO + strip)

| Binary            | Size   |
|-------------------|--------|
| `session-start`   | 512 KB |
| `permissions`     | 1.4 MB |
| `changelog`       | 1.4 MB |
| `session-end`     | 444 KB |
| `ensure-worktree` | 396 KB |

## (Recommended) Defense-in-Depth Deny Rules

Fallback protection in case a hook fails to load. Add to `~/.claude/settings.json`:

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

## Worktree Instructions for Claude

Add to your project's `CLAUDE.md` so Claude knows how to work with worktrees:

```markdown
## Git Worktrees (Session Isolation)

Each session gets an isolated git worktree. Use worktree paths for ALL
file operations — never modify files in the main checkout directly.

Worktree paths are printed at session start:
  <repo>/.worktrees/<short-id>/

When the permissions hook denies a write with WORKTREE_MISSING:<repo>,
run `ensure-worktree <repo>` to create a worktree on-demand, then retry.
```

## Logo Prompt

The logo was generated with the following prompt (ChatGPT/DALL-E):

> Minimal flat vector logo on a transparent background. A friendly dog face
> viewed from the front, stylized with clean geometric lines, in electric
> teal (#1ABC9C). The dog wears a small muzzle (nose guard) made of fine
> wireframe lines. A subtle code bracket `{ }` is integrated into the muzzle
> design. Small padlock on the muzzle strap. Clean, modern, techy — suitable
> for a GitHub repo icon at 200x200px. No text, no background shapes, no
> gradients. PNG with alpha transparency.

## License

[MIT](LICENSE) -- Frits Vlaanderen
