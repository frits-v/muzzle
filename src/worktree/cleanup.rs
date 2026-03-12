//! Worktree cleanup, pruning, and rollback operations.

use crate::config;
use crate::session::SpecEntry;
use super::git;
use std::fs;
use std::path::Path;
use std::process::Command;

/// Remove a worktree. If dirty (uncommitted changes), warns but doesn't force.
/// Returns (dirty, error).
pub fn remove(entry: &SpecEntry) -> (bool, Option<String>) {
    let wt = Path::new(&entry.wt_path);
    if !wt.exists() {
        return (false, None); // Already gone
    }

    // Check for uncommitted changes
    let dirty = Command::new("git")
        .args(["-C", &entry.wt_path, "diff-index", "--quiet", "HEAD", "--"])
        .status()
        .map(|s| !s.success())
        .unwrap_or(true);

    if dirty {
        return (true, None);
    }

    // Clean — remove worktree
    let mut err = None;
    if git::run_git(&["-C", &entry.repo_path, "worktree", "remove", &entry.wt_path]).is_err() {
        if let Err(e) = git::run_git(&["-C", &entry.repo_path, "worktree", "remove", "--force", &entry.wt_path]) {
            err = Some(e);
        }
    }

    if err.is_none() && entry.branch.starts_with("wt/") {
        // Clean up ephemeral branch
        let _ = git::run_git(&["-C", &entry.repo_path, "branch", "-D", &entry.branch]);
    }

    (false, err)
}

/// Prune stale worktree metadata in a repo.
pub fn prune_stale_worktrees(repo_path: &Path) {
    let repo_str = repo_path.to_string_lossy().to_string();
    let _ = git::run_git(&["-C", &repo_str, "worktree", "prune"]);
}

/// Remove empty .worktrees/ directories.
pub fn clean_empty_worktree_dirs(repo_path: &Path) {
    let wt_dir = config::worktree_dir(repo_path);
    if let Ok(entries) = fs::read_dir(&wt_dir) {
        if entries.count() == 0 {
            let _ = fs::remove_dir(&wt_dir);
        }
    }
}

/// Rollback: remove all worktrees created so far.
pub fn rollback(entries: &[SpecEntry]) {
    for e in entries {
        let _ = git::run_git(&["-C", &e.repo_path, "worktree", "remove", "--force", &e.wt_path]);
        let _ = fs::remove_dir_all(&e.wt_path);
        if e.branch.starts_with("wt/") {
            let _ = git::run_git(&["-C", &e.repo_path, "branch", "-D", &e.branch]);
        }
    }
}
