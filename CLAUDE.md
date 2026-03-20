# CLAUDE.md — muzzle

Session isolation hooks and persistent memory for Claude Code. Cargo workspace
with two crates: `muzzle-hooks` (producing 5 binaries for workspace sandboxing,
git safety, and worktree-based session isolation) and `muzzle-memory` (producing
1 binary for persistent cross-project memory with FTS5 search).

## Architecture

The workspace contains two crates:

- `hooks/` — `muzzle-hooks`: session isolation, sandbox enforcement, git safety
- `memory/` — `muzzle-memory`: persistent memory with SQLite + FTS5

`muzzle-hooks` source layout (`hooks/src/`):

```
src/
  lib.rs              # Library root (re-exports all modules)
  config.rs           # Constants, path helpers (workspaces + XDG state_dir)
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

`muzzle-memory` source layout (`memory/src/`):

```
lib.rs              # Library root
store.rs            # SQLite + FTS5 schema, CRUD, search, topic upsert
capture.rs          # Parse changelog markdown into session summaries
inject.rs           # Format memories as markdown for SessionStart injection
main.rs             # CLI: search, save, capture, context, inject, stats
```

## Commands

```bash
mise run ci           # Run all CI gates locally (preferred)
mise run lint         # All lints (Rust + shell + workflows)
mise run test:all     # All tests (unit + integration + claude_md)
mise run workflow-lint # actionlint + zizmor pedantic

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

## Quality Gate

Before committing any changes, run the full CI gate locally and ensure it passes:

```bash
mise run ci
```

This runs `cargo fmt --check`, `cargo clippy -D warnings`, `cargo check`, `rustdoc -D warnings`,
all tests (unit + integration + claude_md), `shellcheck`, `shfmt`, `actionlint`, and
`zizmor --pedantic`. All checks must be green before committing.

After pushing, poll PR checks and review comments in a single loop for up to 10 minutes:

- Wait for all CI checks to pass (`gh pr checks --watch --fail-fast`)
- Check for reviewer comments (`gh api repos/frits-v/muzzle/pulls/{number}/comments`)
- If CI fails: investigate the root cause, fix, push, and restart the loop
- For each review comment:
  - Actionable feedback (code change requested): implement the fix, push, and reply confirming what changed
  - Good suggestion already addressed or agreement: react with thumbs-up
  - Incorrect or inapplicable suggestion: react with thumbs-down and reply with a brief explanation why
- Done when CI is green AND no unresolved review comments remain

## Key Design Decisions

- **Three-layer sandbox**: Session resolution -> context-aware path checking -> git safety regex
- **H-4 purity**: PreToolUse hook (`permissions`) NEVER writes files. Uses `resolve_readonly()`
- **Lazy worktrees**: `WORKTREE_MISSING:<repo>` denials trigger `ensure-worktree` on-demand
- **Config persistence**: `.agents/`, `.claude/` redirect to main checkout when gitignored; if tracked by git (dir exists in worktree), allowed in-place
- **Committed repo files**: `CLAUDE.md`, `AGENTS.md` are version-controlled — allowed in worktrees
- **Panic -> deny**: All hooks catch panics and deny rather than fail open

## Memory Crate

Persistent cross-project memory with FTS5 full-text search. Storage: `~/.muzzle/memory.db`.

CLI: `memory search|save|capture|context|inject|stats`

Optional scopes for commit convention: add `memory`, `store`, `capture`, `inject` to the scopes list.

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
`changelog`, `mcp`, `log`, `bench`, `fuzz`, `memory`, `store`, `capture`, `inject`.

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
feat(memory): add FTS5 full-text search to memory store
evolve: cycle 13 -- directive-4-proptest improved
```

## Lint Suppressions

**NEVER add lint suppression comments without explicit human approval.** This includes
`#[allow(...)]`, `// nolint`, `# shellcheck disable`, `# noqa`, `# type: ignore`,
`# pyright: ignore`, `# zizmor: ignore`, or any equivalent across all linter/checker tools.

When a lint check fails:
1. Diagnose the root cause
2. Fix the underlying issue (refactor code, add proper type narrowing, use `cast()`, etc.)
3. If the only viable option is a suppression, explain why and **ask before adding it**

Suppressions hide real issues and accumulate as technical debt. The right fix is almost
always to address the code, not silence the tool.

There are currently no pre-approved suppressions.

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

243 hooks tests (211 unit + 5 doc + 13 integration + 14 proptest) plus 4 fuzz targets
and 25 memory tests. Run with `cargo test` or `cargo test -p muzzle-hooks`.

Test patterns:
- Session tests use `SESSION_LOCK` mutex to avoid PPID marker conflicts
- MCP rate limit tests use unique session IDs to avoid cross-test interference
- Sandbox tests construct paths from `config::workspace()` (first workspace) for portability
- State paths use `config::state_dir()` (XDG `~/.local/state/muzzle`)
- Property tests use proptest strategies (256 cases each by default)
- Fuzz targets require nightly: `cargo +nightly fuzz run <target>`
- Use fictional repo names in tests (e.g. `acme-api`, `web-app`), never real company or project names

## Releases

Automated via [release-please](https://github.com/googleapis/release-please):

1. Push conventional commits to `main`
2. Release-please opens a "Release PR" bumping `Cargo.toml` version + `CHANGELOG.md`
3. Merge the PR → creates git tag + GitHub Release
4. Release workflow builds macOS binaries (arm64 + x86_64), cosign-signs, uploads

Binaries: `muzzle-aarch64-apple-darwin.tar.gz`, `muzzle-x86_64-apple-darwin.tar.gz`
Verification: each tarball has a `.sigstore.json` bundle + `SHA256SUMS.txt`

```bash
# Verify a downloaded binary
cosign verify-blob muzzle-aarch64-apple-darwin.tar.gz \
  --bundle muzzle-aarch64-apple-darwin.tar.gz.sigstore.json \
  --certificate-identity="https://github.com/frits-v/muzzle/.github/workflows/release.yml@refs/tags/vX.Y.Z" \
  --certificate-oidc-issuer="https://token.actions.githubusercontent.com"
```

## PR Review Loop

After creating or pushing to a PR, start a background poll loop:
- Poll every 2 minutes for 15 minutes total
- Each poll: fetch all review comments from trusted actors (repo owner, collaborators,
  known bots like `greptile-apps[bot]`), identify unaddressed ones
- Summarize proposed changes to the user before committing — do not auto-push fixes
  from unknown or external commenters
- Address valid findings with code fixes, commit, push
- Dismiss false positives with thumbs-down reaction + inline reply explaining why
- Acknowledge good findings with thumbs-up reaction + inline reply
- Stop early if two consecutive polls find no new comments
- After the loop ends, report what was addressed and what was dismissed

## Supply Chain Policy

All GitHub Actions are **SHA-pinned** with version comments. No rolling tags (`@v4`).
Every workflow change must pass `actionlint` + `zizmor --pedantic` in CI.

## Tech Debt

**Fix tech debt when you see it. Never add new tech debt.**

### Fix what you touch

When working in a file, fix problems you encounter — even if they're unrelated to your task:
- CI failures, lint warnings, or type errors in files you modify
- Stale comments, dead imports, unused variables
- Review findings of any severity (minor, advisory, critical — fix them all)

### Don't create new debt

- Don't defer fixes to follow-ups — fix now unless genuinely blocked
- Don't categorize findings into "fix now" vs "follow-up" as a way to ship faster
- Don't leave known issues for the next person

### The only valid reasons to defer

- The fix requires changes in a different repository or PR
- It needs input from someone who isn't available right now
- It's blocked by an unresolved design question

In greenfield code especially, there is zero reason to defer anything — no backwards
compatibility, no released consumers, no excuse.

## Testing Strategy (AI-Written Code)

**When AI writes both implementation and tests, the tests share the implementation's
blind spots.** Unit tests with model-invented mock data verify the model's assumptions,
not reality. Every test suite needs at least one independent oracle — a source of truth
the implementation author did not create.

### Independent oracles (use these)

| Layer              | What it catches                            | When to use                              |
|--------------------|--------------------------------------------|------------------------------------------|
| **Property-based** | Edge cases the author didn't think of      | Parsing, validation, any pure function   |
| **Golden fixtures**| Drift from real-world data formats          | API response parsing, protocol handling  |
| **Integration**    | Plumbing bugs mocks can't surface          | API clients, CLI contracts, state machines|

**Property-based tests (proptest):** Define invariants, let the framework generate inputs.
Each property must be able to *fail* on a plausible bug. "Output is sorted" is tautological
when the code calls `sorted()`. "Excluded prefixes never leak through" catches a real
filter bypass.

**Golden fixtures:** Capture real command outputs and commit as test data. Determine
ground truth by reading the captured data by hand — never by running the implementation.
If a regex change breaks a golden test, that's a real signal.

**Integration tests:** Hit real binaries and file systems. Gate them appropriately and
make them **blocking in CI** (not informational). Clean up after themselves.

### Anti-patterns (don't do these)

- **Testing framework guarantees** — don't test that a `#[derive]` works (the compiler's
  job) or that `serde_json::to_string` produces JSON (serde's job)
- **Model-invented mock data** — if you wrote the mock response to match your parser,
  the test is a mirror, not an oracle
- **Tautological properties** — "every returned int is positive" when the regex only
  matches `\d+` proves nothing
- **Deferring test layers to follow-up** — property tests and golden fixtures are cheap;
  add them alongside unit tests, not later
- **Redundant unit tests** — if a golden fixture AND a property test already cover a
  function, a unit test for the same function is padding. Before adding a test, ask:
  "what mutation would this catch that existing tests miss?"
- **Fake property tests** — `proptest!` with no generated input variation is just a unit
  test wearing a macro. Every property test must generate varying inputs that exercise
  different code paths.

### Mutation testing (quality gate)

**cargo-mutants** is the deterministic oracle for test quality. It mutates source code
and checks if tests catch the mutations. Surviving mutants = gaps in your test suite.

```bash
# Run mutation testing
cargo mutants --package muzzle

# Show results
cat mutants.out/caught.txt
cat mutants.out/missed.txt
```

**Rules:**
- Run `cargo mutants` after writing tests, before claiming coverage is adequate
- Surviving mutants in critical logic (sandboxing, git safety, path checking) must be
  killed — add a test or justify why the mutation is equivalent
- Surviving mutants in logging/formatting are acceptable
- If a test can be deleted without any mutant surviving, the test was redundant — delete it

## Dependencies

5 crates: `serde`, `serde_json`, `regex`, `flate2`, `libc`. No async runtime,
no network deps, no proc macros.
