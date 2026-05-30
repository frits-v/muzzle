# Sandbox Architecture

Muzzle uses defense-in-depth to enforce worktree isolation. Two independent
systems — Claude Code's native OS-level sandbox and muzzle's PreToolUse hooks —
work together to ensure all file writes go through worktrees during active
sessions.

## Why two layers?

Claude Code's sandbox uses macOS Seatbelt (`sandbox-exec`) or Linux bubblewrap
(`bwrap`) to enforce filesystem restrictions at the kernel level. This is
unbypassable — even renamed binaries, interpreter writes, and shell tricks
cannot escape it.

However, the OS-level sandbox **only covers the Bash tool**. The Edit, Write,
and Read tools execute in-process via Node.js `fs` APIs and are never wrapped
by Seatbelt or bubblewrap. Muzzle's PreToolUse hook covers these tools at the
application level.

| Tool       | OS-Level Sandbox (Bash) | Muzzle PreToolUse (Edit/Write) |
|------------|-------------------------|--------------------------------|
| Bash       | Seatbelt/bwrap          | Regex write-path detection     |
| Edit       | Not covered             | Path sandbox (FR-WE-1..5)      |
| Write      | Not covered             | Path sandbox (FR-WE-1..5)      |
| Read       | Not covered             | Read-only, no enforcement      |
| MCP tools  | Not covered             | Route + rate limit             |

## Recommended settings.json

Add this to `~/.claude/settings.json` or `.claude/settings.json`:

```json
{
  "sandbox": {
    "enabled": true,
    "allowUnsandboxedCommands": false,
    "filesystem": {
      "denyWrite": ["~/src"],
      "allowWrite": [
        ".worktrees",
        ".agents",
        ".claude",
        ".claude-tmp",
        "CLAUDE.md",
        "AGENTS.md",
        "GOALS.md",
        "GOALS.yaml"
      ]
    }
  }
}
```

### What each setting does

| Setting                      | Purpose                                                  |
|------------------------------|----------------------------------------------------------|
| `enabled`                    | Activates OS-level sandbox for all Bash commands         |
| `allowUnsandboxedCommands`   | `false` disables the `dangerouslyDisableSandbox` escape  |
| `denyWrite: ["~/src"]`       | Blocks Bash writes to the workspace root                 |
| `allowWrite: [".worktrees"]` | Permits writes to worktree directories                   |
| `allowWrite: [".agents"]`    | Permits writes to persistent artifacts (specs, handoffs) |
| `allowWrite: [".claude"]`    | Permits writes to Claude Code config                     |
| `allowWrite: [".claude-tmp"]`| Permits writes to session temp files                     |
| `allowWrite: ["CLAUDE.md"]`  | Permits writes to version-controlled project config      |

### Path conventions

- `~/` resolves to the home directory
- Paths without prefix are relative to the settings file's directory
- `denyWrite` takes precedence over `allowWrite` for the same path
- Arrays merge across settings scopes (managed + user + project + local)

## How the allowlists align

Muzzle's `sandbox.rs` already has an allowlist for persistent paths during
worktree sessions (FR-WE-3 through FR-WE-5). The `allowWrite` paths in the
sandbox config must match these:

| Write target              | CC Sandbox (`allowWrite`) | Muzzle (`sandbox.rs`)     |
|---------------------------|---------------------------|---------------------------|
| Source code (main)        | Blocked by `denyWrite`    | Blocked by WORKTREE_MISSING |
| `.worktrees/`             | `.worktrees`              | FR-WE-3: worktree paths  |
| `.agents/`                | `.agents`                 | FR-WE-4: config paths    |
| `.claude/`                | `.claude`                 | FR-WE-4: config paths    |
| `.claude-tmp/`            | `.claude-tmp`             | FR-WE-5: state directory  |
| `CLAUDE.md`, `AGENTS.md`  | Explicit allowWrite       | Committed repo files      |
| State dir (changelogs)    | Outside project, allowed  | FR-WE-5: state directory  |

## What happens without the sandbox

If the OS-level sandbox is not enabled, muzzle falls back to regex-based
write-path detection in `gitcheck::check_bash_write_paths()`. This catches
common bypass vectors:

- `sed -i`, `perl -i`, `ruby -i` (in-place editors)
- `cp`, `mv`, `install`, `rsync` (file copy/move)
- `dd of=`, `patch` (other write commands)
- Shell redirects (`>`, `>>`), `tee`

However, regex detection **can be bypassed** by:

- Renaming binaries: `cp $(which sed) .bin/zet && .bin/zet -i ...`
- Interpreter writes: `python3 -c "open('file','w').write('...')"`
- Nested shells: `bash -c 'sed -i s/foo/bar/ file.rs'`
- Eval: `eval "sed -i '' 's/foo/bar/' file.rs"`
- Command substitution: `$(cp /tmp/x src/lib.rs)` or `` `cp /tmp/x src/lib.rs` ``
- Indirection: `xargs -I{} sed -i '' 's/old/new/' {} <<< file.rs`
- Find exec: `find . -exec sed -i 's/old/new/' {} \;`
- Heredoc writes: `cat << 'EOF' > file.rs`
- Variable expansion: `cmd=sed; $cmd -i ...`
- Brace grouping: `{ cp /tmp/x src/lib.rs; }`
- Any command not in the pattern list

The OS-level sandbox catches all of these because it operates at the kernel
VFS layer, regardless of which binary performs the write.

## SessionStart detection

The `session_start` hook checks if the sandbox is enabled by reading
settings.json files on each session startup. If not enabled, it emits a
context message instructing the agent to ask the human operator to enable it.
It also warns if `allowUnsandboxedCommands` is `true` (the default), since
this provides an escape hatch that bypasses OS-level enforcement.

## Persistent artifact handling

Some files must persist in the main checkout across sessions even when
worktrees are active:

- **Specs, plans, handoffs**: `.agents/` directory
- **Project config**: `.claude/`, `CLAUDE.md`, `AGENTS.md`
- **Goal tracking**: `GOALS.md`, `GOALS.yaml`

Both muzzle's `sandbox.rs` and the CC sandbox `allowWrite` config permit
writes to these paths. If you write artifacts to a worktree path (e.g.
`.worktrees/abc123/docs/spec.md`), copy them to the main checkout too —
worktrees are ephemeral and get cleaned up between sessions.
