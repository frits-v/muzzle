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

// In-place file modification commands (bypass vectors)
static RE_SED_INPLACE: LazyLock<Regex> = LazyLock::new(|| {
    // sed -i '' 'pattern' file  OR  sed -i 'pattern' file  OR  sed -i.bak 'pattern' file
    Regex::new(r"\bsed\s+-i\b").unwrap()
});
static RE_PERL_INPLACE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\bperl\s+-[^\s]*i").unwrap());
static RE_RUBY_INPLACE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\bruby\s+-[^\s]*i").unwrap());

// File copy/move commands
static RE_CP: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\bcp\b").unwrap());
static RE_MV: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\bmv\b").unwrap());
// Match standalone `install` utility only, not package managers (npm install, pip install, etc.).
// Require `install` at the start of a command segment (after |, &&, ;, or line start).
static RE_INSTALL: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?:^|\|{1,2}|&&|;\s*)\s*install\b").unwrap());
static RE_RSYNC: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\brsync\b").unwrap());
static RE_DD_OF: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\bdd\b[^;|&]*\bof=([^\s;|&]+)").unwrap());
static RE_PATCH: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\bpatch\b").unwrap());

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
            "BLOCKED: Deleting semantic version tags is not allowed. \
             To sync local tags with remote: git fetch --prune origin \"+refs/tags/*:refs/tags/*\". \
             To fix a released version: release a new patch instead."
                .into(),
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
///
/// Returns paths with optional prefixes:
/// - No prefix: absolute write target from redirect/tee
/// - `gitc:` prefix: git -C working directory (not a direct write target)
/// - `rel:` prefix: relative path from a file-mutating command (sed -i, cp, mv, etc.)
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

    // In-place edit commands: extract the last non-option argument as the target file.
    // These are the most common bypass vectors for Edit hook denials.
    if RE_SED_INPLACE.is_match(cmd) {
        if let Some(target) = extract_last_file_arg(cmd, "sed") {
            push_write_path(&mut paths, &target);
        }
    }
    if RE_PERL_INPLACE.is_match(cmd) {
        if let Some(target) = extract_last_file_arg(cmd, "perl") {
            push_write_path(&mut paths, &target);
        }
    }
    if RE_RUBY_INPLACE.is_match(cmd) {
        if let Some(target) = extract_last_file_arg(cmd, "ruby") {
            push_write_path(&mut paths, &target);
        }
    }

    // cp/mv/install/rsync: destination is the last argument
    if RE_CP.is_match(cmd) {
        if let Some(dest) = extract_copy_dest(cmd, "cp") {
            push_write_path(&mut paths, &dest);
        }
    }
    if RE_MV.is_match(cmd) {
        if let Some(dest) = extract_copy_dest(cmd, "mv") {
            push_write_path(&mut paths, &dest);
        }
    }
    if RE_INSTALL.is_match(cmd) {
        if let Some(dest) = extract_copy_dest(cmd, "install") {
            push_write_path(&mut paths, &dest);
        }
    }
    if RE_RSYNC.is_match(cmd) {
        if let Some(dest) = extract_copy_dest(cmd, "rsync") {
            push_write_path(&mut paths, &dest);
        }
    }

    // dd of=<path>
    for caps in RE_DD_OF.captures_iter(cmd) {
        if let Some(m) = caps.get(1) {
            let p = m.as_str().trim();
            push_write_path(&mut paths, p);
        }
    }

    // patch: target file is usually the last argument or via -o
    if RE_PATCH.is_match(cmd) {
        if let Some(target) = extract_last_file_arg(cmd, "patch") {
            push_write_path(&mut paths, &target);
        }
    }

    paths
}

/// Push a write path, using `rel:` prefix for relative paths.
fn push_write_path(paths: &mut Vec<String>, path: &str) {
    if path.starts_with('/') {
        paths.push(path.to_string());
    } else if !path.is_empty() && !path.starts_with('-') {
        paths.push(format!("rel:{}", path));
    }
}

/// Extract the last non-option, non-pattern argument from a command.
/// Used for sed -i, perl -i, ruby -i, patch — the file target is typically last.
fn extract_last_file_arg(cmd: &str, tool: &str) -> Option<String> {
    // Find the tool invocation and get everything after it
    let tool_pattern = format!(r"\b{}\b", regex::escape(tool));
    let re = Regex::new(&tool_pattern).ok()?;
    let m = re.find(cmd)?;
    let after_tool = &cmd[m.end()..];

    // Split on pipe/semicolon/&&/< to isolate this command
    // The < split prevents input redirects (< file.patch) from being parsed as arguments
    let segment = after_tool
        .split(['|', ';', '<'])
        .next()
        .unwrap_or(after_tool);
    let segment = segment.split("&&").next().unwrap_or(segment);

    // Get the last whitespace-delimited token that looks like a file path
    // (not an option flag, not a quoted pattern)
    let tokens: Vec<&str> = segment.split_whitespace().collect();
    for &tok in tokens.iter().rev() {
        // Check quote-bounded patterns on the original token BEFORE trimming
        if tok.starts_with("'/") || tok.starts_with("\"/") {
            continue;
        }
        if tok.contains("/d'") || tok.contains("/d\"") {
            continue;
        }
        let cleaned = tok.trim_matches(|c| c == '"' || c == '\'');
        // Skip option flags
        if cleaned.starts_with('-') {
            continue;
        }
        if cleaned.is_empty() {
            continue;
        }
        // Skip sed address/substitution patterns like /pattern/d or s/foo/bar/
        if cleaned.starts_with('/') && cleaned.ends_with('/') {
            continue;
        }
        if cleaned.starts_with("s/") {
            continue;
        }
        // This looks like a file path
        return Some(cleaned.to_string());
    }
    None
}

/// Extract the destination path from cp/mv/install/rsync commands.
/// The destination is the last non-option argument.
fn extract_copy_dest(cmd: &str, tool: &str) -> Option<String> {
    let tool_pattern = format!(r"\b{}\b", regex::escape(tool));
    let re = Regex::new(&tool_pattern).ok()?;
    let m = re.find(cmd)?;
    let after_tool = &cmd[m.end()..];

    // Split on pipe/semicolon/&&
    let segment = after_tool.split(['|', ';']).next().unwrap_or(after_tool);
    let segment = segment.split("&&").next().unwrap_or(segment);

    let tokens: Vec<&str> = segment.split_whitespace().collect();

    // Collect non-option arguments, tracking explicit -t destination
    let mut args: Vec<&str> = Vec::new();
    let mut explicit_dest: Option<&str> = None;
    let mut capture_dest = false;
    for &tok in &tokens {
        if capture_dest {
            capture_dest = false;
            let cleaned = tok.trim_matches(|c| c == '"' || c == '\'');
            explicit_dest = Some(cleaned);
            continue;
        }
        // Flags that take a value: -t (target dir), --target-directory
        // The -t value IS the write destination
        if tok == "-t" || tok == "--target-directory" {
            capture_dest = true;
            continue;
        }
        if tok.starts_with('-') {
            continue;
        }
        let cleaned = tok.trim_matches(|c| c == '"' || c == '\'');
        if !cleaned.is_empty() {
            args.push(cleaned);
        }
    }

    // If -t was used, that's the explicit destination
    if let Some(dest) = explicit_dest {
        return Some(dest.to_string());
    }

    // Otherwise, destination is the last argument (need at least 2: source + dest)
    if args.len() >= 2 {
        return Some(args.last().unwrap().to_string());
    }
    None
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

    #[test]
    fn test_delete_local_semver_tag_suggests_fetch_prune() {
        let r = check_git_safety("git tag -d v1.0.0");
        if let GitResult::Block(msg) = r {
            assert!(
                msg.contains("git fetch --prune origin"),
                "local tag delete should suggest fetch --prune, got: {msg}"
            );
        } else {
            panic!("expected Block");
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

    // ---- Bypass vector detection tests ----

    #[test]
    fn test_sed_inplace_absolute_path() {
        let paths = check_bash_write_paths("sed -i '' 's/foo/bar/' /usr/src/file.rs");
        assert!(
            paths.iter().any(|p| p == "/usr/src/file.rs"),
            "sed -i with absolute path should be detected: {:?}",
            paths
        );
    }

    #[test]
    fn test_sed_inplace_relative_path() {
        let paths = check_bash_write_paths("sed -i '' '/pattern/d' hooks/src/gitcheck.rs");
        assert!(
            paths.iter().any(|p| p == "rel:hooks/src/gitcheck.rs"),
            "sed -i with relative path should return rel: prefix: {:?}",
            paths
        );
    }

    #[test]
    fn test_sed_inplace_macos_variant() {
        let paths = check_bash_write_paths("sed -i '' 's/old/new/g' src/main.rs");
        assert!(
            paths.iter().any(|p| p == "rel:src/main.rs"),
            "sed -i '' (macOS) should detect target: {:?}",
            paths
        );
    }

    #[test]
    fn test_perl_inplace() {
        let paths = check_bash_write_paths("perl -i -pe 's/foo/bar/' src/lib.rs");
        assert!(
            paths.iter().any(|p| p == "rel:src/lib.rs"),
            "perl -i should detect target: {:?}",
            paths
        );
    }

    #[test]
    fn test_ruby_inplace() {
        let paths = check_bash_write_paths("ruby -i -pe 'gsub(/foo/,\"bar\")' config.yml");
        assert!(
            paths.iter().any(|p| p == "rel:config.yml"),
            "ruby -i should detect target: {:?}",
            paths
        );
    }

    #[test]
    fn test_cp_absolute_paths() {
        let paths = check_bash_write_paths("cp /tmp/fixed.rs /home/user/src/file.rs");
        assert!(
            paths.iter().any(|p| p == "/home/user/src/file.rs"),
            "cp with absolute dest should be detected: {:?}",
            paths
        );
    }

    #[test]
    fn test_cp_relative_dest() {
        let paths = check_bash_write_paths("cp /tmp/fixed.rs hooks/src/gitcheck.rs");
        assert!(
            paths.iter().any(|p| p == "rel:hooks/src/gitcheck.rs"),
            "cp with relative dest should return rel: prefix: {:?}",
            paths
        );
    }

    #[test]
    fn test_cp_with_flags() {
        let paths = check_bash_write_paths("cp -f /tmp/fixed.rs hooks/src/gitcheck.rs");
        assert!(
            paths.iter().any(|p| p == "rel:hooks/src/gitcheck.rs"),
            "cp -f should still detect dest: {:?}",
            paths
        );
    }

    #[test]
    fn test_mv_relative_dest() {
        let paths = check_bash_write_paths("mv /tmp/backup.rs src/lib.rs");
        assert!(
            paths.iter().any(|p| p == "rel:src/lib.rs"),
            "mv with relative dest should return rel: prefix: {:?}",
            paths
        );
    }

    #[test]
    fn test_install_dest() {
        let paths = check_bash_write_paths("install -m 755 /tmp/binary /usr/local/bin/tool");
        assert!(
            paths.iter().any(|p| p == "/usr/local/bin/tool"),
            "install should detect absolute dest: {:?}",
            paths
        );
    }

    #[test]
    fn test_rsync_dest() {
        let paths = check_bash_write_paths("rsync -av /tmp/src/ /home/user/dest/");
        assert!(
            paths.iter().any(|p| p == "/home/user/dest/"),
            "rsync should detect absolute dest: {:?}",
            paths
        );
    }

    #[test]
    fn test_dd_of_path() {
        let paths = check_bash_write_paths("dd if=/dev/zero of=/tmp/output.img bs=1M count=10");
        assert!(
            paths.iter().any(|p| p == "/tmp/output.img"),
            "dd of= should detect target: {:?}",
            paths
        );
    }

    #[test]
    fn test_dd_of_relative() {
        let paths = check_bash_write_paths("dd if=/dev/zero of=output.bin bs=1M count=1");
        assert!(
            paths.iter().any(|p| p == "rel:output.bin"),
            "dd of= with relative path should return rel: prefix: {:?}",
            paths
        );
    }

    #[test]
    fn test_patch_target() {
        let paths = check_bash_write_paths("patch -p1 src/main.rs < fix.patch");
        assert!(
            paths.iter().any(|p| p == "rel:src/main.rs"),
            "patch should detect target file: {:?}",
            paths
        );
    }

    #[test]
    fn test_bypass_chain_sed_then_cp() {
        // The exact bypass from the incident: sed to temp, then cp back
        let paths = check_bash_write_paths(
            "sed '/pattern/d' hooks/src/gitcheck.rs > /tmp/fixed.rs && cp /tmp/fixed.rs hooks/src/gitcheck.rs",
        );
        assert!(
            paths.iter().any(|p| p == "/tmp/fixed.rs"),
            "redirect to /tmp should be detected: {:?}",
            paths
        );
        assert!(
            paths.iter().any(|p| p == "rel:hooks/src/gitcheck.rs"),
            "cp to relative dest should be detected: {:?}",
            paths
        );
    }

    #[test]
    fn test_safe_commands_no_false_positives() {
        // These should NOT produce write paths
        let safe_cmds = [
            "cat src/main.rs",
            "grep -r 'pattern' src/",
            "ls -la",
            "cargo build",
            "cargo test",
            "echo hello",
            "sed 's/foo/bar/' src/main.rs", // sed without -i is read-only (to stdout)
        ];
        for cmd in &safe_cmds {
            let paths = check_bash_write_paths(cmd);
            let non_gitc: Vec<_> = paths.iter().filter(|p| !p.starts_with("gitc:")).collect();
            assert!(
                non_gitc.is_empty(),
                "safe command {:?} should produce no write paths, got {:?}",
                cmd,
                non_gitc
            );
        }
    }

    #[test]
    fn test_cp_single_arg_no_false_positive() {
        // cp with only one non-option arg shouldn't produce a path (incomplete command)
        let paths = check_bash_write_paths("cp --help");
        let cp_paths: Vec<_> = paths.iter().filter(|p| !p.starts_with("gitc:")).collect();
        assert!(
            cp_paths.is_empty(),
            "cp --help should not produce write paths: {:?}",
            cp_paths
        );
    }

    #[test]
    fn test_cp_dash_t_captures_destination() {
        // cp -t <dest> <src> — the -t value IS the write destination
        let paths = check_bash_write_paths("cp -t /path/to/checkout/file.rs /tmp/replacement.rs");
        assert!(
            paths.iter().any(|p| p == "/path/to/checkout/file.rs"),
            "cp -t should capture destination: {:?}",
            paths
        );
    }

    #[test]
    fn test_cp_dash_t_relative() {
        let paths = check_bash_write_paths("cp -t src/lib.rs /tmp/fixed.rs");
        assert!(
            paths.iter().any(|p| p == "rel:src/lib.rs"),
            "cp -t with relative dest should return rel: prefix: {:?}",
            paths
        );
    }

    #[test]
    fn test_install_no_false_positive_package_managers() {
        // Package manager install commands should NOT produce write paths
        let safe_cmds = [
            "npm install express",
            "pip install requests",
            "apt-get install -y nginx libssl-dev",
            "cargo install ripgrep",
            "brew install jq",
        ];
        for cmd in &safe_cmds {
            let paths = check_bash_write_paths(cmd);
            let non_gitc: Vec<_> = paths.iter().filter(|p| !p.starts_with("gitc:")).collect();
            assert!(
                non_gitc.is_empty(),
                "package manager {:?} should not produce write paths, got {:?}",
                cmd,
                non_gitc
            );
        }
    }

    #[test]
    fn test_install_standalone_utility() {
        // Standalone install utility should be detected
        let paths = check_bash_write_paths("install -m 755 /tmp/bin /usr/local/bin/tool");
        assert!(
            paths.iter().any(|p| p == "/usr/local/bin/tool"),
            "standalone install should detect dest: {:?}",
            paths
        );
    }
}
