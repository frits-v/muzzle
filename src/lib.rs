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
pub fn worktree_missing_msg(repo: &str) -> String {
    format!(
        "WORKTREE_MISSING:{repo} \
         — Run: .claude/hooks/bin/ensure-worktree {repo}"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_worktree_missing_msg_format() {
        let msg = worktree_missing_msg("my-repo");
        assert!(msg.starts_with("WORKTREE_MISSING:my-repo"));
        assert!(msg.contains("ensure-worktree my-repo"));
    }

    #[test]
    fn test_worktree_missing_msg_special_chars() {
        let msg = worktree_missing_msg("repo-with-dashes");
        assert!(msg.starts_with("WORKTREE_MISSING:repo-with-dashes"));

        let msg = worktree_missing_msg(".dotfile-repo");
        assert!(msg.starts_with("WORKTREE_MISSING:.dotfile-repo"));
    }
}
