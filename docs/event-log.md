# Typed event log

The `vela-kernel` crate contains Vela's first persistence primitive: a synchronous SQLite append-only log with typed replay.

## Observable contract

- `StreamId` accepts any non-empty UTF-8 string and treats it as opaque.
- Event payload versions start at `1`; append rejects version `0` before opening a write transaction.
- A new stream accepts `ExpectedVersion::NoStream`; an existing stream accepts only `ExpectedVersion::Exact(current)`.
- Successful appends receive versions 1, 2, 3, and so on. A stale expectation returns `WrongExpectedVersion` and commits nothing.
- Every append is one SQLite transaction. The connection uses WAL journaling and `synchronous=FULL`, and success is returned only after commit.
- Replay returns one stream in ascending, contiguous order as values decoded by the caller's typed `Event` implementation. A missing stream returns an empty vector.
- Unknown event type/payload version, malformed payload, invalid stored version, and a version gap are errors. Replay never silently skips persisted data.

## Stable errors

The public error variants are the compatibility surface:

- `EventLogError::WrongExpectedVersion` reports the requested and current stream state; no row is written.
- `EventLogError::InvalidPayloadVersion` reports an invalid caller-supplied payload version; no row is written.
- `ReplayError::UnsupportedEvent` carries the stored `event_type` and `payload_version`.
- `ReplayError::MalformedPayload` carries the stream version and decoder diagnostic.
- `ReplayError::VersionGap` carries the expected and observed versions.
- `ReplayError::InvalidStoredVersion` rejects a version that cannot be represented by the API.
- Storage failures remain explicit errors rather than being treated as caller, concurrency, or compatibility failures. Append-side `Storage` and `Encode` variants expose their wrapped SQLite or JSON errors through `std::error::Error::source`; replay-side `Storage` exposes its wrapped SQLite error the same way. Caller, concurrency, range, and replay compatibility failures have no underlying source.

`Event::decode` returns only `DecodeError::UnsupportedEvent` or
`DecodeError::MalformedPayload`; the log maps those into replay errors and adds
the authoritative persisted stream version. Decoders cannot fabricate storage,
ordering, or stream-position failures. `DecodeError` implements Rust's standard
`Display` and `Error` traits so downstream decoders can expose and compose these
failures without an adapter.

The persisted row contains only `stream_id`, `stream_version`, `event_type`, `payload_version`, and JSON `payload`. No timestamp, event ID, actor, correlation metadata, snapshot, batch append, async runtime, or distributed guarantee is part of this slice.

See [ADR-0002](adr/0002-typed-sqlite-event-log.md) for the durability boundary and rationale.
