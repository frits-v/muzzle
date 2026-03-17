//! Constants, path helpers, and shared configuration for muzzle.
//!
//! Home, workspaces, and state directory are resolved at runtime from
//! the environment. Not hardcoded — the compiled binary works on any machine.
//!
//! **Workspaces** are repo roots where sandbox/worktree enforcement applies.
//! **State directory** holds session artifacts (PID markers, changelogs, specs).

use std::path::{Path, PathBuf};

/// Max PPID walk depth for session resolution.
pub const PPID_WALK_DEPTH: usize = 3;

// Legacy constants — used by session_start.rs glob patterns for migration.
// New code should use path helpers directly.

/// PID marker subdirectory (relative to state_dir).
pub const PID_MARKER_DIR: &str = "by-pid";

/// Legacy spec file prefix (for migration scanning).
pub const SPEC_FILE_PREFIX: &str = ".claude-worktrees-";
/// Legacy spec file suffix.
pub const SPEC_FILE_SUFFIX: &str = ".env";

/// Legacy changelog prefix (for migration scanning).
pub const CHANGELOG_PREFIX: &str = ".claude-changelog-";
/// Legacy changelog suffix.
pub const CHANGELOG_SUFFIX: &str = ".md";

/// Legacy trace log prefix (for migration scanning).
pub const TRACE_PREFIX: &str = ".claude-trace-";
/// Legacy trace log suffix.
pub const TRACE_SUFFIX: &str = ".md";

/// Atlassian rate-limit sliding window in seconds (5 min).
pub const ATLASSIAN_RATE_WINDOW: u64 = 300;
/// Max Atlassian write calls per rate window before prompting.
pub const ATLASSIAN_RATE_LIMIT: usize = 3;

/// Max age (hours) before orphaned worktrees are pruned.
pub const ORPHAN_WORKTREE_MAX_AGE_HOURS: u64 = 24;
/// Max age (days) before stale spec files are removed.
pub const STALE_SPEC_FILE_MAX_AGE_DAYS: u64 = 7;
/// Max age (days) before stale temp directories are removed.
pub const STALE_TEMP_DIR_MAX_AGE_DAYS: u64 = 7;
/// Max age (days) before stale PID markers are removed.
pub const STALE_PID_MARKER_MAX_AGE_DAYS: u64 = 1;
/// Safety cap on cleanup iterations to avoid runaway loops.
pub const MAX_CLEANUP_ITERATIONS: usize = 50;

// ── Core resolution ─────────────────────────────────────────────────

/// Resolve $HOME from environment or dirs fallback.
pub fn home() -> PathBuf {
    if let Ok(h) = std::env::var("HOME") {
        if !h.is_empty() {
            return PathBuf::from(h);
        }
    }
    dirs_fallback().unwrap_or_else(|| PathBuf::from("/tmp"))
}

fn dirs_fallback() -> Option<PathBuf> {
    // On macOS/Linux, HOME is almost always set. This is a last resort.
    // We avoid libc dependency; just return None and let the caller use /tmp.
    None
}

/// Config file path: `~/.config/muzzle/config`.
pub fn config_file() -> PathBuf {
    home().join(".config").join("muzzle").join("config")
}

/// Read a key from the config file (simple `key = value` format).
fn read_config_key(key: &str) -> Option<String> {
    let content = std::fs::read_to_string(config_file()).ok()?;
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            if k.trim() == key {
                let v = v.trim();
                if !v.is_empty() {
                    return Some(v.to_string());
                }
            }
        }
    }
    None
}

// ── Workspaces ──────────────────────────────────────────────────────

/// All configured workspace roots (repo directories under sandbox enforcement).
///
/// Resolution order:
/// 1. `MUZZLE_WORKSPACES` env var (comma-separated)
/// 2. `workspaces` key in config (comma-separated)
/// 3. Legacy `MUZZLE_WORKSPACE` env var (single path)
/// 4. Legacy `workspace` key in config (single path)
/// 5. `$HOME/src` default
pub fn workspaces() -> Vec<PathBuf> {
    // New multi-workspace env
    if let Ok(val) = std::env::var("MUZZLE_WORKSPACES") {
        let paths = parse_path_list(&val);
        if !paths.is_empty() {
            return paths;
        }
    }

    // New multi-workspace config key
    if let Some(val) = read_config_key("workspaces") {
        let paths = parse_path_list(&val);
        if !paths.is_empty() {
            return paths;
        }
    }

    // Legacy single workspace env
    if let Ok(ws) = std::env::var("MUZZLE_WORKSPACE") {
        if !ws.is_empty() {
            return vec![PathBuf::from(ws)];
        }
    }

    // Legacy single workspace config key
    if let Some(ws) = read_config_key("workspace") {
        return vec![PathBuf::from(ws)];
    }

    vec![home().join("src")]
}

/// Legacy single-workspace accessor. Returns the first configured workspace.
///
/// Prefer `workspaces()` or `workspace_for_path()` for multi-workspace support.
pub fn workspace() -> PathBuf {
    workspaces()
        .into_iter()
        .next()
        .unwrap_or_else(|| home().join("src"))
}

/// Find which configured workspace contains a given path, if any.
pub fn workspace_for_path(path: &Path) -> Option<PathBuf> {
    workspaces().into_iter().find(|ws| is_under(path, ws))
}

/// Check if PWD is under any configured workspace.
pub fn is_in_any_workspace() -> bool {
    let Ok(pwd) = std::env::current_dir() else {
        return false;
    };
    let wss = workspaces();
    wss.iter().any(|ws| is_under(&pwd, ws))
}

/// Check if PWD is under any configured workspace.
///
/// Alias for `is_in_any_workspace()` — kept for backward compatibility.
pub fn is_in_workspace() -> bool {
    is_in_any_workspace()
}

/// Validate that all workspace directories exist.
///
/// Returns `Ok(paths)` if all exist, `Err(message)` listing missing ones.
pub fn validate_workspaces() -> Result<Vec<PathBuf>, String> {
    let wss = workspaces();
    let missing: Vec<_> = wss.iter().filter(|ws| !ws.is_dir()).collect();
    if missing.is_empty() {
        Ok(wss)
    } else {
        let paths: Vec<_> = missing.iter().map(|p| p.display().to_string()).collect();
        Err(format!(
            "Workspace directories do not exist: {}. \
             Set MUZZLE_WORKSPACES or create them.",
            paths.join(", ")
        ))
    }
}

/// Legacy workspace validator. Validates the first workspace.
pub fn validate_workspace() -> Result<PathBuf, String> {
    let ws = workspace();
    if ws.is_dir() {
        Ok(ws)
    } else {
        Err(format!(
            "Workspace directory does not exist: {}. \
             Set MUZZLE_WORKSPACE or create the directory.",
            ws.display()
        ))
    }
}

/// Parse a comma-separated list of paths, trimming whitespace.
fn parse_path_list(s: &str) -> Vec<PathBuf> {
    s.split(',')
        .map(|p| p.trim())
        .filter(|p| !p.is_empty())
        .map(PathBuf::from)
        .collect()
}

// ── State directory ─────────────────────────────────────────────────

/// XDG state directory for session artifacts.
///
/// Resolution order:
/// 1. `MUZZLE_STATE_DIR` env var
/// 2. `state_dir` key in config
/// 3. `$XDG_STATE_HOME/muzzle`
/// 4. `$HOME/.local/state/muzzle` default
///
/// Structure:
/// ```text
/// ~/.local/state/muzzle/
/// ├── by-pid/        # PID markers for session resolution
/// ├── changelogs/    # Per-session changelog files
/// ├── specs/         # Worktree spec files
/// ├── traces/        # Decision trace logs
/// └── tmp/           # Per-session temp directories
/// ```
pub fn state_dir() -> PathBuf {
    if let Ok(sd) = std::env::var("MUZZLE_STATE_DIR") {
        if !sd.is_empty() {
            return PathBuf::from(sd);
        }
    }

    if let Some(sd) = read_config_key("state_dir") {
        return PathBuf::from(sd);
    }

    // XDG_STATE_HOME fallback
    if let Ok(xdg) = std::env::var("XDG_STATE_HOME") {
        if !xdg.is_empty() {
            return PathBuf::from(xdg).join("muzzle");
        }
    }

    home().join(".local").join("state").join("muzzle")
}

/// Validate (and create if missing) the state directory.
pub fn validate_state_dir() -> Result<PathBuf, String> {
    let sd = state_dir();
    if sd.is_dir() {
        return Ok(sd);
    }
    // Attempt to create — state_dir is owned by muzzle, safe to auto-create.
    std::fs::create_dir_all(&sd).map_err(|e| {
        format!(
            "Cannot create state directory {}: {}. Set MUZZLE_STATE_DIR.",
            sd.display(),
            e
        )
    })?;
    Ok(sd)
}

/// Ensure all state subdirectories exist.
pub fn ensure_state_subdirs() -> Result<(), String> {
    let sd = validate_state_dir()?;
    for sub in &["by-pid", "changelogs", "specs", "traces", "tmp"] {
        let dir = sd.join(sub);
        if !dir.is_dir() {
            std::fs::create_dir_all(&dir)
                .map_err(|e| format!("Cannot create {}: {}", dir.display(), e))?;
        }
    }
    Ok(())
}

// ── Path helpers (state-dir relative) ───────────────────────────────

/// PID marker file path for a given PID.
pub fn pid_marker_path(pid: u32) -> PathBuf {
    state_dir().join(PID_MARKER_DIR).join(pid.to_string())
}

/// PID marker directory path.
pub fn pid_marker_dir_path() -> PathBuf {
    state_dir().join(PID_MARKER_DIR)
}

/// Spec file path for a session.
pub fn spec_file_path(session_id: &str) -> PathBuf {
    state_dir()
        .join("specs")
        .join(format!("{}{}", session_id, SPEC_FILE_SUFFIX))
}

/// Changelog path for a session.
pub fn changelog_path(session_id: &str) -> PathBuf {
    state_dir()
        .join("changelogs")
        .join(format!("{}{}", session_id, CHANGELOG_SUFFIX))
}

/// Gzipped changelog path.
pub fn changelog_gz_path(session_id: &str) -> PathBuf {
    state_dir()
        .join("changelogs")
        .join(format!("{}{}.gz", session_id, CHANGELOG_SUFFIX))
}

/// Trace log path for a session.
pub fn trace_path(session_id: &str) -> PathBuf {
    state_dir()
        .join("traces")
        .join(format!("{}{}", session_id, TRACE_SUFFIX))
}

/// Gzipped trace log path.
pub fn trace_gz_path(session_id: &str) -> PathBuf {
    state_dir()
        .join("traces")
        .join(format!("{}{}.gz", session_id, TRACE_SUFFIX))
}

/// Changelog convenience symlink path.
pub fn changelog_symlink() -> PathBuf {
    state_dir().join("current-changelog.md")
}

/// Per-session temp directory path.
pub fn session_tmp_dir(session_id: &str) -> PathBuf {
    state_dir().join("tmp").join(session_id)
}

/// Rate limit directory for a session.
pub fn rate_limit_dir(session_id: &str) -> PathBuf {
    session_tmp_dir(session_id).join("rate-limits")
}

// ── Worktree helpers (workspace-relative) ───────────────────────────

/// .worktrees directory for a repo.
pub fn worktree_dir(repo_path: &Path) -> PathBuf {
    repo_path.join(".worktrees")
}

/// Worktree path for a repo + short ID.
pub fn worktree_path(repo_path: &Path, short_id: &str) -> PathBuf {
    repo_path.join(".worktrees").join(short_id)
}

// ── Utility ─────────────────────────────────────────────────────────

/// First 8 chars of a session ID.
pub fn short_id(session_id: &str) -> String {
    if session_id.len() > 8 {
        session_id[..8].to_string()
    } else {
        session_id.to_string()
    }
}

/// Check if `path` is under (or equal to) `dir`.
pub fn is_under(path: &Path, dir: &Path) -> bool {
    let path_s = path.to_string_lossy();
    let dir_s = dir.to_string_lossy();

    let path_trimmed = path_s.trim_end_matches('/');
    let dir_trimmed = dir_s.trim_end_matches('/');

    if path_trimmed == dir_trimmed {
        return true;
    }

    let prefix = format!("{}/", dir_trimmed);
    path_trimmed.starts_with(&prefix)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Use the crate-level ENV_LOCK shared across all modules
    use crate::ENV_LOCK;

    #[test]
    fn test_short_id() {
        assert_eq!(short_id("abc12345-6789-0000-1111-222233334444"), "abc12345");
        assert_eq!(short_id("short"), "short");
        assert_eq!(short_id("12345678"), "12345678");
        assert_eq!(short_id("1234567890"), "12345678");
        assert_eq!(short_id(""), "");
    }

    #[test]
    fn test_is_under() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let ws = workspace();
        assert!(is_under(&ws.join("web-app/app.py"), &ws));
        assert!(is_under(&ws, &ws));
        assert!(is_under(&PathBuf::from(format!("{}/", ws.display())), &ws));
        assert!(!is_under(&PathBuf::from(format!("{}x", ws.display())), &ws));
        assert!(!is_under(&PathBuf::from("/tmp/foo"), &ws));
        assert!(!is_under(&home(), &ws));
    }

    #[test]
    fn test_pid_marker_path() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let path = pid_marker_path(12345);
        let expected = state_dir().join("by-pid/12345");
        assert_eq!(path, expected);
    }

    #[test]
    fn test_pid_marker_dir_path() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let path = pid_marker_dir_path();
        assert!(path.ends_with("by-pid"));
    }

    #[test]
    fn test_spec_file_path() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let path = spec_file_path("test-session-id");
        let expected = state_dir().join("specs/test-session-id.env");
        assert_eq!(path, expected);
    }

    #[test]
    fn test_changelog_path() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let path = changelog_path("test-session-id");
        let expected = state_dir().join("changelogs/test-session-id.md");
        assert_eq!(path, expected);
    }

    #[test]
    fn test_session_tmp_dir() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let path = session_tmp_dir("test-session-id");
        let expected = state_dir().join("tmp/test-session-id");
        assert_eq!(path, expected);
    }

    #[test]
    fn test_home_and_workspace_not_empty() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        assert!(!home().as_os_str().is_empty());
        assert!(!workspace().as_os_str().is_empty());
    }

    #[test]
    fn test_validate_workspace_exists() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        // Use a known-existing directory so this works on CI too
        let tmp = std::env::temp_dir();
        std::env::set_var("MUZZLE_WORKSPACE", tmp.as_os_str());
        let result = validate_workspace();
        std::env::remove_var("MUZZLE_WORKSPACE");
        assert!(result.is_ok(), "workspace should exist: {:?}", result);
    }

    #[test]
    fn test_validate_workspace_missing() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        std::env::set_var("MUZZLE_WORKSPACE", "/tmp/muzzle-nonexistent-test-dir");
        let result = validate_workspace();
        std::env::remove_var("MUZZLE_WORKSPACE");
        assert!(result.is_err());
        let msg = result.unwrap_err();
        assert!(msg.contains("does not exist"), "error: {}", msg);
        assert!(msg.contains("MUZZLE_WORKSPACE"), "error: {}", msg);
    }

    #[test]
    fn test_config_file_path() {
        let path = config_file();
        assert!(path.ends_with(".config/muzzle/config"));
    }

    #[test]
    fn test_changelog_gz_path() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let path = changelog_gz_path("sess-1");
        let expected = state_dir().join("changelogs/sess-1.md.gz");
        assert_eq!(path, expected);
    }

    #[test]
    fn test_trace_path() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let path = trace_path("sess-2");
        let expected = state_dir().join("traces/sess-2.md");
        assert_eq!(path, expected);
    }

    #[test]
    fn test_trace_gz_path() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let path = trace_gz_path("sess-2");
        let expected = state_dir().join("traces/sess-2.md.gz");
        assert_eq!(path, expected);
    }

    #[test]
    fn test_changelog_symlink_path() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let path = changelog_symlink();
        assert!(path.ends_with("current-changelog.md"));
    }

    #[test]
    fn test_rate_limit_dir() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let path = rate_limit_dir("sess-3");
        assert!(path.ends_with("sess-3/rate-limits"));
    }

    #[test]
    fn test_worktree_dir() {
        let repo = Path::new("/tmp/my-repo");
        assert_eq!(worktree_dir(repo), PathBuf::from("/tmp/my-repo/.worktrees"));
    }

    #[test]
    fn test_worktree_path() {
        let repo = Path::new("/tmp/my-repo");
        assert_eq!(
            worktree_path(repo, "abc12345"),
            PathBuf::from("/tmp/my-repo/.worktrees/abc12345")
        );
    }

    #[test]
    fn test_workspace_env_override() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        std::env::set_var("MUZZLE_WORKSPACE", "/tmp/test-ws");
        let ws = workspace();
        std::env::remove_var("MUZZLE_WORKSPACE");
        assert_eq!(ws, PathBuf::from("/tmp/test-ws"));
    }

    #[test]
    fn test_workspace_empty_env_falls_back() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        std::env::set_var("MUZZLE_WORKSPACE", "");
        let ws = workspace();
        std::env::remove_var("MUZZLE_WORKSPACE");
        // Should fall through to config or default, not be empty
        assert!(!ws.as_os_str().is_empty());
    }

    // ── New multi-workspace tests ───────────────────────────────────

    #[test]
    fn test_state_dir_default() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        std::env::remove_var("MUZZLE_STATE_DIR");
        std::env::remove_var("XDG_STATE_HOME");
        let sd = state_dir();
        assert!(
            sd.ends_with(".local/state/muzzle"),
            "default state_dir should be ~/.local/state/muzzle, got: {}",
            sd.display()
        );
    }

    #[test]
    fn test_state_dir_env_override() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        std::env::set_var("MUZZLE_STATE_DIR", "/tmp/muzzle-state-test");
        let sd = state_dir();
        std::env::remove_var("MUZZLE_STATE_DIR");
        assert_eq!(sd, PathBuf::from("/tmp/muzzle-state-test"));
    }

    #[test]
    fn test_state_dir_xdg_override() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        std::env::remove_var("MUZZLE_STATE_DIR");
        std::env::set_var("XDG_STATE_HOME", "/tmp/xdg-state");
        let sd = state_dir();
        std::env::remove_var("XDG_STATE_HOME");
        assert_eq!(sd, PathBuf::from("/tmp/xdg-state/muzzle"));
    }

    #[test]
    fn test_workspaces_env_comma_separated() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        std::env::set_var("MUZZLE_WORKSPACES", "/tmp/ws-a, /tmp/ws-b, /tmp/ws-c");
        let wss = workspaces();
        std::env::remove_var("MUZZLE_WORKSPACES");
        assert_eq!(
            wss,
            vec![
                PathBuf::from("/tmp/ws-a"),
                PathBuf::from("/tmp/ws-b"),
                PathBuf::from("/tmp/ws-c"),
            ]
        );
    }

    #[test]
    fn test_workspaces_legacy_singular_fallback() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        std::env::remove_var("MUZZLE_WORKSPACES");
        std::env::set_var("MUZZLE_WORKSPACE", "/tmp/legacy-ws");
        let wss = workspaces();
        std::env::remove_var("MUZZLE_WORKSPACE");
        assert_eq!(wss, vec![PathBuf::from("/tmp/legacy-ws")]);
    }

    #[test]
    fn test_workspaces_empty_env_falls_back() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        std::env::set_var("MUZZLE_WORKSPACES", "");
        std::env::set_var("MUZZLE_WORKSPACE", "");
        let wss = workspaces();
        std::env::remove_var("MUZZLE_WORKSPACES");
        std::env::remove_var("MUZZLE_WORKSPACE");
        assert!(!wss.is_empty(), "should fall back to default");
    }

    #[test]
    fn test_workspace_for_path_finds_match() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        std::env::set_var("MUZZLE_WORKSPACES", "/tmp/ws-a, /tmp/ws-b");
        let result = workspace_for_path(Path::new("/tmp/ws-b/some-repo/file.rs"));
        std::env::remove_var("MUZZLE_WORKSPACES");
        assert_eq!(result, Some(PathBuf::from("/tmp/ws-b")));
    }

    #[test]
    fn test_workspace_for_path_no_match() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        std::env::set_var("MUZZLE_WORKSPACES", "/tmp/ws-a, /tmp/ws-b");
        let result = workspace_for_path(Path::new("/home/user/unmanaged/file.rs"));
        std::env::remove_var("MUZZLE_WORKSPACES");
        assert_eq!(result, None);
    }

    #[test]
    fn test_is_in_any_workspace() {
        // This test can't easily set PWD, so just verify it returns a bool
        // without panicking. The underlying is_under() is tested separately.
        let _ = is_in_any_workspace();
    }

    #[test]
    fn test_parse_path_list_trims_whitespace() {
        let paths = parse_path_list("  /a , /b/c ,  /d  ");
        assert_eq!(
            paths,
            vec![
                PathBuf::from("/a"),
                PathBuf::from("/b/c"),
                PathBuf::from("/d"),
            ]
        );
    }

    #[test]
    fn test_parse_path_list_empty() {
        assert!(parse_path_list("").is_empty());
        assert!(parse_path_list("  ,  , ").is_empty());
    }

    #[test]
    fn test_validate_workspaces_all_exist() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let tmp = std::env::temp_dir();
        std::env::set_var("MUZZLE_WORKSPACES", tmp.to_string_lossy().as_ref());
        let result = validate_workspaces();
        std::env::remove_var("MUZZLE_WORKSPACES");
        assert!(result.is_ok(), "temp dir should exist: {:?}", result);
    }

    #[test]
    fn test_validate_workspaces_some_missing() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let val = format!(
            "{}, /tmp/muzzle-nonexistent-ws",
            std::env::temp_dir().display()
        );
        std::env::set_var("MUZZLE_WORKSPACES", &val);
        let result = validate_workspaces();
        std::env::remove_var("MUZZLE_WORKSPACES");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("muzzle-nonexistent-ws"));
    }

    #[test]
    fn test_pid_marker_uses_state_dir() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        std::env::set_var("MUZZLE_STATE_DIR", "/tmp/muzzle-test-state");
        let path = pid_marker_path(999);
        std::env::remove_var("MUZZLE_STATE_DIR");
        assert_eq!(path, PathBuf::from("/tmp/muzzle-test-state/by-pid/999"));
    }

    #[test]
    fn test_changelog_uses_state_dir() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        std::env::set_var("MUZZLE_STATE_DIR", "/tmp/muzzle-test-state");
        let path = changelog_path("sess-42");
        std::env::remove_var("MUZZLE_STATE_DIR");
        assert_eq!(
            path,
            PathBuf::from("/tmp/muzzle-test-state/changelogs/sess-42.md")
        );
    }

    #[test]
    fn test_spec_uses_state_dir() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        std::env::set_var("MUZZLE_STATE_DIR", "/tmp/muzzle-test-state");
        let path = spec_file_path("sess-42");
        std::env::remove_var("MUZZLE_STATE_DIR");
        assert_eq!(
            path,
            PathBuf::from("/tmp/muzzle-test-state/specs/sess-42.env")
        );
    }
}
