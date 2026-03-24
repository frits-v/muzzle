//! SQLite schema creation for muzzle-persona.

use rusqlite::{Connection, Result};

/// Create all persona tables and indexes if they do not already exist.
pub fn ensure_schema(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS personas (
            id                  INTEGER PRIMARY KEY,
            name                TEXT NOT NULL UNIQUE,
            traits              TEXT NOT NULL,
            expertise           TEXT NOT NULL,
            role_instructions   TEXT NOT NULL DEFAULT '{}',
            affinity_scores     TEXT NOT NULL DEFAULT '{}',
            role_counts         TEXT NOT NULL DEFAULT '{}',
            status              TEXT NOT NULL DEFAULT 'active',
            assigned_to_session TEXT,
            created_at          TEXT NOT NULL,
            last_assigned       TEXT
        );

        CREATE TABLE IF NOT EXISTS persona_feedback (
            id          INTEGER PRIMARY KEY,
            persona_id  INTEGER NOT NULL REFERENCES personas(id),
            timestamp   TEXT NOT NULL,
            project     TEXT NOT NULL,
            role        TEXT NOT NULL,
            observation TEXT NOT NULL,
            source      TEXT NOT NULL,
            compacted   INTEGER NOT NULL DEFAULT 0
        );

        CREATE TABLE IF NOT EXISTS persona_assignments (
            id          INTEGER PRIMARY KEY,
            persona_id  INTEGER NOT NULL REFERENCES personas(id),
            session_id  TEXT NOT NULL,
            project     TEXT,
            role        TEXT NOT NULL,
            team_name   TEXT,
            agent_name  TEXT NOT NULL,
            assigned_at TEXT NOT NULL,
            released_at TEXT
        );

        CREATE INDEX IF NOT EXISTS idx_personas_status
            ON personas(status);
        CREATE INDEX IF NOT EXISTS idx_personas_session
            ON personas(assigned_to_session);
        CREATE INDEX IF NOT EXISTS idx_feedback_persona
            ON persona_feedback(persona_id, compacted);
        CREATE INDEX IF NOT EXISTS idx_assignments_persona
            ON persona_assignments(persona_id);
        CREATE INDEX IF NOT EXISTS idx_assignments_session
            ON persona_assignments(session_id);
        ",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table_exists(conn: &Connection, name: &str) -> bool {
        let count: i64 = conn
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type='table' AND name=?1",
                rusqlite::params![name],
                |row| row.get(0),
            )
            .unwrap_or(0);
        count > 0
    }

    #[test]
    fn ensure_schema_creates_tables() {
        let conn = Connection::open_in_memory().unwrap();
        ensure_schema(&conn).unwrap();

        assert!(table_exists(&conn, "personas"), "personas table missing");
        assert!(
            table_exists(&conn, "persona_feedback"),
            "persona_feedback table missing"
        );
        assert!(
            table_exists(&conn, "persona_assignments"),
            "persona_assignments table missing"
        );
    }

    #[test]
    fn ensure_schema_is_idempotent() {
        let conn = Connection::open_in_memory().unwrap();
        ensure_schema(&conn).unwrap();
        // Second call must not fail
        ensure_schema(&conn).unwrap();
    }
}
