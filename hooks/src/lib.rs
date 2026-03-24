//! Muzzle — session isolation and workspace sandboxing for AI coding agents.
//!
//! Provides a three-layer hook system for Claude Code:
//! 1. **Session resolution** — identify the active session via PPID walk
//! 2. **Context-aware sandbox** — enforce worktree isolation for writes
//! 3. **Git safety** — block dangerous git operations (force push, etc.)
//!
//! Each module corresponds to a functional layer. The `bin/` directory contains
//! the hook entry points that Claude Code invokes.

#![warn(missing_docs)]

pub mod changelog;
pub mod config;
pub mod gitcheck;
pub mod log;
pub mod mcp;
pub mod output;
pub mod sandbox;
pub mod session;
pub mod worktree;

/// Format a WORKTREE_MISSING denial message for lazy worktree creation.
///
/// The message serves two purposes:
/// 1. Prescriptive action: tells the agent to run ensure-worktree
/// 2. Explicit prohibition: names common bypass vectors to prevent rationalization
pub fn worktree_missing_msg(repo: &str) -> String {
    let bin = config::bin_dir().join("ensure-worktree");
    format!(
        "WORKTREE_MISSING:{repo} \
         — Run: {} {repo}\n\
         DO NOT use Bash (sed -i, cp, mv, perl -i, dd, patch, etc.) to bypass this check. \
         All file writes to the main checkout are forbidden during worktree sessions.",
        bin.display()
    )
}

/// Shared test mutex for all tests that mutate `MUZZLE_WORKSPACE` env var.
/// Since `env::set_var` is process-wide and `cargo test` runs in parallel,
/// every test that writes to this env var must hold this lock to prevent
/// races with tests in other modules that call `config::workspace()`.
#[cfg(test)]
pub(crate) static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_worktree_missing_msg_format() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let msg = worktree_missing_msg("my-repo");
        assert!(msg.starts_with("WORKTREE_MISSING:my-repo"));
        assert!(msg.contains("ensure-worktree my-repo"));
        // Must contain an absolute-looking path, not the old relative one
        assert!(
            !msg.contains(".claude/hooks/bin"),
            "should not contain hardcoded relative path: {msg}"
        );
        assert!(msg.contains("DO NOT use Bash"));
        assert!(msg.contains("sed -i, cp, mv"));
    }

    #[test]
    fn test_worktree_missing_msg_special_chars() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let msg = worktree_missing_msg("repo-with-dashes");
        assert!(msg.starts_with("WORKTREE_MISSING:repo-with-dashes"));

        let msg = worktree_missing_msg(".dotfile-repo");
        assert!(msg.starts_with("WORKTREE_MISSING:.dotfile-repo"));
    }

    #[test]
    fn test_worktree_missing_msg_uses_bin_dir() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        std::env::set_var("MUZZLE_BIN_DIR", "/opt/muzzle/bin");
        let msg = worktree_missing_msg("acme-api");
        std::env::remove_var("MUZZLE_BIN_DIR");
        assert!(
            msg.contains("/opt/muzzle/bin/ensure-worktree acme-api"),
            "should use MUZZLE_BIN_DIR: {msg}"
        );
    }

    #[test]
    fn test_worktree_missing_msg_bypass_prohibition() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let msg = worktree_missing_msg("web-app");
        assert!(
            msg.contains("forbidden"),
            "message must explicitly prohibit bypass"
        );
        assert!(
            msg.contains("DO NOT"),
            "message must use prescriptive language"
        );
    }
}
