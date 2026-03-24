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
    LazyLock::new(|| Regex::new(r"[12]?>>?\s*([^\s;|&)]+)").unwrap());
static RE_TEE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\btee\s+(?:-a\s+)?([^\s;|&]+)").unwrap());
static RE_GIT_C_PATH: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"\bgit\s+-C\s+("[^"]+"|'[^']+'|(\S+))"#).unwrap());

// In-place file modification commands (bypass vectors)
static RE_SED_INPLACE: LazyLock<Regex> = LazyLock::new(|| {
    // Match sed in-place edits: -i (possibly combined like -Ei, -ni), --in-place, --in-place=SUFFIX.
    // Both alternatives are anchored under \bsed\b to avoid matching other tools.
    // Uses [a-zA-Z]* on both sides of `i` so combined flags like -Ei, -ni, -in, -iE all match.
    // sed has no -I flag conflict unlike perl/ruby.
    Regex::new(r"\bsed\b(?:[^|;&\n]*\s-[a-zA-Z]*i[a-zA-Z.]*(?:\b|\.)|[^|;&\n]*\s--in-place\b)")
        .unwrap()
});
// Use [a-z0-9]* to match only lowercase flags, excluding -I (include path).
// Match -i in the first flag group OR as a separate flag later (e.g. `perl -w -i`).
static RE_PERL_INPLACE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\bperl\b[^|;&\n]*\s-[a-z0-9]*i[a-z0-9.]*(?:\b|\.)").unwrap());
static RE_RUBY_INPLACE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\bruby\b[^|;&\n]*\s-[a-z0-9]*i[a-z0-9.]*(?:\b|\.)").unwrap());

// File copy/move commands — anchored to command-start position to avoid matching
// inside compound commands like `git mv` or `git cp`.
// Allow optional sudo/env prefix (matching RE_INSTALL).
static RE_CP: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?:^|\|{1,2}|&&|;\s*)\s*(?:sudo\s+|env\s+)?cp\b").unwrap());
static RE_MV: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?:^|\|{1,2}|&&|;\s*)\s*(?:sudo\s+|env\s+)?mv\b").unwrap());
// Match standalone `install` utility only, not package managers (npm install, pip install, etc.).
// Require `install` at the start of a command segment (after |, &&, ;, or line start).
// Also match `sudo install` and `env install` for elevated-privilege invocations.
static RE_INSTALL: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?:^|\|{1,2}|&&|;\s*)\s*(?:sudo\s+|env\s+)?install\b").unwrap());
static RE_RSYNC: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?:^|\|{1,2}|&&|;\s*)\s*(?:sudo\s+|env\s+)?rsync\b").unwrap());
static RE_DD_OF: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\bdd\b[^;|&]*\bof=([^\s;|&]+)").unwrap());
// Anchor to command-start position to avoid matching inside git format-patch / --patch
static RE_PATCH: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?:^|\|{1,2}|&&|;\s*)\s*patch\b").unwrap());

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

    // Redirect targets (absolute and relative)
    for caps in RE_REDIRECT.captures_iter(cmd) {
        if let Some(m) = caps.get(1) {
            let p = m.as_str().trim();
            push_write_path(&mut paths, p);
        }
    }

    // Tee targets (absolute and relative)
    for caps in RE_TEE.captures_iter(cmd) {
        if let Some(m) = caps.get(1) {
            let p = m.as_str().trim();
            push_write_path(&mut paths, p);
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

    // In-place edit commands: extract all file arguments as write targets.
    // These are the most common bypass vectors for Edit hook denials.
    // Tools like sed -i, perl -i accept multiple files — all must be checked.
    // Use find_iter to catch all occurrences in multi-stage commands
    // (e.g. `sed -i ... && sed -i ...`).
    for m in RE_SED_INPLACE.find_iter(cmd) {
        for target in extract_file_args(cmd, m.start(), "sed") {
            push_write_path(&mut paths, &target);
        }
    }
    for m in RE_PERL_INPLACE.find_iter(cmd) {
        for target in extract_file_args(cmd, m.start(), "perl") {
            push_write_path(&mut paths, &target);
        }
    }
    for m in RE_RUBY_INPLACE.find_iter(cmd) {
        for target in extract_file_args(cmd, m.start(), "ruby") {
            push_write_path(&mut paths, &target);
        }
    }

    // cp/mv/install/rsync: destination is the last argument.
    // Use find_iter to catch all occurrences in multi-stage commands.
    for m in RE_CP.find_iter(cmd) {
        if let Some(dest) = extract_copy_dest(cmd, m.end()) {
            push_write_path(&mut paths, &dest);
        }
    }
    for m in RE_MV.find_iter(cmd) {
        if let Some(dest) = extract_copy_dest(cmd, m.end()) {
            push_write_path(&mut paths, &dest);
        }
    }
    for m in RE_INSTALL.find_iter(cmd) {
        if let Some(dest) = extract_copy_dest(cmd, m.end()) {
            push_write_path(&mut paths, &dest);
        }
    }
    for m in RE_RSYNC.find_iter(cmd) {
        if let Some(dest) = extract_copy_dest(cmd, m.end()) {
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
    for m in RE_PATCH.find_iter(cmd) {
        for target in extract_file_args(cmd, m.start(), "patch") {
            push_write_path(&mut paths, &target);
        }
    }

    paths
}

/// Push a write path, using `rel:` prefix for relative paths.
fn push_write_path(paths: &mut Vec<String>, path: &str) {
    // Skip remote destinations — these are network targets, not local writes.
    // SCP-style: user@host:/path
    if path.contains('@') && path.contains(':') {
        return;
    }
    // rsync daemon URLs: rsync://host/module or host::module (no slashes before ::)
    if path.starts_with("rsync://") {
        return;
    }
    if let Some(pos) = path.find("::") {
        // Only skip if :: appears before any / (rsync daemon syntax: host::module)
        if !path[..pos].contains('/') {
            return;
        }
    }
    if path.starts_with('/') {
        paths.push(path.to_string());
    } else if !path.is_empty() && !path.starts_with('-') {
        paths.push(format!("rel:{}", path));
    }
}

/// Extract all non-option, non-pattern arguments from a command.
/// Used for sed -i, perl -i, ruby -i, patch — these tools accept multiple file
/// targets, so we must check all of them, not just the last one.
///
/// `match_start` is the start offset of the RE_* regex match in `cmd`.
/// `tool` is the tool name to find within the matched region. This ensures we
/// parse from the correct invocation, not a false match in a filename or string.
fn extract_file_args(cmd: &str, match_start: usize, tool: &str) -> Vec<String> {
    let mut results = Vec::new();
    let region = &cmd[match_start..];
    // Find the tool name within the matched region
    let tool_pattern = format!(r"\b{}\b", regex::escape(tool));
    let re = match Regex::new(&tool_pattern) {
        Ok(r) => r,
        Err(_) => return results,
    };
    let m = match re.find(region) {
        Some(m) => m,
        None => return results,
    };
    let after_tool = &region[m.end()..];

    // Split on pipe/semicolon/&&/</> to isolate this command.
    // The > split prevents stdout redirects from being parsed as file arguments,
    // closing a bypass vector where the redirect target masked the real write target.
    let segment = after_tool
        .split(['|', ';', '<', '>'])
        .next()
        .unwrap_or(after_tool);
    let segment = segment.split("&&").next().unwrap_or(segment);

    // Collect all whitespace-delimited tokens that look like file paths
    // (not option flags, not quoted patterns, not flag-value arguments)
    let tokens: Vec<&str> = segment.split_whitespace().collect();
    let mut skip_next = false;
    for &tok in tokens.iter() {
        // Skip the value argument of flags that take a parameter (-e, -f for sed/perl)
        if skip_next {
            skip_next = false;
            continue;
        }
        let cleaned_flag = tok.trim_matches(|c| c == '"' || c == '\'');
        if cleaned_flag == "-e" || cleaned_flag == "-f" {
            skip_next = true;
            continue;
        }
        // Skip quoted sed/perl address expressions like '/pattern/d' or
        // '/pattern/' but NOT quoted absolute paths like '/home/user/file.rs'.
        // Sed address expressions end with a sed command char before the quote.
        if tok.contains("/d'") || tok.contains("/d\"") {
            continue;
        }
        if (tok.starts_with("'/") && tok.ends_with("/'"))
            || (tok.starts_with("\"/") && tok.ends_with("/\""))
        {
            continue;
        }
        let cleaned = tok.trim_matches(|c| c == '"' || c == '\'');
        // Skip option flags
        if cleaned.starts_with('-') {
            continue;
        }
        if cleaned.is_empty() || cleaned.bytes().all(|b| b.is_ascii_digit()) {
            continue;
        }
        // Skip sed/perl expressions: s/foo/bar/, /pattern/d, y/abc/def/, etc.
        // Require at least 3 slashes to distinguish from valid paths in
        // single-char directories like `b/src/lib.rs` (2 slashes).
        if cleaned.starts_with('/') && cleaned.ends_with('/') {
            continue;
        }
        let slash_count = cleaned.bytes().filter(|&b| b == b'/').count();
        if cleaned.len() >= 2
            && cleaned.as_bytes()[0].is_ascii_alphabetic()
            && cleaned.as_bytes()[1] == b'/'
            && slash_count >= 3
        {
            continue;
        }
        // This looks like a file path
        results.push(cleaned.to_string());
    }
    results
}

/// Extract the destination path from cp/mv/install/rsync commands.
/// The destination is the last non-option argument.
///
/// `tool_match_end` is the end offset of the RE_* regex match in `cmd`,
/// ensuring we parse from the correct invocation rather than re-searching.
fn extract_copy_dest(cmd: &str, tool_match_end: usize) -> Option<String> {
    let after_tool = &cmd[tool_match_end..];

    // Split on pipe/semicolon/&&/</> to isolate this command
    let segment = after_tool
        .split(['|', ';', '<', '>'])
        .next()
        .unwrap_or(after_tool);
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
        // --target-directory=<path> combined form
        if let Some(val) = tok.strip_prefix("--target-directory=") {
            let cleaned = val.trim_matches(|c| c == '"' || c == '\'');
            if !cleaned.is_empty() {
                explicit_dest = Some(cleaned);
            }
            continue;
        }
        if tok.starts_with('-') {
            continue;
        }
        let cleaned = tok.trim_matches(|c| c == '"' || c == '\'');
        // Skip bare numeric tokens (fd redirects like 2>/dev/null leave a trailing digit),
        // fd redirect fragments like 2>&1, and bare & from &>/dev/null splits.
        if cleaned.is_empty()
            || cleaned.bytes().all(|b| b.is_ascii_digit())
            || cleaned.contains(">&")
            || cleaned == "&"
        {
            continue;
        }
        args.push(cleaned);
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

    #[test]
    fn test_perl_include_path_no_false_positive() {
        // perl -Ilib is an include path flag, NOT an in-place edit
        let safe_cmds = [
            "perl -Ilib script.pl",
            "perl -Ilib -e 'print 1'",
            "ruby -Ilib spec/test_spec.rb",
            "ruby -Ilib -e 'puts 1'",
        ];
        for cmd in &safe_cmds {
            let paths = check_bash_write_paths(cmd);
            let non_gitc: Vec<_> = paths.iter().filter(|p| !p.starts_with("gitc:")).collect();
            assert!(
                non_gitc.is_empty(),
                "-I include flag {:?} should not produce write paths, got {:?}",
                cmd,
                non_gitc
            );
        }
    }

    #[test]
    fn test_git_format_patch_no_false_positive() {
        // git format-patch, git show --patch, git diff --patch are read-only
        let safe_cmds = [
            "git format-patch -1 HEAD",
            "git show --patch HEAD",
            "git diff --patch HEAD~1 src/file.rs",
        ];
        for cmd in &safe_cmds {
            let paths = check_bash_write_paths(cmd);
            let non_gitc: Vec<_> = paths.iter().filter(|p| !p.starts_with("gitc:")).collect();
            assert!(
                non_gitc.is_empty(),
                "git patch command {:?} should not produce write paths, got {:?}",
                cmd,
                non_gitc
            );
        }
    }

    #[test]
    fn test_sed_long_form_inplace() {
        let paths = check_bash_write_paths("sed --in-place 's/foo/bar/' src/lib.rs");
        assert!(
            paths.iter().any(|p| p == "rel:src/lib.rs"),
            "sed --in-place should detect target: {:?}",
            paths
        );
    }

    #[test]
    fn test_sed_long_form_inplace_with_suffix() {
        let paths = check_bash_write_paths("sed --in-place=.bak 's/old/new/' config.yml");
        assert!(
            paths.iter().any(|p| p == "rel:config.yml"),
            "sed --in-place=.bak should detect target: {:?}",
            paths
        );
    }

    #[test]
    fn test_git_mv_no_false_positive() {
        // git mv is a git operation, not the standalone mv command
        let safe_cmds = [
            "git mv src/old.rs src/new.rs",
            "git -C /repo/.worktrees/abc mv file1.rs file2.rs",
        ];
        for cmd in &safe_cmds {
            let paths = check_bash_write_paths(cmd);
            let non_gitc: Vec<_> = paths.iter().filter(|p| !p.starts_with("gitc:")).collect();
            assert!(
                non_gitc.is_empty(),
                "git mv {:?} should not produce write paths, got {:?}",
                cmd,
                non_gitc
            );
        }
    }

    #[test]
    fn test_git_cp_no_false_positive() {
        let paths = check_bash_write_paths("git cp src/old.rs src/new.rs");
        let non_gitc: Vec<_> = paths.iter().filter(|p| !p.starts_with("gitc:")).collect();
        assert!(
            non_gitc.is_empty(),
            "git cp should not produce write paths, got {:?}",
            non_gitc
        );
    }

    #[test]
    fn test_perl_separate_inplace_flag() {
        // perl -w -i should still be detected when -i is a separate flag
        let paths = check_bash_write_paths("perl -w -i -pe 's/foo/bar/' file.rs");
        assert!(
            paths.iter().any(|p| p == "rel:file.rs"),
            "perl -w -i should detect target: {:?}",
            paths
        );
    }

    #[test]
    fn test_ruby_separate_inplace_flag() {
        let paths = check_bash_write_paths("ruby -v -i -pe 'gsub(/foo/,\"bar\")' file.rb");
        assert!(
            paths.iter().any(|p| p == "rel:file.rb"),
            "ruby -v -i should detect target: {:?}",
            paths
        );
    }

    #[test]
    fn test_sudo_install_detected() {
        let paths = check_bash_write_paths("sudo install -m 755 /tmp/binary /usr/local/bin/tool");
        assert!(
            paths.iter().any(|p| p == "/usr/local/bin/tool"),
            "sudo install should detect dest: {:?}",
            paths
        );
    }

    #[test]
    fn test_env_install_detected() {
        let paths = check_bash_write_paths("env install -m 755 /tmp/binary /usr/local/bin/tool");
        assert!(
            paths.iter().any(|p| p == "/usr/local/bin/tool"),
            "env install should detect dest: {:?}",
            paths
        );
    }

    #[test]
    fn test_target_directory_equals_form() {
        // cp --target-directory=/path <src> should detect the destination
        let paths = check_bash_write_paths("cp --target-directory=/home/user/src/file.rs /tmp/src");
        assert!(
            paths.iter().any(|p| p == "/home/user/src/file.rs"),
            "cp --target-directory=<path> should detect dest: {:?}",
            paths
        );
    }

    #[test]
    fn test_target_directory_equals_relative() {
        let paths = check_bash_write_paths("cp --target-directory=src/ /tmp/file.rs");
        assert!(
            paths.iter().any(|p| p == "rel:src/"),
            "cp --target-directory=<rel> should return rel: prefix: {:?}",
            paths
        );
    }

    #[test]
    fn test_sed_combined_flags_ni() {
        // sed -ni.bak combines -n and -i flags — must be detected
        let paths = check_bash_write_paths("sed -ni.bak 's/foo/bar/' file.rs");
        assert!(
            paths.iter().any(|p| p == "rel:file.rs"),
            "sed -ni.bak should detect target: {:?}",
            paths
        );
    }

    #[test]
    fn test_sed_transliterate_no_false_positive() {
        // sed y/abc/def/ is a transliterate expression, not a file path
        let paths = check_bash_write_paths("sed -i '' 'y/abc/def/' file.rs");
        assert!(
            !paths.iter().any(|p| p.contains("y/abc")),
            "sed y/ expression should not be treated as file path: {:?}",
            paths
        );
        // But the actual file target should still be detected
        assert!(
            paths.iter().any(|p| p == "rel:file.rs"),
            "sed -i target file should still be detected: {:?}",
            paths
        );
    }

    #[test]
    fn test_rsync_remote_host_no_false_positive() {
        // SCP-style remote destinations should not produce write paths
        let safe_cmds = [
            "rsync -av ./dist/ deploy@prod:/var/www/html/",
            "rsync -avz /local/build/ user@backup:/data/",
        ];
        for cmd in &safe_cmds {
            let paths = check_bash_write_paths(cmd);
            let non_gitc: Vec<_> = paths.iter().filter(|p| !p.starts_with("gitc:")).collect();
            assert!(
                non_gitc.is_empty(),
                "rsync to remote host {:?} should not produce write paths, got {:?}",
                cmd,
                non_gitc
            );
        }
    }

    #[test]
    fn test_sed_redirect_does_not_mask_target() {
        // > redirect should be split on so the actual -i target is found
        let paths = check_bash_write_paths("sed -i 's/foo/bar/' src/main.rs > /tmp/anything");
        assert!(
            paths.iter().any(|p| p == "rel:src/main.rs"),
            "sed -i target must be detected even with > redirect: {:?}",
            paths
        );
    }

    #[test]
    fn test_sed_multi_file_all_detected() {
        // sed -i with multiple files — all must be detected
        let paths = check_bash_write_paths("sed -i 's/foo/bar/' src/lib.rs src/main.rs");
        assert!(
            paths.iter().any(|p| p == "rel:src/lib.rs"),
            "first file must be detected: {:?}",
            paths
        );
        assert!(
            paths.iter().any(|p| p == "rel:src/main.rs"),
            "second file must be detected: {:?}",
            paths
        );
    }

    #[test]
    fn test_gawk_inplace_not_matched_by_sed_regex() {
        // --in-place on non-sed tools should not trigger sed detection
        let paths = check_bash_write_paths("gawk --in-place=.bak '{print}' data.txt");
        // gawk is not in our detection set, so no write paths from sed regex
        let sed_paths: Vec<_> = paths
            .iter()
            .filter(|p| !p.starts_with("gitc:") && !p.starts_with("/"))
            .collect();
        assert!(
            sed_paths.is_empty(),
            "gawk --in-place should not be caught by sed regex: {:?}",
            sed_paths
        );
    }

    #[test]
    fn test_perl_inplace_backup_suffix() {
        let paths = check_bash_write_paths("perl -i.bak -pe 's/foo/bar/' src/lib.rs");
        assert!(
            paths.iter().any(|p| p == "rel:src/lib.rs"),
            "perl -i.bak should detect target: {:?}",
            paths
        );
    }

    #[test]
    fn test_ruby_inplace_backup_suffix() {
        let paths = check_bash_write_paths("ruby -i.bak -pe 'gsub(/foo/,\"bar\")' config.yml");
        assert!(
            paths.iter().any(|p| p == "rel:config.yml"),
            "ruby -i.bak should detect target: {:?}",
            paths
        );
    }

    #[test]
    fn test_tool_in_filename_no_mismatch() {
        // cp in a filename (src/cp.rs) should not cause the real cp to be missed
        let paths = check_bash_write_paths("cat src/cp.rs; cp src/lib.rs /tmp/evil.rs");
        assert!(
            paths.iter().any(|p| p == "/tmp/evil.rs"),
            "real cp dest must be detected despite cp in filename: {:?}",
            paths
        );
    }

    #[test]
    fn test_rsync_daemon_url_no_false_positive() {
        let safe_cmds = [
            "rsync ./dist/ rsync://backup.server/module/path",
            "rsync -av /local/ backup::module/path",
        ];
        for cmd in &safe_cmds {
            let paths = check_bash_write_paths(cmd);
            let non_gitc: Vec<_> = paths.iter().filter(|p| !p.starts_with("gitc:")).collect();
            assert!(
                non_gitc.is_empty(),
                "rsync daemon URL {:?} should not produce write paths, got {:?}",
                cmd,
                non_gitc
            );
        }
    }

    #[test]
    fn test_quoted_absolute_path_detected() {
        let paths = check_bash_write_paths("sed -i 's/foo/bar/' '/home/user/src/lib.rs'");
        assert!(
            paths.iter().any(|p| p == "/home/user/src/lib.rs"),
            "quoted absolute path must be detected: {:?}",
            paths
        );
    }

    #[test]
    fn test_cp_with_fd_redirect_2_dev_null() {
        let paths =
            check_bash_write_paths("cp /tmp/evil.rs /home/user/checkout/src/lib.rs 2>/dev/null");
        assert!(
            paths.iter().any(|p| p == "/home/user/checkout/src/lib.rs"),
            "cp dest must be detected despite 2>/dev/null: {:?}",
            paths
        );
    }

    #[test]
    fn test_single_char_dir_not_treated_as_sed_expr() {
        // b/lib.rs (1 slash) and b/src/lib.rs (2 slashes) must not be skipped
        let paths = check_bash_write_paths("sed -i 's/foo/bar/' b/lib.rs");
        assert!(
            paths.iter().any(|p| p == "rel:b/lib.rs"),
            "single-char dir path must be detected: {:?}",
            paths
        );
        let paths = check_bash_write_paths("sed -i 's/foo/bar/' b/src/lib.rs");
        assert!(
            paths.iter().any(|p| p == "rel:b/src/lib.rs"),
            "nested single-char dir path must be detected: {:?}",
            paths
        );
    }

    #[test]
    fn test_perl_combined_pie_flags() {
        let paths = check_bash_write_paths("perl -pie 's/foo/bar/' src/lib.rs");
        assert!(
            paths.iter().any(|p| p == "rel:src/lib.rs"),
            "perl -pie should detect target: {:?}",
            paths
        );
    }

    #[test]
    fn test_multi_stage_cp_both_detected() {
        // Both cp invocations in a chained command must be detected
        let paths = check_bash_write_paths("cp /tmp/a.rs /safe/dest && cp /tmp/b.rs src/lib.rs");
        assert!(
            paths.iter().any(|p| p == "/safe/dest"),
            "first cp dest must be detected: {:?}",
            paths
        );
        assert!(
            paths.iter().any(|p| p == "rel:src/lib.rs"),
            "second cp dest must be detected: {:?}",
            paths
        );
    }

    #[test]
    fn test_sed_flag_value_not_false_positive() {
        // -f takes a script file argument — it should not be treated as a write target
        let paths = check_bash_write_paths("sed -i -f script.sed file.rs");
        assert!(
            !paths.iter().any(|p| p.contains("script.sed")),
            "-f argument should not be a write target: {:?}",
            paths
        );
        assert!(
            paths.iter().any(|p| p == "rel:file.rs"),
            "actual file target should still be detected: {:?}",
            paths
        );
    }

    #[test]
    fn test_sed_multiple_e_flags() {
        // -e takes an expression argument — should not be a write target
        let paths = check_bash_write_paths("sed -i -e 's/foo/bar/' -e 's/baz/qux/' file.rs");
        assert!(
            paths.iter().any(|p| p == "rel:file.rs"),
            "file target should be detected with multiple -e flags: {:?}",
            paths
        );
    }

    #[test]
    fn test_sed_uppercase_flag_combined_with_i() {
        // sed -Ei combines extended regex flag with in-place — must be detected
        let paths = check_bash_write_paths("sed -Ei 's/foo/bar/' src/main.rs");
        assert!(
            paths.iter().any(|p| p == "rel:src/main.rs"),
            "sed -Ei should detect target: {:?}",
            paths
        );
        // sed -in, -iE where i is NOT the last flag
        let paths = check_bash_write_paths("sed -in 's/foo/bar/' src/lib.rs");
        assert!(
            paths.iter().any(|p| p == "rel:src/lib.rs"),
            "sed -in should detect target: {:?}",
            paths
        );
        let paths = check_bash_write_paths("sed -iE 's/foo/bar/' src/lib.rs");
        assert!(
            paths.iter().any(|p| p == "rel:src/lib.rs"),
            "sed -iE should detect target: {:?}",
            paths
        );
    }

    #[test]
    fn test_cp_with_2_redirect_ampersand() {
        // 2>&1 should not corrupt the destination detection
        let paths = check_bash_write_paths("cp /tmp/evil.rs /home/user/src/lib.rs 2>&1 | cat");
        assert!(
            paths.iter().any(|p| p == "/home/user/src/lib.rs"),
            "cp dest must be detected despite 2>&1: {:?}",
            paths
        );
    }

    #[test]
    fn test_relative_redirect_detected() {
        let paths = check_bash_write_paths("echo hacked > src/main.rs");
        assert!(
            paths.iter().any(|p| p == "rel:src/main.rs"),
            "relative redirect must be detected: {:?}",
            paths
        );
    }

    #[test]
    fn test_relative_tee_detected() {
        let paths = check_bash_write_paths("echo data | tee src/lib.rs");
        assert!(
            paths.iter().any(|p| p == "rel:src/lib.rs"),
            "relative tee must be detected: {:?}",
            paths
        );
    }

    #[test]
    fn test_sudo_cp_detected() {
        let paths = check_bash_write_paths("sudo cp /tmp/evil.rs /home/user/src/lib.rs");
        assert!(
            paths.iter().any(|p| p == "/home/user/src/lib.rs"),
            "sudo cp must be detected: {:?}",
            paths
        );
    }

    #[test]
    fn test_sudo_mv_detected() {
        let paths = check_bash_write_paths("sudo mv /tmp/evil.rs src/lib.rs");
        assert!(
            paths.iter().any(|p| p == "rel:src/lib.rs"),
            "sudo mv must be detected: {:?}",
            paths
        );
    }

    #[test]
    fn test_redirect_dev_null_allowed() {
        // /dev/null should be captured but sandbox allows it
        let paths = check_bash_write_paths("echo test > /dev/null");
        assert!(
            paths.iter().any(|p| p == "/dev/null"),
            "/dev/null redirect should be captured: {:?}",
            paths
        );
    }
}
