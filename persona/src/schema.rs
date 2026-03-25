//! SQLite schema for muzzle-persona.

use rusqlite::{Connection, Result};

/// Create all persona tables and indexes if they do not already exist.
pub fn ensure_schema(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS personas (
            id                  INTEGER PRIMARY KEY AUTOINCREMENT,
            name                TEXT NOT NULL UNIQUE,
            traits              TEXT NOT NULL DEFAULT '[]',
            expertise           TEXT NOT NULL DEFAULT '[]',
            role_instructions   TEXT NOT NULL DEFAULT '{}',
            affinity_scores     TEXT NOT NULL DEFAULT '{}',
            role_counts         TEXT NOT NULL DEFAULT '{}',
            status              TEXT NOT NULL DEFAULT 'active',
            assigned_to_session TEXT,
            created_at          TEXT NOT NULL,
            last_assigned       TEXT
        );

        CREATE TABLE IF NOT EXISTS persona_feedback (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            persona_id  INTEGER NOT NULL REFERENCES personas(id),
            timestamp   TEXT NOT NULL,
            project     TEXT NOT NULL,
            role        TEXT NOT NULL,
            observation TEXT NOT NULL,
            source      TEXT NOT NULL DEFAULT 'session',
            compacted   INTEGER NOT NULL DEFAULT 0
        );

        CREATE TABLE IF NOT EXISTS persona_assignments (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            persona_id  INTEGER NOT NULL REFERENCES personas(id),
            session_id  TEXT NOT NULL,
            project     TEXT NOT NULL,
            role        TEXT NOT NULL,
            agent_slot  TEXT NOT NULL,
            team_name   TEXT,
            agent_name  TEXT,
            assigned_at TEXT NOT NULL,
            released_at TEXT
        );

        CREATE INDEX IF NOT EXISTS idx_personas_status
            ON personas(status);

        CREATE INDEX IF NOT EXISTS idx_personas_assigned_session
            ON personas(assigned_to_session)
            WHERE assigned_to_session IS NOT NULL;

        CREATE INDEX IF NOT EXISTS idx_feedback_persona
            ON persona_feedback(persona_id);

        CREATE INDEX IF NOT EXISTS idx_assignments_session
            ON persona_assignments(session_id);

        CREATE INDEX IF NOT EXISTS idx_assignments_persona
            ON persona_assignments(persona_id);
        ",
    )
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn in_memory() -> Connection {
        Connection::open_in_memory().expect("open in-memory db")
    }

    #[test]
    fn ensure_schema_creates_tables() {
        let conn = in_memory();
        ensure_schema(&conn).expect("schema creation should succeed");

        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master
                 WHERE type='table'
                   AND name IN ('personas','persona_feedback','persona_assignments')",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 3, "all three tables must exist");
    }

    #[test]
    fn ensure_schema_is_idempotent() {
        let conn = in_memory();
        ensure_schema(&conn).expect("first call");
        ensure_schema(&conn).expect("second call should not error");
    }
}
