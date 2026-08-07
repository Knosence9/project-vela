# ADR-0094: Read-only bounded one-shot schedule inventory CLI paging

- **Status:** accepted
- **Date:** 2026-08-07
- **Decision and execution issue:** [#989](https://github.com/Knosence9/project-vela/issues/989)
- **Related:** ADR-0083, ADR-0092, ADR-0093

## Context

ADR-0093 exposes allocation- and replay-bounded exact-ID keyset paging through `ScheduleStore::list_page`. Operators otherwise need custom kernel code to use it and could duplicate page-size validation, cursor interpretation, diagnostics, serialization, or storage authority policy.

The smallest responsible adapter should preserve the kernel's canonical selected-window validation and existing complete schedule representation without introducing a second cursor, filter, or lifecycle contract.

## Decision

Add `vela-dev schedule page DATABASE PAGE_SIZE [AFTER]`. `PAGE_SIZE` must form a positive, at-most-1024 `SchedulePageSize`. Optional `AFTER` must form one exact non-blank `ScheduleId`; it is an exclusive caller-owned keyset cursor and need not identify an existing schedule. Both inputs are validated before storage access.

The command opens only the selected existing database through `ScheduleStore::open_read_only` and delegates selection to `ScheduleStore::list_page`. Success emits compact `{"schedules":[...],"next_after":...}` JSON. Schedules reuse the complete inventory object, retain exact-ID ordering, and preserve JSON-escaped caller-authored strings and lifecycle evidence. `next_after` contains the last emitted exact ID only when the kernel validated a lookahead; terminal, empty, and beyond-end pages emit `null`.

Invalid sizes and cursors emit `invalid_schedule_page_size` and `invalid_schedule_id` before storage access. Missing or incompatible storage, selected-window corruption, projection failures, and serialization failures emit `schedule_page_inspection_failed`, return non-zero, and emit no partial stdout. Missing storage is never created.

The command reads no clock, mutates nothing, persists no cursor, and grants no status- or due-filtered discovery, lifecycle, worker, lease, dispatch, retry, permission, or execution authority.

## Alternatives considered

### Add paging flags to `schedule inspect`

Rejected because a distinct output shape with continuation evidence deserves an explicit command and keeps the existing complete-inventory contract behavior-compatible.

### Accept an empty cursor as the first page

Rejected because `ScheduleId` has one canonical non-blank validation contract. Cursor absence already represents the first page without inventing a sentinel identity.

### Add lifecycle or due filtering

Rejected because the existing storage shape cannot bound the number of nonmatching schedules scanned to fill one result page. ADR-0093 deliberately leaves bounded filtered paging as a separate index or sparse-cursor decision.

## Consequences

- Operators can traverse one-shot schedules without complete-inventory replay or allocation.
- CLI cursor and lookahead behavior remain identical to the kernel contract.
- Selected lookahead corruption fails closed while corruption outside the selected window remains isolated.
- Existing complete, status-filtered, and due inventory commands remain unchanged.
- No schema, event, mutation, clock, or execution authority is added.

## Verification

Strict RED→GREEN CLI tests prove exact-ID ordering, continuation without overlap, terminal and empty results, exact string escaping, size and cursor validation before storage access, missing-storage non-creation, selected-lookahead failure, and corruption isolation outside the bounded window. The complete repository quality gate must remain green.

## Revisit when

Reconsider before adding status- or due-filtered pages, snapshot tokens across calls, destructive deletion, resume or undo semantics, clocks, workers, leases, dispatch, permissions, retries, or execution.
