//! Git safety checks for Bash commands.
//!
//! FR-GS-1 through FR-GS-9: All 9 git safety patterns.

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

// Pre-compiled regexes for the 9 git safety patterns.
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
static RE_GH_API_COMMIT_PATH: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\bgh\s+api\b.*/(?:contents/|git/(?:commits|trees|refs|blobs))").unwrap()
});
static RE_GH_API_WRITE_METHOD: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(-X[=\s]*(PUT|POST|PATCH|DELETE)|--method[=\s]+(PUT|POST|PATCH|DELETE))").unwrap()
});
static RE_GH_API_WRITE_BODY: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\s(-f|--field|-F|--raw-field|-d|--data|--input)[=\s]").unwrap());

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

// Bash write-path extraction uses a tokenizer (see `tokenize_bash` and
// `check_bash_write_paths`) for redirects, tee, and `git -C` so quoting,
// fd-redirect digits, and operators without whitespace are handled correctly.
// The regexes below cover the remaining file-mutating commands that the
// tokenizer doesn't model — in-place editors and copy/move utilities — which
// are bypass vectors for editing the main checkout after the Edit tool is denied.
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

/// Run all 9 git safety checks against a Bash command.
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

    // FR-GS-9: gh api calls that create server-side commits (bypass signing)
    if RE_GH_API_COMMIT_PATH.is_match(cmd)
        && (RE_GH_API_WRITE_METHOD.is_match(cmd) || RE_GH_API_WRITE_BODY.is_match(cmd))
    {
        return GitResult::Block(
            "WHAT: gh api to a commit-creating endpoint makes a server-side commit \
             that bypasses local GPG/SSH signing. \
             FIX: Commit locally with `git commit` (signed) and push instead. \
             REF: CLAUDE.md#supply-chain-policy"
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

    // Block bare mutating git commands (no -C, no cd context).
    // When worktrees are active, mutating git ops must target the worktree explicitly.
    // Per-segment analysis: each command segment is checked independently for -C and cd.
    if let Some(subcmd) = find_bare_mutating_git(cmd) {
        return Some(format!(
            "WHAT: Bare `git {subcmd}` runs in the main checkout CWD, not the session worktree. \
             FIX: Use `git -C <repo>/.worktrees/{short_id}/ {subcmd} ...` instead. \
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
/// Redirect, tee, and `git -C` targets are found by tokenizing the command
/// (honoring shell quoting), which avoids the regex-on-raw-string pitfall where
/// `>` inside a quoted argument — e.g. `--description "foo/<name>/modules/"` —
/// was mistaken for a redirect. File-mutating commands the tokenizer doesn't
/// model (sed -i, perl/ruby -i, cp, mv, install, rsync, dd, patch) are matched
/// by regex afterward.
///
/// Returns paths with optional prefixes:
/// - No prefix: absolute write target from a redirect/tee/file-mutating command
/// - `gitc:` prefix: git -C working directory (not a direct write target)
/// - `rel:` prefix: relative path from a file-mutating command (sed -i, cp, mv, etc.)
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
                // tee accepts multiple output files — capture every absolute
                // target up to the next separator, not just the first.
                while let Some(BashToken::Word(target)) = tokens.get(j) {
                    if target.starts_with('/') {
                        paths.push(target.clone());
                    }
                    j += 1;
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

/// Find a bare (no `-C`, no `cd` context) mutating git subcommand in a
/// (possibly compound) command.
///
/// Splits on shell separators (`&&`, `||`, `;`, `|`, `&`) and checks each
/// segment independently. A segment is "bare" when the git invocation has
/// no `-C` flag AND no preceding `cd` in the same segment.
///
/// Command substitutions (`$(...)`, backticks) are stripped first so an inner
/// `cd` cannot exempt the outer git (`git add $(cd /tmp && echo .)`).
///
/// Returns the subcommand name if a bare mutating invocation is found.
fn find_bare_mutating_git(cmd: &str) -> Option<String> {
    let cmd = strip_command_substitution(cmd);
    for segment in RE_CMD_SEP.split(&cmd) {
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

/// Strip `$(...)` and backtick command substitutions from a command.
///
/// Removes the entire substitution span (including its contents) so an inner
/// `cd`/`-C` inside a substitution can't exempt the outer git invocation, and
/// so a substitution's operators don't fragment segment splitting. Handles
/// nested `$(...)`. Unbalanced openers consume to end of string.
fn strip_command_substitution(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    let mut depth = 0usize;
    let mut in_backtick = false;
    while let Some(c) = chars.next() {
        if in_backtick {
            if c == '`' {
                in_backtick = false;
            }
            continue;
        }
        if depth > 0 {
            match c {
                '(' => depth += 1,
                ')' => depth -= 1,
                _ => {}
            }
            continue;
        }
        if c == '$' && chars.peek() == Some(&'(') {
            chars.next();
            depth = 1;
            continue;
        }
        if c == '`' {
            in_backtick = true;
            continue;
        }
        out.push(c);
    }
    out
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
    fn test_bash_write_paths_tee_multiple_targets() {
        // tee accepts multiple output files — every absolute target must be seen,
        // not just the first.
        let paths = check_bash_write_paths("echo x | tee /tmp/a.txt /tmp/b.txt /tmp/c.txt");
        for want in ["/tmp/a.txt", "/tmp/b.txt", "/tmp/c.txt"] {
            assert!(
                paths.iter().any(|p| p == want),
                "expected {want} in tee targets: {:?}",
                paths
            );
        }
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

    // FR-GS-9: gh api server-side commit endpoints
    #[test]
    fn test_gh_api_commit_endpoints_blocked() {
        let blocked = [
            "gh api repos/owner/repo/contents/README.md -X PUT -f message=update",
            "gh api repos/owner/repo/git/commits -X POST",
            "gh api repos/owner/repo/git/trees -X POST",
            "gh api repos/owner/repo/git/refs -X POST",
            "gh api repos/owner/repo/git/blobs -X POST",
            // Method flag before path
            "gh api -X POST repos/owner/repo/git/commits -f tree=abc123",
            "gh api --method POST repos/owner/repo/git/commits",
            "gh api --method PUT repos/owner/repo/contents/file.txt -f message=update",
            // Equals-delimited method flag
            "gh api --method=POST repos/owner/repo/git/commits -f tree=abc123",
            "gh api --method=PUT repos/owner/repo/contents/file.txt -f content=...",
            // Short flag concatenated (no space)
            "gh api repos/owner/repo/git/commits -XPOST -f tree=abc123",
            "gh api repos/owner/repo/contents/file.txt -XPUT -f content=...",
            // Implicit POST via body fields (no -X/--method)
            "gh api repos/owner/repo/contents/file.txt -f message=create -f content=abc",
            "gh api repos/owner/repo/git/commits -f message=bypass -f tree=abc123",
            // Equals-delimited body flags (implicit POST)
            r#"gh api repos/owner/repo/git/commits --data='{"message":"bypass","tree":"abc123"}'"#,
            "gh api repos/owner/repo/contents/file.txt --field=message=create --field=content=aGVsbG8=",
            "gh api repos/owner/repo/git/commits --input=payload.json",
        ];
        for cmd in &blocked {
            let r = check_git_safety(cmd);
            assert!(
                matches!(r, GitResult::Block(_)),
                "expected BLOCK for {:?}",
                cmd
            );
        }
    }

    #[test]
    fn test_gh_api_read_endpoints_not_blocked() {
        let allowed = [
            "gh api repos/owner/repo/pulls/123",
            "gh api repos/owner/repo/issues",
            "gh api repos/owner/repo/commits",
            "gh api repos/owner/repo/contents/README.md",
            "gh api repos/owner/repo/git/refs",
            "gh api repos/owner/repo/git/trees/abc123",
            "gh api repos/owner/repo/git/blobs/abc123",
            "gh api repos/owner/repo/git/commits/abc123",
            "gh pr list",
        ];
        for cmd in &allowed {
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
    fn test_cd_in_separate_segment_still_blocks_git() {
        // `;` splits into two segments; the `git add` segment carries no cd,
        // so it is bare and blocked. (The shell would change dirs, but the hook
        // sees the full pre-execution command and can't track CWD across `;`.)
        let cmd = "cd /ws/repo/.worktrees/abc12345; git add .";
        let reason = check_worktree_enforcement(cmd, true, "abc12345");
        assert!(
            reason.is_some(),
            "cd in a separate segment should block bare git"
        );
    }

    #[test]
    fn test_command_substitution_cd_does_not_bypass() {
        // Regression: a cd inside a $(...) substitution must not exempt the
        // outer bare git invocation.
        let cmd = "git add $(cd /tmp && echo .)";
        let reason = check_worktree_enforcement(cmd, true, "abc12345");
        assert!(
            reason.is_some(),
            "cd inside command substitution should not bypass bare git check"
        );
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
