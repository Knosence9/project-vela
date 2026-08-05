# ADR-0064: Atomic latest-due recurrence task materialization

- **Status:** accepted
- **Date:** 2026-08-05
- **Decision and execution issue:** [#921](https://github.com/Knosence9/project-vela/issues/921)

## Context

ADR-0062 atomically selects and persists one latest-due recurrence occurrence, while ADR-0050 atomically binds one already-persisted exact occurrence to a caller-owned task. Composing those boundaries leaves a crash and race gap between the durable latest-only choice and inert task creation.

The smallest responsible next boundary preserves the canonical persisted-then-materialized occurrence history while committing it with the task start. It must not turn projection into execution authority or silently define recovery for independently persisted occurrences.

## Decision

`RecurrenceStore::materialize_latest_due_occurrence(id, expected_revision, start_offset, cutoff, task_id)` strictly replays one exact immutable recurrence, validates the caller-observed definition revision, and reuses the private constant-space latest-due projection from ADR-0060. The caller owns the authored start, inclusive cutoff, and exact task identity.

When a coordinate is due, the operation requires both the selected occurrence stream and task stream to be absent. One immediate transaction rechecks the exact recurrence revision and both absences, then appends canonical version-1 `recurrence.occurrence_persisted`, version-1 `recurrence.occurrence_materialized` at occurrence revision 2, and the authoritative goal in `task.started` at task revision 1. Success returns `LatestDueMaterializationSelection` containing the complete materialized occurrence and the same following authored offset or finite-completion cursor as latest-due projection.

When the starting coordinate is future, success returns no materialization and preserves the unchanged starting cursor without writing either stream. Skipped authored coordinates are neither inspected nor persisted. Existing selected provenance is strictly replayed and rejected as `OccurrenceAlreadyPersisted`, including persisted-only and materialized histories; callers retain `materialize_occurrence` as the explicit recovery boundary for persisted-only evidence. Selected corruption fails closed while skipped corruption remains outside the exact boundary.

After a competing write, exact recurrence replay and revision validation take precedence, followed by selected occurrence replay, then task collision classification. Missing or stale definitions, invalid starts, existing or malformed selected provenance, task collisions, serialization or storage failures, and races leave no partial occurrence or task history.

The operation reads no ambient clock, persists no cursor or skipped-run evidence, discovers no unrelated recurrence, generates no identity, and grants no claim, lease, dispatch, retry, workflow, provider/tool, permission, or execution authority.

## Alternatives considered

### Persist latest due, then materialize exact occurrence

Rejected for callers that require one consequential operation because a process failure can leave selected provenance without its intended task and a competing actor can consume either stream between calls. Both explicit lower-level boundaries remain available for recovery and independently owned policy.

### Materialize an unpersisted projection without occurrence provenance

Rejected because task provenance would no longer replay through the canonical persisted-then-materialized occurrence lifecycle.

### Consume an already-persisted selected occurrence

Rejected because that folds recovery policy into latest-only selection and makes a prior independently authored stream ambiguous. Exact `materialize_occurrence` already expresses that transition with an observed occurrence revision.

### Generate a task identity or read the clock

Rejected because identity and time remain caller authorities. The kernel records deterministic intent; it does not become a worker or dispatcher.

## Consequences

- A caller can commit one explicit latest-only catch-up choice and its inert task binding without partial provenance or orphan task state.
- Read-only selection, persistence-only selection, exact persisted-occurrence materialization, and combined latest-only materialization retain separate authority boundaries.
- Future horizons remain write-free and resumable; finite completion remains explicit.
- Skipped coordinates and returned cursors remain non-durable policy coordinates, not acceptance, waiver, or execution evidence.
- The event log gains one crate-private primitive for two ordered events on one absent stream plus one event on another absent stream under one prerequisite revision.

## Verification

RED→GREEN integration tests cover between-instant selection, canonical persisted-to-materialized replay, authoritative task goal, skipped-coordinate absence, future write-free resumption, finite completion at `u64::MAX`, missing and stale definitions, invalid starts, task collisions, duplicate selected provenance, and racing writers that commit exactly one complete occurrence/task binding. The complete repository quality gate must remain green.

## Revisit when

Reconsider before adding a CLI adapter, recovery of persisted-only latest selections, durable catch-up cursors or skip evidence, idempotent materialization, mutable recurrence definitions, global due discovery, ambient clocks, generated identities, claims or leases, dispatch, retries, or execution.
