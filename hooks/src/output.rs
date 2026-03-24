//! JSON response helpers for Claude Code hooks.

use serde::Serialize;

/// The top-level JSON envelope for PreToolUse responses.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HookResponse {
    /// The hook-specific payload (present for PreToolUse responses).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hook_specific_output: Option<HookSpecificOutput>,
}

/// Carries the PreToolUse decision.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HookSpecificOutput {
    /// Hook event type (always "PreToolUse" for permission hooks).
    pub hook_event_name: String,
    /// One of "allow", "deny", or "ask".
    pub permission_decision: String,
    /// Human-readable reason for deny/ask decisions.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub permission_decision_reason: Option<String>,
    /// Modified tool input parameters (for AllowWithUpdatedInput decisions).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_input: Option<serde_json::Map<String, serde_json::Value>>,
}

/// Permission decision enum for internal use.
#[derive(Debug, Clone, PartialEq)]
pub enum Decision {
    /// Permit the tool call without prompting.
    Allow,
    /// Block the tool call with a reason message.
    Deny(String),
    /// Prompt the user for confirmation with a reason message.
    Ask(String),
    /// Permit the tool call and modify its input parameters.
    AllowWithUpdatedInput(serde_json::Map<String, serde_json::Value>),
}

impl Decision {
    /// Convert to JSON string for stdout.
    pub fn to_json(&self) -> String {
        let resp = match self {
            Decision::Allow => HookResponse {
                hook_specific_output: Some(HookSpecificOutput {
                    hook_event_name: "PreToolUse".into(),
                    permission_decision: "allow".into(),
                    permission_decision_reason: None,
                    updated_input: None,
                }),
            },
            Decision::Deny(reason) => HookResponse {
                hook_specific_output: Some(HookSpecificOutput {
                    hook_event_name: "PreToolUse".into(),
                    permission_decision: "deny".into(),
                    permission_decision_reason: Some(reason.clone()),
                    updated_input: None,
                }),
            },
            Decision::Ask(reason) => HookResponse {
                hook_specific_output: Some(HookSpecificOutput {
                    hook_event_name: "PreToolUse".into(),
                    permission_decision: "ask".into(),
                    permission_decision_reason: Some(reason.clone()),
                    updated_input: None,
                }),
            },
            Decision::AllowWithUpdatedInput(updated) => HookResponse {
                hook_specific_output: Some(HookSpecificOutput {
                    hook_event_name: "PreToolUse".into(),
                    permission_decision: "allow".into(),
                    permission_decision_reason: None,
                    updated_input: Some(updated.clone()),
                }),
            },
        };
        // serde_json::to_string should not fail on our static types
        serde_json::to_string(&resp).unwrap_or_else(|_| {
            r#"{"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"allow"}}"#.into()
        })
    }

    /// Print JSON to stdout and exit.
    pub fn emit_and_exit(&self) -> ! {
        println!("{}", self.to_json());
        std::process::exit(0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_allow_json() {
        let json = Decision::Allow.to_json();
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("invalid JSON");
        let output = &parsed["hookSpecificOutput"];
        assert_eq!(output["permissionDecision"], "allow");
        assert_eq!(output["hookEventName"], "PreToolUse");
        assert!(
            output.get("permissionDecisionReason").is_none()
                || output["permissionDecisionReason"].is_null()
        );
    }

    #[test]
    fn test_deny_json() {
        let json = Decision::Deny("test reason".into()).to_json();
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("invalid JSON");
        let output = &parsed["hookSpecificOutput"];
        assert_eq!(output["permissionDecision"], "deny");
        assert_eq!(output["permissionDecisionReason"], "test reason");
    }

    #[test]
    fn test_ask_json() {
        let json = Decision::Ask("confirm this".into()).to_json();
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("invalid JSON");
        let output = &parsed["hookSpecificOutput"];
        assert_eq!(output["permissionDecision"], "ask");
        assert_eq!(output["permissionDecisionReason"], "confirm this");
    }

    #[test]
    fn test_allow_with_updated_input() {
        let mut updated = serde_json::Map::new();
        updated.insert(
            "prompt".into(),
            serde_json::Value::String("modified prompt".into()),
        );
        let json = Decision::AllowWithUpdatedInput(updated).to_json();
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("invalid JSON");
        let output = &parsed["hookSpecificOutput"];
        assert_eq!(output["permissionDecision"], "allow");
        assert_eq!(output["updatedInput"]["prompt"], "modified prompt");
    }

    #[test]
    fn test_json_escaping() {
        let json = Decision::Deny(r#"path with "quotes" and \backslashes"#.into()).to_json();
        assert!(
            serde_json::from_str::<serde_json::Value>(&json).is_ok(),
            "invalid JSON: {}",
            json
        );
        assert!(json.contains("quotes"));
    }
}
