use muzzle_memory::{capture, inject, store};
use std::{env, fs, process};

fn main() {
    let args: Vec<String> = env::args().collect();

    let cmd = args.get(1).map(|s| s.as_str());
    let result = match cmd {
        Some("search") => cmd_search(&args[2..]),
        Some("save") => cmd_save(&args[2..]),
        Some("capture") => cmd_capture(&args[2..]),
        Some("context") => cmd_context(&args[2..]),
        Some("inject") => cmd_inject(&args[2..]),
        Some("stats") => cmd_stats(),
        _ => {
            print_usage();
            process::exit(1);
        }
    };

    if let Err(e) = result {
        eprintln!("error: {e}");
        process::exit(1);
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn db_path() -> String {
    match env::var("HOME") {
        Ok(home) if !home.is_empty() => format!("{home}/.muzzle/memory.db"),
        _ => {
            eprintln!("warning: HOME not set, using /tmp/.muzzle/memory.db (data may be lost)");
            "/tmp/.muzzle/memory.db".to_string()
        }
    }
}

fn open_store() -> Result<store::Store, String> {
    let path = db_path();
    // Ensure parent directory exists.
    if let Some(parent) = std::path::Path::new(&path).parent() {
        fs::create_dir_all(parent).map_err(|e| format!("create dir: {e}"))?;
    }
    store::Store::open(&path).map_err(|e| format!("open db: {e}"))
}

/// Derive project name from CWD: `parent_basename/basename`.
///
/// Uses `git rev-parse --show-toplevel` first so that worktree paths
/// (e.g. `~/src/cn/Hermosa/.worktrees/abc123/`) resolve to the main
/// repo (`cn/Hermosa`). Falls back to raw CWD if not in a git repo.
fn project_from_cwd() -> String {
    // Try git toplevel first (resolves worktrees to main repo).
    if let Ok(output) = std::process::Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .output()
    {
        if output.status.success() {
            let toplevel = String::from_utf8_lossy(&output.stdout).trim().to_string();
            return parent_slash_base(std::path::Path::new(&toplevel));
        }
    }
    // Fallback: CWD parent/basename.
    let cwd = env::current_dir().unwrap_or_default();
    parent_slash_base(&cwd)
}

/// Derive `parent/basename` from a path, guarding against root-level paths
/// (e.g., `/my-project` → `"my-project"`, not `"/my-project"`).
fn parent_slash_base(path: &std::path::Path) -> String {
    let base = path.file_name().unwrap_or_default().to_string_lossy();
    let parent = path
        .parent()
        .and_then(|p| p.file_name())
        .unwrap_or_default()
        .to_string_lossy();
    if parent.is_empty() {
        base.to_string()
    } else {
        format!("{parent}/{base}")
    }
}

/// Resolve the muzzle state directory (mirrors hooks/src/config.rs logic).
fn resolve_state_dir() -> String {
    if let Ok(sd) = env::var("MUZZLE_STATE_DIR") {
        if !sd.is_empty() {
            return sd;
        }
    }
    if let Ok(xdg) = env::var("XDG_STATE_HOME") {
        if !xdg.is_empty() {
            return format!("{xdg}/muzzle");
        }
    }
    let home = env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    format!("{home}/.local/state/muzzle")
}

/// Simple flag lookup: find `flag` in `args` and return the next element.
fn flag_val<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    args.iter()
        .position(|a| a == flag)
        .and_then(|i| args.get(i + 1))
        .map(|s| s.as_str())
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let truncated: String = s.chars().take(max).collect();
        format!("{truncated}...")
    }
}

fn print_usage() {
    eprintln!("Usage: memory <command> [args...]");
    eprintln!();
    eprintln!("Commands:");
    eprintln!("  search  <query> [-p project]");
    eprintln!(
        "  save    <title> <content> [--type TYPE] [--topic KEY] [--source SRC] [-p project]"
    );
    eprintln!("  capture [changelog-path] [session-id] [project]");
    eprintln!("  context [project]");
    eprintln!("  inject  [project]");
    eprintln!("  stats");
}

// ---------------------------------------------------------------------------
// Subcommands
// ---------------------------------------------------------------------------

fn cmd_search(args: &[String]) -> Result<(), String> {
    let query = find_positional(args, 0).ok_or("search requires a <query>")?;
    let project = flag_val(args, "-p");

    let store = open_store()?;
    let results = store
        .search(query, project, 10)
        .map_err(|e| format!("search: {e}"))?;

    if results.is_empty() {
        println!("No results found.");
        return Ok(());
    }

    for (i, r) in results.iter().enumerate() {
        let preview = truncate(&r.content, 200);
        println!(
            "{:>3}. [{:.4}] {} | {} | {}",
            i + 1,
            r.rank,
            r.project,
            r.obs_type,
            r.title,
        );
        println!("     {preview}");
    }

    Ok(())
}

fn cmd_save(args: &[String]) -> Result<(), String> {
    let title = find_positional(args, 0).ok_or("save requires <title>")?;
    let content = find_positional(args, 1).ok_or("save requires <content>")?;

    let obs_type = flag_val(args, "--type").unwrap_or("learning");
    let topic_key = flag_val(args, "--topic");
    let source = flag_val(args, "--source").unwrap_or("agent");
    let project = flag_val(args, "-p")
        .map(|s| s.to_string())
        .unwrap_or_else(project_from_cwd);
    let session_id = env::var("MUZZLE_SESSION_ID").unwrap_or_else(|_| "manual".to_string());

    let mut store = open_store()?;
    let cwd = env::current_dir()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();
    store
        .register_session(&session_id, &project, &cwd)
        .map_err(|e| format!("register session: {e}"))?;

    let id = store
        .save_observation(store::NewObservation {
            session_id,
            obs_type: obs_type.to_string(),
            title: title.to_string(),
            content: content.to_string(),
            project,
            scope: None,
            topic_key: topic_key.map(|s| s.to_string()),
            source: source.to_string(),
        })
        .map_err(|e| format!("save: {e}"))?;

    println!("Saved observation #{id}");
    Ok(())
}

fn cmd_capture(args: &[String]) -> Result<(), String> {
    // Resolve changelog path: explicit arg, or follow the current-changelog symlink.
    let (changelog_path_buf, session_id_owned, project_owned);
    let (changelog_path, session_id, project): (&str, &str, &str);

    if let Some(path) = args.first() {
        changelog_path = path;
        session_id = args.get(1).map(|s| s.as_str()).unwrap_or("unknown");
        project = args
            .get(2)
            .map(|s| s.as_str())
            .unwrap_or_else(|| "unknown");
    } else {
        // Zero-arg mode: resolve from symlink.
        let state_dir = resolve_state_dir();
        let symlink = format!("{state_dir}/current-changelog.md");
        let target = fs::read_link(&symlink)
            .map_err(|e| format!("read symlink {symlink}: {e}"))?;
        // Target is relative: "changelogs/<session-id>.md"
        changelog_path_buf = format!(
            "{state_dir}/{}",
            target.to_string_lossy()
        );
        // Extract session ID from filename: "<uuid>.md" → "<uuid>"
        session_id_owned = target
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        project_owned = project_from_cwd();
        changelog_path = &changelog_path_buf;
        session_id = &session_id_owned;
        project = &project_owned;
    }

    let changelog = match fs::read_to_string(changelog_path) {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            // No changelog = no mutating tool calls this session — nothing to capture.
            return Ok(());
        }
        Err(e) => return Err(format!("read changelog: {e}")),
    };

    let summary = capture::parse_changelog(&changelog);
    if summary.is_empty() {
        return Ok(());
    }

    let mut store = open_store()?;
    let cwd = env::current_dir()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();
    store
        .register_session(session_id, project, &cwd)
        .map_err(|e| format!("register session: {e}"))?;

    let id = store
        .save_observation(store::NewObservation {
            session_id: session_id.to_string(),
            obs_type: "session_summary".to_string(),
            title: format!("Session {session_id}"),
            content: summary,
            project: project.to_string(),
            scope: None,
            topic_key: Some(format!("session/{session_id}")),
            source: "changelog".to_string(),
        })
        .map_err(|e| format!("save: {e}"))?;

    eprintln!("Captured session summary #{id}");
    Ok(())
}

fn cmd_context(args: &[String]) -> Result<(), String> {
    let project = args
        .first()
        .map(|s| s.to_string())
        .unwrap_or_else(project_from_cwd);

    let store = open_store()?;
    let observations = store
        .recent_context(&project, 10)
        .map_err(|e| format!("recent_context: {e}"))?;

    if observations.is_empty() {
        println!("No observations for project '{project}'.");
        return Ok(());
    }

    println!("# Context: {project}\n");
    for obs in &observations {
        let preview = truncate(&obs.content, 200);
        println!(
            "- **{}** [{}]: {}\n  {}\n",
            obs.obs_type, obs.source, obs.title, preview
        );
    }

    Ok(())
}

fn cmd_inject(args: &[String]) -> Result<(), String> {
    let project = args
        .first()
        .map(|s| s.to_string())
        .unwrap_or_else(project_from_cwd);

    let store = open_store()?;
    let observations = store
        .recent_context(&project, 10)
        .map_err(|e| format!("recent_context: {e}"))?;

    let context = inject::format_context(&observations, &project);
    if context.is_empty() {
        println!("{{}}");
        return Ok(());
    }

    // JSON-escape the markdown content.
    let escaped = serde_json::to_string(&context).map_err(|e| format!("json escape: {e}"))?;

    println!(
        "{{\"hookSpecificOutput\":{{\"hookEventName\":\"SessionStart\",\"additionalContext\":{escaped}}}}}"
    );
    Ok(())
}

fn cmd_stats() -> Result<(), String> {
    let store = open_store()?;
    let stats = store.stats().map_err(|e| format!("stats: {e}"))?;

    println!("Sessions:     {}", stats.total_sessions);
    println!("Observations: {}", stats.total_observations);
    println!("Projects:     {}", stats.projects.join(", "));
    Ok(())
}

// ---------------------------------------------------------------------------
// Positional arg extraction (skips flags and their values)
// ---------------------------------------------------------------------------

/// Flags that consume the following argument as their value.
const VALUE_FLAGS: &[&str] = &["-p", "--type", "--topic", "--source"];

/// Find the Nth positional argument in `args`, skipping flags and flag values.
///
/// Only flags listed in `VALUE_FLAGS` consume the next argument as their value.
/// Unknown `-` args are treated as boolean flags (no value consumed).
fn find_positional(args: &[String], n: usize) -> Option<&str> {
    let mut count = 0usize;
    let mut skip_next = false;

    for arg in args {
        if skip_next {
            skip_next = false;
            continue;
        }
        if arg.starts_with('-') {
            if VALUE_FLAGS.contains(&arg.as_str()) {
                skip_next = true; // skip this flag's value
            }
            continue;
        }
        if count == n {
            return Some(arg.as_str());
        }
        count += 1;
    }
    None
}
