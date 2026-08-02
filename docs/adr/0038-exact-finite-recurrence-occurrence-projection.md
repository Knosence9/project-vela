# ADR-0038: Exact finite recurrence occurrence projection

- **Status:** accepted
- **Date:** 2026-08-02
- **Decision and execution issue:** [#861](https://github.com/Knosence9/project-vela/issues/861)
- **Related:** ADR-0036, ADR-0037

## Context

ADR-0037 persists immutable finite fixed-interval definitions with a representable final occurrence, but deliberately does not expose occurrence generation or lifecycle. A caller still needs a precise way to inspect one authored offset before any responsible range-enumeration, persistence, or materialization contract can be designed.

Exact lookup is smaller than enumeration: the caller owns one offset, work is constant, and no pagination, catch-up window, cursor, or selection policy is implied.

## Decision

`FixedIntervalRecurrence::occurrence_at(offset)` projects one exact caller-owned zero-based offset. Offset zero returns the anchor, and `count - 1` returns the definition's validated final occurrence. An offset equal to or greater than the occurrence count returns typed `RecurrenceOccurrenceLookupError::OutOfRange` evidence preserving the exact recurrence ID, rejected offset, and occurrence count.

A successful `RecurrenceOccurrence` preserves the recurrence's exact ID, validated task goal, requested offset, derived `ScheduleInstant`, and definition revision. In-range projection reuses `ScheduleInstant::checked_advance_by`; representability is an invariant because recurrence construction and strict replay validate the final offset before exposing a definition.

Lookup is deterministic and read-only. It accesses neither storage nor ambient time and performs no unbounded enumeration. The recurrence-ID/offset coordinate is projection provenance only. It is not a persisted occurrence identity, idempotency key, one-shot schedule or task ID, claim, lifecycle revision, permission grant, execution count, or evidence that work is due or should run.

## Alternatives considered

### Return only a `ScheduleInstant`

Rejected because stripping recurrence, goal, offset, and definition-revision provenance would force callers to reconstruct or accidentally misassociate the derived instant.

### Return `None` outside the finite range

Rejected because typed evidence makes the exact authored bound and rejected caller input inspectable rather than conflating invalid input with missing recurrence state.

### Enumerate a range now

Rejected because enumeration introduces a limit, cursor, ordering, allocation, and catch-up-shaped contract. Exact lookup establishes projection semantics without those policy decisions.

### Persist projected occurrences

Rejected because durable occurrences require explicit identity, idempotency, lifecycle, provenance, and materialization decisions. Read-only projection grants none of that authority.

## Consequences

- Callers can inspect any exact valid finite offset in constant time with complete immutable provenance.
- Exact lower and upper bounds are explicit and typed.
- No event schema or recurrence history changes.
- Enumeration, persisted occurrence identity, catch-up, materialization, dispatch, and execution remain deferred.

## Verification

RED→GREEN tests prove offset-zero, interior, final, exact-`u64::MAX`, and out-of-range behavior while preserving exact recurrence provenance. The complete repository quality gate must remain green.

## Revisit when

Reconsider before adding range enumeration or pagination, occurrence event streams, durable occurrence identity, generated schedule or task IDs, materialization, catch-up or missed-run policy, cancellation, claims, dispatch, retries, workers, calendar or time-zone semantics, ambient clocks, or execution.
