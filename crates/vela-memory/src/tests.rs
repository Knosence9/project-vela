use super::*;

#[test]
fn parses_and_renders_round_trip_with_paragraph_entries() {
    let entries = vec![
        "First memory entry".to_string(),
        "Second memory entry\nwith another line".to_string(),
        "Third entry".to_string(),
    ];

    let rendered = render_entries(&entries);
    assert_eq!(
        rendered,
        "First memory entry\n\nSecond memory entry\nwith another line\n\nThird entry"
    );
    assert_eq!(parse_entries(&rendered), entries);
    assert_eq!(parse_entries(&render_storage(&entries)), entries);
}

#[test]
fn initialize_memory_migrates_legacy_separator_files() {
    let vela_home =
        std::env::temp_dir().join(format!("vela-memory-test-{}", unix_timestamp_nanos()));
    let memories_dir = vela_home.join("memories");
    fs::create_dir_all(&memories_dir).unwrap();
    fs::write(memories_dir.join("MEMORY.md"), "alpha§beta§gamma").unwrap();
    fs::write(memories_dir.join("USER.md"), "delta§epsilon").unwrap();

    let report = initialize_memory(&vela_home).unwrap();
    assert_eq!(
        report.memory_char_count,
        "alpha\n\nbeta\n\ngamma".chars().count()
    );
    assert_eq!(report.user_char_count, "delta\n\nepsilon".chars().count());
    assert_eq!(
        fs::read_to_string(memories_dir.join("MEMORY.md")).unwrap(),
        "<!-- vela-memory-format: v2 -->\n\nalpha\n\nbeta\n\ngamma"
    );
    assert_eq!(
        fs::read_to_string(memories_dir.join("USER.md")).unwrap(),
        "<!-- vela-memory-format: v2 -->\n\ndelta\n\nepsilon"
    );

    fs::remove_dir_all(&vela_home).unwrap();
}

#[test]
fn marker_prevents_false_legacy_migration_for_section_sign_entries() {
    let vela_home = std::env::temp_dir().join(format!(
        "vela-memory-test-marker-{}",
        unix_timestamp_nanos()
    ));
    let memories_dir = vela_home.join("memories");
    fs::create_dir_all(&memories_dir).unwrap();
    fs::write(
        memories_dir.join("MEMORY.md"),
        "<!-- vela-memory-format: v2 -->\n\nentry with § symbol",
    )
    .unwrap();
    fs::write(
        memories_dir.join("USER.md"),
        "<!-- vela-memory-format: v2 -->\n",
    )
    .unwrap();

    let view = view_memory(&vela_home, MemoryTarget::Memory).unwrap();
    assert_eq!(view.entries, vec!["entry with § symbol".to_string()]);
    assert_eq!(
        fs::read_to_string(memories_dir.join("MEMORY.md")).unwrap(),
        "<!-- vela-memory-format: v2 -->\n\nentry with § symbol"
    );

    fs::remove_dir_all(&vela_home).unwrap();
}

#[test]
fn stage_add_rejects_duplicate_existing_or_pending_entries() {
    let vela_home =
        std::env::temp_dir().join(format!("vela-memory-test-dup-{}", unix_timestamp_nanos()));
    initialize_memory(&vela_home).unwrap();
    add_memory_entry(&vela_home, MemoryTarget::User, "remember this").unwrap();

    let err = stage_add_memory_entry(&vela_home, MemoryTarget::User, "remember this").unwrap_err();
    assert!(err.to_string().contains("already exists"));

    stage_add_memory_entry(&vela_home, MemoryTarget::User, "another memory").unwrap();
    let err = stage_add_memory_entry(&vela_home, MemoryTarget::User, "another memory").unwrap_err();
    assert!(err.to_string().contains("already pending approval"));

    fs::remove_dir_all(&vela_home).unwrap();
}

#[test]
fn approve_pending_reports_stale_replace_clearly() {
    let vela_home =
        std::env::temp_dir().join(format!("vela-memory-test-stale-{}", unix_timestamp_nanos()));
    initialize_memory(&vela_home).unwrap();
    add_memory_entry(&vela_home, MemoryTarget::Memory, "old value").unwrap();
    let pending =
        stage_replace_memory_entry(&vela_home, MemoryTarget::Memory, "old", "new value").unwrap();
    replace_memory_entry(
        &vela_home,
        MemoryTarget::Memory,
        "old",
        "someone else changed it",
    )
    .unwrap();

    let err = approve_pending(&vela_home, &pending.id).unwrap_err();
    assert!(err.to_string().contains("became stale or conflicted"));

    fs::remove_dir_all(&vela_home).unwrap();
}

#[test]
fn approve_pending_reports_stale_remove_clearly() {
    let vela_home = std::env::temp_dir().join(format!(
        "vela-memory-test-stale-remove-{}",
        unix_timestamp_nanos()
    ));
    initialize_memory(&vela_home).unwrap();
    add_memory_entry(&vela_home, MemoryTarget::Memory, "old value").unwrap();
    let pending = stage_remove_memory_entry(&vela_home, MemoryTarget::Memory, "old").unwrap();
    replace_memory_entry(
        &vela_home,
        MemoryTarget::Memory,
        "old",
        "someone else changed it",
    )
    .unwrap();

    let err = approve_pending(&vela_home, &pending.id).unwrap_err();
    assert!(err.to_string().contains("became stale or conflicted"));

    fs::remove_dir_all(&vela_home).unwrap();
}
