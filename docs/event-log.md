# Typed event log

The `vela-kernel` crate contains Vela's first persistence primitive: a synchronous SQLite append-only log with typed replay.

## Observable contract

- `StreamId` accepts any non-empty UTF-8 string and treats it as opaque.
- Event types accept any non-empty static string and otherwise remain opaque; append rejects an empty discriminator before encoding or opening a write transaction.
- Event payload versions start at `1`; append rejects version `0` before opening a write transaction.
- A new stream accepts `ExpectedVersion::NoStream`; an existing stream accepts only `ExpectedVersion::Exact(current)`.
- Successful appends receive versions 1, 2, 3, and so on. A stale expectation returns `WrongExpectedVersion` and commits nothing.
- Every append is one SQLite transaction. Opening verifies that SQLite activated WAL journaling, configures `synchronous=FULL`, and fails rather than weakening that boundary. Append success is returned only after commit.
- Replay returns one stream in ascending, contiguous order as values decoded by the caller's typed `Event` implementation. A missing stream returns an empty vector.
- Empty or unknown event types, unknown payload versions, malformed payload, invalid stored stream or payload versions, and a version gap are errors. Replay never silently skips persisted data.

## Stable errors

The public error variants are the compatibility surface. `EventLogError` and `ReplayError` are non-exhaustive so the pre-1.0 kernel can add explicit failures without breaking downstream wildcard matches; callers must include a fallback arm:

- `EventLogError::WrongExpectedVersion` reports the requested and current stream state; no row is written.
- `EventLogError::InvalidEventType` reports an empty caller-supplied discriminator; no row is written.
- `EventLogError::InvalidPayloadVersion` reports an invalid caller-supplied payload version; no row is written.
- `EventLogError::UnsupportedJournalMode` reports the effective SQLite journal mode when opening cannot establish WAL (for example, `memory` for `:memory:`).
- `ReplayError::UnsupportedEvent` carries the authoritative stored `event_type` and `payload_version`, even if a decoder supplies different context in its `DecodeError`.
- `ReplayError::InvalidStoredEventType` rejects an empty stored discriminator before invoking the typed decoder.
- `ReplayError::MalformedPayload` carries the stream version and decoder diagnostic.
- `ReplayError::VersionGap` carries the expected and observed versions.
- `ReplayError::InvalidStoredVersion` rejects a stored stream version below `1` or otherwise outside the public stream-version domain.
- `ReplayError::InvalidStoredPayloadVersion` preserves and rejects a stored payload version below `1` or otherwise outside the public `u32` domain instead of classifying valid SQLite access as a storage failure.
- Storage failures remain explicit errors rather than being treated as caller, concurrency, or compatibility failures. Append-side `Storage` and `Encode` variants expose their wrapped SQLite or JSON errors through `std::error::Error::source`; replay-side `Storage` exposes its wrapped SQLite error the same way. Caller, concurrency, range, and replay compatibility failures have no underlying source.

`Event::decode` returns only `DecodeError::UnsupportedEvent` or
`DecodeError::MalformedPayload`; the log maps those into replay errors and adds
the authoritative persisted stream version. Decoders cannot fabricate storage,
ordering, or stream-position failures. `DecodeError` implements Rust's standard
`Display` and `Error` traits so downstream decoders can expose and compose these
failures without an adapter.

The persisted row contains only `stream_id`, `stream_version`, `event_type`, `payload_version`, and JSON `payload`. No timestamp, event ID, actor, correlation metadata, snapshot, batch append, async runtime, or distributed guarantee is part of this slice.

See [ADR-0002](adr/0002-typed-sqlite-event-log.md) for the durability boundary and rationale.
