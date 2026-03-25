//! Integration tests for muzzle-persona.

use muzzle_persona::broker;
use muzzle_persona::preamble::{format_preamble, MAX_PREAMBLE_CHARS};
use muzzle_persona::release;
use muzzle_persona::schema::ensure_schema;
use muzzle_persona::seed::{insert_seed, parse_seed};
use rusqlite::{params, Connection};

const SEED_TOML: &str = r#"
[meta]
version = 1
trait_vocabulary = ["pragmatic", "curious", "methodical", "thorough", "creative"]
expertise_vocabulary = ["backend", "security", "testing", "frontend", "infrastructure"]
role_vocabulary = ["code-reviewer", "security-review", "testing", "general", "implementation"]
first_names = ["Alice", "Bob", "Carol", "Dave", "Eve", "Frank", "Grace", "Hank"]
last_names = ["Smith", "Jones", "Davis", "Brown", "Wilson", "Taylor", "Moore", "Clark"]

[[personas]]
name = "Alice Smith"
traits = ["pragmatic", "methodical"]
expertise = ["backend", "performance"]
[personas.role_instructions]
code-reviewer = "Focus on correctness and performance."
general = "Be thorough."

[[personas]]
name = "Bob Jones"
traits = ["curious"]
expertise = ["security"]
[personas.role_instructions]
security-review = "Check for injection vectors."
general = "Be thorough."

[[personas]]
name = "Carol Davis"
traits = ["methodical"]
expertise = ["testing", "backend"]
[personas.role_instructions]
testing = "Aim for full branch coverage."
general = "Be thorough."
"#;

fn setup() -> Connection {
    let conn = Connection::open_in_memory().expect("in-memory db");
    ensure_schema(&conn).expect("schema");
    let seed = parse_seed(SEED_TOML).expect("parse seed");
    insert_seed(&conn, &seed).expect("seed");
    conn
}

fn seed_count() -> usize {
    let seed = parse_seed(SEED_TOML).expect("parse seed");
    seed.personas.len()
}

#[test]
fn full_lifecycle() {
    let conn = setup();

    // Assign 2 roles.
    let assignments = broker::assign(
        &conn,
        &["code-reviewer", "security-review"],
        "acme-api",
        "int-sess-001",
        "agent-x",
        Some("eng-team"),
        None,
    )
    .expect("assign should succeed");

    assert_eq!(assignments.len(), 2);

    // Verify preambles are within the 500-char budget.
    for a in &assignments {
        let preamble = format_preamble(a);
        assert!(
            preamble.chars().count() <= MAX_PREAMBLE_CHARS,
            "preamble for {} exceeds 500 chars",
            a.name
        );
        assert!(preamble.ends_with("---\n"));
    }

    // Verify locks: both assigned personas must be locked to the session.
    for a in &assignments {
        let locked: Option<String> = conn
            .query_row(
                "SELECT assigned_to_session FROM personas WHERE id = ?1",
                params![a.persona_id],
                |r| r.get(0),
            )
            .expect("query locked");
        assert_eq!(
            locked.as_deref(),
            Some("int-sess-001"),
            "persona {} should be locked",
            a.name
        );
    }

    // Release the session.
    release::release(&conn, "int-sess-001").expect("release should succeed");

    // Verify locks cleared.
    for a in &assignments {
        let locked: Option<String> = conn
            .query_row(
                "SELECT assigned_to_session FROM personas WHERE id = ?1",
                params![a.persona_id],
                |r| r.get(0),
            )
            .expect("query after release");
        assert!(
            locked.is_none(),
            "persona {} should have no session lock after release",
            a.name
        );
    }

    // Verify role_counts incremented for both personas.
    for a in &assignments {
        let role_counts_json: String = conn
            .query_row(
                "SELECT role_counts FROM personas WHERE id = ?1",
                params![a.persona_id],
                |r| r.get(0),
            )
            .expect("role_counts query");
        let counts: std::collections::HashMap<String, u32> =
            serde_json::from_str(&role_counts_json).expect("parse role_counts");
        let total: u32 = counts.values().sum();
        assert!(
            total > 0,
            "role_counts should be non-zero after release for {}",
            a.name
        );
    }

    // Verify released_at is set on assignments.
    let unreleased: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM persona_assignments WHERE session_id = 'int-sess-001' AND released_at IS NULL",
            [],
            |r| r.get(0),
        )
        .expect("released_at query");
    assert_eq!(unreleased, 0, "all assignments should have released_at set");
}

#[test]
fn assign_triggers_auto_grow_when_pool_exhausted() {
    let conn = setup();
    let initial_count = seed_count();

    // Assign more roles than there are seed personas. The broker auto-grows on demand.
    let extra_roles = initial_count + 1;
    let roles: Vec<&str> = std::iter::repeat("general").take(extra_roles).collect();

    let assignments = broker::assign(
        &conn,
        &roles,
        "web-app",
        "int-sess-002",
        "agent-y",
        None,
        None,
    )
    .expect("assign with auto-grow should succeed");

    assert_eq!(
        assignments.len(),
        extra_roles,
        "should get one assignment per role even when pool is exhausted"
    );

    // Verify total personas in DB grew beyond the seed count.
    let total: i64 = conn
        .query_row("SELECT COUNT(*) FROM personas", [], |r| r.get(0))
        .expect("count query");
    assert!(
        total > initial_count as i64,
        "total personas ({total}) should exceed seed count ({initial_count}) after auto-grow"
    );
}
