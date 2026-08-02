# ADR-0040: Read-only finite recurrence inventory

- **Status:** accepted
- **Date:** 2026-08-02
- **Decision and execution issue:** [#865](https://github.com/Knosence9/project-vela/issues/865)
- **Related:** ADR-0037, ADR-0038, ADR-0039

## Context

ADR-0037 persists immutable finite fixed-interval recurrence definitions, while ADR-0038 and ADR-0039 project their inert occurrences. Callers can load one known recurrence, but cannot discover the definitions already present without owning every ID. Exposing a CLI or persisted occurrence lifecycle before a complete read boundary would combine unrelated authority and policy decisions.

The next responsible slice is deterministic, fail-closed recurrence inventory through the existing event-log snapshot boundary. Read-only opening must also make filesystem mutation authority explicit.

## Decision

`RecurrenceStore::open_read_only(path)` delegates to `EventLog::open_read_only`. It opens existing compatible recurrence evidence without creating a database or granting SQLite write authority. Mutation methods remain callable for API compatibility but fail at the read-only storage boundary and append no event.

`RecurrenceStore::list()` discovers streams from authoritative `recurrence.fixed_interval_created` events in one SQLite read snapshot. It validates the owning `recurrence:` stream ID and projects each complete history through the same logic as exact `load`. Results preserve every exact definition field and revision and are ordered by exact `RecurrenceId`; an empty store returns an empty vector. Unrelated event streams are excluded.

Discovery is fail-closed. Invalid owning stream IDs, malformed payloads, unsupported events, and histories other than one valid creation event return typed errors before any partial inventory is returned.

Inventory is deterministic and inert. It does not read ambient time, persist or enumerate occurrences, establish cursor or catch-up state, materialize schedules or tasks, grant permission, dispatch, or execute work.

## Alternatives considered

### Add the recurrence CLI first

Rejected because CLI JSON and diagnostics should build on a tested kernel read boundary rather than duplicate event-log discovery and validation.

### Discover streams by prefix alone

Rejected because authoritative creation-event discovery matches existing durable inventory boundaries and excludes unrelated streams without treating arbitrary prefixed data as a recurrence definition.

### Skip malformed definitions

Rejected because returning a plausible partial inventory would hide durable corruption and could cause callers to make decisions from incomplete state.

## Consequences

- Existing recurrence definitions can be inspected without prior ID knowledge.
- Exact ordering and one-snapshot validation make inventory deterministic.
- Read-only opening cannot create the selected database or append recurrence events.
- No event schema or recurrence lifecycle changes.
- Writable and read-only recurrence CLI, persisted occurrence identity and lifecycle, generated schedule/task IDs, catch-up, claims, cancellation, retries, workers, calendar/time-zone semantics, and execution remain deferred.

## Verification

RED→GREEN tests prove complete exact-ID ordering, unrelated-stream exclusion, empty inventory, missing-path no-creation behavior, failed read-only mutation, malformed owning stream rejection, and invalid discovered-history rejection. The complete repository quality gate must remain green.

## Revisit when

Reconsider before adding alternate discovery keys, pagination over definitions, partial-success semantics, a recurrence CLI, mutable recurrence histories, persisted occurrences, catch-up policy, generated identities, schedule/task materialization, claims, cancellation, dispatch, retries, workers, calendars, time zones, ambient clocks, or execution.
