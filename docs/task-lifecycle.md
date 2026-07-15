# Persisted task lifecycle

The `vela-kernel` crate provides a deliberately small task lifecycle: starting, completing with an output, cancelling with a reason, failing with a diagnostic, associating with a session, and loading a task over the typed SQLite event log.

## Observable contract

- `TaskId` is an opaque, non-empty UTF-8 string. The store isolates task streams with the internal `task:` prefix.
- `TaskGoal` is a non-empty UTF-8 string. Whitespace is meaningful; the kernel does not trim or reinterpret caller input.
- `TaskOutput` is a non-empty UTF-8 completion output. Whitespace is meaningful; the kernel persists and returns the value without trimming or reinterpretation.
- `TaskCancellation` is a non-empty UTF-8 cancellation reason. Whitespace is meaningful; the kernel persists and returns the value without trimming or reinterpretation.
- `TaskFailure` is a non-empty UTF-8 diagnostic. Whitespace is meaningful; the kernel persists and returns the value without trimming or reinterpretation.
- A task starts without a session association. `TaskStore::associate_session` requires an existing task and an existing open session, then appends one `task.session_associated` event at payload version `1`. Its payload carries both identifiers, and replay projects the session identifier into the task. The append atomically guards both the task version and the observed session version, so a racing session close either follows a valid association or causes association to reload and reject the closed session.
- Association is immutable. Repeating it with either the same or another target returns `TaskStoreError::AlreadyAssociated` with the persisted session identifier and appends no event. A missing target returns `SessionNotFound`, and a closed target returns `SessionClosed`, also without appending.
- Association is independent of task terminal status: active and terminal tasks may be associated, and a later task terminal transition preserves an existing association. Closing or reopening the session does not alter the task stream or projected association.
- `TaskStore::start` appends one `task.started` event at payload version `1` with `ExpectedVersion::NoStream`, then returns a task whose status is `Active`.
- `TaskStore::complete` loads an existing active task and appends a `task.completed` event containing the supplied output at payload version `2` with the exact current stream version. It returns the task with `Completed` status and the same output.
- `TaskStore::cancel` loads an existing active task and appends a `task.cancelled` event containing the supplied reason at payload version `2` with the exact current stream version. It returns the task with `Cancelled` status and the same reason.
- `TaskStore::fail` loads an existing active task and appends a `task.failed` event containing the supplied diagnostic at payload version `2` with the exact current stream version. It returns the task with `Failed` status and the same diagnostic.
- `TaskStore::load` replays the task stream. It returns the same ID, goal, and current status after reopening the database. A missing stream returns `None`.
- Newly completed tasks expose the output persisted by their completion event. Legacy payload-version-1 completion events with the old empty payload remain replayable and expose no output. Active, cancelled, and failed tasks expose no completion output.
- Newly cancelled tasks expose the reason persisted by their cancellation event. Legacy payload-version-1 cancellation events with the old empty payload remain replayable and expose no reason. Active, completed, and failed tasks expose no cancellation reason.
- Active, completed, and cancelled tasks expose no failure diagnostic. Newly failed tasks expose the diagnostic persisted by their failure event. Legacy payload-version-1 failure events with the old empty payload remain replayable and expose no diagnostic.
- Starting an existing ID returns `TaskStoreError::AlreadyExists` and leaves the original history unchanged. The event log's expected-version transaction also enforces this under racing writers.
- Completing a missing ID returns `TaskStoreError::NotFound` without creating a stream. Completing an already completed task returns `TaskStoreError::AlreadyCompleted` and leaves the completed history unchanged; exact-version appends also enforce the transition under racing writers.
- Cancelling a missing ID returns `TaskStoreError::NotFound` without creating a stream. A repeated cancellation returns `TaskStoreError::AlreadyCancelled`.
- Failing a missing ID returns `TaskStoreError::NotFound` without creating a stream. A repeated failure returns `TaskStoreError::AlreadyFailed`.
- Every terminal transition rejects a task already completed, cancelled, or failed with the error for its persisted status and preserves the winning history.
- Terminal transitions racing on the same active task persist exactly one terminal event. The losing writer reloads the winning event and reports the persisted terminal state.
- Unknown event discriminators or payload versions and malformed payloads, including an empty persisted completion output, cancellation reason, failure diagnostic, or association identifier, remain explicit `ReplayError` values wrapped by `TaskStoreError::Replay`; persisted data is never skipped.
- Valid histories start with one `task.started` event, contain at most one `task.session_associated` event, and contain at most one terminal `task.completed`, `task.cancelled`, or `task.failed` event. Association may appear before or after the terminal event. Terminal-first, duplicate starts, associations, or terminal events, events after a terminal event other than the single association, and association payloads naming a different task are invalid history rather than implicit state changes.

`TaskStoreError` is non-exhaustive. Wrapped event-log, replay, and session-store failures are exposed through `std::error::Error::source`; domain errors such as `AlreadyExists`, `NotFound`, `AlreadyCompleted`, `AlreadyCancelled`, `AlreadyFailed`, `SessionNotFound`, `SessionClosed`, `AlreadyAssociated`, and `InvalidHistory` have no source.

## Non-goals

This slice does not add a structured result, failure, or cancellation taxonomy, artifacts, stack traces, retry policy, timestamps, actors, model messages, tools, child tasks, moving or detaching associations, session-side task indexes, async execution, cooperative runtime cancellation, or a runtime interface. Those require separate lifecycle events and acceptance tests rather than assumptions in the persisted state.

See [`event-log.md`](event-log.md) for the underlying append, durability, concurrency, and replay guarantees.
