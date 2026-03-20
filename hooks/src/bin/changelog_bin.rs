//! PostToolUse hook for Claude Code.
//!
//! Receives JSON on stdin: {"tool_name": "...", "tool_input": {...}, "tool_output": {...}}
//! No stdout. Writes to `.claude-changelog-{session-id}.md`
//! Skips read-only tools (FR-AL-2).

use muzzle::changelog::{self, InputFields, OutputFields, ToolInput};
use muzzle::config;
use muzzle::session;
use std::io::{self, Read};

fn main() {
    // Changelog is best-effort — panic should not block the session
    let _ = std::panic::catch_unwind(run);
}

fn run() {
    // Skip when not running inside any configured workspace
    if !config::is_in_any_workspace() {
        std::process::exit(0);
    }

    // Read stdin
    let mut data = String::new();
    if io::stdin().read_to_string(&mut data).is_err() {
        std::process::exit(0);
    }

    let input: ToolInput = match serde_json::from_str(&data) {
        Ok(v) => v,
        Err(_) => std::process::exit(0),
    };

    if input.tool_name.is_empty() {
        std::process::exit(0);
    }

    // Parse input and output fields
    let input_fields = InputFields::from_value(&input.tool_input);
    let output_fields = OutputFields::from_value(&input.tool_output);

    // Skip read-only tools (FR-AL-2)
    if changelog::is_read_only(&input.tool_name, &input_fields) {
        std::process::exit(0);
    }

    // Resolve session to find changelog path
    let sess = session::resolve();
    let log_path = if sess.has_session() {
        sess.changelog_path.clone()
    } else {
        // Fallback: write to symlink (points to most recent session)
        config::changelog_symlink()
    };

    // Format and append entry
    let entry = changelog::format_entry(&input.tool_name, &input_fields, &output_fields);
    let _ = changelog::append_to_changelog(&log_path, &entry);
}
