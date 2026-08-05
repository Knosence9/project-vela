# ADR-0074: Bounded claimed recurrence occurrence paging

- **Status:** accepted
- **Date:** 2026-08-05
- **Decision and execution issue:** [#943](https://github.com/Knosence9/project-vela/issues/943)
- **Related:** ADR-0039, ADR-0048, ADR-0054, ADR-0068, ADR-0069, ADR-0072

## Context

Exact claimed recurrence occurrences can be loaded only when callers already know both the recurrence identity and authored offset. Recovery and operator surfaces need bounded visibility into current reservations without granting mutation, global discovery, or worker authority.

The existing persisted and materialized page boundaries establish an allocation-bounded, resumable model: inspect one authored-offset window, filter by strict current lifecycle state, and advance the cursor by inspected authored coordinates rather than result density. Reusing that model avoids an unbounded global claim inventory and keeps sparse recurrence evidence deterministic.

## Decision

Add `RecurrenceStore::claimed_occurrences_page(id, start_offset, page_size)`. The operation strictly loads one exact immutable recurrence definition, projects at most the existing validated `OccurrencePageSize` authored coordinates, and strictly replays every present occurrence stream in that selected window.

The result contains complete `ClaimedRecurrenceOccurrence` values only for coordinates whose current lifecycle state is claimed, in increasing authored-offset order. Missing, persisted-only, released, and materialized coordinates are omitted. `next_offset` identifies the first uninspected authored coordinate or finite completion, so valid all-gap pages still advance deterministically.

Missing recurrence definitions and out-of-range starts preserve existing typed failures. Malformed or impossible evidence in the selected window fails closed before any partial page is returned. Unrelated recurrence streams and coordinates outside the selected authored window are not inspected and cannot block a valid page.

The operation works through read-only storage, reads no clock, mutates nothing, persists no cursor, and grants no global discovery, claim-next selection, generated identity, worker identity, lease, dispatch, retry, permission, workflow, provider/tool, or execution authority.

## Alternatives considered

### Discover claims globally from claim events

Rejected because an unbounded cross-recurrence inventory has different ordering, allocation, corruption-isolation, and pagination requirements. Exact recurrence ownership keeps this slice bounded and consistent with existing recurrence pages.

### Return every persisted lifecycle with a status field

Rejected as unnecessary scope. Existing persisted and materialized projections already expose their own evidence, while recovery callers need the exact current-claim subset.

### Persist a claim cursor

Rejected because inspection coordinates are caller-owned read state, not durable acceptance or work-consumption evidence.

## Consequences

- Callers can inspect current reservations through a bounded read-only recurrence-local window.
- Sparse pages advance independently of claim density and preserve exact occurrence revisions.
- Releases and materializations disappear from subsequent claimed pages without rewriting earlier evidence.
- Global inventory and claim-next selection remain separate future authority boundaries.

## Verification

Strict RED→GREEN integration tests cover sparse mixed lifecycle filtering, exact revisions, all-gap and finite cursor behavior, read-only reopen, missing definitions, out-of-range starts, and selected-window corruption isolation. The complete repository quality gate must remain green.

## Revisit when

Reconsider before adding CLI exposure, cross-recurrence claimed inventory, claim-next selection, durable cursors, generated task identity, workers, leases or expiry, ambient clocks, dispatch, retries, permissions, or execution.
