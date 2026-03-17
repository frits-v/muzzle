//! Integration tests for hooks binaries.
//!
//! These tests exercise the binaries end-to-end by invoking them as subprocesses
//! with JSON on stdin and checking stdout/exit codes.
//!
//! Run with: cargo test --test integration

use std::io::Write;
use std::process::{Command, Stdio};

/// Read a config key from ~/.config/muzzle/config.
fn read_config_key(key: &str) -> Option<String> {
    let home = std::env::var("HOME").expect("HOME not set");
    let config_path = format!("{}/.config/muzzle/config", home);
    let content = std::fs::read_to_string(&config_path).ok()?;
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            if k.trim() == key {
                let v = v.trim();
                if !v.is_empty() {
                    return Some(v.to_string());
                }
            }
        }
    }
    None
}

/// Resolve the first workspace path the same way the binary does.
fn test_workspace() -> String {
    // New multi-workspace env
    if let Ok(val) = std::env::var("MUZZLE_WORKSPACES") {
        if let Some(first) = val.split(',').next() {
            let first = first.trim();
            if !first.is_empty() {
                return first.to_string();
            }
        }
    }
    // New multi-workspace config key
    if let Some(val) = read_config_key("workspaces") {
        if let Some(first) = val.split(',').next() {
            let first = first.trim();
            if !first.is_empty() {
                return first.to_string();
            }
        }
    }
    // Legacy single workspace
    if let Ok(ws) = std::env::var("MUZZLE_WORKSPACE") {
        if !ws.is_empty() {
            return ws;
        }
    }
    if let Some(ws) = read_config_key("workspace") {
        return ws;
    }
    let home = std::env::var("HOME").expect("HOME not set");
    format!("{}/src", home)
}

/// Resolve the state directory the same way the binary does.
fn test_state_dir() -> String {
    if let Ok(sd) = std::env::var("MUZZLE_STATE_DIR") {
        if !sd.is_empty() {
            return sd;
        }
    }
    if let Some(sd) = read_config_key("state_dir") {
        return sd;
    }
    if let Ok(xdg) = std::env::var("XDG_STATE_HOME") {
        if !xdg.is_empty() {
            return format!("{}/muzzle", xdg);
        }
    }
    let home = std::env::var("HOME").expect("HOME not set");
    format!("{}/.local/state/muzzle", home)
}

/// Helper: run a binary with JSON on stdin, return (stdout, stderr, exit_code).
fn run_binary(name: &str, json_input: &str) -> (String, String, i32) {
    let binary = format!("target/debug/{}", name);
    let mut child = Command::new(&binary)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap_or_else(|e| {
            panic!(
                "failed to spawn {}: {} — run `cargo build` first",
                binary, e
            )
        });

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
    assert!(
        stdout.contains("allow"),
        "Read should be allowed, got: {}",
        stdout
    );
}

#[test]
fn test_permissions_system_path_denies() {
    let input = r#"{"tool_name":"Write","tool_input":{"file_path":"/etc/hosts"}}"#;
    let (stdout, _stderr, code) = run_binary("permissions", input);
    assert_eq!(code, 0);
    assert!(
        stdout.contains("deny"),
        "Write to /etc/hosts should be denied, got: {}",
        stdout
    );
}

#[test]
fn test_permissions_force_push_denies() {
    let input = r#"{"tool_name":"Bash","tool_input":{"command":"git push --force origin main"}}"#;
    let (stdout, _stderr, code) = run_binary("permissions", input);
    assert_eq!(code, 0);
    assert!(
        stdout.contains("deny"),
        "Force push should be denied, got: {}",
        stdout
    );
}

#[test]
fn test_permissions_safe_bash_allows() {
    let input = r#"{"tool_name":"Bash","tool_input":{"command":"echo hello"}}"#;
    let (stdout, _stderr, code) = run_binary("permissions", input);
    assert_eq!(code, 0);
    assert!(
        stdout.contains("allow"),
        "echo should be allowed, got: {}",
        stdout
    );
}

#[test]
fn test_permissions_empty_input_exits_clean() {
    let (_, _, code) = run_binary("permissions", "{}");
    assert_eq!(code, 0);
}

#[test]
fn test_permissions_invalid_json_exits_clean() {
    let (_, _, code) = run_binary("permissions", "not json");
    assert_eq!(
        code, 0,
        "invalid JSON should exit 0 (fail open to settings.json)"
    );
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
    assert!(
        stderr.contains("Usage"),
        "should print usage, got: {}",
        stderr
    );
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
// WORKTREE_MISSING integration tests
// ---------------------------------------------------------------------------

// These tests create a fake session by writing a PID marker file for the test
// process's PID, then spawn the permissions binary (which becomes a child of
// this process). The PPID walk in the permissions binary finds the fake marker
// at depth 1.
//
// A mutex serializes these tests to prevent concurrent PID marker writes.

use std::path::PathBuf;
use std::sync::Mutex;

static WORKTREE_TEST_LOCK: Mutex<()> = Mutex::new(());

struct TestSessionGuard {
    marker_path: PathBuf,
    spec_path: PathBuf,
}

impl Drop for TestSessionGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.marker_path);
        let _ = std::fs::remove_file(&self.spec_path);
    }
}

/// Set up a fake session for integration testing.
///
/// Writes a PID marker for the current process (which is the PPID of any
/// child processes we spawn) and returns a guard that cleans up on drop.
/// PID markers and spec files go to the state directory (XDG).
fn setup_fake_session(session_id: &str) -> TestSessionGuard {
    let sd = test_state_dir();

    let pid = std::process::id();
    let marker_dir = format!("{}/by-pid", sd);
    let marker_path = PathBuf::from(format!("{}/{}", marker_dir, pid));

    std::fs::create_dir_all(&marker_dir).expect("create marker dir");
    std::fs::write(&marker_path, session_id).expect("write marker");

    let spec_dir = format!("{}/specs", sd);
    std::fs::create_dir_all(&spec_dir).expect("create specs dir");
    let spec_path = PathBuf::from(format!("{}/{}.env", spec_dir, session_id));

    TestSessionGuard {
        marker_path,
        spec_path,
    }
}

#[test]
fn test_permissions_worktree_missing_bash_git_op() {
    let _lock = WORKTREE_TEST_LOCK.lock().unwrap();
    let _guard = setup_fake_session("test-wt-missing-00000001");

    // No spec file → worktree_active=false → FR-WE-2 path
    let cmd = format!("git -C {}/ops status", test_workspace());
    let input = format!(
        r#"{{"tool_name":"Bash","tool_input":{{"command":"{}"}}}}"#,
        cmd
    );
    let (stdout, _stderr, code) = run_binary("permissions", &input);

    assert_eq!(code, 0);
    assert!(
        stdout.contains("WORKTREE_MISSING"),
        "git op on workspace repo without worktree should deny with WORKTREE_MISSING, got: {}",
        stdout
    );
    assert!(
        stdout.contains("ops"),
        "WORKTREE_MISSING should reference repo name 'ops', got: {}",
        stdout
    );
}

#[test]
fn test_permissions_worktree_missing_write_to_repo() {
    let _lock = WORKTREE_TEST_LOCK.lock().unwrap();
    let _guard = setup_fake_session("test-wt-missing-00000002");

    // No spec file → worktree_active=false → FR-WE-2 path
    let file_path = format!("{}/ops/main.tf", test_workspace());
    let input = format!(
        r#"{{"tool_name":"Write","tool_input":{{"file_path":"{}"}}}}"#,
        file_path
    );
    let (stdout, _stderr, code) = run_binary("permissions", &input);

    assert_eq!(code, 0);
    assert!(
        stdout.contains("WORKTREE_MISSING"),
        "Write to workspace repo without worktree should deny with WORKTREE_MISSING, got: {}",
        stdout
    );
}

#[test]
fn test_permissions_worktree_missing_with_active_session() {
    let _lock = WORKTREE_TEST_LOCK.lock().unwrap();
    let guard = setup_fake_session("test-wt-missing-00000003");

    // Create spec file with one repo to make worktree_active=true,
    // then test write to a DIFFERENT repo that has no worktree dir.
    std::fs::write(
        &guard.spec_path,
        "web-app|wt/test-wt-m|/fake/wt/path|/fake/repo/path\n",
    )
    .expect("write spec");

    // Now Write to "ops" — which has no .worktrees/test-wt-m/ dir
    let file_path = format!("{}/ops/main.tf", test_workspace());
    let input = format!(
        r#"{{"tool_name":"Write","tool_input":{{"file_path":"{}"}}}}"#,
        file_path
    );
    let (stdout, _stderr, code) = run_binary("permissions", &input);

    assert_eq!(code, 0);
    assert!(
        stdout.contains("WORKTREE_MISSING"),
        "Write to different repo (worktree_active=true, but ops has no wt dir) should WORKTREE_MISSING, got: {}",
        stdout
    );
}

// ---------------------------------------------------------------------------
// changelog binary tests
// ---------------------------------------------------------------------------

#[test]
fn test_changelog_read_only_skips() {
    // Read-only tool should produce no output and exit 0
    let input =
        r#"{"tool_name":"Read","tool_input":{"file_path":"/tmp/test.txt"},"tool_output":{}}"#;
    let (stdout, _stderr, code) = run_binary("changelog", input);
    assert_eq!(code, 0);
    assert!(stdout.is_empty(), "read-only tool should produce no stdout");
}

#[test]
fn test_changelog_empty_input_exits_clean() {
    let (_, _, code) = run_binary("changelog", "{}");
    assert_eq!(code, 0);
}
