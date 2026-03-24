use muzzle_persona::{broker, preamble, release, schema, seed};
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

// ---------------------------------------------------------------------------
// DB path
// ---------------------------------------------------------------------------

fn db_path() -> Result<PathBuf, String> {
    let home = std::env::var("HOME").map_err(|_| "HOME env var not set".to_string())?;
    Ok(PathBuf::from(home).join(".muzzle").join("memory.db"))
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
