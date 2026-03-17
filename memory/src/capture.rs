//! Parse muzzle changelog markdown into a structured session summary.
//!
//! The changelog is written by `hooks/src/changelog.rs` via PostToolUse.
//! Each mutation line has the form:
//!
//! ```text
//! `2026-03-16 14:00:00` **Edit**: `path/to/file`
//! `2026-03-16 14:00:00` **Write**: `path/to/file`
//! `2026-03-16 14:00:00` **NotebookEdit**: `path/to/notebook`
//! `2026-03-16 14:00:00` **COMMIT** `abc1234` on `main`
//! `2026-03-16 14:00:00` **PUSH** `origin` `feature/foo` (abc..def) -> `origin/feature/foo`
//! `2026-03-16 14:00:00` **PR Created**: Org/Repo - PR Title
//! `2026-03-16 14:00:00` **mcp__github__create_branch**
//! ```

use std::collections::HashSet;

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Parse a muzzle changelog and return a compact session summary.
///
/// Extracts three categories:
/// - **Files**: paths touched by Edit, Write, or NotebookEdit (deduplicated)
/// - **Git**: COMMIT and PUSH lines (as-is, without the timestamp prefix)
/// - **External**: MCP tool lines (as-is, without the timestamp prefix)
///
/// Returns an empty string if no mutation lines are found.
///
/// # Format
///
/// ```text
/// Files: path1, path2, path3
/// Git: `sha` on `branch`; **PUSH** `origin` `branch`
/// External: **PR Created**: Org/Repo - Title; **mcp__...**
/// ```
pub fn parse_changelog(changelog: &str) -> String {
    let mut files: Vec<String> = Vec::new();
    let mut files_seen: HashSet<String> = HashSet::new();
    let mut git_ops: Vec<String> = Vec::new();
    let mut external_ops: Vec<String> = Vec::new();

    for line in changelog.lines() {
        // Every mutation line starts with a backtick timestamp: `YYYY-...`
        let trimmed = line.trim();
        if !trimmed.starts_with('`') {
            continue;
        }

        // Strip the leading timestamp token: `<ts>` <rest>
        let rest = match strip_timestamp(trimmed) {
            Some(r) => r,
            None => continue,
        };

        // --- File mutations ---
        if let Some(path) = extract_file_path(rest) {
            if files_seen.insert(path.clone()) {
                files.push(path);
            }
            continue;
        }

        // --- Git operations (COMMIT / PUSH) ---
        if rest.contains("**COMMIT**") || rest.contains("**PUSH**") {
            git_ops.push(rest.to_string());
            continue;
        }

        // --- MCP / external operations ---
        // Generic MCP entries produced by format_entry for mcp__ tools.
        // Also catches named variants like **PR Created**, **Branch Created**,
        // **GitHub Issue**, **Issue Tracker** which come from mcp__ tools.
        if is_external_op(rest) {
            external_ops.push(rest.to_string());
        }
    }

    if files.is_empty() && git_ops.is_empty() && external_ops.is_empty() {
        return String::new();
    }

    let mut parts: Vec<String> = Vec::new();

    if !files.is_empty() {
        parts.push(format!("Files: {}", files.join(", ")));
    }
    if !git_ops.is_empty() {
        parts.push(format!("Git: {}", git_ops.join("; ")));
    }
    if !external_ops.is_empty() {
        parts.push(format!("External: {}", external_ops.join("; ")));
    }

    parts.join("\n")
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Strip the leading `` `<timestamp>` `` token and return the remainder, or
/// `None` if the line doesn't match the expected prefix pattern.
fn strip_timestamp(line: &str) -> Option<&str> {
    // Format: `YYYY-MM-DD HH:MM:SS` <rest>
    // Locate the closing backtick after the opening one.
    let after_open = line.strip_prefix('`')?;
    let close = after_open.find('`')?;
    let rest = after_open[close + 1..].trim();
    if rest.is_empty() {
        None
    } else {
        Some(rest)
    }
}

/// Extract the file path from an Edit, Write, or NotebookEdit line.
///
/// Patterns (after timestamp is stripped):
/// - `**Edit**: \`path\``
/// - `**Write**: \`path\``
/// - `**NotebookEdit**: \`path\``
fn extract_file_path(rest: &str) -> Option<String> {
    let marker = if rest.starts_with("**Edit**:") {
        "**Edit**:"
    } else if rest.starts_with("**Write**:") {
        "**Write**:"
    } else if rest.starts_with("**NotebookEdit**:") {
        "**NotebookEdit**:"
    } else {
        return None;
    };

    // After the marker, expect a space then a backtick-wrapped path.
    let after_marker = rest[marker.len()..].trim();
    extract_backtick_value(after_marker)
}

/// Extract the content of the first `` `value` `` in `s`.
fn extract_backtick_value(s: &str) -> Option<String> {
    let inner = s.strip_prefix('`')?;
    let end = inner.find('`')?;
    let value = &inner[..end];
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

/// Return `true` if the line (timestamp stripped) represents an MCP or
/// other external tool operation.
///
/// Matches:
/// - `**PR Created**:`         — mcp__github__create_pull_request
/// - `**Branch Created**:`     — mcp__github__create_branch
/// - `**GitHub Issue**:`       — mcp__github__create_issue
/// - `**Issue Tracker**:`      — mcp__atlassian__createJiraIssue
/// - `**mcp__…**`              — catch-all generic MCP entry
fn is_external_op(rest: &str) -> bool {
    rest.starts_with("**PR Created**")
        || rest.starts_with("**Branch Created**")
        || rest.starts_with("**GitHub Issue**")
        || rest.starts_with("**Issue Tracker**")
        || rest.contains("mcp__")
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // A representative changelog covering all mutation types.
    const FULL_CHANGELOG: &str = "\
## Session: 2026-03-16 14:00:00 (abc12345)
`2026-03-16 14:00:01` **Edit**: `hooks/src/session.rs`
`2026-03-16 14:00:02` **Write**: `memory/src/store.rs`
`2026-03-16 14:00:03` **COMMIT** `abc1234` on `feature/memory`
`2026-03-16 14:00:04` **PUSH** `origin` `feature/memory` (abc..def) -> `origin/feature/memory`
`2026-03-16 14:00:05` **PR Created**: Acme/muzzle - Add memory crate
`2026-03-16 14:00:06` **mcp__claude_ai_Atlassian__createJiraIssue**
";

    #[test]
    fn test_parse_changelog_entries() {
        let summary = parse_changelog(FULL_CHANGELOG);
        assert!(!summary.is_empty(), "summary should not be empty");

        // Files section
        assert!(
            summary.contains("Files:"),
            "expected Files section in: {summary}"
        );
        assert!(
            summary.contains("hooks/src/session.rs"),
            "expected session.rs in: {summary}"
        );
        assert!(
            summary.contains("memory/src/store.rs"),
            "expected store.rs in: {summary}"
        );

        // Git section
        assert!(
            summary.contains("Git:"),
            "expected Git section in: {summary}"
        );
        assert!(
            summary.contains("**COMMIT**"),
            "expected COMMIT entry in: {summary}"
        );
        assert!(
            summary.contains("**PUSH**"),
            "expected PUSH entry in: {summary}"
        );

        // External section
        assert!(
            summary.contains("External:"),
            "expected External section in: {summary}"
        );
        assert!(
            summary.contains("**PR Created**"),
            "expected PR Created entry in: {summary}"
        );
        assert!(
            summary.contains("mcp__claude_ai_Atlassian__createJiraIssue"),
            "expected Jira MCP entry in: {summary}"
        );
    }

    #[test]
    fn test_parse_empty_changelog() {
        assert_eq!(parse_changelog(""), "");
        assert_eq!(parse_changelog("   \n\n   "), "");
        // Header only, no mutation lines
        assert_eq!(
            parse_changelog("## Session: 2026-03-16 14:00:00 (abc12345)\n"),
            ""
        );
    }

    #[test]
    fn test_parse_deduplicates_files() {
        let changelog = "\
`2026-03-16 14:00:01` **Edit**: `memory/src/store.rs`
`2026-03-16 14:00:02` **Edit**: `memory/src/store.rs`
`2026-03-16 14:00:03` **Write**: `memory/src/store.rs`
`2026-03-16 14:00:04` **Edit**: `memory/src/lib.rs`
";
        let summary = parse_changelog(changelog);
        assert!(summary.contains("Files:"), "expected Files section");

        // store.rs must appear exactly once despite three edits
        let count = summary.matches("memory/src/store.rs").count();
        assert_eq!(
            count, 1,
            "store.rs should appear exactly once, got: {summary}"
        );

        // lib.rs must appear once
        assert!(
            summary.contains("memory/src/lib.rs"),
            "lib.rs missing from: {summary}"
        );
    }

    #[test]
    fn test_parse_skips_non_mutation_lines() {
        let changelog = "\
## Session: 2026-03-16 14:00:00 (abc12345)
This is a plain paragraph line.
  - A bullet point
### Semantic Summary
- **What**: Did stuff
`2026-03-16 14:00:01` **Edit**: `only/this/file.rs`
";
        let summary = parse_changelog(changelog);

        // Only the Edit line should contribute
        assert!(
            summary.contains("only/this/file.rs"),
            "expected edit path in: {summary}"
        );
        assert!(
            !summary.contains("plain paragraph"),
            "plain text should be ignored"
        );
        assert!(
            !summary.contains("bullet point"),
            "bullets should be ignored"
        );
        assert!(
            !summary.contains("Semantic Summary"),
            "headers should be ignored"
        );
    }

    // ---------------------------------------------------------------------------
    // Additional edge-case coverage
    // ---------------------------------------------------------------------------

    #[test]
    fn test_parse_notebookedit() {
        let changelog = "`2026-03-16 14:00:01` **NotebookEdit**: `notebooks/analysis.ipynb`\n";
        let summary = parse_changelog(changelog);
        assert!(
            summary.contains("notebooks/analysis.ipynb"),
            "expected notebook path in: {summary}"
        );
    }

    #[test]
    fn test_parse_git_only() {
        let changelog = "\
`2026-03-16 14:00:01` **COMMIT** `deadbeef` on `main`
`2026-03-16 14:00:02` **PUSH** `origin` `main`
";
        let summary = parse_changelog(changelog);
        assert!(!summary.contains("Files:"), "no files expected");
        assert!(summary.contains("Git:"), "expected Git section");
        assert!(!summary.contains("External:"), "no external expected");
    }

    #[test]
    fn test_parse_external_variants() {
        let changelog = "\
`2026-03-16 14:00:01` **Branch Created**: Acme/muzzle/feature/test
`2026-03-16 14:00:02` **GitHub Issue**: Acme/muzzle - Fix bug
`2026-03-16 14:00:03` **Issue Tracker**: PROJ - Memory module
";
        let summary = parse_changelog(changelog);
        assert!(summary.contains("External:"), "expected External section");
        assert!(
            summary.contains("**Branch Created**"),
            "Branch Created missing"
        );
        assert!(summary.contains("**GitHub Issue**"), "GitHub Issue missing");
        assert!(
            summary.contains("**Issue Tracker**"),
            "Issue Tracker missing"
        );
    }

    #[test]
    fn test_parse_generic_bash_not_captured() {
        // Generic Bash lines (neither COMMIT nor PUSH) should be skipped
        let changelog = "`2026-03-16 14:00:01` **Bash**: `make build`\n";
        let summary = parse_changelog(changelog);
        assert_eq!(
            summary, "",
            "generic Bash should produce no output, got: {summary}"
        );
    }

    #[test]
    fn test_output_format_structure() {
        let summary = parse_changelog(FULL_CHANGELOG);
        let lines: Vec<&str> = summary.lines().collect();
        // Files line first
        assert!(
            lines[0].starts_with("Files:"),
            "first line should be Files:"
        );
        // Git line second
        assert!(lines[1].starts_with("Git:"), "second line should be Git:");
        // External line third
        assert!(
            lines[2].starts_with("External:"),
            "third line should be External:"
        );
    }
}
