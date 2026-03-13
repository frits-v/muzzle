//! SessionStart hook for Claude Code.
//!
//! Receives JSON on stdin: {"session_id": "uuid", "source": "startup|resume|clear|compact"}
//! On startup: register PID, create changelog, create worktrees, output paths to stdout.
//! On resume: restore state from spec file, re-register PID, output paths.
//! On clear/compact: re-register PID, output existing paths.
//! Crash recovery runs synchronously with bounded timeout after startup completes.

use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use flate2::Compression;
use muzzle::config;
use muzzle::session::{self, SpecEntry, State};
use muzzle::worktree;
use serde::Deserialize;
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

#[derive(Deserialize)]
struct HookInput {
    session_id: Option<String>,
    source: Option<String>,
}

fn main() {
    let result = std::panic::catch_unwind(run);
    if result.is_err() {
        eprintln!("ERROR: session-start hook panicked");
        std::process::exit(1);
    }
}

fn run() {
    // Skip when not running inside the configured workspace
    if !config::is_in_workspace() {
        std::process::exit(0);
    }

    // Read hook input from stdin
    let mut data = String::new();
    if let Err(e) = io::stdin().read_to_string(&mut data) {
        eprintln!("muzzle/session-start: failed to read stdin: {}", e);
        std::process::exit(1);
    }

    let input: HookInput = match serde_json::from_str(&data) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("muzzle/session-start: failed to parse JSON: {}", e);
            std::process::exit(1);
        }
    };

    let session_id = input.session_id.unwrap_or_else(|| "unknown".to_string());
    let source = input.source.unwrap_or_else(|| "startup".to_string());

    // Initialize session state
    let mut sess = session::resolve_with_id(&session_id);
    let timestamp = chrono_utc_now();

    // Register PID marker (all sources)
    if let Err(e) = session::register_pid(&session_id) {
        eprintln!("muzzle/session-start: failed to register PID: {}", e);
        // Non-fatal: continue even if PID registration fails
    }

    // Ensure session temp dir exists
    if let Err(e) = fs::create_dir_all(&sess.tmp_dir) {
        eprintln!("muzzle/session-start: failed to create temp dir: {}", e);
    }

    match source.as_str() {
        "startup" => handle_startup(&mut sess, &timestamp),
        "resume" => handle_resume(&mut sess, &timestamp),
        "clear" | "compact" => handle_clear_compact(&sess, &source, &timestamp),
        _ => handle_startup(&mut sess, &timestamp),
    }

    // Crash recovery: synchronous with bounded 5-second timeout (FR-CR-6)
    let start = Instant::now();
    crash_recovery(&sess, start);
}

fn handle_startup(sess: &mut State, timestamp: &str) {
    // Gzip leftover changelogs from crashed sessions (FR-CR-5)
    gzip_stale_files_excluding(
        &config::workspace(),
        &format!("{}*{}", config::CHANGELOG_PREFIX, config::CHANGELOG_SUFFIX),
        &sess.id,
    );
    gzip_stale_files_excluding(
        &config::workspace(),
        &format!("{}*{}", config::TRACE_PREFIX, config::TRACE_SUFFIX),
        &sess.id,
    );

    // Create fresh changelog
    let header = format!("## Session: {} ({})\n\n", timestamp, sess.id);
    if let Err(e) = fs::write(&sess.changelog_path, &header) {
        eprintln!("muzzle/session-start: failed to create changelog: {}", e);
    }

    // Update convenience symlink
    update_symlink(&sess.id);

    // Create worktrees
    let result = worktree::create(sess);
    if result.failed {
        // H-1: Hard fail — log the error but DON'T fall back to direct-edit mode
        let _ = append_to_changelog(
            &sess.changelog_path,
            &format!(
                "\n### Worktree creation FAILED\n- Error: {}\n- ALL repo writes will be blocked for this session\n",
                result.error
            ),
        );
        eprintln!("muzzle/session-start: {}", result.error);
        emit_context(&format!(
            "\nWARNING: Worktree creation failed. All repo writes will be blocked.\nError: {}",
            result.error
        ));
        return;
    }

    if !result.entries.is_empty() {
        // Save spec file
        if let Err(e) = session::write_spec_file(&sess.spec_file, &result.entries) {
            eprintln!("muzzle/session-start: failed to save spec file: {}", e);
        }
        // Refresh worktree state
        sess.worktree_active = true;
        // Output paths to stdout
        output_worktree_paths(&result.entries);
    }
}

fn handle_resume(sess: &mut State, timestamp: &str) {
    // Restore changelog
    if !restore_changelog(sess) {
        let header = format!(
            "## Session: {} ({})\n\n> **Note**: Original changelog not found — created fresh on resume.\n\n",
            timestamp, sess.id
        );
        let _ = fs::write(&sess.changelog_path, &header);
    }

    // Restore trace log
    let _ = restore_gz_file(
        &config::trace_gz_path(&sess.id),
        &config::trace_path(&sess.id),
    );

    // Append resume marker
    let _ = append_to_changelog(
        &sess.changelog_path,
        &format!("\n---\n### Session resumed: {}\n", timestamp),
    );

    // Update symlink
    update_symlink(&sess.id);

    // Restore worktrees from spec file
    if let Ok(entries) = session::read_spec_file(&sess.spec_file) {
        if !entries.is_empty() {
            let (restored, errors) = worktree::restore_worktrees(sess, &entries);

            for err_msg in &errors {
                let _ = append_to_changelog(
                    &sess.changelog_path,
                    &format!("\n### Worktree restore warning\n- {}\n", err_msg),
                );
            }

            if !restored.is_empty() {
                let _ = session::write_spec_file(&sess.spec_file, &restored);
                sess.worktree_active = true;
                output_worktree_paths(&restored);
            }
        }
    }
}

fn handle_clear_compact(sess: &State, source: &str, timestamp: &str) {
    // Ensure changelog exists
    if !sess.changelog_path.exists() && !restore_changelog_const(sess) {
        let header = format!(
            "## Session: {} ({})\n\n> **Note**: Changelog recreated after {}.\n\n",
            timestamp, sess.id, source
        );
        let _ = fs::write(&sess.changelog_path, &header);
    }

    // Update symlink
    update_symlink(&sess.id);

    // Output existing worktree paths
    if let Ok(entries) = session::read_spec_file(&sess.spec_file) {
        let active: Vec<_> = entries
            .into_iter()
            .filter(|e| Path::new(&e.wt_path).exists())
            .collect();
        if !active.is_empty() {
            output_worktree_paths(&active);
        }
    }
}

fn output_worktree_paths(entries: &[SpecEntry]) {
    if entries.is_empty() {
        return;
    }
    let mut text = String::from("\nActive worktrees for this session:\n");
    for e in entries {
        text.push_str(&format!("  {}: {} (branch: {})\n", e.repo, e.wt_path, e.branch));
    }
    text.push_str("\nUse these worktree paths for ALL file operations (reads, writes, git commands).\n");
    text.push_str("Do NOT use the main checkout directly — use the worktree path above.");
    emit_context(&text);
}

/// Emit text as JSON hookSpecificOutput.additionalContext to stdout.
/// Claude Code injects this as a system-reminder in the session context.
fn emit_context(text: &str) {
    let output = serde_json::json!({
        "hookSpecificOutput": {
            "hookEventName": "SessionStart",
            "additionalContext": text
        }
    });
    println!("{}", output);
}

fn update_symlink(session_id: &str) {
    let symlink = config::changelog_symlink();
    let _ = fs::remove_file(&symlink);
    let target = format!(".claude-changelog-{}.md", session_id);
    let _ = std::os::unix::fs::symlink(&target, &symlink);
}

fn restore_changelog(sess: &mut State) -> bool {
    restore_changelog_const(sess)
}

fn restore_changelog_const(sess: &State) -> bool {
    // Check if .md still exists
    if sess.changelog_path.exists() {
        let _ = fs::remove_file(config::changelog_gz_path(&sess.id));
        return true;
    }

    // Try to ungzip
    restore_gz_file(&config::changelog_gz_path(&sess.id), &sess.changelog_path)
}

fn restore_gz_file(gz_path: &Path, dest_path: &Path) -> bool {
    if dest_path.exists() {
        return true; // Already exists
    }

    let Ok(gz_file) = fs::File::open(gz_path) else {
        return false;
    };

    let mut decoder = GzDecoder::new(gz_file);
    let mut content = Vec::new();
    if decoder.read_to_end(&mut content).is_err() {
        let _ = fs::remove_file(gz_path); // Corrupt
        return false;
    }

    if fs::write(dest_path, &content).is_err() {
        return false;
    }

    let _ = fs::remove_file(gz_path);
    true
}

fn append_to_changelog(path: &Path, entry: &str) -> io::Result<()> {
    let mut f = fs::OpenOptions::new()
        .append(true)
        .create(true)
        .open(path)?;
    write!(f, "{}", entry)?;
    Ok(())
}

fn gzip_stale_files_excluding(dir: &Path, pattern: &str, exclude_session_id: &str) {
    let full_pattern = dir.join(pattern);
    let Ok(matches) = glob_matches(&full_pattern.to_string_lossy()) else {
        return;
    };
    for f in matches {
        if !exclude_session_id.is_empty() {
            if let Some(name) = f.file_name().and_then(|n| n.to_str()) {
                if name.contains(exclude_session_id) {
                    continue;
                }
            }
        }
        let _ = gzip_file(&f);
    }
}

fn glob_matches(pattern: &str) -> Result<Vec<PathBuf>, ()> {
    // Simple glob: split on * and match
    let dir = Path::new(pattern).parent().unwrap_or(Path::new("."));
    let file_pattern = Path::new(pattern)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();

    let Ok(entries) = fs::read_dir(dir) else {
        return Err(());
    };

    let parts: Vec<&str> = file_pattern.split('*').collect();
    let mut matches = Vec::new();

    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        let mut ok = true;
        let mut pos = 0;
        for part in &parts {
            if part.is_empty() {
                continue;
            }
            if let Some(idx) = name[pos..].find(part) {
                pos += idx + part.len();
            } else {
                ok = false;
                break;
            }
        }
        // First part must be prefix, last part must be suffix
        if ok && !parts.is_empty() {
            if !parts[0].is_empty() && !name.starts_with(parts[0]) {
                ok = false;
            }
            if !parts[parts.len() - 1].is_empty() && !name.ends_with(parts[parts.len() - 1]) {
                ok = false;
            }
        }
        if ok {
            matches.push(entry.path());
        }
    }

    Ok(matches)
}

fn gzip_file(path: &Path) -> io::Result<()> {
    let content = fs::read(path)?;
    let gz_path = PathBuf::from(format!("{}.gz", path.display()));

    let gz_file = fs::File::create(&gz_path)?;
    let mut encoder = GzEncoder::new(gz_file, Compression::default());
    encoder.write_all(&content)?;
    encoder.finish()?;

    fs::remove_file(path)?;
    Ok(())
}

fn crash_recovery(sess: &State, start: Instant) {
    let timeout = Duration::from_secs(5);

    // FR-CR-3: Clean stale temp dirs (7 days)
    if start.elapsed() < timeout {
        clean_stale_dirs(
            &config::workspace().join(".claude-tmp"),
            config::STALE_TEMP_DIR_MAX_AGE_DAYS,
        );
    }

    // FR-CR-4: Clean stale PID markers (1 day)
    if start.elapsed() < timeout {
        clean_stale_files(
            &config::pid_marker_dir_path(),
            config::STALE_PID_MARKER_MAX_AGE_DAYS,
        );
    }

    // FR-CR-2: Clean stale spec files (7 days)
    if start.elapsed() < timeout {
        clean_stale_glob(
            &config::workspace(),
            &format!("{}*{}", config::SPEC_FILE_PREFIX, config::SPEC_FILE_SUFFIX),
            config::STALE_SPEC_FILE_MAX_AGE_DAYS,
        );
    }

    // FR-CR-1: Prune orphaned worktrees
    if start.elapsed() < timeout {
        prune_orphan_worktrees(sess);
    }
}

fn clean_stale_dirs(parent_dir: &Path, max_age_days: u64) {
    let Ok(entries) = fs::read_dir(parent_dir) else {
        return;
    };
    let cutoff = SystemTime::now() - Duration::from_secs(max_age_days * 24 * 3600);
    let mut count = 0;

    for entry in entries.flatten() {
        if count >= config::MAX_CLEANUP_ITERATIONS {
            break;
        }
        let Ok(ft) = entry.file_type() else { continue };
        if !ft.is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        if name == "by-pid" {
            continue;
        }
        let Ok(meta) = entry.metadata() else { continue };
        let Ok(modified) = meta.modified() else {
            continue;
        };
        if modified < cutoff {
            let _ = fs::remove_dir_all(entry.path());
            count += 1;
        }
    }
}

fn clean_stale_files(dir: &Path, max_age_days: u64) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let cutoff = SystemTime::now() - Duration::from_secs(max_age_days * 24 * 3600);
    let mut count = 0;

    for entry in entries.flatten() {
        if count >= config::MAX_CLEANUP_ITERATIONS {
            break;
        }
        let Ok(ft) = entry.file_type() else { continue };
        if ft.is_dir() {
            continue;
        }
        let Ok(meta) = entry.metadata() else { continue };
        let Ok(modified) = meta.modified() else {
            continue;
        };
        if modified < cutoff {
            let _ = fs::remove_file(entry.path());
            count += 1;
        }
    }
}

fn clean_stale_glob(dir: &Path, pattern: &str, max_age_days: u64) {
    let full_pattern = dir.join(pattern);
    let Ok(matches) = glob_matches(&full_pattern.to_string_lossy()) else {
        return;
    };
    let cutoff = SystemTime::now() - Duration::from_secs(max_age_days * 24 * 3600);
    let mut count = 0;

    for f in matches {
        if count >= config::MAX_CLEANUP_ITERATIONS {
            break;
        }
        let Ok(meta) = fs::metadata(&f) else { continue };
        let Ok(modified) = meta.modified() else {
            continue;
        };
        if modified < cutoff {
            let _ = fs::remove_file(&f);
            count += 1;
        }
    }
}

fn prune_orphan_worktrees(sess: &State) {
    let workspace = config::workspace();
    let Ok(entries) = fs::read_dir(&workspace) else {
        return;
    };

    let cutoff =
        SystemTime::now() - Duration::from_secs(config::ORPHAN_WORKTREE_MAX_AGE_HOURS * 3600);
    let mut count = 0;

    for entry in entries.flatten() {
        if count >= config::MAX_CLEANUP_ITERATIONS {
            break;
        }
        let Ok(ft) = entry.file_type() else { continue };
        if !ft.is_dir() {
            continue;
        }

        let repo_path = entry.path();
        if !repo_path.join(".git").exists() {
            continue;
        }

        let wt_parent = repo_path.join(".worktrees");
        let Ok(wt_entries) = fs::read_dir(&wt_parent) else {
            continue;
        };

        let active_worktrees = worktree::get_active_worktrees(&repo_path);

        for wte in wt_entries.flatten() {
            if count >= config::MAX_CLEANUP_ITERATIONS {
                break;
            }
            let Ok(ft) = wte.file_type() else { continue };
            if !ft.is_dir() {
                continue;
            }

            let wt_path = wte.path();
            let name = wte.file_name().to_string_lossy().to_string();

            // Skip current session's worktree
            if name == sess.short_id {
                continue;
            }

            // Check age
            let Ok(meta) = wte.metadata() else { continue };
            let Ok(modified) = meta.modified() else {
                continue;
            };
            if modified >= cutoff {
                continue;
            }

            // Check if it's an active git worktree
            let wt_str = wt_path.to_string_lossy().to_string();
            let is_active = active_worktrees
                .iter()
                .any(|ap| ap.trim_end_matches('/') == wt_str.trim_end_matches('/'));

            if !is_active {
                let _ = fs::remove_dir_all(&wt_path);
                count += 1;
            }
        }

        // Prune stale worktree metadata
        worktree::prune_stale_worktrees(&repo_path);
        worktree::clean_empty_worktree_dirs(&repo_path);

        // Clean orphaned wt/ branches
        clean_orphaned_wt_branches(&repo_path);
    }
}

fn clean_orphaned_wt_branches(repo_path: &Path) {
    let repo_str = repo_path.to_string_lossy().to_string();

    let Ok(branch_output) =
        worktree::run_git_output(&["-C", &repo_str, "branch", "--list", "wt/*"])
    else {
        return;
    };

    let active_output =
        worktree::run_git_output(&["-C", &repo_str, "worktree", "list", "--porcelain"])
            .unwrap_or_default();

    for line in branch_output.lines() {
        let branch = line.trim();
        if branch.is_empty() {
            continue;
        }
        let search = format!("branch refs/heads/{}", branch);
        if !active_output.contains(&search) {
            let _ = worktree::run_git_output(&["-C", &repo_str, "branch", "-D", branch]);
        }
    }
}

/// UTC timestamp without chrono dependency.
fn chrono_utc_now() -> String {
    use std::time::SystemTime;
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = now.as_secs();
    let days = secs / 86400;
    let time_secs = secs % 86400;
    let hours = time_secs / 3600;
    let minutes = (time_secs % 3600) / 60;
    let seconds = time_secs % 60;

    let z = days as i64 + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u32;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };

    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
        y, m, d, hours, minutes, seconds
    )
}
