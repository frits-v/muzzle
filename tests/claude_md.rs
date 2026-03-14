//! Validates that CLAUDE.md claims match the actual codebase.
//!
//! Replaces the shell script (scripts/check-claude-md.sh) with a portable
//! Rust integration test that runs on any platform via `cargo test`.
//!
//! Checks:
//! 1. Binary count matches [[bin]] entries in Cargo.toml
//! 2. Architecture tree lists every .rs file in src/
//! 3. Dependency count matches [dependencies] in Cargo.toml
//! 4. Every Cargo.toml dependency is mentioned in CLAUDE.md
//! 5. Make targets referenced in CLAUDE.md exist in Makefile

use std::collections::HashSet;
use std::fs;
use std::path::Path;

fn read_file(name: &str) -> String {
    fs::read_to_string(name).unwrap_or_else(|e| panic!("{name} not found: {e}"))
}

fn rs_files_in(dir: &Path) -> HashSet<String> {
    let mut files = HashSet::new();
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                files.extend(rs_files_in(&path));
            } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
                if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                    files.insert(name.to_string());
                }
            }
        }
    }
    files
}

/// Extract all backtick-quoted words from text.
fn backtick_words(text: &str) -> HashSet<String> {
    let mut words = HashSet::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '`' {
            let word: String = chars.by_ref().take_while(|&c| c != '`').collect();
            if !word.is_empty() {
                words.insert(word);
            }
        }
    }
    words
}

// --- 1. Binary count ---

#[test]
fn claude_md_binary_count_matches() {
    let claude = read_file("CLAUDE.md");
    let cargo = read_file("Cargo.toml");

    // Extract "producing N binaries" from CLAUDE.md
    let claimed: usize = claude
        .lines()
        .find_map(|line| {
            if let Some(pos) = line.find("producing ") {
                let rest = &line[pos + "producing ".len()..];
                rest.split_whitespace().next().and_then(|n| n.parse().ok())
            } else {
                None
            }
        })
        .expect("CLAUDE.md should contain 'producing N binaries'");

    let actual = cargo.lines().filter(|l| l.trim() == "[[bin]]").count();

    assert_eq!(
        claimed, actual,
        "CLAUDE.md says {claimed} binaries, Cargo.toml has {actual}"
    );
}

// --- 2. Architecture tree completeness ---

/// States for the architecture block parser.
enum ArchState {
    /// Haven't seen "## Architecture" yet.
    Searching,
    /// Seen the heading, waiting for the opening ``` fence.
    WaitingForFence,
    /// Inside the code block, collecting .rs filenames.
    InBlock,
}

#[test]
fn claude_md_architecture_tree_complete() {
    let claude = read_file("CLAUDE.md");
    let actual_files = rs_files_in(Path::new("src"));

    let mut state = ArchState::Searching;
    let mut arch_files = HashSet::new();
    for line in claude.lines() {
        match state {
            ArchState::Searching => {
                if line.starts_with("## Architecture") {
                    state = ArchState::WaitingForFence;
                }
            }
            ArchState::WaitingForFence => {
                if line.trim().starts_with("```") {
                    state = ArchState::InBlock;
                }
            }
            ArchState::InBlock => {
                if line.trim() == "```" {
                    break;
                }
                for word in line.split_whitespace() {
                    if word.ends_with(".rs") {
                        arch_files.insert(word.to_string());
                    }
                }
            }
        }
    }

    let missing: Vec<_> = actual_files.difference(&arch_files).collect();
    assert!(
        missing.is_empty(),
        "Source files not in CLAUDE.md architecture tree: {missing:?}"
    );

    let stale: Vec<_> = arch_files.difference(&actual_files).collect();
    assert!(
        stale.is_empty(),
        "CLAUDE.md architecture tree lists files that don't exist: {stale:?}"
    );
}

// --- 3. Dependency count ---

#[test]
fn claude_md_dependency_count_matches() {
    let claude = read_file("CLAUDE.md");
    let cargo = read_file("Cargo.toml");

    // Extract "N crates" from CLAUDE.md
    let claimed: usize = claude
        .lines()
        .find_map(|line| {
            if let Some(pos) = line.find(" crates") {
                // Walk backward to find the number
                let prefix = line[..pos].trim();
                prefix
                    .rsplit_once(|c: char| !c.is_ascii_digit())
                    .map(|(_, n)| n)
                    .or(Some(prefix))
                    .and_then(|n| n.parse().ok())
            } else {
                None
            }
        })
        .expect("CLAUDE.md should contain 'N crates'");

    // Count [dependencies] entries
    let mut in_deps = false;
    let mut actual = 0usize;
    for line in cargo.lines() {
        let trimmed = line.trim();
        if trimmed == "[dependencies]" {
            in_deps = true;
            continue;
        }
        if trimmed.starts_with('[') {
            in_deps = false;
            continue;
        }
        if in_deps && !trimmed.is_empty() && !trimmed.starts_with('#') && trimmed.contains('=') {
            actual += 1;
        }
    }

    assert_eq!(
        claimed, actual,
        "CLAUDE.md says {claimed} crates, Cargo.toml has {actual}"
    );
}

// --- 4. Named dependencies mentioned ---

#[test]
fn claude_md_mentions_all_dependencies() {
    let claude = read_file("CLAUDE.md");
    let cargo = read_file("Cargo.toml");

    let backticks = backtick_words(&claude);

    let mut in_deps = false;
    let mut missing = Vec::new();
    for line in cargo.lines() {
        let trimmed = line.trim();
        if trimmed == "[dependencies]" {
            in_deps = true;
            continue;
        }
        if trimmed.starts_with('[') {
            in_deps = false;
            continue;
        }
        if in_deps && !trimmed.is_empty() && !trimmed.starts_with('#') {
            if let Some(name) = trimmed.split(['=', ' ']).next() {
                if !name.is_empty() && !backticks.contains(name) {
                    missing.push(name.to_string());
                }
            }
        }
    }

    assert!(
        missing.is_empty(),
        "Dependencies in Cargo.toml not mentioned (backtick-quoted) in CLAUDE.md: {missing:?}"
    );
}

// --- 5. Make targets exist ---

#[test]
fn claude_md_make_targets_exist() {
    let claude = read_file("CLAUDE.md");
    let makefile = read_file("Makefile");

    // Collect make targets defined in Makefile (lines starting with "target:")
    let defined: HashSet<String> = makefile
        .lines()
        .filter_map(|line| {
            if let Some(target) = line.split(':').next() {
                let t = target.trim();
                if !t.is_empty() && !t.starts_with('#') && !t.starts_with('.') && !t.contains(' ') {
                    return Some(t.to_string());
                }
            }
            None
        })
        .collect();

    // Extract "make <target>" references from CLAUDE.md, but only inside
    // backtick-fenced code blocks or inline backtick spans to avoid matching
    // natural-language prose like "make sure" or "make use of".
    let mut missing = Vec::new();
    let mut in_code_block = false;
    for line in claude.lines() {
        if line.trim().starts_with("```") {
            in_code_block = !in_code_block;
            continue;
        }

        // Scan code blocks fully, and inline backtick spans in prose lines
        let search_text = if in_code_block {
            line.to_string()
        } else {
            // Extract only backtick-quoted spans from prose lines
            let mut spans = String::new();
            let mut in_backtick = false;
            for ch in line.chars() {
                if ch == '`' {
                    in_backtick = !in_backtick;
                    spans.push(' ');
                } else if in_backtick {
                    spans.push(ch);
                }
            }
            spans
        };

        let mut rest = search_text.as_str();
        while let Some(pos) = rest.find("make ") {
            let after = &rest[pos + 5..];
            let target: String = after
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '-' || *c == '_')
                .collect();
            if !target.is_empty() && !defined.contains(&target) {
                missing.push(target);
            }
            rest = &rest[pos + 5..];
        }
    }

    // Deduplicate
    missing.sort();
    missing.dedup();

    assert!(
        missing.is_empty(),
        "Make targets referenced in CLAUDE.md but not in Makefile: {missing:?}"
    );
}
