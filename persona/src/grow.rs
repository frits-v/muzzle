//! Auto-grow: generate new personas from the seed vocabulary when the pool runs dry.

use rusqlite::{params, Connection, Result};
use std::collections::HashMap;
use std::time::SystemTime;

use crate::seed::SeedMeta;

// ---------------------------------------------------------------------------
// Minimal deterministic PRNG (xorshift64)
// ---------------------------------------------------------------------------

/// A simple xorshift64 pseudo-random number generator.
pub struct Rng(pub u64);

impl Rng {
    /// Seed from current time (nanoseconds | 1 to avoid zero seed).
    pub fn from_time() -> Self {
        let nanos = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64;
        Rng(nanos | 1)
    }

    /// Advance the state and return the next pseudo-random value.
    pub fn step(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    /// Return a value in `[0, max)`.
    pub fn range(&mut self, max: usize) -> usize {
        if max == 0 {
            return 0;
        }
        (self.step() as usize) % max
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Pick `n` distinct items from `pool` using `rng`. Returns fewer items if
/// the pool is smaller than `n`.
fn pick_n_distinct<'a>(pool: &'a [String], n: usize, rng: &mut Rng) -> Vec<&'a String> {
    if pool.is_empty() || n == 0 {
        return Vec::new();
    }
    let take = n.min(pool.len());
    let mut indices: Vec<usize> = (0..pool.len()).collect();
    // Partial Fisher-Yates: shuffle only the first `take` positions.
    for i in 0..take {
        let j = i + rng.range(pool.len() - i);
        indices.swap(i, j);
    }
    indices[..take].iter().map(|&i| &pool[i]).collect()
}

/// Weighted pick: pick 1–2 expertise tags where underrepresented tags have
/// higher weight (`weight = 1 / (count + 1)`).
fn pick_expertise_weighted(
    vocab: &[String],
    counts: &HashMap<String, usize>,
    rng: &mut Rng,
) -> Vec<String> {
    if vocab.is_empty() {
        return Vec::new();
    }

    // Build a cumulative weight table.
    let weights: Vec<f64> = vocab
        .iter()
        .map(|tag| 1.0 / (counts.get(tag).copied().unwrap_or(0) as f64 + 1.0))
        .collect();
    let total_weight: f64 = weights.iter().sum();

    let pick_one = |rng: &mut Rng, exclude: Option<usize>| -> usize {
        // Adjusted total when one index is excluded.
        let adj_total = if let Some(ex) = exclude {
            total_weight - weights[ex]
        } else {
            total_weight
        };

        // Map a uniform [0,1) value to a weighted index.
        let r = (rng.step() as f64 / u64::MAX as f64) * adj_total;
        let mut cum = 0.0;
        for (i, &w) in weights.iter().enumerate() {
            if Some(i) == exclude {
                continue;
            }
            cum += w;
            if r <= cum {
                return i;
            }
        }
        // Fallback: last non-excluded index.
        (0..vocab.len()).rfind(|&i| Some(i) != exclude).unwrap_or(0)
    };

    // Always pick at least 1; pick 2 if vocab has at least 2 entries.
    let first = pick_one(rng, None);
    if vocab.len() < 2 {
        return vec![vocab[first].clone()];
    }
    let second = pick_one(rng, Some(first));
    vec![vocab[first].clone(), vocab[second].clone()]
}

// ---------------------------------------------------------------------------
// Grow function
// ---------------------------------------------------------------------------

/// Generate `count` new active personas drawn from the seed vocabulary.
///
/// Returns the number of personas actually inserted (may be fewer than
/// `count` if unique names could not be found within the retry budget).
pub fn grow(conn: &Connection, meta: &SeedMeta, count: usize, rng: &mut Rng) -> Result<usize> {
    if meta.first_names.is_empty() || meta.last_names.is_empty() {
        return Ok(0);
    }

    // Build current expertise counts for weighted selection.
    let mut expertise_counts: HashMap<String, usize> = HashMap::new();
    {
        let mut stmt = conn.prepare("SELECT expertise FROM personas WHERE status = 'active'")?;
        let rows = stmt.query_map([], |row| {
            let json: String = row.get(0)?;
            Ok(json)
        })?;
        for row in rows {
            let json = row?;
            let tags: Vec<String> = serde_json::from_str(&json).unwrap_or_default();
            for tag in tags {
                *expertise_counts.entry(tag).or_insert(0) += 1;
            }
        }
    }

    let now = crate::seed::now_iso8601();
    let mut inserted = 0usize;

    for _ in 0..count {
        // Try up to 10 times to generate a unique name.
        let mut candidate_name: Option<String> = None;
        for _ in 0..10 {
            let first = &meta.first_names[rng.range(meta.first_names.len())];
            let last = &meta.last_names[rng.range(meta.last_names.len())];
            let name = format!("{first} {last}");

            let exists: i64 = conn.query_row(
                "SELECT COUNT(*) FROM personas WHERE name = ?1",
                params![name],
                |r| r.get(0),
            )?;
            if exists == 0 {
                candidate_name = Some(name);
                break;
            }
        }

        let name = match candidate_name {
            Some(n) => n,
            None => continue, // Could not find a unique name; skip this slot.
        };

        // Pick 2 traits (or fewer if vocabulary is small).
        let trait_refs = pick_n_distinct(&meta.trait_vocabulary, 2, rng);
        let traits: Vec<String> = trait_refs.into_iter().cloned().collect();

        // Pick 1–2 expertise tags with weighted selection.
        let expertise = pick_expertise_weighted(&meta.expertise_vocabulary, &expertise_counts, rng);

        // Update local counts for subsequent iterations.
        for tag in &expertise {
            *expertise_counts.entry(tag.clone()).or_insert(0) += 1;
        }

        let traits_json = serde_json::to_string(&traits).unwrap_or_else(|_| "[]".into());
        let expertise_json = serde_json::to_string(&expertise).unwrap_or_else(|_| "[]".into());

        conn.execute(
            "INSERT INTO personas
                 (name, traits, expertise, role_instructions,
                  affinity_scores, role_counts, status, created_at)
             VALUES (?1, ?2, ?3, '{}', '{}', '{}', 'active', ?4)",
            params![name, traits_json, expertise_json, now],
        )?;
        inserted += 1;
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
    use crate::seed::{insert_seed, parse_seed};
    use rusqlite::Connection;

    const SAMPLE_TOML: &str = r#"
[meta]
version = 1
trait_vocabulary = ["pragmatic", "curious", "methodical", "thorough"]
expertise_vocabulary = ["backend", "security", "testing", "frontend"]
role_vocabulary = ["code-reviewer", "general"]
first_names = ["Alice", "Bob", "Carol", "Dave", "Eve"]
last_names = ["Smith", "Jones", "Davis", "Brown", "Wilson"]

[[personas]]
name = "Alice Smith"
traits = ["pragmatic"]
expertise = ["backend"]
[personas.role_instructions]
general = "Be thorough."
"#;

    fn setup() -> Connection {
        let conn = Connection::open_in_memory().expect("in-memory db");
        ensure_schema(&conn).expect("schema");
        let seed = parse_seed(SAMPLE_TOML).expect("parse");
        insert_seed(&conn, &seed).expect("seed");
        conn
    }

    #[test]
    fn grow_one_persona() {
        let conn = setup();
        let seed = parse_seed(SAMPLE_TOML).expect("parse");
        let mut rng = Rng(12345);

        let added = grow(&conn, &seed.meta, 1, &mut rng).expect("grow");
        assert_eq!(added, 1, "should have grown 1 persona");

        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM personas", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 2, "total personas should be 2");
    }

    #[test]
    fn grown_persona_has_valid_traits() {
        let conn = setup();
        let seed = parse_seed(SAMPLE_TOML).expect("parse");
        let mut rng = Rng(12345);

        grow(&conn, &seed.meta, 1, &mut rng).expect("grow");

        // Fetch the newly grown persona (id > 1 since seed persona has id=1).
        let (traits_json, expertise_json, status): (String, String, String) = conn
            .query_row(
                "SELECT traits, expertise, status FROM personas ORDER BY id DESC LIMIT 1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();

        let traits: Vec<String> = serde_json::from_str(&traits_json).unwrap();
        let expertise: Vec<String> = serde_json::from_str(&expertise_json).unwrap();

        assert!(
            !traits.is_empty(),
            "grown persona must have at least one trait"
        );
        assert!(
            !expertise.is_empty(),
            "grown persona must have at least one expertise tag"
        );
        assert_eq!(status, "active", "grown persona must be active");

        // All traits must be from the vocabulary.
        for t in &traits {
            assert!(
                seed.meta.trait_vocabulary.contains(t),
                "trait {t} not in vocabulary"
            );
        }
        // All expertise tags must be from the vocabulary.
        for e in &expertise {
            assert!(
                seed.meta.expertise_vocabulary.contains(e),
                "expertise {e} not in vocabulary"
            );
        }
    }
}
