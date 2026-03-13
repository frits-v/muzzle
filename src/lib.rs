pub mod changelog;
pub mod config;
pub mod gitcheck;
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
