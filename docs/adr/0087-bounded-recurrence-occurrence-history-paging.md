# ADR-0087: Bounded recurrence occurrence history paging

- **Status:** accepted
- **Date:** 2026-08-07
- **Decision and execution issue:** [#973](https://github.com/Knosence9/project-vela/issues/973)
- **Related:** ADR-0039, ADR-0048, ADR-0074, ADR-0085, ADR-0086

## Context

ADR-0085 exposes complete typed history when a caller already knows one recurrence identity and authored offset. Recovery and audit surfaces otherwise need one exact query per coordinate and cannot discover which coordinates have durable lifecycle evidence without decoding raw event-log rows.

The finite recurrence definition already provides a stable authored-offset space and the existing occurrence page size provides a validated work and allocation bound. Reusing those coordinates permits recurrence-local discovery without introducing an unbounded global inventory or durable cursor.

## Decision

Add `RecurrenceStore::occurrence_histories_page(id, start_offset, page_size)`. The operation strictly loads one exact immutable recurrence definition, projects at most one validated `OccurrencePageSize` authored window, and replays every present occurrence stream in that window.

Each result is a `RecurrenceOccurrenceHistory` containing its exact authored offset and complete revision-bearing `RecurrenceOccurrenceHistoryEntry` sequence. Histories remain ordered by increasing offset. Missing streams are omitted, so a valid page may be empty. `next_offset` identifies the first uninspected authored coordinate or finite completion and therefore advances independently of history density.

The selected definition is replayed once. Every present selected stream reuses the same canonical lifecycle projector as exact history, preserving persistence, claim, release, and materialization evidence while rejecting malformed, unsupported, divergent, or impossible histories before any page is returned. Unrelated recurrences and coordinates outside the selected window are not inspected.

The boundary works through read-only storage, reads no clock, mutates nothing, persists no cursor, and grants no global discovery, persistence, cancellation, claim, release, materialization, worker, lease, dispatch, permission, retry, or execution authority.

## Alternatives considered

### Discover occurrence events globally

Rejected because an unbounded cross-recurrence inventory requires separate ordering, pagination, indexing, and corruption-domain decisions.

### Return current occurrence state only

Rejected because audit and recovery callers need prior claim/release cycles and exact revisions, not only the latest projection.

### Use the next present history as the cursor

Rejected because finding it requires unbounded sparse discovery or a new storage index. Authored offsets already provide finite deterministic progress across empty windows.

### Add the CLI adapter in the same slice

Rejected because the reusable kernel contract and its corruption isolation should be independently verified before fixing a JSON and diagnostic contract.

## Consequences

- Callers can discover complete sparse occurrence histories with bounded recurrence-local work.
- Empty pages advance deterministically and finite completion remains explicit.
- Exact and paged history share one lifecycle projection path.
- No event, database migration, mutable lifecycle, clock, or execution authority is added.

## Verification

Strict RED→GREEN tests prove sparse repeated claim/release and materialized histories, exact offsets and revisions, all-gap and finite cursor behavior, read-only reopen, missing and out-of-range inputs, selected-window fail-closed corruption, and isolation from unrelated and out-of-window corruption. The complete repository quality gate must remain green.

## Revisit when

Reconsider before adding a CLI adapter, global or cross-recurrence history discovery, persisted cursors, destructive deletion, claim interruption, undo or resume semantics, clocks, workers, leases, dispatch, permissions, retries, or execution.
