# ADR-0002: Persist typed events in a local SQLite log

- **Status:** accepted
- **Date:** 2026-07-13
- **Owners:** Project Vela maintainers
- **Related issue:** [#342](https://github.com/Knosence9/project-vela/issues/342)
- **Related pull request:** [#345](https://github.com/Knosence9/project-vela/pull/345)

## Context

Vela needs a smallest durable kernel boundary for ordered, typed events and deterministic replay. The envelope, concurrency semantics, and successful-append guarantee are persisted compatibility contracts, so issue #342 approved them before implementation.

## Decision

Each row persists only `stream_id`, `stream_version`, `event_type`, `payload_version`, and JSON `payload`. Event families implement a typed Rust `Event` boundary that supplies stable type/version discriminators and decodes replayed bytes. IDs are opaque non-empty UTF-8 strings. Event UUIDs, timestamps, actors, correlation metadata, and snapshots are deferred.

A caller appends one event using `ExpectedVersion::NoStream` or `ExpectedVersion::Exact(n)`. Versions start at 1 and are assigned by the log. A mismatch returns a stable concurrency error and writes nothing.

The first store is a synchronous, single-node local SQLite file using `rusqlite`. `(stream_id, stream_version)` is unique. Every append uses one immediate transaction and reports success only after commit. Connections select WAL mode and `synchronous=FULL`; therefore success carries SQLite and the underlying filesystem's ordinary process-restart and host/power-loss guarantees. It does not cover broken or dishonest storage, copying a live database incorrectly, distributed failure, or multi-event atomic batches.

Replay selects one stream in strictly ascending contiguous order from version 1 and returns typed values. A missing stream is empty. Unknown event type/payload version, malformed JSON, invalid stored versions, and gaps are errors rather than skipped or reinterpreted data. Determinism means the same committed bytes yield the same ordered typed values.

## Alternatives considered

### In-memory log

It would simplify tests but cannot establish the approved restart and durability boundary.

### Async or distributed store

An async runtime, network API, replication, and distributed ordering add no value to this first local slice.

### Rich envelope and batch appends

IDs, timestamps, causation/correlation, actors, snapshots, and atomic batches may become useful, but committing them now would create unsupported contracts.

## Consequences

### Positive

- The first kernel persistence boundary is small, typed, inspectable, and restart-safe.
- Optimistic concurrency rejects stale writers atomically.
- Explicit type and payload versions make incompatible persisted data fail visibly.

### Negative

- Event families must implement decoding dispatch explicitly.
- The API is synchronous and supports only one event per transaction.
- WAL databases must be backed up or copied using SQLite-aware procedures.

## Verification

Behavior tests reopen the database before replay, verify ordered typed values, reject stale expected versions without an extra write, return an empty missing stream, and reject unknown event types and version gaps. The repository quality gate must pass.

## Revisit when

Reconsider when Vela needs atomic event batches, snapshots, richer metadata, migrations/upcasters, asynchronous access, multiple processes with measured contention, replication, or a non-local durability boundary.
