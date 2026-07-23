use std::{error::Error, fmt, path::PathBuf};

use serde_json::{Value, json};
use tempfile::tempdir;
use vela_kernel::{
    event_log::ReplayError,
    tool::{
        DurableToolInvocationError, PermissionDecision, Tool, ToolAuthorizer, ToolEffect,
        ToolError, ToolId, ToolInvocationError, ToolInvocationId, ToolInvocationStatus,
        ToolInvocationStore, ToolInvocationStoreError, ToolRequest, invoke_tool_durable,
    },
};

#[derive(Debug)]
struct FakeToolError;

impl fmt::Display for FakeToolError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("sensitive adapter diagnostic")
    }
}

impl Error for FakeToolError {}

struct FakeTool {
    id: ToolId,
    calls: usize,
    result: Option<Result<Value, ToolError>>,
}

impl FakeTool {
    fn succeeding(output: Value) -> Self {
        Self {
            id: ToolId::new("test.echo").unwrap(),
            calls: 0,
            result: Some(Ok(output)),
        }
    }

    fn failing() -> Self {
        Self {
            id: ToolId::new("test.echo").unwrap(),
            calls: 0,
            result: Some(Err(ToolError::new(FakeToolError))),
        }
    }
}

impl Tool for FakeTool {
    fn id(&self) -> &ToolId {
        &self.id
    }

    fn effect(&self) -> ToolEffect {
        ToolEffect::Pure
    }

    fn invoke(&mut self, _input: &Value) -> Result<Value, ToolError> {
        self.calls += 1;
        self.result.take().expect("tool must not be retried")
    }
}

struct FakeAuthorizer {
    decision: PermissionDecision,
    calls: usize,
    observed_intent: bool,
    database: Option<PathBuf>,
}

impl FakeAuthorizer {
    fn new(decision: PermissionDecision) -> Self {
        Self {
            decision,
            calls: 0,
            observed_intent: false,
            database: None,
        }
    }

    fn blocking_terminal_append(database: PathBuf) -> Self {
        Self {
            decision: PermissionDecision::Allow,
            calls: 0,
            observed_intent: false,
            database: Some(database),
        }
    }
}

impl ToolAuthorizer for FakeAuthorizer {
    fn authorize(&mut self, request: ToolRequest<'_>) -> PermissionDecision {
        self.calls += 1;
        if let Some(path) = &self.database {
            let connection = rusqlite::Connection::open(path).unwrap();
            self.observed_intent = connection
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM events WHERE stream_id = 'tool-invocation:terminal-write-fails' AND event_type = 'tool.invocation_intended')",
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            connection
                .execute_batch(
                    "CREATE TRIGGER reject_tool_terminal
                     BEFORE INSERT ON events
                     WHEN NEW.stream_id = 'tool-invocation:terminal-write-fails'
                          AND NEW.stream_version = 2
                     BEGIN
                         SELECT RAISE(ABORT, 'terminal append blocked');
                     END;",
                )
                .unwrap();
        }
        assert_eq!(request.effect(), ToolEffect::Pure);
        self.decision
    }
}

fn invocation_id(value: &str) -> ToolInvocationId {
    ToolInvocationId::new(value).unwrap()
}

#[test]
fn invocation_ids_are_non_blank_stable_values() {
    assert!(ToolInvocationId::new("").is_err());
    assert!(ToolInvocationId::new(" \t\n").is_err());
    let id = invocation_id("call-1");
    assert_eq!(id.as_str(), "call-1");
    assert_eq!(id.to_string(), "call-1");
}

#[test]
fn invocations_list_in_id_order_with_latest_status_after_reopening() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let mut store = ToolInvocationStore::open(&path).unwrap();
    assert!(store.list().unwrap().is_empty());

    let mut successful_tool = FakeTool::succeeding(json!({"secret": "not persisted"}));
    invoke_tool_durable(
        &mut store,
        invocation_id("charlie"),
        &mut successful_tool,
        &mut FakeAuthorizer::new(PermissionDecision::Allow),
        &json!(null),
    )
    .unwrap();
    let mut denied_tool = FakeTool::succeeding(json!(null));
    invoke_tool_durable(
        &mut store,
        invocation_id("alpha"),
        &mut denied_tool,
        &mut FakeAuthorizer::new(PermissionDecision::Deny),
        &json!(null),
    )
    .unwrap_err();
    let mut failed_tool = FakeTool::failing();
    invoke_tool_durable(
        &mut store,
        invocation_id("delta"),
        &mut failed_tool,
        &mut FakeAuthorizer::new(PermissionDecision::Allow),
        &json!(null),
    )
    .unwrap_err();
    rusqlite::Connection::open(&path)
        .unwrap()
        .execute_batch(
            "INSERT INTO events VALUES ('tool-invocation:bravo', 1, 'tool.invocation_intended', 1, X'7B22746F6F6C5F6964223A22746573742E6563686F222C22656666656374223A2270757265227D');
             INSERT INTO events VALUES ('task:alpha', 1, 'task.started', 1, X'7B22676F616C223A2269676E6F7265227D');
             INSERT INTO events VALUES ('session:charlie', 1, 'session.created', 1, X'7B227469746C65223A2269676E6F7265227D');",
        )
        .unwrap();
    let event_count_before: i64 = rusqlite::Connection::open(&path)
        .unwrap()
        .query_row("SELECT COUNT(*) FROM events", [], |row| row.get(0))
        .unwrap();

    let invocations = ToolInvocationStore::open(&path).unwrap().list().unwrap();

    let actual: Vec<_> = invocations
        .iter()
        .map(|invocation| (invocation.id().as_str(), invocation.status()))
        .collect();
    assert_eq!(
        actual,
        vec![
            ("alpha", ToolInvocationStatus::Denied),
            ("bravo", ToolInvocationStatus::Pending),
            ("charlie", ToolInvocationStatus::Succeeded),
            ("delta", ToolInvocationStatus::Failed),
        ]
    );
    let event_count_after: i64 = rusqlite::Connection::open(&path)
        .unwrap()
        .query_row("SELECT COUNT(*) FROM events", [], |row| row.get(0))
        .unwrap();
    assert_eq!(event_count_after, event_count_before);
}

#[test]
fn listing_rejects_a_malformed_owning_stream_id() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    ToolInvocationStore::open(&path).unwrap();
    rusqlite::Connection::open(&path)
        .unwrap()
        .execute(
            "INSERT INTO events VALUES ('wrong-prefix', 1, 'tool.invocation_intended', 1, X'7B22746F6F6C5F6964223A22746573742E6563686F222C22656666656374223A2270757265227D')",
            [],
        )
        .unwrap();

    let error = ToolInvocationStore::open(&path)
        .unwrap()
        .list()
        .unwrap_err();

    assert!(matches!(
        error,
        ToolInvocationStoreError::InvalidStreamId { stream_id } if stream_id == "wrong-prefix"
    ));
}

#[test]
fn denial_is_persisted_after_intent_without_calling_the_tool() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let id = invocation_id("denied");
    let mut store = ToolInvocationStore::open(&path).unwrap();
    let mut tool = FakeTool::succeeding(json!({"secret_output": "must not run"}));
    let mut authorizer = FakeAuthorizer::new(PermissionDecision::Deny);

    let error = invoke_tool_durable(
        &mut store,
        id.clone(),
        &mut tool,
        &mut authorizer,
        &json!({"credential": "input-secret"}),
    )
    .unwrap_err();

    assert!(matches!(
        error,
        DurableToolInvocationError::Invocation(ToolInvocationError::Denied { .. })
    ));
    assert_eq!(authorizer.calls, 1);
    assert_eq!(tool.calls, 0);
    let invocation = ToolInvocationStore::open(&path)
        .unwrap()
        .load(&id)
        .unwrap()
        .unwrap();
    assert_eq!(invocation.id(), &id);
    assert_eq!(invocation.tool_id().as_str(), "test.echo");
    assert_eq!(invocation.effect(), ToolEffect::Pure);
    assert_eq!(invocation.status(), ToolInvocationStatus::Denied);

    let events: Vec<(String, Vec<u8>)> = rusqlite::Connection::open(&path)
        .unwrap()
        .prepare("SELECT event_type, payload FROM events ORDER BY stream_version")
        .unwrap()
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(events[0].0, "tool.invocation_intended");
    assert_eq!(events[0].1, br#"{"tool_id":"test.echo","effect":"pure"}"#);
    assert_eq!(
        events[1],
        ("tool.invocation_denied".to_owned(), b"{}".to_vec())
    );
    let persisted = events
        .iter()
        .flat_map(|(_, payload)| payload)
        .copied()
        .collect::<Vec<_>>();
    let persisted = String::from_utf8(persisted).unwrap();
    assert!(!persisted.contains("input-secret"));
}

#[test]
fn success_returns_exact_output_and_replays_metadata_only_status() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let id = invocation_id("success");
    let output = json!({"credential": "output-secret"});
    let mut store = ToolInvocationStore::open(&path).unwrap();
    let mut tool = FakeTool::succeeding(output.clone());
    let mut authorizer = FakeAuthorizer::new(PermissionDecision::Allow);

    let actual = invoke_tool_durable(
        &mut store,
        id.clone(),
        &mut tool,
        &mut authorizer,
        &json!({"credential": "input-secret"}),
    )
    .unwrap();

    assert_eq!(actual, output);
    assert_eq!(authorizer.calls, 1);
    assert_eq!(tool.calls, 1);
    assert_eq!(
        ToolInvocationStore::open(&path)
            .unwrap()
            .load(&id)
            .unwrap()
            .unwrap()
            .status(),
        ToolInvocationStatus::Succeeded
    );
    let payloads: Vec<Vec<u8>> = rusqlite::Connection::open(&path)
        .unwrap()
        .prepare("SELECT payload FROM events ORDER BY stream_version")
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    let persisted = String::from_utf8(payloads.into_iter().flatten().collect()).unwrap();
    assert!(!persisted.contains("input-secret"));
    assert!(!persisted.contains("output-secret"));
}

#[test]
fn adapter_failure_is_sourced_and_persists_no_diagnostic() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let id = invocation_id("failed");
    let mut store = ToolInvocationStore::open(&path).unwrap();
    let mut tool = FakeTool::failing();
    let mut authorizer = FakeAuthorizer::new(PermissionDecision::Allow);

    let error = invoke_tool_durable(
        &mut store,
        id.clone(),
        &mut tool,
        &mut authorizer,
        &json!(null),
    )
    .unwrap_err();

    assert!(matches!(
        error,
        DurableToolInvocationError::Invocation(ToolInvocationError::Tool { .. })
    ));
    assert_eq!(
        error.source().unwrap().to_string(),
        "tool test.echo failed: sensitive adapter diagnostic"
    );
    assert_eq!(tool.calls, 1);
    assert_eq!(
        ToolInvocationStore::open(&path)
            .unwrap()
            .load(&id)
            .unwrap()
            .unwrap()
            .status(),
        ToolInvocationStatus::Failed
    );
    let payloads: Vec<Vec<u8>> = rusqlite::Connection::open(&path)
        .unwrap()
        .prepare("SELECT payload FROM events")
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert!(
        !String::from_utf8(payloads.into_iter().flatten().collect())
            .unwrap()
            .contains("sensitive adapter diagnostic")
    );
}

#[test]
fn duplicate_invocation_id_fails_before_authorization_or_execution() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let id = invocation_id("duplicate");
    let mut store = ToolInvocationStore::open(&path).unwrap();
    let mut first_tool = FakeTool::succeeding(json!(1));
    invoke_tool_durable(
        &mut store,
        id.clone(),
        &mut first_tool,
        &mut FakeAuthorizer::new(PermissionDecision::Allow),
        &json!(null),
    )
    .unwrap();
    let mut second_tool = FakeTool::succeeding(json!(2));
    let mut second_authorizer = FakeAuthorizer::new(PermissionDecision::Allow);

    let error = invoke_tool_durable(
        &mut store,
        id.clone(),
        &mut second_tool,
        &mut second_authorizer,
        &json!(null),
    )
    .unwrap_err();

    assert!(matches!(
        error,
        DurableToolInvocationError::Store(ToolInvocationStoreError::AlreadyExists { invocation_id })
            if invocation_id == id
    ));
    assert_eq!(second_authorizer.calls, 0);
    assert_eq!(second_tool.calls, 0);
}

#[test]
fn intent_only_history_replays_pending_without_resuming() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    ToolInvocationStore::open(&path).unwrap();
    rusqlite::Connection::open(&path)
        .unwrap()
        .execute(
            "INSERT INTO events VALUES ('tool-invocation:pending', 1, 'tool.invocation_intended', 1, X'7B22746F6F6C5F6964223A22746573742E6563686F222C22656666656374223A2270757265227D')",
            [],
        )
        .unwrap();

    let invocation = ToolInvocationStore::open(&path)
        .unwrap()
        .load(&invocation_id("pending"))
        .unwrap()
        .unwrap();

    assert_eq!(invocation.status(), ToolInvocationStatus::Pending);
    assert_eq!(invocation.tool_id().as_str(), "test.echo");
}

#[test]
fn malformed_repeated_and_post_terminal_histories_fail_closed() {
    for (name, rows) in [
        (
            "terminal-first",
            vec![(1, "tool.invocation_succeeded", "{}")],
        ),
        (
            "repeated-terminal",
            vec![
                (
                    1,
                    "tool.invocation_intended",
                    r#"{"tool_id":"test.echo","effect":"pure"}"#,
                ),
                (2, "tool.invocation_failed", "{}"),
                (3, "tool.invocation_succeeded", "{}"),
            ],
        ),
        (
            "repeated-intent",
            vec![
                (
                    1,
                    "tool.invocation_intended",
                    r#"{"tool_id":"test.echo","effect":"pure"}"#,
                ),
                (
                    2,
                    "tool.invocation_intended",
                    r#"{"tool_id":"test.echo","effect":"pure"}"#,
                ),
            ],
        ),
    ] {
        let directory = tempdir().unwrap();
        let path = directory.path().join("events.sqlite3");
        ToolInvocationStore::open(&path).unwrap();
        let connection = rusqlite::Connection::open(&path).unwrap();
        for (version, event_type, payload) in rows {
            connection
                .execute(
                    "INSERT INTO events VALUES (?1, ?2, ?3, 1, ?4)",
                    rusqlite::params![
                        format!("tool-invocation:{name}"),
                        version,
                        event_type,
                        payload.as_bytes()
                    ],
                )
                .unwrap();
        }

        assert!(matches!(
            ToolInvocationStore::open(&path)
                .unwrap()
                .load(&invocation_id(name))
                .unwrap_err(),
            ToolInvocationStoreError::InvalidHistory { .. }
        ));
    }
}

#[test]
fn malformed_intent_metadata_is_a_source_preserving_replay_error() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    ToolInvocationStore::open(&path).unwrap();
    rusqlite::Connection::open(&path)
        .unwrap()
        .execute(
            "INSERT INTO events VALUES ('tool-invocation:malformed', 1, 'tool.invocation_intended', 1, X'7B22746F6F6C5F6964223A222020222C22656666656374223A2270757265227D')",
            [],
        )
        .unwrap();

    let error = ToolInvocationStore::open(&path)
        .unwrap()
        .load(&invocation_id("malformed"))
        .unwrap_err();

    assert!(matches!(
        error,
        ToolInvocationStoreError::Replay(ReplayError::MalformedPayload {
            stream_version: 1,
            ..
        })
    ));
    assert!(error.source().is_some());

    let list_error = ToolInvocationStore::open(&path)
        .unwrap()
        .list()
        .unwrap_err();
    assert!(matches!(
        list_error,
        ToolInvocationStoreError::Replay(ReplayError::MalformedPayload {
            stream_version: 1,
            ..
        })
    ));
    assert!(list_error.source().is_some());
}

#[test]
fn terminal_append_failure_returns_exact_result_without_retry() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let output = json!({"result": "still available"});
    let mut store = ToolInvocationStore::open(&path).unwrap();
    let mut tool = FakeTool::succeeding(output.clone());
    let mut authorizer = FakeAuthorizer::blocking_terminal_append(path.clone());

    let error = invoke_tool_durable(
        &mut store,
        invocation_id("terminal-write-fails"),
        &mut tool,
        &mut authorizer,
        &json!(null),
    )
    .unwrap_err();

    let DurableToolInvocationError::TerminalPersistence { result, error } = error else {
        panic!("expected terminal persistence error");
    };
    assert_eq!(result.unwrap(), output);
    assert!(matches!(error, ToolInvocationStoreError::EventLog(_)));
    assert!(authorizer.observed_intent);
    assert_eq!(authorizer.calls, 1);
    assert_eq!(tool.calls, 1);
    assert_eq!(
        ToolInvocationStore::open(&path)
            .unwrap()
            .load(&invocation_id("terminal-write-fails"))
            .unwrap()
            .unwrap()
            .status(),
        ToolInvocationStatus::Pending
    );
}
