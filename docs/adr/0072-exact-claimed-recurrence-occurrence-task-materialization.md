# ADR-0072: Exact claimed recurrence occurrence task materialization

- **Status:** accepted
- **Date:** 2026-08-05
- **Decision and execution issue:** [#939](https://github.com/Knosence9/project-vela/issues/939)
- **Related:** ADR-0034, ADR-0050, ADR-0068, ADR-0069

## Context

ADR-0068 and ADR-0069 provide exact-revision reservation and explicit recovery for one persisted recurrence occurrence. A successful claim remains inert, however: callers cannot consume that reservation into durable task state without first releasing it, which discards the useful distinction between claimed consumption and available-state materialization.

One-shot schedules already establish the responsible boundary: consuming a claim must atomically append both the selected lifecycle transition and the caller-identified task start. Claim-next selection, generated identity, workers, leases, dispatch, and execution remain separate authorities.

## Decision

Add `RecurrenceStore::materialize_claimed_occurrence(id, offset, expected_occurrence_revision, task_id)`. The operation strictly replays one exact persisted occurrence, validates the caller-observed revision before lifecycle state, and requires that coordinate to be currently claimed and not already materialized.

Success atomically appends version-1 `recurrence.occurrence_materialized` at `ExpectedVersion::Exact(expected_occurrence_revision)` and the existing version-1 `task.started` event at `ExpectedVersion::NoStream`. The task receives the occurrence's authoritative goal. The returned `MaterializedRecurrenceOccurrence` preserves complete occurrence provenance, the resulting occurrence revision, and the exact caller-owned task ID.

Strict replay accepts `persisted -> (claimed -> released)*` followed by `claimed -> materialized` as a terminal history, while retaining direct available-state materialization. `materialize_occurrence` remains the distinct persisted-or-released boundary and continues to reject current claims.

Missing provenance, stale revisions, available or already-materialized lifecycle state, task collisions, read-only storage, malformed evidence, and contention append no occurrence transition and create no orphan task. A competing release or claimed materialization against one observed revision commits at most one complete transition; the loser receives typed concurrent-modification evidence. A cutoff is intentionally absent because the preceding claim already recorded the caller-owned due decision.

The operation scans no unrelated coordinate, reads no clock, generates no identity, and grants no inventory, claim-next, worker, lease, dispatch, retry, permission, workflow, provider/tool, or execution authority.

## Alternatives considered

### Broaden `materialize_occurrence`

Rejected because silently accepting both available and claimed state would blur two distinct authority boundaries and change the existing CLI contract.

### Release before every materialization

Rejected because recovery and successful claim consumption are different consequential transitions. Requiring release would record misleading recovery evidence and expose an avoidable competing-write window.

### Add task identity to the claim

Rejected because a reservation does not need to bind the eventual task identity, and changing persisted claim evidence would conflate selection with task creation.

## Consequences

- Callers can consume one exact claim into inert task state without fabricating recovery.
- Occurrence and task evidence commit atomically, including under task collisions and competing releases.
- Existing available-state materialization remains explicit and backward compatible.
- Worker identity, liveness, leases, dispatch, retry, permission, and execution policy remain caller-owned.

## Verification

Strict RED→GREEN integration tests cover successful authoritative-goal materialization, resulting revision, read-only reopen, claimed-lookup disappearance, materialized paging and reverse task provenance, missing, stale, persisted, released, and terminal state failures, revision-before-lifecycle precedence, task collisions, read-only rejection, and a claimed-materialization/release race with no orphan task. The complete repository quality gate must remain green.

## Revisit when

Reconsider before adding CLI exposure, claimed inventory, claim-next selection, generated task identity, worker identity, leases or expiry, ambient clocks, dispatch, retries, permissions, or execution.
