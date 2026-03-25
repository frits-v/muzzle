//! muzzle-persona CLI — 14 subcommands for persona management.

use muzzle_persona::broker;
use muzzle_persona::grow;
use muzzle_persona::preamble::format_preamble;
use muzzle_persona::release;
use muzzle_persona::schema::ensure_schema;
use muzzle_persona::seed::{insert_seed, now_iso8601, parse_seed};
use rusqlite::{params, Connection, Result as DbResult};
use std::collections::HashMap;

// ---------------------------------------------------------------------------
// Arg helpers
// ---------------------------------------------------------------------------

fn parse_arg(args: &[String], key: &str) -> Option<String> {
    let prefix = format!("--{key}=");
    args.iter()
        .find(|a| a.starts_with(&prefix))
        .map(|a| a[prefix.len()..].to_string())
}

fn positional(args: &[String], index: usize) -> Option<&str> {
    args.get(index).map(|s| s.as_str())
}

// ---------------------------------------------------------------------------
// DB open
// ---------------------------------------------------------------------------

fn open_db() -> DbResult<Connection> {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    let db_path = format!("{home}/.muzzle/memory.db");

    // Ensure directory exists.
    let dir = format!("{home}/.muzzle");
    let _ = std::fs::create_dir_all(&dir);

    let conn = Connection::open(&db_path)?;
    ensure_schema(&conn)?;

    // Auto-seed if empty.
    let count: i64 = conn.query_row("SELECT COUNT(*) FROM personas", [], |r| r.get(0))?;
    if count == 0 {
        let seed_str = include_str!("../personas-seed.toml");
        if let Ok(seed) = parse_seed(seed_str) {
            let _ = insert_seed(&conn, &seed);
        }
    }

    Ok(conn)
}

// ---------------------------------------------------------------------------
// Subcommand implementations
// ---------------------------------------------------------------------------

fn run_assign(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let roles_json = parse_arg(args, "roles").ok_or("--roles required")?;
    let project = parse_arg(args, "project").ok_or("--project required")?;
    let session = parse_arg(args, "session").ok_or("--session required")?;
    let agent_name = parse_arg(args, "agent-name").ok_or("--agent-name required")?;
    let team_name = parse_arg(args, "team-name");
    let summon = parse_arg(args, "summon");

    let roles: Vec<String> = serde_json::from_str(&roles_json)?;
    let role_refs: Vec<&str> = roles.iter().map(|s| s.as_str()).collect();

    let conn = open_db()?;
    let assignments = broker::assign(
        &conn,
        &role_refs,
        &project,
        &session,
        &agent_name,
        team_name.as_deref(),
        summon.as_deref(),
    )?;

    let mut output = serde_json::Map::new();
    let mut preambles = Vec::new();
    for a in &assignments {
        preambles.push(format_preamble(a));
    }
    output.insert(
        "assignments".to_string(),
        serde_json::to_value(&assignments)?,
    );
    output.insert(
        "preambles".to_string(),
        serde_json::to_value(&preambles)?,
    );

    println!("{}", serde_json::to_string_pretty(&output)?);
    Ok(())
}

fn run_release(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let session = parse_arg(args, "session").ok_or("--session required")?;
    let conn = open_db()?;
    release::release(&conn, &session)?;
    println!("released session {session}");
    Ok(())
}

fn run_list(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let status_filter = parse_arg(args, "status").unwrap_or_else(|| "active".to_string());
    let expertise_filter = parse_arg(args, "expertise");

    let conn = open_db()?;

    let sql = if status_filter == "all" {
        "SELECT name, status, expertise, last_assigned FROM personas ORDER BY name".to_string()
    } else {
        format!(
            "SELECT name, status, expertise, last_assigned FROM personas WHERE status = '{}' ORDER BY name",
            status_filter.replace('\'', "''")
        )
    };

    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([], |row| {
        let name: String = row.get(0)?;
        let status: String = row.get(1)?;
        let expertise_json: String = row.get(2)?;
        let last_assigned: Option<String> = row.get(3)?;
        Ok((name, status, expertise_json, last_assigned))
    })?;

    println!("{:<25} {:<10} {:<30} LAST_ASSIGNED", "NAME", "STATUS", "EXPERTISE");
    println!("{}", "-".repeat(80));
    for row in rows {
        let (name, status, expertise_json, last_assigned) = row?;
        let expertise: Vec<String> = serde_json::from_str(&expertise_json).unwrap_or_default();
        if let Some(ref ex_filter) = expertise_filter {
            if !expertise.iter().any(|e| e == ex_filter) {
                continue;
            }
        }
        let expertise_str = expertise.join(", ");
        let last = last_assigned.as_deref().unwrap_or("-");
        println!("{:<25} {:<10} {:<30} {}", name, status, expertise_str, last);
    }
    Ok(())
}

fn run_show(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let name = positional(args, 0).ok_or("persona name required")?;
    let conn = open_db()?;

    let row = conn.query_row(
        "SELECT id, name, traits, expertise, role_instructions, affinity_scores,
                role_counts, status, assigned_to_session, created_at, last_assigned
           FROM personas WHERE name = ?1",
        params![name],
        |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, String>(6)?,
                row.get::<_, String>(7)?,
                row.get::<_, Option<String>>(8)?,
                row.get::<_, String>(9)?,
                row.get::<_, Option<String>>(10)?,
            ))
        },
    );

    match row {
        Err(rusqlite::Error::QueryReturnedNoRows) => {
            return Err(format!("persona '{name}' not found").into());
        }
        Err(e) => return Err(e.into()),
        Ok((id, nm, traits, expertise, ri, affinity, counts, status, session, created, last)) => {
            println!("id:                  {id}");
            println!("name:                {nm}");
            println!("status:              {status}");
            println!("traits:              {traits}");
            println!("expertise:           {expertise}");
            println!("role_instructions:   {ri}");
            println!("affinity_scores:     {affinity}");
            println!("role_counts:         {counts}");
            println!("assigned_to_session: {}", session.as_deref().unwrap_or("-"));
            println!("created_at:          {created}");
            println!("last_assigned:       {}", last.as_deref().unwrap_or("-"));
        }
    }
    Ok(())
}

fn run_history(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let name = positional(args, 0).ok_or("persona name required")?;
    let project_filter = parse_arg(args, "project");
    let limit: i64 = parse_arg(args, "limit")
        .and_then(|v| v.parse().ok())
        .unwrap_or(20);

    let conn = open_db()?;

    let persona_id: i64 = conn.query_row(
        "SELECT id FROM personas WHERE name = ?1",
        params![name],
        |r| r.get(0),
    )?;

    let sql = if project_filter.is_some() {
        "SELECT role, project, agent_slot, assigned_at, released_at
           FROM persona_assignments
          WHERE persona_id = ?1 AND project = ?2
          ORDER BY assigned_at DESC LIMIT ?3"
            .to_string()
    } else {
        "SELECT role, project, agent_slot, assigned_at, released_at
           FROM persona_assignments
          WHERE persona_id = ?1
          ORDER BY assigned_at DESC LIMIT ?2"
            .to_string()
    };

    println!("{:<20} {:<20} {:<15} {:<25} RELEASED_AT", "ROLE", "PROJECT", "SLOT", "ASSIGNED_AT");
    println!("{}", "-".repeat(90));

    if let Some(proj) = project_filter {
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params![persona_id, proj, limit], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, Option<String>>(4)?,
            ))
        })?;
        for row in rows {
            let (role, project, slot, assigned, released) = row?;
            println!(
                "{:<20} {:<20} {:<15} {:<25} {}",
                role,
                project,
                slot,
                assigned,
                released.as_deref().unwrap_or("-")
            );
        }
    } else {
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params![persona_id, limit], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, Option<String>>(4)?,
            ))
        })?;
        for row in rows {
            let (role, project, slot, assigned, released) = row?;
            println!(
                "{:<20} {:<20} {:<15} {:<25} {}",
                role,
                project,
                slot,
                assigned,
                released.as_deref().unwrap_or("-")
            );
        }
    }
    Ok(())
}

fn run_feedback(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let name = positional(args, 0).ok_or("persona name required")?;
    let observation = parse_arg(args, "observation").ok_or("--observation required")?;
    let role = parse_arg(args, "role").unwrap_or_else(|| "general".to_string());

    let conn = open_db()?;

    let persona_id: i64 = conn.query_row(
        "SELECT id FROM personas WHERE name = ?1",
        params![name],
        |r| r.get(0),
    )?;

    let now = now_iso8601();
    conn.execute(
        "INSERT INTO persona_feedback (persona_id, timestamp, project, role, observation, source)
         VALUES (?1, ?2, '', ?3, ?4, 'cli')",
        params![persona_id, now, role, observation],
    )?;

    println!("feedback recorded for {name}");
    Ok(())
}

fn run_search(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let role = parse_arg(args, "role").unwrap_or_else(|| "general".to_string());
    let limit: usize = parse_arg(args, "limit")
        .and_then(|v| v.parse().ok())
        .unwrap_or(5);

    let conn = open_db()?;

    let mut stmt = conn.prepare(
        "SELECT name, expertise, affinity_scores
           FROM personas
          WHERE status = 'active' AND assigned_to_session IS NULL
          ORDER BY name",
    )?;

    struct Row {
        name: String,
        expertise: Vec<String>,
        affinity_scores: HashMap<String, f32>,
    }

    let rows: Vec<Row> = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
            ))
        })?
        .filter_map(|r| r.ok())
        .map(|(name, expertise_json, affinity_json)| Row {
            name,
            expertise: serde_json::from_str(&expertise_json).unwrap_or_default(),
            affinity_scores: serde_json::from_str(&affinity_json).unwrap_or_default(),
        })
        .collect();

    // Simple score: affinity + expertise match bonus.
    let mut scored: Vec<(f32, &Row)> = rows
        .iter()
        .map(|r| {
            let affinity = r.affinity_scores.get(&role).copied().unwrap_or(0.0);
            let expertise_bonus = if r.expertise.iter().any(|e| {
                muzzle_persona::types::expertise_for_role(&role).contains(&e.as_str())
            }) {
                0.3
            } else {
                0.0
            };
            (affinity + expertise_bonus, r)
        })
        .collect();

    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    scored.truncate(limit);

    println!("{:<5} {:<25} {:<8} EXPERTISE", "RANK", "NAME", "SCORE");
    println!("{}", "-".repeat(60));
    for (rank, (score, row)) in scored.iter().enumerate() {
        let expertise_str = row.expertise.join(", ");
        println!("{:<5} {:<25} {:<8.3} {}", rank + 1, row.name, score, expertise_str);
    }
    Ok(())
}

fn run_retire(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let name = positional(args, 0).ok_or("persona name required")?;
    let conn = open_db()?;
    let updated = conn.execute(
        "UPDATE personas SET status = 'archived' WHERE name = ?1",
        params![name],
    )?;
    if updated == 0 {
        return Err(format!("persona '{name}' not found").into());
    }
    println!("retired {name}");
    Ok(())
}

fn run_reactivate(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let name = positional(args, 0).ok_or("persona name required")?;
    let conn = open_db()?;
    let updated = conn.execute(
        "UPDATE personas SET status = 'active' WHERE name = ?1",
        params![name],
    )?;
    if updated == 0 {
        return Err(format!("persona '{name}' not found").into());
    }
    println!("reactivated {name}");
    Ok(())
}

fn run_grow(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let count: usize = parse_arg(args, "count")
        .and_then(|v| v.parse().ok())
        .unwrap_or(5);

    let conn = open_db()?;
    let seed_str = include_str!("../personas-seed.toml");
    let seed_file = parse_seed(seed_str)?;
    let added = grow::grow(&conn, &seed_file.meta, count, &mut grow::Rng::from_time())?;
    println!("grew {added} persona(s)");
    Ok(())
}

fn run_compact(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let older_than: i64 = parse_arg(args, "older-than")
        .and_then(|v| v.parse().ok())
        .unwrap_or(30);

    let conn = open_db()?;

    // Find uncompacted feedback older than `older_than` days.
    let mut stmt = conn.prepare(
        "SELECT pf.id, pf.persona_id, p.name, pf.role, pf.observation,
                strftime('%Y-%m', pf.timestamp) as month
           FROM persona_feedback pf
           JOIN personas p ON p.id = pf.persona_id
          WHERE pf.compacted = 0
            AND pf.timestamp < datetime('now', ?1)
          ORDER BY pf.persona_id, pf.role, month",
    )?;
    let cutoff = format!("-{older_than} days");

    struct FeedbackRow {
        id: i64,
        persona_id: i64,
        name: String,
        role: String,
        month: String,
    }

    let rows: Vec<FeedbackRow> = stmt
        .query_map(params![cutoff], |r| {
            Ok(FeedbackRow {
                id: r.get(0)?,
                persona_id: r.get(1)?,
                name: r.get(2)?,
                role: r.get(3)?,
                // column 4 is observation — not used in the summary, skip
                month: r.get(5)?,
            })
        })?
        .filter_map(|r| r.ok())
        .collect();

    if rows.is_empty() {
        println!("nothing to compact");
        return Ok(());
    }

    // Group by (persona_id, role, month) and build a summary observation.
    let mut groups: HashMap<(i64, String, String), (String, Vec<i64>)> = HashMap::new();
    for row in &rows {
        let key = (row.persona_id, row.role.clone(), row.month.clone());
        let entry = groups.entry(key).or_insert_with(|| (row.name.clone(), Vec::new()));
        entry.1.push(row.id);
    }

    let mut compacted = 0usize;
    for ((persona_id, role, month), (name, ids)) in &groups {
        let summary = format!(
            "Compacted {month} ({} feedback entries for {name} as {role})",
            ids.len()
        );
        let now = now_iso8601();

        // Insert compacted summary row.
        conn.execute(
            "INSERT INTO persona_feedback (persona_id, timestamp, project, role, observation, source, compacted)
             VALUES (?1, ?2, '', ?3, ?4, 'compact', 1)",
            params![persona_id, now, role, summary],
        )?;

        // Mark originals as compacted.
        for &id in ids {
            conn.execute(
                "UPDATE persona_feedback SET compacted = 1 WHERE id = ?1",
                params![id],
            )?;
        }
        compacted += ids.len();
    }

    println!("compacted {compacted} feedback entries into {} summaries", groups.len());
    Ok(())
}

fn run_stats(_args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let conn = open_db()?;

    let total: i64 = conn.query_row("SELECT COUNT(*) FROM personas", [], |r| r.get(0))?;
    let active: i64 = conn.query_row(
        "SELECT COUNT(*) FROM personas WHERE status = 'active'",
        [],
        |r| r.get(0),
    )?;
    let archived: i64 = conn.query_row(
        "SELECT COUNT(*) FROM personas WHERE status = 'archived'",
        [],
        |r| r.get(0),
    )?;
    let locked: i64 = conn.query_row(
        "SELECT COUNT(*) FROM personas WHERE assigned_to_session IS NOT NULL",
        [],
        |r| r.get(0),
    )?;
    let total_assignments: i64 =
        conn.query_row("SELECT COUNT(*) FROM persona_assignments", [], |r| r.get(0))?;
    let total_feedback: i64 =
        conn.query_row("SELECT COUNT(*) FROM persona_feedback", [], |r| r.get(0))?;

    println!("personas total:      {total}");
    println!("personas active:     {active}");
    println!("personas archived:   {archived}");
    println!("personas locked:     {locked}");
    println!("total assignments:   {total_assignments}");
    println!("total feedback:      {total_feedback}");
    Ok(())
}

fn run_seed(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    // Only --sync is implemented.
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    let seed_path = format!("{home}/.muzzle/personas-seed.toml");

    let toml_str = std::fs::read_to_string(&seed_path)
        .map_err(|e| format!("could not read {seed_path}: {e}"))?;
    let seed_file = parse_seed(&toml_str)?;

    let conn = open_db()?;
    let inserted = insert_seed(&conn, &seed_file)?;
    println!("sync complete: {inserted} new persona(s) inserted from {seed_path}");
    let _ = args; // --sync flag is informational only
    Ok(())
}

fn run_orphan_cleanup(_args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let conn = open_db()?;
    let updated = conn.execute(
        "UPDATE personas SET assigned_to_session = NULL
          WHERE assigned_to_session IS NOT NULL
            AND last_assigned < datetime('now', '-24 hours')",
        [],
    )?;
    println!("orphan cleanup: {updated} stale lock(s) cleared");
    Ok(())
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

fn main() {
    let args: Vec<String> = std::env::args().collect();

    let subcommand = args.get(1).map(|s| s.as_str());

    let result: Result<(), Box<dyn std::error::Error>> = match subcommand {
        Some("assign") => run_assign(&args[2..]),
        Some("release") => run_release(&args[2..]),
        Some("list") => run_list(&args[2..]),
        Some("show") => run_show(&args[2..]),
        Some("history") => run_history(&args[2..]),
        Some("feedback") => run_feedback(&args[2..]),
        Some("search") => run_search(&args[2..]),
        Some("retire") => run_retire(&args[2..]),
        Some("reactivate") => run_reactivate(&args[2..]),
        Some("grow") => run_grow(&args[2..]),
        Some("compact") => run_compact(&args[2..]),
        Some("stats") => run_stats(&args[2..]),
        Some("seed") => run_seed(&args[2..]),
        Some("orphan-cleanup") => run_orphan_cleanup(&args[2..]),
        Some(cmd) => {
            eprintln!("error: unknown subcommand '{cmd}'");
            std::process::exit(1);
        }
        None => {
            eprintln!(
                "usage: muzzle-persona <subcommand> [options]

subcommands:
  assign         --roles=JSON --project=P --session=S --agent-name=N [--team-name=T] [--summon=Name]
  release        --session=S
  list           [--status=active|archived|all] [--expertise=tag]
  show           <name>
  history        <name> [--project=P] [--limit=20]
  feedback       <name> --observation=... [--role=R]
  search         --role=R [--limit=5]
  retire         <name>
  reactivate     <name>
  grow           [--count=5]
  compact        [--older-than=30]
  stats
  seed           --sync
  orphan-cleanup
"
            );
            std::process::exit(1);
        }
    };

    if let Err(e) = result {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}
