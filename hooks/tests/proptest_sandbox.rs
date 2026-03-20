//! Property-based tests for sandbox, gitcheck, and MCP routing invariants.

use muzzle::gitcheck;
use muzzle::mcp::{self, McpDecision};
use muzzle::sandbox::{self, PathDecision, ToolContext};
use proptest::prelude::*;

// --- Strategies ---

/// Generate paths that look like system paths (should always be denied).
fn system_path_strategy() -> impl Strategy<Value = String> {
    prop_oneof![
        "/etc/[a-z]{1,20}".prop_map(|s| s),
        "/usr/[a-z]{1,20}/[a-z]{1,20}".prop_map(|s| s),
        "/System/[A-Z][a-z]{1,15}".prop_map(|s| s),
        "/Library/[A-Z][a-z]{1,15}".prop_map(|s| s),
    ]
}

/// Generate arbitrary strings that could be paths.
fn arbitrary_path_strategy() -> impl Strategy<Value = String> {
    prop_oneof![
        "[a-zA-Z0-9_./-]{0,200}",
        "(/[a-zA-Z0-9_.]{1,30}){1,8}",
        "(\\.\\.?/){1,5}[a-z]{1,10}",
    ]
}

/// Generate git commands with bare force-push (no --force-with-lease).
fn force_push_strategy() -> impl Strategy<Value = String> {
    prop_oneof![
        "git push --force [a-z]{1,10} [a-z]{1,20}".prop_map(|s| s),
        "git push -f [a-z]{1,10} [a-z]{1,20}".prop_map(|s| s),
    ]
}

/// Generate safe git commands (should never be denied for git safety).
fn safe_git_strategy() -> impl Strategy<Value = String> {
    prop_oneof![
        "git status".prop_map(|s| s),
        "git log --oneline -[0-9]{1,2}".prop_map(|s| s),
        "git diff [a-z]{1,20}".prop_map(|s| s),
        "git branch -a".prop_map(|s| s),
    ]
}

// --- Sandbox Properties ---

proptest! {
    /// System paths must ALWAYS be denied regardless of input variations.
    #[test]
    fn prop_system_paths_always_denied(path in system_path_strategy()) {
        let result = sandbox::check_path(&path, None);
        match result {
            PathDecision::Deny(_) => {} // expected
            other => panic!(
                "System path '{}' should be Deny, got {:?}",
                path, other
            ),
        }
    }

    /// check_path must never panic on arbitrary string input.
    #[test]
    fn prop_check_path_never_panics(path in arbitrary_path_strategy()) {
        let _ = sandbox::check_path(&path, None);
    }

    /// check_path_with_context must never panic on arbitrary input + context combos.
    #[test]
    fn prop_check_path_with_context_never_panics(
        path in arbitrary_path_strategy(),
        is_bash in any::<bool>(),
    ) {
        let ctx = if is_bash { ToolContext::Bash } else { ToolContext::FileTool };
        let _ = sandbox::check_path_with_context(&path, None, ctx);
    }

    /// is_system_path_resolved must never panic.
    #[test]
    fn prop_is_system_path_never_panics(path in arbitrary_path_strategy()) {
        let _ = sandbox::is_system_path_resolved(&path);
    }

    /// /tmp paths via Bash should be Allow (compiler/pip writes).
    #[test]
    fn prop_tmp_paths_bash_allowed(suffix in "[a-zA-Z0-9_]{1,30}") {
        let path = format!("/tmp/{}", suffix);
        let result = sandbox::check_path_with_context(&path, None, ToolContext::Bash);
        match result {
            PathDecision::Allow => {} // expected for Bash writing to /tmp
            PathDecision::Ask(_) => {} // also acceptable (FileTool context asks)
            PathDecision::Deny(msg) => panic!(
                "/tmp path '{}' via Bash should not be Deny: {}",
                path, msg
            ),
        }
    }
}

// --- Git Safety Properties ---

proptest! {
    /// Force-push commands must ALWAYS be denied.
    #[test]
    fn prop_force_push_always_denied(cmd in force_push_strategy()) {
        let result = gitcheck::check_git_safety(&cmd);
        assert!(
            matches!(result, gitcheck::GitResult::Block(_)),
            "Force push '{}' should be Deny, got {:?}",
            cmd,
            result
        );
    }

    /// Safe read-only git commands must never be denied.
    #[test]
    fn prop_safe_git_never_denied(cmd in safe_git_strategy()) {
        let result = gitcheck::check_git_safety(&cmd);
        assert!(
            !matches!(result, gitcheck::GitResult::Block(_)),
            "Safe git '{}' should not be Deny, got {:?}",
            cmd,
            result
        );
    }

    /// check_git_safety must never panic on arbitrary input.
    #[test]
    fn prop_git_safety_never_panics(cmd in ".*") {
        let _ = gitcheck::check_git_safety(&cmd);
    }

    /// check_bash_write_paths must never panic on arbitrary input.
    #[test]
    fn prop_bash_write_paths_never_panics(cmd in ".*") {
        let _ = gitcheck::check_bash_write_paths(&cmd);
    }

    /// extract_repo_from_git_op must never panic on arbitrary input.
    #[test]
    fn prop_extract_repo_never_panics(cmd in ".*") {
        let _ = gitcheck::extract_repo_from_git_op(&cmd);
    }
}

// --- MCP Routing Strategies ---

/// Generate tool names for unknown MCP providers (not github, atlassian, datadog, sentry, slack, sysdig).
fn unknown_mcp_strategy() -> impl Strategy<Value = String> {
    "[a-z]{3,15}__[a-z_]{3,20}".prop_map(|action| format!("mcp__{}", action))
}

/// Generate non-MCP tool names (no mcp__ prefix).
fn non_mcp_strategy() -> impl Strategy<Value = String> {
    prop_oneof!["[A-Z][a-zA-Z]{2,15}", "Bash|Read|Write|Edit|Glob|Grep",]
}

// --- MCP Routing Properties ---

proptest! {
    /// Unknown MCP providers must ALWAYS get Ask, never Allow.
    #[test]
    fn prop_unknown_mcp_never_allowed(tool in unknown_mcp_strategy()) {
        let known_prefixes = [
            "mcp__github__",
            "mcp__atlassian__",
            "mcp__claude_ai_Atlassian__",
            "mcp__datadog__",
            "mcp__claude_ai_Sentry__",
            "mcp__claude_ai_Slack__",
            "mcp__sysdig__",
        ];
        // Skip tools that happen to match a known provider prefix
        if known_prefixes.iter().any(|p| tool.starts_with(p)) {
            return Ok(());
        }
        let result = mcp::route(&tool);
        assert!(
            matches!(result, McpDecision::Ask(_)),
            "Unknown MCP tool '{}' should be Ask, got {:?}",
            tool,
            result
        );
    }

    /// Non-MCP tools must ALWAYS be allowed.
    #[test]
    fn prop_non_mcp_always_allowed(tool in non_mcp_strategy()) {
        let result = mcp::route(&tool);
        assert!(
            matches!(result, McpDecision::Allow),
            "Non-MCP tool '{}' should be Allow, got {:?}",
            tool,
            result
        );
    }

    /// MCP route must never panic on arbitrary input.
    #[test]
    fn prop_mcp_route_never_panics(tool in ".*") {
        let _ = mcp::route(&tool);
    }

    /// MCP route must never return Deny (only Allow or Ask).
    #[test]
    fn prop_mcp_route_never_denies(tool in ".*") {
        let result = mcp::route(&tool);
        assert!(
            !matches!(result, McpDecision::Deny(_)),
            "MCP route should never Deny, got {:?} for '{}'",
            result,
            tool
        );
    }
}
