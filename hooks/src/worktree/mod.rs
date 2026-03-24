//! Git worktree creation, removal, and management.
//!
//! FR-WT-1 through FR-WT-7: Worktree creation with retry, rollback,
//! default branch resolution, CLAUDE_WORKTREES parsing, auto-sandbox.

pub mod cleanup;
pub mod git;

use crate::config;
use crate::session::{SpecEntry, State};
use crate::vcs::VcsKind;
use std::fs;
use std::path::Path;

// Re-export commonly used items for external consumers.
pub use cleanup::{clean_empty_worktree_dirs, prune_stale_worktrees, remove};
pub use git::{get_active_worktrees, is_git_repo, run_git_output};

/// Error type for worktree operations.
#[derive(Debug)]
pub enum WorktreeError {
    /// Worktree creation failed (after retry).
    CreateFailed(String),
    /// Partial creation requires rollback of already-created worktrees.
    RollbackNeeded(String),
    /// Underlying filesystem I/O error.
    IoError(std::io::Error),
}

impl std::fmt::Display for WorktreeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WorktreeError::CreateFailed(msg) => write!(f, "{}", msg),
            WorktreeError::RollbackNeeded(msg) => write!(f, "{}", msg),
            WorktreeError::IoError(e) => write!(f, "I/O error: {}", e),
        }
    }
}

impl From<std::io::Error> for WorktreeError {
    fn from(e: std::io::Error) -> Self {
        WorktreeError::IoError(e)
    }
}

/// Outcome of worktree creation.
pub struct CreateResult {
    /// Successfully created worktree spec entries.
    pub entries: Vec<SpecEntry>,
    /// True if creation failed (all worktrees rolled back).
    pub failed: bool,
    /// Human-readable error message on failure.
    pub error: String,
}

/// Resolve a repo name to a path by searching all configured workspaces.
///
/// Checks `<workspace>/<repo>` for each workspace; returns the first existing directory.
fn resolve_repo_path(repo: &str) -> Option<std::path::PathBuf> {
    for ws in config::workspaces() {
        let candidate = ws.join(repo);
        if candidate.is_dir() {
            return Some(candidate);
        }
    }
    None
}

/// Create worktrees for the session.
///
/// If CLAUDE_WORKTREES is set, uses those specs.
/// Otherwise, auto-sandboxes the primary repo under PWD.
/// On failure after retry: rolls back all created worktrees and returns failed=true.
pub fn create(sess: &State) -> CreateResult {
    if let Ok(env_specs) = std::env::var("CLAUDE_WORKTREES") {
        if !env_specs.is_empty() {
            return create_from_env(sess, &env_specs);
        }
    }
    create_auto_sandbox(sess)
}

/// Parse CLAUDE_WORKTREES=repo:branch,repo2:branch2
fn create_from_env(sess: &State, env_specs: &str) -> CreateResult {
    let mut result = CreateResult {
        entries: Vec::new(),
        failed: false,
        error: String::new(),
    };

    for spec in env_specs.split(',') {
        let spec = spec.trim();
        if spec.is_empty() {
            continue;
        }

        let parts: Vec<&str> = spec.splitn(2, ':').collect();
        let repo = parts[0];
        let branch = if parts.len() > 1 && !parts[1].is_empty() {
            Some(parts[1].to_string())
        } else {
            None
        };

        let repo_path = match resolve_repo_path(repo) {
            Some(p) => p,
            None => {
                crate::log::emit_full(
                    "WARN",
                    "session-start",
                    &format!("skipping {} — not found in any workspace", repo),
                    None,
                    None,
                );
                continue;
            }
        };
        if !git::is_git_repo(&repo_path) {
            crate::log::emit_full(
                "WARN",
                "session-start",
                &format!("skipping {} — not a git repo", repo),
                None,
                Some(&repo_path.display().to_string()),
            );
            continue;
        }

        match create_single_worktree(sess, repo, &repo_path, branch.as_deref()) {
            Ok(entry) => result.entries.push(entry),
            Err(e) => {
                // Rollback all created worktrees
                cleanup::rollback(&result.entries);
                result.failed = true;
                result.error = format!("Worktree creation failed for {}: {}", repo, e);
                result.entries.clear();
                return result;
            }
        }
    }

    result
}

/// Walk up from PWD to find a git repo under workspace.
fn create_auto_sandbox(sess: &State) -> CreateResult {
    let mut result = CreateResult {
        entries: Vec::new(),
        failed: false,
        error: String::new(),
    };

    let pwd = match std::env::current_dir() {
        Ok(p) => p,
        Err(e) => {
            result.failed = true;
            result.error = format!("Cannot determine PWD: {}", e);
            return result;
        }
    };

    let workspaces = config::workspaces();
    let mut check_dir = pwd.clone();
    let mut auto_repo: Option<String> = None;
    let mut found_workspace: Option<std::path::PathBuf> = None;

    // Walk up from PWD to find a git repo within any configured workspace.
    while workspaces.iter().any(|ws| config::is_under(&check_dir, ws))
        && !workspaces.contains(&check_dir)
    {
        if git::is_git_repo(&check_dir) {
            auto_repo = check_dir
                .file_name()
                .map(|n| n.to_string_lossy().to_string());
            found_workspace = config::workspace_for_path(&check_dir);
            break;
        }
        check_dir = match check_dir.parent() {
            Some(p) => p.to_path_buf(),
            None => break,
        };
    }

    let Some(repo) = auto_repo else {
        // Not inside a git repo — no worktrees needed
        return result;
    };

    let repo_path = found_workspace
        .unwrap_or_else(config::workspace)
        .join(&repo);
    match create_single_worktree(sess, &repo, &repo_path, None) {
        Ok(entry) => result.entries.push(entry),
        Err(e) => {
            // H-1: Hard fail — no silent fallback to direct edit mode
            result.failed = true;
            result.error = format!("Auto-sandbox failed for {}: {}", repo, e);
        }
    }

    result
}

/// Create one worktree with retry logic (FR-WT-4).
fn create_single_worktree(
    sess: &State,
    repo: &str,
    repo_path: &Path,
    branch: Option<&str>,
) -> Result<SpecEntry, WorktreeError> {
    let wt_path = config::worktree_path(repo_path, &sess.short_id);

    // Ensure .worktrees dir exists
    fs::create_dir_all(config::worktree_dir(repo_path))?;

    // Fetch and resolve default branch
    let default_branch = git::fetch_and_resolve_default_branch(repo_path, &sess.tmp_dir);

    // Determine branch strategy (FR-WT-3)
    let (actual_branch, args) = match branch {
        None => {
            // No branch specified: create ephemeral wt/<short-id>
            let br = format!("wt/{}", sess.short_id);
            let args = vec![
                "-C".into(),
                repo_path.to_string_lossy().into(),
                "worktree".into(),
                "add".into(),
                "-b".into(),
                br.clone(),
                wt_path.to_string_lossy().into(),
                format!("origin/{}", default_branch),
            ];
            (br, args)
        }
        Some(b) if b == default_branch => {
            // Default branch requested: redirect to ephemeral (don't lock default)
            let br = format!("wt/{}", sess.short_id);
            let args = vec![
                "-C".into(),
                repo_path.to_string_lossy().into(),
                "worktree".into(),
                "add".into(),
                "-b".into(),
                br.clone(),
                wt_path.to_string_lossy().into(),
                format!("origin/{}", default_branch),
            ];
            (br, args)
        }
        Some(b) if git::branch_exists(repo_path, b) => {
            // Existing branch: check it out
            let args = vec![
                "-C".into(),
                repo_path.to_string_lossy().into(),
                "worktree".into(),
                "add".into(),
                wt_path.to_string_lossy().into(),
                b.into(),
            ];
            (b.to_string(), args)
        }
        Some(b) => {
            // New branch: create from origin/<default>
            let args = vec![
                "-C".into(),
                repo_path.to_string_lossy().into(),
                "worktree".into(),
                "add".into(),
                "-b".into(),
                b.into(),
                wt_path.to_string_lossy().into(),
                format!("origin/{}", default_branch),
            ];
            (b.to_string(), args)
        }
    };

    // First attempt
    if git::run_git_strings(&args).is_err() {
        // FR-WT-4: Retry after prune
        let rp = repo_path.to_string_lossy().to_string();
        let _ = git::run_git(&["-C", &rp, "worktree", "prune"]);
        if let Err(e) = git::run_git_strings(&args) {
            // FR-WT-5: Clean up partial worktree
            let _ = fs::remove_dir_all(&wt_path);
            return Err(WorktreeError::CreateFailed(format!(
                "git worktree add failed (after retry): {}",
                e
            )));
        }
    }

    Ok(SpecEntry {
        repo: repo.to_string(),
        branch: actual_branch,
        wt_path: wt_path.to_string_lossy().to_string(),
        repo_path: repo_path.to_string_lossy().to_string(),
        vcs_kind: VcsKind::Git,
    })
}

/// Create a worktree on-demand for a single repo.
///
/// Used by the `ensure-worktree` binary for lazy worktree creation.
/// Validates the repo is a git repo under workspace, then delegates to
/// `create_single_worktree()` with no branch (ephemeral `wt/<short-id>`).
/// Returns the spec entry for the created worktree.
pub fn ensure_for_repo(sess: &State, repo: &str) -> Result<SpecEntry, WorktreeError> {
    let repo_path = resolve_repo_path(repo).ok_or_else(|| {
        let searched: Vec<_> = config::workspaces()
            .iter()
            .map(|ws| ws.join(repo).display().to_string())
            .collect();
        WorktreeError::CreateFailed(format!(
            "{} not found in any workspace. Searched: {}",
            repo,
            searched.join(", ")
        ))
    })?;
    if !git::is_git_repo(&repo_path) {
        return Err(WorktreeError::CreateFailed(format!(
            "{} is not a git repo at {}",
            repo,
            repo_path.display()
        )));
    }

    // Check if worktree already exists (idempotent)
    let wt_path = config::worktree_path(&repo_path, &sess.short_id);
    if wt_path.exists() && git::is_valid_worktree(&wt_path.to_string_lossy()) {
        // Return existing entry without re-creating
        let branch = format!("wt/{}", sess.short_id);
        return Ok(SpecEntry {
            repo: repo.to_string(),
            branch,
            wt_path: wt_path.to_string_lossy().to_string(),
            repo_path: repo_path.to_string_lossy().to_string(),
            vcs_kind: VcsKind::Git,
        });
    }

    create_single_worktree(sess, repo, &repo_path, None)
}

/// Restore worktrees from a spec file for session resume.
pub fn restore_worktrees(sess: &State, entries: &[SpecEntry]) -> (Vec<SpecEntry>, Vec<String>) {
    let mut restored = Vec::new();
    let mut errors = Vec::new();

    for entry in entries {
        // If worktree already exists and is valid, keep it
        if git::is_valid_worktree(&entry.wt_path) {
            restored.push(entry.clone());
            continue;
        }

        // Source repo must exist
        let repo_path = Path::new(&entry.repo_path);
        if !git::is_git_repo(repo_path) {
            errors.push(format!(
                "Skipping {} — source repo gone at {}",
                entry.repo, entry.repo_path
            ));
            continue;
        }

        // Fetch origin
        git::fetch_origin(repo_path, &sess.tmp_dir);

        // Prune stale metadata
        let repo_str = repo_path.to_string_lossy().to_string();
        let _ = git::run_git(&["-C", &repo_str, "worktree", "prune"]);

        // Ensure parent dir
        if let Some(parent) = Path::new(&entry.wt_path).parent() {
            let _ = fs::create_dir_all(parent);
        }

        let result = if git::branch_exists(repo_path, &entry.branch) {
            git::run_git(&[
                "-C",
                &repo_str,
                "worktree",
                "add",
                &entry.wt_path,
                &entry.branch,
            ])
        } else if entry.branch.starts_with("wt/") {
            // Ephemeral branch was deleted — recreate from origin/default
            let default_branch = git::fetch_and_resolve_default_branch(repo_path, &sess.tmp_dir);
            git::run_git(&[
                "-C",
                &repo_str,
                "worktree",
                "add",
                "-b",
                &entry.branch,
                &entry.wt_path,
                &format!("origin/{}", default_branch),
            ])
        } else {
            // Branch doesn't exist — create from origin/default
            let default_branch = git::fetch_and_resolve_default_branch(repo_path, &sess.tmp_dir);
            git::run_git(&[
                "-C",
                &repo_str,
                "worktree",
                "add",
                "-b",
                &entry.branch,
                &entry.wt_path,
                &format!("origin/{}", default_branch),
            ])
        };

        match result {
            Ok(()) => restored.push(entry.clone()),
            Err(e) => errors.push(format!(
                "Failed to restore {}:{} — {}",
                entry.repo, entry.branch, e
            )),
        }
    }

    (restored, errors)
}
