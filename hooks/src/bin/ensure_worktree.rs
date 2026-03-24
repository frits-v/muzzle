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
use muzzle::vcs::VcsKind;
use muzzle::worktree;

fn main() {
    let result = std::panic::catch_unwind(run);
    match result {
        Ok(()) => {}
        Err(_) => {
            muzzle::log::error("ensure-worktree", "panicked — aborting for safety");
            std::process::exit(1);
        }
    }
}

fn run() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 || args[1].is_empty() {
        muzzle::log::error(
            "ensure-worktree",
            "Usage: ensure-worktree <repo-name> [vcs-kind]",
        );
        std::process::exit(1);
    }

    let repo = &args[1];

    // Parse optional VCS kind from second argument (default: Git)
    let vcs_kind: VcsKind = args.get(2).and_then(|s| s.parse().ok()).unwrap_or_default();

    // Validate all workspaces exist before attempting anything
    if let Err(msg) = config::validate_workspaces() {
        muzzle::log::error("ensure-worktree", &msg);
        std::process::exit(1);
    }

    // Ensure state directory for spec files
    if let Err(msg) = config::ensure_state_subdirs() {
        muzzle::log::error("ensure-worktree", &msg);
        std::process::exit(1);
    }

    // Resolve session (read-write mode — this binary is invoked as a Bash command)
    let sess = session::resolve();
    if !sess.has_session() {
        muzzle::log::error(
            "ensure-worktree",
            "no active session found — must run inside a Claude Code session",
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

    // Create worktree — route through appropriate VCS backend
    let entry = match vcs_kind {
        VcsKind::Jj | VcsKind::JjColocated => {
            use muzzle::vcs::jj::JjBackend;
            use muzzle::vcs::VcsBackend;
            let jj = JjBackend {
                colocated: vcs_kind == VcsKind::JjColocated,
            };
            // Resolve repo path
            let repo_path = match config::workspaces()
                .iter()
                .map(|ws| ws.join(repo))
                .find(|p| p.is_dir())
            {
                Some(p) => p,
                None => {
                    muzzle::log::error("ensure-worktree", &format!("repo not found: {repo}"));
                    std::process::exit(1);
                }
            };
            let dest = config::worktree_path(&repo_path, &sess.short_id);
            match jj.workspace_add(&repo_path, &dest, &sess.short_id, None, &sess.tmp_dir) {
                Ok(entry) => entry,
                Err(e) => {
                    muzzle::log::error("ensure-worktree", &format!("jj workspace add failed: {e}"));
                    std::process::exit(1);
                }
            }
        }
        VcsKind::Git => match worktree::ensure_for_repo(&sess, repo) {
            Ok(entry) => entry,
            Err(e) => {
                muzzle::log::emit_full(
                    "ERROR",
                    "ensure-worktree",
                    &format!("failed to create worktree for {}", repo),
                    None,
                    Some(&e.to_string()),
                );
                std::process::exit(1);
            }
        },
    };

    // Update spec file
    if let Err(e) = session::append_spec_entry(&sess.spec_file, &entry) {
        muzzle::log::emit_full(
            "ERROR",
            "ensure-worktree",
            "worktree created but failed to update spec file",
            None,
            Some(&e.to_string()),
        );
        // Still print the path — the worktree exists even if spec write failed
        println!("{}", entry.wt_path);
        std::process::exit(1);
    }

    println!("{}", entry.wt_path);
}
