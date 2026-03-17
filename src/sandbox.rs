//! Path sandboxing for write operations.
//!
//! FR-PS-1 through FR-PS-7, FR-WE-1 through FR-WE-5.

use crate::config;
use crate::session::State;
use std::path::Path;

/// A path sandboxing decision.
#[derive(Debug, Clone, PartialEq)]
pub enum PathDecision {
    /// Path write is allowed.
    Allow,
    /// Path write is denied with a reason message.
    Deny(String),
    /// User should be prompted before allowing this write.
    Ask(String),
}

/// Whether the write originates from a direct file tool or a Bash command.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ToolContext {
    /// Write/Edit/NotebookEdit — Claude's own deliberate file write.
    FileTool,
    /// Bash — may be a system tool writing to /tmp (compiler, pip, etc.).
    Bash,
}

/// Evaluate a file path against sandboxing rules.
/// Requires session state for worktree enforcement.
pub fn check_path(raw_path: &str, sess: Option<&State>) -> PathDecision {
    check_path_with_context(raw_path, sess, ToolContext::FileTool)
}

/// Evaluate a file path with explicit tool context.
pub fn check_path_with_context(
    raw_path: &str,
    sess: Option<&State>,
    ctx: ToolContext,
) -> PathDecision {
    let home = config::home();
    let workspace = config::workspace();
    let home_str = home.to_string_lossy().to_string();

    // Expand ~ to home
    let path_str = if let Some(rest) = raw_path.strip_prefix("~/") {
        format!("{}/{}", home_str, rest)
    } else {
        raw_path.to_string()
    };

    // FR-PS-2: Safe device paths — always allowed
    match path_str.as_str() {
        "/dev/null" | "/dev/zero" | "/dev/stdout" | "/dev/stderr" | "/dev/stdin" => {
            return PathDecision::Allow;
        }
        _ => {}
    }
    if path_str.starts_with("/dev/fd/") {
        return PathDecision::Allow;
    }

    // FR-PS-1: System paths — check BEFORE resolving symlinks
    if is_system_path(&path_str) {
        return PathDecision::Deny(format!(
            "BLOCKED: Cannot write to system path: {}",
            raw_path
        ));
    }

    // Resolve to absolute (follow symlinks)
    let resolved = resolve_path(&path_str);

    // System paths check on resolved path (catches /private/etc, /private/var on macOS)
    if is_system_path(&resolved) || is_private_system_path(&resolved) {
        return PathDecision::Deny(format!(
            "BLOCKED: Cannot write to system path: {}",
            raw_path
        ));
    }

    // FR-PS-3: Temp directory
    if resolved.starts_with("/tmp/")
        || resolved.starts_with("/private/tmp/")
        || resolved == "/tmp"
        || resolved == "/private/tmp"
    {
        return match ctx {
            // Bash: system tools write to /tmp (compilers, pip, etc.) — allow
            ToolContext::Bash => PathDecision::Allow,
            // Write/Edit: Claude should use .claude-tmp/<session>/ instead
            ToolContext::FileTool => PathDecision::Ask(format!(
                "Write to {} — prefer .claude-tmp/<session-id>/ for session-scoped temp files",
                raw_path
            )),
        };
    }

    let ws_str = workspace.to_string_lossy().to_string();
    let claude_tmp_prefix = format!("{}/.claude-tmp/", ws_str);
    let global_claude_prefix = format!("{}/.claude/", home_str);

    // Worktree enforcement (FR-WE-1 through FR-WE-5)
    if let Some(sess) = sess {
        if sess.resolved && sess.has_session() {
            if sess.worktree_active {
                // Worktrees are active — strict sandbox mode

                // FR-WE-3: Allow writes to worktree paths
                if resolved.contains("/.worktrees/") {
                    // Redirect .agents/ and .claude/ writes to main checkout when
                    // gitignored. If the repo tracks these dirs, allow in worktree.
                    // Detection: parse <worktree>/.gitignore natively (no process spawn).
                    const WORKTREES_SEG: &str = "/.worktrees/";
                    if let Some(wt_idx) = resolved.find(WORKTREES_SEG) {
                        let after_wt = &resolved[wt_idx..]; // "/.worktrees/<id>/..."
                        if let Some(id_slash) = after_wt[WORKTREES_SEG.len()..].find('/') {
                            let after_id = &after_wt[WORKTREES_SEG.len() + id_slash..]; // "/..." after ID
                            if is_persistent_repo_config(after_id) {
                                let wt_root = &resolved[..wt_idx + WORKTREES_SEG.len() + id_slash];
                                // Check the actual file path against .gitignore.
                                // is_dir=false because we're checking a file write.
                                if is_path_gitignored(wt_root, &resolved, false) {
                                    let repo_prefix = &resolved[..wt_idx];
                                    return PathDecision::Deny(format!(
                                        "REDIRECT: config path must persist across sessions. \
                                         Write to: {}{}",
                                        repo_prefix, after_id
                                    ));
                                }
                                // Not ignored → tracked by git → allow in worktree
                                return PathDecision::Allow;
                            }
                        }
                    }
                    return PathDecision::Allow;
                }

                // FR-WE-4: Allow writes to config paths
                if is_config_path(&resolved, &ws_str) {
                    return PathDecision::Allow;
                }

                // FR-WE-5: Allow writes to session temp
                if resolved.starts_with(&claude_tmp_prefix) {
                    return PathDecision::Allow;
                }

                // Global Claude config
                if resolved.starts_with(&global_claude_prefix) {
                    return PathDecision::Allow;
                }

                // FR-WE-1: Block writes to main checkout — redirect or WORKTREE_MISSING
                if config::is_under(Path::new(&resolved), &workspace) {
                    let repo = extract_repo(&resolved, &ws_str);
                    let wt_dir = format!("{}/{}/.worktrees/{}", ws_str, repo, sess.short_id);
                    if !repo.is_empty() && !Path::new(&wt_dir).exists() {
                        return PathDecision::Deny(crate::worktree_missing_msg(&repo));
                    }
                    let rel = extract_rel_path(&resolved, &repo, &ws_str);
                    return PathDecision::Deny(format!(
                        "REDIRECT: Use worktree path instead: {}/{}/.worktrees/{}/{}",
                        ws_str, repo, sess.short_id, rel
                    ));
                }
            } else {
                // FR-WE-2: Session exists but NO worktrees (creation failed) — DENY all repo writes
                // AR-5: No legacy direct-edit mode
                if config::is_under(Path::new(&resolved), &workspace) {
                    // Allow writes to existing worktree paths from other sessions
                    if resolved.contains("/.worktrees/") {
                        return PathDecision::Allow;
                    }
                    // Allow config paths even when worktrees failed
                    if is_config_path(&resolved, &ws_str) {
                        return PathDecision::Allow;
                    }
                    // Allow .claude-tmp/
                    if resolved.starts_with(&claude_tmp_prefix) {
                        return PathDecision::Allow;
                    }
                    // Allow changelog/trace files at workspace root
                    if let Some(base) = Path::new(&resolved).file_name().and_then(|n| n.to_str()) {
                        if base.starts_with(".claude-changelog")
                            || base.starts_with(".claude-trace")
                            || base.starts_with(".claude-worktrees")
                        {
                            return PathDecision::Allow;
                        }
                    }
                    // FR-WE-2: Return WORKTREE_MISSING with repo name for lazy creation
                    let repo = extract_repo(&resolved, &ws_str);
                    if !repo.is_empty() {
                        return PathDecision::Deny(crate::worktree_missing_msg(&repo));
                    }
                    return PathDecision::Deny(
                        "BLOCKED: No worktree for this session. All repo writes are blocked to prevent editing the main checkout.".into(),
                    );
                }

                // Global Claude config
                if resolved.starts_with(&global_claude_prefix) {
                    return PathDecision::Allow;
                }
            }
        } else {
            // No session context — allow workspace and config
            if config::is_under(Path::new(&resolved), &workspace) {
                return PathDecision::Allow;
            }
            if resolved.starts_with(&global_claude_prefix) {
                return PathDecision::Allow;
            }
        }
    } else {
        // No session state at all — allow workspace and config
        if config::is_under(Path::new(&resolved), &workspace) {
            return PathDecision::Allow;
        }
        if resolved.starts_with(&global_claude_prefix) {
            return PathDecision::Allow;
        }
    }

    // FR-PS-5: Dangerous dotfiles — ASK
    if is_dangerous_dotfile(&resolved, &home_str) {
        return PathDecision::Ask(format!("Write to {} — outside normal workspace", raw_path));
    }

    // FR-PS-6: Other paths under HOME — allowed
    if config::is_under(Path::new(&resolved), &home) {
        return PathDecision::Allow;
    }

    // FR-PS-7: Outside HOME — ASK
    PathDecision::Ask(format!("Write to {} — outside normal workspace", raw_path))
}

/// Collapse `.` and `..` segments from a path string without touching the filesystem.
///
/// This provides defense-in-depth for paths that cannot be `canonicalize`d
/// (because they don't exist yet). Unlike `canonicalize`, this does NOT
/// resolve symlinks — it only normalizes logical traversal.
fn normalize_dot_segments(path: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for seg in path.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            other => parts.push(other),
        }
    }
    if path.starts_with('/') {
        format!("/{}", parts.join("/"))
    } else if parts.is_empty() {
        ".".to_string()
    } else {
        parts.join("/")
    }
}

/// Resolve a path string to absolute, following symlinks where possible.
///
/// Falls back to logical `..` normalization when `canonicalize` fails
/// (path doesn't exist on disk). This prevents `..` traversal attacks
/// against string-based path checks.
fn resolve_path(path: &str) -> String {
    // Try full symlink resolution
    if let Ok(resolved) = std::fs::canonicalize(path) {
        return resolved.to_string_lossy().to_string();
    }

    // Path might not exist yet — try resolving parent
    let p = Path::new(path);
    if let Some(parent) = p.parent() {
        if let Ok(rp) = std::fs::canonicalize(parent) {
            if let Some(filename) = p.file_name() {
                return rp.join(filename).to_string_lossy().to_string();
            }
        }
    }

    // Normalize .. segments even when path doesn't exist on disk
    let normalized = normalize_dot_segments(path);

    // Try making it absolute
    if !Path::new(&normalized).is_absolute() {
        if let Ok(abs) = std::env::current_dir() {
            return abs.join(&normalized).to_string_lossy().to_string();
        }
    }

    normalized
}

/// Check if a raw path resolves to a system path (public, for git -C checks).
pub fn is_system_path_resolved(raw_path: &str) -> bool {
    let resolved = resolve_path(raw_path);
    is_system_path(raw_path) || is_system_path(&resolved) || is_private_system_path(&resolved)
}

/// Check if a path is a system path.
fn is_system_path(path: &str) -> bool {
    let prefixes = [
        "/etc/",
        "/usr/",
        "/System/",
        "/Library/",
        "/bin/",
        "/sbin/",
        "/var/",
        "/opt/",
    ];
    for p in &prefixes {
        if path.starts_with(p) {
            return true;
        }
    }
    matches!(
        path,
        "/etc" | "/usr" | "/System" | "/Library" | "/bin" | "/sbin" | "/var" | "/opt"
    )
}

/// Check macOS /private/ versions of system paths.
fn is_private_system_path(path: &str) -> bool {
    path.starts_with("/private/etc/") || path.starts_with("/private/var/")
}

/// Check if a file path is gitignored at a given root.
///
/// Uses the `ignore` crate (from ripgrep) to parse gitignore files with
/// full gitignore semantics: globs, negation, directory-only patterns,
/// anchoring, `**/` prefixes, etc.
///
/// Checks three sources (matching git's own precedence):
/// 1. `<root>/.gitignore` — repo-local patterns
/// 2. `<root>/.git/info/exclude` — repo-local unshared patterns
/// 3. Global gitignore via `core.excludesFile` (`build_global()`)
///
/// Returns `true` if ignored, `false` if not ignored or no gitignore files exist.
fn is_path_gitignored(root: &str, path: &str, is_dir: bool) -> bool {
    use ignore::gitignore::GitignoreBuilder;

    let mut builder = GitignoreBuilder::new(root);

    // Add repo-local sources, skipping NotFound (file may not exist).
    // Non-NotFound errors (permission denied, parse errors) → bail conservatively.
    for source in &[
        format!("{}/.gitignore", root),
        format!("{}/.git/info/exclude", root),
    ] {
        if let Some(err) = builder.add(source) {
            if err
                .io_error()
                .is_some_and(|e| e.kind() != std::io::ErrorKind::NotFound)
            {
                return false;
            }
        }
    }

    // Check repo-local patterns first.
    if let Ok(gi) = builder.build() {
        if gi.matched_path_or_any_parents(path, is_dir).is_ignore() {
            return true;
        }
    }

    // Check global gitignore (reads core.excludesFile from git config).
    let (global_gi, _) = GitignoreBuilder::new(root).build_global();
    global_gi
        .matched_path_or_any_parents(path, is_dir)
        .is_ignore()
}

/// Check if a subpath (relative to a repo root) is a persistent config path.
///
/// Used by both `is_config_path` (FR-WE-4) and the FR-WE-3 worktree redirect
/// to ensure consistent recognition. Adding a new persistent path type here
/// automatically covers both direct writes AND worktree-intercepted writes.
///
/// `subpath` starts with "/" — e.g. "/.agents/foo.md", "/CLAUDE.md".
/// Trailing slashes in `starts_with` prevent false positives (e.g. `.agentsfoo`).
fn is_persistent_repo_config(subpath: &str) -> bool {
    // .agents/ and .claude/ are often gitignored local config. The FR-WE-3 redirect
    // path uses `is_path_gitignored()` (backed by the `ignore` crate) to distinguish:
    // ignored paths redirect to the main checkout, tracked paths are allowed in worktrees.
    subpath.starts_with("/.agents/")
        || subpath == "/.agents"
        || subpath.starts_with("/.claude/")
        || subpath == "/.claude"
}

/// Check if a subpath is a committed repo file that should be editable in worktrees.
///
/// Unlike `.agents/` and `.claude/` (gitignored local config), `CLAUDE.md` and
/// `AGENTS.md` are committed files. They should be editable in worktrees and on
/// main checkouts — not redirected.
fn is_committed_repo_file(subpath: &str) -> bool {
    subpath == "/CLAUDE.md" || subpath == "/AGENTS.md"
}

/// Check if a path is a project config or committed repo file (FR-WE-4).
///
/// Matches both workspace-level paths (e.g. `<ws>/.agents/`, `<ws>/CLAUDE.md`)
/// and per-repo paths (e.g. `<ws>/acme-api/.agents/`, `<ws>/web-app/CLAUDE.md`).
///
/// Two categories:
/// - **Persistent config** (`.agents/`, `.claude/`): gitignored, redirect to main checkout
/// - **Committed files** (`CLAUDE.md`, `AGENTS.md`): version-controlled, allowed in worktrees
fn is_config_path(path: &str, ws: &str) -> bool {
    let claude_prefix = format!("{}/.claude/", ws);
    let agents_prefix = format!("{}/.agents/", ws);
    let claude_md = format!("{}/CLAUDE.md", ws);
    let agents_md = format!("{}/AGENTS.md", ws);

    // Workspace-level config
    if path.starts_with(&claude_prefix)
        || path.starts_with(&agents_prefix)
        || path == claude_md
        || path == agents_md
    {
        return true;
    }

    // Per-repo config paths: <ws>/<repo>/.agents/, <ws>/<repo>/CLAUDE.md, etc.
    // Note: trailing slashes in starts_with() prevent false positives like .agentsfoo/
    let ws_prefix = format!("{}/", ws);
    if let Some(rest) = path.strip_prefix(&ws_prefix) {
        // rest = "acme-api/.agents/foo.md" or "web-app/CLAUDE.md"
        // First segment is the repo name; after_repo is everything after it
        if let Some(slash_idx) = rest.find('/') {
            let after_repo = &rest[slash_idx..];
            if is_persistent_repo_config(after_repo) || is_committed_repo_file(after_repo) {
                return true;
            }
        }
    }

    false
}

/// Check if a path is a dangerous dotfile (FR-PS-5).
fn is_dangerous_dotfile(path: &str, home: &str) -> bool {
    let dangerous_files = [
        format!("{}/.bashrc", home),
        format!("{}/.bash_profile", home),
        format!("{}/.zshrc", home),
        format!("{}/.zprofile", home),
        format!("{}/.gitconfig", home),
    ];
    for d in &dangerous_files {
        if path == d {
            return true;
        }
    }

    let dangerous_dirs = [format!("{}/.ssh/", home), format!("{}/.aws/", home)];
    for d in &dangerous_dirs {
        if path.starts_with(d) {
            return true;
        }
    }
    false
}

/// Extract the repo name from a workspace path.
fn extract_repo(path: &str, ws: &str) -> String {
    let prefix = format!("{}/", ws);
    if let Some(rest) = path.strip_prefix(&prefix) {
        if let Some(idx) = rest.find('/') {
            return rest[..idx].to_string();
        }
        return rest.to_string();
    }
    String::new()
}

/// Extract the relative path within a repo.
fn extract_rel_path(path: &str, repo: &str, ws: &str) -> String {
    let prefix = format!("{}/{}/", ws, repo);
    if let Some(rest) = path.strip_prefix(&prefix) {
        return rest.to_string();
    }
    Path::new(path)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config;
    use crate::ENV_LOCK;
    use std::path::PathBuf;

    fn sess_with_worktrees() -> State {
        State {
            id: "abc12345-test".into(),
            short_id: "abc12345".into(),
            tmp_dir: config::session_tmp_dir("abc12345-test"),
            spec_file: config::spec_file_path("abc12345-test"),
            changelog_path: config::changelog_path("abc12345-test"),
            worktree_active: true,
            resolved: true,
        }
    }

    fn sess_no_worktrees() -> State {
        State {
            id: "abc12345-test".into(),
            short_id: "abc12345".into(),
            tmp_dir: config::session_tmp_dir("abc12345-test"),
            spec_file: config::spec_file_path("abc12345-test"),
            changelog_path: config::changelog_path("abc12345-test"),
            worktree_active: false,
            resolved: true,
        }
    }

    fn no_session() -> State {
        State {
            id: String::new(),
            short_id: String::new(),
            tmp_dir: PathBuf::new(),
            spec_file: PathBuf::new(),
            changelog_path: PathBuf::new(),
            worktree_active: false,
            resolved: true,
        }
    }

    #[test]
    fn test_system_path_deny() {
        let paths = [
            "/etc/hosts",
            "/usr/local/bin/foo",
            "/System/Library/test",
            "/Library/test",
            "/bin/sh",
            "/sbin/mount",
            "/var/log/syslog",
            "/opt/homebrew/bin/foo",
        ];
        let sess = no_session();
        for p in &paths {
            let result = check_path(p, Some(&sess));
            assert!(
                matches!(result, PathDecision::Deny(_)),
                "expected DENY for system path {:?}, got {:?}",
                p,
                result
            );
        }
    }

    #[test]
    fn test_device_path_allow() {
        let paths = [
            "/dev/null",
            "/dev/zero",
            "/dev/stdout",
            "/dev/stderr",
            "/dev/stdin",
            "/dev/fd/3",
        ];
        let sess = no_session();
        for p in &paths {
            let result = check_path(p, Some(&sess));
            assert!(
                matches!(result, PathDecision::Allow),
                "expected ALLOW for device path {:?}, got {:?}",
                p,
                result
            );
        }
    }

    #[test]
    fn test_temp_path_bash_allow() {
        let paths = ["/tmp/foo.txt", "/tmp/some/deep/path", "/private/tmp/bar"];
        let sess = no_session();
        for p in &paths {
            let result = check_path_with_context(p, Some(&sess), ToolContext::Bash);
            assert!(
                matches!(result, PathDecision::Allow),
                "expected ALLOW for temp path via Bash {:?}, got {:?}",
                p,
                result
            );
        }
    }

    #[test]
    fn test_temp_path_file_tool_ask() {
        let paths = ["/tmp/foo.txt", "/tmp/some/deep/path", "/private/tmp/bar"];
        let sess = no_session();
        for p in &paths {
            let result = check_path_with_context(p, Some(&sess), ToolContext::FileTool);
            assert!(
                matches!(result, PathDecision::Ask(_)),
                "expected ASK for temp path via Write/Edit {:?}, got {:?}",
                p,
                result
            );
        }
    }

    #[test]
    fn test_worktree_path_allow() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let sess = sess_with_worktrees();
        let ws = config::workspace();
        let ws_str = ws.to_string_lossy();
        let paths = [
            format!("{}/web-app/.worktrees/abc12345/some/file.py", ws_str),
            format!("{}/api-server/.worktrees/abc12345/test.py", ws_str),
        ];
        for p in &paths {
            let result = check_path(p, Some(&sess));
            assert!(
                matches!(result, PathDecision::Allow),
                "expected ALLOW for worktree path {:?}, got {:?}",
                p,
                result
            );
        }
    }

    #[test]
    fn test_main_checkout_deny() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let sess = sess_with_worktrees();
        let ws = config::workspace();
        let ws_str = ws.to_string_lossy();
        let paths = [
            format!("{}/web-app/app/main.py", ws_str),
            format!("{}/api-server/src/test.py", ws_str),
        ];
        for p in &paths {
            let result = check_path(p, Some(&sess));
            assert!(
                matches!(result, PathDecision::Deny(_)),
                "expected DENY for main checkout path {:?}, got {:?}",
                p,
                result
            );
        }
    }

    #[test]
    fn test_no_worktree_session_deny() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let sess = sess_no_worktrees();
        let ws = config::workspace();
        let p = format!("{}/web-app/app/main.py", ws.display());
        let result = check_path(&p, Some(&sess));
        assert!(
            matches!(result, PathDecision::Deny(_)),
            "expected DENY when worktrees failed, got {:?}",
            result
        );
    }

    #[test]
    fn test_no_worktree_session_allows_existing_worktree_paths() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let sess = sess_no_worktrees();
        let ws = config::workspace();
        let ws_str = ws.to_string_lossy();
        // Worktree from a previous/other session should still be writable
        let paths = [
            format!("{}/.github/.worktrees/sentinel/.qlty/qlty.toml", ws_str),
            format!("{}/web-app/.worktrees/other-session/app/main.py", ws_str),
        ];
        for p in &paths {
            let result = check_path(p, Some(&sess));
            assert!(
                matches!(result, PathDecision::Allow),
                "expected ALLOW for existing worktree path {:?} even with failed worktrees, got {:?}",
                p,
                result
            );
        }
    }

    #[test]
    fn test_config_paths_always_allowed() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let sess = sess_with_worktrees();
        let ws = config::workspace();
        let ws_str = ws.to_string_lossy();
        let paths = [
            format!("{}/CLAUDE.md", ws_str),
            format!("{}/AGENTS.md", ws_str),
            format!("{}/.claude/hooks-v2/test.go", ws_str),
            format!("{}/.agents/domains/payments.md", ws_str),
        ];
        for p in &paths {
            let result = check_path(p, Some(&sess));
            assert!(
                matches!(result, PathDecision::Allow),
                "expected ALLOW for config path {:?}, got {:?}",
                p,
                result
            );
        }
    }

    #[test]
    fn test_session_temp_always_allowed() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let sess = sess_with_worktrees();
        let ws = config::workspace();
        let p = format!("{}/.claude-tmp/abc12345-test/output.txt", ws.display());
        let result = check_path(&p, Some(&sess));
        assert!(
            matches!(result, PathDecision::Allow),
            "expected ALLOW for session temp, got {:?}",
            result
        );
    }

    #[test]
    fn test_dangerous_dotfiles_ask() {
        let home = config::home();
        let home_str = home.to_string_lossy();
        let paths = [
            format!("{}/.bashrc", home_str),
            format!("{}/.zshrc", home_str),
            format!("{}/.ssh/config", home_str),
            format!("{}/.aws/credentials", home_str),
            format!("{}/.gitconfig", home_str),
        ];
        let sess = no_session();
        for p in &paths {
            let result = check_path(p, Some(&sess));
            assert!(
                matches!(result, PathDecision::Ask(_)),
                "expected ASK for dangerous dotfile {:?}, got {:?}",
                p,
                result
            );
        }
    }

    #[test]
    fn test_home_paths_allow() {
        let home = config::home();
        let p = format!("{}/Documents/test.txt", home.display());
        let sess = no_session();
        let result = check_path(&p, Some(&sess));
        assert!(
            matches!(result, PathDecision::Allow),
            "expected ALLOW for home path, got {:?}",
            result
        );
    }

    #[test]
    fn test_outside_home_ask() {
        let sess = no_session();
        let result = check_path("/some/random/path", Some(&sess));
        assert!(
            matches!(result, PathDecision::Ask(_)),
            "expected ASK for path outside HOME, got {:?}",
            result
        );
    }

    #[test]
    fn test_tilde_expansion() {
        let sess = no_session();
        let result = check_path("~/.ssh/config", Some(&sess));
        assert!(
            matches!(result, PathDecision::Ask(_)),
            "expected ASK for ~/.ssh/config, got {:?}",
            result
        );
    }

    #[test]
    fn test_claude_config_always_allowed() {
        let sess = sess_with_worktrees();
        let home = config::home();
        let p = format!("{}/.claude/settings.json", home.display());
        let result = check_path(&p, Some(&sess));
        assert!(
            matches!(result, PathDecision::Allow),
            "expected ALLOW for global claude config, got {:?}",
            result
        );
    }

    #[test]
    fn test_no_worktree_session_config_allowed() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let sess = sess_no_worktrees();
        let ws = config::workspace();
        let ws_str = ws.to_string_lossy();
        let paths = [
            format!("{}/.claude/hooks-v2/test.go", ws_str),
            format!("{}/.agents/test.md", ws_str),
            format!("{}/CLAUDE.md", ws_str),
            format!("{}/.claude-tmp/abc12345-test/output.txt", ws_str),
        ];
        for p in &paths {
            let result = check_path(p, Some(&sess));
            assert!(
                matches!(result, PathDecision::Allow),
                "expected ALLOW for config path {:?} even with failed worktrees, got {:?}",
                p,
                result
            );
        }
    }

    #[test]
    fn test_repo_agents_allowed_in_worktree_mode() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let sess = sess_with_worktrees();
        let ws = config::workspace();
        let ws_str = ws.to_string_lossy();
        // Writing to <repo>/.agents/ should be allowed (not redirected to worktree)
        let paths = [
            format!("{}/acme-api/.agents/learnings/test.md", ws_str),
            format!("{}/web-app/.agents/handoff/test.md", ws_str),
            format!("{}/acme-api/.agents/council/report.md", ws_str),
        ];
        for p in &paths {
            let result = check_path(p, Some(&sess));
            assert!(
                matches!(result, PathDecision::Allow),
                "expected ALLOW for repo .agents/ path {:?}, got {:?}",
                p,
                result
            );
        }
    }

    #[test]
    fn test_repo_claude_md_allowed_in_worktree_mode() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let sess = sess_with_worktrees();
        let ws = config::workspace();
        let ws_str = ws.to_string_lossy();
        // Per-repo CLAUDE.md and AGENTS.md should be allowed (not redirected)
        let paths = [
            format!("{}/acme-api/CLAUDE.md", ws_str),
            format!("{}/web-app/AGENTS.md", ws_str),
            format!("{}/api-server/.claude/hooks/test.rs", ws_str),
        ];
        for p in &paths {
            let result = check_path(p, Some(&sess));
            assert!(
                matches!(result, PathDecision::Allow),
                "expected ALLOW for repo config path {:?}, got {:?}",
                p,
                result
            );
        }
    }

    #[test]
    fn test_worktree_config_paths_redirected() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let sess = sess_with_worktrees();
        let ws = config::workspace();
        let ws_str = ws.to_string_lossy();

        // Create temp worktree dirs with .gitignore that ignores .agents/ and .claude/
        let wt1 = format!("{}/redir-test/.worktrees/abc12345", ws_str);
        let wt2 = format!("{}/redir-test2/.worktrees/xyz99999", ws_str);
        std::fs::create_dir_all(&wt1).expect("create wt1");
        std::fs::create_dir_all(&wt2).expect("create wt2");
        std::fs::write(format!("{}/.gitignore", wt1), ".agents/\n.claude/\n")
            .expect("write .gitignore wt1");
        std::fs::write(format!("{}/.gitignore", wt2), ".agents/\n.claude/\n")
            .expect("write .gitignore wt2");

        let paths = [
            format!("{}/.agents/learnings/test.md", wt1),
            format!("{}/.agents/handoff/notes.md", wt2),
            format!("{}/.claude/hooks/test.rs", wt1),
        ];
        for p in &paths {
            let result = check_path(p, Some(&sess));
            assert!(
                matches!(result, PathDecision::Deny(_)),
                "expected DENY/REDIRECT for worktree config path {:?}, got {:?}",
                p,
                result
            );
            if let PathDecision::Deny(msg) = &result {
                assert!(
                    msg.contains("REDIRECT"),
                    "expected REDIRECT message for {:?}, got: {}",
                    p,
                    msg
                );
                let target = msg.split("Write to: ").nth(1).unwrap_or("");
                assert!(
                    !target.contains("/.worktrees/"),
                    "redirect target should not contain /.worktrees/: {}",
                    target
                );
            }
        }

        // Cleanup
        let _ = std::fs::remove_dir_all(format!("{}/redir-test", ws_str));
        let _ = std::fs::remove_dir_all(format!("{}/redir-test2", ws_str));
    }

    #[test]
    fn test_worktree_committed_files_allowed() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let sess = sess_with_worktrees();
        let ws = config::workspace();
        let ws_str = ws.to_string_lossy();
        // CLAUDE.md and AGENTS.md are committed repo files — allow in worktrees
        let paths = [
            format!("{}/acme-api/.worktrees/abc12345/CLAUDE.md", ws_str),
            format!("{}/web-app/.worktrees/xyz99999/AGENTS.md", ws_str),
        ];
        for p in &paths {
            let result = check_path(p, Some(&sess));
            assert!(
                matches!(result, PathDecision::Allow),
                "expected ALLOW for committed file in worktree {:?}, got {:?}",
                p,
                result
            );
        }
    }

    #[test]
    fn test_worktree_agents_allowed_when_not_gitignored() {
        // When .agents/ is NOT in .gitignore, it's tracked → allow in worktree.
        let _lock = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let sess = sess_with_worktrees();
        let ws = config::workspace();
        let ws_str = ws.to_string_lossy();

        // Worktree with .gitignore that does NOT ignore .agents/
        let wt_root = format!("{}/tracked-repo/.worktrees/abc12345", ws_str);
        std::fs::create_dir_all(&wt_root).expect("create wt dir");
        std::fs::write(format!("{}/.gitignore", wt_root), "target/\n*.log\n")
            .expect("write .gitignore");

        let test_path = format!("{}/.agents/brainstorm/notes.md", wt_root);
        let result = check_path(&test_path, Some(&sess));
        assert!(
            matches!(result, PathDecision::Allow),
            "expected ALLOW for .agents/ not in .gitignore, got {:?}",
            result
        );

        let _ = std::fs::remove_dir_all(format!("{}/tracked-repo", ws_str));
    }

    #[test]
    fn test_worktree_agents_allowed_when_no_gitignore() {
        // When no .gitignore exists at all, nothing is ignored → allow.
        let _lock = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let sess = sess_with_worktrees();
        let ws = config::workspace();
        let ws_str = ws.to_string_lossy();

        let wt_root = format!("{}/nogi-repo/.worktrees/abc12345", ws_str);
        std::fs::create_dir_all(&wt_root).expect("create wt dir");
        // No .gitignore file at all

        let test_path = format!("{}/.agents/brainstorm/notes.md", wt_root);
        let result = check_path(&test_path, Some(&sess));
        assert!(
            matches!(result, PathDecision::Allow),
            "expected ALLOW when no .gitignore exists, got {:?}",
            result
        );

        let _ = std::fs::remove_dir_all(format!("{}/nogi-repo", ws_str));
    }

    #[test]
    fn test_worktree_non_agents_still_allowed() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let sess = sess_with_worktrees();
        let ws = config::workspace();
        let ws_str = ws.to_string_lossy();
        // Regular worktree files should still be allowed (not redirected)
        let paths = [
            format!("{}/acme-api/.worktrees/abc12345/dags/train.py", ws_str),
            format!("{}/web-app/.worktrees/abc12345/app/main.py", ws_str),
        ];
        for p in &paths {
            let result = check_path(p, Some(&sess));
            assert!(
                matches!(result, PathDecision::Allow),
                "expected ALLOW for regular worktree path {:?}, got {:?}",
                p,
                result
            );
        }
    }

    #[test]
    fn test_changelog_files_allowed_no_worktree() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let sess = sess_no_worktrees();
        let ws = config::workspace();
        let ws_str = ws.to_string_lossy();
        let paths = [
            format!("{}/.claude-changelog-abc12345-test.md", ws_str),
            format!("{}/.claude-trace-abc12345-test.md", ws_str),
            format!("{}/.claude-worktrees-abc12345-test.env", ws_str),
        ];
        for p in &paths {
            let result = check_path(p, Some(&sess));
            assert!(
                matches!(result, PathDecision::Allow),
                "expected ALLOW for session meta file {:?}, got {:?}",
                p,
                result
            );
        }
    }

    // ── Sandbox edge-case hardening tests (directive 2) ──────────────

    #[test]
    fn test_symlink_to_system_path() {
        // Create a symlink inside a temp dir that points to a system path.
        // The sandbox must resolve the symlink and deny the write.
        use std::fs;
        use std::os::unix::fs::symlink;

        let tmp = std::env::temp_dir().join("muzzle-test-symlink");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).expect("create test dir");

        let link = tmp.join("etc-link");
        let _ = fs::remove_file(&link);
        symlink("/etc", &link).expect("create symlink to /etc");

        let attack_path = format!("{}/hosts", link.display());
        let result = is_system_path_resolved(&attack_path);
        assert!(result, "symlink to /etc should resolve to a system path");

        // Also test via check_path — should be Deny
        let sess = no_session();
        let decision = check_path(&attack_path, Some(&sess));
        assert!(
            matches!(decision, PathDecision::Deny(_)),
            "expected DENY for symlink traversal to system path, got {:?}",
            decision
        );

        // Cleanup
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_symlink_traversal_via_parent() {
        // Symlink that makes a path look like it's in workspace but resolves outside.
        use std::fs;
        use std::os::unix::fs::symlink;

        let tmp = std::env::temp_dir().join("muzzle-test-symlink-parent");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).expect("create test dir");

        // Create symlink: tmp/escape -> /
        let link = tmp.join("escape");
        let _ = fs::remove_file(&link);
        symlink("/", &link).expect("create symlink to /");

        // Path through symlink resolves to /etc/hosts
        let attack_path = format!("{}/etc/hosts", link.display());
        let result = is_system_path_resolved(&attack_path);
        assert!(result, "symlink traversal through root should be detected");

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_path_with_spaces() {
        let sess = no_session();
        let home = config::home();

        // Path with spaces under HOME — should be allowed
        let p = format!("{}/My Documents/some file.txt", home.display());
        let result = check_path(&p, Some(&sess));
        assert!(
            matches!(result, PathDecision::Allow),
            "expected ALLOW for home path with spaces, got {:?}",
            result
        );

        // System path with spaces — still blocked
        let result = check_path("/etc/my config/hosts", Some(&sess));
        assert!(
            matches!(result, PathDecision::Deny(_)),
            "expected DENY for system path with spaces, got {:?}",
            result
        );
    }

    #[test]
    fn test_unicode_filenames() {
        let sess = no_session();
        let home = config::home();

        // Unicode filenames under HOME — should be allowed
        let paths = [
            format!("{}/Documents/日本語ファイル.txt", home.display()),
            format!("{}/données/résumé.pdf", home.display()),
            format!("{}/src/émoji_🦀_test.rs", home.display()),
        ];
        for p in &paths {
            let result = check_path(p, Some(&sess));
            assert!(
                matches!(result, PathDecision::Allow),
                "expected ALLOW for unicode filename {:?}, got {:?}",
                p,
                result
            );
        }

        // Unicode in system path — still blocked
        let result = check_path("/etc/données/config", Some(&sess));
        assert!(
            matches!(result, PathDecision::Deny(_)),
            "expected DENY for system path with unicode, got {:?}",
            result
        );
    }

    #[test]
    fn test_dot_dot_traversal_system_path() {
        // Path traversal using /../ to reach system paths.
        // On real filesystems canonicalize resolves this, but we also need
        // the raw-string check as defense-in-depth for non-existent paths.
        let home = config::home();

        // This path canonicalizes to /etc/hosts if the prefix exists
        let attack = format!("{}/../../etc/hosts", home.display());
        let resolved = resolve_path(&attack);
        // canonicalize should resolve this since $HOME exists
        assert!(
            is_system_path(&resolved) || is_private_system_path(&resolved),
            "dot-dot traversal to /etc should resolve to system path, resolved to: {}",
            resolved
        );
    }

    #[test]
    fn test_double_slash_system_path() {
        let sess = no_session();

        // Double-slash paths: //etc/hosts should still be blocked after resolution
        let result = check_path("//etc/hosts", Some(&sess));
        assert!(
            matches!(result, PathDecision::Deny(_)),
            "expected DENY for double-slash system path, got {:?}",
            result
        );

        // Multiple slashes
        let result = check_path("///etc///hosts", Some(&sess));
        assert!(
            matches!(result, PathDecision::Deny(_)),
            "expected DENY for multi-slash system path, got {:?}",
            result
        );
    }

    #[test]
    fn test_macos_private_prefix_deny() {
        let sess = no_session();

        // macOS resolves /etc -> /private/etc, /var -> /private/var
        let paths = ["/private/etc/hosts", "/private/var/log/syslog"];
        for p in &paths {
            let result = check_path(p, Some(&sess));
            assert!(
                matches!(result, PathDecision::Deny(_)),
                "expected DENY for /private/ system path {:?}, got {:?}",
                p,
                result
            );
        }
    }

    #[test]
    fn test_empty_path() {
        let sess = no_session();
        // Empty path should not panic — graceful handling
        let result = check_path("", Some(&sess));
        // Empty resolves to cwd; should not crash regardless of decision
        assert!(
            !matches!(result, PathDecision::Deny(ref m) if m.contains("panic")),
            "empty path should not cause panic, got {:?}",
            result
        );
    }

    #[test]
    fn test_very_long_path() {
        let sess = no_session();
        // Very long path — should not cause stack overflow or panic
        let long_segment = "a".repeat(255);
        let long_path = format!("/tmp/{}/{}/{}", long_segment, long_segment, long_segment);
        let result = check_path_with_context(&long_path, Some(&sess), ToolContext::Bash);
        // /tmp paths via Bash → Allow
        assert!(
            matches!(result, PathDecision::Allow),
            "expected ALLOW for long /tmp path via Bash, got {:?}",
            result
        );
    }

    #[test]
    fn test_trailing_slash_system_paths() {
        let sess = no_session();
        // Trailing slashes should not bypass system path checks
        let paths = ["/etc/", "/usr/", "/var/", "/opt/"];
        for p in &paths {
            let result = check_path(p, Some(&sess));
            assert!(
                matches!(result, PathDecision::Deny(_)),
                "expected DENY for system path with trailing slash {:?}, got {:?}",
                p,
                result
            );
        }
    }

    #[test]
    fn test_no_session_state_null() {
        // Passing None for session state should not panic
        let result = check_path("/tmp/test.txt", None);
        // No session → falls through to HOME/outside-HOME checks
        assert!(
            !matches!(result, PathDecision::Deny(_)),
            "expected non-DENY for /tmp with no session, got {:?}",
            result
        );
    }

    #[test]
    fn test_worktree_path_with_spaces_allowed() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let sess = sess_with_worktrees();
        let ws = config::workspace();
        let ws_str = ws.to_string_lossy();
        // Repo name or file path with spaces inside worktree
        let p = format!(
            "{}/my-repo/.worktrees/abc12345/path with spaces/file.py",
            ws_str
        );
        let result = check_path(&p, Some(&sess));
        assert!(
            matches!(result, PathDecision::Allow),
            "expected ALLOW for worktree path with spaces, got {:?}",
            result
        );
    }

    #[test]
    fn test_dot_dot_in_worktree_path() {
        // Attempt to escape worktree via /../
        let sess = sess_with_worktrees();
        let ws = config::workspace();
        let ws_str = ws.to_string_lossy();
        let attack = format!("{}/web-app/.worktrees/abc12345/../../secret.key", ws_str);
        // If the worktree dir exists, canonicalize resolves the /../
        // and the result would be <ws>/web-app/secret.key which is a main
        // checkout path → should be DENY/REDIRECT, not ALLOW
        let result = check_path(&attack, Some(&sess));
        assert!(
            !matches!(result, PathDecision::Allow),
            "dot-dot escape from worktree should not be allowed, got {:?}",
            result
        );
    }

    #[test]
    fn test_case_sensitivity_system_paths() {
        let sess = no_session();
        // On case-sensitive filesystems, /Etc/hosts is NOT /etc/hosts
        // But we should still check — macOS HFS+ is case-insensitive
        // The sandbox uses exact string matching, which is correct for case-sensitive systems
        let result = check_path("/Etc/hosts", Some(&sess));
        // On case-insensitive macOS, canonicalize might resolve to /etc/hosts
        // On case-sensitive Linux, this path doesn't exist
        // Either way, the test verifies no panic and sensible behavior
        assert!(
            !matches!(result, PathDecision::Allow),
            "/Etc/hosts should not be blindly allowed, got {:?}",
            result
        );
    }

    #[test]
    fn test_resolve_path_nonexistent_deep() {
        // resolve_path should handle deeply nested non-existent paths without panic
        let deep = "/nonexistent/a/b/c/d/e/f/g/h/i/j/k/l/m/test.txt";
        let resolved = resolve_path(deep);
        // Should return the path as-is (absolute, no canonicalization possible)
        assert!(
            resolved.starts_with('/'),
            "resolved path should be absolute, got: {}",
            resolved
        );
    }

    #[test]
    fn test_dev_fd_range() {
        let sess = no_session();
        // File descriptors beyond typical range — still allowed (no range check)
        let paths = ["/dev/fd/0", "/dev/fd/99", "/dev/fd/255"];
        for p in &paths {
            let result = check_path(p, Some(&sess));
            assert!(
                matches!(result, PathDecision::Allow),
                "expected ALLOW for {:?}, got {:?}",
                p,
                result
            );
        }
    }

    #[test]
    fn test_worktree_missing_for_unknown_repo() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let sess = sess_no_worktrees();
        let ws = config::workspace();
        let ws_str = ws.to_string_lossy();
        // Write to a repo path when no worktrees exist → WORKTREE_MISSING
        let p = format!("{}/unknown-repo/src/main.rs", ws_str);
        let result = check_path(&p, Some(&sess));
        if let PathDecision::Deny(msg) = &result {
            assert!(
                msg.contains("WORKTREE_MISSING"),
                "expected WORKTREE_MISSING in deny message, got: {}",
                msg
            );
        } else {
            panic!(
                "expected DENY with WORKTREE_MISSING for repo without worktree, got {:?}",
                result
            );
        }
    }

    // ── normalize_dot_segments unit tests ────────────────────────────

    #[test]
    fn test_normalize_basic() {
        assert_eq!(normalize_dot_segments("/a/b/c"), "/a/b/c");
        assert_eq!(normalize_dot_segments("/a/./b"), "/a/b");
        assert_eq!(normalize_dot_segments("/a/b/../c"), "/a/c");
        assert_eq!(normalize_dot_segments("/a/b/../../c"), "/c");
    }

    #[test]
    fn test_normalize_above_root() {
        // Going above root should clamp at root
        assert_eq!(normalize_dot_segments("/a/../../../etc"), "/etc");
        assert_eq!(normalize_dot_segments("/../etc/hosts"), "/etc/hosts");
    }

    #[test]
    fn test_normalize_empty_and_dot() {
        // Empty string is relative → normalizes to "."
        assert_eq!(normalize_dot_segments(""), ".");
        assert_eq!(normalize_dot_segments("."), ".");
        assert_eq!(normalize_dot_segments("./a"), "a");
    }

    #[test]
    fn test_normalize_preserves_absolute() {
        assert!(normalize_dot_segments("/tmp/foo").starts_with('/'));
        assert!(!normalize_dot_segments("relative/path").starts_with('/'));
    }

    #[test]
    fn test_normalize_double_slashes() {
        // Double slashes produce empty segments which are skipped
        assert_eq!(normalize_dot_segments("//etc//hosts"), "/etc/hosts");
    }
}
