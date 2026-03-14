//! On-demand worktree creation binary.
//!
//! Usage: `ensure-worktree <repo-name>`
//!
//! Resolves the current session, creates a worktree for the given repo
//! (idempotent — reuses existing), updates the spec file, and prints
//! the worktree path to stdout.
//!
//! Exit codes:
//!   0 — success (worktree path on stdout)
//!   1 — error (message on stderr)

use muzzle::config;
use muzzle::session;
use muzzle::worktree;

fn main() {
    let result = std::panic::catch_unwind(run);
    match result {
        Ok(()) => {}
        Err(_) => {
            eprintln!("ERROR: ensure-worktree panicked — aborting for safety");
            std::process::exit(1);
        }
    }
}

fn run() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 2 || args[1].is_empty() {
        eprintln!("Usage: ensure-worktree <repo-name>");
        std::process::exit(1);
    }

    let repo = &args[1];

    // Validate workspace exists before attempting anything
    if let Err(msg) = config::validate_workspace() {
        eprintln!("ERROR: {}", msg);
        std::process::exit(1);
    }

    // Resolve session (read-write mode — this binary is invoked as a Bash command)
    let sess = session::resolve();
    if !sess.has_session() {
        eprintln!(
            "ERROR: No active session found. \
             ensure-worktree must run inside a Claude Code session."
        );
        std::process::exit(1);
    }

    // Check if already in spec file (idempotent fast path)
    if let Ok(entries) = session::read_spec_file(&sess.spec_file) {
        if let Some(existing) = entries.iter().find(|e| e.repo == *repo) {
            // Already registered — print path and exit
            println!("{}", existing.wt_path);
            return;
        }
    }

    // Create worktree
    let entry = match worktree::ensure_for_repo(&sess, repo) {
        Ok(entry) => entry,
        Err(e) => {
            eprintln!("ERROR: Failed to create worktree for {}: {}", repo, e);
            std::process::exit(1);
        }
    };

    // Update spec file
    if let Err(e) = session::append_spec_entry(&sess.spec_file, &entry) {
        eprintln!(
            "ERROR: Worktree created but failed to update spec file: {}",
            e
        );
        // Still print the path — the worktree exists even if spec write failed
        println!("{}", entry.wt_path);
        std::process::exit(1);
    }

    println!("{}", entry.wt_path);
}
