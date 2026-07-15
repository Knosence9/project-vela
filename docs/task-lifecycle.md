# Persisted task lifecycle

The `vela-kernel` crate starts Vela's task/session boundary with a deliberately small lifecycle: starting, completing, cancelling, failing, and loading a task over the typed SQLite event log.

## Observable contract

- `TaskId` is an opaque, non-empty UTF-8 string. The store isolates task streams with the internal `task:` prefix.
- `TaskGoal` is a non-empty UTF-8 string. Whitespace is meaningful; the kernel does not trim or reinterpret caller input.
- `TaskStore::start` appends one `task.started` event at payload version `1` with `ExpectedVersion::NoStream`, then returns a task whose status is `Active`.
- `TaskStore::complete` loads an existing active task and appends an empty `task.completed` event at payload version `1` with exact expected version `1`. It returns the task with `Completed` status.
- `TaskStore::cancel` loads an existing active task and appends an empty `task.cancelled` event at payload version `1` with exact expected version `1`. It returns the task with `Cancelled` status.
- `TaskStore::fail` loads an existing active task and appends an empty `task.failed` event at payload version `1` with exact expected version `1`. It returns the task with `Failed` status.
- `TaskStore::load` replays the task stream. It returns the same ID, goal, and current status after reopening the database. A missing stream returns `None`.
- Starting an existing ID returns `TaskStoreError::AlreadyExists` and leaves the original history unchanged. The event log's expected-version transaction also enforces this under racing writers.
- Completing a missing ID returns `TaskStoreError::NotFound` without creating a stream. Completing an already completed task returns `TaskStoreError::AlreadyCompleted` and leaves the completed history unchanged; exact-version appends also enforce the transition under racing writers.
- Cancelling a missing ID returns `TaskStoreError::NotFound` without creating a stream. A repeated cancellation returns `TaskStoreError::AlreadyCancelled`.
- Failing a missing ID returns `TaskStoreError::NotFound` without creating a stream. A repeated failure returns `TaskStoreError::AlreadyFailed`.
- Every terminal transition rejects a task already completed, cancelled, or failed with the error for its persisted status and preserves the winning history.
- Terminal transitions racing on the same active task persist exactly one terminal event. The losing writer reloads the winning event and reports the persisted terminal state.
- Unknown event discriminators or payload versions and malformed payloads remain explicit `ReplayError` values wrapped by `TaskStoreError::Replay`; persisted data is never skipped.
- The only valid histories are one `task.started` event, optionally followed by one terminal `task.completed`, `task.cancelled`, or `task.failed` event. Terminal-first, duplicate starts or terminal events, and events after a terminal event are invalid history rather than implicit state changes.

`TaskStoreError` is non-exhaustive. Wrapped event-log and replay failures are exposed through `std::error::Error::source`; domain errors such as `AlreadyExists`, `NotFound`, `AlreadyCompleted`, `AlreadyCancelled`, `AlreadyFailed`, and `InvalidHistory` have no source.

## Non-goals

This slice does not add a failure reason or diagnostic, cancellation reason, completion output, retries, timestamps, actors, model messages, tools, child tasks, sessions, async execution, cooperative runtime cancellation, or a runtime interface. Those require separate lifecycle events and acceptance tests rather than assumptions in the persisted state.

See [`event-log.md`](event-log.md) for the underlying append, durability, concurrency, and replay guarantees.
