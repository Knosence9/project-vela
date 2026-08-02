# ADR-0039: Bounded finite recurrence occurrence paging

- **Status:** accepted
- **Date:** 2026-08-02
- **Decision and execution issue:** [#863](https://github.com/Knosence9/project-vela/issues/863)
- **Related:** ADR-0036, ADR-0037, ADR-0038

## Context

ADR-0038 establishes exact read-only occurrence projection but deliberately defers enumeration until its limit, cursor, ordering, allocation, and catch-up semantics are explicit. Unbounded collection would let caller-controlled recurrence counts drive memory use, while an implicit empty result for an invalid start would hide bad coordinates.

The next responsible slice is bounded paging over the immutable authored offsets. It must not imply that an occurrence is due, persisted, or authorized to run.

## Decision

`OccurrencePageSize` accepts positive sizes through 1024. Zero and larger values return typed validation evidence before projection or allocation.

`FixedIntervalRecurrence::occurrences_page(start_offset, page_size)` requires an authored zero-based start offset. A start at or beyond the occurrence count returns the existing typed out-of-range evidence. Valid pages contain occurrences in strictly increasing offset order and truncate at the authored finite count. Every item reuses `occurrence_at`, preserving the exact recurrence ID, goal, offset, instant, and definition revision.

`RecurrenceOccurrencePage::next_offset()` returns the first unreturned offset only when authored occurrences remain; the final page returns `None`. End-bound calculation is saturation-safe before it is clamped to the occurrence count, so caller-controlled offsets cannot wrap. The fixed maximum bounds allocation independently of the authored recurrence count.

Paging is deterministic and read-only. Its offset cursor is projection coordinates only, not a durable cursor, occurrence identity, idempotency key, lifecycle revision, due or catch-up decision, permission, claim, or execution evidence.

## Alternatives considered

### Return all remaining occurrences

Rejected because a valid recurrence may contain up to `u64::MAX` authored offsets, making eager allocation unbounded.

### Treat a start at the finite end as an empty page

Rejected because there is no authored occurrence at that coordinate. Typed out-of-range evidence keeps invalid input distinct from a valid truncated final page.

### Expose an unbounded lazy iterator

Rejected for this slice because it leaves work bounds implicit and does not establish a portable page/cursor contract for later read boundaries.

### Persist the page cursor

Rejected because persistence introduces lifecycle, ownership, concurrency, and resumption semantics that read-only projection does not require.

## Consequences

- Callers can enumerate finite recurrences through deterministic bounded pages.
- Every non-final cursor names the exact first unreturned authored offset.
- Allocation is capped at 1024 occurrence projections per call.
- No event schema, recurrence history, ambient clock, or execution boundary changes.
- Persisted occurrences, catch-up policy, materialization, claims, dispatch, retries, and execution remain deferred.

## Verification

RED→GREEN tests prove interior and truncated final pages, ordering, complete provenance, cursor behavior, maximum-instant arithmetic, invalid starts, and zero/oversized page validation. The complete repository quality gate must remain green.

## Revisit when

Reconsider before changing the page cap or cursor shape, exposing paging through storage or CLI boundaries, or adding persisted occurrence identity, generated schedule/task IDs, catch-up, materialization, claims, dispatch, retries, workers, calendar/time-zone semantics, ambient clocks, or execution.
