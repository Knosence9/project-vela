# ADR-0073: Writable exact claimed recurrence occurrence materialization CLI

- **Status:** accepted
- **Date:** 2026-08-05
- **Decision and execution issue:** [#941](https://github.com/Knosence9/project-vela/issues/941)
- **Related:** ADR-0051, ADR-0069, ADR-0071, ADR-0072

## Context

ADR-0072 provides an atomic kernel boundary that consumes one exact current recurrence occurrence claim into caller-identified inert task state. Operators cannot invoke that boundary through the developer CLI. The existing `recurrence materialize` command intentionally accepts only available persisted or released provenance, so broadening it would erase the authority distinction between direct materialization and successful claim consumption.

## Decision

Add `vela-dev recurrence materialize-claimed DATABASE RECURRENCE_ID OFFSET EXPECTED_OCCURRENCE_REVISION TASK_ID`. The command validates both exact caller-owned identities before storage access, opens only the caller-selected database through `RecurrenceStore::open`, and delegates strict replay, revision-before-lifecycle validation, claimed-state enforcement, task uniqueness, and atomic persistence to `RecurrenceStore::materialize_claimed_occurrence`.

Success emits the existing compact `MaterializedRecurrenceOccurrence` projection preserving exact recurrence ID, goal, offset, instant, definition revision, resulting occurrence revision, and task ID. Invalid identities emit `invalid_recurrence_id` or `invalid_task_id` before storage creation. Storage, replay, missing, stale, available, released, materialized, task-collision, contention, append, read-only, and serialization failures emit `recurrence_claimed_occurrence_materialization_failed`, return non-zero, and emit no stdout.

The command remains distinct from `recurrence materialize`. It adds no cutoff because the preceding claim recorded caller-owned due authority. It scans no unrelated coordinate, reads no clock, generates no identity, and grants no inventory, claim-next, worker, lease, dispatch, retry, permission, provider/tool, workflow, or execution authority.

## Alternatives considered

### Let `recurrence materialize` accept claims

Rejected because available-state and claimed-state consumption are distinct consequential authorities with different lifecycle preconditions.

### Require release before CLI materialization

Rejected because recording false recovery evidence discards the durable meaning of successful claim consumption and adds a competing-write window.

### Add a cutoff to the command

Rejected because due validation already occurred when the exact revision was claimed. Revalidating against another horizon would introduce conflicting time authority.

## Consequences

- Operators can consume one exact observed claim into inert task state through deterministic JSON.
- Invalid identities cannot create the selected database, and failed atomic operations create no orphan task.
- Existing direct available-state materialization remains behaviorally unchanged.
- Selection, generated identity, workers, liveness, dispatch, retry, permission, and execution remain caller-owned.

## Verification

Strict RED→GREEN CLI integration tests cover deterministic success output, durable read-only reopen, claimed lookup disappearance, reverse task provenance, pre-storage identity validation, and fail-closed missing, available, released, stale, terminal, and task-collision behavior. The complete repository quality gate must remain green.

## Revisit when

Reconsider before adding claimed inventory, claim-next selection, generated task identity, workers, leases or expiry, ambient clocks, dispatch, retries, permissions, or execution.
