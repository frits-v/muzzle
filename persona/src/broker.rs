//! Broker: hot-path persona assignment with affinity scoring and session locking.

use rusqlite::{Connection, Result};
use std::collections::HashMap;

use crate::grow;
use crate::seed::{self, now_iso8601};
use crate::types::{expertise_for_role, normalize_role, Assignment, PersonaStatus};

// ---------------------------------------------------------------------------
// Internal working struct
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct Candidate {
    id: i64,
    name: String,
    traits: Vec<String>,
    expertise: Vec<String>,
    role_instructions: HashMap<String, String>,
    affinity_scores: HashMap<String, f32>,
    last_assigned: Option<String>,
}

// ---------------------------------------------------------------------------
// Scoring helpers
// ---------------------------------------------------------------------------

fn expertise_overlap(persona_expertise: &[String], role: &str) -> f32 {
    let required = expertise_for_role(role);
    if required.is_empty() {
        return 1.0;
    }
    for tag in persona_expertise {
        if required.contains(&tag.as_str()) {
            return 1.0;
        }
    }
    0.0
}

/// Parse an ISO 8601 date string ("YYYY-MM-DDT..." or "YYYY-MM-DD") and return
/// the number of days since the Unix epoch.
fn days_from_iso8601(s: &str) -> Option<u64> {
    // We only need the date part.
    let date = s.get(..10)?;
    let year: u64 = date.get(..4)?.parse().ok()?;
    let month: u64 = date.get(5..7)?.parse().ok()?;
    let day: u64 = date.get(8..10)?.parse().ok()?;

    // Civil-to-epoch-day (Gregorian proleptic), same algorithm as now_iso8601.
    let (y, m) = if month <= 2 {
        (year - 1, month + 9)
    } else {
        (year, month - 3)
    };
    let era = y / 400;
    let yoe = y - era * 400;
    let doy = (153 * m + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some(era * 146_097 + doe - 719_468)
}

/// Returns the current day as days-since-epoch.
fn today_days() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        / 86400
}

fn recency_penalty(last_assigned: &Option<String>) -> f32 {
    let Some(ref s) = last_assigned else {
        return 0.0;
    };
    let Some(last_day) = days_from_iso8601(s) else {
        return 0.0;
    };
    let today = today_days();
    let days_since = today.saturating_sub(last_day) as f32;
    (0.5_f32 - days_since * 0.05_f32).max(0.0)
}

fn score(candidate: &Candidate, role: &str) -> f32 {
    let affinity = candidate.affinity_scores.get(role).copied().unwrap_or(0.0);
    let overlap = expertise_overlap(&candidate.expertise, role) * 0.3;
    let penalty = recency_penalty(&candidate.last_assigned);
    affinity + overlap - penalty
}

/// Tie-break ordering: lower last_assigned day wins (NULL → 0).
fn last_assigned_day(c: &Candidate) -> u64 {
    c.last_assigned
        .as_deref()
        .and_then(days_from_iso8601)
        .unwrap_or(0)
}

// ---------------------------------------------------------------------------
// Query helpers
// ---------------------------------------------------------------------------

fn load_available_candidates(conn: &Connection) -> Result<Vec<Candidate>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, traits, expertise, role_instructions, affinity_scores, last_assigned
         FROM personas
         WHERE status = 'active' AND assigned_to_session IS NULL",
    )?;

    let candidates = stmt
        .query_map([], |row| {
            let id: i64 = row.get(0)?;
            let name: String = row.get(1)?;
            let traits_json: String = row.get(2)?;
            let expertise_json: String = row.get(3)?;
            let role_instructions_json: String = row.get(4)?;
            let affinity_scores_json: String = row.get(5)?;
            let last_assigned: Option<String> = row.get(6)?;

            let traits: Vec<String> = serde_json::from_str(&traits_json).map_err(|e| {
                rusqlite::Error::FromSqlConversionFailure(
                    2,
                    rusqlite::types::Type::Text,
                    Box::new(e),
                )
            })?;
            let expertise: Vec<String> = serde_json::from_str(&expertise_json).map_err(|e| {
                rusqlite::Error::FromSqlConversionFailure(
                    3,
                    rusqlite::types::Type::Text,
                    Box::new(e),
                )
            })?;
            let role_instructions: HashMap<String, String> =
                serde_json::from_str(&role_instructions_json).map_err(|e| {
                    rusqlite::Error::FromSqlConversionFailure(
                        4,
                        rusqlite::types::Type::Text,
                        Box::new(e),
                    )
                })?;
            let affinity_scores: HashMap<String, f32> = serde_json::from_str(&affinity_scores_json)
                .map_err(|e| {
                    rusqlite::Error::FromSqlConversionFailure(
                        5,
                        rusqlite::types::Type::Text,
                        Box::new(e),
                    )
                })?;

            Ok(Candidate {
                id,
                name,
                traits,
                expertise,
                role_instructions,
                affinity_scores,
                last_assigned,
            })
        })?
        .collect::<Result<Vec<_>>>()?;

    Ok(candidates)
}

// ---------------------------------------------------------------------------
// Public assign function
// ---------------------------------------------------------------------------

/// Assign personas to the given role slots in a single `BEGIN IMMEDIATE` transaction.
///
/// Returns one [`Assignment`] per role slot, indexed from 0.
pub fn assign(
    conn: &Connection,
    roles: &[&str],
    project: &str,
    session_id: &str,
    agent_name: &str,
    summon: Option<&str>,
) -> Result<Vec<Assignment>> {
    // 1. Normalize roles.
    let normalized_roles: Vec<&str> = roles.iter().map(|r| normalize_role(r)).collect();

    // 2. Begin exclusive transaction.
    conn.execute_batch("BEGIN IMMEDIATE")?;

    let result = assign_inner(
        conn,
        &normalized_roles,
        project,
        session_id,
        agent_name,
        summon,
    );

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

fn assign_inner(
    conn: &Connection,
    normalized_roles: &[&str],
    project: &str,
    session_id: &str,
    agent_name: &str,
    summon: Option<&str>,
) -> Result<Vec<Assignment>> {
    let now = now_iso8601();

    // 3. Load all available candidates (active + unassigned).
    let mut pool = load_available_candidates(conn)?;

    // 5. Handle summon: find by name (even if archived), reactivate if needed, pin to slot 0.
    let mut summon_candidate: Option<Candidate> = None;
    if let Some(name) = summon {
        // Check if the persona exists in pool first.
        if let Some(pos) = pool.iter().position(|c| c.name == name) {
            summon_candidate = Some(pool.remove(pos));
        } else {
            // Try to find the persona regardless of status.
            struct ArchivedRow {
                id: i64,
                name: String,
                traits_json: String,
                expertise_json: String,
                ri_json: String,
                aff_json: String,
                last_assigned: Option<String>,
                status: String,
            }
            let maybe: Option<ArchivedRow> = conn
                .query_row(
                    "SELECT id, name, traits, expertise, role_instructions, affinity_scores, last_assigned, status
                     FROM personas WHERE name = ?1",
                    [name],
                    |row| {
                        Ok(ArchivedRow {
                            id: row.get(0)?,
                            name: row.get(1)?,
                            traits_json: row.get(2)?,
                            expertise_json: row.get(3)?,
                            ri_json: row.get(4)?,
                            aff_json: row.get(5)?,
                            last_assigned: row.get(6)?,
                            status: row.get(7)?,
                        })
                    },
                )
                .ok();

            if let Some(ArchivedRow {
                id,
                name: pname,
                traits_json,
                expertise_json,
                ri_json,
                aff_json,
                last_assigned,
                status,
            }) = maybe
            {
                // Reactivate if archived.
                if status == PersonaStatus::Archived.to_db_str() {
                    conn.execute("UPDATE personas SET status = 'active' WHERE id = ?1", [id])?;
                }
                // Also clear any stale session lock.
                conn.execute(
                    "UPDATE personas SET assigned_to_session = NULL WHERE id = ?1",
                    [id],
                )?;

                let traits: Vec<String> = serde_json::from_str(&traits_json).map_err(|e| {
                    rusqlite::Error::FromSqlConversionFailure(
                        0,
                        rusqlite::types::Type::Text,
                        Box::new(e),
                    )
                })?;
                let expertise: Vec<String> =
                    serde_json::from_str(&expertise_json).map_err(|e| {
                        rusqlite::Error::FromSqlConversionFailure(
                            0,
                            rusqlite::types::Type::Text,
                            Box::new(e),
                        )
                    })?;
                let role_instructions: HashMap<String, String> = serde_json::from_str(&ri_json)
                    .map_err(|e| {
                        rusqlite::Error::FromSqlConversionFailure(
                            0,
                            rusqlite::types::Type::Text,
                            Box::new(e),
                        )
                    })?;
                let affinity_scores: HashMap<String, f32> = serde_json::from_str(&aff_json)
                    .map_err(|e| {
                        rusqlite::Error::FromSqlConversionFailure(
                            0,
                            rusqlite::types::Type::Text,
                            Box::new(e),
                        )
                    })?;

                summon_candidate = Some(Candidate {
                    id,
                    name: pname,
                    traits,
                    expertise,
                    role_instructions,
                    affinity_scores,
                    last_assigned,
                });
            }
        }
    }

    // Build the assignments list.
    let mut assignments: Vec<Assignment> = Vec::with_capacity(normalized_roles.len());

    for (slot, &role) in normalized_roles.iter().enumerate() {
        let picked = if slot == 0 {
            if let Some(c) = summon_candidate.take() {
                c
            } else {
                pick_or_grow(conn, &mut pool, role)?
            }
        } else {
            pick_or_grow(conn, &mut pool, role)?
        };

        // 9. UPDATE: lock persona to session.
        conn.execute(
            "UPDATE personas SET assigned_to_session = ?1, last_assigned = ?2 WHERE id = ?3",
            rusqlite::params![session_id, now, picked.id],
        )?;

        // 10. INSERT into persona_assignments.
        conn.execute(
            "INSERT INTO persona_assignments (persona_id, session_id, project, role, agent_name, assigned_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![picked.id, session_id, project, role, agent_name, now],
        )?;

        let role_instructions = picked
            .role_instructions
            .get(role)
            .cloned()
            .unwrap_or_default();

        assignments.push(Assignment {
            agent_slot: slot,
            persona_id: picked.id,
            name: picked.name,
            traits: picked.traits,
            expertise: picked.expertise,
            role_instructions,
        });
    }

    Ok(assignments)
}

/// Try to pick the best candidate; if the pool is exhausted, grow one new
/// persona and retry.  Returns an error only when grow itself fails (name
/// exhaustion).
fn pick_or_grow(conn: &Connection, pool: &mut Vec<Candidate>, role: &str) -> Result<Candidate> {
    if !pool.is_empty() {
        return pick_best(pool, role);
    }

    // Pool is empty — grow one persona from the seed file, then reload the
    // pool so the new persona is available.
    let toml_str = include_str!("../personas-seed.toml");
    let seed_file = seed::parse_seed(toml_str).map_err(|_| rusqlite::Error::QueryReturnedNoRows)?;
    let mut rng = grow::Rng::from_time();
    let grown = grow::grow(conn, &seed_file.meta, 1, &mut rng)?;
    if grown == 0 {
        return Err(rusqlite::Error::QueryReturnedNoRows);
    }

    // Reload candidates (including the freshly inserted persona) and pick.
    *pool = load_available_candidates(conn)?;
    pick_best(pool, role)
}

/// Pick the best candidate for `role` from `pool`, remove it, and return it.
/// Returns an error if the pool is empty.
fn pick_best(pool: &mut Vec<Candidate>, role: &str) -> Result<Candidate> {
    if pool.is_empty() {
        return Err(rusqlite::Error::QueryReturnedNoRows);
    }

    // Find index of highest scorer; ties broken by earliest last_assigned.
    let best_idx = pool
        .iter()
        .enumerate()
        .max_by(|(_, a), (_, b)| {
            let sa = score(a, role);
            let sb = score(b, role);
            sa.partial_cmp(&sb)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| last_assigned_day(b).cmp(&last_assigned_day(a)))
        })
        .map(|(i, _)| i)
        .unwrap();

    Ok(pool.swap_remove(best_idx))
}

// ---------------------------------------------------------------------------
// PersonaStatus helper
// ---------------------------------------------------------------------------

impl PersonaStatus {
    fn to_db_str(&self) -> &'static str {
        match self {
            PersonaStatus::Active => "active",
            PersonaStatus::Archived => "archived",
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{schema, seed};
    use rusqlite::Connection;

    fn setup_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        schema::ensure_schema(&conn).unwrap();
        let toml_str = include_str!("../personas-seed.toml");
        let seed_data = seed::parse_seed(toml_str).unwrap();
        seed::insert_seed(&conn, &seed_data).unwrap();
        conn
    }

    #[test]
    fn assign_single_role() {
        let conn = setup_db();
        let assignments = assign(
            &conn,
            &["code-reviewer"],
            "test-project",
            "session-001",
            "worker-1",
            None,
        )
        .unwrap();
        assert_eq!(assignments.len(), 1);
        assert!(!assignments[0].name.is_empty());
        assert!(assignments[0].persona_id > 0);
    }

    #[test]
    fn assign_multiple_roles_returns_distinct_personas() {
        let conn = setup_db();
        let assignments = assign(
            &conn,
            &["code-reviewer", "code-reviewer", "researcher"],
            "test-project",
            "session-002",
            "worker",
            None,
        )
        .unwrap();
        assert_eq!(assignments.len(), 3);
        let ids: Vec<i64> = assignments.iter().map(|a| a.persona_id).collect();
        let unique: std::collections::HashSet<i64> = ids.iter().copied().collect();
        assert_eq!(unique.len(), 3);
    }

    #[test]
    fn assign_locks_persona_to_session() {
        let conn = setup_db();
        let assignments = assign(
            &conn,
            &["code-reviewer"],
            "test-project",
            "session-003",
            "worker-1",
            None,
        )
        .unwrap();
        let pid = assignments[0].persona_id;
        let locked: Option<String> = conn
            .query_row(
                "SELECT assigned_to_session FROM personas WHERE id = ?1",
                [pid],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(locked.as_deref(), Some("session-003"));
    }

    #[test]
    fn assign_with_summon() {
        let conn = setup_db();
        let assignments = assign(
            &conn,
            &["code-reviewer"],
            "test-project",
            "session-004",
            "worker-1",
            Some("Elena Vasquez"),
        )
        .unwrap();
        assert_eq!(assignments.len(), 1);
        assert_eq!(assignments[0].name, "Elena Vasquez");
    }
}
