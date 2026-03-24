//! Auto-grow algorithm: synthesise new personas from seed name pools and
//! vocabulary lists when the broker pool is exhausted.

use rusqlite::{Connection, Result};
use std::collections::HashMap;

use crate::seed::{now_iso8601, SeedMeta};

// ---------------------------------------------------------------------------
// Simple PRNG (no `rand` crate dependency)
// ---------------------------------------------------------------------------

/// Minimal xorshift64 PRNG — sufficient for persona generation.
pub struct Rng(pub u64);

impl Rng {
    /// Seed from wall-clock nanoseconds.
    pub fn from_time() -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64;
        Self(nanos | 1) // ensure non-zero seed
    }

    /// Produce the next pseudorandom `u64` (xorshift64 step).
    pub fn step(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    /// Uniform integer in `[0, max)`.
    pub fn range(&mut self, max: usize) -> usize {
        (self.step() as usize) % max
    }
}

// ---------------------------------------------------------------------------
// Weighted expertise selection helpers
// ---------------------------------------------------------------------------

/// Count how many active personas carry each expertise tag.
fn expertise_counts(conn: &Connection, vocab: &[String]) -> Result<HashMap<String, u32>> {
    let mut counts: HashMap<String, u32> = vocab.iter().map(|t| (t.clone(), 0)).collect();

    let mut stmt = conn.prepare("SELECT expertise FROM personas WHERE status = 'active'")?;
    let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;

    for row in rows {
        let expertise_json = row?;
        let tags: Vec<String> = serde_json::from_str(&expertise_json).unwrap_or_default();
        for tag in tags {
            if let Some(c) = counts.get_mut(&tag) {
                *c += 1;
            }
        }
    }

    Ok(counts)
}

/// Pick 1 or 2 expertise tags using weight = 1/(count+1) for each tag.
fn pick_expertise(rng: &mut Rng, vocab: &[String], counts: &HashMap<String, u32>) -> Vec<String> {
    if vocab.is_empty() {
        return vec![];
    }

    // Build cumulative weight table.
    let weights: Vec<f64> = vocab
        .iter()
        .map(|t| {
            let c = counts.get(t).copied().unwrap_or(0);
            1.0 / (c as f64 + 1.0)
        })
        .collect();

    let total: f64 = weights.iter().sum();

    let pick_one = |rng: &mut Rng, exclude: Option<usize>| -> usize {
        // Recompute total excluding the excluded index if needed.
        let eff_total: f64 = weights
            .iter()
            .enumerate()
            .filter(|(i, _)| Some(*i) != exclude)
            .map(|(_, w)| w)
            .sum();
        if eff_total == 0.0 {
            return exclude.map(|e| if e == 0 { 1 } else { 0 }).unwrap_or(0);
        }
        // Scale a random u64 into [0, eff_total).
        let r = (rng.step() as f64 / u64::MAX as f64) * eff_total;
        let mut acc = 0.0;
        for (i, w) in weights.iter().enumerate() {
            if Some(i) == exclude {
                continue;
            }
            acc += w;
            if r < acc {
                return i;
            }
        }
        // Fallback to last non-excluded index.
        weights
            .iter()
            .enumerate()
            .filter(|(i, _)| Some(*i) != exclude)
            .next_back()
            .map(|(i, _)| i)
            .unwrap_or(0)
    };

    // Pick 1 or 2 tags: pick 2 when total weight supports it and RNG says so.
    // Use 2 tags ~50% of the time, but only when vocab has at least 2 entries.
    let first_idx = pick_one(rng, None);
    if vocab.len() >= 2 && (total > 0.0) && (rng.step() % 2 == 0) {
        let second_idx = pick_one(rng, Some(first_idx));
        vec![vocab[first_idx].clone(), vocab[second_idx].clone()]
    } else {
        vec![vocab[first_idx].clone()]
    }
}

// ---------------------------------------------------------------------------
// Public grow function
// ---------------------------------------------------------------------------

/// Grow `count` new personas from the seed meta's name pools and vocabularies.
///
/// Returns the number of personas actually inserted.
pub fn grow(conn: &Connection, meta: &SeedMeta, count: usize, rng: &mut Rng) -> Result<usize> {
    if meta.first_names.is_empty() || meta.last_names.is_empty() {
        return Err(rusqlite::Error::QueryReturnedNoRows);
    }
    if meta.trait_vocabulary.is_empty() || meta.expertise_vocabulary.is_empty() {
        return Err(rusqlite::Error::QueryReturnedNoRows);
    }

    let ts = now_iso8601();
    let mut inserted = 0usize;

    for _ in 0..count {
        // 1. Generate a unique name (max 10 attempts).
        let name = find_unique_name(conn, meta, rng)?;

        // 2. Pick 2 traits at random (without repetition).
        let trait1_idx = rng.range(meta.trait_vocabulary.len());
        let trait2_idx = {
            let mut idx = rng.range(meta.trait_vocabulary.len());
            let mut attempts = 0usize;
            while idx == trait1_idx && meta.trait_vocabulary.len() > 1 && attempts < 10 {
                idx = rng.range(meta.trait_vocabulary.len());
                attempts += 1;
            }
            idx
        };
        let traits = vec![
            meta.trait_vocabulary[trait1_idx].clone(),
            meta.trait_vocabulary[trait2_idx].clone(),
        ];

        // 3. Pick 1-2 expertise tags, weighted toward underrepresented tags.
        let counts = expertise_counts(conn, &meta.expertise_vocabulary)?;
        let expertise = pick_expertise(rng, &meta.expertise_vocabulary, &counts);

        // 4. role_instructions is empty for auto-grown personas.
        let role_instructions: HashMap<String, String> = HashMap::new();

        // 5. Serialize and INSERT.
        let traits_json = serde_json::to_string(&traits)
            .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
        let expertise_json = serde_json::to_string(&expertise)
            .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
        let role_instructions_json = serde_json::to_string(&role_instructions)
            .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;

        conn.execute(
            "INSERT INTO personas (name, traits, expertise, role_instructions, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![
                name,
                traits_json,
                expertise_json,
                role_instructions_json,
                ts
            ],
        )?;
        inserted += 1;
    }

    Ok(inserted)
}

/// Try to generate a name that doesn't already exist in the personas table.
/// Returns an error after 10 failed attempts.
fn find_unique_name(conn: &Connection, meta: &SeedMeta, rng: &mut Rng) -> Result<String> {
    for _ in 0..10 {
        let first = &meta.first_names[rng.range(meta.first_names.len())];
        let last = &meta.last_names[rng.range(meta.last_names.len())];
        let name = format!("{first} {last}");
        let exists: i64 = conn.query_row(
            "SELECT COUNT(*) FROM personas WHERE name = ?1",
            rusqlite::params![name],
            |row| row.get(0),
        )?;
        if exists == 0 {
            return Ok(name);
        }
    }
    Err(rusqlite::Error::QueryReturnedNoRows)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{schema, seed};
    use rusqlite::Connection;

    #[test]
    fn grow_one_persona() {
        let conn = Connection::open_in_memory().unwrap();
        schema::ensure_schema(&conn).unwrap();
        let toml_str = include_str!("../personas-seed.toml");
        let seed_data = seed::parse_seed(toml_str).unwrap();
        seed::insert_seed(&conn, &seed_data).unwrap();

        let before: i64 = conn
            .query_row("SELECT COUNT(*) FROM personas", [], |r| r.get(0))
            .unwrap();
        grow(&conn, &seed_data.meta, 1, &mut Rng(12345)).unwrap();
        let after: i64 = conn
            .query_row("SELECT COUNT(*) FROM personas", [], |r| r.get(0))
            .unwrap();
        assert_eq!(after, before + 1);
    }

    #[test]
    fn grown_persona_has_valid_traits() {
        let conn = Connection::open_in_memory().unwrap();
        schema::ensure_schema(&conn).unwrap();
        let toml_str = include_str!("../personas-seed.toml");
        let seed_data = seed::parse_seed(toml_str).unwrap();
        seed::insert_seed(&conn, &seed_data).unwrap();

        grow(&conn, &seed_data.meta, 1, &mut Rng(12345)).unwrap();

        let (traits_json, expertise_json): (String, String) = conn
            .query_row(
                "SELECT traits, expertise FROM personas ORDER BY id DESC LIMIT 1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        let traits: Vec<String> = serde_json::from_str(&traits_json).unwrap();
        let expertise: Vec<String> = serde_json::from_str(&expertise_json).unwrap();

        assert_eq!(traits.len(), 2);
        assert!(!expertise.is_empty());
        for t in &traits {
            assert!(seed_data.meta.trait_vocabulary.contains(t));
        }
        for e in &expertise {
            assert!(seed_data.meta.expertise_vocabulary.contains(e));
        }
    }
}
