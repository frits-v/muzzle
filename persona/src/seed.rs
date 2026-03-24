//! TOML seed file parser and idempotent database loader for personas.

use rusqlite::{Connection, Result};
use serde::Deserialize;
use std::collections::HashMap;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct SeedFile {
    pub meta: SeedMeta,
    pub personas: Vec<SeedPersona>,
}

#[derive(Debug, Deserialize)]
pub struct SeedMeta {
    pub version: u32,
    pub trait_vocabulary: Vec<String>,
    pub expertise_vocabulary: Vec<String>,
    pub role_vocabulary: Vec<String>,
    pub first_names: Vec<String>,
    pub last_names: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct SeedPersona {
    pub name: String,
    pub traits: Vec<String>,
    pub expertise: Vec<String>,
    #[serde(default)]
    pub role_instructions: HashMap<String, String>,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn now_iso8601() -> String {
    let dur = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = dur.as_secs();
    let days = secs / 86400;
    let day_secs = secs % 86400;
    let hours = day_secs / 3600;
    let mins = (day_secs % 3600) / 60;
    let s = day_secs % 60;
    let z = days as i64 + 719_468;
    let era = z / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}T{hours:02}:{mins:02}:{s:02}Z")
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Parse a TOML string into a `SeedFile`.
pub fn parse_seed(toml_str: &str) -> Result<SeedFile, toml::de::Error> {
    toml::from_str(toml_str)
}

/// Insert seed personas into the database using INSERT OR IGNORE.
///
/// Returns the number of rows actually inserted (rows skipped due to UNIQUE
/// constraint are not counted).
pub fn insert_seed(conn: &Connection, seed: &SeedFile) -> Result<usize> {
    let ts = now_iso8601();
    let mut inserted = 0usize;

    for persona in &seed.personas {
        let traits_json = serde_json::to_string(&persona.traits)
            .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
        let expertise_json = serde_json::to_string(&persona.expertise)
            .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
        let role_instructions_json = serde_json::to_string(&persona.role_instructions)
            .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;

        let rows = conn.execute(
            "INSERT OR IGNORE INTO personas
                (name, traits, expertise, role_instructions, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![
                persona.name,
                traits_json,
                expertise_json,
                role_instructions_json,
                ts
            ],
        )?;
        inserted += rows;
    }

    Ok(inserted)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::ensure_schema;

    const SEED_TOML: &str = include_str!("../personas-seed.toml");

    #[test]
    fn parse_seed_from_str() {
        let seed = parse_seed(SEED_TOML).expect("TOML should parse");
        assert_eq!(seed.meta.version, 1);
        assert!(!seed.meta.trait_vocabulary.is_empty());
        assert!(!seed.meta.expertise_vocabulary.is_empty());
        assert!(!seed.meta.role_vocabulary.is_empty());
        assert!(!seed.meta.first_names.is_empty());
        assert!(!seed.meta.last_names.is_empty());
        assert_eq!(seed.personas.len(), 5);
        assert_eq!(seed.personas[0].name, "Elena Vasquez");
        assert!(seed.personas[0].traits.contains(&"pragmatic".to_string()));
        assert!(seed.personas[0].expertise.contains(&"security".to_string()));
        assert!(seed.personas[0]
            .role_instructions
            .contains_key("security-review"));
    }

    #[test]
    fn load_seed_into_db() {
        let conn = Connection::open_in_memory().unwrap();
        ensure_schema(&conn).unwrap();
        let seed = parse_seed(SEED_TOML).unwrap();
        let count = insert_seed(&conn, &seed).unwrap();
        assert_eq!(count, seed.personas.len(), "all personas should be inserted");
    }

    #[test]
    fn insert_seed_is_idempotent() {
        let conn = Connection::open_in_memory().unwrap();
        ensure_schema(&conn).unwrap();
        let seed = parse_seed(SEED_TOML).unwrap();
        insert_seed(&conn, &seed).unwrap();
        let second = insert_seed(&conn, &seed).unwrap();
        assert_eq!(second, 0, "second insert should insert 0 rows");
    }
}
