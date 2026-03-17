//! SQLite + FTS5 persistent storage engine for observations.

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::ops::Deref;
use std::time::SystemTime;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Input for creating/upserting an observation.
#[derive(Debug, Default)]
pub struct NewObservation {
    pub session_id: String,
    pub obs_type: String,
    pub title: String,
    pub content: String,
    pub project: String,
    pub scope: Option<String>,
    pub topic_key: Option<String>,
    pub source: String,
}

/// A stored observation row.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Observation {
    pub id: i64,
    pub session_id: String,
    #[serde(rename = "type")]
    pub obs_type: String,
    pub title: String,
    pub content: String,
    pub project: String,
    pub scope: String,
    pub topic_key: Option<String>,
    pub source: String,
    pub revision_count: i64,
    pub duplicate_count: i64,
    pub created_at: String,
    pub updated_at: String,
}

/// FTS5 search result: an observation with a relevance rank.
#[derive(Debug, Serialize)]
pub struct SearchResult {
    #[serde(flatten)]
    pub observation: Observation,
    pub rank: f64,
}

impl Deref for SearchResult {
    type Target = Observation;
    fn deref(&self) -> &Self::Target {
        &self.observation
    }
}

/// Aggregate statistics.
#[derive(Debug, Serialize)]
pub struct Stats {
    pub total_sessions: i64,
    pub total_observations: i64,
    pub projects: Vec<String>,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Format a `SystemTime` as an approximate ISO 8601 string (UTC-ish).
fn iso_now() -> String {
    let dur = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = dur.as_secs();

    // Decompose epoch seconds into date/time components.
    let days = secs / 86400;
    let time_of_day = secs % 86400;
    let hours = time_of_day / 3600;
    let minutes = (time_of_day % 3600) / 60;
    let seconds = time_of_day % 60;

    // Convert days since epoch to Y-M-D (simplified, no leap-second pedantry).
    let (year, month, day) = days_to_ymd(days);

    format!("{year:04}-{month:02}-{day:02}T{hours:02}:{minutes:02}:{seconds:02}Z")
}

fn days_to_ymd(mut days: u64) -> (u64, u64, u64) {
    // Algorithm from Howard Hinnant (public domain).
    days += 719_468;
    let era = days / 146_097;
    let doe = days % 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

// ---------------------------------------------------------------------------
// Store
// ---------------------------------------------------------------------------

/// Persistent SQLite + FTS5 store.
pub struct Store {
    conn: Connection,
}

impl Store {
    /// Open (or create) the database at `path`.
    /// Use `":memory:"` for tests.
    pub fn open(path: &str) -> rusqlite::Result<Self> {
        let conn = Connection::open(path)?;
        // journal_mode returns the new mode — must use pragma_update_and_check.
        let _: String =
            conn.pragma_update_and_check(None, "journal_mode", "WAL", |row| row.get(0))?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        let store = Self { conn };
        store.migrate()?;
        Ok(store)
    }

    // -- Schema ---------------------------------------------------------------

    fn migrate(&self) -> rusqlite::Result<()> {
        self.conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS sessions (
                id TEXT PRIMARY KEY,
                project TEXT NOT NULL,
                directory TEXT NOT NULL,
                started_at TEXT NOT NULL,
                ended_at TEXT,
                summary TEXT
            );

            CREATE TABLE IF NOT EXISTS observations (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id TEXT NOT NULL REFERENCES sessions(id),
                type TEXT NOT NULL,
                title TEXT NOT NULL,
                content TEXT NOT NULL,
                project TEXT NOT NULL,
                scope TEXT NOT NULL DEFAULT 'project',
                topic_key TEXT,
                source TEXT NOT NULL DEFAULT 'changelog',
                revision_count INTEGER NOT NULL DEFAULT 1,
                duplicate_count INTEGER NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                deleted_at TEXT
            );

            CREATE INDEX IF NOT EXISTS idx_obs_project
                ON observations(project);
            CREATE INDEX IF NOT EXISTS idx_obs_session
                ON observations(session_id);
            CREATE INDEX IF NOT EXISTS idx_obs_topic
                ON observations(project, scope, topic_key);
            CREATE INDEX IF NOT EXISTS idx_obs_created
                ON observations(created_at DESC);

            CREATE VIRTUAL TABLE IF NOT EXISTS observations_fts USING fts5(
                title, content, project, type,
                content=observations, content_rowid=id
            );

            -- Keep FTS in sync via triggers.
            CREATE TRIGGER IF NOT EXISTS obs_ai AFTER INSERT ON observations BEGIN
                INSERT INTO observations_fts(rowid, title, content, project, type)
                VALUES (new.id, new.title, new.content, new.project, new.type);
            END;

            CREATE TRIGGER IF NOT EXISTS obs_au AFTER UPDATE ON observations BEGIN
                INSERT INTO observations_fts(observations_fts, rowid, title, content, project, type)
                VALUES ('delete', old.id, old.title, old.content, old.project, old.type);
                INSERT INTO observations_fts(rowid, title, content, project, type)
                VALUES (new.id, new.title, new.content, new.project, new.type);
            END;

            CREATE TRIGGER IF NOT EXISTS obs_ad AFTER DELETE ON observations BEGIN
                INSERT INTO observations_fts(observations_fts, rowid, title, content, project, type)
                VALUES ('delete', old.id, old.title, old.content, old.project, old.type);
            END;
            ",
        )
    }

    // -- Sessions -------------------------------------------------------------

    /// Register a session (idempotent).
    pub fn register_session(
        &self,
        id: &str,
        project: &str,
        directory: &str,
    ) -> rusqlite::Result<()> {
        self.conn.execute(
            "INSERT OR IGNORE INTO sessions (id, project, directory, started_at)
             VALUES (?1, ?2, ?3, ?4)",
            params![id, project, directory, iso_now()],
        )?;
        Ok(())
    }

    // -- Observations ---------------------------------------------------------

    /// Save (or upsert) an observation.
    ///
    /// If `topic_key` is `Some` and a non-deleted row with the same
    /// `(project, scope, topic_key)` exists, the existing row is updated
    /// and `revision_count` is incremented. Otherwise a new row is inserted.
    ///
    /// Returns the row id.
    pub fn save_observation(&self, obs: NewObservation) -> rusqlite::Result<i64> {
        let now = iso_now();
        let scope = obs.scope.unwrap_or_else(|| "project".to_string());

        // Upsert path: try to find an existing row with the same topic_key.
        if let Some(ref topic_key) = obs.topic_key {
            let existing_id: Option<i64> = self
                .conn
                .query_row(
                    "SELECT id FROM observations
                     WHERE project = ?1 AND scope = ?2 AND topic_key = ?3
                       AND deleted_at IS NULL
                     LIMIT 1",
                    params![obs.project, scope, topic_key],
                    |row| row.get(0),
                )
                .optional()?;

            if let Some(id) = existing_id {
                self.conn.execute(
                    "UPDATE observations
                     SET title = ?1, content = ?2, type = ?3, source = ?4,
                         revision_count = revision_count + 1, updated_at = ?5
                     WHERE id = ?6",
                    params![obs.title, obs.content, obs.obs_type, obs.source, now, id],
                )?;
                return Ok(id);
            }
        }

        // Insert path.
        self.conn.execute(
            "INSERT INTO observations
                 (session_id, type, title, content, project, scope, topic_key,
                  source, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                obs.session_id,
                obs.obs_type,
                obs.title,
                obs.content,
                obs.project,
                scope,
                obs.topic_key,
                obs.source,
                now,
                now,
            ],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    // -- Queries --------------------------------------------------------------

    /// Full-text search. Returns results ordered by relevance.
    ///
    /// User input is wrapped in double quotes to force FTS5 phrase matching
    /// and prevent query syntax injection (`AND`, `OR`, `NOT`, `col:`, `*`).
    pub fn search(
        &self,
        query: &str,
        project: Option<&str>,
        limit: i64,
    ) -> rusqlite::Result<Vec<SearchResult>> {
        // Sanitize: wrap in double quotes (phrase query) and escape embedded quotes.
        let safe_query = format!("\"{}\"", query.replace('"', "\"\""));

        let sql = if project.is_some() {
            "SELECT o.id, o.session_id, o.type, o.title, o.content, o.project,
                    o.scope, o.topic_key, o.source, o.revision_count,
                    o.duplicate_count, o.created_at, o.updated_at,
                    f.rank
             FROM observations_fts f
             JOIN observations o ON o.id = f.rowid
             WHERE observations_fts MATCH ?1
               AND o.project = ?2
               AND o.deleted_at IS NULL
             ORDER BY f.rank
             LIMIT ?3"
        } else {
            "SELECT o.id, o.session_id, o.type, o.title, o.content, o.project,
                    o.scope, o.topic_key, o.source, o.revision_count,
                    o.duplicate_count, o.created_at, o.updated_at,
                    f.rank
             FROM observations_fts f
             JOIN observations o ON o.id = f.rowid
             WHERE observations_fts MATCH ?1
               AND o.deleted_at IS NULL
             ORDER BY f.rank
             LIMIT ?2"
        };

        let mut stmt = self.conn.prepare(sql)?;

        let rows = if let Some(proj) = project {
            stmt.query_map(params![safe_query, proj, limit], Self::map_search_row)?
        } else {
            stmt.query_map(params![safe_query, limit], Self::map_search_row)?
        };

        rows.collect()
    }

    /// Return the N most recent non-deleted observations for a project.
    pub fn recent_context(&self, project: &str, limit: i64) -> rusqlite::Result<Vec<Observation>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, session_id, type, title, content, project,
                    scope, topic_key, source, revision_count,
                    duplicate_count, created_at, updated_at
             FROM observations
             WHERE project = ?1 AND deleted_at IS NULL
             ORDER BY created_at DESC
             LIMIT ?2",
        )?;

        let rows = stmt.query_map(params![project, limit], Self::map_obs_row)?;
        rows.collect()
    }

    /// Soft-delete an observation by id.
    pub fn soft_delete(&self, id: i64) -> rusqlite::Result<()> {
        self.conn.execute(
            "UPDATE observations SET deleted_at = ?1 WHERE id = ?2",
            params![iso_now(), id],
        )?;
        Ok(())
    }

    /// Aggregate statistics.
    pub fn stats(&self) -> rusqlite::Result<Stats> {
        let total_sessions: i64 =
            self.conn
                .query_row("SELECT COUNT(*) FROM sessions", [], |r| r.get(0))?;

        let total_observations: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM observations WHERE deleted_at IS NULL",
            [],
            |r| r.get(0),
        )?;

        let mut stmt = self.conn.prepare(
            "SELECT DISTINCT project FROM observations WHERE deleted_at IS NULL ORDER BY project",
        )?;
        let projects: Vec<String> = stmt
            .query_map([], |row| row.get(0))?
            .collect::<rusqlite::Result<Vec<String>>>()?;

        Ok(Stats {
            total_sessions,
            total_observations,
            projects,
        })
    }

    // -- Row mappers ----------------------------------------------------------

    fn map_obs_row(row: &rusqlite::Row) -> rusqlite::Result<Observation> {
        Ok(Observation {
            id: row.get(0)?,
            session_id: row.get(1)?,
            obs_type: row.get(2)?,
            title: row.get(3)?,
            content: row.get(4)?,
            project: row.get(5)?,
            scope: row.get(6)?,
            topic_key: row.get(7)?,
            source: row.get(8)?,
            revision_count: row.get(9)?,
            duplicate_count: row.get(10)?,
            created_at: row.get(11)?,
            updated_at: row.get(12)?,
        })
    }

    fn map_search_row(row: &rusqlite::Row) -> rusqlite::Result<SearchResult> {
        Ok(SearchResult {
            observation: Observation {
                id: row.get(0)?,
                session_id: row.get(1)?,
                obs_type: row.get(2)?,
                title: row.get(3)?,
                content: row.get(4)?,
                project: row.get(5)?,
                scope: row.get(6)?,
                topic_key: row.get(7)?,
                source: row.get(8)?,
                revision_count: row.get(9)?,
                duplicate_count: row.get(10)?,
                created_at: row.get(11)?,
                updated_at: row.get(12)?,
            },
            rank: row.get(13)?,
        })
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn test_store() -> Store {
        Store::open(":memory:").expect("open in-memory store")
    }

    fn seed_session(store: &Store, id: &str, project: &str) {
        store
            .register_session(id, project, "/tmp/test")
            .expect("register session");
    }

    fn make_obs(session_id: &str, project: &str, title: &str, content: &str) -> NewObservation {
        NewObservation {
            session_id: session_id.to_string(),
            obs_type: "learning".to_string(),
            title: title.to_string(),
            content: content.to_string(),
            project: project.to_string(),
            source: "test".to_string(),
            ..Default::default()
        }
    }

    // 1. open creates tables ---------------------------------------------------

    #[test]
    fn test_open_creates_tables() {
        let store = test_store();
        let count: i64 = store
            .conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='observations'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 1, "observations table must exist");
    }

    // 2. save and search -------------------------------------------------------

    #[test]
    fn test_save_and_search_observation() {
        let store = test_store();
        seed_session(&store, "s1", "proj-a");

        let id = store
            .save_observation(make_obs(
                "s1",
                "proj-a",
                "Retry logic",
                "Exponential backoff works",
            ))
            .unwrap();
        assert!(id > 0);

        let results = store.search("retry", None, 10).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "Retry logic");
    }

    // 3. search filters by project ---------------------------------------------

    #[test]
    fn test_search_filters_by_project() {
        let store = test_store();
        seed_session(&store, "s1", "alpha");
        seed_session(&store, "s2", "beta");

        store
            .save_observation(make_obs(
                "s1",
                "alpha",
                "Alpha note",
                "Deploy pipeline details",
            ))
            .unwrap();
        store
            .save_observation(make_obs(
                "s2",
                "beta",
                "Beta note",
                "Deploy pipeline details",
            ))
            .unwrap();

        let results = store.search("deploy", Some("alpha"), 10).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].project, "alpha");
    }

    // 4. topic_key upsert ------------------------------------------------------

    #[test]
    fn test_topic_key_upsert() {
        let store = test_store();
        seed_session(&store, "s1", "proj");

        let obs1 = NewObservation {
            session_id: "s1".to_string(),
            obs_type: "learning".to_string(),
            title: "WAF rules v1".to_string(),
            content: "Initial findings".to_string(),
            project: "proj".to_string(),
            scope: Some("project".to_string()),
            topic_key: Some("waf-rules".to_string()),
            source: "test".to_string(),
        };
        let id1 = store.save_observation(obs1).unwrap();

        let obs2 = NewObservation {
            session_id: "s1".to_string(),
            obs_type: "learning".to_string(),
            title: "WAF rules v2".to_string(),
            content: "Updated findings".to_string(),
            project: "proj".to_string(),
            scope: Some("project".to_string()),
            topic_key: Some("waf-rules".to_string()),
            source: "test".to_string(),
        };
        let id2 = store.save_observation(obs2).unwrap();

        // Same row was updated.
        assert_eq!(id1, id2);

        // Only one row exists.
        let count: i64 = store
            .conn
            .query_row(
                "SELECT COUNT(*) FROM observations WHERE deleted_at IS NULL",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);

        // revision_count is 2.
        let rev: i64 = store
            .conn
            .query_row(
                "SELECT revision_count FROM observations WHERE id = ?1",
                params![id1],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(rev, 2);
    }

    // 5. soft delete -----------------------------------------------------------

    #[test]
    fn test_soft_delete() {
        let store = test_store();
        seed_session(&store, "s1", "proj");

        let id = store
            .save_observation(make_obs(
                "s1",
                "proj",
                "Secret finding",
                "Sensitive content here",
            ))
            .unwrap();

        // Searchable before delete.
        assert_eq!(store.search("secret", None, 10).unwrap().len(), 1);

        store.soft_delete(id).unwrap();

        // Not searchable after soft delete.
        assert_eq!(store.search("secret", None, 10).unwrap().len(), 0);

        // Not in recent_context either.
        assert_eq!(store.recent_context("proj", 10).unwrap().len(), 0);
    }

    // 6. recent_context --------------------------------------------------------

    #[test]
    fn test_recent_context() {
        let store = test_store();
        seed_session(&store, "s1", "proj");

        // Insert 5 observations. Because iso_now() has second resolution and
        // these run fast, manually set created_at to guarantee ordering.
        for i in 0..5 {
            store
                .conn
                .execute(
                    "INSERT INTO observations
                        (session_id, type, title, content, project, scope, source,
                         created_at, updated_at)
                     VALUES (?1, 'learning', ?2, 'body', 'proj', 'project', 'test',
                             ?3, ?3)",
                    params![
                        "s1",
                        format!("Note {i}"),
                        format!("2026-03-16T00:00:{:02}Z", i),
                    ],
                )
                .unwrap();
        }

        let recent = store.recent_context("proj", 3).unwrap();
        assert_eq!(recent.len(), 3);
        // Most recent first.
        assert_eq!(recent[0].title, "Note 4");
        assert_eq!(recent[1].title, "Note 3");
        assert_eq!(recent[2].title, "Note 2");
    }

    // 7. stats -----------------------------------------------------------------

    #[test]
    fn test_stats() {
        let store = test_store();
        seed_session(&store, "s1", "alpha");
        seed_session(&store, "s2", "beta");

        store
            .save_observation(make_obs("s1", "alpha", "A1", "content"))
            .unwrap();
        store
            .save_observation(make_obs("s1", "alpha", "A2", "content"))
            .unwrap();
        store
            .save_observation(make_obs("s2", "beta", "B1", "content"))
            .unwrap();

        let stats = store.stats().unwrap();
        assert_eq!(stats.total_sessions, 2);
        assert_eq!(stats.total_observations, 3);
        assert_eq!(stats.projects, vec!["alpha", "beta"]);
    }

    // 8. FTS5 special character safety -------------------------------------------

    #[test]
    fn test_search_special_chars_safe() {
        let store = test_store();
        seed_session(&store, "s1", "proj");
        store
            .save_observation(make_obs("s1", "proj", "Normal title", "Normal content"))
            .unwrap();

        // These should not cause FTS5 parse errors.
        assert!(store
            .search("title:secret OR content:password", None, 10)
            .is_ok());
        assert!(store.search("test AND \"quoted\"", None, 10).is_ok());
        assert!(store.search("test*", None, 10).is_ok());
        assert!(store.search("", None, 10).is_ok());
    }
}
