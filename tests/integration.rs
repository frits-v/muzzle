//! Integration tests for hooks binaries.
//!
//! These tests exercise the binaries end-to-end by invoking them as subprocesses
//! with JSON on stdin and checking stdout/exit codes.
//!
//! Run with: cargo test --test integration

use std::io::Write;
use std::process::{Command, Stdio};

/// Helper: run a binary with JSON on stdin, return (stdout, stderr, exit_code).
fn run_binary(name: &str, json_input: &str) -> (String, String, i32) {
    let binary = format!("target/debug/{}", name);
    let mut child = Command::new(&binary)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap_or_else(|e| panic!("failed to spawn {}: {} — run `cargo build` first", binary, e));

    if let Some(ref mut stdin) = child.stdin {
        stdin.write_all(json_input.as_bytes()).unwrap();
    }
    drop(child.stdin.take());

    let output = child.wait_with_output().expect("failed to wait");
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let code = output.status.code().unwrap_or(-1);

    (stdout, stderr, code)
}

// ---------------------------------------------------------------------------
// permissions binary tests
// ---------------------------------------------------------------------------

#[test]
fn test_permissions_read_tool_allows() {
    let input = r#"{"tool_name":"Read","tool_input":{"file_path":"/tmp/test.txt"}}"#;
    let (stdout, _stderr, code) = run_binary("permissions", input);
    assert_eq!(code, 0);
    assert!(stdout.contains("allow"), "Read should be allowed, got: {}", stdout);
}

#[test]
fn test_permissions_system_path_denies() {
    let input = r#"{"tool_name":"Write","tool_input":{"file_path":"/etc/hosts"}}"#;
    let (stdout, _stderr, code) = run_binary("permissions", input);
    assert_eq!(code, 0);
    assert!(stdout.contains("deny"), "Write to /etc/hosts should be denied, got: {}", stdout);
}

#[test]
fn test_permissions_force_push_denies() {
    let input = r#"{"tool_name":"Bash","tool_input":{"command":"git push --force origin main"}}"#;
    let (stdout, _stderr, code) = run_binary("permissions", input);
    assert_eq!(code, 0);
    assert!(stdout.contains("deny"), "Force push should be denied, got: {}", stdout);
}

#[test]
fn test_permissions_safe_bash_allows() {
    let input = r#"{"tool_name":"Bash","tool_input":{"command":"echo hello"}}"#;
    let (stdout, _stderr, code) = run_binary("permissions", input);
    assert_eq!(code, 0);
    assert!(stdout.contains("allow"), "echo should be allowed, got: {}", stdout);
}

#[test]
fn test_permissions_empty_input_exits_clean() {
    let (_, _, code) = run_binary("permissions", "{}");
    assert_eq!(code, 0);
}

#[test]
fn test_permissions_invalid_json_exits_clean() {
    let (_, _, code) = run_binary("permissions", "not json");
    assert_eq!(code, 0, "invalid JSON should exit 0 (fail open to settings.json)");
}

// ---------------------------------------------------------------------------
// ensure-worktree binary tests
// ---------------------------------------------------------------------------

#[test]
fn test_ensure_worktree_no_args() {
    let output = Command::new("target/debug/ensure-worktree")
        .output()
        .expect("failed to run ensure-worktree");

    let code = output.status.code().unwrap_or(-1);
    assert_eq!(code, 1, "no args should exit 1");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Usage"), "should print usage, got: {}", stderr);
}

#[test]
fn test_ensure_worktree_no_session() {
    let output = Command::new("target/debug/ensure-worktree")
        .arg("nonexistent-repo")
        .output()
        .expect("failed to run ensure-worktree");

    let code = output.status.code().unwrap_or(-1);
    assert_eq!(code, 1, "no session should exit 1");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("No active session") || stderr.contains("ERROR"),
        "should report no session, got: {}",
        stderr
    );
}

// ---------------------------------------------------------------------------
// changelog binary tests
// ---------------------------------------------------------------------------

#[test]
fn test_changelog_read_only_skips() {
    // Read-only tool should produce no output and exit 0
    let input = r#"{"tool_name":"Read","tool_input":{"file_path":"/tmp/test.txt"},"tool_output":{}}"#;
    let (stdout, _stderr, code) = run_binary("changelog", input);
    assert_eq!(code, 0);
    assert!(stdout.is_empty(), "read-only tool should produce no stdout");
}

#[test]
fn test_changelog_empty_input_exits_clean() {
    let (_, _, code) = run_binary("changelog", "{}");
    assert_eq!(code, 0);
}
