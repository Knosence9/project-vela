use super::*;

#[test]
fn append_event_targets_requested_session() {
    let vela_home =
        std::env::temp_dir().join(format!("vela-state-test-{}", unix_timestamp_nanos()));
    let report = initialize_persistence(&vela_home).unwrap();

    let first = resolve_runtime_session(
        &report.state_db_path,
        &SessionRequest {
            command_name: "chat".to_string(),
            query_present: true,
            query_text: Some("first".to_string()),
            image_present: false,
            image_path: None,
            resume: None,
            continue_last: None,
        },
    )
    .unwrap();
    let _second = resolve_runtime_session(
        &report.state_db_path,
        &SessionRequest {
            command_name: "chat".to_string(),
            query_present: true,
            query_text: Some("second".to_string()),
            image_present: false,
            image_path: None,
            resume: None,
            continue_last: None,
        },
    )
    .unwrap();

    assert!(append_event_to_session(
        &report.state_db_path,
        &first.session_id,
        "review_candidate_created",
        "{}".to_string(),
    )
    .unwrap());

    let conn = Connection::open(&report.state_db_path).unwrap();
    let first_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM session_events WHERE session_id = ?1 AND event_type = 'review_candidate_created'",
            params![first.session_id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(first_count, 1);

    let _ = fs::remove_dir_all(&vela_home);
}

#[test]
fn runtime_session_titles_follow_user_visible_inputs() {
    let vela_home =
        std::env::temp_dir().join(format!("vela-state-title-test-{}", unix_timestamp_nanos()));
    let report = initialize_persistence(&vela_home).unwrap();

    let interactive = resolve_runtime_session(
        &report.state_db_path,
        &SessionRequest {
            command_name: "chat".to_string(),
            query_present: false,
            query_text: None,
            image_present: false,
            image_path: None,
            resume: None,
            continue_last: None,
        },
    )
    .unwrap();
    assert_eq!(interactive.title, "chat interactive");

    let queried = resolve_runtime_session(
        &report.state_db_path,
        &SessionRequest {
            command_name: "chat".to_string(),
            query_present: true,
            query_text: Some("please always use terse answers".to_string()),
            image_present: false,
            image_path: None,
            resume: None,
            continue_last: None,
        },
    )
    .unwrap();
    assert_eq!(queried.title, "chat: please always use terse answers");

    let normalized_query = resolve_runtime_session(
        &report.state_db_path,
        &SessionRequest {
            command_name: "chat".to_string(),
            query_present: true,
            query_text: Some("  please   keep\n   spacing\t tidy  ".to_string()),
            image_present: false,
            image_path: None,
            resume: None,
            continue_last: None,
        },
    )
    .unwrap();
    assert_eq!(normalized_query.title, "chat: please keep spacing tidy");

    let repeated_query = resolve_runtime_session(
        &report.state_db_path,
        &SessionRequest {
            command_name: "chat".to_string(),
            query_present: true,
            query_text: Some("please always use terse answers".to_string()),
            image_present: false,
            image_path: None,
            resume: None,
            continue_last: None,
        },
    )
    .unwrap();
    assert_eq!(repeated_query.title, queried.title);
    assert_ne!(repeated_query.session_id, queried.session_id);

    let long_query = resolve_runtime_session(
        &report.state_db_path,
        &SessionRequest {
            command_name: "chat".to_string(),
            query_present: true,
            query_text: Some(
                "0123456789 0123456789 0123456789 0123456789 0123456789 0123456789 0123456789 0123456789 0123456789"
                    .to_string(),
            ),
            image_present: false,
            image_path: None,
            resume: None,
            continue_last: None,
        },
    )
    .unwrap();
    assert_eq!(
        long_query.title,
        "chat: 0123456789 0123456789 0123456789 0123456789 0123456789 0123456789 0123456789 01…"
    );

    let blank_query = resolve_runtime_session(
        &report.state_db_path,
        &SessionRequest {
            command_name: "chat".to_string(),
            query_present: true,
            query_text: Some("   \n\t  ".to_string()),
            image_present: false,
            image_path: None,
            resume: None,
            continue_last: None,
        },
    )
    .unwrap();
    assert_eq!(blank_query.title, "chat interactive");

    let image_only = resolve_runtime_session(
        &report.state_db_path,
        &SessionRequest {
            command_name: "chat".to_string(),
            query_present: false,
            query_text: None,
            image_present: true,
            image_path: Some("/tmp/example-diagram.png".to_string()),
            resume: None,
            continue_last: None,
        },
    )
    .unwrap();
    assert_eq!(image_only.title, "chat image: example-diagram.png");

    let blank_image_path = resolve_runtime_session(
        &report.state_db_path,
        &SessionRequest {
            command_name: "chat".to_string(),
            query_present: false,
            query_text: None,
            image_present: true,
            image_path: Some("   ".to_string()),
            resume: None,
            continue_last: None,
        },
    )
    .unwrap();
    assert_eq!(blank_image_path.title, "chat image");

    let _ = fs::remove_dir_all(&vela_home);
}

#[test]
fn resolve_command_session_reuses_latest_matching_command() {
    let vela_home = std::env::temp_dir().join(format!(
        "vela-state-gateway-test-{}",
        unix_timestamp_nanos()
    ));
    let report = initialize_persistence(&vela_home).unwrap();

    let first = resolve_command_session(
        &report.state_db_path,
        "gateway",
        InteractionMode::Interactive,
    )
    .unwrap();
    let second = resolve_command_session(
        &report.state_db_path,
        "gateway",
        InteractionMode::Interactive,
    )
    .unwrap();

    assert_eq!(first.session_id, second.session_id);
    assert!(matches!(first.action, SessionAction::Created));
    assert!(matches!(second.action, SessionAction::ResumedLatest));

    assert!(append_message_to_session(
        &report.state_db_path,
        &second.session_id,
        "system",
        "Gateway bootstrap ready.",
        Some(json!({"source": "gateway", "direction": "egress"}).to_string()),
    )
    .unwrap());

    let summary = current_command_session_summary(&report.state_db_path, "gateway")
        .unwrap()
        .unwrap();
    assert_eq!(summary.id, first.session_id);
    assert_eq!(summary.runtime_state, "ready");
    assert_eq!(summary.message_count, 1);

    let _ = fs::remove_dir_all(&vela_home);
}

#[test]
fn branch_and_compression_preserve_lineage_and_inspection() {
    let vela_home =
        std::env::temp_dir().join(format!("vela-state-branch-test-{}", unix_timestamp_nanos()));
    let report = initialize_persistence(&vela_home).unwrap();
    let parent = resolve_runtime_session(
        &report.state_db_path,
        &SessionRequest {
            command_name: "chat".to_string(),
            query_present: true,
            query_text: Some("parent turn".to_string()),
            image_present: false,
            image_path: None,
            resume: None,
            continue_last: None,
        },
    )
    .unwrap();
    assert!(append_message_to_session(
        &report.state_db_path,
        &parent.session_id,
        "assistant",
        "parent reply",
        None
    )
    .unwrap());

    let branch = branch_session(
        &report.state_db_path,
        &parent.session_id,
        Some("branch-a"),
        Some("explore alternative"),
    )
    .unwrap();
    let compression = compress_session(
        &report.state_db_path,
        &branch.session_id,
        "branch compressed summary",
    )
    .unwrap();
    let inspection = inspect_session(&report.state_db_path, &branch.session_id, 20)
        .unwrap()
        .expect("branch inspection");
    assert_eq!(
        inspection.branch.parent_session_id.as_deref(),
        Some(parent.session_id.as_str())
    );
    assert_eq!(
        inspection.branch.branch_note.as_deref(),
        Some("explore alternative")
    );
    assert_eq!(inspection.messages.len(), 2);
    assert_eq!(inspection.compressions.len(), 1);
    assert!(inspection.child_sessions.is_empty());
    assert_eq!(inspection.compressions[0].id, compression.id);
    let parent_inspection = inspect_session(&report.state_db_path, &parent.session_id, 20)
        .unwrap()
        .expect("parent inspection");
    assert_eq!(parent_inspection.child_sessions.len(), 1);
    assert_eq!(parent_inspection.child_sessions[0].id, branch.session_id);
    let summary = load_summary(
        &Connection::open(&report.state_db_path).unwrap(),
        &branch.session_id,
        &branch.title,
    )
    .unwrap();
    assert_eq!(
        summary.parent_session_id.as_deref(),
        Some(parent.session_id.as_str())
    );
    assert_eq!(summary.runtime_state, "ready");

    let _ = fs::remove_dir_all(&vela_home);
}

#[test]
fn continue_target_prefers_latest_session_in_branch_subtree() {
    let vela_home = std::env::temp_dir().join(format!(
        "vela-state-continue-test-{}",
        unix_timestamp_nanos()
    ));
    let report = initialize_persistence(&vela_home).unwrap();
    let root = resolve_runtime_session(
        &report.state_db_path,
        &SessionRequest {
            command_name: "chat".to_string(),
            query_present: true,
            query_text: Some("root turn".to_string()),
            image_present: false,
            image_path: None,
            resume: None,
            continue_last: None,
        },
    )
    .unwrap();
    let branch_a = branch_session(
        &report.state_db_path,
        &root.session_id,
        Some("branch-a"),
        None,
    )
    .unwrap();
    let _branch_b = branch_session(
        &report.state_db_path,
        &root.session_id,
        Some("branch-b"),
        None,
    )
    .unwrap();
    let branch_a_child = branch_session(
        &report.state_db_path,
        &branch_a.session_id,
        Some("branch-a-child"),
        None,
    )
    .unwrap();

    let continued = resolve_runtime_session(
        &report.state_db_path,
        &SessionRequest {
            command_name: "chat".to_string(),
            query_present: true,
            query_text: Some("continue root".to_string()),
            image_present: false,
            image_path: None,
            resume: None,
            continue_last: Some(root.title.clone()),
        },
    )
    .unwrap();
    assert_eq!(continued.session_id, branch_a_child.session_id);
    assert_eq!(
        continued.continue_resolution.as_deref(),
        Some("latest-in-subtree")
    );
    assert_eq!(
        continued.continue_target.as_deref(),
        Some(root.title.as_str())
    );
    assert_eq!(
        continued.continue_anchor_session_id.as_deref(),
        Some(root.session_id.as_str())
    );

    let continued_branch = resolve_runtime_session(
        &report.state_db_path,
        &SessionRequest {
            command_name: "chat".to_string(),
            query_present: true,
            query_text: Some("continue branch-a".to_string()),
            image_present: false,
            image_path: None,
            resume: None,
            continue_last: Some(branch_a.title.clone()),
        },
    )
    .unwrap();
    assert_eq!(continued_branch.session_id, branch_a_child.session_id);
    assert_eq!(
        continued_branch.continue_resolution.as_deref(),
        Some("latest-in-subtree")
    );
    assert_eq!(
        continued_branch.continue_anchor_session_id.as_deref(),
        Some(branch_a.session_id.as_str())
    );

    let compressed = compress_session(
        &report.state_db_path,
        &branch_a.session_id,
        "branch-a compressed summary",
    )
    .unwrap();
    assert_eq!(compressed.session_id, branch_a.session_id);

    let continued_after_compress = resolve_runtime_session(
        &report.state_db_path,
        &SessionRequest {
            command_name: "chat".to_string(),
            query_present: true,
            query_text: Some("continue root after compress".to_string()),
            image_present: false,
            image_path: None,
            resume: None,
            continue_last: Some(root.title.clone()),
        },
    )
    .unwrap();
    assert_eq!(
        continued_after_compress.session_id,
        branch_a_child.session_id
    );
    assert_eq!(
        continued_after_compress.continue_resolution.as_deref(),
        Some("latest-in-subtree")
    );

    let continued_exact = resolve_runtime_session(
        &report.state_db_path,
        &SessionRequest {
            command_name: "chat".to_string(),
            query_present: true,
            query_text: Some("continue branch-b".to_string()),
            image_present: false,
            image_path: None,
            resume: None,
            continue_last: Some("branch-b".to_string()),
        },
    )
    .unwrap();
    assert_eq!(
        continued_exact.continue_resolution.as_deref(),
        Some("exact-anchor")
    );
    assert_eq!(continued_exact.continue_target.as_deref(), Some("branch-b"));

    let continued_latest = resolve_runtime_session(
        &report.state_db_path,
        &SessionRequest {
            command_name: "chat".to_string(),
            query_present: true,
            query_text: Some("continue latest".to_string()),
            image_present: false,
            image_path: None,
            resume: None,
            continue_last: Some(String::new()),
        },
    )
    .unwrap();
    assert_eq!(
        continued_latest.continue_resolution.as_deref(),
        Some("latest-global")
    );
    assert_eq!(continued_latest.continue_target.as_deref(), Some(""));
    assert!(continued_latest.continue_anchor_session_id.is_none());

    let latest_summary = current_session_summary(&report.state_db_path)
        .unwrap()
        .unwrap();
    assert_eq!(latest_summary.runtime_state, "ready");

    let _ = fs::remove_dir_all(&vela_home);
}

#[test]
fn session_runtime_state_tracks_bounded_phase_labels() {
    let vela_home = std::env::temp_dir().join(format!(
        "vela-state-runtime-state-test-{}",
        unix_timestamp_nanos()
    ));
    let report = initialize_persistence(&vela_home).unwrap();
    let session = resolve_runtime_session(
        &report.state_db_path,
        &SessionRequest {
            command_name: "chat".to_string(),
            query_present: false,
            query_text: None,
            image_present: false,
            image_path: None,
            resume: None,
            continue_last: None,
        },
    )
    .unwrap();

    let summary = current_session_summary(&report.state_db_path)
        .unwrap()
        .unwrap();
    assert_eq!(summary.runtime_state, "ready");

    for (runtime_state, expected_label) in [
        (SessionRuntimeState::Receive, "receive"),
        (SessionRuntimeState::Deliberate, "deliberate"),
        (SessionRuntimeState::ToolRequest, "tool-request"),
        (SessionRuntimeState::ToolResult, "tool-result"),
        (SessionRuntimeState::Reflect, "reflect"),
        (SessionRuntimeState::Retry, "retry"),
        (SessionRuntimeState::Respond, "respond"),
        (SessionRuntimeState::Finish, "finish"),
        (SessionRuntimeState::Failed, "failed"),
    ] {
        set_session_runtime_state(&report.state_db_path, &session.session_id, runtime_state)
            .unwrap();
        let inspection = inspect_session(&report.state_db_path, &session.session_id, 20)
            .unwrap()
            .unwrap();
        assert_eq!(inspection.runtime_state, expected_label);
    }

    let finished = current_session_summary(&report.state_db_path)
        .unwrap()
        .unwrap();
    assert_eq!(finished.runtime_state, "failed");

    let _ = fs::remove_dir_all(&vela_home);
}

#[test]
fn compression_summary_policy_is_enforced() {
    let vela_home = std::env::temp_dir().join(format!(
        "vela-state-compress-test-{}",
        unix_timestamp_nanos()
    ));
    let report = initialize_persistence(&vela_home).unwrap();
    let session = resolve_runtime_session(
        &report.state_db_path,
        &SessionRequest {
            command_name: "chat".to_string(),
            query_present: true,
            query_text: Some("compress me".to_string()),
            image_present: false,
            image_path: None,
            resume: None,
            continue_last: None,
        },
    )
    .unwrap();

    let empty_error =
        compress_session(&report.state_db_path, &session.session_id, "   ").unwrap_err();
    assert!(empty_error
        .to_string()
        .contains("compression summary cannot be empty"));

    let long_summary = "x".repeat(SESSION_COMPRESSION_CHAR_LIMIT + 1);
    let long_error =
        compress_session(&report.state_db_path, &session.session_id, &long_summary).unwrap_err();
    assert!(long_error
        .to_string()
        .contains("compression summary exceeds"));

    compress_session(&report.state_db_path, &session.session_id, "same summary").unwrap();
    let duplicate_error =
        compress_session(&report.state_db_path, &session.session_id, "same summary").unwrap_err();
    assert!(duplicate_error
        .to_string()
        .contains("matches the latest persisted summary"));

    let _ = fs::remove_dir_all(&vela_home);
}

#[test]
fn initialize_persistence_migrates_legacy_messages_without_metadata_column() {
    let vela_home = std::env::temp_dir().join(format!(
        "vela-state-legacy-metadata-test-{}",
        unix_timestamp_nanos()
    ));
    fs::create_dir_all(&vela_home).unwrap();
    let state_db_path = vela_home.join("state.db");
    let conn = Connection::open(&state_db_path).unwrap();
    conn.execute_batch(
        "
        CREATE TABLE state_meta (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );
        CREATE TABLE sessions (
            id TEXT PRIMARY KEY,
            title TEXT NOT NULL,
            command_name TEXT NOT NULL,
            interaction_mode TEXT NOT NULL,
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL
        );
        CREATE TABLE messages (
            id TEXT PRIMARY KEY,
            session_id TEXT NOT NULL,
            role TEXT NOT NULL,
            content TEXT NOT NULL,
            created_at INTEGER NOT NULL,
            FOREIGN KEY(session_id) REFERENCES sessions(id)
        );
        CREATE TABLE session_events (
            id TEXT PRIMARY KEY,
            session_id TEXT NOT NULL,
            event_type TEXT NOT NULL,
            payload_json TEXT NOT NULL,
            created_at INTEGER NOT NULL,
            FOREIGN KEY(session_id) REFERENCES sessions(id)
        );
        CREATE VIRTUAL TABLE message_fts USING fts5(
            message_id UNINDEXED,
            session_id UNINDEXED,
            title UNINDEXED,
            content
        );
        ",
    )
    .unwrap();
    conn.execute(
        "INSERT INTO sessions(id, title, command_name, interaction_mode, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            "session-legacy",
            "Legacy",
            "chat",
            "single-turn",
            1_i64,
            1_i64
        ],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO messages(id, session_id, role, content, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            "message-legacy",
            "session-legacy",
            "assistant",
            "hello",
            1_i64
        ],
    )
    .unwrap();
    drop(conn);

    let report = initialize_persistence(&vela_home).unwrap();
    let inspection = inspect_latest_session(&report.state_db_path, 10)
        .unwrap()
        .expect("legacy session inspection");
    assert_eq!(inspection.messages.len(), 1);
    assert_eq!(inspection.messages[0].metadata_json, None);

    let conn = Connection::open(&report.state_db_path).unwrap();
    let metadata_exists: bool = conn
        .prepare("PRAGMA table_info(messages)")
        .unwrap()
        .query_map([], |row| row.get::<_, String>(1))
        .unwrap()
        .filter_map(Result::ok)
        .any(|name| name == "metadata_json");
    assert!(metadata_exists);

    let _ = fs::remove_dir_all(&vela_home);
}
