//! PreToolUse hook for Claude Code.
//!
//! Receives JSON on stdin: {"tool_name": "Edit", "tool_input": {...}}
//! Returns JSON on stdout with permission decision (allow/deny/ask).
//!
//! H-4: MUST BE PURE — no file writes, no worktree creation, no side effects.
//! Uses session::resolve_readonly() to avoid writing PID marker cache files.
//! Atlassian rate limiting is handled separately via mcp::check_atlassian_rate_limit()
//! which only writes to .claude-tmp/ (acceptable scratch space).

use muzzle::gitcheck;
use muzzle::mcp;
use muzzle::output::Decision;
use muzzle::sandbox;
use muzzle::session;
use serde::Deserialize;
use std::io::{self, Read};

#[derive(Deserialize)]
struct HookInput {
    tool_name: String,
    #[serde(default)]
    tool_input: serde_json::Value,
}

#[derive(Deserialize, Default)]
struct BashInput {
    #[serde(default)]
    command: String,
}

#[derive(Deserialize, Default)]
struct FileInput {
    #[serde(default)]
    file_path: String,
    #[serde(default)]
    notebook_path: String,
}

fn main() {
    let result = std::panic::catch_unwind(run);
    if result.is_err() {
        // Panic → deny (never fail open on crash)
        Decision::Deny("BLOCKED: permissions hook panicked — denying for safety".into())
            .emit_and_exit();
    }
}

fn run() {
    // Read stdin
    let mut data = String::new();
    if io::stdin().read_to_string(&mut data).is_err() {
        std::process::exit(0); // Fail open — fall through to settings.json
    }

    let input: HookInput = match serde_json::from_str(&data) {
        Ok(v) => v,
        Err(_) => std::process::exit(0),
    };

    if input.tool_name.is_empty() {
        std::process::exit(0);
    }

    // Route by tool name
    let decision = route(&input);
    decision.emit_and_exit();
}

fn route(input: &HookInput) -> Decision {
    // Always-safe tools (read-only, no side effects)
    if is_always_safe(&input.tool_name) {
        return Decision::Allow;
    }

    // Filesystem writes
    if matches!(input.tool_name.as_str(), "Edit" | "Write" | "NotebookEdit") {
        return check_filesystem(input);
    }

    // Bash commands
    if input.tool_name == "Bash" {
        return check_bash(input);
    }

    // MCP tools
    if input.tool_name.starts_with("mcp__") {
        return check_mcp(input);
    }

    // Everything else
    Decision::Allow
}

fn is_always_safe(tool: &str) -> bool {
    matches!(
        tool,
        "Read"
            | "Glob"
            | "Grep"
            | "WebSearch"
            | "WebFetch"
            | "Task"
            | "TaskOutput"
            | "TaskStop"
            | "TaskCreate"
            | "TaskGet"
            | "TaskUpdate"
            | "TaskList"
            | "AskUserQuestion"
            | "EnterPlanMode"
            | "ExitPlanMode"
            | "ListMcpResourcesTool"
            | "ReadMcpResourceTool"
            | "ToolSearch"
            | "EnterWorktree"
            | "Skill"
            | "SendMessage"
            | "TeamCreate"
            | "TeamDelete"
    )
}

fn check_filesystem(input: &HookInput) -> Decision {
    let fi: FileInput = serde_json::from_value(input.tool_input.clone()).unwrap_or_default();

    let file_path = if input.tool_name == "NotebookEdit" {
        &fi.notebook_path
    } else {
        &fi.file_path
    };

    if file_path.is_empty() {
        return Decision::Allow; // No path — let Claude Code handle it
    }

    let sess = session::resolve_readonly();
    let result =
        sandbox::check_path_with_context(file_path, Some(&sess), sandbox::ToolContext::FileTool);

    match result {
        sandbox::PathDecision::Allow => Decision::Allow,
        sandbox::PathDecision::Deny(reason) => Decision::Deny(reason),
        sandbox::PathDecision::Ask(reason) => Decision::Ask(reason),
    }
}

fn check_bash(input: &HookInput) -> Decision {
    let bi: BashInput = serde_json::from_value(input.tool_input.clone()).unwrap_or_default();

    if bi.command.is_empty() {
        return Decision::Allow;
    }

    // Git safety checks (FR-GS-1 through FR-GS-8)
    match gitcheck::check_git_safety(&bi.command) {
        gitcheck::GitResult::Block(reason) => return Decision::Deny(reason),
        gitcheck::GitResult::Ok => {}
    }

    // gh merge checks
    let gh_result = gitcheck::check_gh_merge(&bi.command);
    if gh_result.should_ask {
        return Decision::Ask(gh_result.reason);
    }

    // Worktree enforcement
    let sess = session::resolve_readonly();
    if sess.has_session() {
        if let Some(reason) =
            gitcheck::check_worktree_enforcement(&bi.command, sess.worktree_active, &sess.short_id)
        {
            return Decision::Deny(reason);
        }

        // FR-WE-2: If session exists but no worktrees, return WORKTREE_MISSING
        // so the agent can lazily create a worktree via ensure-worktree.
        if !sess.worktree_active
            && bi.command.contains("git")
            && gitcheck::is_repo_git_op(&bi.command)
            && !gitcheck::is_worktree_management_op(&bi.command)
        {
            if let Some(repo) = gitcheck::extract_repo_from_git_op(&bi.command) {
                return Decision::Deny(muzzle::worktree_missing_msg(&repo));
            }
            return Decision::Deny(
                "BLOCKED: No worktree for this session. \
                 Cannot determine target repo from command."
                    .into(),
            );
        }
    }

    // Bash write-path scanning
    let write_paths = gitcheck::check_bash_write_paths(&bi.command);
    for wp in &write_paths {
        let is_git_c = wp.starts_with("gitc:");
        let is_rel = wp.starts_with("rel:");
        let actual_path = wp
            .strip_prefix("gitc:")
            .or_else(|| wp.strip_prefix("rel:"))
            .unwrap_or(wp);

        if is_git_c {
            // git -C is a working directory, not a write target.
            // Worktree enforcement already handles git -C above.
            // Only block if it targets a system path (e.g. git -C /etc/).
            if sandbox::is_system_path_resolved(actual_path) {
                return Decision::Deny(format!(
                    "BLOCKED: git -C targets system path: {}",
                    actual_path
                ));
            }
            continue;
        }

        if is_rel {
            // Relative path from a file-mutating command (sed -i, cp, mv, etc.).
            // When worktrees are active, relative writes target the main checkout
            // (CWD is the main checkout unless explicitly cd'd to a worktree).
            // Block these to prevent Edit-hook bypass via Bash.
            if sess.has_session() && sess.worktree_active {
                return Decision::Deny(format!(
                    "BLOCKED: File-mutating Bash command targets main checkout \
                     path '{}'. {}",
                    actual_path,
                    muzzle::worktree_missing_msg("(detected from Bash)")
                ));
            }
            // No worktree active — can't resolve relative path, allow through
            continue;
        }

        let result =
            sandbox::check_path_with_context(actual_path, Some(&sess), sandbox::ToolContext::Bash);
        match result {
            sandbox::PathDecision::Deny(reason) => return Decision::Deny(reason),
            sandbox::PathDecision::Ask(reason) => return Decision::Ask(reason),
            sandbox::PathDecision::Allow => {}
        }
    }

    Decision::Allow
}

fn check_mcp(input: &HookInput) -> Decision {
    // Resolve session (read-only) to get session ID for rate limiting.
    // Rate limiting writes to .claude-tmp/ which is acceptable scratch space.
    let sess = session::resolve_readonly();
    let session_id = if sess.has_session() {
        Some(sess.id.as_str())
    } else {
        None
    };

    let decision = mcp::route_with_session(&input.tool_name, session_id);

    match decision {
        mcp::McpDecision::Allow => Decision::Allow,
        mcp::McpDecision::Deny(reason) => Decision::Deny(reason),
        mcp::McpDecision::Ask(reason) => Decision::Ask(reason),
    }
}
