# Persisted task lifecycle

The `vela-kernel` crate starts Vela's task/session boundary with one deliberately small capability: starting and loading an active task over the typed SQLite event log.

## Observable contract

- `TaskId` is an opaque, non-empty UTF-8 string. The store isolates task streams with the internal `task:` prefix.
- `TaskGoal` is a non-empty UTF-8 string. Whitespace is meaningful; the kernel does not trim or reinterpret caller input.
- `TaskStore::start` appends one `task.started` event at payload version `1` with `ExpectedVersion::NoStream`, then returns a task whose status is `Active`.
- `TaskStore::load` replays the task stream. It returns the same ID, goal, and active status after reopening the database. A missing stream returns `None`.
- Starting an existing ID returns `TaskStoreError::AlreadyExists` and leaves the original history unchanged. The event log's expected-version transaction also enforces this under racing writers.
- Unknown event discriminators or payload versions and malformed payloads remain explicit `ReplayError` values wrapped by `TaskStoreError::Replay`; persisted data is never skipped.
- More than one recognized start event is invalid history rather than an implicit overwrite.

`TaskStoreError` is non-exhaustive. Wrapped event-log and replay failures are exposed through `std::error::Error::source`; domain errors such as `AlreadyExists` and `InvalidHistory` have no source.

## Non-goals

This slice does not add task completion, cancellation, retries, timestamps, actors, model messages, tools, child tasks, sessions, async execution, or a runtime interface. Those require separate lifecycle events and acceptance tests rather than assumptions in the first persisted state.

See [`event-log.md`](event-log.md) for the underlying append, durability, concurrency, and replay guarantees.
