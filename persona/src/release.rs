//! Session release: unlock personas, update role counts, recompute affinity scores.

use rusqlite::{params, Connection, Result};
use std::collections::HashMap;

// ---------------------------------------------------------------------------
// Sentiment classification
// ---------------------------------------------------------------------------

const POSITIVE_KEYWORDS: &[&str] = &[
    "great",
    "caught",
    "good",
    "solid",
    "excellent",
    "precise",
    "thorough",
    "correct",
    "helpful",
    "fast",
];

const NEGATIVE_KEYWORDS: &[&str] = &[
    "missed",
    "verbose",
    "wrong",
    "slow",
    "confused",
    "irrelevant",
    "shallow",
    "broke",
    "failed",
    "noisy",
];

#[derive(Debug, Clone, PartialEq, Eq)]
enum Sentiment {
    Positive,
    Negative,
    Neutral,
}

fn classify_sentiment(text: &str) -> Sentiment {
    let lower = text.to_lowercase();
    let pos: usize = POSITIVE_KEYWORDS
        .iter()
        .filter(|&&kw| lower.contains(kw))
        .count();
    let neg: usize = NEGATIVE_KEYWORDS
        .iter()
        .filter(|&&kw| lower.contains(kw))
        .count();
    if pos > neg {
        Sentiment::Positive
    } else if neg > pos {
        Sentiment::Negative
    } else {
        Sentiment::Neutral
    }
}

// ---------------------------------------------------------------------------
// Affinity recomputation
// ---------------------------------------------------------------------------

fn recompute_affinity(
    conn: &Connection,
    persona_id: i64,
    role_counts: &HashMap<String, u32>,
) -> Result<HashMap<String, f32>> {
    let total: u32 = role_counts.values().sum();

    // Load feedback for this persona.
    let mut stmt =
        conn.prepare("SELECT role, observation FROM persona_feedback WHERE persona_id = ?1")?;
    let rows = stmt.query_map(params![persona_id], |row| {
        let role: String = row.get(0)?;
        let observation: String = row.get(1)?;
        Ok((role, observation))
    })?;

    // Count sentiments per role.
    let mut positive: HashMap<String, u32> = HashMap::new();
    let mut negative: HashMap<String, u32> = HashMap::new();
    for row in rows {
        let (role, obs) = row?;
        match classify_sentiment(&obs) {
            Sentiment::Positive => *positive.entry(role).or_insert(0) += 1,
            Sentiment::Negative => *negative.entry(role).or_insert(0) += 1,
            Sentiment::Neutral => {}
        }
    }

    let mut scores: HashMap<String, f32> = HashMap::new();
    for (role, &count) in role_counts {
        let raw = if total == 0 {
            0.0_f32
        } else {
            count as f32 / total as f32
        };
        let boost = positive.get(role).copied().unwrap_or(0) as f32 * 0.02;
        let penalty = negative.get(role).copied().unwrap_or(0) as f32 * 0.03;
        let affinity = (raw + boost - penalty).clamp(0.0, 1.0);
        scores.insert(role.clone(), affinity);
    }

    Ok(scores)
}

// ---------------------------------------------------------------------------
// Public release function
// ---------------------------------------------------------------------------

/// Release all active assignments for `session_id`.
///
/// All work is done inside a single `BEGIN IMMEDIATE` transaction:
/// 1. Find unreleased assignments.
/// 2. Increment `role_counts` for each persona.
/// 3. Recompute `affinity_scores` from updated counts + feedback.
/// 4. Clear session locks.
/// 5. Stamp `released_at` on the assignments.
/// 6. Retire personas inactive for > 30 days.
pub fn release(conn: &Connection, session_id: &str) -> Result<()> {
    conn.execute_batch("BEGIN IMMEDIATE")?;

    let result = (|| -> Result<()> {
        // Step 1: find unreleased assignments.
        let mut stmt = conn.prepare(
            "SELECT pa.persona_id, pa.role
               FROM persona_assignments pa
              WHERE pa.session_id = ?1
                AND pa.released_at IS NULL",
        )?;
        let rows = stmt.query_map(params![session_id], |row| {
            let persona_id: i64 = row.get(0)?;
            let role: String = row.get(1)?;
            Ok((persona_id, role))
        })?;

        // Group roles by persona_id.
        let mut persona_roles: HashMap<i64, Vec<String>> = HashMap::new();
        for row in rows {
            let (persona_id, role) = row?;
            persona_roles.entry(persona_id).or_default().push(role);
        }

        // Steps 2 & 3: for each persona, update role_counts and affinity_scores.
        for (&persona_id, roles) in &persona_roles {
            // Load current role_counts.
            let role_counts_json: String = conn.query_row(
                "SELECT role_counts FROM personas WHERE id = ?1",
                params![persona_id],
                |r| r.get(0),
            )?;
            let mut role_counts: HashMap<String, u32> =
                serde_json::from_str(&role_counts_json).unwrap_or_default();

            // Increment counts for each role assigned this session.
            for role in roles {
                *role_counts.entry(role.clone()).or_insert(0) += 1;
            }

            // Recompute affinity scores.
            let affinity_scores = recompute_affinity(conn, persona_id, &role_counts)?;

            let updated_counts_json =
                serde_json::to_string(&role_counts).unwrap_or_else(|_| "{}".into());
            let updated_affinity_json =
                serde_json::to_string(&affinity_scores).unwrap_or_else(|_| "{}".into());

            conn.execute(
                "UPDATE personas
                    SET role_counts = ?1, affinity_scores = ?2
                  WHERE id = ?3",
                params![updated_counts_json, updated_affinity_json, persona_id],
            )?;
        }

        // Step 4: clear session locks.
        conn.execute(
            "UPDATE personas SET assigned_to_session = NULL
              WHERE assigned_to_session = ?1",
            params![session_id],
        )?;

        // Step 5: stamp released_at.
        let now = crate::seed::now_iso8601();
        conn.execute(
            "UPDATE persona_assignments
                SET released_at = ?1
              WHERE session_id = ?2
                AND released_at IS NULL",
            params![now, session_id],
        )?;

        // Step 6: retire long-inactive personas.
        conn.execute(
            "UPDATE personas
                SET status = 'archived'
              WHERE status = 'active'
                AND last_assigned < datetime('now', '-30 days')
                AND assigned_to_session IS NULL",
            [],
        )?;

        Ok(())
    })();

    match result {
        Ok(()) => {
            conn.execute_batch("COMMIT")?;
            Ok(())
        }
        Err(e) => {
            let _ = conn.execute_batch("ROLLBACK");
            Err(e)
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::ensure_schema;
    use crate::seed::{insert_seed, now_iso8601, parse_seed};
    use rusqlite::Connection;

    const SAMPLE_TOML: &str = r#"
[meta]
version = 1
trait_vocabulary = ["pragmatic", "curious", "methodical"]
expertise_vocabulary = ["backend", "security", "testing"]
role_vocabulary = ["code-reviewer", "security-review", "testing", "general"]
first_names = ["Alice", "Bob", "Carol"]
last_names = ["Smith", "Jones", "Davis"]

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
"#;

    fn setup() -> Connection {
        let conn = Connection::open_in_memory().expect("in-memory db");
        ensure_schema(&conn).expect("schema");
        let seed = parse_seed(SAMPLE_TOML).expect("parse seed");
        insert_seed(&conn, &seed).expect("seed");
        conn
    }

    fn lock_persona(conn: &Connection, persona_id: i64, session_id: &str) {
        conn.execute(
            "UPDATE personas SET assigned_to_session = ?1, last_assigned = ?2 WHERE id = ?3",
            params![session_id, now_iso8601(), persona_id],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO persona_assignments
                 (persona_id, session_id, project, role, agent_slot, assigned_at)
             VALUES (?1, ?2, 'test-proj', 'general', 'agent-0', ?3)",
            params![persona_id, session_id, now_iso8601()],
        )
        .unwrap();
    }

    fn first_persona_id(conn: &Connection) -> i64 {
        conn.query_row("SELECT id FROM personas ORDER BY id LIMIT 1", [], |r| {
            r.get(0)
        })
        .unwrap()
    }

    #[test]
    fn release_clears_session_lock() {
        let conn = setup();
        let id = first_persona_id(&conn);
        lock_persona(&conn, id, "sess-r01");

        release(&conn, "sess-r01").expect("release");

        let session: Option<String> = conn
            .query_row(
                "SELECT assigned_to_session FROM personas WHERE id = ?1",
                params![id],
                |r| r.get(0),
            )
            .unwrap();
        assert!(session.is_none(), "session lock should be cleared");
    }

    #[test]
    fn release_updates_role_counts() {
        let conn = setup();
        let id = first_persona_id(&conn);
        lock_persona(&conn, id, "sess-r02");

        release(&conn, "sess-r02").expect("release");

        let role_counts_json: String = conn
            .query_row(
                "SELECT role_counts FROM personas WHERE id = ?1",
                params![id],
                |r| r.get(0),
            )
            .unwrap();
        let role_counts: HashMap<String, u32> = serde_json::from_str(&role_counts_json).unwrap();
        assert_eq!(
            role_counts.get("general").copied().unwrap_or(0),
            1,
            "general count should be 1"
        );
    }

    #[test]
    fn release_sets_released_at() {
        let conn = setup();
        let id = first_persona_id(&conn);
        lock_persona(&conn, id, "sess-r03");

        release(&conn, "sess-r03").expect("release");

        let released_at: Option<String> = conn
            .query_row(
                "SELECT released_at FROM persona_assignments WHERE session_id = 'sess-r03'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(released_at.is_some(), "released_at should be set");
    }

    #[test]
    fn classify_sentiment_positive() {
        assert_eq!(
            classify_sentiment("great work, very helpful and correct"),
            Sentiment::Positive
        );
    }

    #[test]
    fn classify_sentiment_negative() {
        assert_eq!(
            classify_sentiment("missed the point and was verbose and wrong"),
            Sentiment::Negative
        );
    }

    #[test]
    fn classify_sentiment_neutral_on_tie() {
        // One positive keyword, one negative keyword → tie → Neutral.
        assert_eq!(
            classify_sentiment("great but also missed one thing"),
            Sentiment::Neutral
        );
    }

    #[test]
    fn release_recomputes_affinity_scores() {
        let conn = setup();
        let id = first_persona_id(&conn);
        lock_persona(&conn, id, "sess-r04");

        // Add positive feedback for the role.
        conn.execute(
            "INSERT INTO persona_feedback
                 (persona_id, timestamp, project, role, observation, source)
             VALUES (?1, ?2, 'test-proj', 'general', 'great and helpful work', 'session')",
            params![id, now_iso8601()],
        )
        .unwrap();

        release(&conn, "sess-r04").expect("release");

        let affinity_json: String = conn
            .query_row(
                "SELECT affinity_scores FROM personas WHERE id = ?1",
                params![id],
                |r| r.get(0),
            )
            .unwrap();
        let scores: HashMap<String, f32> = serde_json::from_str(&affinity_json).unwrap();
        let score = scores.get("general").copied().unwrap_or(0.0);
        // raw = 1.0 (only one role), boost = 2 * 0.02 = 0.04 → clamped to 1.0
        assert!(score > 0.0, "affinity should be positive after release");
    }

    #[test]
    fn release_is_idempotent_on_empty_session() {
        let conn = setup();
        // Releasing a session with no assignments should not error.
        release(&conn, "sess-nonexistent").expect("release on empty session");
    }
}
