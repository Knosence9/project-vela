# ADR-0080: Bounded next-available recurrence task materialization

- **Status:** accepted
- **Date:** 2026-08-06
- **Decision and execution issue:** [#955](https://github.com/Knosence9/project-vela/issues/955)
- **Related:** ADR-0034, ADR-0050, ADR-0069, ADR-0076, ADR-0078

## Context

ADR-0078 lets a caller reserve the earliest available due coordinate in one exact bounded recurrence window. ADR-0050 separately lets a caller atomically bind one exact available persisted or released coordinate to an inert task. A caller that does not need a recoverable claim gap must still page availability, choose a coordinate, and attempt exact materialization, recreating ordering, cursor, cutoff, strict-replay, and lifecycle-race policy.

One-shot schedules already distinguish recoverable claim-next from direct atomic materialize-next. The smallest equivalent recurrence boundary remains scoped to one caller-selected recurrence and authored-offset window.

## Decision

Add `RecurrenceStore::materialize_next_available_occurrence(id, start_offset, page_size, cutoff, task_id)`. The operation strictly loads one exact recurrence and inspects at most `OccurrencePageSize` authored coordinates using the same availability projection as claim-next. Missing, claimed, and materialized coordinates are skipped. Persisted-only and explicitly released coordinates are eligible at their exact current revisions.

The earliest available coordinate at or before the inclusive caller-owned cutoff is atomically bound to the exact caller-owned task identity. One transaction appends `recurrence.occurrence_materialized` against the selected occurrence revision and authoritative-goal `task.started` against an absent task stream while rechecking the immutable recurrence and complete selected window. Success returns `MaterializeNextRecurrenceOccurrenceSelection` containing the complete materialized binding and the following authored offset, or finite completion.

A future first-available coordinate writes nothing and preserves that coordinate as `next_offset`. A complete window without available evidence writes nothing and advances to the first uninspected authored offset or finite completion. The cursor remains caller-owned projection state and is not persisted.

A competing selected-window lifecycle transition restarts the same bounded selection. Every restart follows persisted progress, and the fourth conflicted append attempt returns typed materialize-next contention exhaustion. A pre-existing task identity returns `TaskAlreadyExists` without changing occurrence state. Every replay, storage, append, read-only, or contention failure creates no partial occurrence/task pair. Selected-window corruption fails closed; unrelated recurrences and out-of-window coordinates cannot block selection.

The operation reads no ambient clock, performs no cross-recurrence inventory, generates no identity, and grants no worker identity, lease, dispatch, workflow, provider/tool, permission, retry-of-work, or execution authority.

## Alternatives considered

### Require callers to compose available paging and exact materialization

Rejected because each caller would need to reproduce deterministic selection, complete-window optimistic concurrency, future-horizon cursors, and corruption handling.

### Claim and then materialize in one operation

Rejected because that would record a transient reservation with no recovery opportunity. Direct atomic materialization and recoverable claim consumption are distinct accepted authorities.

### Materialize a currently claimed coordinate selected by inventory

Rejected because claims carry no worker owner. Consuming an arbitrary claim without its exact caller-observed revision would weaken the existing claim boundary.

### Select across recurrence definitions or generate task identities

Rejected because no bounded global ordering or cursor exists and identity allocation remains caller authority.

## Consequences

- Callers can bind one deterministic available due coordinate without a crash gap between selection and task creation.
- Released coordinates can be consumed at their exact latest lifecycle revision; current claims remain untouched.
- Empty windows, future horizons, and finite completion retain the claim-next cursor contract.
- Task collisions and lifecycle races create no orphan task or partial occurrence transition.
- Cross-recurrence discovery, durable cursors, workers, leases, dispatch, retries, and execution remain deferred.

## Verification

RED→GREEN tests prove sparse earliest selection, released-revision reuse, future cursor preservation, empty and finite window progress, typed missing and bounds failures, task-collision atomicity, read-only rejection, selected-window corruption isolation, unrelated-corruption isolation, concurrent callers binding distinct coordinates, and deterministic exhaustion after four continuous conflicts. The complete repository quality gate must remain green.

## Revisit when

Reconsider before adding a CLI adapter, cross-recurrence selection, durable cursors, generated task identity, worker ownership, leases or expiry, ambient clocks, dispatch, retries, permissions, or execution.
