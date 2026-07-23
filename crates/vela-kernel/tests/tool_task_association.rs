use serde_json::{Value, json};
use tempfile::tempdir;
use vela_kernel::{
    event_log::ReplayError,
    task::{TaskCancellation, TaskFailure, TaskGoal, TaskId, TaskOutput, TaskStore},
    tool::{
        DurableToolInvocationError, PermissionDecision, Tool, ToolAuthorizer, ToolEffect,
        ToolError, ToolId, ToolInvocationId, ToolInvocationStore, ToolInvocationStoreError,
        ToolRequest, invoke_tool_durable, invoke_tool_for_task_durable,
    },
};

struct FakeTool {
    id: ToolId,
    calls: usize,
}

impl FakeTool {
    fn new() -> Self {
        Self {
            id: ToolId::new("test.echo").unwrap(),
            calls: 0,
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

    fn invoke(&mut self, input: &Value) -> Result<Value, ToolError> {
        self.calls += 1;
        Ok(input.clone())
    }
}

struct FakeAuthorizer {
    calls: usize,
}

impl FakeAuthorizer {
    fn new() -> Self {
        Self { calls: 0 }
    }
}

impl ToolAuthorizer for FakeAuthorizer {
    fn authorize(&mut self, _request: ToolRequest<'_>) -> PermissionDecision {
        self.calls += 1;
        PermissionDecision::Allow
    }
}

fn task_id(value: &str) -> TaskId {
    TaskId::new(value).unwrap()
}

fn invocation_id(value: &str) -> ToolInvocationId {
    ToolInvocationId::new(value).unwrap()
}

#[test]
fn associated_invocation_projects_immutable_task_id_after_reopening() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let task_id = task_id("task-1");
    TaskStore::open(&path)
        .unwrap()
        .start(task_id.clone(), TaskGoal::new("exercise tool").unwrap())
        .unwrap();
    let invocation_id = invocation_id("call-1");
    let mut store = ToolInvocationStore::open(&path).unwrap();
    let mut tool = FakeTool::new();
    let mut authorizer = FakeAuthorizer::new();

    let output = invoke_tool_for_task_durable(
        &mut store,
        &task_id,
        invocation_id.clone(),
        &mut tool,
        &mut authorizer,
        &json!({"secret": "not persisted"}),
    )
    .unwrap();

    assert_eq!(output, json!({"secret": "not persisted"}));
    assert_eq!(authorizer.calls, 1);
    assert_eq!(tool.calls, 1);
    TaskStore::open(&path)
        .unwrap()
        .complete(&task_id, TaskOutput::new("finished later").unwrap())
        .unwrap();
    let reopened = ToolInvocationStore::open(&path).unwrap();
    assert_eq!(
        reopened.load(&invocation_id).unwrap().unwrap().task_id(),
        Some(&task_id)
    );
    assert_eq!(reopened.list().unwrap()[0].task_id(), Some(&task_id));

    let events: Vec<(u32, Vec<u8>)> = rusqlite::Connection::open(&path)
        .unwrap()
        .prepare(
            "SELECT payload_version, payload FROM events WHERE stream_id = 'tool-invocation:call-1' ORDER BY stream_version",
        )
        .unwrap()
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(events[0].0, 2);
    assert_eq!(
        events[0].1,
        br#"{"tool_id":"test.echo","effect":"pure","task_id":"task-1"}"#
    );
    let persisted = String::from_utf8(
        events
            .into_iter()
            .flat_map(|(_, payload)| payload)
            .collect(),
    )
    .unwrap();
    assert!(persisted.contains("task-1"));
    assert!(!persisted.contains("not persisted"));
}

#[test]
fn unassociated_invocation_projects_no_task_id() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let id = invocation_id("unassociated");
    let mut store = ToolInvocationStore::open(&path).unwrap();
    invoke_tool_durable(
        &mut store,
        id.clone(),
        &mut FakeTool::new(),
        &mut FakeAuthorizer::new(),
        &json!(null),
    )
    .unwrap();

    assert_eq!(store.load(&id).unwrap().unwrap().task_id(), None);
}

#[test]
fn missing_and_terminal_tasks_fail_before_intent_authorization_or_execution() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let mut tasks = TaskStore::open(&path).unwrap();
    let completed = task_id("completed");
    tasks
        .start(completed.clone(), TaskGoal::new("complete").unwrap())
        .unwrap();
    tasks
        .complete(&completed, TaskOutput::new("done").unwrap())
        .unwrap();
    let cancelled = task_id("cancelled");
    tasks
        .start(cancelled.clone(), TaskGoal::new("cancel").unwrap())
        .unwrap();
    tasks
        .cancel(&cancelled, TaskCancellation::new("stopped").unwrap())
        .unwrap();
    let failed = task_id("failed");
    tasks
        .start(failed.clone(), TaskGoal::new("fail").unwrap())
        .unwrap();
    tasks
        .fail(&failed, TaskFailure::new("broken").unwrap())
        .unwrap();
    drop(tasks);

    for id in [task_id("missing"), completed, cancelled, failed] {
        let mut store = ToolInvocationStore::open(&path).unwrap();
        let mut tool = FakeTool::new();
        let mut authorizer = FakeAuthorizer::new();
        let invocation_id = invocation_id(&format!("for-{id}"));

        let error = invoke_tool_for_task_durable(
            &mut store,
            &id,
            invocation_id.clone(),
            &mut tool,
            &mut authorizer,
            &json!(null),
        )
        .unwrap_err();

        assert!(matches!(
            error,
            DurableToolInvocationError::Store(
                ToolInvocationStoreError::TaskNotFound { .. }
                    | ToolInvocationStoreError::TaskNotActive { .. }
            )
        ));
        assert_eq!(authorizer.calls, 0);
        assert_eq!(tool.calls, 0);
        assert!(store.load(&invocation_id).unwrap().is_none());
    }
}

#[test]
fn duplicate_associated_invocation_fails_before_authorization_or_execution() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    let task_id = task_id("task-1");
    TaskStore::open(&path)
        .unwrap()
        .start(task_id.clone(), TaskGoal::new("exercise tool").unwrap())
        .unwrap();
    let id = invocation_id("duplicate");
    let mut store = ToolInvocationStore::open(&path).unwrap();
    invoke_tool_for_task_durable(
        &mut store,
        &task_id,
        id.clone(),
        &mut FakeTool::new(),
        &mut FakeAuthorizer::new(),
        &json!(null),
    )
    .unwrap();
    let mut tool = FakeTool::new();
    let mut authorizer = FakeAuthorizer::new();

    let error = invoke_tool_for_task_durable(
        &mut store,
        &task_id,
        id.clone(),
        &mut tool,
        &mut authorizer,
        &json!(null),
    )
    .unwrap_err();

    assert!(matches!(
        error,
        DurableToolInvocationError::Store(ToolInvocationStoreError::AlreadyExists {
            invocation_id
        }) if invocation_id == id
    ));
    assert_eq!(authorizer.calls, 0);
    assert_eq!(tool.calls, 0);
}

#[test]
fn malformed_associated_task_id_fails_closed_with_a_sourced_replay_error() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("events.sqlite3");
    ToolInvocationStore::open(&path).unwrap();
    rusqlite::Connection::open(&path)
        .unwrap()
        .execute(
            "INSERT INTO events VALUES ('tool-invocation:malformed-task', 1, 'tool.invocation_intended', 2, ?1)",
            [br#"{"tool_id":"test.echo","effect":"pure","task_id":""}"#.as_slice()],
        )
        .unwrap();

    let error = ToolInvocationStore::open(&path)
        .unwrap()
        .load(&invocation_id("malformed-task"))
        .unwrap_err();

    assert!(matches!(
        error,
        ToolInvocationStoreError::Replay(ReplayError::MalformedPayload {
            stream_version: 1,
            ..
        })
    ));
    assert!(std::error::Error::source(&error).is_some());
}
