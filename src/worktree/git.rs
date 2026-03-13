//! Git helper operations for worktree management.
//!
//! Run commands, resolve default branches, check branch existence, fetch origin.

use std::fs;
use std::path::Path;
use std::process::Command;

/// Run a git command (with &str args) and return Ok(()) or Err(error message).
pub fn run_git(args: &[&str]) -> Result<(), String> {
    run_git_generic(args)
}

/// Run a git command (with String args) and return Ok(()) or Err(error message).
pub fn run_git_strings(args: &[String]) -> Result<(), String> {
    let refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    run_git_generic(&refs)
}

fn run_git_generic(args: &[&str]) -> Result<(), String> {
    let status = Command::new("git")
        .args(args)
        .stderr(std::process::Stdio::piped())
        .status()
        .map_err(|e| format!("failed to run git: {}", e))?;

    if status.success() {
        Ok(())
    } else {
        Err(format!("git {} exited with {}", args.join(" "), status))
    }
}

/// Run a git command and capture stdout.
pub fn run_git_output(args: &[&str]) -> Result<String, String> {
    let output = Command::new("git")
        .args(args)
        .output()
        .map_err(|e| format!("failed to run git: {}", e))?;

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

/// Check if a directory is a git repository.
pub fn is_git_repo(path: &Path) -> bool {
    path.join(".git").exists()
}

/// Check if a path is a valid worktree checkout.
pub fn is_valid_worktree(path: &str) -> bool {
    Path::new(path).join(".git").exists()
}

/// Check if a branch exists locally or on origin.
pub fn branch_exists(repo_path: &Path, branch: &str) -> bool {
    let repo_str = repo_path.to_string_lossy().to_string();
    if run_git(&[
        "-C",
        &repo_str,
        "show-ref",
        "--verify",
        "--quiet",
        &format!("refs/heads/{}", branch),
    ])
    .is_ok()
    {
        return true;
    }
    if run_git(&[
        "-C",
        &repo_str,
        "show-ref",
        "--verify",
        "--quiet",
        &format!("refs/remotes/origin/{}", branch),
    ])
    .is_ok()
    {
        return true;
    }
    false
}

/// Resolve the default branch (FR-WT-2).
/// Order: gh API -> origin/HEAD -> origin/main|master -> current HEAD
pub fn fetch_and_resolve_default_branch(repo_path: &Path, tmp_dir: &Path) -> String {
    let repo_str = repo_path.to_string_lossy().to_string();

    // 1. Try gh API (authoritative)
    if let Ok(output) = Command::new("gh")
        .args([
            "repo",
            "view",
            "--json",
            "defaultBranchRef",
            "--jq",
            ".defaultBranchRef.name",
        ])
        .env("GIT_DIR", format!("{}/.git", repo_str))
        .current_dir(repo_path)
        .output()
    {
        if output.status.success() {
            let branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !branch.is_empty() {
                fetch_origin(repo_path, tmp_dir);
                return branch;
            }
        }
    }

    // 2. Fallback: refs/remotes/origin/HEAD
    if let Ok(output) = Command::new("git")
        .args(["-C", &repo_str, "symbolic-ref", "refs/remotes/origin/HEAD"])
        .output()
    {
        if output.status.success() {
            let full = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if let Some(branch) = full.strip_prefix("refs/remotes/origin/") {
                if !branch.is_empty() {
                    fetch_origin(repo_path, tmp_dir);
                    return branch.to_string();
                }
            }
        }
    }

    // 3. Fallback: origin/main or origin/master
    if run_git(&[
        "-C",
        &repo_str,
        "show-ref",
        "--verify",
        "--quiet",
        "refs/remotes/origin/main",
    ])
    .is_ok()
    {
        fetch_origin(repo_path, tmp_dir);
        return "main".to_string();
    }
    if run_git(&[
        "-C",
        &repo_str,
        "show-ref",
        "--verify",
        "--quiet",
        "refs/remotes/origin/master",
    ])
    .is_ok()
    {
        fetch_origin(repo_path, tmp_dir);
        return "master".to_string();
    }

    // 4. Fallback: current HEAD
    if let Ok(output) = Command::new("git")
        .args(["-C", &repo_str, "rev-parse", "--abbrev-ref", "HEAD"])
        .output()
    {
        if output.status.success() {
            let branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !branch.is_empty() && branch != "HEAD" {
                fetch_origin(repo_path, tmp_dir);
                return branch;
            }
        }
    }

    fetch_origin(repo_path, tmp_dir);
    "master".to_string() // last resort
}

/// Run git fetch origin --prune (non-fatal).
pub fn fetch_origin(repo_path: &Path, tmp_dir: &Path) {
    let repo_str = repo_path.to_string_lossy().to_string();
    let err_log = tmp_dir.join("fetch-error.log");

    let mut cmd = Command::new("git");
    cmd.args(["-C", &repo_str, "fetch", "origin", "--prune"]);

    if let Ok(f) = fs::File::create(&err_log) {
        cmd.stderr(f);
    }
    let _ = cmd.status();
}

/// Get the list of active worktree paths from git.
pub fn get_active_worktrees(repo_path: &Path) -> Vec<String> {
    let repo_str = repo_path.to_string_lossy().to_string();
    let Ok(output) = run_git_output(&["-C", &repo_str, "worktree", "list", "--porcelain"]) else {
        return Vec::new();
    };

    output
        .lines()
        .filter_map(|line| line.strip_prefix("worktree ").map(|s| s.to_string()))
        .collect()
}
