//! Integration tests for muzzle-memory: full lifecycle, cross-project search,
//! topic upsert across sessions, and capture→store→inject roundtrip.

use muzzle_memory::capture;
use muzzle_memory::inject;
use muzzle_memory::store::{NewObservation, Store};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn open_store() -> Store {
    Store::open(":memory:").expect("open in-memory store")
}

fn reg(store: &Store, id: &str, project: &str) {
    store
        .register_session(id, project, "/tmp/test")
        .expect("register session");
}

// A realistic changelog that exercises all mutation categories.
const REALISTIC_CHANGELOG: &str = "\
## Session: 2026-03-16 14:00:00 (abc12345)
`2026-03-16 14:00:01` **Edit**: `memory/src/store.rs`
`2026-03-16 14:00:02` **Write**: `memory/src/capture.rs`
`2026-03-16 14:00:03` **Edit**: `memory/src/store.rs`
`2026-03-16 14:00:04` **COMMIT** `abc1234` on `feature/memory`
`2026-03-16 14:00:05` **PUSH** `origin` `feature/memory` (abc..def) -> `origin/feature/memory`
`2026-03-16 14:00:06` **PR Created**: Acme/muzzle - Add memory crate
`2026-03-16 14:00:07` **mcp__claude_ai_Atlassian__createJiraIssue**
";

// ---------------------------------------------------------------------------
// test_full_lifecycle
// ---------------------------------------------------------------------------

#[test]
fn test_full_lifecycle() {
    // 1. Open in-memory store.
    let mut store = open_store();

    // 2. Register a session.
    reg(&store, "sess-full", "work/backend");

    // 3. Parse a realistic changelog string.
    let summary = capture::parse_changelog(REALISTIC_CHANGELOG);
    assert!(
        !summary.is_empty(),
        "parse_changelog should return non-empty summary"
    );

    // 4. Save the result as a session_summary observation.
    let id1 = store
        .save_observation(NewObservation {
            session_id: "sess-full".to_string(),
            obs_type: "session_summary".to_string(),
            title: "Session summary".to_string(),
            content: summary,
            project: "work/backend".to_string(),
            source: "changelog".to_string(),
            ..Default::default()
        })
        .expect("save session_summary");
    assert!(id1 > 0, "observation id must be positive");

    // 5. Save an agent-driven observation (type=bugfix, with a topic_key).
    let id2 = store
        .save_observation(NewObservation {
            session_id: "sess-full".to_string(),
            obs_type: "bugfix".to_string(),
            title: "Fix payment webhook retry".to_string(),
            content: "Added exponential backoff to payments/webhooks.py".to_string(),
            project: "work/backend".to_string(),
            scope: Some("project".to_string()),
            topic_key: Some("payment-webhook-retry".to_string()),
            source: "agent".to_string(),
        })
        .expect("save bugfix observation");
    assert!(id2 > 0, "bugfix observation id must be positive");

    // 6. Search for a term — verify results found.
    let results = store
        .search("payment", Some("work/backend"), 10)
        .expect("search");
    assert!(
        !results.is_empty(),
        "search for 'payment' should return at least one result"
    );

    // 7. Get recent_context — verify both observations returned.
    let recent = store
        .recent_context("work/backend", 10)
        .expect("recent_context");
    assert_eq!(
        recent.len(),
        2,
        "recent_context should return both observations, got {}",
        recent.len()
    );

    // 8. Get stats — verify counts (1 session, 2 observations).
    let stats = store.stats().expect("stats");
    assert_eq!(stats.total_sessions, 1, "expected 1 session");
    assert_eq!(stats.total_observations, 2, "expected 2 observations");

    // 9. Format context via inject::format_context() — verify non-empty markdown.
    let markdown = inject::format_context(&recent, "work/backend");
    assert!(
        !markdown.is_empty(),
        "format_context should return non-empty markdown"
    );
    assert!(
        markdown.starts_with("# Session Memory (work/backend)"),
        "markdown should start with the session memory header"
    );
}

// ---------------------------------------------------------------------------
// test_cross_project_search
// ---------------------------------------------------------------------------

#[test]
fn test_cross_project_search() {
    // 1. Open in-memory store.
    let mut store = open_store();

    // 2. Register 2 sessions in different projects.
    reg(&store, "sess-backend", "work/backend");
    reg(&store, "sess-infra", "work/infra");

    // 3. Save one observation per project.
    store
        .save_observation(NewObservation {
            session_id: "sess-backend".to_string(),
            obs_type: "learning".to_string(),
            title: "Deploy pipeline for backend".to_string(),
            content: "cloud deploy rolling deploy details".to_string(),
            project: "work/backend".to_string(),
            source: "agent".to_string(),
            ..Default::default()
        })
        .expect("save backend observation");

    store
        .save_observation(NewObservation {
            session_id: "sess-infra".to_string(),
            obs_type: "learning".to_string(),
            title: "Deploy pipeline for infra".to_string(),
            content: "Terraform apply deploy steps".to_string(),
            project: "work/infra".to_string(),
            source: "agent".to_string(),
            ..Default::default()
        })
        .expect("save ops observation");

    // 4. Unscoped search — returns both.
    let all = store.search("deploy", None, 10).expect("unscoped search");
    assert_eq!(
        all.len(),
        2,
        "unscoped search for 'deploy' should return 2 results, got {}",
        all.len()
    );

    // 5. Project-scoped search — returns only matching project.
    let backend_only = store
        .search("deploy", Some("work/backend"), 10)
        .expect("scoped search");
    assert_eq!(
        backend_only.len(),
        1,
        "scoped search should return 1 result for work/backend, got {}",
        backend_only.len()
    );

    // 6. Verify content matches expected project.
    assert_eq!(
        backend_only[0].project, "work/backend",
        "result project should be work/backend"
    );
    assert!(
        backend_only[0].content.contains("cloud deploy"),
        "content should be from backend observation"
    );
}

// ---------------------------------------------------------------------------
// test_topic_key_across_sessions
// ---------------------------------------------------------------------------

#[test]
fn test_topic_key_across_sessions() {
    // 1. Open in-memory store.
    let mut store = open_store();

    // 2. Register 2 sessions for the same project.
    reg(&store, "sess-a", "work/backend");
    reg(&store, "sess-b", "work/backend");

    // 3. Save observation with topic_key in session 1.
    let id1 = store
        .save_observation(NewObservation {
            session_id: "sess-a".to_string(),
            obs_type: "learning".to_string(),
            title: "WAF rules v1".to_string(),
            content: "Initial WAF findings from session A".to_string(),
            project: "work/backend".to_string(),
            scope: Some("project".to_string()),
            topic_key: Some("waf-rules".to_string()),
            source: "agent".to_string(),
        })
        .expect("save obs session 1");

    // 4. Save observation with SAME topic_key in session 2.
    let id2 = store
        .save_observation(NewObservation {
            session_id: "sess-b".to_string(),
            obs_type: "learning".to_string(),
            title: "WAF rules v2".to_string(),
            content: "Updated WAF findings from session B".to_string(),
            project: "work/backend".to_string(),
            scope: Some("project".to_string()),
            topic_key: Some("waf-rules".to_string()),
            source: "agent".to_string(),
        })
        .expect("save obs session 2");

    // 5. Verify only 1 observation exists (upsert), revision_count = 2.
    assert_eq!(id1, id2, "same topic_key should upsert the same row");

    let recent = store
        .recent_context("work/backend", 10)
        .expect("recent_context");
    assert_eq!(
        recent.len(),
        1,
        "only 1 observation should exist after upsert, got {}",
        recent.len()
    );
    assert_eq!(
        recent[0].revision_count, 2,
        "revision_count should be 2 after two saves"
    );

    // 6. Verify content is from session 2 (latest wins).
    assert!(
        recent[0].content.contains("session B"),
        "content should be from session B (latest), got: {}",
        recent[0].content
    );
    assert_eq!(
        recent[0].title, "WAF rules v2",
        "title should be from session B"
    );
}

// ---------------------------------------------------------------------------
// test_capture_and_inject_roundtrip
// ---------------------------------------------------------------------------

#[test]
fn test_capture_and_inject_roundtrip() {
    // 1. Parse a changelog string.
    let summary = capture::parse_changelog(REALISTIC_CHANGELOG);
    assert!(!summary.is_empty(), "parse_changelog should produce output");

    // 2. Save to store.
    let mut store = open_store();
    reg(&store, "sess-roundtrip", "work/backend");

    store
        .save_observation(NewObservation {
            session_id: "sess-roundtrip".to_string(),
            obs_type: "session_summary".to_string(),
            title: "Roundtrip session summary".to_string(),
            content: summary,
            project: "work/backend".to_string(),
            source: "changelog".to_string(),
            ..Default::default()
        })
        .expect("save roundtrip observation");

    // 3. Retrieve via recent_context.
    let recent = store
        .recent_context("work/backend", 10)
        .expect("recent_context");
    assert_eq!(recent.len(), 1, "should have exactly 1 observation");

    // 4. Format via format_context.
    let formatted = inject::format_context(&recent, "work/backend");
    assert!(
        !formatted.is_empty(),
        "formatted context should be non-empty"
    );

    // 5. Verify formatted output contains file paths from the original changelog.
    // REALISTIC_CHANGELOG has Edit entries for memory/src/store.rs and
    // memory/src/capture.rs — both should appear in the formatted output
    // (possibly truncated at 150 chars, but the paths are short enough to survive).
    assert!(
        formatted.contains("memory/src/store.rs"),
        "formatted context should mention store.rs; got:\n{formatted}"
    );
    assert!(
        formatted.contains("memory/src/capture.rs"),
        "formatted context should mention capture.rs; got:\n{formatted}"
    );
}
