//! Audit log formatting and read-only detection for PostToolUse hooks.
//!
//! FR-AL-1 through FR-AL-5: Per-session changelog, mutations only,
//! commit/push detail, MCP tool logging, UTC timestamps.

use regex::Regex;
use serde::Deserialize;
use std::collections::HashSet;
use std::io::Write;
use std::path::Path;
use std::process::Command;
use std::sync::LazyLock;

/// Input from Claude Code's PostToolUse hook.
#[derive(Debug, Deserialize)]
pub struct ToolInput {
    pub tool_name: String,
    #[serde(default)]
    pub tool_input: serde_json::Value,
    #[serde(default)]
    pub tool_output: serde_json::Value,
}

/// Common fields from tool_input.
#[derive(Debug, Default)]
pub struct InputFields {
    pub command: String,
    pub file_path: String,
    pub notebook_path: String,
    pub repo: String,
    pub title: String,
    pub branch: String,
    pub project_key: String,
    pub summary: String,
}

impl InputFields {
    pub fn from_value(v: &serde_json::Value) -> Self {
        Self {
            command: v
                .get("command")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string(),
            file_path: v
                .get("file_path")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string(),
            notebook_path: v
                .get("notebook_path")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string(),
            repo: v
                .get("repo")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string(),
            title: v
                .get("title")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string(),
            branch: v
                .get("branch")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string(),
            project_key: v
                .get("projectKey")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string(),
            summary: v
                .get("summary")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string(),
        }
    }
}

/// Common fields from tool_output.
#[derive(Debug, Default)]
pub struct OutputFields {
    pub stdout: String,
    pub stderr: String,
}

impl OutputFields {
    pub fn from_value(v: &serde_json::Value) -> Self {
        Self {
            stdout: v
                .get("stdout")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string(),
            stderr: v
                .get("stderr")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string(),
        }
    }
}

/// Set of read-only tools to skip logging for.
static READ_ONLY_TOOLS: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
    let mut s = HashSet::new();
    for tool in &[
        "Read",
        "Glob",
        "Grep",
        "WebSearch",
        "WebFetch",
        "Task",
        "TaskList",
        "TaskGet",
        "TaskCreate",
        "TaskUpdate",
        "TaskOutput",
        "TaskStop",
        "AskUserQuestion",
        "EnterPlanMode",
        "ExitPlanMode",
        "ListMcpResourcesTool",
        "ReadMcpResourceTool",
        "Skill",
        "ToolSearch",
        "SendMessage",
        "TeamCreate",
        "TeamDelete",
        "EnterWorktree",
    ] {
        s.insert(*tool);
    }
    s
});

/// Set of read-only Bash first-words.
static READ_ONLY_BASH_COMMANDS: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
    let mut s = HashSet::new();
    for cmd in &[
        "ls",
        "find",
        "tree",
        "cat",
        "head",
        "tail",
        "wc",
        "grep",
        "rg",
        "file",
        "stat",
        "du",
        "df",
        "which",
        "type",
        "whoami",
        "pwd",
        "date",
        "uname",
        "id",
        "env",
        "printenv",
        "echo",
        "printf",
        "diff",
        "/usr/bin/diff",
        "jq",
        "shasum",
        "md5sum",
        "basename",
        "dirname",
        "realpath",
        "readlink",
    ] {
        s.insert(*cmd);
    }
    s
});

static RE_READ_ONLY_GIT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^\s*git\s+(status|log|diff|show|branch|remote|tag|rev-parse|rev-list|ls-files|ls-tree|blame|shortlog|describe|stash\s+list|config\s+--get|config\s+--list)(\s|$)").unwrap()
});

static RE_READ_ONLY_GH: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^\s*gh\s+(pr\s+(view|checks|list|diff|status)|issue\s+(view|list)|run\s+(view|list)|release\s+view|repo\s+clone)(\s|$)").unwrap()
});

static RE_GIT_COMMIT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"git\s.*commit").unwrap());
static RE_GIT_PUSH: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"git\s.*push").unwrap());
static RE_COMMIT_SHA: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\[([^ ]+) ([0-9a-f]{7,})\]").unwrap());
static RE_PUSH_RANGE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[0-9a-f]+\.{2,3}[0-9a-f]+").unwrap());
static RE_PUSH_REF: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"-> ([^\s]+)").unwrap());
static RE_GIT_DIR: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"-C\s+(\S+)").unwrap());

/// Check if a tool/command should be skipped (read-only).
pub fn is_read_only(tool_name: &str, input: &InputFields) -> bool {
    if READ_ONLY_TOOLS.contains(tool_name) {
        return true;
    }

    // Skip MCP read-only tools
    if tool_name.starts_with("mcp__") {
        let parts: Vec<&str> = tool_name.split("__").collect();
        if parts.len() >= 3 {
            let action = parts[parts.len() - 1];
            if action.starts_with("get")
                || action.starts_with("search")
                || action.starts_with("list")
                || action.starts_with("lookup")
                || action.starts_with("fetch")
                || action == "atlassianUserInfo"
                || action.starts_with("getAccessible")
            {
                return true;
            }
        }
    }

    // Skip read-only Bash commands
    if tool_name == "Bash" && !input.command.is_empty() {
        let first_word = extract_first_word(&input.command);
        if READ_ONLY_BASH_COMMANDS.contains(first_word.as_str()) {
            return true;
        }
        if RE_READ_ONLY_GIT.is_match(&input.command) {
            return true;
        }
        if RE_READ_ONLY_GH.is_match(&input.command) {
            return true;
        }
    }

    false
}

/// Format a changelog entry for a tool use.
pub fn format_entry(tool_name: &str, input: &InputFields, output: &OutputFields) -> String {
    let ts = chrono_utc_now();

    match tool_name {
        "Bash" => format_bash_entry(&ts, input, output),
        "Edit" => format!("`{}` **Edit**: `{}`", ts, input.file_path),
        "Write" => format!("`{}` **Write**: `{}`", ts, input.file_path),
        "NotebookEdit" => format!("`{}` **NotebookEdit**: `{}`", ts, input.notebook_path),
        "mcp__github__create_pull_request" => {
            format!("`{}` **PR Created**: {} - {}", ts, input.repo, input.title)
        }
        "mcp__github__create_branch" => {
            format!(
                "`{}` **Branch Created**: {}/{}",
                ts, input.repo, input.branch
            )
        }
        "mcp__github__create_issue" => {
            format!(
                "`{}` **GitHub Issue**: {} - {}",
                ts, input.repo, input.title
            )
        }
        "mcp__atlassian__createJiraIssue" | "mcp__claude_ai_Atlassian__createJiraIssue" => {
            format!(
                "`{}` **Jira Issue**: {} - {}",
                ts, input.project_key, input.summary
            )
        }
        _ if tool_name.starts_with("mcp__") => {
            format!("`{}` **{}**", ts, tool_name)
        }
        _ => format!("`{}` **{}**", ts, tool_name),
    }
}

/// Format a Bash command changelog entry with commit/push detail.
fn format_bash_entry(ts: &str, input: &InputFields, output: &OutputFields) -> String {
    let cmd = &input.command;
    let combined = format!("{}{}", output.stdout, output.stderr);

    // Git commit detection
    if RE_GIT_COMMIT.is_match(cmd) {
        if let Some(caps) = RE_COMMIT_SHA.captures(&combined) {
            let commit_branch = caps.get(1).map(|m| m.as_str()).unwrap_or("");
            let commit_sha = caps.get(2).map(|m| m.as_str()).unwrap_or("");

            let mut entry = format!(
                "`{}` **COMMIT** `{}` on `{}`",
                ts, commit_sha, commit_branch
            );

            let git_dir = extract_git_dir(cmd);
            let file_list = get_commit_files(&git_dir, commit_sha);
            let diff_stat = get_commit_diffstat(&git_dir, commit_sha);

            if !file_list.is_empty() {
                for f in file_list.trim().lines() {
                    if !f.is_empty() {
                        entry.push_str(&format!("\n  - `{}`", f));
                    }
                }
            }
            if !diff_stat.is_empty() {
                entry.push_str(&format!("\n  > {}", diff_stat));
            }
            return entry;
        }
    }

    // Git push detection
    if RE_GIT_PUSH.is_match(cmd) {
        let push_remote = extract_push_arg(cmd, 1);
        let push_branch = extract_push_arg(cmd, 2);

        if !push_remote.is_empty() {
            let mut entry = format!("`{}` **PUSH** `{}`", ts, push_remote);
            if !push_branch.is_empty() {
                entry.push_str(&format!(" `{}`", push_branch));
            }
            if let Some(m) = RE_PUSH_RANGE.find(&combined) {
                entry.push_str(&format!(" ({})", m.as_str()));
            }
            if let Some(caps) = RE_PUSH_REF.captures(&combined) {
                if let Some(m) = caps.get(1) {
                    entry.push_str(&format!(" -> `{}`", m.as_str()));
                }
            }
            return entry;
        }
    }

    // Generic Bash command (truncated)
    let truncated = if cmd.len() > 200 {
        format!("{}...", &cmd[..200])
    } else {
        cmd.to_string()
    };
    format!("`{}` **Bash**: `{}`", ts, truncated)
}

/// Get UTC timestamp in YYYY-MM-DD HH:MM:SS format.
fn chrono_utc_now() -> String {
    use std::time::SystemTime;
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = now.as_secs();

    // Manual UTC formatting (avoids chrono dependency)
    let days = secs / 86400;
    let time_secs = secs % 86400;
    let hours = time_secs / 3600;
    let minutes = (time_secs % 3600) / 60;
    let seconds = time_secs % 60;

    // Days since epoch to y/m/d (civil_from_days algorithm)
    let (year, month, day) = days_to_date(days as i64);

    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
        year, month, day, hours, minutes, seconds
    )
}

/// Convert days since Unix epoch to (year, month, day).
fn days_to_date(days: i64) -> (i64, u32, u32) {
    // Howard Hinnant's algorithm
    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u32;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

/// Extract the first word of a command string.
fn extract_first_word(cmd: &str) -> String {
    let cmd = cmd.trim();
    for (i, c) in cmd.char_indices() {
        if c == ' ' || c == '\t' || c == '\n' {
            return cmd[..i].to_string();
        }
    }
    cmd.to_string()
}

/// Extract the -C path from a git command.
fn extract_git_dir(cmd: &str) -> String {
    RE_GIT_DIR
        .captures(cmd)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().to_string())
        .unwrap_or_default()
}

/// Extract the nth non-flag argument after 'push'.
fn extract_push_arg(cmd: &str, n: usize) -> String {
    if let Some(idx) = cmd.find("push") {
        let rest = cmd[idx + 4..].trim();
        let mut count = 0usize;
        for part in rest.split_whitespace() {
            if part.starts_with('-') {
                continue;
            }
            count += 1;
            if count == n {
                return part.to_string();
            }
        }
    }
    String::new()
}

/// Get the list of changed files for a commit.
fn get_commit_files(git_dir: &str, sha: &str) -> String {
    let parent_ref = format!("{}^", sha);
    let mut args: Vec<&str> = Vec::new();
    if !git_dir.is_empty() {
        args.extend_from_slice(&["-C", git_dir]);
    }
    args.extend_from_slice(&["diff", "--name-only", &parent_ref, sha]);

    Command::new("git")
        .args(&args)
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_default()
}

/// Get the diffstat summary for a commit.
fn get_commit_diffstat(git_dir: &str, sha: &str) -> String {
    let parent_ref = format!("{}^", sha);
    let mut args: Vec<&str> = Vec::new();
    if !git_dir.is_empty() {
        args.extend_from_slice(&["-C", git_dir]);
    }
    args.extend_from_slice(&["diff", "--stat", &parent_ref, sha]);

    Command::new("git")
        .args(&args)
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .and_then(|s| s.trim().lines().last().map(|l| l.to_string()))
        .unwrap_or_default()
}

/// Append an entry to the changelog file.
pub fn append_to_changelog(path: &Path, entry: &str) -> Result<(), std::io::Error> {
    let mut f = std::fs::OpenOptions::new()
        .append(true)
        .create(true)
        .open(path)?;
    writeln!(f, "{}", entry)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_read_only_skip_tools() {
        let tools = [
            "Read",
            "Glob",
            "Grep",
            "WebSearch",
            "WebFetch",
            "Task",
            "TaskList",
            "TaskGet",
            "TaskCreate",
            "TaskUpdate",
            "AskUserQuestion",
            "EnterPlanMode",
            "ExitPlanMode",
            "Skill",
            "ToolSearch",
            "SendMessage",
        ];
        for tool in &tools {
            assert!(
                is_read_only(tool, &InputFields::default()),
                "{} should be read-only",
                tool
            );
        }
    }

    #[test]
    fn test_is_read_only_mutating_not_skipped() {
        let tools = ["Edit", "Write", "NotebookEdit", "Bash"];
        for tool in &tools {
            assert!(
                !is_read_only(tool, &InputFields::default()),
                "{} should NOT be read-only",
                tool
            );
        }
    }

    #[test]
    fn test_is_read_only_bash_commands() {
        let cmds = [
            "ls -la",
            "find . -name test",
            "cat file.txt",
            "grep -r pattern",
            "echo hello",
        ];
        for cmd in &cmds {
            let input = InputFields {
                command: cmd.to_string(),
                ..Default::default()
            };
            assert!(
                is_read_only("Bash", &input),
                "Bash {:?} should be read-only",
                cmd
            );
        }
    }

    #[test]
    fn test_is_read_only_bash_git() {
        let cmds = [
            "git status",
            "git log --oneline",
            "git diff HEAD",
            "git branch -a",
            "git remote -v",
        ];
        for cmd in &cmds {
            let input = InputFields {
                command: cmd.to_string(),
                ..Default::default()
            };
            assert!(
                is_read_only("Bash", &input),
                "Bash {:?} should be read-only",
                cmd
            );
        }
    }

    #[test]
    fn test_is_read_only_bash_mutating_git() {
        let cmds = [
            "git commit -m 'test'",
            "git push origin feature",
            "git add .",
            "git merge feature",
        ];
        for cmd in &cmds {
            let input = InputFields {
                command: cmd.to_string(),
                ..Default::default()
            };
            assert!(
                !is_read_only("Bash", &input),
                "Bash {:?} should NOT be read-only",
                cmd
            );
        }
    }

    #[test]
    fn test_is_read_only_mcp_read_tools() {
        let tools = [
            "mcp__github__get_pull_request",
            "mcp__claude_ai_Atlassian__searchJiraIssuesUsingJql",
            "mcp__datadog__list_hosts",
            "mcp__claude_ai_Sentry__get_issue_details",
        ];
        for tool in &tools {
            assert!(
                is_read_only(tool, &InputFields::default()),
                "{} should be read-only",
                tool
            );
        }
    }

    #[test]
    fn test_format_entry_edit() {
        let entry = format_entry(
            "Edit",
            &InputFields {
                file_path: "/path/to/file.py".into(),
                ..Default::default()
            },
            &OutputFields::default(),
        );
        assert!(
            entry.contains("**Edit**"),
            "expected Edit marker in: {}",
            entry
        );
        assert!(
            entry.contains("/path/to/file.py"),
            "expected file path in: {}",
            entry
        );
    }

    #[test]
    fn test_format_entry_write() {
        let entry = format_entry(
            "Write",
            &InputFields {
                file_path: "/path/to/new.py".into(),
                ..Default::default()
            },
            &OutputFields::default(),
        );
        assert!(
            entry.contains("**Write**"),
            "expected Write marker in: {}",
            entry
        );
    }

    #[test]
    fn test_format_entry_bash_generic() {
        let entry = format_entry(
            "Bash",
            &InputFields {
                command: "make build".into(),
                ..Default::default()
            },
            &OutputFields::default(),
        );
        assert!(
            entry.contains("**Bash**"),
            "expected Bash marker in: {}",
            entry
        );
        assert!(
            entry.contains("make build"),
            "expected command in: {}",
            entry
        );
    }

    #[test]
    fn test_format_entry_bash_git_commit() {
        let entry = format_entry(
            "Bash",
            &InputFields {
                command: "git commit -m 'test'".into(),
                ..Default::default()
            },
            &OutputFields {
                stdout: "[main abc1234] test commit".into(),
                ..Default::default()
            },
        );
        assert!(
            entry.contains("**COMMIT**"),
            "expected COMMIT marker in: {}",
            entry
        );
        assert!(entry.contains("abc1234"), "expected SHA in: {}", entry);
    }

    #[test]
    fn test_format_entry_bash_git_push() {
        let entry = format_entry(
            "Bash",
            &InputFields {
                command: "git push origin feature".into(),
                ..Default::default()
            },
            &OutputFields {
                stderr: "abc1234..def5678 feature -> feature".into(),
                ..Default::default()
            },
        );
        assert!(
            entry.contains("**PUSH**"),
            "expected PUSH marker in: {}",
            entry
        );
        assert!(entry.contains("origin"), "expected remote in: {}", entry);
    }

    #[test]
    fn test_format_entry_bash_truncation() {
        let long_cmd = "x".repeat(300);
        let entry = format_entry(
            "Bash",
            &InputFields {
                command: long_cmd,
                ..Default::default()
            },
            &OutputFields::default(),
        );
        assert!(entry.contains("..."), "expected truncation indicator");
        assert!(entry.len() < 500, "entry should be truncated");
    }

    #[test]
    fn test_format_entry_mcp_tool() {
        let entry = format_entry(
            "mcp__github__create_pull_request",
            &InputFields {
                repo: "ChowNow/Hermosa".into(),
                title: "Fix bug".into(),
                ..Default::default()
            },
            &OutputFields::default(),
        );
        assert!(
            entry.contains("**PR Created**"),
            "expected PR Created marker in: {}",
            entry
        );
    }

    #[test]
    fn test_format_entry_jira_issue() {
        let entry = format_entry(
            "mcp__claude_ai_Atlassian__createJiraIssue",
            &InputFields {
                project_key: "CN".into(),
                summary: "New ticket".into(),
                ..Default::default()
            },
            &OutputFields::default(),
        );
        assert!(
            entry.contains("**Jira Issue**"),
            "expected Jira Issue marker in: {}",
            entry
        );
        assert!(entry.contains("CN"), "expected project key in: {}", entry);
    }

    #[test]
    fn test_days_to_date() {
        // 2024-01-01 = 19723 days since epoch
        let (y, m, d) = days_to_date(19723);
        assert_eq!((y, m, d), (2024, 1, 1));

        // 1970-01-01 = day 0
        let (y, m, d) = days_to_date(0);
        assert_eq!((y, m, d), (1970, 1, 1));
    }
}
