use rusqlite::Connection;

#[test]
fn full_lifecycle() {
    let conn = Connection::open_in_memory().unwrap();
    muzzle_persona::schema::ensure_schema(&conn).unwrap();
    let toml_str = include_str!("../personas-seed.toml");
    let seed = muzzle_persona::seed::parse_seed(toml_str).unwrap();
    muzzle_persona::seed::insert_seed(&conn, &seed).unwrap();

    // Assign 2 roles
    let assignments = muzzle_persona::broker::assign(
        &conn,
        &["code-reviewer", "researcher"],
        "acme-api",
        "sess-e2e",
        "w1",
        None,
        None,
    )
    .unwrap();
    assert_eq!(assignments.len(), 2);

    // Verify preambles
    for a in &assignments {
        let preamble = muzzle_persona::preamble::format_preamble(a);
        assert!(preamble.len() <= 500);
        assert!(preamble.contains(&a.name));
    }

    // Verify locks set
    let locked: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM personas WHERE assigned_to_session IS NOT NULL",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(locked, 2);

    // Release
    muzzle_persona::release::release(&conn, "sess-e2e").unwrap();

    // Verify locks cleared
    let locked: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM personas WHERE assigned_to_session IS NOT NULL",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(locked, 0);

    // Verify role_counts incremented
    let counts_json: String = conn
        .query_row(
            "SELECT role_counts FROM personas WHERE id = ?1",
            [assignments[0].persona_id],
            |r| r.get(0),
        )
        .unwrap();
    let counts: std::collections::HashMap<String, u32> =
        serde_json::from_str(&counts_json).unwrap();
    assert!(*counts.values().next().unwrap_or(&0) >= 1);

    // Verify assignment records have released_at
    let released: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM persona_assignments WHERE session_id = 'sess-e2e' AND released_at IS NOT NULL",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(released, 2);
}

#[test]
fn assign_triggers_auto_grow_when_pool_exhausted() {
    let conn = Connection::open_in_memory().unwrap();
    muzzle_persona::schema::ensure_schema(&conn).unwrap();
    let toml_str = include_str!("../personas-seed.toml");
    let seed = muzzle_persona::seed::parse_seed(toml_str).unwrap();
    muzzle_persona::seed::insert_seed(&conn, &seed).unwrap();

    let seed_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM personas", [], |r| r.get(0))
        .unwrap();

    // Assign more roles than seed personas exist
    let mut roles: Vec<&str> = Vec::new();
    for _ in 0..(seed_count as usize + 1) {
        roles.push("general");
    }

    let assignments =
        muzzle_persona::broker::assign(&conn, &roles, "acme-api", "sess-grow", "w1", None, None)
            .unwrap();
    assert_eq!(assignments.len(), roles.len());

    // Verify auto-grow happened
    let total: i64 = conn
        .query_row("SELECT COUNT(*) FROM personas", [], |r| r.get(0))
        .unwrap();
    assert!(total > seed_count);
}
