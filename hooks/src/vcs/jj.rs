//! Jujutsu (jj) backend for [`VcsBackend`] trait.
//!
//! Provides workspace management and safety checks for Jujutsu repositories,
//! including colocated mode where both `.jj/` and `.git/` are present.

use crate::gitcheck::{self, AskResult, GitResult};
use crate::session::SpecEntry;
use crate::vcs::{VcsBackend, VcsKind, WorkspaceInfo};
use regex::Regex;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::LazyLock;

/// Jujutsu (jj) backend using jj workspaces for session isolation.
pub struct JjBackend {
    /// True if the repo is colocated (`.jj/` + `.git/`).
    pub colocated: bool,
}

// Pre-compiled regexes for jj safety patterns.
static RE_JJ_GIT_PUSH: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\bjj\s+git\s+push\b").unwrap());
static RE_JJ_PUSH_BOOKMARK: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\bjj\s+git\s+push\b.*(-b|--bookmark)\s+").unwrap());
static RE_JJ_BOOKMARK_DELETE_MAIN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\bjj\s+bookmark\s+delete\s+(main|master|trunk)\b").unwrap());
static RE_JJ_REPO_FLAG: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\bjj\s+-R\s+(\S+)").unwrap());
static RE_JJ_WORKSPACE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\bjj\s+workspace\b").unwrap());
static RE_JJ_EDIT_IMMUTABLE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\bjj\s+edit\s+(root|trunk)\b").unwrap());

impl VcsBackend for JjBackend {
    fn kind(&self) -> VcsKind {
        if self.colocated {
            VcsKind::JjColocated
        } else {
            VcsKind::Jj
        }
    }

    fn workspace_add(
        &self,
        repo_path: &Path,
        dest: &Path,
        session_id: &str,
        _branch: Option<&str>,
        _tmp_dir: &Path,
    ) -> Result<SpecEntry, String> {
        let repo_str = repo_path.to_string_lossy();
        let dest_str = dest.to_string_lossy();

        let status = Command::new("jj")
            .args(["workspace", "add", &dest_str, "--name", session_id])
            .current_dir(repo_path)
            .status()
            .map_err(|e| format!("failed to run jj workspace add: {e}"))?;

        if !status.success() {
            return Err(format!("jj workspace add failed for {dest_str}"));
        }

        let repo_name = repo_path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();

        Ok(SpecEntry {
            repo: repo_name,
            branch: String::new(), // jj workspaces aren't branch-bound
            wt_path: dest_str.to_string(),
            repo_path: repo_str.to_string(),
            vcs_kind: self.kind(),
        })
    }

    fn workspace_remove(&self, entry: &SpecEntry, _force: bool) -> (bool, Option<String>) {
        // Run forget from repo root (workspace dir may be in a bad state).
        let ws_name = std::path::Path::new(&entry.wt_path)
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        let forget_status = Command::new("jj")
            .args(["workspace", "forget", &ws_name])
            .current_dir(&entry.repo_path)
            .status();

        match forget_status {
            Ok(status) if status.success() => {}
            Ok(_) => {
                return (
                    false,
                    Some(format!("jj workspace forget failed for {}", entry.wt_path)),
                );
            }
            Err(e) => {
                return (
                    false,
                    Some(format!("failed to run jj workspace forget: {e}")),
                );
            }
        }

        if let Err(e) = std::fs::remove_dir_all(&entry.wt_path) {
            return (
                false,
                Some(format!("failed to remove {}: {e}", entry.wt_path)),
            );
        }

        (true, None)
    }

    fn workspace_list(&self, repo_path: &Path) -> Vec<WorkspaceInfo> {
        let output = Command::new("jj")
            .args(["workspace", "list"])
            .current_dir(repo_path)
            .output();

        let output = match output {
            Ok(o) if o.status.success() => o,
            _ => return Vec::new(),
        };

        let stdout = String::from_utf8_lossy(&output.stdout);
        stdout
            .lines()
            .filter_map(|line| {
                // Format: "name: <revision> (at <path>)"
                let name = line.split(':').next()?.trim().to_string();
                let path = if let Some(start) = line.find("(at ") {
                    let rest = &line[start + 4..];
                    rest.strip_suffix(')').unwrap_or(rest).trim().to_string()
                } else {
                    return Some(WorkspaceInfo {
                        name,
                        path: PathBuf::new(),
                    });
                };
                Some(WorkspaceInfo {
                    name,
                    path: PathBuf::from(path),
                })
            })
            .collect()
    }

    fn workspace_prune(&self, _repo_path: &Path) {
        // No-op: jj handles stale workspaces automatically.
    }

    fn is_clean(&self, _path: &Path) -> bool {
        // jj auto-snapshots working copy changes, so the workspace is
        // effectively always clean from a conflict perspective.
        true
    }

    fn fetch(&self, repo_path: &Path, _tmp_dir: &Path) {
        let _ = Command::new("jj")
            .args(["git", "fetch"])
            .current_dir(repo_path)
            .status();
    }

    fn default_branch(&self, repo_path: &Path, _tmp_dir: &Path) -> String {
        let output = Command::new("jj")
            .args(["bookmark", "list"])
            .current_dir(repo_path)
            .output();

        if let Ok(o) = output {
            let stdout = String::from_utf8_lossy(&o.stdout);
            for candidate in &["main", "master", "trunk"] {
                if stdout.lines().any(|l| l.starts_with(candidate)) {
                    return (*candidate).to_string();
                }
            }
        }
        "main".to_string()
    }

    fn is_repo(&self, path: &Path) -> bool {
        path.join(".jj").is_dir()
    }

    fn is_valid_workspace(&self, path: &str) -> bool {
        Path::new(path).join(".jj").is_dir()
    }

    fn check_safety(&self, cmd: &str) -> GitResult {
        // Block bare `jj git push` without explicit bookmark flag.
        if RE_JJ_GIT_PUSH.is_match(cmd) && !RE_JJ_PUSH_BOOKMARK.is_match(cmd) {
            return GitResult::Block(
                "jj git push requires explicit -b/--bookmark flag".to_string(),
            );
        }

        // Block deletion of protected bookmarks.
        if RE_JJ_BOOKMARK_DELETE_MAIN.is_match(cmd) {
            return GitResult::Block(
                "refusing to delete protected bookmark (main/master/trunk)".to_string(),
            );
        }

        // In colocated mode, also enforce git safety for raw git commands.
        if self.colocated {
            let git_result = gitcheck::check_git_safety(cmd);
            if git_result != GitResult::Ok {
                return git_result;
            }
        }

        GitResult::Ok
    }

    fn check_ask(&self, _cmd: &str) -> AskResult {
        // jj has no merge equivalent that needs user confirmation.
        AskResult {
            should_ask: false,
            reason: String::new(),
        }
    }

    fn check_workspace_enforcement(
        &self,
        cmd: &str,
        workspace_active: bool,
        short_id: &str,
    ) -> Option<String> {
        if workspace_active && RE_JJ_EDIT_IMMUTABLE.is_match(cmd) {
            return Some(format!(
                "blocked: jj edit of immutable revision in workspace {short_id}"
            ));
        }

        // In colocated mode, also enforce git worktree rules for raw git commands.
        if self.colocated {
            let git_enforcement =
                gitcheck::check_worktree_enforcement(cmd, workspace_active, short_id, self.kind());
            if git_enforcement.is_some() {
                return git_enforcement;
            }
        }

        None
    }

    fn extract_repo_from_op(&self, cmd: &str) -> Option<String> {
        RE_JJ_REPO_FLAG.captures(cmd).map(|caps| {
            let path_str = caps.get(1).unwrap().as_str();
            Path::new(path_str)
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string()
        })
    }

    fn is_workspace_management_op(&self, cmd: &str) -> bool {
        RE_JJ_WORKSPACE.is_match(cmd)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_jj_safety_bare_push_blocked() {
        let backend = JjBackend { colocated: false };
        let result = backend.check_safety("jj git push");
        assert!(
            matches!(result, GitResult::Block(_)),
            "bare jj git push should be blocked"
        );
    }

    #[test]
    fn test_jj_safety_bookmark_push_allowed() {
        let backend = JjBackend { colocated: false };
        let result = backend.check_safety("jj git push -b feature");
        assert_eq!(result, GitResult::Ok, "push with -b flag should be allowed");
    }

    #[test]
    fn test_jj_safety_delete_main_blocked() {
        let backend = JjBackend { colocated: false };
        let result = backend.check_safety("jj bookmark delete main");
        assert!(
            matches!(result, GitResult::Block(_)),
            "deleting main bookmark should be blocked"
        );
    }

    #[test]
    fn test_jj_safety_delete_feature_allowed() {
        let backend = JjBackend { colocated: false };
        let result = backend.check_safety("jj bookmark delete feature-x");
        assert_eq!(
            result,
            GitResult::Ok,
            "deleting feature bookmark should be allowed"
        );
    }

    #[test]
    fn test_jj_colocated_git_safety() {
        let backend = JjBackend { colocated: true };
        let result = backend.check_safety("git push --force");
        assert!(
            matches!(result, GitResult::Block(_)),
            "colocated mode should block dangerous git commands"
        );
    }
}
