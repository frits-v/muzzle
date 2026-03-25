//! PreToolUse hook for persona injection into named Agent calls.
//!
//! SEPARATE from permissions — preserves H-4 purity on the permissions binary.
//! Must be registered FIRST in the hook chain (before permissions) so its
//! updatedInput is preserved (Claude Code bug #15897: later hooks' updatedInput
//! overwrites earlier hooks').
//!
//! Fail-open: any error → exit 0 (passthrough, no denial).

use muzzle::config;
use muzzle::session;
use serde::{Deserialize, Serialize};
use std::io::{self, Read};

#[derive(Deserialize)]
struct HookInput {
    tool_name: String,
    #[serde(default)]
    tool_input: serde_json::Value,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct HookResponse {
    hook_specific_output: HookSpecificOutput,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct HookSpecificOutput {
    hook_event_name: String,
    permission_decision: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    permission_decision_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    updated_input: Option<serde_json::Map<String, serde_json::Value>>,
}

fn main() {
    // Fail-open on panic (NOT deny — this is persona injection, not security)
    let _ = std::panic::catch_unwind(run);
    // If run() returned or panicked, exit 0 (passthrough)
}

fn run() {
    // Read stdin
    let mut data = String::new();
    if io::stdin().read_to_string(&mut data).is_err() {
        return; // exit 0
    }

    let input: HookInput = match serde_json::from_str(&data) {
        Ok(v) => v,
        Err(_) => return,
    };

    // Not a persona-eligible call — pass through silently
    if !is_persona_eligible(&input) {
        return;
    }

    // Attempt injection; on failure, pass through silently
    if let Some(response) = try_inject_persona(&input) {
        if let Ok(json) = serde_json::to_string(&response) {
            println!("{json}");
        }
    }
}

fn is_persona_eligible(input: &HookInput) -> bool {
    if input.tool_name != "Agent" {
        return false;
    }
    let obj = match input.tool_input.as_object() {
        Some(o) => o,
        None => return false,
    };
    obj.contains_key("name") || obj.contains_key("team_name")
}

fn infer_role(tool_input: &serde_json::Value) -> String {
    // 1. Explicit subagent_type field
    if let Some(t) = tool_input.get("subagent_type").and_then(|v| v.as_str()) {
        if !t.is_empty() {
            return t.to_string();
        }
    }

    // 2. Description keyword matching
    if let Some(desc) = tool_input.get("description").and_then(|v| v.as_str()) {
        let lower = desc.to_lowercase();
        if lower.contains("review") {
            return "code-reviewer".to_string();
        }
        if lower.contains("security") {
            return "security-review".to_string();
        }
        if lower.contains("research") {
            return "researcher".to_string();
        }
        if lower.contains("test") {
            return "testing".to_string();
        }
        if lower.contains("debug") {
            return "debugging".to_string();
        }
        if lower.contains("architect") {
            return "architecture".to_string();
        }
    }

    // 3. Fallback
    "general".to_string()
}

fn try_inject_persona(input: &HookInput) -> Option<HookResponse> {
    let obj = input.tool_input.as_object()?;

    // Extract agent_name from name or team_name
    let agent_name = obj
        .get("name")
        .or_else(|| obj.get("team_name"))
        .and_then(|v| v.as_str())?;

    // Extract optional team_name (only when name was the primary key)
    let team_name = if obj.contains_key("name") {
        obj.get("team_name").and_then(|v| v.as_str())
    } else {
        None
    };

    let role = infer_role(&input.tool_input);

    // Resolve session (read-only — H-4 compliant)
    let sess = session::resolve_readonly();
    let session_id = if sess.has_session() {
        sess.id.clone()
    } else {
        return None;
    };

    // Get project name from first workspace basename
    let project = config::workspace()
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown")
        .to_string();

    // Build muzzle-persona assign arguments
    let persona_bin = config::bin_dir().join("muzzle-persona");

    let roles_arg = format!("--roles=[{role}]");
    let project_arg = format!("--project={project}");
    let session_arg = format!("--session={session_id}");
    let agent_arg = format!("--agent-name={agent_name}");

    let mut cmd = std::process::Command::new(&persona_bin);
    cmd.arg("assign")
        .arg(&roles_arg)
        .arg(&project_arg)
        .arg(&session_arg)
        .arg(&agent_arg);

    if let Some(tn) = team_name {
        cmd.arg(format!("--team-name={tn}"));
    }

    let output = cmd.output().ok()?;
    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8(output.stdout).ok()?;
    let assignments: serde_json::Value = serde_json::from_str(stdout.trim()).ok()?;

    // Get preamble from first assignment
    let preamble = assignments
        .as_array()?
        .first()?
        .get("preamble")
        .and_then(|v| v.as_str())?;

    if preamble.is_empty() {
        return None;
    }

    // Clone tool_input and prepend preamble to prompt
    let mut modified = obj.clone();
    let existing_prompt = modified
        .get("prompt")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let new_prompt = format!("{preamble}\n\n{existing_prompt}");
    modified.insert("prompt".to_string(), serde_json::Value::String(new_prompt));

    Some(HookResponse {
        hook_specific_output: HookSpecificOutput {
            hook_event_name: "PreToolUse".to_string(),
            permission_decision: "allow".to_string(),
            permission_decision_reason: None,
            updated_input: Some(modified),
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_with_name_detected() {
        let input = HookInput {
            tool_name: "Agent".into(),
            tool_input: serde_json::json!({"name": "worker-1", "prompt": "Do the thing"}),
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
        assert_eq!(
            infer_role(&serde_json::json!({"subagent_type": "code-reviewer"})),
            "code-reviewer"
        );
    }

    #[test]
    fn infer_role_from_description() {
        assert_eq!(
            infer_role(&serde_json::json!({"description": "Security audit worker"})),
            "security-review"
        );
    }

    #[test]
    fn infer_role_fallback_to_general() {
        assert_eq!(
            infer_role(&serde_json::json!({"prompt": "Do the thing"})),
            "general"
        );
    }
}
