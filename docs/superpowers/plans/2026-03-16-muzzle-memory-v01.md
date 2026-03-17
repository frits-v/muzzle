# muzzle-memory v0.1 Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers-extended-cc:subagent-driven-development (if subagents available) or superpowers-extended-cc:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add persistent, cross-project, FTS5-searchable memory to muzzle via a new `memory` crate in a Cargo workspace, with auto-capture from changelogs, agent-driven save CLI, and SessionStart context injection.

**Architecture:** Convert muzzle from a single Cargo package to a workspace with two members: `hooks` (existing code) and `memory` (new SQLite+FTS5 crate). The memory binary integrates into the existing hook lifecycle — SessionEnd captures, SessionStart injects. Memory reads session state from env/files directly (same pattern as existing hooks — no shared crate needed for v0.1).

**Tech Stack:** Rust, rusqlite (bundled SQLite3 + FTS5), serde/serde_json, existing muzzle session/config patterns.

**Spec:** `.agents/brainstorm/2026-03-16-muzzle-memory.md`

---

## Chunk 1: Cargo Workspace Restructure

### Task 0: Convert to Cargo workspace

Move existing code into `hooks/` subdirectory and create a root workspace Cargo.toml. All existing binaries and tests must continue to work identically.

**Files:**
- Create: `hooks/Cargo.toml`
- Move: `src/` → `hooks/src/`
- Move: `tests/` → `hooks/tests/` (if exists)
- Modify: `Cargo.toml` → workspace root
- Create: `memory/Cargo.toml` (placeholder)
- Create: `memory/src/lib.rs` (placeholder)
- Create: `memory/src/main.rs` (placeholder)
- Modify: `Makefile`

- [ ] **Step 1: Create hooks/ directory and move source**

```bash
cd ~/src/muzzle/.worktrees/20bc4905
mkdir -p hooks
git mv src hooks/src
git mv tests hooks/tests 2>/dev/null || true
```

- [ ] **Step 2: Create hooks/Cargo.toml**

```toml
[package]
name = "muzzle-hooks"
version = "0.2.0"
edition = "2021"
description = "Session isolation and workspace sandboxing for AI coding agents"
license = "MIT"

[dependencies]
serde = { workspace = true }
serde_json = { workspace = true }
regex = "1"
flate2 = "1"
libc = "0.2"
ignore = "0.4"

[dev-dependencies]
proptest = "1"

[lib]
name = "muzzle"
path = "src/lib.rs"

[[bin]]
name = "session-start"
path = "src/bin/session_start.rs"

[[bin]]
name = "permissions"
path = "src/bin/permissions.rs"

[[bin]]
name = "changelog"
path = "src/bin/changelog_bin.rs"

[[bin]]
name = "session-end"
path = "src/bin/session_end.rs"

[[bin]]
name = "ensure-worktree"
path = "src/bin/ensure_worktree.rs"
```

- [ ] **Step 3: Replace root Cargo.toml with workspace**

```toml
[workspace]
members = ["hooks", "memory"]
resolver = "2"

[workspace.dependencies]
serde = { version = "1", features = ["derive"] }
serde_json = "1"

[profile.release]
opt-level = "s"
lto = true
strip = true
```

- [ ] **Step 4: Create memory crate placeholder**

`memory/Cargo.toml`:
```toml
[package]
name = "muzzle-memory"
version = "0.1.0"
edition = "2021"
description = "Persistent cross-project memory for AI coding agents"
license = "MIT"

[dependencies]
serde = { workspace = true }
serde_json = { workspace = true }
rusqlite = { version = "0.32", features = ["bundled", "bundled-full"] }

[[bin]]
name = "memory"
path = "src/main.rs"
```

`memory/src/lib.rs`:
```rust
//! muzzle-memory — persistent cross-project memory with FTS5 search.
```

`memory/src/main.rs`:
```rust
fn main() {
    eprintln!("muzzle-memory: not yet implemented");
    std::process::exit(1);
}
```

- [ ] **Step 5: Update Makefile for workspace**

Key changes to existing Makefile:
- Build/test commands become workspace-scoped
- Deploy list adds `memory` binary
- Dirty-check paths change to `hooks/` and `memory/`

```makefile
HOOKS_BINS = session-start permissions changelog session-end ensure-worktree
MEMORY_BINS = memory
ALL_BINS = $(HOOKS_BINS) $(MEMORY_BINS)

build:
	cargo build

test: test-unit

test-unit:
	cargo test --workspace

release:
	cargo build --release

install: release
	@mkdir -p bin
	@for b in $(ALL_BINS); do \
		if [ -f target/release/$$b ]; then \
			cp target/release/$$b bin/$$b; \
			echo "  installed bin/$$b"; \
		fi; \
	done

DEPLOY_TARGET ?= $(HOME)/.local/share/muzzle

deploy: release
	@if [ -n "$$(git status --porcelain -- hooks/ memory/ Cargo.toml Cargo.lock Makefile)" ]; then \
		echo "ERROR: Uncommitted changes in tracked build files."; \
		git status --short -- hooks/ memory/ Cargo.toml Cargo.lock Makefile; \
		exit 1; \
	fi
	@echo "Deploying to $(DEPLOY_TARGET)/"
	@mkdir -p $(DEPLOY_TARGET)/bin
	@for b in $(ALL_BINS); do \
		if [ -f target/release/$$b ]; then \
			cp target/release/$$b $(DEPLOY_TARGET)/bin/$$b; \
			echo "  bin/$$b"; \
		fi; \
	done
	@echo "Deployed to $(DEPLOY_TARGET)/"

lint:
	cargo clippy --workspace --all-targets -- -D warnings

fmt:
	cargo fmt --all -- --check

fmt-fix:
	cargo fmt --all

check:
	cargo check --workspace

clean:
	cargo clean

sizes: release
	@echo "Binary sizes:"
	@for b in $(ALL_BINS); do \
		if [ -f target/release/$$b ]; then \
			ls -lh target/release/$$b | awk '{print "  " $$NF ": " $$5}'; \
		fi; \
	done
```

- [ ] **Step 6: Build and test workspace**

```bash
cargo build --workspace
cargo test --workspace
```

Expected: All existing hook tests pass. Memory placeholder compiles.

- [ ] **Step 7: Verify release binaries**

```bash
cargo build --release
ls -la target/release/{session-start,permissions,changelog,session-end,ensure-worktree,memory}
```

Expected: All 6 binaries present.

- [ ] **Step 8: Commit**

```bash
git add -A
git commit -m "refactor: convert to Cargo workspace (hooks + memory crates)"
```

---

## Chunk 2: SQLite Store + FTS5 Schema

### Task 1: SQLite store with schema, CRUD, FTS5 search, topic upsert, soft delete

**Files:**
- Create: `memory/src/store.rs`
- Modify: `memory/src/lib.rs`

- [ ] **Step 1: Write failing tests in store.rs**

Add a `#[cfg(test)] mod tests` block with these tests:
- `test_open_creates_tables` — verify observations table exists after open
- `test_save_and_search_observation` — save one, FTS5 search finds it
- `test_search_filters_by_project` — two projects, filter returns only one
- `test_topic_key_upsert` — same topic_key updates instead of duplicating, revision_count increments
- `test_soft_delete` — soft-deleted observations don't appear in search
- `test_recent_context` — returns N most recent, ordered DESC
- `test_stats` — correct counts and project list

All tests use `Store::open(":memory:")` for in-memory SQLite.

- [ ] **Step 2: Run tests to verify they fail**

```bash
cargo test -p muzzle-memory
```

Expected: Compilation errors.

- [ ] **Step 3: Implement Store**

Types:
- `Store` — wraps `rusqlite::Connection`
- `NewObservation` — input params (derive Default)
- `Observation` — stored row (derive Serialize, Deserialize)
- `SearchResult` — Observation + rank score
- `Stats` — aggregate counts

Methods:
- `Store::open(path)` — open DB, run migrations, WAL mode
- `Store::register_session(id, project, directory)` — INSERT OR IGNORE
- `Store::save_observation(NewObservation)` — topic_key upsert logic, FTS5 sync via triggers
- `Store::search(query, project?, limit)` — FTS5 MATCH + JOIN + optional project filter
- `Store::recent_context(project, limit)` — ORDER BY created_at DESC
- `Store::soft_delete(id)` — SET deleted_at
- `Store::stats()` — COUNT queries

Schema (in migrate()):
- `sessions` table with id, project, directory, started_at, ended_at, summary
- `observations` table with all columns from brainstorm spec
- `observations_fts` virtual table (FTS5 content-sync with observations)
- AFTER INSERT/UPDATE/DELETE triggers to keep FTS in sync
- Indexes on project, session_id, topic_key composite, created_at

Timestamp: Use `SystemTime::UNIX_EPOCH` arithmetic (no chrono dep).

- [ ] **Step 4: Update lib.rs**

```rust
pub mod store;
```

- [ ] **Step 5: Run tests**

```bash
cargo test -p muzzle-memory
```

Expected: All 7 tests pass.

- [ ] **Step 6: Commit**

```bash
git add memory/src/store.rs memory/src/lib.rs
git commit -m "feat(memory): SQLite + FTS5 store with CRUD, search, topic upsert, soft delete"
```

---

## Chunk 3: Changelog Capture

### Task 2: Parse changelog markdown into observations

**Files:**
- Create: `memory/src/capture.rs`
- Modify: `memory/src/lib.rs`

- [ ] **Step 1: Write failing tests**

Tests:
- `test_parse_changelog_entries` — extracts files, git ops, MCP calls from real-format changelog
- `test_parse_empty_changelog` — returns empty string
- `test_parse_read_only_skipped` — lines with Read/Grep/Glob are not captured

- [ ] **Step 2: Run to verify failure**

```bash
cargo test -p muzzle-memory capture
```

- [ ] **Step 3: Implement parse_changelog()**

Parse muzzle changelog markdown format:
- Lines starting with `- **Edit**`, `- **Write**` → extract file path after `—`
- Lines with `git commit`, `git push` → extract as git ops
- Lines with `mcp__` → extract as external ops
- Deduplicate file paths
- Return formatted summary: `Files: ...\nGit: ...\nExternal: ...`

- [ ] **Step 4: Add to lib.rs**

```rust
pub mod capture;
```

- [ ] **Step 5: Run tests**

```bash
cargo test -p muzzle-memory capture
```

Expected: All pass.

- [ ] **Step 6: Commit**

```bash
git add memory/src/capture.rs memory/src/lib.rs
git commit -m "feat(memory): changelog parser for auto-capture at SessionEnd"
```

---

## Chunk 4: CLI + Context Injection

### Task 3: CLI binary with subcommands

**Files:**
- Modify: `memory/src/main.rs`

- [ ] **Step 1: Implement CLI**

Subcommands (simple positional arg parsing, no clap):
- `memory search <query> [-p project]` — FTS5 search, print results
- `memory save <title> <content> [--type TYPE] [--topic KEY] [--source SRC] [-p project]`
- `memory context [project]` — recent observations formatted as markdown
- `memory capture <changelog-path> <session-id> <project>` — parse changelog + save
- `memory stats` — show counts
- `memory inject [project]` — output JSON for Claude Code SessionStart hook

Project name derivation: `parent_dir/basename` of CWD (claude-mem pattern).
Session ID: from `MUZZLE_SESSION_ID` env var (set by session-start hook), fallback "manual".
DB path: `~/.muzzle/memory.db`.

The `inject` subcommand outputs Claude Code hook JSON:
```json
{"hookSpecificOutput":{"hookEventName":"SessionStart","additionalContext":"..."}}
```

- [ ] **Step 2: Build and smoke test**

```bash
cargo build -p muzzle-memory
./target/debug/memory stats
./target/debug/memory save "Test" "Content" --type learning
./target/debug/memory search test
./target/debug/memory context
```

- [ ] **Step 3: Commit**

```bash
git add memory/src/main.rs
git commit -m "feat(memory): CLI with search, save, capture, context, inject, stats"
```

### Task 4: Context injection module

**Files:**
- Create: `memory/src/inject.rs`
- Modify: `memory/src/lib.rs`

- [ ] **Step 1: Write test**

```rust
#[test]
fn test_format_context_empty() {
    assert_eq!(format_context(&[], "proj"), "");
}

#[test]
fn test_format_context_truncates_long_content() {
    // observation with 500-char content should be truncated in output
}
```

- [ ] **Step 2: Implement format_context()**

Takes `&[Observation]` + project name → markdown string.
Truncate individual observation content to 150 chars for token budget.
Cap at 10 observations.

- [ ] **Step 3: Add to lib.rs, run tests**

```bash
cargo test -p muzzle-memory inject
```

- [ ] **Step 4: Commit**

```bash
git add memory/src/inject.rs memory/src/lib.rs
git commit -m "feat(memory): context injection formatter for SessionStart"
```

---

## Chunk 5: Integration Tests + Deploy

### Task 5: Integration tests

**Files:**
- Create: `memory/tests/integration.rs`

- [ ] **Step 1: Write integration tests**

Tests:
- `test_full_lifecycle` — register session → capture changelog → agent save → search → context → stats
- `test_cross_project_search` — two projects, unscoped search finds both, scoped finds one

- [ ] **Step 2: Run all workspace tests**

```bash
cargo test --workspace
```

Expected: All pass (hooks + memory).

- [ ] **Step 3: Commit**

```bash
git add memory/tests/
git commit -m "test(memory): integration tests for lifecycle and cross-project search"
```

### Task 6: Deploy and hook wiring

- [ ] **Step 1: Build release and deploy**

```bash
cd ~/src/muzzle/.worktrees/20bc4905
make deploy
ls -la ~/.local/share/muzzle/bin/
```

Expected: 6 binaries including `memory`.

- [ ] **Step 2: Add SessionEnd capture hook**

In `~/.claude/settings.json`, add to SessionEnd array (BEFORE the session-end-maintenance hook):

```json
{
  "type": "command",
  "command": "/Users/fritsvlaanderen/.local/share/muzzle/bin/memory capture \"${CLAUDE_CHANGELOG_PATH:-}\" \"${CLAUDE_SESSION_ID:-unknown}\" \"$(basename $(dirname $(pwd)))/$(basename $(pwd))\"",
  "timeout": 10
}
```

Note: `CLAUDE_CHANGELOG_PATH` and `CLAUDE_SESSION_ID` may need to be derived differently depending on what Claude Code exposes to hooks. Test and adjust.

- [ ] **Step 3: Add SessionStart inject hook**

In `~/.claude/settings.json`, add to SessionStart array (after muzzle session-start):

```json
{
  "type": "command",
  "command": "/Users/fritsvlaanderen/.local/share/muzzle/bin/memory inject",
  "timeout": 5
}
```

- [ ] **Step 4: Test end-to-end**

Start a new Claude Code session, do some work, end it. Start another session and verify memory context is injected.

- [ ] **Step 5: Final commit**

```bash
git add -A
git commit -m "feat(memory): muzzle-memory v0.1 complete — FTS5 search, auto-capture, CLI, hooks"
```
