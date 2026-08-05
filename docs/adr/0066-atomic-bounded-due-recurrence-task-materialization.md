# ADR-0066: Atomic bounded due recurrence task materialization

- **Status:** accepted
- **Date:** 2026-08-05
- **Decision and execution issue:** [#925](https://github.com/Knosence9/project-vela/issues/925)

## Context

ADR-0058 atomically persists one caller-selected bounded due page. ADR-0064 establishes that due selection, canonical provenance, and an inert task binding can form one consequential operation. Repeated exact materialization after page persistence leaves a crash boundary that can produce a task-bound prefix.

## Decision

`RecurrenceStore::materialize_due_occurrences_page(id, expected_revision, start_offset, page_size, cutoff, task_ids)` strictly replays one exact immutable recurrence, validates its caller-observed revision, and reuses ADR-0056's bounded inclusive-cutoff projection and cursor semantics.

The caller supplies exactly one distinct task ID for every selected occurrence, in authored-offset order. A mismatch returns typed selected and supplied counts. A duplicate returns typed identity evidence. A future empty page requires zero IDs, writes nothing, and preserves its resumable cursor.

Every selected occurrence stream and task stream must be absent. One immediate transaction rechecks the recurrence revision and all absences, then appends canonical version-1 `recurrence.occurrence_persisted`, version-1 `recurrence.occurrence_materialized` at occurrence revision 2, and authoritative-goal `task.started` at task revision 1 for each selected coordinate. Success returns all complete `MaterializedRecurrenceOccurrence` bindings in authored-offset order plus the due-page `next_offset`.

Missing or stale definitions, invalid starts, task-count mismatch, duplicate or colliding task identities, existing or malformed selected provenance, serialization or storage failure, and racing writers commit none of the page. Existing exact `materialize_occurrence` remains the explicit recovery boundary for independently persisted provenance; this operation never consumes or skips it.

The operation reads no ambient clock, discovers no unrelated recurrence, generates no identity, persists no cursor or skip evidence, and grants no claim, lease, dispatch, retry, workflow, provider/tool, permission, or execution authority.

## Alternatives considered

### Persist the page, then materialize each coordinate

Rejected for callers requiring one consequential operation because process failure or contention can leave a task-bound prefix. Both lower-level boundaries remain available when the caller explicitly owns that recovery policy.

### Skip existing provenance or task identities

Rejected because sparse idempotence would hide a recovery and identity-allocation policy. Exact existing evidence instead fails closed with no new page writes.

### Generate task identities

Rejected because identity remains caller authority. The kernel validates and records the ordered binding without becoming a dispatcher.

## Consequences

- One bounded all-due page and all of its inert task bindings commit together.
- Count and duplicate errors are typed before storage mutation.
- Future horizons remain write-free and resumable; final-page and maximum-instant behavior remain inherited from due paging.
- The event log gains one crate-private primitive for ordered event pairs and single events across absent streams under one prerequisite revision.

## Verification

RED→GREEN integration tests cover ordered multi-coordinate materialization, canonical revisions and authoritative task goals, reopen and provenance lookup, future empty pages, typed count and duplicate failures, selected-provenance and task collisions, and racing complete-page writers. The complete repository quality gate must remain green.

## Revisit when

Reconsider before adding a CLI adapter, persisted-only automatic recovery, idempotent skipping, generated identities, durable cursors or skip evidence, mutable recurrence definitions, global due discovery, ambient clocks, claims or leases, dispatch, retries, or execution.