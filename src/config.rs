//! Constants, path helpers, and shared configuration for muzzle.
//!
//! Home and Workspace are resolved at runtime from the environment.
//! Not hardcoded — the compiled binary works on any machine.

use std::path::{Path, PathBuf};

/// Max PPID walk depth for session resolution.
pub const PPID_WALK_DEPTH: usize = 3;

/// PID marker directory (relative to Workspace).
pub const PID_MARKER_DIR: &str = ".claude-tmp/by-pid";

/// Worktree spec file name prefix (relative to Workspace).
pub const SPEC_FILE_PREFIX: &str = ".claude-worktrees-";
/// Worktree spec file name suffix.
pub const SPEC_FILE_SUFFIX: &str = ".env";

/// Changelog file name prefix (relative to Workspace).
pub const CHANGELOG_PREFIX: &str = ".claude-changelog-";
/// Changelog file name suffix.
pub const CHANGELOG_SUFFIX: &str = ".md";

/// Trace log file name prefix (relative to Workspace).
pub const TRACE_PREFIX: &str = ".claude-trace-";
/// Trace log file name suffix.
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

/// Resolve the workspace path.
///
/// Resolution order:
/// 1. `MUZZLE_WORKSPACE` env var
/// 2. `workspace` key in `~/.config/muzzle/config`
/// 3. `$HOME/src` default
pub fn workspace() -> PathBuf {
    if let Ok(ws) = std::env::var("MUZZLE_WORKSPACE") {
        if !ws.is_empty() {
            return PathBuf::from(ws);
        }
    }
    if let Some(ws) = read_config_key("workspace") {
        return PathBuf::from(ws);
    }
    home().join("src")
}

/// PID marker file path for a given PID.
pub fn pid_marker_path(pid: u32) -> PathBuf {
    workspace().join(PID_MARKER_DIR).join(pid.to_string())
}

/// PID marker directory path.
pub fn pid_marker_dir_path() -> PathBuf {
    workspace().join(PID_MARKER_DIR)
}

/// Spec file path for a session.
pub fn spec_file_path(session_id: &str) -> PathBuf {
    workspace().join(format!(
        "{}{}{}",
        SPEC_FILE_PREFIX, session_id, SPEC_FILE_SUFFIX
    ))
}

/// Changelog path for a session.
pub fn changelog_path(session_id: &str) -> PathBuf {
    workspace().join(format!(
        "{}{}{}",
        CHANGELOG_PREFIX, session_id, CHANGELOG_SUFFIX
    ))
}

/// Gzipped changelog path.
pub fn changelog_gz_path(session_id: &str) -> PathBuf {
    workspace().join(format!(
        "{}{}{}.gz",
        CHANGELOG_PREFIX, session_id, CHANGELOG_SUFFIX
    ))
}

/// Trace log path for a session.
pub fn trace_path(session_id: &str) -> PathBuf {
    workspace().join(format!("{}{}{}", TRACE_PREFIX, session_id, TRACE_SUFFIX))
}

/// Gzipped trace log path.
pub fn trace_gz_path(session_id: &str) -> PathBuf {
    workspace().join(format!("{}{}{}.gz", TRACE_PREFIX, session_id, TRACE_SUFFIX))
}

/// Changelog convenience symlink path.
pub fn changelog_symlink() -> PathBuf {
    workspace().join(".claude-changelog.md")
}

/// Per-session temp directory path.
pub fn session_tmp_dir(session_id: &str) -> PathBuf {
    workspace().join(".claude-tmp").join(session_id)
}

/// Rate limit directory for a session.
pub fn rate_limit_dir(session_id: &str) -> PathBuf {
    session_tmp_dir(session_id).join("rate-limits")
}

/// .worktrees directory for a repo.
pub fn worktree_dir(repo_path: &Path) -> PathBuf {
    repo_path.join(".worktrees")
}

/// Worktree path for a repo + short ID.
pub fn worktree_path(repo_path: &Path, short_id: &str) -> PathBuf {
    repo_path.join(".worktrees").join(short_id)
}

/// First 8 chars of a session ID.
pub fn short_id(session_id: &str) -> String {
    if session_id.len() > 8 {
        session_id[..8].to_string()
    } else {
        session_id.to_string()
    }
}

/// Validate that the workspace directory exists.
///
/// Returns `Ok(path)` if it exists, `Err(message)` with a clear error otherwise.
/// Use this at binary entry points for early failure with actionable guidance.
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

/// Check if PWD is under the workspace.
pub fn is_in_workspace() -> bool {
    let Ok(pwd) = std::env::current_dir() else {
        return false;
    };
    is_under(&pwd, &workspace())
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
        let expected = workspace().join(".claude-tmp/by-pid/12345");
        assert_eq!(path, expected);
    }

    #[test]
    fn test_spec_file_path() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let path = spec_file_path("test-session-id");
        let expected = workspace().join(".claude-worktrees-test-session-id.env");
        assert_eq!(path, expected);
    }

    #[test]
    fn test_changelog_path() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let path = changelog_path("test-session-id");
        let expected = workspace().join(".claude-changelog-test-session-id.md");
        assert_eq!(path, expected);
    }

    #[test]
    fn test_session_tmp_dir() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let path = session_tmp_dir("test-session-id");
        let expected = workspace().join(".claude-tmp/test-session-id");
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
        // Point MUZZLE_WORKSPACE at a nonexistent path
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
    fn test_pid_marker_dir_path() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let path = pid_marker_dir_path();
        assert!(path.ends_with(".claude-tmp/by-pid"));
    }

    #[test]
    fn test_changelog_gz_path() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let path = changelog_gz_path("sess-1");
        let expected = workspace().join(".claude-changelog-sess-1.md.gz");
        assert_eq!(path, expected);
    }

    #[test]
    fn test_trace_path() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let path = trace_path("sess-2");
        let expected = workspace().join(".claude-trace-sess-2.md");
        assert_eq!(path, expected);
    }

    #[test]
    fn test_trace_gz_path() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let path = trace_gz_path("sess-2");
        let expected = workspace().join(".claude-trace-sess-2.md.gz");
        assert_eq!(path, expected);
    }

    #[test]
    fn test_changelog_symlink_path() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let path = changelog_symlink();
        assert!(path.ends_with(".claude-changelog.md"));
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
}
