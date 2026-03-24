use muzzle_persona::seed::now_iso8601;
use muzzle_persona::{broker, grow, preamble, release, schema, seed};
use rusqlite::Connection;
use std::path::PathBuf;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    // args[0] = binary name; args[1] = subcommand
    match args.get(1).map(|s| s.as_str()) {
        Some("assign") => {
            if let Err(e) = run_assign(&args[2..]) {
                eprintln!("error: {e}");
                std::process::exit(1);
            }
        }
        Some("release") => {
            if let Err(e) = run_release(&args[2..]) {
                eprintln!("error: {e}");
                std::process::exit(1);
            }
        }
        Some("list") => {
            if let Err(e) = run_list(&args[2..]) {
                eprintln!("error: {e}");
                std::process::exit(1);
            }
        }
        Some("show") => {
            if let Err(e) = run_show(&args[2..]) {
                eprintln!("error: {e}");
                std::process::exit(1);
            }
        }
        Some("history") => {
            if let Err(e) = run_history(&args[2..]) {
                eprintln!("error: {e}");
                std::process::exit(1);
            }
        }
        Some("feedback") => {
            if let Err(e) = run_feedback(&args[2..]) {
                eprintln!("error: {e}");
                std::process::exit(1);
            }
        }
        Some("search") => {
            if let Err(e) = run_search(&args[2..]) {
                eprintln!("error: {e}");
                std::process::exit(1);
            }
        }
        Some("retire") => {
            if let Err(e) = run_retire(&args[2..]) {
                eprintln!("error: {e}");
                std::process::exit(1);
            }
        }
        Some("reactivate") => {
            if let Err(e) = run_reactivate(&args[2..]) {
                eprintln!("error: {e}");
                std::process::exit(1);
            }
        }
        Some("grow") => {
            if let Err(e) = run_grow(&args[2..]) {
                eprintln!("error: {e}");
                std::process::exit(1);
            }
        }
        Some("compact") => {
            if let Err(e) = run_compact(&args[2..]) {
                eprintln!("error: {e}");
                std::process::exit(1);
            }
        }
        Some("stats") => {
            if let Err(e) = run_stats(&args[2..]) {
                eprintln!("error: {e}");
                std::process::exit(1);
            }
        }
        Some("seed") => {
            if let Err(e) = run_seed(&args[2..]) {
                eprintln!("error: {e}");
                std::process::exit(1);
            }
        }
        Some("orphan-cleanup") => {
            if let Err(e) = run_orphan_cleanup() {
                eprintln!("error: {e}");
                std::process::exit(1);
            }
        }
        Some(cmd) => {
            eprintln!("error: unknown subcommand '{cmd}'");
            std::process::exit(1);
        }
        None => {
            eprintln!("error: no subcommand given");
            std::process::exit(1);
        }
    }
}

// ---------------------------------------------------------------------------
// Argument parsing helpers
// ---------------------------------------------------------------------------

fn parse_arg(args: &[String], key: &str) -> Option<String> {
    args.iter()
        .find(|a| a.starts_with(&format!("--{key}=")))
        .map(|a| a[key.len() + 3..].to_string())
}

/// Return the first positional argument (element that doesn't start with `--`).
fn positional(args: &[String]) -> Option<&str> {
    args.iter()
        .find(|a| !a.starts_with("--"))
        .map(|s| s.as_str())
}

// ---------------------------------------------------------------------------
// DB path
// ---------------------------------------------------------------------------

fn db_path() -> Result<PathBuf, String> {
    let home = std::env::var("HOME").map_err(|_| "HOME env var not set".to_string())?;
    Ok(PathBuf::from(home).join(".muzzle").join("memory.db"))
}

fn open_db() -> Result<Connection, String> {
    let path = db_path()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("failed to create DB directory: {e}"))?;
    }
    let conn = Connection::open(&path)
        .map_err(|e| format!("failed to open DB at {}: {e}", path.display()))?;
    schema::ensure_schema(&conn).map_err(|e| format!("schema error: {e}"))?;
    Ok(conn)
}

// ---------------------------------------------------------------------------
// assign subcommand
// ---------------------------------------------------------------------------

fn run_assign(args: &[String]) -> Result<(), String> {
    // 1. Parse required arguments.
    let roles_json = parse_arg(args, "roles").ok_or("--roles=<JSON array> is required")?;
    let project = parse_arg(args, "project").ok_or("--project=<name> is required")?;
    let session_id = parse_arg(args, "session").ok_or("--session=<id> is required")?;
    let agent_name = parse_arg(args, "agent-name").ok_or("--agent-name=<name> is required")?;
    let summon = parse_arg(args, "summon");
    let team_name = parse_arg(args, "team-name");

    // 2. Open (or create) the DB.
    let path = db_path()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("failed to create DB directory: {e}"))?;
    }
    let conn = Connection::open(&path)
        .map_err(|e| format!("failed to open DB at {}: {e}", path.display()))?;

    // 3. Ensure schema.
    schema::ensure_schema(&conn).map_err(|e| format!("schema error: {e}"))?;

    // 4. Seed if the personas table is empty.
    let count: i64 = conn
        .query_row("SELECT count(*) FROM personas", [], |row| row.get(0))
        .map_err(|e| format!("DB query error: {e}"))?;
    if count == 0 {
        let toml_str = include_str!("../personas-seed.toml");
        let seed_data = seed::parse_seed(toml_str).map_err(|e| format!("seed parse error: {e}"))?;
        seed::insert_seed(&conn, &seed_data).map_err(|e| format!("seed insert error: {e}"))?;
    }

    // 5. Parse roles JSON array.
    let roles_vec: Vec<String> = serde_json::from_str(&roles_json)
        .map_err(|e| format!("--roles must be a JSON array of strings: {e}"))?;
    let roles_ref: Vec<&str> = roles_vec.iter().map(|s| s.as_str()).collect();

    // 6. Assign.
    let assignments = broker::assign(
        &conn,
        &roles_ref,
        &project,
        &session_id,
        &agent_name,
        summon.as_deref(),
        team_name.as_deref(),
    )
    .map_err(|e| format!("assign error: {e}"))?;

    // 7. Build output JSON.
    let output: Vec<serde_json::Value> = assignments
        .iter()
        .map(|a| {
            let p = preamble::format_preamble(a);
            serde_json::json!({
                "agent_slot": a.agent_slot,
                "persona_id": a.persona_id,
                "name": a.name,
                "preamble": p,
            })
        })
        .collect();

    println!(
        "{}",
        serde_json::to_string_pretty(&output)
            .map_err(|e| format!("JSON serialization error: {e}"))?
    );

    Ok(())
}

// ---------------------------------------------------------------------------
// release subcommand
// ---------------------------------------------------------------------------

fn run_release(args: &[String]) -> Result<(), String> {
    // 1. Parse required arguments.
    let session_id = parse_arg(args, "session").ok_or("--session=<id> is required")?;

    // 2. Open the DB.
    let path = db_path()?;
    let conn = Connection::open(&path)
        .map_err(|e| format!("failed to open DB at {}: {e}", path.display()))?;

    // 3. Ensure schema.
    schema::ensure_schema(&conn).map_err(|e| format!("schema error: {e}"))?;

    // 4. Release.
    release::release(&conn, &session_id).map_err(|e| format!("release error: {e}"))?;

    eprintln!("released session {session_id}");
    Ok(())
}

// ---------------------------------------------------------------------------
// list subcommand
// ---------------------------------------------------------------------------

fn run_list(args: &[String]) -> Result<(), String> {
    let status_filter = parse_arg(args, "status").unwrap_or_else(|| "active".to_string());
    let expertise_filter = parse_arg(args, "expertise");

    let conn = open_db()?;

    let status_clause = match status_filter.as_str() {
        "all" => "1=1".to_string(),
        "archived" => "status = 'archived'".to_string(),
        _ => "status = 'active'".to_string(),
    };

    let query = format!(
        "SELECT name, status, traits, expertise, last_assigned FROM personas WHERE {status_clause} ORDER BY name"
    );

    let mut stmt = conn
        .prepare(&query)
        .map_err(|e| format!("query error: {e}"))?;

    struct Row {
        name: String,
        status: String,
        traits: String,
        expertise: String,
        last_assigned: Option<String>,
    }

    let rows: Vec<Row> = stmt
        .query_map([], |row| {
            Ok(Row {
                name: row.get(0)?,
                status: row.get(1)?,
                traits: row.get(2)?,
                expertise: row.get(3)?,
                last_assigned: row.get(4)?,
            })
        })
        .map_err(|e| format!("query error: {e}"))?
        .filter_map(|r| r.ok())
        .filter(|r| {
            if let Some(ref tag) = expertise_filter {
                let tags: Vec<String> = serde_json::from_str(&r.expertise).unwrap_or_default();
                tags.iter().any(|t| t == tag)
            } else {
                true
            }
        })
        .collect();

    if rows.is_empty() {
        println!("(no personas found)");
        return Ok(());
    }

    println!(
        "{:<25} {:<10} {:<30} {:<30} LAST_ASSIGNED",
        "NAME", "STATUS", "TRAITS", "EXPERTISE"
    );
    println!("{}", "-".repeat(110));
    for r in &rows {
        let traits: Vec<String> = serde_json::from_str(&r.traits).unwrap_or_default();
        let expertise: Vec<String> = serde_json::from_str(&r.expertise).unwrap_or_default();
        println!(
            "{:<25} {:<10} {:<30} {:<30} {}",
            r.name,
            r.status,
            traits.join(", "),
            expertise.join(", "),
            r.last_assigned.as_deref().unwrap_or("-"),
        );
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// show subcommand
// ---------------------------------------------------------------------------

fn run_show(args: &[String]) -> Result<(), String> {
    let name = positional(args).ok_or("name argument is required")?;

    let conn = open_db()?;

    struct Row {
        id: i64,
        name: String,
        traits: String,
        expertise: String,
        role_instructions: String,
        affinity_scores: String,
        role_counts: String,
        status: String,
        assigned_to_session: Option<String>,
        created_at: String,
        last_assigned: Option<String>,
    }

    let row: Row = conn
        .query_row(
            "SELECT id, name, traits, expertise, role_instructions, affinity_scores, role_counts,
                    status, assigned_to_session, created_at, last_assigned
             FROM personas WHERE name = ?1",
            rusqlite::params![name],
            |row| {
                Ok(Row {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    traits: row.get(2)?,
                    expertise: row.get(3)?,
                    role_instructions: row.get(4)?,
                    affinity_scores: row.get(5)?,
                    role_counts: row.get(6)?,
                    status: row.get(7)?,
                    assigned_to_session: row.get(8)?,
                    created_at: row.get(9)?,
                    last_assigned: row.get(10)?,
                })
            },
        )
        .map_err(|_| format!("persona not found: {name}"))?;

    let traits: Vec<String> = serde_json::from_str(&row.traits).unwrap_or_default();
    let expertise: Vec<String> = serde_json::from_str(&row.expertise).unwrap_or_default();
    let affinity: serde_json::Value =
        serde_json::from_str(&row.affinity_scores).unwrap_or(serde_json::Value::Null);
    let role_counts: serde_json::Value =
        serde_json::from_str(&row.role_counts).unwrap_or(serde_json::Value::Null);
    let role_instructions: serde_json::Value =
        serde_json::from_str(&row.role_instructions).unwrap_or(serde_json::Value::Null);

    println!("id:                  {}", row.id);
    println!("name:                {}", row.name);
    println!("status:              {}", row.status);
    println!("traits:              {}", traits.join(", "));
    println!("expertise:           {}", expertise.join(", "));
    println!("created_at:          {}", row.created_at);
    println!(
        "last_assigned:       {}",
        row.last_assigned.as_deref().unwrap_or("-")
    );
    println!(
        "assigned_to_session: {}",
        row.assigned_to_session.as_deref().unwrap_or("-")
    );
    println!("affinity_scores:     {affinity}");
    println!("role_counts:         {role_counts}");
    println!("role_instructions:   {role_instructions}");

    Ok(())
}

// ---------------------------------------------------------------------------
// history subcommand
// ---------------------------------------------------------------------------

fn run_history(args: &[String]) -> Result<(), String> {
    let name = positional(args).ok_or("name argument is required")?;
    let project_filter = parse_arg(args, "project");
    let limit: i64 = parse_arg(args, "limit")
        .and_then(|s| s.parse().ok())
        .unwrap_or(20);

    let conn = open_db()?;

    let persona_id: i64 = conn
        .query_row(
            "SELECT id FROM personas WHERE name = ?1",
            rusqlite::params![name],
            |row| row.get(0),
        )
        .map_err(|_| format!("persona not found: {name}"))?;

    struct Row {
        role: String,
        project: Option<String>,
        agent_name: String,
        assigned_at: String,
        released_at: Option<String>,
    }

    // Fetch all rows for this persona (filtered by project if given), then truncate.
    let mut stmt = conn
        .prepare(
            "SELECT role, project, agent_name, assigned_at, released_at
             FROM persona_assignments
             WHERE persona_id = ?1
             ORDER BY assigned_at DESC",
        )
        .map_err(|e| format!("query error: {e}"))?;

    let all_rows: Vec<Row> = stmt
        .query_map(rusqlite::params![persona_id], |row| {
            Ok(Row {
                role: row.get(0)?,
                project: row.get(1)?,
                agent_name: row.get(2)?,
                assigned_at: row.get(3)?,
                released_at: row.get(4)?,
            })
        })
        .map_err(|e| format!("query error: {e}"))?
        .filter_map(|r| r.ok())
        .collect();

    let rows: Vec<Row> = all_rows
        .into_iter()
        .filter(|r| {
            if let Some(ref proj) = project_filter {
                r.project.as_deref() == Some(proj.as_str())
            } else {
                true
            }
        })
        .take(limit as usize)
        .collect();

    if rows.is_empty() {
        println!("(no assignment history for {name})");
        return Ok(());
    }

    println!("Assignment history for: {name}");
    println!(
        "{:<20} {:<20} {:<20} {:<25} RELEASED_AT",
        "ROLE", "PROJECT", "AGENT", "ASSIGNED_AT"
    );
    println!("{}", "-".repeat(105));
    for r in &rows {
        println!(
            "{:<20} {:<20} {:<20} {:<25} {}",
            r.role,
            r.project.as_deref().unwrap_or("-"),
            r.agent_name,
            r.assigned_at,
            r.released_at.as_deref().unwrap_or("-"),
        );
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// feedback subcommand
// ---------------------------------------------------------------------------

fn run_feedback(args: &[String]) -> Result<(), String> {
    let name = positional(args).ok_or("name argument is required")?;
    let observation = parse_arg(args, "observation").ok_or("--observation=<text> is required")?;
    let role = parse_arg(args, "role").unwrap_or_else(|| "general".to_string());

    let conn = open_db()?;

    let persona_id: i64 = conn
        .query_row(
            "SELECT id FROM personas WHERE name = ?1",
            rusqlite::params![name],
            |row| row.get(0),
        )
        .map_err(|_| format!("persona not found: {name}"))?;

    let now = now_iso8601();

    conn.execute(
        "INSERT INTO persona_feedback (persona_id, timestamp, project, role, observation, source)
         VALUES (?1, ?2, '', ?3, ?4, 'user')",
        rusqlite::params![persona_id, now, role, observation],
    )
    .map_err(|e| format!("insert error: {e}"))?;

    println!("feedback recorded for {name}");
    Ok(())
}

// ---------------------------------------------------------------------------
// search subcommand
// ---------------------------------------------------------------------------

fn run_search(args: &[String]) -> Result<(), String> {
    let role = parse_arg(args, "role").ok_or("--role=<role> is required")?;
    let limit: usize = parse_arg(args, "limit")
        .and_then(|s| s.parse().ok())
        .unwrap_or(5);

    let conn = open_db()?;

    struct Candidate {
        name: String,
        traits: String,
        expertise: String,
        affinity_scores: String,
        last_assigned: Option<String>,
    }

    let mut stmt = conn
        .prepare(
            "SELECT name, traits, expertise, affinity_scores, last_assigned
             FROM personas
             WHERE status = 'active'
             ORDER BY name",
        )
        .map_err(|e| format!("query error: {e}"))?;

    let candidates: Vec<Candidate> = stmt
        .query_map([], |row| {
            Ok(Candidate {
                name: row.get(0)?,
                traits: row.get(1)?,
                expertise: row.get(2)?,
                affinity_scores: row.get(3)?,
                last_assigned: row.get(4)?,
            })
        })
        .map_err(|e| format!("query error: {e}"))?
        .filter_map(|r| r.ok())
        .collect();

    // Compute scores using same logic as broker.
    use muzzle_persona::types::expertise_for_role;

    struct Scored {
        name: String,
        score: f32,
        expertise: Vec<String>,
        traits: Vec<String>,
    }

    let required_expertise = expertise_for_role(&role);

    let mut scored: Vec<Scored> = candidates
        .into_iter()
        .map(|c| {
            let expertise: Vec<String> = serde_json::from_str(&c.expertise).unwrap_or_default();
            let traits: Vec<String> = serde_json::from_str(&c.traits).unwrap_or_default();
            let affinity: std::collections::HashMap<String, f32> =
                serde_json::from_str(&c.affinity_scores).unwrap_or_default();

            let affinity_score = affinity.get(&role).copied().unwrap_or(0.0);

            let has_overlap = required_expertise.is_empty()
                || expertise
                    .iter()
                    .any(|t| required_expertise.contains(&t.as_str()));
            let overlap = if has_overlap { 1.0_f32 } else { 0.0_f32 } * 0.3;

            // Recency penalty (mirrors broker logic).
            let recency = c
                .last_assigned
                .as_deref()
                .and_then(days_from_iso8601)
                .map(|last_day| {
                    let today = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs()
                        / 86400;
                    let days_since = today.saturating_sub(last_day) as f32;
                    (0.5_f32 - days_since * 0.05_f32).max(0.0)
                })
                .unwrap_or(0.0);

            Scored {
                name: c.name,
                score: affinity_score + overlap - recency,
                expertise,
                traits,
            }
        })
        .collect();

    scored.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    scored.truncate(limit);

    if scored.is_empty() {
        println!("(no active personas found)");
        return Ok(());
    }

    println!("Top candidates for role '{role}':");
    println!("{:<25} {:>8}  {:<25} TRAITS", "NAME", "SCORE", "EXPERTISE");
    println!("{}", "-".repeat(90));
    for s in &scored {
        println!(
            "{:<25} {:>8.3}  {:<25} {}",
            s.name,
            s.score,
            s.expertise.join(", "),
            s.traits.join(", "),
        );
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// orphan-cleanup subcommand
// ---------------------------------------------------------------------------

fn run_orphan_cleanup() -> Result<(), String> {
    let conn = open_db()?;

    let cleared = conn
        .execute(
            "UPDATE personas SET assigned_to_session = NULL
             WHERE assigned_to_session IS NOT NULL
               AND last_assigned < datetime('now', '-24 hours')",
            [],
        )
        .map_err(|e| format!("update error: {e}"))?;

    println!("cleared {cleared} orphaned lock(s)");
    Ok(())
}

/// Parse ISO 8601 date to days since Unix epoch (mirrored from broker).
fn days_from_iso8601(s: &str) -> Option<u64> {
    let date = s.get(..10)?;
    let year: u64 = date.get(..4)?.parse().ok()?;
    let month: u64 = date.get(5..7)?.parse().ok()?;
    let day: u64 = date.get(8..10)?.parse().ok()?;
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

// ---------------------------------------------------------------------------
// retire subcommand
// ---------------------------------------------------------------------------

fn run_retire(args: &[String]) -> Result<(), String> {
    let name = positional(args).ok_or("name argument is required")?;

    let conn = open_db()?;

    let rows = conn
        .execute(
            "UPDATE personas SET status = 'archived' WHERE name = ?1",
            rusqlite::params![name],
        )
        .map_err(|e| format!("update error: {e}"))?;

    if rows == 0 {
        return Err(format!("persona not found: {name}"));
    }

    println!("retired {name}");
    Ok(())
}

// ---------------------------------------------------------------------------
// reactivate subcommand
// ---------------------------------------------------------------------------

fn run_reactivate(args: &[String]) -> Result<(), String> {
    let name = positional(args).ok_or("name argument is required")?;

    let conn = open_db()?;

    let rows = conn
        .execute(
            "UPDATE personas SET status = 'active' WHERE name = ?1",
            rusqlite::params![name],
        )
        .map_err(|e| format!("update error: {e}"))?;

    if rows == 0 {
        return Err(format!("persona not found: {name}"));
    }

    println!("reactivated {name}");
    Ok(())
}

// ---------------------------------------------------------------------------
// grow subcommand
// ---------------------------------------------------------------------------

fn run_grow(args: &[String]) -> Result<(), String> {
    let count: usize = parse_arg(args, "count")
        .and_then(|s| s.parse().ok())
        .unwrap_or(5);

    let conn = open_db()?;

    let toml_str = include_str!("../personas-seed.toml");
    let seed_data = seed::parse_seed(toml_str).map_err(|e| format!("seed parse error: {e}"))?;
    let mut rng = grow::Rng::from_time();
    let created = grow::grow(&conn, &seed_data.meta, count, &mut rng)
        .map_err(|e| format!("grow error: {e}"))?;

    println!("created {created} new persona(s)");
    Ok(())
}

// ---------------------------------------------------------------------------
// compact subcommand
// ---------------------------------------------------------------------------

fn run_compact(args: &[String]) -> Result<(), String> {
    let older_than: i64 = parse_arg(args, "older-than")
        .and_then(|s| s.parse().ok())
        .unwrap_or(30);

    let conn = open_db()?;

    // Compute the cutoff timestamp: now minus older_than days.
    let cutoff_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .saturating_sub(older_than as u64 * 86400);
    let cutoff_days = cutoff_secs / 86400;
    // Format as YYYY-MM-DD using the same civil-calendar logic.
    let cutoff_str = {
        let z = cutoff_days as i64 + 719_468;
        let era = z / 146_097;
        let doe = z - era * 146_097;
        let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
        let y = yoe + era * 400;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let d = doy - (153 * mp + 2) / 5 + 1;
        let m = if mp < 10 { mp + 3 } else { mp - 9 };
        let y = if m <= 2 { y + 1 } else { y };
        format!("{y:04}-{m:02}-{d:02}")
    };

    // Group uncompacted feedback entries older than cutoff by (persona_id, role, month).
    // month = substr(timestamp, 1, 7)  →  "YYYY-MM"
    struct Group {
        persona_id: i64,
        role: String,
        month: String,
        observations: Vec<String>,
        ids: Vec<i64>,
    }

    let mut stmt = conn
        .prepare(
            "SELECT id, persona_id, role, substr(timestamp, 1, 7) AS month, observation
             FROM persona_feedback
             WHERE compacted = 0 AND timestamp < ?1
             ORDER BY persona_id, role, month",
        )
        .map_err(|e| format!("query error: {e}"))?;

    struct FeedRow {
        id: i64,
        persona_id: i64,
        role: String,
        month: String,
        observation: String,
    }

    let feed_rows: Vec<FeedRow> = stmt
        .query_map(rusqlite::params![cutoff_str], |row| {
            Ok(FeedRow {
                id: row.get(0)?,
                persona_id: row.get(1)?,
                role: row.get(2)?,
                month: row.get(3)?,
                observation: row.get(4)?,
            })
        })
        .map_err(|e| format!("query error: {e}"))?
        .filter_map(|r| r.ok())
        .collect();

    // Build groups.
    let mut groups: Vec<Group> = Vec::new();
    for row in feed_rows {
        let key = (&row.persona_id, &row.role, &row.month);
        if let Some(g) = groups
            .iter_mut()
            .find(|g| g.persona_id == *key.0 && g.role == *key.1 && g.month == *key.2)
        {
            g.observations.push(row.observation);
            g.ids.push(row.id);
        } else {
            groups.push(Group {
                persona_id: row.persona_id,
                role: row.role.clone(),
                month: row.month.clone(),
                observations: vec![row.observation],
                ids: vec![row.id],
            });
        }
    }

    // Only compact groups with more than one entry.
    let compactable: Vec<&Group> = groups.iter().filter(|g| g.ids.len() > 1).collect();

    if compactable.is_empty() {
        println!("nothing to compact");
        return Ok(());
    }

    let now = now_iso8601();
    let mut total_compacted = 0usize;

    for g in &compactable {
        // Build summary observation.
        let summary = format!(
            "[compacted {} entries for {}/{}] {}",
            g.ids.len(),
            g.month,
            g.role,
            g.observations.join(" | ")
        );

        // Insert summary entry.
        conn.execute(
            "INSERT INTO persona_feedback (persona_id, timestamp, project, role, observation, source, compacted)
             VALUES (?1, ?2, '', ?3, ?4, 'compact', 0)",
            rusqlite::params![g.persona_id, now, g.role, summary],
        )
        .map_err(|e| format!("insert error: {e}"))?;

        // Mark originals as compacted.
        for id in &g.ids {
            conn.execute(
                "UPDATE persona_feedback SET compacted = 1 WHERE id = ?1",
                rusqlite::params![id],
            )
            .map_err(|e| format!("update error: {e}"))?;
        }

        total_compacted += g.ids.len();
    }

    println!(
        "compacted {total_compacted} entries into {} summary record(s)",
        compactable.len()
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// stats subcommand
// ---------------------------------------------------------------------------

fn run_stats(_args: &[String]) -> Result<(), String> {
    let conn = open_db()?;

    let total_personas: i64 = conn
        .query_row("SELECT COUNT(*) FROM personas", [], |r| r.get(0))
        .map_err(|e| format!("query error: {e}"))?;
    let active: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM personas WHERE status = 'active'",
            [],
            |r| r.get(0),
        )
        .map_err(|e| format!("query error: {e}"))?;
    let archived: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM personas WHERE status = 'archived'",
            [],
            |r| r.get(0),
        )
        .map_err(|e| format!("query error: {e}"))?;
    let total_assignments: i64 = conn
        .query_row("SELECT COUNT(*) FROM persona_assignments", [], |r| r.get(0))
        .map_err(|e| format!("query error: {e}"))?;
    let total_feedback: i64 = conn
        .query_row("SELECT COUNT(*) FROM persona_feedback", [], |r| r.get(0))
        .map_err(|e| format!("query error: {e}"))?;
    let uncompacted: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM persona_feedback WHERE compacted = 0",
            [],
            |r| r.get(0),
        )
        .map_err(|e| format!("query error: {e}"))?;

    println!("total_personas:      {total_personas}");
    println!("active:              {active}");
    println!("archived:            {archived}");
    println!("total_assignments:   {total_assignments}");
    println!("total_feedback:      {total_feedback}");
    println!("uncompacted_feedback:{uncompacted}");

    Ok(())
}

// ---------------------------------------------------------------------------
// seed --sync subcommand
// ---------------------------------------------------------------------------

fn run_seed(args: &[String]) -> Result<(), String> {
    let sync = args.iter().any(|a| a == "--sync");
    if !sync {
        return Err("--sync flag is required".to_string());
    }

    let home = std::env::var("HOME").map_err(|_| "HOME env var not set".to_string())?;
    let seed_path = PathBuf::from(home)
        .join(".muzzle")
        .join("personas-seed.toml");

    let toml_str = std::fs::read_to_string(&seed_path)
        .map_err(|e| format!("failed to read {}: {e}", seed_path.display()))?;

    let seed_data = seed::parse_seed(&toml_str).map_err(|e| format!("seed parse error: {e}"))?;

    let conn = open_db()?;
    let inserted =
        seed::insert_seed(&conn, &seed_data).map_err(|e| format!("seed insert error: {e}"))?;

    println!("synced seed: {inserted} new persona(s) added");
    Ok(())
}
