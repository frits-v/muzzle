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
                }),
            },
            Decision::Deny(reason) => HookResponse {
                hook_specific_output: Some(HookSpecificOutput {
                    hook_event_name: "PreToolUse".into(),
                    permission_decision: "deny".into(),
                    permission_decision_reason: Some(reason.clone()),
                }),
            },
            Decision::Ask(reason) => HookResponse {
                hook_specific_output: Some(HookSpecificOutput {
                    hook_event_name: "PreToolUse".into(),
                    permission_decision: "ask".into(),
                    permission_decision_reason: Some(reason.clone()),
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
    fn test_json_escaping() {
        let json = Decision::Deny(r#"path with "quotes" and \backslashes"#.into()).to_json();
        assert!(
            serde_json::from_str::<serde_json::Value>(&json).is_ok(),
            "invalid JSON: {}",
            json
        );
        assert!(json.contains("quotes"));
    }

    #[test]
    fn test_deny_unicode_reason() {
        let json = Decision::Deny("path: /tmp/\u{1F4A9}/file \u{00E9}\u{00F1}".into()).to_json();
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("invalid JSON");
        let reason = parsed["hookSpecificOutput"]["permissionDecisionReason"]
            .as_str()
            .unwrap();
        assert!(reason.contains('\u{1F4A9}'));
        assert!(reason.contains('\u{00E9}'));
    }

    #[test]
    fn test_deny_empty_reason() {
        let json = Decision::Deny(String::new()).to_json();
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("invalid JSON");
        assert_eq!(parsed["hookSpecificOutput"]["permissionDecisionReason"], "");
    }

    #[test]
    fn test_ask_long_reason() {
        let long = "x".repeat(10_000);
        let json = Decision::Ask(long.clone()).to_json();
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("invalid JSON");
        assert_eq!(
            parsed["hookSpecificOutput"]["permissionDecisionReason"]
                .as_str()
                .unwrap()
                .len(),
            10_000
        );
    }

    #[test]
    fn test_deny_newlines_and_tabs() {
        let json = Decision::Deny("line1\nline2\ttab".into()).to_json();
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("invalid JSON");
        let reason = parsed["hookSpecificOutput"]["permissionDecisionReason"]
            .as_str()
            .unwrap();
        assert!(reason.contains('\n'));
        assert!(reason.contains('\t'));
    }

    #[test]
    fn test_decision_debug_impl() {
        assert!(format!("{:?}", Decision::Allow).contains("Allow"));
        assert!(format!("{:?}", Decision::Deny("r".into())).contains("Deny"));
        assert!(format!("{:?}", Decision::Ask("q".into())).contains("Ask"));
    }

    #[test]
    fn test_decision_clone_and_eq() {
        let d1 = Decision::Deny("reason".into());
        let d2 = d1.clone();
        assert_eq!(d1, d2);
        assert_ne!(Decision::Allow, Decision::Deny("x".into()));
        assert_ne!(Decision::Ask("a".into()), Decision::Ask("b".into()));
    }

    #[test]
    fn test_allow_has_no_reason_key() {
        let json = Decision::Allow.to_json();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(
            parsed["hookSpecificOutput"]
                .get("permissionDecisionReason")
                .is_none(),
            "Allow should omit permissionDecisionReason entirely"
        );
    }
}
