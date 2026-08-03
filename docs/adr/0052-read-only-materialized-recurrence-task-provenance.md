# ADR-0052: Read-only materialized recurrence provenance by exact task identity

- **Status:** accepted
- **Date:** 2026-08-03
- **Decision and execution issue:** [#895](https://github.com/Knosence9/project-vela/issues/895)
- **Related:** ADR-0050, ADR-0051

## Context

ADR-0050 atomically binds one exact persisted recurrence occurrence to one caller-owned task. Exact recurrence ID and offset lookup can inspect that binding, but a caller starting from a task identity cannot recover its recurrence provenance. One-shot schedules already expose an exact reverse lookup that treats duplicate durable task bindings as corruption rather than choosing an arbitrary owner.

A global occurrence inventory would add unrelated discovery authority and scaling policy. An exact task query is the smaller responsible boundary.

## Decision

`RecurrenceStore::find_materialized_by_task_id(task_id)` is a read-only historical query. In one ordered event-log query it selects occurrence streams whose version-1 `recurrence.occurrence_materialized` marker contains the exact JSON task identity. Invalid JSON in an unrelated marker cannot block the selected task query. Every selected stream is then strictly decoded in full, its canonical byte-length-prefixed recurrence ID and offset are recovered without delimiter ambiguity, and its persisted provenance is validated against the exact immutable recurrence definition.

No matching stream returns `None`. One valid binding returns the complete existing `MaterializedRecurrenceOccurrence`, including exact occurrence provenance, occurrence revision, and task ID. More than one valid selected stream returns `RecurrenceStoreError::AmbiguousTaskBinding` with the exact task ID and occurrence count; no arbitrary binding is returned. A malformed selected stream identity, event, payload, lifecycle shape, missing definition, or divergent definition provenance fails closed.

The query reads no clock, mutates no state, scans no unmaterialized occurrence stream, generates no identity, and grants no catch-up, due-selection, lifecycle, permission, dispatch, retry, provider/tool, workflow, or execution authority.

## Alternatives considered

### Inventory every materialized occurrence and filter in memory

Rejected because it would decode unrelated occurrence histories, let unrelated corruption block an exact query, and introduce global inventory authority that callers did not request.

### Return the first matching binding

Rejected because durable corruption could make ordering determine provenance. Duplicate task bindings are explicit ambiguity evidence.

### Trust only the materialization marker

Rejected because the marker does not independently prove the owning coordinate, persisted goal, instant, or authoritative recurrence definition. Complete selected-stream replay and definition validation remain mandatory.

## Consequences

- Callers can recover exact recurrence provenance from one task identity without mutation.
- Canonical stream parsing makes separator-containing and multibyte recurrence IDs reversible and rejects non-canonical aliases.
- Unrelated malformed materialization JSON is isolated, while every selected candidate remains fail-closed.
- The event-log gains a reusable exact JSON-text marker selector that returns complete ordered stream histories.
- Global occurrence inventory and CLI exposure remain deferred.

## Verification

RED→GREEN tests prove successful lookup after reopen, unrelated-task absence, duplicate-binding ambiguity, selected provenance corruption failure, and unrelated malformed marker isolation. Existing recurrence replay and materialization tests remain green, and the complete repository quality gate must pass.

## Revisit when

Reconsider before adding global materialized-occurrence inventory, task-binding repair, mutable recurrence definitions, catch-up cursors, cancellation, generated task identities, or dispatch authority.
