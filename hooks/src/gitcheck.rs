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

// Bash write-path extraction uses a tokenizer instead of regex so quoting,
// fd-redirect digits, and operators without surrounding whitespace are all
// handled correctly. See `tokenize_bash` and `check_bash_write_paths`.

/// Run all 8 git safety checks against a Bash command.
///
/// Denial messages use the WHAT/FIX/REF remediation format so the agent
/// can self-repair without human intervention.
pub fn check_git_safety(cmd: &str) -> GitResult {
    // FR-GS-1: Force push without --force-with-lease
    if RE_GIT_PUSH.is_match(cmd)
        && RE_FORCE_FLAG.is_match(cmd)
        && !RE_FORCE_WITH_LEASE.is_match(cmd)
    {
        return GitResult::Block(
            "WHAT: Force push without --force-with-lease. \
             FIX: Use `git push --force-with-lease origin <branch>` instead. \
             REF: CLAUDE.md#supply-chain-policy"
                .into(),
        );
    }

    // FR-GS-2: Push to main/master
    if RE_PUSH_TO_MAIN.is_match(cmd) {
        return GitResult::Block(
            "WHAT: Direct push to main/master. \
             FIX: Create a feature branch and open a PR instead. \
             REF: CLAUDE.md#commit-convention"
                .into(),
        );
    }

    // FR-GS-3: Refspec push to main/master
    if RE_REFSPEC_MAIN.is_match(cmd) {
        return GitResult::Block(
            "WHAT: Push to main/master via refspec. \
             FIX: Create a feature branch and open a PR instead. \
             REF: CLAUDE.md#commit-convention"
                .into(),
        );
    }

    // FR-GS-4: Delete main/master
    if RE_DELETE_MAIN.is_match(cmd) {
        return GitResult::Block(
            "WHAT: Deleting main/master branch is not allowed. \
             FIX: Do not delete protected branches. \
             REF: CLAUDE.md#supply-chain-policy"
                .into(),
        );
    }
    if RE_DELETE_REFSPEC.is_match(cmd) {
        return GitResult::Block(
            "WHAT: Deleting main/master branch via empty refspec is not allowed. \
             FIX: Do not delete protected branches. \
             REF: CLAUDE.md#supply-chain-policy"
                .into(),
        );
    }

    // FR-GS-5: --no-verify
    if RE_NO_VERIFY.is_match(cmd) {
        return GitResult::Block(
            "WHAT: git push --no-verify bypasses pre-push hooks. \
             FIX: Fix the hook failures instead of skipping them. \
             REF: CLAUDE.md#lint-suppression-policy"
                .into(),
        );
    }

    // FR-GS-6: --follow-tags
    if RE_FOLLOW_TAGS.is_match(cmd) {
        return GitResult::Block(
            "WHAT: git push --follow-tags pushes ALL matching local tags. \
             FIX: Push tags explicitly: `git push origin <tag>`. \
             REF: CLAUDE.md#releases"
                .into(),
        );
    }

    // FR-GS-7: Delete semver tags (local and remote)
    if RE_DELETE_SEMVER_TAG.is_match(cmd) {
        return GitResult::Block(
            "WHAT: Deleting semantic version tags is not allowed. \
             FIX: Release a new patch version instead. \
             REF: CLAUDE.md#releases"
                .into(),
        );
    }
    if RE_DELETE_REMOTE_TAG.is_match(cmd) {
        return GitResult::Block(
            "WHAT: Deleting remote semantic version tags is not allowed. \
             FIX: Release a new patch version instead. \
             REF: CLAUDE.md#releases"
                .into(),
        );
    }

    // FR-GS-8: Hard reset to origin/main|master
    if RE_HARD_RESET.is_match(cmd) {
        return GitResult::Block(
            "WHAT: git reset --hard origin/main|master discards all local work. \
             FIX: Use `git stash` or `git reset --soft` instead. \
             REF: CLAUDE.md#key-design-decisions"
                .into(),
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
                        "WHAT: Git operation targets main checkout ({repo}), not the session worktree. \
                         FIX: Use `git -C {ws_str}/{repo}/.worktrees/{short_id}/` instead. \
                         REF: docs/architecture.md#key-invariants"
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
                        "WHAT: Git operation targets main checkout ({repo}), not the session worktree. \
                         FIX: Use `git -C {ws_str}/{repo}/.worktrees/{short_id}/` instead. \
                         REF: docs/architecture.md#key-invariants"
                    ));
                }
            }
        }
    }

    // Block bare git checkout/switch with no path context
    if !RE_GIT_C.is_match(cmd) && !RE_CD_PATH.is_match(cmd) && RE_GIT_CHECKOUT_SWITCH.is_match(cmd)
    {
        return Some(format!(
            "WHAT: Bare git checkout/switch — worktrees are active. \
             FIX: Use `git -C <repo>/.worktrees/{short_id}/ checkout ...` instead. \
             REF: docs/architecture.md#key-invariants"
        ));
    }

    None
}

/// Token yielded by `tokenize_bash`.
#[derive(Debug, PartialEq)]
enum BashToken {
    /// A word — the concatenation of unquoted and quoted parts between
    /// shell-metacharacter boundaries.
    Word(String),
    /// A write redirect: `>`, `>>`, `1>`, `2>`, `1>>`, `2>>`, `&>`, `&>>`.
    Redirect,
    /// A command separator: `|`, `||`, `&`, `&&`, `;`.
    Separator,
}

/// Minimal Bash tokenizer used by `check_bash_write_paths`.
///
/// Handles single/double quotes, backslash escapes, and the write-redirect
/// and command-separator operators we care about. Input redirect (`<`) is
/// intentionally ignored — we only sandbox writes.
fn tokenize_bash(cmd: &str) -> Vec<BashToken> {
    let mut tokens = Vec::new();
    let mut cur = String::new();
    let mut cur_has_quoted = false;
    let mut chars = cmd.chars().peekable();

    fn flush(cur: &mut String, has_q: &mut bool, tokens: &mut Vec<BashToken>) {
        if !cur.is_empty() || *has_q {
            tokens.push(BashToken::Word(std::mem::take(cur)));
            *has_q = false;
        }
    }

    while let Some(c) = chars.next() {
        match c {
            ' ' | '\t' | '\n' => flush(&mut cur, &mut cur_has_quoted, &mut tokens),
            '\'' => {
                cur_has_quoted = true;
                for nc in chars.by_ref() {
                    if nc == '\'' {
                        break;
                    }
                    cur.push(nc);
                }
            }
            '"' => {
                cur_has_quoted = true;
                while let Some(nc) = chars.next() {
                    if nc == '"' {
                        break;
                    }
                    if nc == '\\' {
                        if let Some(&esc) = chars.peek() {
                            cur.push(esc);
                            chars.next();
                        }
                    } else {
                        cur.push(nc);
                    }
                }
            }
            '\\' => {
                if let Some(&nc) = chars.peek() {
                    cur.push(nc);
                    chars.next();
                }
            }
            '>' => {
                // A leading `1`/`2` on the current word is an fd specifier for
                // this redirect, not part of a preceding word.
                if cur.as_str() == "1" || cur.as_str() == "2" {
                    cur.clear();
                }
                flush(&mut cur, &mut cur_has_quoted, &mut tokens);
                if chars.peek() == Some(&'>') {
                    chars.next();
                }
                tokens.push(BashToken::Redirect);
            }
            '|' => {
                flush(&mut cur, &mut cur_has_quoted, &mut tokens);
                if chars.peek() == Some(&'|') {
                    chars.next();
                }
                tokens.push(BashToken::Separator);
            }
            '&' => {
                flush(&mut cur, &mut cur_has_quoted, &mut tokens);
                if chars.peek() == Some(&'>') {
                    // `&>` and `&>>` combine stdout+stderr redirect
                    chars.next();
                    if chars.peek() == Some(&'>') {
                        chars.next();
                    }
                    tokens.push(BashToken::Redirect);
                } else {
                    if chars.peek() == Some(&'&') {
                        chars.next();
                    }
                    tokens.push(BashToken::Separator);
                }
            }
            ';' => {
                flush(&mut cur, &mut cur_has_quoted, &mut tokens);
                tokens.push(BashToken::Separator);
            }
            _ => cur.push(c),
        }
    }
    flush(&mut cur, &mut cur_has_quoted, &mut tokens);
    tokens
}

/// Extract write-target paths from a Bash command.
///
/// Tokenizes the command (honoring shell quoting) and walks the token stream
/// to find absolute paths that would actually be written. This avoids the
/// regex-on-raw-string pitfall where `>` inside a quoted argument — e.g.
/// `--description "foo/<name>/modules/"` — was mistaken for a redirect.
pub fn check_bash_write_paths(cmd: &str) -> Vec<String> {
    let mut paths = Vec::new();
    let tokens = tokenize_bash(cmd);

    let mut i = 0;
    while i < tokens.len() {
        match &tokens[i] {
            BashToken::Redirect => {
                if let Some(BashToken::Word(target)) = tokens.get(i + 1) {
                    if target.starts_with('/') {
                        paths.push(target.clone());
                    }
                }
                i += 2;
                continue;
            }
            BashToken::Word(w) if w == "tee" => {
                let mut j = i + 1;
                // Skip tee flags like -a, -i, --append
                while let Some(BashToken::Word(fw)) = tokens.get(j) {
                    if fw.starts_with('-') {
                        j += 1;
                    } else {
                        break;
                    }
                }
                if let Some(BashToken::Word(target)) = tokens.get(j) {
                    if target.starts_with('/') {
                        paths.push(target.clone());
                    }
                }
                i = j + 1;
                continue;
            }
            BashToken::Word(w) if w == "git" => {
                // Scan forward to the next separator for a -C argument.
                let mut j = i + 1;
                while let Some(tok) = tokens.get(j) {
                    match tok {
                        BashToken::Separator => break,
                        BashToken::Word(fw) if fw == "-C" => {
                            if let Some(BashToken::Word(target)) = tokens.get(j + 1) {
                                if target.starts_with('/') {
                                    paths.push(format!("gitc:{}", target));
                                }
                            }
                            break;
                        }
                        _ => j += 1,
                    }
                }
            }
            _ => {}
        }
        i += 1;
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
    fn test_bash_write_paths_ignore_quoted_content() {
        // Angle-bracket placeholders and other path-like tokens inside
        // quoted arguments are literal text, not shell syntax. They must
        // not be mistaken for redirects or tee targets.
        let cases = [
            r#"bd create --description "services/<name>/modules/foo""#,
            r#"bd create --description 'services/<name>/modules/foo'"#,
            r#"echo "redirect to >/etc/passwd in docs""#,
            r#"gh issue create --body "see <path>/usr/local/bin""#,
            r#"gh pr comment --body 'pipe to tee /etc/shadow here'"#,
        ];
        for cmd in &cases {
            let paths = check_bash_write_paths(cmd);
            let non_gitc: Vec<_> = paths.iter().filter(|p| !p.starts_with("gitc:")).collect();
            assert!(
                non_gitc.is_empty(),
                "expected no write paths for {:?}, got {:?}",
                cmd,
                non_gitc
            );
        }
    }

    #[test]
    fn test_bash_write_paths_quoted_redirect_target() {
        // A legitimately quoted redirect target must still be caught —
        // this is the case the prior regex-based scanner missed entirely.
        let cases = [
            (r#"echo hi > "/tmp/output.log""#, "/tmp/output.log"),
            (r#"echo hi > '/tmp/output.log'"#, "/tmp/output.log"),
            (r#"echo hi >"/etc/passwd""#, "/etc/passwd"),
            (r#"cat f | tee "/tmp/teed.log""#, "/tmp/teed.log"),
        ];
        for (cmd, expected) in &cases {
            let paths = check_bash_write_paths(cmd);
            assert!(
                paths.iter().any(|p| p == expected),
                "expected {:?} in paths for {:?}, got {:?}",
                expected,
                cmd,
                paths
            );
        }
    }

    #[test]
    fn test_bash_write_paths_fd_redirects_not_captured_as_paths() {
        // `2>&1` is an fd-to-fd redirect; `&1` is not a path. Similarly
        // `1> /tmp/foo` targets fd 1 (stdout) to /tmp/foo — we want the path.
        let paths = check_bash_write_paths("cmd 1> /tmp/out 2>&1");
        assert!(
            paths.iter().any(|p| p == "/tmp/out"),
            "should capture fd-1 redirect target, got {:?}",
            paths
        );
        assert!(
            !paths.iter().any(|p| p.contains("&1")),
            "should not capture &1 as a path, got {:?}",
            paths
        );
    }

    #[test]
    fn test_bash_write_paths_no_whitespace_redirect() {
        // `cmd>/tmp/foo` (no whitespace) is a valid redirect and must be caught.
        let paths = check_bash_write_paths("echo hi>/tmp/nospace.log");
        assert!(
            paths.iter().any(|p| p == "/tmp/nospace.log"),
            "should capture no-whitespace redirect, got {:?}",
            paths
        );
    }

    #[test]
    fn test_bash_write_paths_combined_stdout_stderr_redirect() {
        // `&>` and `&>>` redirect both stdout and stderr.
        let paths = check_bash_write_paths("cmd &> /tmp/all.log");
        assert!(
            paths.iter().any(|p| p == "/tmp/all.log"),
            "should capture &> target, got {:?}",
            paths
        );
    }

    #[test]
    fn test_bash_write_paths_git_c_across_separator() {
        // `-C` after a `;` belongs to a different command — the first `git`
        // should not consume it.
        let paths = check_bash_write_paths("git status; foo -C /tmp/notgit");
        assert!(
            !paths.iter().any(|p| p.starts_with("gitc:")),
            "git -C scan must stop at command separator, got {:?}",
            paths
        );
    }

    #[test]
    fn test_bash_write_paths_escaped_quote_inside_double_quoted_arg() {
        // Bash: `"foo \" > /tmp/evil"` is a single quoted argument whose
        // content contains `"`, ` > /tmp/evil`. The `>` is *inside* the
        // quoted argument and must not be treated as a redirect. A naive
        // quote-stripper that closed the quote at `\"` would leak
        // `/tmp/evil` back out and produce a false positive (or, worse,
        // miss a real write if paired with the wrong heuristic).
        let paths = check_bash_write_paths(r#"echo "foo \" > /tmp/evil""#);
        let non_gitc: Vec<_> = paths.iter().filter(|p| !p.starts_with("gitc:")).collect();
        assert!(
            non_gitc.is_empty(),
            "escaped quote must not break out of quoted arg, got {:?}",
            non_gitc
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
}
