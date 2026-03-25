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
pub mod vcs;
pub mod worktree;

/// Format a WORKTREE_MISSING denial message for lazy worktree creation.
///
/// The message encodes the VCS kind so `ensure-worktree` knows which backend
/// to use when creating the workspace on demand.
pub fn worktree_missing_msg(repo: &str, vcs_kind: vcs::VcsKind) -> String {
    let bin = config::bin_dir().join("ensure-worktree");
    format!(
        "WORKTREE_MISSING:{repo}:{vcs} \
         — Run: {} {repo} {vcs}",
        bin.display(),
        vcs = vcs_kind,
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
        let msg = worktree_missing_msg("my-repo", vcs::VcsKind::Git);
        assert!(msg.starts_with("WORKTREE_MISSING:my-repo:git"));
        assert!(msg.contains("ensure-worktree my-repo git"));
        // Must contain an absolute-looking path, not the old relative one
        assert!(
            !msg.contains(".claude/hooks/bin"),
            "should not contain hardcoded relative path: {msg}"
        );
    }

    #[test]
    fn test_worktree_missing_msg_special_chars() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let msg = worktree_missing_msg("repo-with-dashes", vcs::VcsKind::Git);
        assert!(msg.starts_with("WORKTREE_MISSING:repo-with-dashes:git"));

        let msg = worktree_missing_msg(".dotfile-repo", vcs::VcsKind::Jj);
        assert!(msg.starts_with("WORKTREE_MISSING:.dotfile-repo:jj"));
    }

    #[test]
    fn test_worktree_missing_msg_uses_bin_dir() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        std::env::set_var("MUZZLE_BIN_DIR", "/opt/muzzle/bin");
        let msg = worktree_missing_msg("acme-api", vcs::VcsKind::Git);
        std::env::remove_var("MUZZLE_BIN_DIR");
        assert!(
            msg.contains("/opt/muzzle/bin/ensure-worktree acme-api git"),
            "should use MUZZLE_BIN_DIR: {msg}"
        );
    }

    #[test]
    fn test_worktree_missing_msg_jj_colocated() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let msg = worktree_missing_msg("acme-api", vcs::VcsKind::JjColocated);
        assert!(
            msg.starts_with("WORKTREE_MISSING:acme-api:jj-coloc"),
            "should encode jj-coloc VCS kind: {msg}"
        );
    }
}
