# ADR-0089: Read-only finite recurrence status filtering

- **Status:** accepted
- **Date:** 2026-08-07
- **Decision and execution issue:** [#977](https://github.com/Knosence9/project-vela/issues/977)
- **Related:** ADR-0040, ADR-0082, ADR-0084

## Context

Finite recurrence cancellation is durable and the existing recurrence inventory returns complete active and cancelled definitions. Kernel callers that need one lifecycle class must otherwise load that complete inventory and duplicate filtering against the canonical status projection.

The status is already exact persisted lifecycle evidence. Reusing it as a caller-owned read-only filter avoids a parallel projection or storage query while preserving the inventory's fail-closed discovery boundary.

## Decision

Add `RecurrenceStore::list_by_status(status)`. The operation reuses the same canonical discovery and complete lifecycle projection as `RecurrenceStore::list`, retains only definitions whose exact `RecurrenceStatus` equals the caller-owned filter, and returns them ordered by exact recurrence ID.

Empty and unmatched stores return an empty vector. Every discovered recurrence history is validated before filtering can return, so malformed owning stream IDs, payloads, events, versions, or lifecycle ordering fail closed without a partial result. Unrelated non-recurrence streams remain excluded.

The operation works through read-only storage, reads no clock, mutates nothing, and grants no cancellation, occurrence lifecycle, worker, dispatch, retry, permission, or execution authority.

## Alternatives considered

### Filter after calling `list`

Rejected as the public contract because every caller would duplicate the same status predicate and its intended authority boundary.

### Query cancellation events directly

Rejected because event presence is not a safe substitute for complete canonical lifecycle projection and could expose malformed histories as valid state.

### Add the CLI adapter in the same slice

Rejected because the reusable kernel contract and its fail-closed behavior should be independently verified before fixing a command, argument, JSON, and diagnostic contract.

## Consequences

- Kernel callers can request exactly active or cancelled recurrence definitions without duplicating lifecycle interpretation.
- Full inventory and status-filtered inventory share one discovery and projection path.
- Corruption outside the requested status still fails closed rather than being hidden by filtering.
- No schema, event, lifecycle transition, clock, cursor, or execution authority is added.

## Verification

Strict RED→GREEN tests prove mixed active and cancelled filtering, exact ID ordering, empty and unmatched results, read-only reopen, unrelated-stream exclusion, and fail-closed malformed recurrence handling. The complete repository quality gate must remain green.

## Revisit when

Reconsider before adding a CLI adapter, bounded or global pagination, multi-status predicates, destructive deletion, resume or undo semantics, clocks, workers, leases, dispatch, permissions, retries, or execution.
