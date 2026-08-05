# ADR-0062: Atomic latest-due recurrence provenance persistence

- **Status:** accepted
- **Date:** 2026-08-04
- **Decision and execution issue:** [#917](https://github.com/Knosence9/project-vela/issues/917)

## Context

ADR-0060 makes latest-only catch-up an explicit constant-space selection policy for one exact finite recurrence. Persisting its result through a later exact-offset call would separate the consequential policy choice from the durable write and would not transactionally bind the write to the caller-observed recurrence definition.

The smallest responsible mutation is atomic persistence of only the selected latest-due coordinate. Skipped coordinates and the returned cursor must remain outside durable lifecycle semantics: persisting one selection does not prove that the caller accepted, waived, or acted on earlier work.

## Decision

`RecurrenceStore::persist_latest_due_occurrence(id, expected_revision, start_offset, cutoff)` strictly replays one exact immutable recurrence, validates the caller-observed definition revision, and reuses the same private constant-space projection as `latest_due_occurrence`. Read-only and writable latest-only policy therefore cannot drift.

When a coordinate is due, the operation strictly verifies that only the selected occurrence stream is absent, then appends one canonical version-1 `recurrence.occurrence_persisted` event while transactionally rechecking both selected-stream absence and the exact recurrence revision. Success returns `LatestDueOccurrenceSelection` with the complete persisted coordinate and the same following authored offset or finite-completion cursor as read-only selection.

When the starting coordinate is future, success returns no occurrence and preserves the unchanged starting cursor without writing. Skipped authored coordinates are neither inspected nor persisted. Existing selected provenance is strictly replayed and rejected as `OccurrenceAlreadyPersisted`; malformed selected evidence fails closed. After a competing write, exact recurrence replay takes precedence over selected-stream replay so missing or stale definition evidence remains typed, then a valid selected coordinate is reported as already persisted.

The operation reads no ambient clock, persists no cursor or skipped-coordinate evidence, scans no unrelated recurrence or skipped occurrence, generates no identity, and grants no materialization, task lifecycle, dispatch, retry, workflow, provider/tool, permission, or execution authority.

## Alternatives considered

### Select, then call exact occurrence persistence

Rejected because policy projection and persistence would be separate operations, and the existing exact-offset append does not transactionally guard the recurrence prerequisite.

### Persist every skipped coordinate

Rejected because it changes latest-only policy into all-due persistence, scales with backlog size, and fabricates durable provenance for coordinates the caller deliberately did not select.

### Record a durable catch-up cursor or skip marker

Rejected because occurrence provenance is not acceptance, waiver, or processing evidence. A durable cursor or skip lifecycle needs its own revision-bound semantics and recovery contract.

### Treat an existing selected coordinate as idempotent success

Rejected because an existing stream is independently authored durable evidence. Strict duplicate rejection preserves conflict visibility and matches exact and bounded-page recurrence persistence.

## Consequences

- Callers can atomically persist one explicit latest-only catch-up choice with constant-space selection work.
- The recurrence definition and selected occurrence absence are rechecked in one immediate transaction.
- Future horizons remain write-free and resumable; finite completion remains explicit.
- Corruption in skipped coordinates cannot block the exact selected mutation, while selected corruption fails closed.
- Persisted selection proves only the selected coordinate; skipped work and the returned cursor remain non-durable policy coordinates.

## Verification

RED→GREEN integration tests cover between-instant latest selection, skipped-coordinate absence after reopen, future no-op behavior, finite completion at `u64::MAX`, missing and stale definitions, invalid starts, duplicate rejection, selected corruption failure, skipped-corruption isolation, and racing writers that commit exactly one selected coordinate. The complete repository quality gate must remain green.

## Revisit when

Reconsider before adding a CLI adapter, durable catch-up cursors or skip evidence, idempotent persistence, mutable or cancelled recurrence definitions, global due discovery, ambient clocks, generated task identities, claims or leases, materialization policy, dispatch, retries, or execution.
