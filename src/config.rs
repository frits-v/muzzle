//! Constants, path helpers, and shared configuration for hooks-v3.
//!
//! Home and Workspace are resolved at runtime from the environment.
//! Not hardcoded — the compiled binary works on any machine.

use std::path::{Path, PathBuf};

/// Max PPID walk depth for session resolution.
pub const PPID_WALK_DEPTH: usize = 3;

/// PID marker directory (relative to Workspace).
pub const PID_MARKER_DIR: &str = ".claude-tmp/by-pid";

/// Spec file prefix/suffix (relative to Workspace).
pub const SPEC_FILE_PREFIX: &str = ".claude-worktrees-";
pub const SPEC_FILE_SUFFIX: &str = ".env";

/// Changelog prefix/suffix (relative to Workspace).
pub const CHANGELOG_PREFIX: &str = ".claude-changelog-";
pub const CHANGELOG_SUFFIX: &str = ".md";

/// Trace prefix/suffix (relative to Workspace).
pub const TRACE_PREFIX: &str = ".claude-trace-";
pub const TRACE_SUFFIX: &str = ".md";

/// Atlassian rate limiting.
pub const ATLASSIAN_RATE_WINDOW: u64 = 300; // seconds (5 min)
pub const ATLASSIAN_RATE_LIMIT: usize = 3;

/// Cleanup thresholds.
pub const ORPHAN_WORKTREE_MAX_AGE_HOURS: u64 = 24;
pub const STALE_SPEC_FILE_MAX_AGE_DAYS: u64 = 7;
pub const STALE_TEMP_DIR_MAX_AGE_DAYS: u64 = 7;
pub const STALE_PID_MARKER_MAX_AGE_DAYS: u64 = 1;
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

/// Resolve the workspace path: $HOME/src/cn
pub fn workspace() -> PathBuf {
    home().join("src").join("cn")
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
        let ws = workspace();
        assert!(is_under(&ws.join("Hermosa/app.py"), &ws));
        assert!(is_under(&ws, &ws));
        assert!(is_under(&PathBuf::from(format!("{}/", ws.display())), &ws));
        assert!(!is_under(&PathBuf::from(format!("{}x", ws.display())), &ws));
        assert!(!is_under(&PathBuf::from("/tmp/foo"), &ws));
        assert!(!is_under(&home(), &ws));
    }

    #[test]
    fn test_pid_marker_path() {
        let path = pid_marker_path(12345);
        let expected = workspace().join(".claude-tmp/by-pid/12345");
        assert_eq!(path, expected);
    }

    #[test]
    fn test_spec_file_path() {
        let path = spec_file_path("test-session-id");
        let expected = workspace().join(".claude-worktrees-test-session-id.env");
        assert_eq!(path, expected);
    }

    #[test]
    fn test_changelog_path() {
        let path = changelog_path("test-session-id");
        let expected = workspace().join(".claude-changelog-test-session-id.md");
        assert_eq!(path, expected);
    }

    #[test]
    fn test_session_tmp_dir() {
        let path = session_tmp_dir("test-session-id");
        let expected = workspace().join(".claude-tmp/test-session-id");
        assert_eq!(path, expected);
    }

    #[test]
    fn test_home_and_workspace_not_empty() {
        assert!(!home().as_os_str().is_empty());
        assert!(!workspace().as_os_str().is_empty());
    }
}
