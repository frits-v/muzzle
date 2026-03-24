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

/// Returns true if the tool call is an Agent invocation with a named identity
/// (either `name` or `team_name` key present in tool_input).
fn is_persona_eligible(input: &HookInput) -> bool {
    if input.tool_name != "Agent" {
        return false;
    }
    input.tool_input.get("name").is_some() || input.tool_input.get("team_name").is_some()
}

/// Infer an agent role from the Agent tool_input fields.
///
/// Priority:
/// 1. `subagent_type` field (used verbatim)
/// 2. `description` field keyword match
/// 3. Fallback: "general"
fn infer_role(tool_input: &serde_json::Value) -> String {
    if let Some(st) = tool_input.get("subagent_type").and_then(|v| v.as_str()) {
        if !st.is_empty() {
            return st.to_string();
        }
    }

    if let Some(desc) = tool_input.get("description").and_then(|v| v.as_str()) {
        let desc_lower = desc.to_lowercase();
        if desc_lower.contains("review") {
            return "code-reviewer".to_string();
        }
        if desc_lower.contains("security") {
            return "security-review".to_string();
        }
        if desc_lower.contains("research") {
            return "researcher".to_string();
        }
        if desc_lower.contains("test") {
            return "testing".to_string();
        }
        if desc_lower.contains("debug") {
            return "debugging".to_string();
        }
        if desc_lower.contains("architect") {
            return "architecture".to_string();
        }
    }

    "general".to_string()
}

/// Shell out to `muzzle-persona assign` to get a preamble for the agent.
///
/// Prepends the preamble to the agent's prompt in tool_input.
/// Fail-open: on any error (spawn fail, non-zero exit, bad JSON), returns Allow.
fn check_agent(input: &HookInput) -> Decision {
    let agent_name = input
        .tool_input
        .get("name")
        .or_else(|| input.tool_input.get("team_name"))
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
        .to_string();

    let role = infer_role(&input.tool_input);

    let sess = session::resolve_readonly();
    let session_id = sess.id.clone();

    let project = muzzle::config::workspaces()
        .into_iter()
        .next()
        .and_then(|ws| ws.file_name().map(|n| n.to_string_lossy().to_string()))
        .unwrap_or_else(|| "unknown".to_string());

    let persona_bin = muzzle::config::bin_dir().join("muzzle-persona");
    let roles_json = format!("[\"{role}\"]");

    let output = std::process::Command::new(&persona_bin)
        .args([
            "assign",
            &format!("--roles={roles_json}"),
            &format!("--project={project}"),
            &format!("--session={session_id}"),
            &format!("--agent-name={agent_name}"),
        ])
        .output();

    let output = match output {
        Ok(o) if o.status.success() => o,
        Ok(o) => {
            eprintln!(
                "muzzle-persona assign exited with status {}: {}",
                o.status,
                String::from_utf8_lossy(&o.stderr)
            );
            return Decision::Allow;
        }
        Err(e) => {
            eprintln!("muzzle-persona spawn failed: {e}");
            return Decision::Allow;
        }
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    let assignments: serde_json::Value = match serde_json::from_str(stdout.trim()) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("muzzle-persona output parse error: {e}");
            return Decision::Allow;
        }
    };

    let preamble = assignments
        .as_array()
        .and_then(|arr| arr.first())
        .and_then(|a| a.get("preamble"))
        .and_then(|p| p.as_str())
        .unwrap_or("")
        .to_string();

    if preamble.is_empty() {
        return Decision::Allow;
    }

    // Prepend preamble to the original prompt
    let mut modified = match input.tool_input.as_object() {
        Some(obj) => obj.clone(),
        None => return Decision::Allow,
    };

    let original_prompt = modified
        .get("prompt")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let new_prompt = format!("{preamble}\n\n{original_prompt}");
    modified.insert("prompt".to_string(), serde_json::Value::String(new_prompt));

    Decision::AllowWithUpdatedInput(modified)
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

    // Agent persona injection
    if is_persona_eligible(input) {
        return check_agent(input);
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
    // FR-SB-NOSANDBOXDISABLE: Block sandbox escape attempts.
    // Inspects the raw JSON value before any serde deserialization so a
    // malformed flag (null, string, number) cannot slip through via
    // unwrap_or_default(). Only absent or explicit `false` is allowed.
    if let Some(val) = input
        .tool_input
        .as_object()
        .and_then(|obj| obj.get("dangerouslyDisableSandbox"))
    {
        if *val != serde_json::Value::Bool(false) {
            return Decision::Deny(
                "BLOCKED: dangerouslyDisableSandbox is not allowed — \
                 add required paths to the sandbox allowlist instead"
                    .into(),
            );
        }
    }

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
        let actual_path = wp.strip_prefix("gitc:").unwrap_or(wp);

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_with_name_detected() {
        let input = HookInput {
            tool_name: "Agent".into(),
            tool_input: serde_json::json!({"name": "worker-1", "prompt": "Do the thing", "description": "A worker"}),
        };
        assert!(is_persona_eligible(&input));
    }

    #[test]
    fn agent_with_team_name_detected() {
        let input = HookInput {
            tool_name: "Agent".into(),
            tool_input: serde_json::json!({"team_name": "swarm-123", "prompt": "Do the thing"}),
        };
        assert!(is_persona_eligible(&input));
    }

    #[test]
    fn anonymous_agent_not_detected() {
        let input = HookInput {
            tool_name: "Agent".into(),
            tool_input: serde_json::json!({"prompt": "Do the thing"}),
        };
        assert!(!is_persona_eligible(&input));
    }

    #[test]
    fn non_agent_tool_not_detected() {
        let input = HookInput {
            tool_name: "Bash".into(),
            tool_input: serde_json::json!({"command": "ls"}),
        };
        assert!(!is_persona_eligible(&input));
    }

    #[test]
    fn infer_role_from_subagent_type() {
        let input = serde_json::json!({"subagent_type": "code-reviewer", "prompt": "..."});
        assert_eq!(infer_role(&input), "code-reviewer");
    }

    #[test]
    fn infer_role_from_description() {
        let input = serde_json::json!({"description": "Security audit worker", "prompt": "..."});
        assert_eq!(infer_role(&input), "security-review");
    }

    #[test]
    fn infer_role_fallback_to_general() {
        let input = serde_json::json!({"prompt": "Do the thing"});
        assert_eq!(infer_role(&input), "general");
    }
}
