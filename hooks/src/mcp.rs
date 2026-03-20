//! MCP tool routing decisions.
//!
//! FR-MR-1 through FR-MR-7: GitHub, Atlassian, Datadog, Sentry, Slack, Sysdig routing.
//!
//! Atlassian rate limiting: Writes rate-limit counters to `.claude-tmp/{session-id}/rate-limits/`.
//! This is an acceptable side effect — writing to our own scratch space (same exception
//! as the Go version). The rate limit state needs the session ID, passed as a parameter.

use crate::config;
use std::fs;
use std::time::SystemTime;

/// MCP routing decision.
#[derive(Debug, Clone, PartialEq)]
pub enum McpDecision {
    /// Tool call is safe to execute without prompting.
    Allow,
    /// Tool call is blocked with a reason message.
    Deny(String),
    /// Tool call requires user confirmation with a reason message.
    Ask(String),
}

/// Route an MCP tool call to the appropriate handler.
/// `session_id` is optional; when provided, enables rate limiting for Atlassian tools.
pub fn route(tool_name: &str) -> McpDecision {
    route_with_session(tool_name, None)
}

/// Route an MCP tool call with session context for rate limiting.
pub fn route_with_session(tool_name: &str, session_id: Option<&str>) -> McpDecision {
    if let Some(action) = tool_name.strip_prefix("mcp__github__") {
        return route_github(action);
    }
    if let Some(action) = tool_name.strip_prefix("mcp__atlassian__") {
        return route_atlassian(action, session_id);
    }
    if let Some(action) = tool_name.strip_prefix("mcp__claude_ai_Atlassian__") {
        return route_atlassian(action, session_id);
    }
    if let Some(action) = tool_name.strip_prefix("mcp__datadog__") {
        return route_datadog(action);
    }
    if let Some(action) = tool_name.strip_prefix("mcp__claude_ai_Sentry__") {
        return route_sentry(action);
    }
    if let Some(action) = tool_name.strip_prefix("mcp__claude_ai_Slack__") {
        return route_slack(action);
    }
    if let Some(action) = tool_name.strip_prefix("mcp__sysdig__") {
        return route_sysdig(action);
    }
    if tool_name.starts_with("mcp__") {
        // FR-MR-7: Unknown MCP tools -> ASK
        return McpDecision::Ask(format!(
            "Unknown MCP tool '{}' — requires confirmation",
            tool_name
        ));
    }

    McpDecision::Allow
}

/// FR-MR-1: GitHub MCP routing.
fn route_github(action: &str) -> McpDecision {
    // Read-only
    if action.starts_with("get_") || action.starts_with("list_") || action.starts_with("search_") {
        return McpDecision::Allow;
    }

    // Safe writes
    match action {
        "create_pull_request"
        | "create_branch"
        | "update_pull_request_branch"
        | "create_issue"
        | "add_issue_comment"
        | "update_issue"
        | "create_or_update_file"
        | "push_files"
        | "fork_repository"
        | "create_repository" => return McpDecision::Allow,
        _ => {}
    }

    // Human-judgment
    match action {
        "merge_pull_request" => {
            return McpDecision::Ask("Merge pull request — merging is a human decision".into())
        }
        "create_pull_request_review" => {
            return McpDecision::Ask(
                "Create PR review — visible review on PR, confirm before posting".into(),
            )
        }
        _ => {}
    }

    McpDecision::Ask(format!(
        "GitHub MCP tool '{}' — unknown tool, requires confirmation",
        action
    ))
}

/// FR-MR-2: Atlassian MCP routing.
///
/// Rate limiting for `createJiraIssue`: writes counters to `.claude-tmp/{session-id}/rate-limits/`.
/// This is acceptable scratch-space I/O (same exception as Go version).
fn route_atlassian(action: &str, session_id: Option<&str>) -> McpDecision {
    // Read-only
    if action.starts_with("get")
        || action.starts_with("search")
        || action.starts_with("list")
        || action.starts_with("lookup")
        || action.starts_with("fetch")
        || action == "atlassianUserInfo"
        || action.starts_with("getAccessible")
        || action == "jiraRead"
    {
        return McpDecision::Allow;
    }

    // Safe Jira writes
    match action {
        "addCommentToJiraIssue"
        | "addWorklogToJiraIssue"
        | "editJiraIssue"
        | "transitionJiraIssue"
        | "jiraWrite" => return McpDecision::Allow,
        _ => {}
    }

    // Jira issue creation — rate limited, then ASK
    if action == "createJiraIssue" {
        if let Some(sid) = session_id {
            if !sid.is_empty() && check_atlassian_rate_limit("createJiraIssue", sid) {
                return McpDecision::Ask(format!(
                    "Rate limit: createJiraIssue calls exceeded {} in {}s window. Confirm to continue.",
                    config::ATLASSIAN_RATE_LIMIT, config::ATLASSIAN_RATE_WINDOW
                ));
            }
        }
        return McpDecision::Ask("Create Jira issue — confirm before creating".into());
    }

    // Confluence writes
    if action.contains("Page")
        || action.contains("Comment")
        || action.starts_with("createConfluence")
        || action.starts_with("updateConfluence")
    {
        return McpDecision::Ask(format!(
            "Confluence write ({}) — shared documentation, confirm before modifying",
            action
        ));
    }

    McpDecision::Ask(format!(
        "Atlassian MCP tool '{}' — unknown tool, requires confirmation",
        action
    ))
}

/// Check if the Atlassian rate limit is exceeded.
///
/// Returns true if the count of calls within the rate window exceeds the limit.
/// Writes timestamps to `.claude-tmp/{session-id}/rate-limits/{tool}`.
/// This is an acceptable side effect (scratch space only).
fn check_atlassian_rate_limit(tool: &str, session_id: &str) -> bool {
    let rate_dir = config::rate_limit_dir(session_id);
    if fs::create_dir_all(&rate_dir).is_err() {
        return false;
    }

    let counter_file = rate_dir.join(tool);
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let window = config::ATLASSIAN_RATE_WINDOW;

    // Read existing timestamps, filter to those within the window
    let mut valid_entries: Vec<u64> = Vec::new();
    if let Ok(data) = fs::read_to_string(&counter_file) {
        for line in data.trim().lines() {
            if let Ok(ts) = line.trim().parse::<u64>() {
                if now.saturating_sub(ts) < window {
                    valid_entries.push(ts);
                }
            }
        }
    }

    // Write current timestamp + valid entries
    let mut lines = vec![now.to_string()];
    for ts in &valid_entries {
        lines.push(ts.to_string());
    }
    let _ = fs::write(&counter_file, lines.join("\n") + "\n");

    // Count includes current call
    valid_entries.len() + 1 > config::ATLASSIAN_RATE_LIMIT
}

/// FR-MR-3: Datadog MCP routing.
fn route_datadog(action: &str) -> McpDecision {
    if action.starts_with("get_")
        || action.starts_with("list_")
        || action.starts_with("search_")
        || action.starts_with("query_")
    {
        return McpDecision::Allow;
    }
    McpDecision::Ask(format!(
        "Datadog MCP tool '{}' — write operation, requires confirmation",
        action
    ))
}

/// FR-MR-4: Sentry MCP routing.
fn route_sentry(action: &str) -> McpDecision {
    if action.starts_with("get_")
        || action.starts_with("search_")
        || action.starts_with("find_")
        || action == "whoami"
        || action.starts_with("analyze_")
    {
        return McpDecision::Allow;
    }
    McpDecision::Ask(format!(
        "Sentry MCP tool '{}' — write operation, requires confirmation",
        action
    ))
}

/// FR-MR-5: Slack MCP routing.
fn route_slack(action: &str) -> McpDecision {
    if action.starts_with("slack_read_") || action.starts_with("slack_search_") {
        return McpDecision::Allow;
    }
    if action.starts_with("slack_send_")
        || action.starts_with("slack_schedule_")
        || action.starts_with("slack_create_")
    {
        return McpDecision::Ask(format!(
            "Slack MCP write ({}) — visible to others, confirm before sending",
            action
        ));
    }
    McpDecision::Ask(format!(
        "Slack MCP tool '{}' — unknown tool, requires confirmation",
        action
    ))
}

/// FR-MR-6: Sysdig MCP routing.
fn route_sysdig(action: &str) -> McpDecision {
    if action.starts_with("get_") || action.starts_with("k8s_") || action.starts_with("list_") {
        return McpDecision::Allow;
    }
    McpDecision::Ask(format!(
        "Sysdig MCP tool '{}' — unknown tool, requires confirmation",
        action
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ENV_LOCK;

    // FR-MR-1: GitHub
    #[test]
    fn test_github_read_allow() {
        let tools = [
            "mcp__github__get_file_contents",
            "mcp__github__list_commits",
            "mcp__github__search_code",
            "mcp__github__get_pull_request",
        ];
        for tool in &tools {
            let d = route(tool);
            assert_eq!(d, McpDecision::Allow, "expected ALLOW for {}", tool);
        }
    }

    #[test]
    fn test_github_safe_write_allow() {
        let tools = [
            "mcp__github__create_pull_request",
            "mcp__github__create_branch",
            "mcp__github__create_issue",
            "mcp__github__add_issue_comment",
            "mcp__github__push_files",
        ];
        for tool in &tools {
            let d = route(tool);
            assert_eq!(d, McpDecision::Allow, "expected ALLOW for {}", tool);
        }
    }

    #[test]
    fn test_github_merge_ask() {
        let d = route("mcp__github__merge_pull_request");
        assert!(matches!(d, McpDecision::Ask(_)), "expected ASK for merge");
    }

    #[test]
    fn test_github_review_ask() {
        let d = route("mcp__github__create_pull_request_review");
        assert!(matches!(d, McpDecision::Ask(_)), "expected ASK for review");
    }

    // FR-MR-2: Atlassian
    #[test]
    fn test_atlassian_read_allow() {
        let tools = [
            "mcp__claude_ai_Atlassian__getJiraIssue",
            "mcp__claude_ai_Atlassian__searchJiraIssuesUsingJql",
            "mcp__claude_ai_Atlassian__getConfluencePage",
            "mcp__claude_ai_Atlassian__atlassianUserInfo",
            "mcp__claude_ai_Atlassian__fetch",
        ];
        for tool in &tools {
            let d = route(tool);
            assert_eq!(d, McpDecision::Allow, "expected ALLOW for {}", tool);
        }
    }

    #[test]
    fn test_atlassian_safe_write_allow() {
        let tools = [
            "mcp__claude_ai_Atlassian__addCommentToJiraIssue",
            "mcp__claude_ai_Atlassian__editJiraIssue",
            "mcp__claude_ai_Atlassian__transitionJiraIssue",
        ];
        for tool in &tools {
            let d = route(tool);
            assert_eq!(d, McpDecision::Allow, "expected ALLOW for {}", tool);
        }
    }

    #[test]
    fn test_atlassian_confluence_write_ask() {
        let tools = [
            "mcp__claude_ai_Atlassian__createConfluencePage",
            "mcp__claude_ai_Atlassian__updateConfluencePage",
            "mcp__claude_ai_Atlassian__createConfluenceFooterComment",
        ];
        for tool in &tools {
            let d = route(tool);
            assert!(
                matches!(d, McpDecision::Ask(_)),
                "expected ASK for {}",
                tool
            );
        }
    }

    // FR-MR-3: Datadog
    #[test]
    fn test_datadog_read_allow() {
        let tools = [
            "mcp__datadog__get_dashboard",
            "mcp__datadog__list_hosts",
            "mcp__datadog__search_audit_logs",
            "mcp__datadog__query_metrics",
        ];
        for tool in &tools {
            let d = route(tool);
            assert_eq!(d, McpDecision::Allow, "expected ALLOW for {}", tool);
        }
    }

    #[test]
    fn test_datadog_write_ask() {
        let tools = [
            "mcp__datadog__schedule_downtime",
            "mcp__datadog__mute_host",
            "mcp__datadog__cancel_downtime",
        ];
        for tool in &tools {
            let d = route(tool);
            assert!(
                matches!(d, McpDecision::Ask(_)),
                "expected ASK for {}",
                tool
            );
        }
    }

    // FR-MR-4: Sentry
    #[test]
    fn test_sentry_read_allow() {
        let tools = [
            "mcp__claude_ai_Sentry__get_issue_details",
            "mcp__claude_ai_Sentry__search_issues",
            "mcp__claude_ai_Sentry__find_projects",
            "mcp__claude_ai_Sentry__whoami",
            "mcp__claude_ai_Sentry__analyze_issue_with_seer",
        ];
        for tool in &tools {
            let d = route(tool);
            assert_eq!(d, McpDecision::Allow, "expected ALLOW for {}", tool);
        }
    }

    #[test]
    fn test_sentry_unknown_ask() {
        let d = route("mcp__claude_ai_Sentry__delete_issue");
        assert!(
            matches!(d, McpDecision::Ask(_)),
            "expected ASK for unknown Sentry tool"
        );
    }

    // FR-MR-5: Slack
    #[test]
    fn test_slack_read_allow() {
        let tools = [
            "mcp__claude_ai_Slack__slack_read_channel",
            "mcp__claude_ai_Slack__slack_read_thread",
            "mcp__claude_ai_Slack__slack_search_public",
            "mcp__claude_ai_Slack__slack_search_users",
        ];
        for tool in &tools {
            let d = route(tool);
            assert_eq!(d, McpDecision::Allow, "expected ALLOW for {}", tool);
        }
    }

    #[test]
    fn test_slack_write_ask() {
        let tools = [
            "mcp__claude_ai_Slack__slack_send_message",
            "mcp__claude_ai_Slack__slack_schedule_message",
            "mcp__claude_ai_Slack__slack_create_canvas",
        ];
        for tool in &tools {
            let d = route(tool);
            assert!(
                matches!(d, McpDecision::Ask(_)),
                "expected ASK for {}",
                tool
            );
        }
    }

    // FR-MR-6: Sysdig
    #[test]
    fn test_sysdig_read_allow() {
        let tools = [
            "mcp__sysdig__get_event_info",
            "mcp__sysdig__k8s_list_clusters",
            "mcp__sysdig__list_runtime_events",
        ];
        for tool in &tools {
            let d = route(tool);
            assert_eq!(d, McpDecision::Allow, "expected ALLOW for {}", tool);
        }
    }

    // FR-MR-7: Unknown MCP
    #[test]
    fn test_unknown_mcp_ask() {
        let d = route("mcp__unknown_service__do_something");
        assert!(
            matches!(d, McpDecision::Ask(_)),
            "expected ASK for unknown MCP"
        );
    }

    // Non-MCP -> ALLOW
    #[test]
    fn test_non_mcp_allow() {
        let d = route("SomeOtherTool");
        assert_eq!(d, McpDecision::Allow);
    }

    // Rate limiting tests
    #[test]
    fn test_atlassian_create_jira_with_session() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        // Clean up stale rate files from previous test runs
        let rate_dir = config::rate_limit_dir("test-rate-limit-session-1");
        let _ = std::fs::remove_dir_all(&rate_dir);
        // With session ID, should still ASK (first call, not rate limited)
        let d = route_with_session(
            "mcp__claude_ai_Atlassian__createJiraIssue",
            Some("test-rate-limit-session-1"),
        );
        assert!(
            matches!(d, McpDecision::Ask(ref msg) if msg.contains("Create Jira issue")),
            "expected ASK for createJiraIssue with session, got {:?}",
            d
        );
    }

    #[test]
    fn test_atlassian_create_jira_no_session() {
        // Without session ID, should ASK with normal message
        let d = route_with_session("mcp__claude_ai_Atlassian__createJiraIssue", None);
        assert!(
            matches!(d, McpDecision::Ask(ref msg) if msg.contains("Create Jira issue")),
            "expected ASK for createJiraIssue without session, got {:?}",
            d
        );
    }

    #[test]
    fn test_rate_limit_exceeded() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        // Use a unique session ID so tests don't interfere
        let session_id = "test-rate-limit-exceed";
        let rate_dir = config::rate_limit_dir(session_id);
        let _ = fs::create_dir_all(&rate_dir);
        let counter_file = rate_dir.join("createJiraIssue");

        // Pre-populate with enough entries to exceed the limit
        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let entries: Vec<String> = (0..config::ATLASSIAN_RATE_LIMIT)
            .map(|i| (now - i as u64).to_string())
            .collect();
        let _ = fs::write(&counter_file, entries.join("\n") + "\n");

        // This call should exceed the limit
        let exceeded = check_atlassian_rate_limit("createJiraIssue", session_id);
        assert!(exceeded, "expected rate limit to be exceeded");

        // Cleanup
        let _ = fs::remove_dir_all(&rate_dir);
    }

    #[test]
    fn test_rate_limit_not_exceeded() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let session_id = "test-rate-limit-ok";
        let rate_dir = config::rate_limit_dir(session_id);
        let _ = fs::create_dir_all(&rate_dir);
        let counter_file = rate_dir.join("createJiraIssue");

        // Pre-populate with fewer entries than the limit
        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let _ = fs::write(&counter_file, format!("{}\n", now - 10));

        // This call should NOT exceed the limit (only 2 total: 1 existing + 1 current)
        let exceeded = check_atlassian_rate_limit("createJiraIssue", session_id);
        assert!(!exceeded, "expected rate limit NOT to be exceeded");

        // Cleanup
        let _ = fs::remove_dir_all(&rate_dir);
    }

    #[test]
    fn test_route_with_session_rate_limit_message() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        // Exhaust the rate limit, then verify the route function returns the rate limit message
        let session_id = "test-route-ratelimit-msg";
        let rate_dir = config::rate_limit_dir(session_id);
        let _ = fs::create_dir_all(&rate_dir);
        let counter_file = rate_dir.join("createJiraIssue");

        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let entries: Vec<String> = (0..config::ATLASSIAN_RATE_LIMIT)
            .map(|i| (now - i as u64).to_string())
            .collect();
        let _ = fs::write(&counter_file, entries.join("\n") + "\n");

        let d = route_with_session(
            "mcp__claude_ai_Atlassian__createJiraIssue",
            Some(session_id),
        );
        assert!(
            matches!(d, McpDecision::Ask(ref msg) if msg.contains("Rate limit")),
            "expected rate limit ASK message, got {:?}",
            d
        );

        // Cleanup
        let _ = fs::remove_dir_all(&rate_dir);
    }

    // ── Mutation-killing: verify specific Ask messages ──

    #[test]
    fn test_github_merge_message_specific() {
        let d = route("mcp__github__merge_pull_request");
        if let McpDecision::Ask(msg) = &d {
            assert!(
                msg.contains("merging"),
                "merge Ask should mention 'merging', got: {}",
                msg
            );
        } else {
            panic!("expected Ask, got {:?}", d);
        }
    }

    #[test]
    fn test_github_review_message_specific() {
        let d = route("mcp__github__create_pull_request_review");
        if let McpDecision::Ask(msg) = &d {
            assert!(
                msg.contains("review"),
                "review Ask should mention 'review', got: {}",
                msg
            );
        } else {
            panic!("expected Ask, got {:?}", d);
        }
    }

    #[test]
    fn test_github_unknown_action_message() {
        // Unknown GitHub action falls through to generic ask
        let d = route("mcp__github__delete_repository");
        if let McpDecision::Ask(msg) = &d {
            assert!(
                msg.contains("unknown tool"),
                "unknown GitHub action should say 'unknown tool', got: {}",
                msg
            );
        } else {
            panic!("expected Ask, got {:?}", d);
        }
    }

    #[test]
    fn test_atlassian_read_prefixes_independent() {
        // Each read prefix should independently allow
        let prefixes = [
            ("get", "mcp__claude_ai_Atlassian__getJiraIssue"),
            (
                "search",
                "mcp__claude_ai_Atlassian__searchJiraIssuesUsingJql",
            ),
            ("list", "mcp__claude_ai_Atlassian__listProjects"),
            ("lookup", "mcp__claude_ai_Atlassian__lookupJiraAccountId"),
            ("fetch", "mcp__claude_ai_Atlassian__fetchAtlassian"),
        ];
        for (prefix, tool) in &prefixes {
            let d = route(tool);
            assert_eq!(
                d,
                McpDecision::Allow,
                "Atlassian {} prefix should Allow for {}",
                prefix,
                tool
            );
        }
    }

    #[test]
    fn test_slack_write_messages_specific() {
        let d = route("mcp__claude_ai_Slack__slack_send_message");
        if let McpDecision::Ask(msg) = &d {
            assert!(
                msg.contains("visible to others"),
                "Slack write should mention 'visible to others', got: {}",
                msg
            );
        } else {
            panic!("expected Ask, got {:?}", d);
        }
    }

    #[test]
    fn test_slack_unknown_message() {
        let d = route("mcp__claude_ai_Slack__slack_delete_message");
        if let McpDecision::Ask(msg) = &d {
            assert!(
                msg.contains("unknown tool"),
                "unknown Slack action should say 'unknown tool', got: {}",
                msg
            );
        } else {
            panic!("expected Ask, got {:?}", d);
        }
    }

    #[test]
    fn test_atlassian_confluence_message_specific() {
        let d = route("mcp__claude_ai_Atlassian__createConfluencePage");
        if let McpDecision::Ask(msg) = &d {
            assert!(
                msg.contains("Confluence") && msg.contains("shared documentation"),
                "Confluence write should mention 'Confluence' and 'shared documentation', got: {}",
                msg
            );
        } else {
            panic!("expected Ask, got {:?}", d);
        }
    }

    #[test]
    fn test_rate_limit_boundary_exact() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let session_id = "test-rate-limit-boundary";
        let rate_dir = config::rate_limit_dir(session_id);
        let _ = fs::create_dir_all(&rate_dir);
        let counter_file = rate_dir.join("createJiraIssue");

        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        // Pre-populate with exactly (LIMIT - 1) entries — current call makes it LIMIT
        let entries: Vec<String> = (0..config::ATLASSIAN_RATE_LIMIT - 1)
            .map(|i| (now - i as u64).to_string())
            .collect();
        let _ = fs::write(&counter_file, entries.join("\n") + "\n");

        // At exactly the limit: (LIMIT - 1) existing + 1 current = LIMIT
        // Should NOT exceed (> not >=)
        let exceeded = check_atlassian_rate_limit("createJiraIssue", session_id);
        assert!(
            !exceeded,
            "at exactly the limit ({}) should NOT exceed",
            config::ATLASSIAN_RATE_LIMIT
        );

        // Cleanup
        let _ = fs::remove_dir_all(&rate_dir);
    }
}
