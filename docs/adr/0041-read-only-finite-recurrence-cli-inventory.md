# ADR-0041: Read-only finite recurrence CLI inventory

- **Status:** accepted
- **Date:** 2026-08-02
- **Decision and execution issue:** [#867](https://github.com/Knosence9/project-vela/issues/867)
- **Related:** ADR-0037, ADR-0038, ADR-0039, ADR-0040

## Context

ADR-0040 establishes deterministic, fail-closed recurrence inventory through a read-only storage boundary. Operators still have no supported way to inspect those durable definitions outside the kernel API. Adding writable recurrence creation or occurrence paging at the same time would combine inspection with separate authority and output-contract decisions.

The smallest responsible adapter is a read-only CLI inventory that preserves the complete kernel projection without adding lifecycle or execution semantics.

## Decision

`vela-dev recurrence inspect DATABASE` opens only through `RecurrenceStore::open_read_only` and delegates discovery and validation to `RecurrenceStore::list`.

Success emits one compact JSON object with a `recurrences` array in the kernel's exact recurrence-ID order. Every element preserves `id`, `goal`, `anchor_unix_millis`, `interval_millis`, `occurrence_count`, `final_occurrence_unix_millis`, and `revision`. An empty compatible store emits `{"recurrences":[]}`.

Open, schema, replay, projection, and serialization failures emit one escaped `recurrence_inspection_failed` diagnostic, return non-zero status, and emit no partial stdout. A missing database remains missing because inspection grants neither creation nor write authority.

The command is inert. It accepts no ambient time, occurrence offset or page, cutoff, lifecycle mutation, generated identity, catch-up policy, permission, dispatch, or execution input.

## Alternatives considered

### Add writable recurrence creation

Rejected because mutation requires a separate validation and operator-authority contract. Inspection needs no write access and should not implicitly initialize storage.

### Include projected occurrences

Rejected because definitions can contain many occurrences and ADR-0039 requires an explicit allocation-bounded page contract. Definition inventory should not hide pagination or catch-up policy.

### Reimplement discovery in the CLI

Rejected because ADR-0040 already provides authoritative one-snapshot discovery and fail-closed projection. Duplicating it could produce partial or inconsistently ordered output.

## Consequences

- Operators can inspect complete finite recurrence definitions as deterministic machine-readable JSON.
- Exact caller-authored strings are JSON escaped, and malformed durable state never yields a plausible partial inventory.
- Missing-path inspection cannot create storage.
- Exact-ID lookup, writable recurrence creation, CLI occurrence paging, persisted occurrence lifecycle, catch-up, generated schedule/task identities, claims, retries, workers, calendar/time-zone semantics, and execution remain deferred.

## Verification

RED→GREEN CLI integration tests prove complete exact-ID-ordered JSON, empty inventory, missing-path no-creation behavior, and fail-closed corrupted inventory diagnostics. The complete repository quality gate must remain green.

## Revisit when

Reconsider before adding exact recurrence lookup, writable recurrence commands, occurrence paging, lifecycle state, catch-up policy, generated identities, materialization, claims, cancellation, retries, workers, calendars, time zones, ambient clocks, or execution.
