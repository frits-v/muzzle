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
// Bare mutating git detection (segment splitting + subcommand extraction)
// Matches &&, ||, then single ;, |, or & (background). || before [;|&] so double-pipe isn't split.
static RE_CMD_SEP: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"&&|\|\||[;|&]").unwrap());
static RE_GIT_WORD_BOUNDARY: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\bgit\b").unwrap());

/// Mutating git subcommands that must target a worktree, not CWD.
const MUTATING_GIT_SUBCMDS: &[&str] = &[
    "add",
    "am",
    "apply",
    "checkout",
    "cherry-pick",
    "clean",
    "commit",
    "merge",
    "mv",
    "pull",
    "push",
    "rebase",
    "reset",
    "restore",
    "revert",
    "rm",
    "stash",
    "switch",
];

/// Git global flags that consume a separate argument token (argument is skipped
/// during subcommand extraction). Note: only `-C` is treated as a working-dir
/// context flag; `--git-dir`/`--work-tree`/`--namespace` are consumed for
/// correct parsing but do NOT suppress the bare-command check.
const GIT_FLAGS_WITH_ARG: &[&str] = &["-C", "-c", "--git-dir", "--work-tree", "--namespace"];

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

    let workspaces = crate::config::workspaces();

    // Check git -C <path>
    if let Some(caps) = RE_GIT_C.captures(cmd) {
        if let Some(m) = caps.get(1) {
            let git_path = m
                .as_str()
                .trim_matches(|c| c == '"' || c == '\'' || c == ' ');
            for ws in &workspaces {
                let ws_str = ws.to_string_lossy().to_string();
                if is_main_checkout_path(git_path, &ws_str) {
                    let repo = extract_repo_name(git_path, &ws_str);
                    let wt_dir = format!("{}/{}/.worktrees/{}", ws_str, repo, short_id);
                    if !std::path::Path::new(&wt_dir).exists() {
                        return Some(crate::worktree_missing_msg(&repo));
                    }
                    return Some(format!(
                        "BLOCKED: Git op on main checkout ({repo}). \
                         Use worktree: {ws_str}/{repo}/.worktrees/{short_id}. \
                         Tip: run git -C <wt-path> fetch origin before creating new branches"
                    ));
                }
            }
        }
    }

    // Check cd <path> && git ...
    if let Some(caps) = RE_CD_PATH.captures(cmd) {
        if let Some(m) = caps.get(1) {
            let cd_path = m
                .as_str()
                .trim_matches(|c| c == '"' || c == '\'' || c == ' ');
            for ws in &workspaces {
                let ws_str = ws.to_string_lossy().to_string();
                if cmd.contains("git") && is_main_checkout_path(cd_path, &ws_str) {
                    let repo = extract_repo_name(cd_path, &ws_str);
                    let wt_dir = format!("{}/{}/.worktrees/{}", ws_str, repo, short_id);
                    if !std::path::Path::new(&wt_dir).exists() {
                        return Some(crate::worktree_missing_msg(&repo));
                    }
                    return Some(format!(
                        "BLOCKED: Git op on main checkout ({repo}). \
                         Use worktree: {ws_str}/{repo}/.worktrees/{short_id}. \
                         Tip: run git -C <wt-path> fetch origin before creating new branches"
                    ));
                }
            }
        }
    }

    // Block bare mutating git commands (no -C, no cd context).
    // When worktrees are active, mutating git ops must target the worktree explicitly.
    // Per-segment analysis: each command segment is checked independently for -C and cd.
    if let Some(subcmd) = find_bare_mutating_git(cmd) {
        return Some(format!(
            "BLOCKED: Bare 'git {subcmd}' runs in CWD (main checkout), not the worktree. \
             Use: git -C <repo>/.worktrees/{short_id}/ {subcmd} ..."
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

    let workspaces = crate::config::workspaces();

    // git -C <workspace-path>/<repo>
    if cmd.contains("-C") {
        for ws in &workspaces {
            let ws_str = ws.to_string_lossy().to_string();
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
        }
        // Fallback: try the broader pattern for paths without trailing slash
        let ws_str = workspaces
            .first()
            .map(|w| w.to_string_lossy().to_string())
            .unwrap_or_default();
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
        for ws in &workspaces {
            let ws_str = ws.to_string_lossy().to_string();
            let pattern = format!(r"\bcd\s+\S*{}\S*\s*[;&|]+.*\bgit\b", regex::escape(&ws_str));
            if let Ok(re) = Regex::new(&pattern) {
                if re.is_match(cmd) {
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

/// Find a bare (no `-C`, no `cd` context) mutating git subcommand in a
/// (possibly compound) command.
///
/// Splits on shell separators (`&&`, `||`, `;`, `|`, `&`) and checks each
/// segment independently. A segment is "bare" when the git invocation has
/// no `-C` flag AND no preceding `cd` in the same segment.
///
/// Returns the subcommand name if a bare mutating invocation is found.
fn find_bare_mutating_git(cmd: &str) -> Option<String> {
    for segment in RE_CMD_SEP.split(cmd) {
        // Strip shell-comment tail so `# git add` or `# cd /path` in
        // trailing comments don't trigger false positives or bypasses.
        let segment = strip_shell_comment(segment.trim());
        if !RE_GIT_WORD_BOUNDARY.is_match(&segment) {
            continue;
        }
        if RE_CD_PATH.is_match(&segment) {
            continue;
        }
        if let Some(result) = extract_git_subcommand(&segment) {
            // Skip if git had -C flag (explicit working directory)
            if result.had_dir_flag {
                continue;
            }
            if MUTATING_GIT_SUBCMDS.contains(&result.subcommand) {
                return Some(result.subcommand.to_string());
            }
        }
    }
    None
}

/// Result of extracting a git subcommand from a command segment.
struct GitSubcommand<'a> {
    /// The subcommand name (e.g. "add", "commit", "status").
    subcommand: &'a str,
    /// True if `-C` was seen before the subcommand (explicit working directory).
    had_dir_flag: bool,
}

/// Extract the git subcommand (first non-flag token after `git`).
///
/// Walks tokens after `git`, consuming flag arguments from [`GIT_FLAGS_WITH_ARG`].
/// Tracks whether `-C` was encountered to distinguish `git -C /path add`
/// (not bare) from `git add` (bare).
fn extract_git_subcommand(segment: &str) -> Option<GitSubcommand<'_>> {
    let m = RE_GIT_WORD_BOUNDARY.find(segment)?;
    let after_git = &segment[m.end()..];
    // Skip git-lfs, git-annex, git-crypt, etc. — these are separate binaries
    if after_git.starts_with('-') {
        return None;
    }
    let mut words = after_git.split_whitespace();
    let mut had_dir_flag = false;
    while let Some(word) = words.next() {
        // Flags that consume the next token as their argument
        if GIT_FLAGS_WITH_ARG.contains(&word) {
            if word == "-C" {
                had_dir_flag = true;
            }
            // Skip the argument — handle quoted values spanning multiple tokens
            // (e.g. `-c "user.name=Mr Test"` splits into `"user.name=Mr` and `Test"`)
            if let Some(arg) = words.next() {
                if let Some(quote) = arg.as_bytes().first().copied() {
                    if (quote == b'"' || quote == b'\'')
                        && !arg
                            .as_bytes()
                            .last()
                            .is_some_and(|&b| b == quote && arg.len() > 1)
                    {
                        for w in words.by_ref() {
                            if w.as_bytes().last() == Some(&quote) {
                                break;
                            }
                        }
                    }
                }
            }
            continue;
        }
        // Other flags (--flag, -f, --key=value)
        if word.starts_with('-') {
            continue;
        }
        return Some(GitSubcommand {
            subcommand: word,
            had_dir_flag,
        });
    }
    None
}

/// Strip a shell comment from a command segment.
///
/// Removes everything from the first unquoted `#` to the end of the string.
/// Respects single and double quotes (does not strip `#` inside quotes).
fn strip_shell_comment(s: &str) -> String {
    let mut in_single = false;
    let mut in_double = false;
    for (i, c) in s.char_indices() {
        match c {
            '\'' if !in_double => in_single = !in_single,
            '"' if !in_single => in_double = !in_double,
            '#' if !in_single && !in_double => return s[..i].to_string(),
            _ => {}
        }
    }
    s.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    // Use the crate-level ENV_LOCK shared across all modules
    use crate::ENV_LOCK;

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
        let _lock = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let fixed_ws = "/tmp/muzzle-test-ws";
        std::env::set_var("MUZZLE_WORKSPACE", fixed_ws);
        let cmd = format!("git -C {fixed_ws}/web-app status");
        let reason = check_worktree_enforcement(&cmd, true, "abc12345");
        std::env::remove_var("MUZZLE_WORKSPACE");
        assert!(reason.is_some(), "expected deny for git on main checkout");
    }

    #[test]
    fn test_worktree_enforcement_worktree_allow() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let fixed_ws = "/tmp/muzzle-test-ws";
        std::env::set_var("MUZZLE_WORKSPACE", fixed_ws);
        let cmd = format!("git -C {fixed_ws}/web-app/.worktrees/abc12345 status");
        let reason = check_worktree_enforcement(&cmd, true, "abc12345");
        std::env::remove_var("MUZZLE_WORKSPACE");
        assert!(
            reason.is_none(),
            "expected allow for worktree path, got: {:?}",
            reason
        );
    }

    #[test]
    fn test_worktree_enforcement_worktree_management() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let fixed_ws = "/tmp/muzzle-test-ws";
        std::env::set_var("MUZZLE_WORKSPACE", fixed_ws);
        let cmd = format!("git -C {fixed_ws}/web-app worktree add /path");
        let reason = check_worktree_enforcement(&cmd, true, "abc12345");
        std::env::remove_var("MUZZLE_WORKSPACE");
        assert!(
            reason.is_none(),
            "expected allow for worktree management, got: {:?}",
            reason
        );
    }

    #[test]
    fn test_worktree_enforcement_not_active() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let fixed_ws = "/tmp/muzzle-test-ws";
        std::env::set_var("MUZZLE_WORKSPACE", fixed_ws);
        let cmd = format!("git -C {fixed_ws}/web-app status");
        let reason = check_worktree_enforcement(&cmd, false, "abc12345");
        std::env::remove_var("MUZZLE_WORKSPACE");
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
        let _lock = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let fixed_ws = "/tmp/muzzle-test-ws";
        std::env::set_var("MUZZLE_WORKSPACE", fixed_ws);
        let cmd = format!("git -C {fixed_ws}/web-app status");
        let repo = extract_repo_from_git_op(&cmd);
        std::env::remove_var("MUZZLE_WORKSPACE");
        assert_eq!(
            repo.as_deref(),
            Some("web-app"),
            "should extract web-app from git -C"
        );
    }

    #[test]
    fn test_extract_repo_from_git_op_git_c_subpath() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let fixed_ws = "/tmp/muzzle-test-ws";
        std::env::set_var("MUZZLE_WORKSPACE", fixed_ws);
        let cmd = format!("git -C {fixed_ws}/ops/modules/foo log");
        let repo = extract_repo_from_git_op(&cmd);
        std::env::remove_var("MUZZLE_WORKSPACE");
        assert_eq!(
            repo.as_deref(),
            Some("ops"),
            "should extract ops from nested path"
        );
    }

    #[test]
    fn test_extract_repo_from_git_op_cd_pattern() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let fixed_ws = "/tmp/muzzle-test-ws";
        std::env::set_var("MUZZLE_WORKSPACE", fixed_ws);
        let cmd = format!("cd {fixed_ws}/ops && git status");
        let repo = extract_repo_from_git_op(&cmd);
        std::env::remove_var("MUZZLE_WORKSPACE");
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

    #[test]
    fn test_is_repo_git_op() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let fixed_ws = "/tmp/muzzle-test-ws";
        std::env::set_var("MUZZLE_WORKSPACE", fixed_ws);
        assert!(is_repo_git_op(&format!("git -C {fixed_ws}/web-app status")));
        std::env::remove_var("MUZZLE_WORKSPACE");
        assert!(!is_repo_git_op("git status"));
        assert!(!is_repo_git_op("echo hello"));
    }

    #[test]
    fn test_is_worktree_management_op() {
        assert!(is_worktree_management_op("git worktree add /path"));
        assert!(is_worktree_management_op("git worktree list"));
        assert!(is_worktree_management_op("git worktree remove /p"));
        assert!(!is_worktree_management_op("git status"));
        assert!(!is_worktree_management_op("git branch -a"));
        // Note: uses contains(), so any mention of "worktree" matches
        assert!(is_worktree_management_op("echo worktree"));
    }

    #[test]
    fn test_safe_git_commands_not_blocked() {
        let safe = [
            "git status",
            "git log --oneline -10",
            "git diff HEAD",
            "git branch -a",
            "git fetch origin",
            "git stash",
            "git stash pop",
            "git add src/main.rs",
            "git commit -m 'test'",
        ];
        for cmd in &safe {
            let r = check_git_safety(cmd);
            assert!(matches!(r, GitResult::Ok), "expected OK for {:?}", cmd);
        }
    }

    #[test]
    fn test_non_git_commands_not_blocked() {
        let safe = ["ls -la", "cargo build", "cat file.txt", "make test"];
        for cmd in &safe {
            let r = check_git_safety(cmd);
            assert!(matches!(r, GitResult::Ok), "expected OK for {:?}", cmd);
        }
    }

    // Verify that \bgit\b regex matches even inside echo — defense-in-depth
    #[test]
    fn test_git_in_echo_still_blocked() {
        let r = check_git_safety("echo git push --force origin feat");
        assert!(
            matches!(r, GitResult::Block(_)),
            "defense-in-depth: git inside echo is still blocked"
        );
    }

    // ── Bare mutating git detection ──────────────────────────────────

    #[test]
    fn test_bare_add_commit_push_blocked() {
        let cmd = "git add file.rs && git commit -m 'msg' && git push origin branch";
        let reason = check_worktree_enforcement(cmd, true, "abc12345");
        assert!(reason.is_some(), "bare add+commit+push should be blocked");
        let msg = reason.unwrap();
        assert!(msg.contains("git add"), "should identify 'add': {msg}");
    }

    #[test]
    fn test_bare_commit_amend_blocked() {
        let cmd = "git add . && git commit --amend --no-edit && git push --force-with-lease origin fv/branch";
        let reason = check_worktree_enforcement(cmd, true, "abc12345");
        assert!(reason.is_some(), "bare commit --amend should be blocked");
    }

    #[test]
    fn test_bare_rebase_blocked() {
        let reason = check_worktree_enforcement("git rebase origin/main", true, "abc12345");
        assert!(reason.is_some(), "bare rebase should be blocked");
    }

    #[test]
    fn test_bare_stash_blocked() {
        let reason = check_worktree_enforcement("git stash pop", true, "abc12345");
        assert!(reason.is_some(), "bare stash pop should be blocked");
    }

    #[test]
    fn test_bare_merge_blocked() {
        let reason = check_worktree_enforcement("git merge feature-branch", true, "abc12345");
        assert!(reason.is_some(), "bare merge should be blocked");
    }

    #[test]
    fn test_bare_reset_blocked() {
        let reason = check_worktree_enforcement("git reset HEAD~1", true, "abc12345");
        assert!(reason.is_some(), "bare reset should be blocked");
    }

    #[test]
    fn test_bare_pull_blocked() {
        let reason = check_worktree_enforcement("git pull origin main", true, "abc12345");
        assert!(reason.is_some(), "bare pull should be blocked");
    }

    #[test]
    fn test_bare_readonly_allowed() {
        let allowed = [
            "git status",
            "git log --oneline -10",
            "git diff HEAD",
            "git branch -a",
            "git fetch origin",
            "git remote -v",
            "git describe --tags",
            "git rev-parse HEAD",
        ];
        for cmd in &allowed {
            let reason = check_worktree_enforcement(cmd, true, "abc12345");
            assert!(reason.is_none(), "read-only '{}' should be allowed", cmd);
        }
    }

    #[test]
    fn test_compound_with_c_and_bare_blocked() {
        let cmd = "git -C /ws/repo/.worktrees/abc12345 fetch && git add .";
        let reason = check_worktree_enforcement(cmd, true, "abc12345");
        assert!(
            reason.is_some(),
            "bare 'git add' after -C fetch should be blocked"
        );
    }

    #[test]
    fn test_git_with_c_not_flagged_as_bare() {
        let cmd = "git -C /wt/path commit -m 'msg'";
        let reason = find_bare_mutating_git(cmd);
        assert!(
            reason.is_none(),
            "-C commit should not be flagged as bare: {:?}",
            reason
        );
    }

    #[test]
    fn test_commit_message_no_false_positive() {
        // "merge" in the commit message should NOT trigger — "commit" is the subcommand
        let cmd = "git commit -m 'merge branch X into Y'";
        let reason = check_worktree_enforcement(cmd, true, "abc12345");
        assert!(reason.is_some(), "bare commit should be blocked");
        let msg = reason.unwrap();
        assert!(
            msg.contains("git commit"),
            "should identify 'commit', not 'merge': {msg}"
        );
    }

    #[test]
    fn test_cd_to_worktree_allows_bare_git() {
        // cd to worktree in the SAME segment as git → allowed
        let cmd = "cd /ws/repo/.worktrees/abc12345 && git add . && git commit -m 'msg'";
        let reason = check_worktree_enforcement(cmd, true, "abc12345");
        // The cd is in segment 1, but git add/commit are in segments 2 and 3.
        // Per-segment: segments 2 and 3 have no cd → bare → blocked.
        // This is CORRECT: cd in a previous segment doesn't set CWD for later segments
        // in the permissions hook (which sees the full command pre-execution).
        // The SHELL would change dirs, but the hook can't know that.
        // Users should use `git -C <wt-path>` instead of `cd && git`.
        assert!(
            reason.is_some(),
            "cd in separate segment from git should still block bare git"
        );
    }

    #[test]
    fn test_cd_same_segment_allows_git() {
        // cd and git in the SAME shell segment (not separated by &&)
        // This is unusual but the per-segment cd check handles it
        let cmd = "cd /ws/repo/.worktrees/abc12345; git add .";
        // `;` splits into two segments, so git add is still bare → blocked
        let reason = check_worktree_enforcement(cmd, true, "abc12345");
        assert!(reason.is_some(), "cd in different segment should block");
    }

    #[test]
    fn test_cd_tmp_does_not_bypass_bare_check() {
        // Regression: cd /tmp should NOT bypass the bare git check
        let cmd = "cd /tmp && git add /ws/main-repo/important.rs";
        let reason = check_worktree_enforcement(cmd, true, "abc12345");
        assert!(reason.is_some(), "cd /tmp should not bypass bare git check");
    }

    #[test]
    fn test_ssh_c_flag_does_not_bypass() {
        // Regression: -C inside a quoted SSH command should not skip detection
        let cmd = "env GIT_SSH_COMMAND=\"ssh -C\" git add .";
        let reason = check_worktree_enforcement(cmd, true, "abc12345");
        assert!(
            reason.is_some(),
            "SSH -C in quotes should not bypass bare git check"
        );
    }

    #[test]
    fn test_quoted_c_flag_does_not_bypass() {
        // Regression: `-c "key=val with spaces"` should not break subcommand extraction
        let cmd = "git -c \"user.name=Mr Test\" add .";
        let reason = check_worktree_enforcement(cmd, true, "abc12345");
        assert!(
            reason.is_some(),
            "quoted -c value with spaces should not bypass bare git check"
        );
    }

    #[test]
    fn test_git_extension_no_false_positive() {
        // git-lfs, git-annex, git-crypt are separate binaries, not bare git
        let cmds = [
            "git-lfs push origin branch",
            "git-annex add .",
            "git-crypt unlock",
        ];
        for cmd in &cmds {
            let reason = check_worktree_enforcement(cmd, true, "abc12345");
            assert!(
                reason.is_none(),
                "git extension '{}' should not be blocked",
                cmd
            );
        }
    }

    #[test]
    fn test_shell_comment_cd_does_not_bypass() {
        // Regression: `# cd /path` in a comment should not skip the bare git check
        let cmd = "git add . # cd /ws/repo";
        let reason = check_worktree_enforcement(cmd, true, "abc12345");
        assert!(
            reason.is_some(),
            "shell comment with cd should not bypass bare git check"
        );
    }

    #[test]
    fn test_shell_comment_git_no_false_positive() {
        // Regression: `# git add` in a comment should not cause a false block
        let cmd = "cargo test # git add checkpoint";
        let reason = check_worktree_enforcement(cmd, true, "abc12345");
        assert!(
            reason.is_none(),
            "git in shell comment should not trigger block, got: {:?}",
            reason
        );
    }

    #[test]
    fn test_strip_shell_comment() {
        assert_eq!(strip_shell_comment("git add . # comment"), "git add . ");
        assert_eq!(strip_shell_comment("git add ."), "git add .");
        assert_eq!(
            strip_shell_comment("git commit -m 'msg # not a comment'"),
            "git commit -m 'msg # not a comment'"
        );
        assert_eq!(
            strip_shell_comment("git commit -m \"msg # not a comment\""),
            "git commit -m \"msg # not a comment\""
        );
    }

    #[test]
    fn test_extract_git_subcommand_simple() {
        fn subcmd(s: &str) -> Option<&str> {
            extract_git_subcommand(s).map(|r| r.subcommand)
        }
        assert_eq!(subcmd("git add ."), Some("add"));
        assert_eq!(subcmd("git commit -m 'msg'"), Some("commit"));
        assert_eq!(subcmd("git status"), Some("status"));
    }

    #[test]
    fn test_extract_git_subcommand_with_flags() {
        fn subcmd(s: &str) -> Option<&str> {
            extract_git_subcommand(s).map(|r| r.subcommand)
        }
        assert_eq!(subcmd("git --no-pager log"), Some("log"));
        assert_eq!(subcmd("git -c core.editor=vim commit"), Some("commit"));
    }

    #[test]
    fn test_extract_git_subcommand_with_c_flag() {
        // -C consumes next token; subcommand follows; had_dir_flag is set
        let result = extract_git_subcommand("git -C /some/path status");
        assert_eq!(result.as_ref().map(|r| r.subcommand), Some("status"));
        assert!(
            result.as_ref().map(|r| r.had_dir_flag).unwrap_or(false),
            "-C should set had_dir_flag"
        );
    }

    #[test]
    fn test_find_bare_mutating_git_none_for_readonly() {
        assert!(find_bare_mutating_git("git status").is_none());
        assert!(find_bare_mutating_git("git log --oneline").is_none());
        assert!(find_bare_mutating_git("git diff HEAD").is_none());
        assert!(find_bare_mutating_git("git fetch origin").is_none());
    }

    #[test]
    fn test_find_bare_mutating_git_detects_all_subcmds() {
        for subcmd in MUTATING_GIT_SUBCMDS {
            let cmd = format!("git {} something", subcmd);
            let result = find_bare_mutating_git(&cmd);
            assert_eq!(
                result.as_deref(),
                Some(*subcmd),
                "should detect bare 'git {}'",
                subcmd
            );
        }
    }
}
