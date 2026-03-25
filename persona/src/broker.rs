//! Broker: affinity-scored persona assignment for agent sessions.

use rusqlite::{params, Connection, Result};
use std::collections::HashMap;

use crate::types::{expertise_for_role, normalize_role, Assignment};

// ---------------------------------------------------------------------------
// Internal candidate representation
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct Candidate {
    id: i64,
    name: String,
    traits: Vec<String>,
    expertise: Vec<String>,
    role_instructions: HashMap<String, String>,
    affinity_scores: HashMap<String, f32>,
    /// ISO-8601 date string or None.
    last_assigned: Option<String>,
}

// ---------------------------------------------------------------------------
// Date helpers (civil calendar math, no external dependencies)
// ---------------------------------------------------------------------------

/// Convert the date part of an ISO-8601 string (`YYYY-MM-DD…`) to days since
/// the Unix epoch (1970-01-01 = day 0).  Returns `None` on parse failure.
fn days_from_iso8601(s: &str) -> Option<u64> {
    // Accept "YYYY-MM-DD" prefix; ignore the time part.
    let date = &s[..s.len().min(10)];
    let mut parts = date.splitn(3, '-');
    let y: u64 = parts.next()?.parse().ok()?;
    let m: u64 = parts.next()?.parse().ok()?;
    let d: u64 = parts.next()?.parse().ok()?;

    if m == 0 || m > 12 || d == 0 || d > 31 {
        return None;
    }

    // Howard Hinnant civil-calendar algorithm (public domain) — inverse of
    // seed::days_to_ymd.
    let (y, m) = if m <= 2 { (y - 1, m + 9) } else { (y, m - 3) };
    let era = y / 400;
    let yoe = y % 400;
    let doy = (153 * m + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe;

    // days is relative to the civil calendar epoch (0000-03-01). Subtract the
    // offset so that 1970-01-01 → 0.
    days.checked_sub(719_468)
}

/// Number of days since the Unix epoch for today (UTC).
fn today_days() -> u64 {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    secs / 86_400
}

/// Recency penalty: recent assignments score lower to encourage variety.
///
/// `penalty = max(0.5 - days_since_last * 0.05, 0.0)`
/// NULL last_assigned → 0.0 (no penalty).
fn recency_penalty(last_assigned: &Option<String>) -> f32 {
    let Some(ref s) = last_assigned else {
        return 0.0;
    };
    let Some(last_days) = days_from_iso8601(s) else {
        return 0.0;
    };
    let today = today_days();
    let days_since = today.saturating_sub(last_days) as f32;
    (0.5 - days_since * 0.05_f32).max(0.0)
}

// ---------------------------------------------------------------------------
// Scoring
// ---------------------------------------------------------------------------

fn score(candidate: &Candidate, role: &str) -> f32 {
    let affinity = candidate
        .affinity_scores
        .get(role)
        .copied()
        .unwrap_or(0.0);

    let required = expertise_for_role(role);
    let expertise_overlap: f32 = if required.is_empty() {
        1.0
    } else {
        let has_match = candidate
            .expertise
            .iter()
            .any(|e| required.contains(&e.as_str()));
        if has_match { 1.0 } else { 0.0 }
    };

    affinity + expertise_overlap * 0.3 - recency_penalty(&candidate.last_assigned)
}

/// Tie-break value: earlier last_assigned wins (NULL → epoch 0).
fn tiebreak_days(candidate: &Candidate) -> u64 {
    candidate
        .last_assigned
        .as_deref()
        .and_then(days_from_iso8601)
        .unwrap_or(0)
}

/// Pick the best-scoring candidate from `pool` for `role`, removing it.
///
/// Returns `Err` if the pool is empty.
fn pick_best(pool: &mut Vec<Candidate>, role: &str) -> Result<Candidate> {
    if pool.is_empty() {
        return Err(rusqlite::Error::QueryReturnedNoRows);
    }

    let best_idx = pool
        .iter()
        .enumerate()
        .max_by(|(_, a), (_, b)| {
            let sa = score(a, role);
            let sb = score(b, role);
            sa.partial_cmp(&sb)
                .unwrap_or(std::cmp::Ordering::Equal)
                // Tie-break: prefer earliest last_assigned.
                .then_with(|| tiebreak_days(b).cmp(&tiebreak_days(a)))
        })
        .map(|(i, _)| i)
        .unwrap(); // safe: non-empty

    Ok(pool.swap_remove(best_idx))
}

/// Try `pick_best`; returns error on empty pool (Task 3 will add grow fallback).
fn pick_or_grow(pool: &mut Vec<Candidate>, role: &str) -> Result<Candidate> {
    pick_best(pool, role)
}

// ---------------------------------------------------------------------------
// Loading candidates from DB
// ---------------------------------------------------------------------------

fn load_candidates(conn: &Connection) -> Result<Vec<Candidate>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, traits, expertise, role_instructions,
                affinity_scores, last_assigned
           FROM personas
          WHERE status = 'active'
            AND assigned_to_session IS NULL",
    )?;

    let rows = stmt.query_map([], |row| {
        let id: i64 = row.get(0)?;
        let name: String = row.get(1)?;
        let traits_json: String = row.get(2)?;
        let expertise_json: String = row.get(3)?;
        let role_instructions_json: String = row.get(4)?;
        let affinity_scores_json: String = row.get(5)?;
        let last_assigned: Option<String> = row.get(6)?;

        // Resilient JSON parsing: log and skip corrupt rows.
        let traits: Vec<String> = match serde_json::from_str(&traits_json) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("muzzle-persona: skipping persona {id} — bad traits JSON: {e}");
                return Ok(None);
            }
        };
        let expertise: Vec<String> = match serde_json::from_str(&expertise_json) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("muzzle-persona: skipping persona {id} — bad expertise JSON: {e}");
                return Ok(None);
            }
        };
        let role_instructions: HashMap<String, String> =
            match serde_json::from_str(&role_instructions_json) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!(
                        "muzzle-persona: skipping persona {id} — bad role_instructions JSON: {e}"
                    );
                    return Ok(None);
                }
            };
        let affinity_scores: HashMap<String, f32> =
            match serde_json::from_str(&affinity_scores_json) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!(
                        "muzzle-persona: skipping persona {id} — bad affinity_scores JSON: {e}"
                    );
                    return Ok(None);
                }
            };

        Ok(Some(Candidate {
            id,
            name,
            traits,
            expertise,
            role_instructions,
            affinity_scores,
            last_assigned,
        }))
    })?;

    let mut candidates = Vec::new();
    for row in rows {
        if let Some(candidate) = row? {
            candidates.push(candidate);
        }
    }
    Ok(candidates)
}

// ---------------------------------------------------------------------------
// Summon helper
// ---------------------------------------------------------------------------

/// Find a persona by name (including archived), reactivate if needed, and
/// clear any stale session lock.  Returns the candidate or `None` if not found.
fn find_summon(conn: &Connection, name: &str) -> Result<Option<Candidate>> {
    let row = conn.query_row(
        "SELECT id, name, traits, expertise, role_instructions,
                affinity_scores, status, last_assigned
           FROM personas
          WHERE name = ?1",
        params![name],
        |row| {
            let id: i64 = row.get(0)?;
            let nm: String = row.get(1)?;
            let traits_json: String = row.get(2)?;
            let expertise_json: String = row.get(3)?;
            let role_instructions_json: String = row.get(4)?;
            let affinity_scores_json: String = row.get(5)?;
            let status: String = row.get(6)?;
            let last_assigned: Option<String> = row.get(7)?;
            Ok((
                id,
                nm,
                traits_json,
                expertise_json,
                role_instructions_json,
                affinity_scores_json,
                status,
                last_assigned,
            ))
        },
    );

    let row = match row {
        Ok(r) => r,
        Err(rusqlite::Error::QueryReturnedNoRows) => return Ok(None),
        Err(e) => return Err(e),
    };

    let (id, nm, traits_json, expertise_json, ri_json, aff_json, status, last_assigned) = row;

    let traits: Vec<String> = serde_json::from_str(&traits_json).unwrap_or_default();
    let expertise: Vec<String> = serde_json::from_str(&expertise_json).unwrap_or_default();
    let role_instructions: HashMap<String, String> =
        serde_json::from_str(&ri_json).unwrap_or_default();
    let affinity_scores: HashMap<String, f32> = serde_json::from_str(&aff_json).unwrap_or_default();

    // Reactivate if archived; clear stale lock.
    if status == "archived" {
        conn.execute(
            "UPDATE personas SET status = 'active', assigned_to_session = NULL WHERE id = ?1",
            params![id],
        )?;
    } else {
        // Clear stale lock from a previous session.
        conn.execute(
            "UPDATE personas SET assigned_to_session = NULL WHERE id = ?1",
            params![id],
        )?;
    }

    Ok(Some(Candidate {
        id,
        name: nm,
        traits,
        expertise,
        role_instructions,
        affinity_scores,
        last_assigned,
    }))
}

// ---------------------------------------------------------------------------
// Recent-work query
// ---------------------------------------------------------------------------

fn recent_work_for(conn: &Connection, persona_id: i64, session_id: &str) -> Result<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT role, project, assigned_at
           FROM persona_assignments
          WHERE persona_id = ?1
            AND session_id != ?2
          ORDER BY assigned_at DESC
          LIMIT 3",
    )?;

    let rows = stmt.query_map(params![persona_id, session_id], |row| {
        let role: String = row.get(0)?;
        let project: String = row.get(1)?;
        let assigned_at: String = row.get(2)?;
        Ok((role, project, assigned_at))
    })?;

    let mut entries = Vec::new();
    for row in rows {
        let (role, project, assigned_at) = row?;
        let date = &assigned_at[..assigned_at.len().min(10)];
        entries.push(format!("{role} on {project} ({date})"));
    }
    Ok(entries)
}

// ---------------------------------------------------------------------------
// Public assign function
// ---------------------------------------------------------------------------

/// Assign one or more personas to agent slots for a session.
///
/// Each role string in `roles` is normalized via [`crate::types::normalize_role`]
/// before scoring.  Returns one [`Assignment`] per role slot.
pub fn assign(
    conn: &Connection,
    roles: &[&str],
    project: &str,
    session_id: &str,
    agent_name: &str,
    team_name: Option<&str>,
    summon: Option<&str>,
) -> Result<Vec<Assignment>> {
    let normalized_roles: Vec<&str> = roles.iter().map(|r| normalize_role(r)).collect();

    conn.execute_batch("BEGIN IMMEDIATE")?;

    let result = (|| -> Result<Vec<Assignment>> {
        // Load all available candidates.
        let mut pool = load_candidates(conn)?;

        // Handle summon: prepend the named persona to the pool (or add it
        // exclusively for the first role slot).
        let mut forced_first: Option<Candidate> = None;
        if let Some(summon_name) = summon {
            if let Some(candidate) = find_summon(conn, summon_name)? {
                // Remove from pool if already present.
                pool.retain(|c| c.id != candidate.id);
                forced_first = Some(candidate);
            }
        }

        let now = crate::seed::now_iso8601();
        let mut assignments = Vec::new();

        for (slot_idx, &role) in normalized_roles.iter().enumerate() {
            let candidate = if slot_idx == 0 {
                if let Some(forced) = forced_first.take() {
                    forced
                } else {
                    pick_or_grow(&mut pool, role)?
                }
            } else {
                pick_or_grow(&mut pool, role)?
            };

            // UPDATE persona: lock to session + record last_assigned.
            conn.execute(
                "UPDATE personas
                    SET assigned_to_session = ?1,
                        last_assigned       = ?2
                  WHERE id = ?3",
                params![session_id, &now, candidate.id],
            )?;

            // INSERT assignment record.
            let agent_slot = format!("{agent_name}-{slot_idx}");
            conn.execute(
                "INSERT INTO persona_assignments
                     (persona_id, session_id, project, role, agent_slot,
                      team_name, agent_name, assigned_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    candidate.id,
                    session_id,
                    project,
                    role,
                    &agent_slot,
                    team_name,
                    agent_name,
                    &now,
                ],
            )?;

            // Fetch recent work (prior sessions only).
            let recent_work = recent_work_for(conn, candidate.id, session_id)?;

            let role_instructions = candidate
                .role_instructions
                .get(role)
                .or_else(|| candidate.role_instructions.get("general"))
                .cloned()
                .unwrap_or_default();

            assignments.push(Assignment {
                agent_slot,
                persona_id: candidate.id,
                name: candidate.name,
                traits: candidate.traits,
                expertise: candidate.expertise,
                role_instructions,
                recent_work,
            });
        }

        Ok(assignments)
    })();

    match result {
        Ok(assignments) => {
            conn.execute_batch("COMMIT")?;
            Ok(assignments)
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
    use crate::seed::{insert_seed, parse_seed};
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
        let seed = parse_seed(SAMPLE_TOML).expect("parse seed");
        insert_seed(&conn, &seed).expect("seed");
        conn
    }

    #[test]
    fn assign_single_role() {
        let conn = setup();
        let result = assign(
            &conn,
            &["code-reviewer"],
            "acme-api",
            "sess-001",
            "agent-alpha",
            None,
            None,
        )
        .expect("assign should succeed");

        assert_eq!(result.len(), 1, "one assignment per role");
        let a = &result[0];
        assert!(!a.name.is_empty());
        assert!(!a.traits.is_empty());
        assert_eq!(a.agent_slot, "agent-alpha-0");
    }

    #[test]
    fn assign_multiple_roles_returns_distinct_personas() {
        let conn = setup();
        let result = assign(
            &conn,
            &["code-reviewer", "security-review", "testing"],
            "web-app",
            "sess-002",
            "agent-beta",
            None,
            None,
        )
        .expect("assign should succeed");

        assert_eq!(result.len(), 3);

        // All persona IDs must be distinct.
        let ids: Vec<i64> = result.iter().map(|a| a.persona_id).collect();
        let unique: std::collections::HashSet<i64> = ids.iter().copied().collect();
        assert_eq!(ids.len(), unique.len(), "each role gets a distinct persona");
    }

    #[test]
    fn assign_locks_persona_to_session() {
        let conn = setup();
        let result = assign(
            &conn,
            &["general"],
            "acme-api",
            "sess-003",
            "agent-gamma",
            None,
            None,
        )
        .expect("first assign");

        let persona_id = result[0].persona_id;

        // The assigned persona must be locked to the session.
        let locked_session: Option<String> = conn
            .query_row(
                "SELECT assigned_to_session FROM personas WHERE id = ?1",
                params![persona_id],
                |r| r.get(0),
            )
            .expect("query");
        assert_eq!(locked_session.as_deref(), Some("sess-003"));

        // A second assign in a different session must NOT return the same persona.
        let result2 = assign(
            &conn,
            &["general"],
            "acme-api",
            "sess-004",
            "agent-delta",
            None,
            None,
        )
        .expect("second assign");

        assert_ne!(
            result2[0].persona_id, persona_id,
            "locked persona must not be reassigned"
        );
    }

    #[test]
    fn assign_with_summon() {
        let conn = setup();

        // Archive Bob Jones to verify summon can reactivate.
        conn.execute(
            "UPDATE personas SET status = 'archived' WHERE name = 'Bob Jones'",
            [],
        )
        .expect("archive");

        let result = assign(
            &conn,
            &["security-review"],
            "internal-tool",
            "sess-005",
            "agent-epsilon",
            Some("security-team"),
            Some("Bob Jones"),
        )
        .expect("assign with summon");

        assert_eq!(result.len(), 1);
        assert_eq!(
            result[0].name, "Bob Jones",
            "summon must force the named persona into the first slot"
        );

        // Bob Jones must now be active and locked.
        let (status, session): (String, Option<String>) = conn
            .query_row(
                "SELECT status, assigned_to_session FROM personas WHERE name = 'Bob Jones'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .expect("query");
        assert_eq!(status, "active");
        assert_eq!(session.as_deref(), Some("sess-005"));
    }
}
