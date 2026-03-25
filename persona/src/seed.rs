//! Seed file parsing and database insertion for muzzle-persona.

use rusqlite::{params, Connection, Result};
use serde::Deserialize;
use std::collections::HashMap;
use std::time::SystemTime;

// ---------------------------------------------------------------------------
// Seed file types
// ---------------------------------------------------------------------------

/// Top-level seed file structure.
#[derive(Debug, Deserialize)]
pub struct SeedFile {
    pub meta: SeedMeta,
    #[serde(default)]
    pub personas: Vec<SeedPersona>,
}

/// Metadata block from the seed file.
#[derive(Debug, Deserialize)]
pub struct SeedMeta {
    pub version: u32,
    #[serde(default)]
    pub trait_vocabulary: Vec<String>,
    #[serde(default)]
    pub expertise_vocabulary: Vec<String>,
    #[serde(default)]
    pub role_vocabulary: Vec<String>,
    #[serde(default)]
    pub first_names: Vec<String>,
    #[serde(default)]
    pub last_names: Vec<String>,
}

/// A single persona entry in the seed file.
#[derive(Debug, Deserialize)]
pub struct SeedPersona {
    pub name: String,
    #[serde(default)]
    pub traits: Vec<String>,
    #[serde(default)]
    pub expertise: Vec<String>,
    #[serde(default)]
    pub role_instructions: HashMap<String, String>,
}

// ---------------------------------------------------------------------------
// Public functions
// ---------------------------------------------------------------------------

/// Parse a TOML seed file string into a [`SeedFile`].
pub fn parse_seed(toml_str: &str) -> Result<SeedFile, Box<dyn std::error::Error>> {
    let seed: SeedFile = toml::from_str(toml_str)?;
    Ok(seed)
}

/// Insert personas from a [`SeedFile`] using `INSERT OR IGNORE`.
///
/// Returns the number of rows actually inserted (already-existing rows are
/// silently skipped).
pub fn insert_seed(conn: &Connection, seed: &SeedFile) -> Result<usize> {
    let now = now_iso8601();
    let mut inserted = 0usize;

    for persona in &seed.personas {
        let traits_json = serde_json::to_string(&persona.traits).unwrap_or_else(|_| "[]".into());
        let expertise_json =
            serde_json::to_string(&persona.expertise).unwrap_or_else(|_| "[]".into());
        let role_instructions_json =
            serde_json::to_string(&persona.role_instructions).unwrap_or_else(|_| "{}".into());

        let rows = conn.execute(
            "INSERT OR IGNORE INTO personas
                 (name, traits, expertise, role_instructions,
                  affinity_scores, role_counts, status, created_at)
             VALUES (?1, ?2, ?3, ?4, '{}', '{}', 'active', ?5)",
            params![
                persona.name,
                traits_json,
                expertise_json,
                role_instructions_json,
                now,
            ],
        )?;
        inserted += rows;
    }

    Ok(inserted)
}

/// Return the current UTC time as a proper ISO 8601 string.
///
/// Uses the Howard Hinnant civil-calendar algorithm (public domain) to convert
/// Unix epoch seconds into a `YYYY-MM-DDTHH:MM:SSZ` string without depending
/// on any external time library.
pub fn now_iso8601() -> String {
    let dur = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = dur.as_secs();

    let days = secs / 86400;
    let time_of_day = secs % 86400;
    let hours = time_of_day / 3600;
    let minutes = (time_of_day % 3600) / 60;
    let seconds = time_of_day % 60;

    let (year, month, day) = days_to_ymd(days);

    format!("{year:04}-{month:02}-{day:02}T{hours:02}:{minutes:02}:{seconds:02}Z")
}

fn days_to_ymd(mut days: u64) -> (u64, u64, u64) {
    // Howard Hinnant algorithm (public domain).
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
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::ensure_schema;
    use rusqlite::Connection;

    const SAMPLE_TOML: &str = r#"
[meta]
version = 1
trait_vocabulary = ["pragmatic", "curious"]
expertise_vocabulary = ["backend", "security"]
role_vocabulary = ["code-reviewer", "general"]
first_names = ["Alice", "Bob"]
last_names = ["Smith", "Jones"]

[[personas]]
name = "Alice Smith"
traits = ["pragmatic"]
expertise = ["backend"]
[personas.role_instructions]
code-reviewer = "Focus on correctness."

[[personas]]
name = "Bob Jones"
traits = ["curious"]
expertise = ["security"]
[personas.role_instructions]
general = "Be thorough."
"#;

    fn in_memory_with_schema() -> Connection {
        let conn = Connection::open_in_memory().expect("open in-memory db");
        ensure_schema(&conn).expect("schema");
        conn
    }

    #[test]
    fn parse_seed_from_str() {
        let seed = parse_seed(SAMPLE_TOML).expect("parse should succeed");
        assert_eq!(seed.meta.version, 1);
        assert_eq!(seed.personas.len(), 2);
        assert_eq!(seed.personas[0].name, "Alice Smith");
        assert_eq!(seed.personas[1].expertise, vec!["security"]);
    }

    #[test]
    fn load_seed_into_db() {
        let conn = in_memory_with_schema();
        let seed = parse_seed(SAMPLE_TOML).expect("parse");
        let inserted = insert_seed(&conn, &seed).expect("insert");
        assert_eq!(inserted, 2, "both personas should be inserted");

        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM personas", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 2);
    }

    #[test]
    fn insert_seed_is_idempotent() {
        let conn = in_memory_with_schema();
        let seed = parse_seed(SAMPLE_TOML).expect("parse");

        let first = insert_seed(&conn, &seed).expect("first insert");
        assert_eq!(first, 2);

        let second = insert_seed(&conn, &seed).expect("second insert");
        assert_eq!(second, 0, "duplicate inserts should be ignored");

        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM personas", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 2, "row count must not grow on duplicate inserts");
    }
}
