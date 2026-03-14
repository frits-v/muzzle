//! Git safety checks for Bash commands.
//!
//! FR-GS-1 through FR-GS-8: All 8 git safety patterns.

use regex::Regex;
use std::sync::LazyLock;

/// Result of a git safety check.
#[derive(Debug, Clone, PartialEq)]
pub enum GitResult {
    /// Command is safe to execute.
    Ok,
    /// Command is blocked with a reason message.
    Block(String),
}

/// Result of a gh merge check.
#[derive(Debug, Clone, PartialEq)]
pub struct AskResult {
    /// True if the user should be prompted before proceeding.
    pub should_ask: bool,
    /// Human-readable reason for the prompt.
    pub reason: String,
}

// Pre-compiled regexes for the 8 git safety patterns.
static RE_GIT_PUSH: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\bgit\b.*\bpush\b").unwrap());
static RE_FORCE_FLAG: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(\s--force(\s|$)|\s-f(\s|$))").unwrap());
static RE_FORCE_WITH_LEASE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\s--force-with-lease").unwrap());
static RE_PUSH_TO_MAIN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\bgit\b.*\bpush\s+\S+\s+(main|master)(\s|$)").unwrap());
static RE_REFSPEC_MAIN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\bgit\b.*\bpush\b.*:(refs/heads/)?(main|master)(\s|$)").unwrap());
static RE_DELETE_MAIN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\bgit\b.*\bpush\s.*--delete\s+(main|master)(\s|$)").unwrap());
static RE_DELETE_REFSPEC: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\bgit\b.*\bpush\s+\S+\s+:(main|master)(\s|$)").unwrap());
static RE_NO_VERIFY: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\bgit\b.*\bpush\b.*--no-verify").unwrap());
static RE_FOLLOW_TAGS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\bgit\b.*\bpush\b.*--follow-tags").unwrap());
static RE_DELETE_SEMVER_TAG: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\bgit\b.*\btag\s+-d\s+\S*v[0-9]+\.[0-9]+\.[0-9]+").unwrap());
static RE_DELETE_REMOTE_TAG: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\bgit\b.*\bpush\s+\S+\s+:refs/tags/\S*v[0-9]+\.[0-9]+\.[0-9]+").unwrap()
});
static RE_HARD_RESET: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\bgit\b.*\breset\s+--hard\s+origin/(main|master)").unwrap());
static RE_GH_PR_MERGE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\bgh\s+pr\s+merge\b").unwrap());
static RE_GH_API_MERGE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\bgh\s+api\b.*(/pulls/[0-9]+/merge|/merge)").unwrap());

// Worktree enforcement regexes
static RE_GIT_WORKTREE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\bgit\b[^;|&]*\bworktree\b").unwrap());
static RE_GIT_C: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"\bgit\s+-C\s+("[^"]+"|'[^']+'|\S+)"#).unwrap());
static RE_CD_PATH: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"\bcd\s+("[^"]+"|'[^']+'|\S+)"#).unwrap());
static RE_GIT_CHECKOUT_SWITCH: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\bgit\s+(checkout|switch)\b").unwrap());

// Bash write-path extraction regexes
static RE_REDIRECT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[12]?>>?\s*(/[^\s;|&)]+)").unwrap());
static RE_TEE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\btee\s+(?:-a\s+)?(/[^\s]+)").unwrap());
static RE_GIT_C_PATH: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"\bgit\s+-C\s+("[^"]+"|'[^']+'|(\S+))"#).unwrap());

/// Run all 8 git safety checks against a Bash command.
pub fn check_git_safety(cmd: &str) -> GitResult {
    // FR-GS-1: Force push without --force-with-lease
    if RE_GIT_PUSH.is_match(cmd)
        && RE_FORCE_FLAG.is_match(cmd)
        && !RE_FORCE_WITH_LEASE.is_match(cmd)
    {
        return GitResult::Block(
            "BLOCKED: Force push without --force-with-lease. Use: git push --force-with-lease origin <branch>".into(),
        );
    }

    // FR-GS-2: Push to main/master
    if RE_PUSH_TO_MAIN.is_match(cmd) {
        return GitResult::Block(
            "BLOCKED: Direct push to main/master. Create a feature branch and open a PR instead."
                .into(),
        );
    }

    // FR-GS-3: Refspec push to main/master
    if RE_REFSPEC_MAIN.is_match(cmd) {
        return GitResult::Block(
            "BLOCKED: Push to main/master via refspec. Create a feature branch and open a PR instead."
                .into(),
        );
    }

    // FR-GS-4: Delete main/master
    if RE_DELETE_MAIN.is_match(cmd) {
        return GitResult::Block("BLOCKED: Deleting main/master branch is not allowed.".into());
    }
    if RE_DELETE_REFSPEC.is_match(cmd) {
        return GitResult::Block(
            "BLOCKED: Deleting main/master branch via empty refspec is not allowed.".into(),
        );
    }

    // FR-GS-5: --no-verify
    if RE_NO_VERIFY.is_match(cmd) {
        return GitResult::Block(
            "BLOCKED: git push --no-verify bypasses pre-push hooks. Fix the hook failures instead."
                .into(),
        );
    }

    // FR-GS-6: --follow-tags
    if RE_FOLLOW_TAGS.is_match(cmd) {
        return GitResult::Block(
            "BLOCKED: git push --follow-tags pushes ALL matching local tags. Push tags explicitly: git push origin <tag>".into(),
        );
    }

    // FR-GS-7: Delete semver tags (local and remote)
    if RE_DELETE_SEMVER_TAG.is_match(cmd) {
        return GitResult::Block(
            "BLOCKED: Deleting semantic version tags is not allowed. Release a new patch version instead.".into(),
        );
    }
    if RE_DELETE_REMOTE_TAG.is_match(cmd) {
        return GitResult::Block(
            "BLOCKED: Deleting remote semantic version tags is not allowed. Release a new patch version instead.".into(),
        );
    }

    // FR-GS-8: Hard reset to origin/main|master
    if RE_HARD_RESET.is_match(cmd) {
        return GitResult::Block(
            "BLOCKED: git reset --hard origin/main|master discards all local work. Use: git stash or git reset --soft".into(),
        );
    }

    GitResult::Ok
}

/// Check if a command involves gh merge operations.
pub fn check_gh_merge(cmd: &str) -> AskResult {
    if RE_GH_PR_MERGE.is_match(cmd) {
        return AskResult {
            should_ask: true,
            reason: "gh pr merge — merging is a human decision".into(),
        };
    }
    if RE_GH_API_MERGE.is_match(cmd) {
        return AskResult {
            should_ask: true,
            reason: "gh api merge endpoint — merging is a human decision".into(),
        };
    }
    AskResult {
        should_ask: false,
        reason: String::new(),
    }
}

/// Check if a git command targets the main checkout when worktrees are active.
/// Returns Some(deny reason) or None.
pub fn check_worktree_enforcement(
    cmd: &str,
    worktree_active: bool,
    short_id: &str,
) -> Option<String> {
    if !worktree_active {
        return None;
    }

    // Only check git commands
    if !cmd.contains("git") {
        return None;
    }

    // Allow git worktree management commands
    if RE_GIT_WORKTREE.is_match(cmd) {
        return None;
    }

    let workspace = crate::config::workspace();
    let ws_str = workspace.to_string_lossy().to_string();

    // Check git -C <path>
    if let Some(caps) = RE_GIT_C.captures(cmd) {
        if let Some(m) = caps.get(1) {
            let git_path = m
                .as_str()
                .trim_matches(|c| c == '"' || c == '\'' || c == ' ');
            if is_main_checkout_path(git_path, &ws_str) {
                let repo = extract_repo_name(git_path, &ws_str);
                let wt_dir = format!("{}/{}/.worktrees/{}", ws_str, repo, short_id);
                if !std::path::Path::new(&wt_dir).exists() {
                    return Some(crate::worktree_missing_msg(&repo));
                }
                return Some(format!(
                    "BLOCKED: Git op on main checkout ({}). Use worktree: {}/{}/.worktrees/{}",
                    repo, ws_str, repo, short_id
                ));
            }
        }
    }

    // Check cd <path> && git ...
    if let Some(caps) = RE_CD_PATH.captures(cmd) {
        if let Some(m) = caps.get(1) {
            let cd_path = m
                .as_str()
                .trim_matches(|c| c == '"' || c == '\'' || c == ' ');
            if cmd.contains("git") && is_main_checkout_path(cd_path, &ws_str) {
                let repo = extract_repo_name(cd_path, &ws_str);
                let wt_dir = format!("{}/{}/.worktrees/{}", ws_str, repo, short_id);
                if !std::path::Path::new(&wt_dir).exists() {
                    return Some(crate::worktree_missing_msg(&repo));
                }
                return Some(format!(
                    "BLOCKED: Git op on main checkout ({}). Use worktree: {}/{}/.worktrees/{}",
                    repo, ws_str, repo, short_id
                ));
            }
        }
    }

    // Block bare git checkout/switch with no path context
    if !RE_GIT_C.is_match(cmd) && !RE_CD_PATH.is_match(cmd) && RE_GIT_CHECKOUT_SWITCH.is_match(cmd)
    {
        return Some(format!(
            "BLOCKED: Bare git checkout/switch — worktrees are active. Use: git -C <repo>/.worktrees/{}/ checkout ...",
            short_id
        ));
    }

    None
}

/// Extract write-target paths from a Bash command.
pub fn check_bash_write_paths(cmd: &str) -> Vec<String> {
    let mut paths = Vec::new();

    // Redirect targets
    for caps in RE_REDIRECT.captures_iter(cmd) {
        if let Some(m) = caps.get(1) {
            let p = m.as_str().trim();
            if p.starts_with('/') {
                paths.push(p.to_string());
            }
        }
    }

    // Tee targets
    for caps in RE_TEE.captures_iter(cmd) {
        if let Some(m) = caps.get(1) {
            let p = m.as_str().trim();
            if p.starts_with('/') {
                paths.push(p.to_string());
            }
        }
    }

    // git -C path (prefixed to distinguish)
    for caps in RE_GIT_C_PATH.captures_iter(cmd) {
        if let Some(m) = caps.get(1) {
            let p = m
                .as_str()
                .trim_matches(|c| c == '"' || c == '\'' || c == ' ');
            if p.starts_with('/') {
                paths.push(format!("gitc:{}", p));
            }
        }
    }

    paths
}

/// Check if a path is a main checkout (not .worktrees/ or .claude-tmp/).
fn is_main_checkout_path(path: &str, workspace: &str) -> bool {
    let prefix = format!("{}/", workspace);
    if !path.starts_with(&prefix) {
        return false;
    }
    if path.contains("/.claude-tmp/") || path.contains("/.worktrees/") {
        return false;
    }
    true
}

/// Extract the repo directory name from a workspace path.
fn extract_repo_name(path: &str, workspace: &str) -> String {
    let prefix = format!("{}/", workspace);
    if let Some(rest) = path.strip_prefix(&prefix) {
        if let Some(idx) = rest.find('/') {
            return rest[..idx].to_string();
        }
        return rest.to_string();
    }
    String::new()
}

/// Extract the repo name from a git command targeting a workspace repo.
///
/// Recognizes two patterns:
/// - `git -C <workspace>/<repo>[/...] ...`
/// - `cd <workspace>/<repo>[/...] && git ...`
///
/// Returns `Some(repo_name)` if the command targets a workspace repo, `None` otherwise.
pub fn extract_repo_from_git_op(cmd: &str) -> Option<String> {
    static RE_GIT_WORD: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\bgit\b").unwrap());

    if !RE_GIT_WORD.is_match(cmd) {
        return None;
    }

    let ws = crate::config::workspace();
    let ws_str = ws.to_string_lossy().to_string();

    // git -C <workspace-path>/<repo>
    if cmd.contains("-C") {
        let pattern = format!(
            r#"\bgit\b[^;|&]*-C\s+["']?({}/(\S+?))[/"'\s]"#,
            regex::escape(&ws_str)
        );
        if let Ok(re) = Regex::new(&pattern) {
            if let Some(caps) = re.captures(cmd) {
                if let Some(m) = caps.get(1) {
                    let full_path = m.as_str().trim_matches(|c| c == '"' || c == '\'');
                    let name = extract_repo_name(full_path, &ws_str);
                    if !name.is_empty() {
                        return Some(name);
                    }
                }
            }
        }
        // Fallback: try the broader pattern for paths without trailing slash
        let pattern2 = format!(r"\bgit\b[^;|&]*-C\s+\S*{}", regex::escape(&ws_str));
        if let Ok(re) = Regex::new(&pattern2) {
            if let Some(caps) = re.captures(cmd) {
                if let Some(m) = caps.get(0) {
                    let text = m.as_str();
                    // Extract path after -C
                    if let Some(c_idx) = text.find("-C") {
                        let after_c = text[c_idx + 2..].trim_start();
                        let path = after_c.split_whitespace().next().unwrap_or("");
                        let path = path.trim_matches(|c| c == '"' || c == '\'');
                        let name = extract_repo_name(path, &ws_str);
                        if !name.is_empty() {
                            return Some(name);
                        }
                    }
                }
            }
        }
    }

    // cd <workspace-path>/<repo> && git
    if cmd.contains("cd") {
        let pattern = format!(r"\bcd\s+\S*{}\S*\s*[;&|]+.*\bgit\b", regex::escape(&ws_str));
        if let Ok(re) = Regex::new(&pattern) {
            if re.is_match(cmd) {
                // Extract the cd path
                if let Some(caps) = RE_CD_PATH.captures(cmd) {
                    if let Some(m) = caps.get(1) {
                        let cd_path = m.as_str().trim_matches(|c| c == '"' || c == '\'');
                        let name = extract_repo_name(cd_path, &ws_str);
                        if !name.is_empty() {
                            return Some(name);
                        }
                    }
                }
            }
        }
    }

    None
}

/// Check if a git command targets a workspace repo via -C or cd.
/// Uses proper regex instead of broad string matching (fixes Go bug #2).
pub fn is_repo_git_op(cmd: &str) -> bool {
    extract_repo_from_git_op(cmd).is_some()
}

/// Check if a command is managing worktrees.
pub fn is_worktree_management_op(cmd: &str) -> bool {
    cmd.contains("worktree")
}

#[cfg(test)]
mod tests {
    use super::*;

    // FR-GS-1: Force push without --force-with-lease
    #[test]
    fn test_force_push_without_lease() {
        let blocked = [
            "git push --force origin feature",
            "git push -f origin feature",
            "git -C /some/path push --force origin branch",
        ];
        for cmd in &blocked {
            let r = check_git_safety(cmd);
            assert!(
                matches!(r, GitResult::Block(_)),
                "expected BLOCK for {:?}",
                cmd
            );
        }

        let allowed = [
            "git push --force-with-lease origin feature",
            "git push --force --force-with-lease origin feature",
        ];
        for cmd in &allowed {
            let r = check_git_safety(cmd);
            assert!(matches!(r, GitResult::Ok), "expected OK for {:?}", cmd);
        }
    }

    // FR-GS-2: Push to main/master
    #[test]
    fn test_push_to_main() {
        let blocked = [
            "git push origin main",
            "git push origin master",
            "git -C /path push origin main",
        ];
        for cmd in &blocked {
            let r = check_git_safety(cmd);
            assert!(
                matches!(r, GitResult::Block(_)),
                "expected BLOCK for {:?}",
                cmd
            );
        }

        let allowed = [
            "git push origin feature",
            "git push origin my-branch",
            "git push origin main-feature",
        ];
        for cmd in &allowed {
            let r = check_git_safety(cmd);
            assert!(matches!(r, GitResult::Ok), "expected OK for {:?}", cmd);
        }
    }

    // FR-GS-3: Refspec push to main/master
    #[test]
    fn test_refspec_push_to_main() {
        let blocked = [
            "git push origin feature:main",
            "git push origin feature:master",
            "git push origin feature:refs/heads/main",
        ];
        for cmd in &blocked {
            let r = check_git_safety(cmd);
            assert!(
                matches!(r, GitResult::Block(_)),
                "expected BLOCK for refspec {:?}",
                cmd
            );
        }
    }

    // FR-GS-4: Delete main/master
    #[test]
    fn test_delete_main() {
        let blocked = [
            "git push origin --delete main",
            "git push origin --delete master",
            "git push origin :main",
            "git push origin :master",
        ];
        for cmd in &blocked {
            let r = check_git_safety(cmd);
            assert!(
                matches!(r, GitResult::Block(_)),
                "expected BLOCK for delete {:?}",
                cmd
            );
        }
    }

    // FR-GS-5: --no-verify
    #[test]
    fn test_no_verify() {
        let r = check_git_safety("git push --no-verify origin feature");
        assert!(matches!(r, GitResult::Block(_)));
    }

    // FR-GS-6: --follow-tags
    #[test]
    fn test_follow_tags() {
        let r = check_git_safety("git push --follow-tags origin feature");
        assert!(matches!(r, GitResult::Block(_)));
    }

    // FR-GS-7: Delete semver tags
    #[test]
    fn test_delete_semver_tags() {
        let blocked = [
            "git tag -d v1.0.0",
            "git tag -d module-v3.0.0",
            "git push origin :refs/tags/v1.2.3",
            "git push origin :refs/tags/module-v1.0.0",
        ];
        for cmd in &blocked {
            let r = check_git_safety(cmd);
            assert!(
                matches!(r, GitResult::Block(_)),
                "expected BLOCK for semver tag delete {:?}",
                cmd
            );
        }
    }

    // FR-GS-8: Hard reset to origin/main|master
    #[test]
    fn test_hard_reset() {
        let blocked = [
            "git reset --hard origin/main",
            "git reset --hard origin/master",
        ];
        for cmd in &blocked {
            let r = check_git_safety(cmd);
            assert!(
                matches!(r, GitResult::Block(_)),
                "expected BLOCK for hard reset {:?}",
                cmd
            );
        }

        let allowed = [
            "git reset --hard HEAD~1",
            "git reset --hard origin/feature",
            "git reset --soft origin/main",
        ];
        for cmd in &allowed {
            let r = check_git_safety(cmd);
            assert!(matches!(r, GitResult::Ok), "expected OK for {:?}", cmd);
        }
    }

    #[test]
    fn test_gh_merge() {
        let ask_cmds = [
            "gh pr merge 123",
            "gh pr merge --auto",
            "gh api repos/owner/repo/pulls/123/merge",
        ];
        for cmd in &ask_cmds {
            let r = check_gh_merge(cmd);
            assert!(r.should_ask, "expected ASK for {:?}", cmd);
        }

        let no_cmds = [
            "gh pr view 123",
            "gh pr list",
            "gh api repos/owner/repo/pulls/123",
        ];
        for cmd in &no_cmds {
            let r = check_gh_merge(cmd);
            assert!(!r.should_ask, "expected no-ask for {:?}", cmd);
        }
    }

    #[test]
    fn test_worktree_enforcement_main_checkout_deny() {
        let ws = crate::config::workspace();
        let cmd = format!("git -C {}/web-app status", ws.display());
        let reason = check_worktree_enforcement(&cmd, true, "abc12345");
        assert!(reason.is_some(), "expected deny for git on main checkout");
    }

    #[test]
    fn test_worktree_enforcement_worktree_allow() {
        let ws = crate::config::workspace();
        let cmd = format!("git -C {}/web-app/.worktrees/abc12345 status", ws.display());
        let reason = check_worktree_enforcement(&cmd, true, "abc12345");
        assert!(
            reason.is_none(),
            "expected allow for worktree path, got: {:?}",
            reason
        );
    }

    #[test]
    fn test_worktree_enforcement_worktree_management() {
        let ws = crate::config::workspace();
        let cmd = format!("git -C {}/web-app worktree add /path", ws.display());
        let reason = check_worktree_enforcement(&cmd, true, "abc12345");
        assert!(
            reason.is_none(),
            "expected allow for worktree management, got: {:?}",
            reason
        );
    }

    #[test]
    fn test_worktree_enforcement_not_active() {
        let ws = crate::config::workspace();
        let cmd = format!("git -C {}/web-app status", ws.display());
        let reason = check_worktree_enforcement(&cmd, false, "abc12345");
        assert!(reason.is_none(), "expected no enforcement when inactive");
    }

    #[test]
    fn test_worktree_enforcement_bare_checkout() {
        let reason = check_worktree_enforcement("git checkout feature-branch", true, "abc12345");
        assert!(reason.is_some(), "expected deny for bare git checkout");
    }

    #[test]
    fn test_bash_write_paths_redirect() {
        let paths = check_bash_write_paths("echo hello > /tmp/test.txt 2> /var/log/err");
        let non_gitc: Vec<_> = paths.iter().filter(|p| !p.starts_with("gitc:")).collect();
        assert_eq!(
            non_gitc.len(),
            2,
            "expected 2 redirect paths, got {:?}",
            non_gitc
        );
    }

    #[test]
    fn test_bash_write_paths_tee() {
        let paths = check_bash_write_paths("cat file | tee /tmp/output.txt");
        assert!(
            paths.iter().any(|p| p == "/tmp/output.txt"),
            "expected /tmp/output.txt in paths: {:?}",
            paths
        );
    }

    #[test]
    fn test_extract_repo_from_git_op_git_c() {
        let ws = crate::config::workspace();
        let cmd = format!("git -C {}/web-app status", ws.display());
        let repo = extract_repo_from_git_op(&cmd);
        assert_eq!(
            repo.as_deref(),
            Some("web-app"),
            "should extract web-app from git -C"
        );
    }

    #[test]
    fn test_extract_repo_from_git_op_git_c_subpath() {
        let ws = crate::config::workspace();
        let cmd = format!("git -C {}/ops/modules/foo log", ws.display());
        let repo = extract_repo_from_git_op(&cmd);
        assert_eq!(
            repo.as_deref(),
            Some("ops"),
            "should extract ops from nested path"
        );
    }

    #[test]
    fn test_extract_repo_from_git_op_cd_pattern() {
        let ws = crate::config::workspace();
        let cmd = format!("cd {}/ops && git status", ws.display());
        let repo = extract_repo_from_git_op(&cmd);
        assert_eq!(
            repo.as_deref(),
            Some("ops"),
            "should extract ops from cd pattern"
        );
    }

    #[test]
    fn test_extract_repo_from_git_op_non_workspace() {
        let repo = extract_repo_from_git_op("git -C /tmp/foo status");
        assert!(repo.is_none(), "should return None for non-workspace path");
    }

    #[test]
    fn test_extract_repo_from_git_op_no_git() {
        let repo = extract_repo_from_git_op("echo hello");
        assert!(repo.is_none(), "should return None for non-git command");
    }

    #[test]
    fn test_bash_write_paths_no_absolute() {
        let paths = check_bash_write_paths("echo hello > relative.txt");
        assert!(
            !paths.iter().any(|p| p == "relative.txt"),
            "should not extract relative paths: {:?}",
            paths
        );
    }
}
