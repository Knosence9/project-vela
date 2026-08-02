# ADR-0043: Read-only exact finite recurrence CLI lookup

- **Status:** accepted
- **Date:** 2026-08-02
- **Decision and execution issue:** [#871](https://github.com/Knosence9/project-vela/issues/871)
- **Related:** ADR-0037, ADR-0041, ADR-0042

## Context

ADR-0037 establishes immutable durable finite fixed-interval definitions and an exact `RecurrenceStore::load` projection. ADR-0041 exposes a complete deterministic inventory, while ADR-0042 permits explicit creation. Operators still need to inspect one caller-selected definition without scanning unrelated recurrence streams or acquiring write authority.

The smallest responsible adapter validates one exact identity and delegates read-only replay to the existing kernel lookup.

## Decision

`vela-dev recurrence get DATABASE RECURRENCE_ID` validates the caller-supplied ID through `RecurrenceId` before storage access, opens only the exact existing caller-selected database through `RecurrenceStore::open_read_only`, and calls `RecurrenceStore::load` for the exact recurrence stream.

Success emits the same compact complete recurrence object used by creation and inventory: `id`, `goal`, `anchor_unix_millis`, `interval_millis`, `occurrence_count`, `final_occurrence_unix_millis`, and `revision`. Exact strings are JSON escaped without trimming or normalization.

An invalid ID emits `invalid_recurrence_id` before storage access. A compatible store without the selected recurrence emits `recurrence_not_found`. Open, schema, replay, projection, and serialization failures emit `recurrence_lookup_failed`. Every failure returns non-zero, emits one escaped diagnostic, and emits no stdout. A missing database is never created.

The command is read-only and inert. It does not enumerate unrelated streams, read ambient time, generate identities, mutate recurrence state, persist or project occurrences, choose catch-up policy, materialize, claim, cancel, dispatch, retry, grant permission, or execute work.

## Alternatives considered

### Filter the complete recurrence inventory

Rejected because the kernel already provides exact-stream replay; scanning unrelated streams performs unnecessary work and lets corruption outside the caller-selected stream block lookup.

### Return `null` for a missing identity

Rejected because this command promises one exact recurrence rather than an optional inventory result. A categorized failure prevents absence from being mistaken for successful evidence.

### Open the database before validating the identity

Rejected because invalid caller input must not touch or create storage.

### Combine lookup with occurrence projection

Rejected because occurrence coordinates and output bounds are separate caller-owned inputs and authority.

## Consequences

- Operators can retrieve one exact recurrence through a deterministic machine-readable adapter.
- Invalid identities and missing paths have no filesystem side effects.
- Corruption in unrelated recurrence streams cannot block exact lookup.
- CLI occurrence paging, occurrence persistence and lifecycle, generated identities, catch-up, cancellation, materialization, claims, retries, workers, calendar/time-zone semantics, and execution remain deferred.

## Verification

RED→GREEN CLI integration tests prove exact successful JSON, pre-open ID validation, missing database without creation, explicit absent-recurrence failure, and fail-closed exact-stream corruption. The complete repository quality gate must remain green.

## Revisit when

Reconsider before adding occurrence paging, mutable recurrence histories, persisted occurrences, catch-up policy, generated identities, schedule/task materialization, claims, cancellation, retries, workers, calendars, time zones, ambient clocks, or execution.
