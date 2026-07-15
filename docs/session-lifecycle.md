# Persisted session lifecycle

The `vela-kernel` crate starts Vela's session boundary with durable creation, close, reopen, and load over the typed SQLite event log.

## Observable contract

- `SessionId` is an opaque, non-empty UTF-8 string. The store isolates session streams with the internal `session:` prefix, so task and session IDs with the same external value do not collide.
- `SessionTitle` is a non-empty UTF-8 string. Whitespace is meaningful; the kernel persists and returns the title without trimming or reinterpretation.
- `SessionSummary` is a non-empty UTF-8 string. Whitespace is meaningful; the kernel persists and returns the summary without trimming or reinterpretation.
- `SessionClosure` is a non-empty UTF-8 close reason. Whitespace is meaningful; the kernel persists and returns the reason without trimming or reinterpretation.
- `SessionReopenReason` is a non-empty UTF-8 reopen reason. Whitespace is meaningful; the kernel persists and returns the reason without trimming or reinterpretation.
- `SessionStore::create` appends one `session.created` event at payload version `1` with `ExpectedVersion::NoStream`, then returns a session whose status is `Open`.
- `SessionStore::rename` appends one `session.renamed` event at payload version `1` to an existing open or closed session. It preserves the current status and active close or reopen reason, and repeated renames project the latest title.
- `SessionStore::summarize` appends one `session.summarized` event at payload version `1` to an existing open or closed session. It preserves the title, current status, and active close or reopen reason; repeated summaries project the latest summary. Concurrent summaries retry from the latest expected version, so both valid updates persist in a consistent history.
- `SessionStore::close` requires a close reason and appends one `session.closed` event containing it at payload version `2` to an existing open session, then returns the same identity and title with status `Closed` and the persisted reason. Legacy payload-version-1 close events remain replayable and expose no reason.
- `SessionStore::reopen` requires a reopen reason and appends one `session.reopened` event containing it at payload version `2` to an existing closed session, then returns the same identity and title with status `Open`. Legacy payload-version-1 reopen events remain replayable and expose no reason. Sessions may repeat valid close/reopen transitions.
- Newly created sessions expose no summary and neither a close nor reopen reason. Reopened sessions expose no active close reason and expose the reason from their latest reopen event. Closed sessions expose no active reopen reason and expose the reason from their latest close event.
- `SessionStore::load` replays the session stream. It returns the same ID, title, latest summary, and status after reopening the database. A missing stream returns `None`.
- Existing tasks may persist an immutable association to an open session through `TaskStore::associate_session`. A session may be referenced by zero or more tasks. Session close and reopen events do not mutate those task streams.
- Renaming a missing session returns `SessionStoreError::NotFound` without creating a stream.
- Summarizing a missing session returns `SessionStoreError::NotFound` without creating a stream.
- Closing a missing session returns `SessionStoreError::NotFound` without creating a stream. Closing an already closed session returns `AlreadyClosed` without appending another event; racing close attempts persist exactly one close event and the loser receives `AlreadyClosed`.
- Reopening a missing session returns `SessionStoreError::NotFound` without creating a stream. Reopening an already open session returns `AlreadyOpen` without appending another event; racing reopen attempts persist exactly one reopen event and the loser receives `AlreadyOpen`.
- Creating an existing ID returns `SessionStoreError::AlreadyExists` and leaves the original history unchanged. The event log's expected-version transaction also enforces create, rename, summarize, close, and reopen writes under racing writers.
- Unknown event discriminators or payload versions and malformed payloads, including an empty persisted title in creation or rename events, empty persisted summary, payload-version-2 close reason, or payload-version-2 reopen reason, remain explicit `ReplayError` values wrapped by `SessionStoreError::Replay`; persisted data is never skipped.
- Valid histories start with one `session.created` event, allow title and summary updates after creation, and otherwise alternate `session.closed` and `session.reopened`. Rename, summary, close, or reopen without creation, duplicate creation, duplicate close, duplicate reopen, and any other ordering are invalid history rather than implicit state changes.

`SessionStoreError` is non-exhaustive. Wrapped event-log and replay failures are exposed through `std::error::Error::source`; domain errors such as `AlreadyExists`, `NotFound`, `AlreadyClosed`, `AlreadyOpen`, and `InvalidHistory` have no source.

## Non-goals

This slice does not add automatic or model-generated summaries, structured close- or reopen-reason taxonomies, a session-side task membership index, moving or detaching tasks, messages, turns, branching, compression, timestamps, actors, metadata, a runtime interface, or async execution. Those require separate lifecycle events and acceptance tests rather than assumptions in persisted state.

See [`event-log.md`](event-log.md) for the underlying append, durability, concurrency, and replay guarantees.
