# Architecture

Muzzle is a Cargo workspace producing 6 binaries across two crates. It
provides session isolation, workspace sandboxing, and persistent memory
for AI coding agents (currently targeting Claude Code hooks).

## Crate Map

```
muzzle (workspace)
├── hooks/    muzzle-hooks   5 binaries   Session isolation + sandbox enforcement
└── memory/   muzzle-memory  1 binary     Persistent cross-project memory (SQLite + FTS5)
```

The crates are **independent** — `muzzle-memory` does not depend on
`muzzle-hooks` and vice versa. They share only workspace-level dependency
versions (`serde`, `serde_json`).

## Layer Diagram

`muzzle-hooks` follows a three-layer architecture. Each layer depends only
on layers below it.

```
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

```
binaries ──→ core modules ──→ infrastructure
              │                    ▲
              └────────────────────┘
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

`muzzle-memory` is a flat single-layer crate:

| Module | Purpose |
|---|---|
| `store` | SQLite + FTS5 schema, CRUD, search, topic upsert |
| `capture` | Parse changelog markdown into session summaries |
| `inject` | Format memories as markdown for SessionStart injection |
| `main` | CLI: search, save, capture, context, inject, stats |

## Forbidden Dependencies

These dependency directions are explicitly prohibited:

1. **`muzzle-memory` must not depend on `muzzle-hooks`** — the crates are
   independent and must remain so.
2. **Infrastructure must not import core modules** — `config`, `output`,
   `changelog`, `log`, `mcp` must not import `sandbox`, `gitcheck`,
   `session`, or `worktree`.
3. **No async runtime** — the workspace is synchronous-only. No `tokio`,
   `async-std`, or equivalent.
4. **No network dependencies** — no HTTP clients, no API SDKs. All network
   interaction happens through Claude Code's tool system.
5. **No proc macros** — `serde` derive is via `serde_core` (no proc-macro
   crate). No other proc macros allowed.

## Cross-Cutting Concerns

| Concern | Location | Mechanism |
|---|---|---|
| Logging | `hooks/src/log.rs` | Structured JSON to stderr (`emit()`, `error()`, `warn()`) |
| Error handling | Each binary | `catch_unwind` → deny on panic (fail-closed) |
| Configuration | `hooks/src/config.rs` | Constants + path resolution (workspaces, XDG dirs) |
| Audit trail | `hooks/src/changelog.rs` | Markdown audit log per session |
| State storage | `~/.local/state/muzzle/` | XDG state directory for sessions, specs |
| Memory storage | `~/.muzzle/memory.db` | SQLite + FTS5 database |

## Key Invariants

- **Panic = deny**: all hook binaries catch panics and deny rather than fail open.
- **H-4 purity**: the `permissions` binary (PreToolUse) never writes files.
  It uses `resolve_readonly()`. Separation of read and write is structural.
- **Lazy worktrees**: `WORKTREE_MISSING:<repo>` denials trigger `ensure-worktree`
  on-demand rather than eagerly creating worktrees for all repos.
- **No shared mutable state**: each binary invocation is stateless. Session state
  is persisted to disk (spec files, changelogs) with file locking where needed.

## External Dependencies

5 runtime crates total (hooks: 5, memory: 1 additional):

| Crate | Used By | Purpose |
|---|---|---|
| `serde` | Both | Serialization (derive) |
| `serde_json` | Both | JSON parsing and formatting |
| `regex` | Hooks | Git safety pattern matching |
| `flate2` | Hooks | Gzip compression for session logs |
| `libc` | Hooks | PPID resolution for session identification |
| `rusqlite` | Memory | SQLite + FTS5 (bundled) |

Dev-only: `proptest` (property-based testing).
