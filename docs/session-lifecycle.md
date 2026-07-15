# Persisted session lifecycle

The `vela-kernel` crate starts Vela's session boundary with durable creation, close, and load over the typed SQLite event log.

## Observable contract

- `SessionId` is an opaque, non-empty UTF-8 string. The store isolates session streams with the internal `session:` prefix, so task and session IDs with the same external value do not collide.
- `SessionTitle` is a non-empty UTF-8 string. Whitespace is meaningful; the kernel persists and returns the title without trimming or reinterpretation.
- `SessionStore::create` appends one `session.created` event at payload version `1` with `ExpectedVersion::NoStream`, then returns a session whose status is `Open`.
- `SessionStore::close` appends one empty `session.closed` event at payload version `1` to an existing open session, then returns the same identity and title with status `Closed`.
- `SessionStore::load` replays the session stream. It returns the same ID, title, and status after reopening the database. A missing stream returns `None`.
- Closing a missing session returns `SessionStoreError::NotFound` without creating a stream. Closing an already closed session returns `AlreadyClosed` without appending another event; racing close attempts persist exactly one close event and the loser receives `AlreadyClosed`.
- Creating an existing ID returns `SessionStoreError::AlreadyExists` and leaves the original history unchanged. The event log's expected-version transaction also enforces creation and close transitions under racing writers.
- Unknown event discriminators or payload versions and malformed payloads, including an empty persisted title, remain explicit `ReplayError` values wrapped by `SessionStoreError::Replay`; persisted data is never skipped.
- The valid histories are one `session.created` event for an open session or `session.created` followed by `session.closed` for a closed session. Close without creation, duplicate creation, duplicate close, and any other ordering are invalid history rather than implicit state changes.

`SessionStoreError` is non-exhaustive. Wrapped event-log and replay failures are exposed through `std::error::Error::source`; domain errors such as `AlreadyExists`, `NotFound`, `AlreadyClosed`, and `InvalidHistory` have no source.

## Non-goals

This slice does not add session resume or reopen behavior; close reasons or summaries; task membership; messages; turns; branching; compression; timestamps; actors; metadata; a runtime interface; or async execution. Those require separate lifecycle events and acceptance tests rather than assumptions in persisted state.

See [`event-log.md`](event-log.md) for the underlying append, durability, concurrency, and replay guarantees.
