# ADR-0053: Read-only recurrence task-provenance CLI

- **Status:** accepted
- **Date:** 2026-08-03
- **Decision and execution issue:** [#897](https://github.com/Knosence9/project-vela/issues/897)
- **Related:** ADR-0051, ADR-0052

## Context

ADR-0052 establishes strict read-only reverse lookup from one exact task identity to its materialized recurrence occurrence. Callers can use that boundary only by embedding the kernel, while the developer CLI already exposes the analogous one-shot schedule provenance query.

A global materialized-occurrence inventory would introduce unrelated discovery and scaling policy. A thin exact-task CLI adapter is the smallest responsible automation boundary.

## Decision

`vela-dev recurrence task DATABASE TASK_ID` validates `TASK_ID` through `TaskId` before storage access, opens only the caller-selected existing database through `RecurrenceStore::open_read_only`, and delegates directly to `RecurrenceStore::find_materialized_by_task_id`.

Success emits one compact JSON object containing the exact `task_id` and `occurrence`. A bound occurrence uses the existing complete materialized projection: exact recurrence ID, goal, offset, Unix-millisecond instant, definition revision, occurrence revision, and task ID. A valid unbound identity emits `"occurrence":null`. Serde JSON escaping protects every caller-authored field.

Invalid identities emit `invalid_task_id` before storage access. Open, replay, ambiguity, provenance, and serialization failures emit `recurrence_task_lookup_failed`, return non-zero, and emit no stdout. A missing database remains missing.

The adapter reads no clock, mutates no state, enumerates no unrelated occurrence, and grants no catch-up, due-selection, dispatch, workflow, provider/tool, permission, retry, or execution authority.

## Alternatives considered

### Add task filtering to a global occurrence inventory

Rejected because exact provenance does not require global discovery, ordering, pagination, or allocation policy.

### Reimplement marker lookup in the CLI

Rejected because it would duplicate canonical stream parsing, strict replay, ambiguity detection, and provenance validation already owned by the kernel.

### Return a not-found diagnostic for an unbound task

Rejected because an exact valid identity can legitimately be unrelated to recurrence materialization. Explicit JSON `null` matches the existing schedule task-provenance contract and remains automation-friendly.

## Consequences

- CLI callers can recover complete recurrence provenance from one task identity through a read-only boundary.
- Exact absence remains distinct from invalid identity, missing storage, corruption, and ambiguity.
- Output reuses the materialization shape instead of introducing a second occurrence representation.
- Global inventory, repair, lifecycle expansion, catch-up, dispatch, and execution remain deferred.

## Verification

RED→GREEN tests prove escaped exact output after reopen, unbound `null`, identity validation before storage access, missing-storage preservation, and fail-closed ambiguous binding behavior with empty stdout. The complete repository quality gate must pass.

## Revisit when

Reconsider before adding global materialized-occurrence inventory, task-binding repair, mutable recurrence definitions, catch-up cursors, generated task identities, lifecycle expansion, dispatch, or execution authority.
