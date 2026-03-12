//! Path sandboxing for write operations.
//!
//! FR-PS-1 through FR-PS-7, FR-WE-1 through FR-WE-5.

use crate::config;
use crate::session::State;
use std::path::Path;

/// A path sandboxing decision.
#[derive(Debug, Clone, PartialEq)]
pub enum PathDecision {
    Allow,
    Deny(String),
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
        return PathDecision::Deny(format!("BLOCKED: Cannot write to system path: {}", raw_path));
    }

    // Resolve to absolute (follow symlinks)
    let resolved = resolve_path(&path_str);

    // System paths check on resolved path (catches /private/etc, /private/var on macOS)
    if is_system_path(&resolved) || is_private_system_path(&resolved) {
        return PathDecision::Deny(format!("BLOCKED: Cannot write to system path: {}", raw_path));
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
                    // But redirect .agents/ writes back to main checkout for persistence.
                    // E.g. <repo>/.worktrees/<id>/.agents/foo.md → <repo>/.agents/foo.md
                    const WORKTREES_SEG: &str = "/.worktrees/";
                    if let Some(wt_idx) = resolved.find(WORKTREES_SEG) {
                        let after_wt = &resolved[wt_idx..]; // "/.worktrees/<id>/..."
                        // Find the slash after the worktree ID
                        if let Some(id_slash) = after_wt[WORKTREES_SEG.len()..].find('/') {
                            let after_id = &after_wt[WORKTREES_SEG.len() + id_slash..]; // "/..." after ID
                            if is_persistent_repo_config(after_id) {
                                let repo_prefix = &resolved[..wt_idx];
                                return PathDecision::Deny(format!(
                                    "REDIRECT: config path must persist across sessions. Write to: {}{}",
                                    repo_prefix, after_id
                                ));
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
                        return PathDecision::Deny(format!(
                            "WORKTREE_MISSING:{} — Run: .claude/hooks/bin/ensure-worktree {}",
                            repo, repo
                        ));
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
                        return PathDecision::Deny(format!(
                            "WORKTREE_MISSING:{} — Run: .claude/hooks/bin/ensure-worktree {}",
                            repo, repo
                        ));
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

/// Resolve a path string to absolute, following symlinks where possible.
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

    // Try making it absolute
    if !Path::new(path).is_absolute() {
        if let Ok(abs) = std::env::current_dir() {
            return abs.join(path).to_string_lossy().to_string();
        }
    }

    path.to_string()
}

/// Check if a raw path resolves to a system path (public, for git -C checks).
pub fn is_system_path_resolved(raw_path: &str) -> bool {
    let resolved = resolve_path(raw_path);
    is_system_path(raw_path) || is_system_path(&resolved) || is_private_system_path(&resolved)
}

/// Check if a path is a system path.
fn is_system_path(path: &str) -> bool {
    let prefixes = [
        "/etc/", "/usr/", "/System/", "/Library/", "/bin/", "/sbin/", "/var/", "/opt/",
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

/// Check if a subpath (relative to a repo root) is a persistent config path.
///
/// Used by both `is_config_path` (FR-WE-4) and the FR-WE-3 worktree redirect
/// to ensure consistent recognition. Adding a new persistent path type here
/// automatically covers both direct writes AND worktree-intercepted writes.
///
/// `subpath` starts with "/" — e.g. "/.agents/foo.md", "/CLAUDE.md".
/// Trailing slashes in `starts_with` prevent false positives (e.g. `.agentsfoo`).
fn is_persistent_repo_config(subpath: &str) -> bool {
    subpath.starts_with("/.agents/")
        || subpath == "/.agents"
        || subpath.starts_with("/.claude/")
        || subpath == "/.claude"
        || subpath == "/CLAUDE.md"
        || subpath == "/AGENTS.md"
}

/// Check if a path is a project config path (FR-WE-4).
///
/// Matches both workspace-level config (e.g. `<ws>/.agents/`) and per-repo
/// config (e.g. `<ws>/ml-upsell/.agents/`, `<ws>/my-app/CLAUDE.md`).
/// Per-repo config paths must persist across sessions, not go to worktrees.
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
        // rest = "ml-upsell/.agents/foo.md" or "my-app/CLAUDE.md"
        // First segment is the repo name; after_repo is everything after it
        if let Some(slash_idx) = rest.find('/') {
            let after_repo = &rest[slash_idx..];
            if is_persistent_repo_config(after_repo) {
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

    let dangerous_dirs = [
        format!("{}/.ssh/", home),
        format!("{}/.aws/", home),
    ];
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
        let sess = sess_with_worktrees();
        let ws = config::workspace();
        let ws_str = ws.to_string_lossy();
        let paths = [
            format!("{}/my-app/.worktrees/abc12345/some/file.py", ws_str),
            format!("{}/cuboh-core/.worktrees/abc12345/test.py", ws_str),
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
        let sess = sess_with_worktrees();
        let ws = config::workspace();
        let ws_str = ws.to_string_lossy();
        let paths = [
            format!("{}/my-app/app/main.py", ws_str),
            format!("{}/cuboh-core/src/test.py", ws_str),
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
        let sess = sess_no_worktrees();
        let ws = config::workspace();
        let p = format!("{}/my-app/app/main.py", ws.display());
        let result = check_path(&p, Some(&sess));
        assert!(
            matches!(result, PathDecision::Deny(_)),
            "expected DENY when worktrees failed, got {:?}",
            result
        );
    }

    #[test]
    fn test_no_worktree_session_allows_existing_worktree_paths() {
        let sess = sess_no_worktrees();
        let ws = config::workspace();
        let ws_str = ws.to_string_lossy();
        // Worktree from a previous/other session should still be writable
        let paths = [
            format!("{}/.github/.worktrees/sentinel/.qlty/qlty.toml", ws_str),
            format!("{}/my-app/.worktrees/other-session/app/main.py", ws_str),
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
        let sess = sess_with_worktrees();
        let ws = config::workspace();
        let ws_str = ws.to_string_lossy();
        // Writing to <repo>/.agents/ should be allowed (not redirected to worktree)
        let paths = [
            format!("{}/ml-upsell/.agents/learnings/test.md", ws_str),
            format!("{}/my-app/.agents/handoff/test.md", ws_str),
            format!("{}/ml-upsell/.agents/council/report.md", ws_str),
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
        let sess = sess_with_worktrees();
        let ws = config::workspace();
        let ws_str = ws.to_string_lossy();
        // Per-repo CLAUDE.md and AGENTS.md should be allowed (not redirected)
        let paths = [
            format!("{}/ml-upsell/CLAUDE.md", ws_str),
            format!("{}/my-app/AGENTS.md", ws_str),
            format!("{}/cuboh-core/.claude/hooks/test.rs", ws_str),
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
        let sess = sess_with_worktrees();
        let ws = config::workspace();
        let ws_str = ws.to_string_lossy();
        // All persistent config paths inside worktrees should redirect to main checkout.
        // This verifies the shared is_persistent_repo_config() predicate covers all types.
        let paths = [
            // .agents/
            format!(
                "{}/ml-upsell/.worktrees/abc12345/.agents/learnings/test.md",
                ws_str
            ),
            format!(
                "{}/my-app/.worktrees/xyz99999/.agents/handoff/notes.md",
                ws_str
            ),
            // CLAUDE.md / AGENTS.md
            format!(
                "{}/ml-upsell/.worktrees/abc12345/CLAUDE.md",
                ws_str
            ),
            format!(
                "{}/my-app/.worktrees/xyz99999/AGENTS.md",
                ws_str
            ),
            // .claude/
            format!(
                "{}/ml-upsell/.worktrees/abc12345/.claude/hooks/test.rs",
                ws_str
            ),
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
                    p, msg
                );
                // Ensure the redirect target does NOT contain /.worktrees/
                let target = msg.split("Write to: ").nth(1).unwrap_or("");
                assert!(
                    !target.contains("/.worktrees/"),
                    "redirect target should not contain /.worktrees/: {}",
                    target
                );
            }
        }
    }

    #[test]
    fn test_worktree_non_agents_still_allowed() {
        let sess = sess_with_worktrees();
        let ws = config::workspace();
        let ws_str = ws.to_string_lossy();
        // Regular worktree files should still be allowed (not redirected)
        let paths = [
            format!("{}/ml-upsell/.worktrees/abc12345/dags/train.py", ws_str),
            format!("{}/my-app/.worktrees/abc12345/app/main.py", ws_str),
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
}
