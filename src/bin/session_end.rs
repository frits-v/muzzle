//! SessionEnd hook for Claude Code.
//!
//! Receives JSON on stdin: {"session_id": "uuid"}
//! Removes worktrees (warns on dirty), gzips changelog/trace, cleans PID markers.

use flate2::write::GzEncoder;
use flate2::Compression;
use hooks_v3::config;
use hooks_v3::session::{self, SpecEntry};
use hooks_v3::worktree;
use serde::Deserialize;
use std::fs;
use std::io::{self, Read, Write};
use std::path::Path;
use std::process::Command;

#[derive(Deserialize)]
struct HookInput {
    session_id: Option<String>,
}

fn main() {
    // Cleanup is best-effort — panic should not block session exit
    let _ = std::panic::catch_unwind(run);
}

fn run() {
    // Skip for non-cn workspaces
    if !config::is_in_workspace() {
        std::process::exit(0);
    }

    // Read stdin
    let mut data = String::new();
    if io::stdin().read_to_string(&mut data).is_err() {
        std::process::exit(0);
    }

    let input: HookInput = match serde_json::from_str(&data) {
        Ok(v) => v,
        Err(_) => std::process::exit(0),
    };

    let session_id = match input.session_id {
        Some(id) if !id.is_empty() => id,
        _ => std::process::exit(0),
    };

    let sess = session::resolve_with_id(&session_id);

    // Remove worktrees (FR-SL-5)
    remove_worktrees(&sess);

    // Gzip changelog (FR-SL-5)
    gzip_file_if_exists(&sess.changelog_path);

    // Gzip trace log (FR-SL-5)
    gzip_file_if_exists(&config::trace_path(&sess.id));

    // Clean PID markers (FR-SL-5)
    clean_pid_markers();

    // Clean empty .worktrees/ dirs
    clean_empty_worktree_dirs();
}

fn remove_worktrees(sess: &session::State) {
    let Ok(entries) = session::read_spec_file(&sess.spec_file) else {
        return;
    };

    for entry in &entries {
        let (dirty, err) = worktree::remove(entry);
        if dirty {
            eprintln!(
                "WARN: Worktree {} has uncommitted changes — skipping removal",
                entry.wt_path
            );
            let _ = append_to_changelog(
                &sess.changelog_path,
                &format!(
                    "\n### WARNING: Uncommitted worktree left behind\n- Path: {}\n- Cleanup: `git -C {} worktree remove --force {}`\n",
                    entry.wt_path, entry.repo_path, entry.wt_path,
                ),
            );
            continue;
        }
        if let Some(e) = err {
            eprintln!("WARN: Failed to remove worktree {}: {}", entry.wt_path, e);
        }
    }

    // Prune stale metadata for each unique repo
    let repos = unique_repos(&entries);
    for repo_path in &repos {
        worktree::prune_stale_worktrees(Path::new(repo_path));
        worktree::clean_empty_worktree_dirs(Path::new(repo_path));
    }

    // Remove the spec file
    let _ = fs::remove_file(&sess.spec_file);
}

fn clean_pid_markers() {
    let marker_dir = config::pid_marker_dir_path();
    let mut pid = std::os::unix::process::parent_id();

    for _ in 0..config::PPID_WALK_DEPTH {
        if pid <= 1 {
            break;
        }
        let _ = fs::remove_file(marker_dir.join(pid.to_string()));

        // Walk to parent
        let Ok(output) = Command::new("ps")
            .args(["-o", "ppid=", "-p", &pid.to_string()])
            .output()
        else {
            break;
        };
        let ppid_str = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let Ok(parent_pid) = ppid_str.parse::<u32>() else {
            break;
        };
        pid = parent_pid;
    }
}

fn clean_empty_worktree_dirs() {
    let workspace = config::workspace();
    let Ok(entries) = fs::read_dir(&workspace) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(ft) = entry.file_type() else { continue };
        if !ft.is_dir() {
            continue;
        }
        let repo_path = entry.path();
        if !repo_path.join(".git").exists() {
            continue;
        }
        worktree::clean_empty_worktree_dirs(&repo_path);
    }
}

fn unique_repos(entries: &[SpecEntry]) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut result = Vec::new();
    for e in entries {
        if seen.insert(e.repo_path.clone()) {
            result.push(e.repo_path.clone());
        }
    }
    result
}

fn gzip_file_if_exists(path: &Path) {
    let Ok(content) = fs::read(path) else {
        return;
    };

    let gz_path = path.with_extension(
        path.extension()
            .map(|ext| format!("{}.gz", ext.to_string_lossy()))
            .unwrap_or_else(|| "gz".to_string()),
    );

    let Ok(gz_file) = fs::File::create(&gz_path) else {
        return;
    };

    let mut encoder = GzEncoder::new(gz_file, Compression::default());
    if encoder.write_all(&content).is_err() {
        let _ = fs::remove_file(&gz_path);
        return;
    }
    if encoder.finish().is_err() {
        let _ = fs::remove_file(&gz_path);
        return;
    }

    let _ = fs::remove_file(path);
}

fn append_to_changelog(path: &Path, entry: &str) -> io::Result<()> {
    let mut f = fs::OpenOptions::new()
        .append(true)
        .create(true)
        .open(path)?;
    write!(f, "{}", entry)?;
    Ok(())
}
