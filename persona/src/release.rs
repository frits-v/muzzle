//! Session release: increment role counts, recompute affinity scores, unlock personas.

use rusqlite::{Connection, Result};
use std::collections::HashMap;

use crate::seed::now_iso8601;

// ---------------------------------------------------------------------------
// Keyword lists for feedback sentiment
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

// ---------------------------------------------------------------------------
// Sentiment classification
// ---------------------------------------------------------------------------

#[derive(Debug, PartialEq)]
enum Sentiment {
    Positive,
    Negative,
    Neutral,
}

fn classify_sentiment(observation: &str) -> Sentiment {
    let lower = observation.to_lowercase();
    let pos_hits = POSITIVE_KEYWORDS
        .iter()
        .filter(|&&kw| lower.contains(kw))
        .count();
    let neg_hits = NEGATIVE_KEYWORDS
        .iter()
        .filter(|&&kw| lower.contains(kw))
        .count();

    if pos_hits > neg_hits {
        Sentiment::Positive
    } else if neg_hits > pos_hits {
        Sentiment::Negative
    } else {
        Sentiment::Neutral
    }
}

// ---------------------------------------------------------------------------
// Affinity recomputation
// ---------------------------------------------------------------------------

fn recompute_affinity(
    role_counts: &HashMap<String, u32>,
    feedback: &[(String, String)], // (role, observation)
) -> HashMap<String, f32> {
    let total_assignments: u32 = role_counts.values().sum();

    let mut affinities = HashMap::new();

    for (role, &count) in role_counts {
        let raw = count as f32 / total_assignments.max(1) as f32;

        let pos_count = feedback
            .iter()
            .filter(|(fb_role, obs)| {
                fb_role == role && classify_sentiment(obs) == Sentiment::Positive
            })
            .count() as f32;

        let neg_count = feedback
            .iter()
            .filter(|(fb_role, obs)| {
                fb_role == role && classify_sentiment(obs) == Sentiment::Negative
            })
            .count() as f32;

        let feedback_boost = pos_count * 0.02;
        let feedback_penalty = neg_count * 0.03;

        let affinity = (raw + feedback_boost - feedback_penalty).clamp(0.0, 1.0);
        affinities.insert(role.clone(), affinity);
    }

    affinities
}

// ---------------------------------------------------------------------------
// JSON conversion helpers
// ---------------------------------------------------------------------------

fn parse_role_counts(json: &str) -> Result<HashMap<String, u32>> {
    serde_json::from_str(json).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
    })
}

fn serialize_map<V: serde::Serialize>(map: &HashMap<String, V>) -> Result<String> {
    serde_json::to_string(map).map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))
}

// ---------------------------------------------------------------------------
// Public release function
// ---------------------------------------------------------------------------

/// Release all personas held by `session_id`.
///
/// In a single transaction:
/// 1. Find all non-released assignments for the session.
/// 2. Increment `role_counts[role]` for each persona.
/// 3. Recompute `affinity_scores` from role_counts + feedback.
/// 4. Clear `assigned_to_session` for all locked personas.
/// 5. Set `released_at` on the assignment records.
/// 6. Archive personas inactive for >30 days.
pub fn release(conn: &Connection, session_id: &str) -> Result<()> {
    conn.execute_batch("BEGIN IMMEDIATE")?;

    let result = release_inner(conn, session_id);

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

fn release_inner(conn: &Connection, session_id: &str) -> Result<()> {
    let now = now_iso8601();

    // Step 1: Find all non-released assignments for this session.
    let assignments = {
        let mut stmt = conn.prepare(
            "SELECT pa.persona_id, pa.role
             FROM persona_assignments pa
             WHERE pa.session_id = ?1 AND pa.released_at IS NULL",
        )?;
        let rows = stmt
            .query_map([session_id], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
            })?
            .collect::<Result<Vec<_>>>()?;
        rows
    };

    // Step 2 & 3: For each persona, update role_counts and recompute affinity_scores.
    // Group by persona_id to handle multiple roles per session.
    let mut persona_roles: HashMap<i64, Vec<String>> = HashMap::new();
    for (persona_id, role) in &assignments {
        persona_roles
            .entry(*persona_id)
            .or_default()
            .push(role.clone());
    }

    for (persona_id, roles) in &persona_roles {
        // Load current role_counts.
        let role_counts_json: String = conn.query_row(
            "SELECT role_counts FROM personas WHERE id = ?1",
            [persona_id],
            |row| row.get(0),
        )?;
        let mut role_counts = parse_role_counts(&role_counts_json)?;

        // Increment count for each role assigned in this session.
        for role in roles {
            *role_counts.entry(role.clone()).or_insert(0) += 1;
        }

        // Load feedback for affinity recomputation.
        let feedback: Vec<(String, String)> = {
            let mut stmt = conn.prepare(
                "SELECT role, observation
                 FROM persona_feedback
                 WHERE persona_id = ?1",
            )?;
            let rows = stmt
                .query_map([persona_id], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })?
                .collect::<Result<Vec<_>>>()?;
            rows
        };

        let affinity_scores = recompute_affinity(&role_counts, &feedback);

        let role_counts_str = serialize_map(&role_counts)?;
        let affinity_scores_str = serialize_map(&affinity_scores)?;

        conn.execute(
            "UPDATE personas SET role_counts = ?1, affinity_scores = ?2 WHERE id = ?3",
            rusqlite::params![role_counts_str, affinity_scores_str, persona_id],
        )?;
    }

    // Step 4: Clear assigned_to_session for all personas held by this session.
    conn.execute(
        "UPDATE personas SET assigned_to_session = NULL
         WHERE assigned_to_session = ?1",
        [session_id],
    )?;

    // Step 5: Set released_at on assignment records.
    conn.execute(
        "UPDATE persona_assignments SET released_at = ?1
         WHERE session_id = ?2 AND released_at IS NULL",
        rusqlite::params![now, session_id],
    )?;

    // Step 6: Retirement check — archive inactive personas.
    conn.execute(
        "UPDATE personas SET status = 'archived'
         WHERE status = 'active'
           AND last_assigned < datetime('now', '-30 days')
           AND assigned_to_session IS NULL",
        [],
    )?;

    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{broker, schema, seed};
    use rusqlite::Connection;

    fn setup_with_assignment() -> (Connection, i64) {
        let conn = Connection::open_in_memory().unwrap();
        schema::ensure_schema(&conn).unwrap();
        let toml_str = include_str!("../personas-seed.toml");
        let seed_data = seed::parse_seed(toml_str).unwrap();
        seed::insert_seed(&conn, &seed_data).unwrap();
        let assignments = broker::assign(
            &conn,
            &["code-reviewer"],
            "test-project",
            "session-release-1",
            "w1",
            None,
        )
        .unwrap();
        let pid = assignments[0].persona_id;
        (conn, pid)
    }

    #[test]
    fn release_clears_session_lock() {
        let (conn, pid) = setup_with_assignment();
        release(&conn, "session-release-1").unwrap();
        let locked: Option<String> = conn
            .query_row(
                "SELECT assigned_to_session FROM personas WHERE id = ?1",
                [pid],
                |r| r.get(0),
            )
            .unwrap();
        assert!(locked.is_none());
    }

    #[test]
    fn release_updates_role_counts() {
        let (conn, pid) = setup_with_assignment();
        release(&conn, "session-release-1").unwrap();
        let counts_json: String = conn
            .query_row(
                "SELECT role_counts FROM personas WHERE id = ?1",
                [pid],
                |r| r.get(0),
            )
            .unwrap();
        let counts: std::collections::HashMap<String, u32> =
            serde_json::from_str(&counts_json).unwrap();
        assert!(*counts.get("code-reviewer").unwrap_or(&0) >= 1);
    }

    #[test]
    fn release_sets_released_at() {
        let (conn, _) = setup_with_assignment();
        release(&conn, "session-release-1").unwrap();
        let released: Option<String> = conn
            .query_row(
                "SELECT released_at FROM persona_assignments WHERE session_id = 'session-release-1'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(released.is_some());
    }

    #[test]
    fn release_recomputes_affinity_scores() {
        let (conn, pid) = setup_with_assignment();
        release(&conn, "session-release-1").unwrap();
        let affinity_json: String = conn
            .query_row(
                "SELECT affinity_scores FROM personas WHERE id = ?1",
                [pid],
                |r| r.get(0),
            )
            .unwrap();
        let scores: std::collections::HashMap<String, f32> =
            serde_json::from_str(&affinity_json).unwrap();
        // After one assignment as code-reviewer the affinity must be > 0.
        assert!(scores.get("code-reviewer").copied().unwrap_or(0.0) > 0.0);
    }

    #[test]
    fn release_is_idempotent_on_session_without_assignments() {
        let conn = Connection::open_in_memory().unwrap();
        schema::ensure_schema(&conn).unwrap();
        // Releasing a session that was never assigned must not error.
        release(&conn, "session-nonexistent").unwrap();
    }

    #[test]
    fn classify_sentiment_positive() {
        assert_eq!(
            classify_sentiment("great work, solid catch"),
            Sentiment::Positive
        );
    }

    #[test]
    fn classify_sentiment_negative() {
        assert_eq!(
            classify_sentiment("missed the key issue and was verbose"),
            Sentiment::Negative
        );
    }

    #[test]
    fn classify_sentiment_neutral_on_tie() {
        // "great" (1 pos) and "missed" (1 neg) → tie → neutral
        assert_eq!(classify_sentiment("great but missed"), Sentiment::Neutral);
    }

    #[test]
    fn affinity_clamped_to_one() {
        let mut counts = HashMap::new();
        counts.insert("code-reviewer".to_string(), 1u32);
        // Inject 100 positive feedback entries to push score above 1.0 before clamping.
        let feedback: Vec<(String, String)> = (0..100)
            .map(|_| {
                (
                    "code-reviewer".to_string(),
                    "great solid helpful".to_string(),
                )
            })
            .collect();
        let scores = recompute_affinity(&counts, &feedback);
        let score = scores["code-reviewer"];
        assert!(score <= 1.0, "affinity must be clamped to 1.0, got {score}");
        assert!(score > 0.0, "affinity must be positive");
    }
}
