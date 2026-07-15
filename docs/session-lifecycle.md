# Persisted session lifecycle

The `vela-kernel` crate starts Vela's session boundary with durable creation and load over the typed SQLite event log.

## Observable contract

- `SessionId` is an opaque, non-empty UTF-8 string. The store isolates session streams with the internal `session:` prefix, so task and session IDs with the same external value do not collide.
- `SessionTitle` is a non-empty UTF-8 string. Whitespace is meaningful; the kernel persists and returns the title without trimming or reinterpretation.
- `SessionStore::create` appends one `session.created` event at payload version `1` with `ExpectedVersion::NoStream`, then returns a session whose status is `Open`.
- `SessionStore::load` replays the session stream. It returns the same ID, title, and status after reopening the database. A missing stream returns `None`.
- Creating an existing ID returns `SessionStoreError::AlreadyExists` and leaves the original history unchanged. The event log's expected-version transaction also enforces this under racing writers.
- Unknown event discriminators or payload versions and malformed payloads, including an empty persisted title, remain explicit `ReplayError` values wrapped by `SessionStoreError::Replay`; persisted data is never skipped.
- The only valid history is one `session.created` event. Duplicate creation events are invalid history rather than implicit state changes.

`SessionStoreError` is non-exhaustive. Wrapped event-log and replay failures are exposed through `std::error::Error::source`; domain errors such as `AlreadyExists` and `InvalidHistory` have no source.

## Non-goals

This slice does not add session close, resume, or continue behavior; task membership; messages; turns; branching; compression; timestamps; actors; metadata; a runtime interface; or async execution. Those require separate lifecycle events and acceptance tests rather than assumptions in persisted state.

See [`event-log.md`](event-log.md) for the underlying append, durability, concurrency, and replay guarantees.
