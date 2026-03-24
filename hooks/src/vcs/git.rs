//! Git backend for [`VcsBackend`] trait.
//!
//! Delegates every trait method to existing functions in [`crate::worktree::git`],
//! [`crate::worktree::cleanup`], and [`crate::gitcheck`]. No new behavior — pure
//! delegation.

use crate::config;
use crate::gitcheck::{self, AskResult, GitResult};
use crate::session::SpecEntry;
use crate::vcs::{VcsBackend, VcsKind, WorkspaceInfo};
use crate::worktree::{cleanup, git};
use std::path::{Path, PathBuf};
use std::process::Command;

/// Git backend using git worktrees for session isolation.
///
/// Zero-size struct — all state lives in the repository itself.
pub struct GitBackend;

impl VcsBackend for GitBackend {
    fn kind(&self) -> VcsKind {
        VcsKind::Git
    }

    fn workspace_add(
        &self,
        repo_path: &Path,
        dest: &Path,
        session_id: &str,
        branch: Option<&str>,
        tmp_dir: &Path,
    ) -> Result<SpecEntry, String> {
        let repo_str = repo_path.to_string_lossy().to_string();
        let wt_path = config::worktree_path(repo_path, session_id);

        // Ensure .worktrees dir exists.
        std::fs::create_dir_all(config::worktree_dir(repo_path))
            .map_err(|e| format!("failed to create worktree dir: {e}"))?;

        // Fetch and resolve default branch.
        let default_branch = git::fetch_and_resolve_default_branch(repo_path, tmp_dir);

        // Determine branch strategy (mirrors create_single_worktree in worktree/mod.rs).
        let (actual_branch, args) = match branch {
            None => {
                // No branch specified: create ephemeral wt/<short-id>.
                let br = format!("wt/{session_id}");
                let args = vec![
                    "-C".into(),
                    repo_str.clone(),
                    "worktree".into(),
                    "add".into(),
                    "-b".into(),
                    br.clone(),
                    wt_path.to_string_lossy().to_string(),
                    format!("origin/{default_branch}"),
                ];
                (br, args)
            }
            Some(b) if b == default_branch => {
                // Default branch requested: redirect to ephemeral (don't lock default).
                let br = format!("wt/{session_id}");
                let args = vec![
                    "-C".into(),
                    repo_str.clone(),
                    "worktree".into(),
                    "add".into(),
                    "-b".into(),
                    br.clone(),
                    wt_path.to_string_lossy().to_string(),
                    format!("origin/{default_branch}"),
                ];
                (br, args)
            }
            Some(b) if git::branch_exists(repo_path, b) => {
                // Existing branch: check it out.
                let args = vec![
                    "-C".into(),
                    repo_str.clone(),
                    "worktree".into(),
                    "add".into(),
                    wt_path.to_string_lossy().to_string(),
                    b.into(),
                ];
                (b.to_string(), args)
            }
            Some(b) => {
                // New branch: create from origin/<default>.
                let args = vec![
                    "-C".into(),
                    repo_str.clone(),
                    "worktree".into(),
                    "add".into(),
                    "-b".into(),
                    b.into(),
                    wt_path.to_string_lossy().to_string(),
                    format!("origin/{default_branch}"),
                ];
                (b.to_string(), args)
            }
        };

        // First attempt.
        if git::run_git_strings(&args).is_err() {
            // Retry after prune.
            let _ = git::run_git(&["-C", &repo_str, "worktree", "prune"]);
            if let Err(e) = git::run_git_strings(&args) {
                // Clean up partial worktree.
                let _ = std::fs::remove_dir_all(&wt_path);
                return Err(format!("git worktree add failed (after retry): {e}"));
            }
        }

        let repo_name = dest
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();

        Ok(SpecEntry {
            repo: repo_name,
            branch: actual_branch,
            wt_path: wt_path.to_string_lossy().to_string(),
            repo_path: repo_path.to_string_lossy().to_string(),
            vcs_kind: VcsKind::Git,
        })
    }

    fn workspace_remove(&self, entry: &SpecEntry, force: bool) -> (bool, Option<String>) {
        if force {
            let err = git::run_git(&[
                "-C",
                &entry.repo_path,
                "worktree",
                "remove",
                "--force",
                &entry.wt_path,
            ])
            .err();
            (false, err)
        } else {
            cleanup::remove(entry)
        }
    }

    fn workspace_list(&self, repo_path: &Path) -> Vec<WorkspaceInfo> {
        git::get_active_worktrees(repo_path)
            .into_iter()
            .map(|path| {
                let pb = PathBuf::from(&path);
                WorkspaceInfo {
                    name: pb
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_string(),
                    path: pb,
                }
            })
            .collect()
    }

    fn workspace_prune(&self, repo_path: &Path) {
        cleanup::prune_stale_worktrees(repo_path);
    }

    fn is_clean(&self, path: &Path) -> bool {
        Command::new("git")
            .args([
                "-C",
                &path.to_string_lossy(),
                "diff-index",
                "--quiet",
                "HEAD",
                "--",
            ])
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }

    fn fetch(&self, repo_path: &Path, tmp_dir: &Path) {
        git::fetch_origin(repo_path, tmp_dir);
    }

    fn default_branch(&self, repo_path: &Path, tmp_dir: &Path) -> String {
        git::fetch_and_resolve_default_branch(repo_path, tmp_dir)
    }

    fn is_repo(&self, path: &Path) -> bool {
        git::is_git_repo(path)
    }

    fn is_valid_workspace(&self, path: &str) -> bool {
        git::is_valid_worktree(path)
    }

    fn check_safety(&self, cmd: &str) -> GitResult {
        gitcheck::check_git_safety(cmd)
    }

    fn check_ask(&self, cmd: &str) -> AskResult {
        gitcheck::check_gh_merge(cmd)
    }

    fn check_workspace_enforcement(
        &self,
        cmd: &str,
        workspace_active: bool,
        short_id: &str,
    ) -> Option<String> {
        gitcheck::check_worktree_enforcement(cmd, workspace_active, short_id, self.kind())
    }

    fn extract_repo_from_op(&self, cmd: &str) -> Option<String> {
        gitcheck::extract_repo_from_git_op(cmd)
    }

    fn is_workspace_management_op(&self, cmd: &str) -> bool {
        gitcheck::is_worktree_management_op(cmd)
    }
}
