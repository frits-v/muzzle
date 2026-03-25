//! Session ID resolution via PPID walk.
//!
//! FR-SI-1 through FR-SI-5: Single implementation, 3-level PPID walk,
//! no scan fallback (AR-4), per-invocation caching (FR-SI-5).

use crate::config;
use crate::vcs::VcsKind;
use std::cell::RefCell;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Error type for session operations.
#[derive(Debug)]
pub enum SessionError {
    /// Filesystem I/O error.
    Io(io::Error),
    /// Failed to parse session data (PID, spec file, etc.).
    Parse(String),
}

impl std::fmt::Display for SessionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SessionError::Io(e) => write!(f, "I/O error: {}", e),
            SessionError::Parse(msg) => write!(f, "parse error: {}", msg),
        }
    }
}

impl From<io::Error> for SessionError {
    fn from(e: io::Error) -> Self {
        SessionError::Io(e)
    }
}

/// Holds resolved session information. Cached per invocation.
#[derive(Debug, Clone)]
pub struct State {
    /// Full session UUID.
    pub id: String,
    /// First 8 characters of the session ID.
    pub short_id: String,
    /// Per-session temp directory (`.claude-tmp/{session-id}/`).
    pub tmp_dir: PathBuf,
    /// Worktree spec file path (`.claude-worktrees-{session-id}.env`).
    pub spec_file: PathBuf,
    /// Session changelog path (`.claude-changelog-{session-id}.md`).
    pub changelog_path: PathBuf,
    /// True if this session has at least one worktree registered.
    pub worktree_active: bool,
    /// VCS backend type detected for this session's primary workspace.
    pub vcs_kind: VcsKind,
    /// True once resolution has completed (even if no session was found).
    pub resolved: bool,
}

impl State {
    /// Create an empty (unresolved-miss) state.
    fn empty() -> Self {
        Self {
            id: String::new(),
            short_id: String::new(),
            tmp_dir: PathBuf::new(),
            spec_file: PathBuf::new(),
            changelog_path: PathBuf::new(),
            worktree_active: false,
            vcs_kind: VcsKind::Git,
            resolved: true,
        }
    }

    /// Create a fully populated state from a session ID.
    pub fn from_id(session_id: &str) -> Self {
        let spec_path = config::spec_file_path(session_id);
        let (worktree_active, vcs_kind) = match read_spec_file(&spec_path) {
            Ok(entries) if !entries.is_empty() => (true, entries[0].vcs_kind),
            _ => (false, VcsKind::Git),
        };
        Self {
            id: session_id.to_string(),
            short_id: config::short_id(session_id),
            tmp_dir: config::session_tmp_dir(session_id),
            spec_file: spec_path,
            changelog_path: config::changelog_path(session_id),
            worktree_active,
            vcs_kind,
            resolved: true,
        }
    }

    /// Check if this state has a valid session.
    pub fn has_session(&self) -> bool {
        !self.id.is_empty()
    }
}

// Thread-local cache for the resolved session (FR-SI-5).
thread_local! {
    static CACHED: RefCell<Option<State>> = const { RefCell::new(None) };
}

/// Type for the PPID resolution function (injectable for testing).
pub type ParentPidFn = fn(u32) -> Result<u32, SessionError>;

/// Default PPID resolver: shells out to `ps`.
pub fn get_parent_pid_via_ps(pid: u32) -> Result<u32, SessionError> {
    let output = Command::new("ps")
        .args(["-o", "ppid=", "-p", &pid.to_string()])
        .output()
        .map_err(|e| SessionError::Parse(format!("ps failed for pid {}: {}", pid, e)))?;

    let ppid_str = String::from_utf8_lossy(&output.stdout).trim().to_string();
    ppid_str
        .parse::<u32>()
        .map_err(|e| SessionError::Parse(format!("parse ppid {:?}: {}", ppid_str, e)))
}

/// Resolve the current session by walking the PPID chain.
///
/// Checks up to `PPID_WALK_DEPTH` ancestor PIDs for a marker file.
/// On cache hit at a non-immediate ancestor, writes a marker at the
/// immediate PPID for faster future lookups.
/// Result is cached for the lifetime of the process (FR-SI-5).
pub fn resolve() -> State {
    resolve_inner(get_parent_pid_via_ps, false)
}

/// Resolve the current session (read-only variant).
///
/// Same PPID walk as `resolve()` but NEVER writes the cache marker.
/// Use this from PreToolUse (H-4: must be pure / no side effects).
pub fn resolve_readonly() -> State {
    resolve_inner(get_parent_pid_via_ps, true)
}

/// Resolve with a custom parent-PID function (for testing).
pub fn resolve_with_fn(get_ppid: ParentPidFn) -> State {
    resolve_inner(get_ppid, false)
}

/// Resolve with a custom parent-PID function, read-only (for testing).
pub fn resolve_readonly_with_fn(get_ppid: ParentPidFn) -> State {
    resolve_inner(get_ppid, true)
}

/// Internal resolver: walks the PPID chain looking for a session marker.
/// When `readonly` is true, skips writing the shortcut marker at the
/// immediate PPID (H-4 compliance for PreToolUse).
fn resolve_inner(get_ppid: ParentPidFn, readonly: bool) -> State {
    // Check cache first
    let cached = CACHED.with(|c| c.borrow().clone());
    if let Some(state) = cached {
        return state;
    }

    let mut state = State::empty();

    let mut pid = std::os::unix::process::parent_id();
    let marker_dir = config::pid_marker_dir_path();

    for _ in 0..config::PPID_WALK_DEPTH {
        if pid <= 1 {
            break;
        }

        let marker_path = marker_dir.join(pid.to_string());
        if let Ok(data) = fs::read_to_string(&marker_path) {
            let sid = data.trim().to_string();
            if !sid.is_empty() {
                state = State::from_id(&sid);

                // Cache at immediate PPID for faster future lookups
                // (only in read-write mode — H-4 forbids writes in PreToolUse)
                if !readonly {
                    let my_ppid = std::os::unix::process::parent_id();
                    if pid != my_ppid {
                        let _ = fs::write(marker_dir.join(my_ppid.to_string()), &sid);
                    }
                }
                break;
            }
        }

        // Walk to parent's parent
        match get_ppid(pid) {
            Ok(parent_pid) => pid = parent_pid,
            Err(_) => break,
        }
    }

    // Cache the result
    CACHED.with(|c| *c.borrow_mut() = Some(state.clone()));
    state
}

/// Create a State from a known session ID (used by session-start and session-end
/// which receive the ID directly via JSON input).
pub fn resolve_with_id(session_id: &str) -> State {
    let state = State::from_id(session_id);
    CACHED.with(|c| *c.borrow_mut() = Some(state.clone()));
    state
}

/// Clear the cached state (for testing).
pub fn reset_cache() {
    CACHED.with(|c| *c.borrow_mut() = None);
}

/// Register a PID marker file.
pub fn register_pid(session_id: &str) -> Result<(), SessionError> {
    let dir = config::pid_marker_dir_path();
    fs::create_dir_all(&dir)?;

    let ppid = std::os::unix::process::parent_id();
    let marker_path = dir.join(ppid.to_string());
    fs::write(marker_path, session_id)?;
    Ok(())
}

/// One line in the worktree spec file.
#[derive(Debug, Clone, PartialEq)]
pub struct SpecEntry {
    /// Repository name (e.g. "my-app").
    pub repo: String,
    /// Git branch checked out in the worktree.
    pub branch: String,
    /// Absolute path to the worktree directory.
    pub wt_path: String,
    /// Absolute path to the original repo root.
    pub repo_path: String,
    /// VCS backend type for this workspace entry.
    pub vcs_kind: VcsKind,
}

/// Read and parse the worktree spec file.
pub fn read_spec_file(path: &Path) -> Result<Vec<SpecEntry>, SessionError> {
    let data = fs::read_to_string(path)?;
    let mut entries = Vec::new();

    for line in data.trim().lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let parts: Vec<&str> = line.splitn(5, '|').collect();
        match parts.len() {
            4 => entries.push(SpecEntry {
                repo: parts[0].to_string(),
                branch: parts[1].to_string(),
                wt_path: parts[2].to_string(),
                repo_path: parts[3].to_string(),
                vcs_kind: VcsKind::Git, // backward compat
            }),
            5 => {
                let vcs_kind = parts[4].parse::<VcsKind>().unwrap_or_default();
                entries.push(SpecEntry {
                    repo: parts[0].to_string(),
                    branch: parts[1].to_string(),
                    wt_path: parts[2].to_string(),
                    repo_path: parts[3].to_string(),
                    vcs_kind,
                });
            }
            _ => continue,
        }
    }
    Ok(entries)
}

/// Write worktree entries to the spec file atomically (tempfile + rename).
pub fn write_spec_file(path: &Path, entries: &[SpecEntry]) -> Result<(), SessionError> {
    let content: String = entries
        .iter()
        .map(|e| {
            format!(
                "{}|{}|{}|{}|{}",
                e.repo, e.branch, e.wt_path, e.repo_path, e.vcs_kind
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";

    let tmp_path = path.with_extension("env.tmp");
    fs::write(&tmp_path, &content)?;

    if let Err(e) = fs::rename(&tmp_path, path) {
        let _ = fs::remove_file(&tmp_path);
        return Err(SessionError::Io(e));
    }
    Ok(())
}

/// Append a spec entry to the spec file (idempotent, flock-protected).
///
/// If the repo already has an entry, skips it. Otherwise appends the new entry.
/// Uses POSIX flock(LOCK_EX) to prevent concurrent ensure-worktree calls from
/// losing each other's writes (read-modify-write race).
pub fn append_spec_entry(path: &Path, entry: &SpecEntry) -> Result<(), SessionError> {
    let lock_path = path.with_extension("lock");
    let lock_file = fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(&lock_path)
        .map_err(SessionError::Io)?;

    flock_exclusive(&lock_file)?;

    let result = append_spec_entry_inner(path, entry);

    // Unlock (also happens on drop, but explicit is clearer)
    flock_unlock(&lock_file);
    result
}

fn append_spec_entry_inner(path: &Path, entry: &SpecEntry) -> Result<(), SessionError> {
    let mut entries = match read_spec_file(path) {
        Ok(e) => e,
        Err(SessionError::Io(ref io_err)) if io_err.kind() == io::ErrorKind::NotFound => Vec::new(),
        Err(e) => return Err(e),
    };

    // Idempotent: skip if repo already present
    if entries.iter().any(|e| e.repo == entry.repo) {
        return Ok(());
    }

    entries.push(entry.clone());
    write_spec_file(path, &entries)
}

/// Acquire an exclusive POSIX flock on the given file.
fn flock_exclusive(file: &fs::File) -> Result<(), SessionError> {
    use std::os::unix::io::AsRawFd;
    let fd = file.as_raw_fd();
    let ret = unsafe { libc::flock(fd, libc::LOCK_EX) };
    if ret != 0 {
        return Err(SessionError::Io(io::Error::last_os_error()));
    }
    Ok(())
}

/// Release a POSIX flock on the given file.
fn flock_unlock(file: &fs::File) {
    use std::os::unix::io::AsRawFd;
    let fd = file.as_raw_fd();
    unsafe {
        libc::flock(fd, libc::LOCK_UN);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ENV_LOCK;
    use std::sync::Mutex;

    // Serialize session tests to avoid PPID marker conflicts.
    static SESSION_LOCK: Mutex<()> = Mutex::new(());

    /// Check if the spec file exists and has content (test-only helper).
    fn spec_file_has_content(path: &Path) -> bool {
        fs::metadata(path).map(|m| m.len() > 0).unwrap_or(false)
    }

    #[test]
    fn test_resolve_with_id() {
        let _env = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let _lock = SESSION_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        reset_cache();

        let sess = resolve_with_id("abc12345-6789-0000-1111-222233334444");

        assert_eq!(sess.id, "abc12345-6789-0000-1111-222233334444");
        assert_eq!(sess.short_id, "abc12345");
        assert_eq!(
            sess.tmp_dir,
            config::session_tmp_dir("abc12345-6789-0000-1111-222233334444")
        );
        assert!(sess.resolved);

        reset_cache();
    }

    #[test]
    fn test_resolve_ppid_walk_success() {
        let _lock = SESSION_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        reset_cache();

        // Create a PID marker for our own PPID
        let dir = config::pid_marker_dir_path();
        let _ = fs::create_dir_all(&dir);

        let ppid = std::os::unix::process::parent_id();
        let marker_path = dir.join(ppid.to_string());

        // Save existing marker to restore later
        let existing = fs::read_to_string(&marker_path).ok();

        let expected_sid = "test-session-ppid-walk";
        let _ = fs::write(&marker_path, expected_sid);

        let sess = resolve();

        // Restore marker before asserting (in case of panic)
        if let Some(data) = &existing {
            let _ = fs::write(&marker_path, data);
        } else {
            let _ = fs::remove_file(&marker_path);
        }

        assert_eq!(sess.id, expected_sid);
        assert!(sess.resolved);

        reset_cache();
    }

    #[test]
    fn test_resolve_ppid_miss() {
        let _lock = SESSION_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        reset_cache();

        // Remove our direct PPID marker if any
        let dir = config::pid_marker_dir_path();
        let ppid = std::os::unix::process::parent_id();
        let marker_path = dir.join(ppid.to_string());

        // Save existing marker
        let existing = fs::read_to_string(&marker_path).ok();
        let _ = fs::remove_file(&marker_path);

        // Override PPID walk to return PIDs with no markers
        fn fake_ppid(_pid: u32) -> Result<u32, SessionError> {
            Ok(999_999_999)
        }

        let sess = resolve_with_fn(fake_ppid);

        // Restore marker
        if let Some(data) = &existing {
            let _ = fs::write(&marker_path, data);
        }

        assert!(
            sess.id.is_empty(),
            "expected empty session ID on miss, got {:?}",
            sess.id
        );
        assert!(sess.resolved);

        reset_cache();
    }

    #[test]
    fn test_resolve_no_marker_dir() {
        let _lock = SESSION_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        reset_cache();

        // Remove our direct PPID marker
        let dir = config::pid_marker_dir_path();
        let ppid = std::os::unix::process::parent_id();
        let marker_path = dir.join(ppid.to_string());
        let existing = fs::read_to_string(&marker_path).ok();
        let _ = fs::remove_file(&marker_path);

        fn failing_ppid(_pid: u32) -> Result<u32, SessionError> {
            Err(SessionError::Parse("no such process".into()))
        }

        let sess = resolve_with_fn(failing_ppid);

        // Restore marker
        if let Some(data) = &existing {
            let _ = fs::write(&marker_path, data);
        }

        assert!(sess.id.is_empty());

        reset_cache();
    }

    #[test]
    fn test_spec_file_read_write() {
        let tmp = std::env::temp_dir().join("muzzle-test-spec");
        let _ = fs::create_dir_all(&tmp);
        let spec_path = tmp.join("test.env");

        let entries = vec![
            SpecEntry {
                repo: "web-app".into(),
                branch: "wt/abc12345".into(),
                wt_path: "/path/to/wt".into(),
                repo_path: "/path/to/repo".into(),
                vcs_kind: VcsKind::Git,
            },
            SpecEntry {
                repo: "api-server".into(),
                branch: "feature/test".into(),
                wt_path: "/path/to/wt2".into(),
                repo_path: "/path/to/repo2".into(),
                vcs_kind: VcsKind::Git,
            },
        ];

        write_spec_file(&spec_path, &entries).expect("write failed");
        let read_entries = read_spec_file(&spec_path).expect("read failed");

        assert_eq!(read_entries.len(), 2);
        assert_eq!(read_entries[0].repo, "web-app");
        assert_eq!(read_entries[0].branch, "wt/abc12345");
        assert_eq!(read_entries[1].repo, "api-server");

        let _ = fs::remove_file(&spec_path);
        let _ = fs::remove_dir(&tmp);
    }

    #[test]
    fn test_append_spec_entry_to_empty() {
        let tmp = std::env::temp_dir().join("muzzle-test-append");
        let _ = fs::create_dir_all(&tmp);
        let spec_path = tmp.join("append-empty.env");
        let _ = fs::remove_file(&spec_path); // ensure clean state

        let entry = SpecEntry {
            repo: "ops".into(),
            branch: "wt/abc12345".into(),
            wt_path: "/path/to/ops/wt".into(),
            repo_path: "/path/to/ops".into(),
            vcs_kind: VcsKind::Git,
        };

        append_spec_entry(&spec_path, &entry).expect("append to empty failed");

        let entries = read_spec_file(&spec_path).expect("read failed");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].repo, "ops");

        let _ = fs::remove_file(&spec_path);
        let _ = fs::remove_file(spec_path.with_extension("lock"));
        let _ = fs::remove_dir(&tmp);
    }

    #[test]
    fn test_append_spec_entry_to_existing() {
        let tmp = std::env::temp_dir().join("muzzle-test-append2");
        let _ = fs::create_dir_all(&tmp);
        let spec_path = tmp.join("append-existing.env");

        // Write initial entry
        let entries = vec![SpecEntry {
            repo: "web-app".into(),
            branch: "wt/abc12345".into(),
            wt_path: "/path/to/web-app/wt".into(),
            repo_path: "/path/to/web-app".into(),
            vcs_kind: VcsKind::Git,
        }];
        write_spec_file(&spec_path, &entries).expect("initial write failed");

        // Append new entry
        let new_entry = SpecEntry {
            repo: "ops".into(),
            branch: "wt/abc12345".into(),
            wt_path: "/path/to/ops/wt".into(),
            repo_path: "/path/to/ops".into(),
            vcs_kind: VcsKind::Git,
        };
        append_spec_entry(&spec_path, &new_entry).expect("append failed");

        let result = read_spec_file(&spec_path).expect("read failed");
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].repo, "web-app");
        assert_eq!(result[1].repo, "ops");

        let _ = fs::remove_file(&spec_path);
        let _ = fs::remove_file(spec_path.with_extension("lock"));
        let _ = fs::remove_dir(&tmp);
    }

    #[test]
    fn test_append_spec_entry_idempotent() {
        let tmp = std::env::temp_dir().join("muzzle-test-append3");
        let _ = fs::create_dir_all(&tmp);
        let spec_path = tmp.join("append-idem.env");

        let entry = SpecEntry {
            repo: "ops".into(),
            branch: "wt/abc12345".into(),
            wt_path: "/path/to/ops/wt".into(),
            repo_path: "/path/to/ops".into(),
            vcs_kind: VcsKind::Git,
        };

        append_spec_entry(&spec_path, &entry).expect("first append failed");
        append_spec_entry(&spec_path, &entry).expect("second append failed");

        let result = read_spec_file(&spec_path).expect("read failed");
        assert_eq!(result.len(), 1, "idempotent append should not duplicate");

        let _ = fs::remove_file(&spec_path);
        let _ = fs::remove_file(spec_path.with_extension("lock"));
        let _ = fs::remove_dir(&tmp);
    }

    #[test]
    fn test_resolve_readonly_no_marker_write() {
        let _lock = SESSION_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        reset_cache();

        // Create a marker NOT at our immediate PPID but at a "grandparent" PID.
        // resolve_readonly should find the session but NOT create a shortcut marker
        // at our immediate PPID.
        let dir = config::pid_marker_dir_path();
        let _ = fs::create_dir_all(&dir);

        let my_ppid = std::os::unix::process::parent_id();
        let marker_path = dir.join(my_ppid.to_string());

        // Save and remove existing marker at our PPID
        let existing = fs::read_to_string(&marker_path).ok();
        let _ = fs::remove_file(&marker_path);

        // Create marker at a fake grandparent PID
        let fake_grandparent: u32 = 999_888_777;
        let grandparent_marker = dir.join(fake_grandparent.to_string());
        let expected_sid = "test-readonly-session";
        let _ = fs::write(&grandparent_marker, expected_sid);

        // Our fake ppid resolver returns the grandparent PID
        fn fake_ppid(_pid: u32) -> Result<u32, SessionError> {
            Ok(999_888_777)
        }

        let sess = resolve_readonly_with_fn(fake_ppid);

        // Session should be resolved
        assert_eq!(sess.id, expected_sid);

        // BUT our immediate PPID marker should NOT have been created
        let ppid_marker_exists = fs::read_to_string(&marker_path).is_ok();
        assert!(
            !ppid_marker_exists,
            "resolve_readonly should NOT write a shortcut marker at the immediate PPID"
        );

        // Cleanup
        let _ = fs::remove_file(&grandparent_marker);
        if let Some(data) = &existing {
            let _ = fs::write(&marker_path, data);
        }

        reset_cache();
    }

    #[test]
    fn test_resolve_readwrite_creates_marker() {
        let _env = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let _lock = SESSION_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        reset_cache();

        // Same setup as above but use the read-write variant
        let dir = config::pid_marker_dir_path();
        let _ = fs::create_dir_all(&dir);

        let my_ppid = std::os::unix::process::parent_id();
        let marker_path = dir.join(my_ppid.to_string());

        // Save and remove existing marker at our PPID
        let existing = fs::read_to_string(&marker_path).ok();
        let _ = fs::remove_file(&marker_path);

        // Create marker at a fake grandparent PID
        let fake_grandparent: u32 = 999_888_776;
        let grandparent_marker = dir.join(fake_grandparent.to_string());
        let expected_sid = "test-readwrite-session";
        let _ = fs::write(&grandparent_marker, expected_sid);

        fn fake_ppid(_pid: u32) -> Result<u32, SessionError> {
            Ok(999_888_776)
        }

        let sess = resolve_with_fn(fake_ppid);

        // Session should be resolved
        assert_eq!(sess.id, expected_sid);

        // The read-write variant SHOULD write a shortcut marker at our immediate PPID
        let ppid_marker_data = fs::read_to_string(&marker_path).ok();
        assert_eq!(
            ppid_marker_data.as_deref(),
            Some(expected_sid),
            "resolve (read-write) should write a shortcut marker at the immediate PPID"
        );

        // Cleanup
        let _ = fs::remove_file(&grandparent_marker);
        if let Some(data) = &existing {
            let _ = fs::write(&marker_path, data);
        } else {
            let _ = fs::remove_file(&marker_path);
        }

        reset_cache();
    }

    #[test]
    fn test_state_has_session() {
        let empty = State::empty();
        assert!(!empty.has_session(), "empty state should not have session");

        let populated = State::from_id("test-has-session");
        assert!(
            populated.has_session(),
            "populated state should have session"
        );
    }

    #[test]
    fn test_read_spec_file_empty() {
        let tmp = std::env::temp_dir().join("muzzle-test-spec-empty");
        let _ = fs::create_dir_all(&tmp);
        let spec_path = tmp.join("empty.env");
        fs::write(&spec_path, "").expect("write failed");

        let entries = read_spec_file(&spec_path).expect("read failed");
        assert!(entries.is_empty(), "empty file should yield zero entries");

        let _ = fs::remove_file(&spec_path);
        let _ = fs::remove_dir(&tmp);
    }

    #[test]
    fn test_read_spec_file_whitespace_only() {
        let tmp = std::env::temp_dir().join("muzzle-test-spec-ws");
        let _ = fs::create_dir_all(&tmp);
        let spec_path = tmp.join("whitespace.env");
        fs::write(&spec_path, "  \n\n  \n").expect("write failed");

        let entries = read_spec_file(&spec_path).expect("read failed");
        assert!(
            entries.is_empty(),
            "whitespace-only file should yield zero entries"
        );

        let _ = fs::remove_file(&spec_path);
        let _ = fs::remove_dir(&tmp);
    }

    #[test]
    fn test_read_spec_file_malformed_lines() {
        let tmp = std::env::temp_dir().join("muzzle-test-spec-mal");
        let _ = fs::create_dir_all(&tmp);
        let spec_path = tmp.join("malformed.env");

        // Mix of valid (4 pipe-parts) and invalid lines
        let content = "valid-repo|branch|/wt/path|/repo/path\n\
                        only-two|parts\n\
                        three|pipe|parts\n\
                        also-valid|b2|/wt2|/repo2\n";
        fs::write(&spec_path, content).expect("write failed");

        let entries = read_spec_file(&spec_path).expect("read failed");
        assert_eq!(entries.len(), 2, "should skip malformed lines");
        assert_eq!(entries[0].repo, "valid-repo");
        assert_eq!(entries[1].repo, "also-valid");

        let _ = fs::remove_file(&spec_path);
        let _ = fs::remove_dir(&tmp);
    }

    #[test]
    fn test_read_spec_file_not_found() {
        let result = read_spec_file(Path::new("/tmp/muzzle-nonexistent-spec-file.env"));
        assert!(result.is_err(), "reading nonexistent file should error");
    }

    #[test]
    fn test_spec_file_has_content_empty() {
        let tmp = std::env::temp_dir().join("muzzle-test-has-content");
        let _ = fs::create_dir_all(&tmp);

        // Empty file
        let empty_path = tmp.join("empty.env");
        fs::write(&empty_path, "").expect("write failed");
        assert!(!spec_file_has_content(&empty_path), "empty file → false");

        // Non-empty file
        let nonempty_path = tmp.join("nonempty.env");
        fs::write(&nonempty_path, "data").expect("write failed");
        assert!(
            spec_file_has_content(&nonempty_path),
            "non-empty file → true"
        );

        // Missing file
        assert!(
            !spec_file_has_content(Path::new("/tmp/muzzle-no-such-file")),
            "missing file → false"
        );

        let _ = fs::remove_file(&empty_path);
        let _ = fs::remove_file(&nonempty_path);
        let _ = fs::remove_dir(&tmp);
    }

    #[test]
    fn test_session_error_display() {
        let io_err = SessionError::Io(io::Error::new(io::ErrorKind::NotFound, "gone"));
        assert!(format!("{}", io_err).contains("I/O error"));

        let parse_err = SessionError::Parse("bad data".into());
        assert!(format!("{}", parse_err).contains("parse error"));
        assert!(format!("{}", parse_err).contains("bad data"));
    }

    #[test]
    fn test_register_pid() {
        let _lock = SESSION_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        reset_cache();

        let dir = config::pid_marker_dir_path();
        let _ = fs::create_dir_all(&dir);

        let ppid = std::os::unix::process::parent_id();
        let marker_path = dir.join(ppid.to_string());

        // Save existing marker
        let existing = fs::read_to_string(&marker_path).ok();

        register_pid("test-register-session").expect("register failed");

        let data = fs::read_to_string(&marker_path).expect("read marker failed");

        // Restore marker
        if let Some(orig) = &existing {
            let _ = fs::write(&marker_path, orig);
        } else {
            let _ = fs::remove_file(&marker_path);
        }

        assert_eq!(data, "test-register-session");

        reset_cache();
    }

    #[test]
    fn test_spec_entry_5_field_roundtrip() {
        let tmp = std::env::temp_dir().join("muzzle-test-spec-5field");
        let _ = fs::create_dir_all(&tmp);
        let spec_path = tmp.join("roundtrip.env");

        let entries = vec![
            SpecEntry {
                repo: "acme-api".into(),
                branch: "wt/abc12345".into(),
                wt_path: "/path/to/wt".into(),
                repo_path: "/path/to/repo".into(),
                vcs_kind: VcsKind::Git,
            },
            SpecEntry {
                repo: "web-app".into(),
                branch: "wt/def67890".into(),
                wt_path: "/path/to/wt2".into(),
                repo_path: "/path/to/repo2".into(),
                vcs_kind: VcsKind::Jj,
            },
            SpecEntry {
                repo: "infra".into(),
                branch: "wt/ghi11111".into(),
                wt_path: "/path/to/wt3".into(),
                repo_path: "/path/to/repo3".into(),
                vcs_kind: VcsKind::JjColocated,
            },
        ];

        write_spec_file(&spec_path, &entries).expect("write failed");
        let read_entries = read_spec_file(&spec_path).expect("read failed");

        assert_eq!(read_entries.len(), 3);
        assert_eq!(read_entries[0].vcs_kind, VcsKind::Git);
        assert_eq!(read_entries[1].vcs_kind, VcsKind::Jj);
        assert_eq!(read_entries[2].vcs_kind, VcsKind::JjColocated);
        assert_eq!(read_entries[0].repo, "acme-api");
        assert_eq!(read_entries[1].repo, "web-app");
        assert_eq!(read_entries[2].repo, "infra");

        let _ = fs::remove_file(&spec_path);
        let _ = fs::remove_dir(&tmp);
    }

    #[test]
    fn test_spec_entry_4_field_backward_compat() {
        let tmp = std::env::temp_dir().join("muzzle-test-spec-4field");
        let _ = fs::create_dir_all(&tmp);
        let spec_path = tmp.join("legacy.env");

        // Write legacy 4-field format manually (no vcs_kind column)
        let content = "acme-api|main|/path/to/wt|/path/to/repo\n\
                        web-app|develop|/path/to/wt2|/path/to/repo2\n";
        fs::write(&spec_path, content).expect("write failed");

        let entries = read_spec_file(&spec_path).expect("read failed");
        assert_eq!(entries.len(), 2);
        assert_eq!(
            entries[0].vcs_kind,
            VcsKind::Git,
            "legacy 4-field entry should default to Git"
        );
        assert_eq!(
            entries[1].vcs_kind,
            VcsKind::Git,
            "legacy 4-field entry should default to Git"
        );
        assert_eq!(entries[0].repo, "acme-api");
        assert_eq!(entries[1].repo, "web-app");

        let _ = fs::remove_file(&spec_path);
        let _ = fs::remove_dir(&tmp);
    }
}
