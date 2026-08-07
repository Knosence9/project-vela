# ADR-0091: Bounded finite recurrence inventory paging

- **Status:** accepted
- **Date:** 2026-08-07
- **Decision and execution issue:** [#983](https://github.com/Knosence9/project-vela/issues/983)
- **Related:** ADR-0040, ADR-0089, ADR-0090

## Context

The complete recurrence inventory and lifecycle-status filter validate every authoritative recurrence history before returning deterministic exact-ID-ordered definitions. Their work and allocation grow with the complete inventory. Callers that need incremental inspection cannot impose a storage bound and must repeatedly replay earlier definitions.

Authored recurrence IDs already provide a stable exact ordering and the event log can select authoritative creation streams in that order. An exclusive exact-ID keyset cursor avoids positional drift and does not require a persisted cursor, schema change, or a second identity.

## Decision

Add `RecurrencePageSize`, accepting exactly `1..=1024`, and `RecurrencePage`, containing complete recurrence definitions plus an optional `next_after` exact recurrence ID. Add `RecurrenceStore::list_page(after, page_size)`, where `after` is an optional caller-owned exclusive cursor. The cursor need not identify an existing definition.

The event log selects authoritative `recurrence.fixed_interval_created` streams whose internal stream IDs sort after the cursor, in ascending exact-ID order, with a limit of `page_size + 1`. It replays the complete selected histories in the same statement and canonical recurrence projection validates all selected streams, including the one-item lookahead, before any page is returned. At most `page_size` definitions are emitted. `next_after` is the last emitted exact ID only when the validated lookahead proves another definition exists; otherwise it is absent. Empty stores and cursors beyond the end return an empty terminal page.

Malformed selected stream IDs, payloads, event types, payload versions, version sequences, definitions, or lifecycle ordering fail closed without a partial page. Corruption before the exclusive cursor or after the one-item lookahead is outside the bounded query and cannot block the page. Unrelated event families remain excluded.

The cursor is caller-owned projection state. The operation works through read-only storage, reads no ambient clock, mutates nothing, persists no cursor, and grants no cancellation, occurrence lifecycle, worker, lease, dispatch, retry, permission, or execution authority.

## Alternatives considered

### Slice `RecurrenceStore::list` in memory

Rejected because output would be bounded while storage work, replay, and allocation would still grow with the complete inventory.

### Use a numeric offset

Rejected because every later page would rescan an increasing prefix and concurrent insertions before the offset could create overlaps or omissions unrelated to an explicit authored coordinate.

### Return the lookahead ID as the cursor

Rejected because an exclusive cursor naming the unreturned lookahead would skip that definition. The last returned ID composes directly with the next exclusive query.

### Combine status filtering and paging

Rejected because status filtering may require scanning an unbounded number of nonmatching definitions to fill one result page. A responsible bounded status-page contract needs a separate index, scan bound, or sparse-cursor decision.

## Consequences

- Kernel callers can inspect complete recurrence definitions with bounded stream selection, replay, and allocation.
- Keyset continuation has no overlap and accepts caller-owned cursors that no longer or never existed.
- Selected lookahead corruption blocks a misleading continuation, while corruption beyond the bounded window remains isolated.
- Complete inventory and status filtering retain their existing global fail-closed contracts.
- No schema, event, mutation, clock, or execution authority is added.

## Verification

Strict RED→GREEN tests prove page-size validation, exact-ID ordering, non-overlapping continuation, nonexistent and beyond-end cursors, empty and read-only behavior, unrelated-stream exclusion, selected lookahead failure, and corruption isolation beyond the bounded window. The complete repository quality gate must remain green.

## Revisit when

Reconsider before adding a CLI adapter, status-filtered pages, snapshot tokens across calls, destructive deletion, resume or undo semantics, clocks, workers, leases, dispatch, permissions, retries, or execution.
