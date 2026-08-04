# ADR-0058: Atomic bounded due recurrence provenance persistence

- **Status:** accepted
- **Date:** 2026-08-04
- **Decision and execution issue:** [#907](https://github.com/Knosence9/project-vela/issues/907)

## Context

ADR-0056 and ADR-0057 expose allocation-bounded due projection for one exact finite recurrence through an inclusive caller-owned cutoff. Persisting the selected coordinates one call at a time would make the page-level catch-up unit implicit and allow a process failure or competing writer to leave only a prefix of the caller-selected page durable.

The smallest responsible mutation remains scoped to one exact recurrence, one observed immutable definition revision, and one caller-selected authored window. Ambient clocks, global recurrence discovery, generated identities, materialization, workers, and automatic catch-up policy remain separate authorities.

## Decision

`RecurrenceStore::persist_due_occurrences_page(id, expected_revision, start_offset, page_size, cutoff)` strictly loads one exact recurrence definition and validates the caller-observed definition revision before selecting or writing occurrences. Selection reuses ADR-0056's inclusive cutoff and bounded cursor semantics: complete occurrences are ordered by authored offset, the page contains at most `OccurrencePageSize`, and `next_offset` identifies the first uninspected or future coordinate or finite completion.

Every selected coordinate must have no existing occurrence stream. Existing selected provenance is validated and rejected as typed `OccurrenceAlreadyPersisted` evidence rather than skipped. The event log validates every selected stream and the recurrence prerequisite inside one immediate transaction, then appends one canonical version-1 `recurrence.occurrence_persisted` event to every selected stream. Success therefore persists the complete selected page; serialization, storage, stale prerequisite, duplicate-coordinate, corruption, or competing-write failure persists none of it.

An empty page before the caller cutoff horizon succeeds without writes and preserves its unchanged resumable cursor. Exact projection preserves recurrence identity, goal, authored offset, Unix-millisecond instant, and definition revision, including `u64::MAX` boundaries.

The operation reads no ambient clock, persists no cursor, generates no identity, discovers no unrelated recurrence, and grants no global catch-up, materialization, task lifecycle, claim, lease, dispatch, retry, workflow, provider/tool, permission, or execution authority.

## Alternatives considered

### Call `persist_occurrence` for every due result

Rejected as the kernel contract because a crash or later conflict can durably record only a prefix while the caller reasonably treats the bounded page as one catch-up unit.

### Skip coordinates that already have provenance

Rejected because silently sparse mutation hides competing ownership and makes the returned page ambiguous. Callers can inspect persisted pages and deliberately resume from an unpersisted coordinate.

### Persist a catch-up cursor with the page

Rejected because projection coordinates are not lifecycle state. Cursor durability, retry ownership, and missed-run policy require a separate contract.

### Select across every recurrence

Rejected because global ordering, corruption scope, and catch-up policy exceed this exact-recurrence boundary.

## Consequences

- Callers can durably record one bounded exact-recurrence due unit without partial-page provenance.
- Definition revision and selected-stream absence are checked again in the atomic transaction.
- Existing selected coordinates fail explicitly rather than becoming implicit idempotency.
- Callers still own cutoff choice, cursor retention, retries, identity generation, later materialization, and execution.

## Verification

RED→GREEN integration tests cover bounded inclusive multi-coordinate persistence, strict read-only replay after reopen, atomic duplicate rejection, two-writer complete-page competition, stale revision and out-of-range preflight failures, selected corruption, an empty future-horizon page, final-page cursor semantics, and exact `u64::MAX` occurrences. The complete repository quality gate must remain green.

## Revisit when

Reconsider before adding a CLI adapter, idempotent sparse catch-up, global due discovery, durable catch-up cursors, generated task identities, recurrence cancellation, claims or leases, ambient clocks, dispatch, retries, or execution.
