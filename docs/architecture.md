# Architecture

Muzzle is a single-crate Cargo workspace, `muzzle-hooks`, producing 5
binaries. It provides session isolation and workspace sandboxing for AI
coding agents (currently targeting Claude Code hooks).

## Crate Map

```text
muzzle (workspace)
└── hooks/    muzzle-hooks   5 binaries   Session isolation + sandbox enforcement
```

## Layer Diagram

`muzzle-hooks` follows a three-layer architecture. Each layer depends only
on layers below it.

```text
┌─────────────────────────────────────────────────┐
│  Binaries (bin/)                                │
│  session-start, permissions, changelog,         │
│  session-end, ensure-worktree                   │
│  ─ entry points invoked by Claude Code hooks    │
├─────────────────────────────────────────────────┤
│  Core Modules                                   │
│  sandbox, gitcheck, session, worktree/          │
│  ─ business logic: path checks, git safety,     │
│    session resolution, worktree management      │
├─────────────────────────────────────────────────┤
│  Infrastructure                                 │
│  config, output, changelog, log, mcp            │
│  ─ constants, JSON formatting, audit logging,   │
│    structured logging, MCP tool routing         │
└─────────────────────────────────────────────────┘
```

### Dependency Direction

```text
binaries ──→ core modules ──→ infrastructure
   │                               ▲
   └───────────────────────────────┘
```

- Binaries may import from core modules and infrastructure.
- Core modules may import from infrastructure.
- Infrastructure modules must NOT import from core modules or binaries.

### Module Map

| Module | Layer | Purpose |
|---|---|---|
| `config` | Infra | Constants, path helpers (workspaces, XDG state_dir, bin_dir) |
| `output` | Infra | JSON response formatting for PreToolUse hook results |
| `changelog` | Infra | Audit log formatting, read-only tool detection |
| `log` | Infra | Structured JSON logging to stderr |
| `mcp` | Infra | MCP tool routing (GitHub, Atlassian, Datadog, etc.) |
| `session` | Core | Session ID resolution via PPID walk, spec file I/O |
| `sandbox` | Core | Path sandboxing (7 rules + worktree enforcement) |
| `gitcheck` | Core | 8 git safety regex patterns + worktree enforcement |
| `worktree/` | Core | Worktree creation, git helpers, cleanup, rollback |
| `bin/session_start` | Binary | SessionStart hook — creates worktrees, changelog |
| `bin/permissions` | Binary | PreToolUse hook — sandbox + git safety checks |
| `bin/changelog_bin` | Binary | PostToolUse hook — audit log entries |
| `bin/session_end` | Binary | SessionEnd hook — cleanup worktrees, gzip logs |
| `bin/ensure_worktree` | Binary | On-demand worktree creation |

## Forbidden Dependencies

These dependency directions are explicitly prohibited:

1. **Infrastructure must not import core modules** — `config`, `output`,
   `changelog`, `log`, `mcp` must not import `sandbox`, `gitcheck`,
   `session`, or `worktree`.
2. **No async runtime** — the workspace is synchronous-only. No `tokio`,
   `async-std`, or equivalent.
3. **No network dependencies** — no HTTP clients, no API SDKs. All network
   interaction happens through Claude Code's tool system.
4. **No proc macros** — `serde_derive` (pulled in by `serde`'s `derive`
   feature) is the only permitted proc macro. No others may be added.

## Cross-Cutting Concerns

| Concern | Location | Mechanism |
|---|---|---|
| Logging | `hooks/src/log.rs` | Structured JSON to stderr (`emit()`, `error()`, `warn()`) |
| Error handling | Each binary | `catch_unwind` → deny on panic (fail-closed) |
| Configuration | `hooks/src/config.rs` | Constants + path resolution (workspaces, XDG dirs) |
| Audit trail | `hooks/src/changelog.rs` | Markdown audit log per session |
| State storage | `~/.local/state/muzzle/` (default) | XDG state directory for sessions, specs (`XDG_STATE_HOME`) |

## Key Invariants

- **Panic = deny**: all hook binaries catch panics and deny rather than fail open.
- **H-4 purity**: the `permissions` binary (PreToolUse) never writes files.
  It uses `resolve_readonly()`. Separation of read and write is structural.
- **Lazy worktrees**: `WORKTREE_MISSING:<repo>` denials trigger `ensure-worktree`
  on-demand rather than eagerly creating worktrees for all repos.
- **No shared mutable state**: each binary invocation is stateless. Session state
  is persisted to disk (spec files, changelogs) with file locking where needed.

## External Dependencies

5 runtime crates:

| Crate | Purpose |
|---|---|
| `serde` | Serialization (derive) |
| `serde_json` | JSON parsing and formatting |
| `regex` | Git safety pattern matching |
| `flate2` | Gzip compression for session logs |
| `libc` | PPID resolution for session identification |

Dev-only: `proptest` (property-based testing).
