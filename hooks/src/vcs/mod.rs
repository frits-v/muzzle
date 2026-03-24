//! VCS backend abstraction layer.
//!
//! Provides a trait-based interface for version control operations, enabling
//! muzzle to work with both Git and Jujutsu (jj) repositories. The [`detect`]
//! function probes a directory for `.jj/` and `.git/` markers to determine
//! which backend to use.

pub mod git;
pub mod jj;

use crate::gitcheck::{AskResult, GitResult};
use crate::session::SpecEntry;
use std::path::{Path, PathBuf};

/// The kind of version control system detected for a repository.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum VcsKind {
    /// Standard Git repository.
    #[default]
    Git,
    /// Pure Jujutsu repository (no colocated Git).
    Jj,
    /// Jujutsu repository colocated with Git (both `.jj/` and `.git/` present).
    JjColocated,
}

impl std::fmt::Display for VcsKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VcsKind::Git => write!(f, "git"),
            VcsKind::Jj => write!(f, "jj"),
            VcsKind::JjColocated => write!(f, "jj-coloc"),
        }
    }
}

impl std::str::FromStr for VcsKind {
    type Err = String;

    /// Parse a [`VcsKind`] from its string representation.
    ///
    /// Accepts the same strings produced by [`Display`]: `"git"`, `"jj"`,
    /// `"jj-coloc"`. Returns `Err` with a descriptive message for unknown
    /// values.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "git" => Ok(VcsKind::Git),
            "jj" => Ok(VcsKind::Jj),
            "jj-coloc" => Ok(VcsKind::JjColocated),
            other => Err(format!("unknown VCS kind: {other:?}")),
        }
    }
}

/// Information about a VCS workspace (worktree in Git, workspace in jj).
#[derive(Debug, Clone)]
pub struct WorkspaceInfo {
    /// Workspace name (e.g. branch name or jj workspace identifier).
    pub name: String,
    /// Absolute path to the workspace directory.
    pub path: PathBuf,
}

/// Trait abstracting version control operations for workspace management and
/// safety checks.
///
/// Implementations exist for Git ([`git`] module) and Jujutsu ([`jj`] module).
/// The trait is used as a generic bound, not as a trait object.
pub trait VcsBackend {
    /// Return the VCS kind this backend handles.
    fn kind(&self) -> VcsKind;

    /// Create a new workspace (worktree) for the given repository.
    fn workspace_add(
        &self,
        repo_path: &Path,
        dest: &Path,
        session_id: &str,
        branch: Option<&str>,
        tmp_dir: &Path,
    ) -> Result<SpecEntry, String>;

    /// Remove a workspace. Returns `(success, optional_error_message)`.
    fn workspace_remove(&self, entry: &SpecEntry, force: bool) -> (bool, Option<String>);

    /// List all workspaces for a repository.
    fn workspace_list(&self, repo_path: &Path) -> Vec<WorkspaceInfo>;

    /// Prune stale workspace references.
    fn workspace_prune(&self, repo_path: &Path);

    /// Check whether the working directory is clean (no uncommitted changes).
    fn is_clean(&self, path: &Path) -> bool;

    /// Fetch updates from the remote.
    fn fetch(&self, repo_path: &Path, tmp_dir: &Path);

    /// Determine the default branch for a repository.
    fn default_branch(&self, repo_path: &Path, tmp_dir: &Path) -> String;

    /// Check whether the given path is a repository of this VCS type.
    fn is_repo(&self, path: &Path) -> bool;

    /// Check whether a workspace path string refers to a valid workspace.
    fn is_valid_workspace(&self, path: &str) -> bool;

    /// Run safety checks on a shell command string.
    fn check_safety(&self, cmd: &str) -> GitResult;

    /// Check whether a command should prompt the user for confirmation.
    fn check_ask(&self, cmd: &str) -> AskResult;

    /// Enforce workspace isolation for VCS commands.
    ///
    /// Returns `Some(denial_message)` if the command should be blocked,
    /// `None` if it is allowed.
    fn check_workspace_enforcement(
        &self,
        cmd: &str,
        workspace_active: bool,
        short_id: &str,
    ) -> Option<String>;

    /// Extract the repository name from a VCS operation command, if present.
    fn extract_repo_from_op(&self, cmd: &str) -> Option<String>;

    /// Check whether a command is a workspace/worktree management operation.
    fn is_workspace_management_op(&self, cmd: &str) -> bool;
}

/// Detect the VCS kind for a directory by probing for marker directories.
///
/// Checks for `.jj/` and `.git/` in the given path:
/// - Both present: [`VcsKind::JjColocated`]
/// - Only `.jj/`: [`VcsKind::Jj`]
/// - Only `.git/` or neither: [`VcsKind::Git`] (default)
pub fn detect(path: &Path) -> VcsKind {
    let has_jj = path.join(".jj").is_dir();
    let has_git = path.join(".git").exists(); // .git can be a file (worktree) or dir

    match (has_jj, has_git) {
        (true, true) => VcsKind::JjColocated,
        (true, false) => VcsKind::Jj,
        _ => VcsKind::Git,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn test_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("muzzle-test-vcs-{name}"));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn test_detect_git_only() {
        let tmp = test_dir("git-only");
        fs::create_dir(tmp.join(".git")).unwrap();
        assert_eq!(detect(&tmp), VcsKind::Git);
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_detect_jj_only() {
        let tmp = test_dir("jj-only");
        fs::create_dir(tmp.join(".jj")).unwrap();
        assert_eq!(detect(&tmp), VcsKind::Jj);
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_detect_colocated() {
        let tmp = test_dir("colocated");
        fs::create_dir(tmp.join(".jj")).unwrap();
        fs::create_dir(tmp.join(".git")).unwrap();
        assert_eq!(detect(&tmp), VcsKind::JjColocated);
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_detect_neither() {
        let tmp = test_dir("neither");
        assert_eq!(detect(&tmp), VcsKind::Git);
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_vcs_kind_display_roundtrip() {
        for kind in [VcsKind::Git, VcsKind::Jj, VcsKind::JjColocated] {
            let s = kind.to_string();
            let parsed: VcsKind = s.parse().unwrap();
            assert_eq!(kind, parsed, "roundtrip failed for {kind:?}");
        }
    }
}
