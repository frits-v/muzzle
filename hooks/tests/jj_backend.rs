//! Integration tests for VCS backend abstraction.
//!
//! Tests VCS detection, backend trait implementations (Git and Jj), safety
//! checks, workspace management detection, and spec file roundtrips with
//! VCS kind persistence.
//!
//! Run with: cargo test -p muzzle-hooks --test jj_backend

use muzzle::gitcheck::GitResult;
use muzzle::session::{self, SpecEntry};
use muzzle::vcs::git::GitBackend;
use muzzle::vcs::jj::JjBackend;
use muzzle::vcs::{self, VcsBackend, VcsKind};
use std::fs;
use std::path::PathBuf;

fn create_temp_dir(suffix: &str) -> PathBuf {
    let dir =
        std::env::temp_dir().join(format!("muzzle-test-jj-{}-{}", std::process::id(), suffix));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn cleanup(dir: &PathBuf) {
    let _ = fs::remove_dir_all(dir);
}

// ---------------------------------------------------------------------------
// VCS detection tests
// ---------------------------------------------------------------------------

#[test]
fn test_vcs_detect_git_only() {
    let dir = create_temp_dir("detect-git");
    fs::create_dir(dir.join(".git")).unwrap();
    assert_eq!(vcs::detect(&dir), VcsKind::Git);
    cleanup(&dir);
}

#[test]
fn test_vcs_detect_jj_only() {
    let dir = create_temp_dir("detect-jj");
    fs::create_dir(dir.join(".jj")).unwrap();
    assert_eq!(vcs::detect(&dir), VcsKind::Jj);
    cleanup(&dir);
}

#[test]
fn test_vcs_detect_colocated() {
    let dir = create_temp_dir("detect-coloc");
    fs::create_dir(dir.join(".jj")).unwrap();
    fs::create_dir(dir.join(".git")).unwrap();
    assert_eq!(vcs::detect(&dir), VcsKind::JjColocated);
    cleanup(&dir);
}

#[test]
fn test_vcs_detect_empty() {
    let dir = create_temp_dir("detect-empty");
    assert_eq!(vcs::detect(&dir), VcsKind::Git); // default
    cleanup(&dir);
}

// ---------------------------------------------------------------------------
// GitBackend delegation tests
// ---------------------------------------------------------------------------

#[test]
fn test_git_backend_kind() {
    let backend = GitBackend;
    assert_eq!(backend.kind(), VcsKind::Git);
}

#[test]
fn test_git_backend_safety_delegates() {
    let backend = GitBackend;
    // Safe command
    assert_eq!(backend.check_safety("git status"), GitResult::Ok);
    // Dangerous command
    match backend.check_safety("git push --force origin main") {
        GitResult::Block(_) => {} // expected
        other => panic!("expected Block, got {other:?}"),
    }
}

#[test]
fn test_git_backend_is_repo() {
    let dir = create_temp_dir("git-is-repo");
    let backend = GitBackend;
    assert!(!backend.is_repo(&dir)); // no .git
    fs::create_dir(dir.join(".git")).unwrap();
    assert!(backend.is_repo(&dir));
    cleanup(&dir);
}

// ---------------------------------------------------------------------------
// JjBackend safety tests (regex-only, no jj CLI needed)
// ---------------------------------------------------------------------------

#[test]
fn test_jj_backend_kind() {
    let backend = JjBackend { colocated: false };
    assert_eq!(backend.kind(), VcsKind::Jj);
}

#[test]
fn test_jj_backend_kind_colocated() {
    let backend = JjBackend { colocated: true };
    assert_eq!(backend.kind(), VcsKind::JjColocated);
}

#[test]
fn test_jj_backend_safety_blocks_bare_push() {
    let backend = JjBackend { colocated: false };
    match backend.check_safety("jj git push") {
        GitResult::Block(reason) => assert!(reason.contains("bookmark"), "reason: {reason}"),
        other => panic!("expected Block, got {other:?}"),
    }
}

#[test]
fn test_jj_backend_safety_allows_bookmark_push() {
    let backend = JjBackend { colocated: false };
    assert_eq!(
        backend.check_safety("jj git push -b my-feature"),
        GitResult::Ok
    );
}

#[test]
fn test_jj_backend_safety_allows_long_bookmark_flag() {
    let backend = JjBackend { colocated: false };
    assert_eq!(
        backend.check_safety("jj git push --bookmark my-feature"),
        GitResult::Ok
    );
}

#[test]
fn test_jj_backend_safety_blocks_delete_main() {
    let backend = JjBackend { colocated: false };
    match backend.check_safety("jj bookmark delete main") {
        GitResult::Block(reason) => assert!(reason.contains("protected"), "reason: {reason}"),
        other => panic!("expected Block, got {other:?}"),
    }
}

#[test]
fn test_jj_backend_safety_blocks_delete_master() {
    let backend = JjBackend { colocated: false };
    match backend.check_safety("jj bookmark delete master") {
        GitResult::Block(_) => {} // expected
        other => panic!("expected Block, got {other:?}"),
    }
}

#[test]
fn test_jj_backend_safety_allows_delete_feature() {
    let backend = JjBackend { colocated: false };
    assert_eq!(
        backend.check_safety("jj bookmark delete feature-x"),
        GitResult::Ok
    );
}

#[test]
fn test_jj_backend_safety_allows_safe_commands() {
    let backend = JjBackend { colocated: false };
    assert_eq!(backend.check_safety("jj log"), GitResult::Ok);
    assert_eq!(backend.check_safety("jj status"), GitResult::Ok);
    assert_eq!(backend.check_safety("jj diff"), GitResult::Ok);
    assert_eq!(backend.check_safety("jj new"), GitResult::Ok);
}

#[test]
fn test_jj_backend_colocated_blocks_git_force_push() {
    let backend = JjBackend { colocated: true };
    match backend.check_safety("git push --force") {
        GitResult::Block(_) => {} // expected: colocated delegates to git safety
        other => panic!("expected Block, got {other:?}"),
    }
}

#[test]
fn test_jj_backend_non_colocated_ignores_git_commands() {
    let backend = JjBackend { colocated: false };
    // Non-colocated jj should not check raw git commands
    assert_eq!(backend.check_safety("git push --force"), GitResult::Ok);
}

// ---------------------------------------------------------------------------
// JjBackend workspace management detection
// ---------------------------------------------------------------------------

#[test]
fn test_jj_backend_workspace_management_op() {
    let backend = JjBackend { colocated: false };
    assert!(backend.is_workspace_management_op("jj workspace add ../foo"));
    assert!(backend.is_workspace_management_op("jj workspace list"));
    assert!(backend.is_workspace_management_op("jj workspace forget default"));
    assert!(!backend.is_workspace_management_op("jj log"));
    assert!(!backend.is_workspace_management_op("jj status"));
}

// ---------------------------------------------------------------------------
// JjBackend is_repo
// ---------------------------------------------------------------------------

#[test]
fn test_jj_backend_is_repo() {
    let dir = create_temp_dir("jj-is-repo");
    let backend = JjBackend { colocated: false };
    assert!(!backend.is_repo(&dir)); // no .jj
    fs::create_dir(dir.join(".jj")).unwrap();
    assert!(backend.is_repo(&dir));
    cleanup(&dir);
}

// ---------------------------------------------------------------------------
// JjBackend extract_repo_from_op
// ---------------------------------------------------------------------------

#[test]
fn test_jj_backend_extract_repo_from_op() {
    let backend = JjBackend { colocated: false };
    assert_eq!(
        backend.extract_repo_from_op("jj -R /home/user/acme-api log"),
        Some("acme-api".to_string())
    );
    assert_eq!(backend.extract_repo_from_op("jj log"), None);
}

// ---------------------------------------------------------------------------
// JjBackend workspace enforcement
// ---------------------------------------------------------------------------

#[test]
fn test_jj_backend_blocks_edit_immutable_when_active() {
    let backend = JjBackend { colocated: false };
    let result = backend.check_workspace_enforcement("jj edit root", true, "abc123");
    assert!(result.is_some(), "should block jj edit root");
    let msg = result.unwrap();
    assert!(msg.contains("immutable"), "msg: {msg}");
}

#[test]
fn test_jj_backend_allows_edit_immutable_when_inactive() {
    let backend = JjBackend { colocated: false };
    let result = backend.check_workspace_enforcement("jj edit root", false, "abc123");
    assert!(result.is_none(), "should allow when workspace not active");
}

#[test]
fn test_jj_backend_allows_normal_edit() {
    let backend = JjBackend { colocated: false };
    let result = backend.check_workspace_enforcement("jj edit abc", true, "abc123");
    assert!(result.is_none(), "should allow edit of normal revision");
}

// ---------------------------------------------------------------------------
// Spec file roundtrip with VCS kind
// ---------------------------------------------------------------------------

#[test]
fn test_spec_roundtrip_with_vcs_kinds() {
    let dir = create_temp_dir("spec-roundtrip");
    let spec_path = dir.join("test.env");

    let entries = vec![
        SpecEntry {
            repo: "web-app".to_string(),
            branch: "main".to_string(),
            wt_path: "/tmp/wt1".to_string(),
            repo_path: "/home/user/web-app".to_string(),
            vcs_kind: VcsKind::Git,
        },
        SpecEntry {
            repo: "jj-repo".to_string(),
            branch: String::new(),
            wt_path: "/tmp/wt2".to_string(),
            repo_path: "/home/user/jj-repo".to_string(),
            vcs_kind: VcsKind::Jj,
        },
        SpecEntry {
            repo: "coloc-repo".to_string(),
            branch: "main".to_string(),
            wt_path: "/tmp/wt3".to_string(),
            repo_path: "/home/user/coloc-repo".to_string(),
            vcs_kind: VcsKind::JjColocated,
        },
    ];

    session::write_spec_file(&spec_path, &entries).unwrap();
    let read_back = session::read_spec_file(&spec_path).unwrap();

    assert_eq!(read_back.len(), 3);
    assert_eq!(read_back[0].vcs_kind, VcsKind::Git);
    assert_eq!(read_back[1].vcs_kind, VcsKind::Jj);
    assert_eq!(read_back[2].vcs_kind, VcsKind::JjColocated);
    assert_eq!(read_back[1].branch, ""); // jj has no branch

    cleanup(&dir);
}

#[test]
fn test_spec_backward_compat_4_field() {
    let dir = create_temp_dir("spec-compat");
    let spec_path = dir.join("legacy.env");

    // Write in old 4-field format manually
    fs::write(&spec_path, "acme-api|main|/tmp/wt|/home/user/acme-api\n").unwrap();

    let entries = session::read_spec_file(&spec_path).unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].vcs_kind, VcsKind::Git); // default for legacy format
    assert_eq!(entries[0].repo, "acme-api");

    cleanup(&dir);
}

#[test]
fn test_spec_roundtrip_preserves_all_fields() {
    let dir = create_temp_dir("spec-fields");
    let spec_path = dir.join("full.env");

    let entries = vec![SpecEntry {
        repo: "acme-api".to_string(),
        branch: "feat/cool".to_string(),
        wt_path: "/tmp/wt-acme".to_string(),
        repo_path: "/home/user/acme-api".to_string(),
        vcs_kind: VcsKind::Jj,
    }];

    session::write_spec_file(&spec_path, &entries).unwrap();
    let read_back = session::read_spec_file(&spec_path).unwrap();

    assert_eq!(read_back.len(), 1);
    assert_eq!(read_back[0].repo, "acme-api");
    assert_eq!(read_back[0].branch, "feat/cool");
    assert_eq!(read_back[0].wt_path, "/tmp/wt-acme");
    assert_eq!(read_back[0].repo_path, "/home/user/acme-api");
    assert_eq!(read_back[0].vcs_kind, VcsKind::Jj);

    cleanup(&dir);
}
