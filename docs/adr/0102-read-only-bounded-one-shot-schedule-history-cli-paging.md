# ADR-0102: Read-only bounded one-shot schedule history CLI paging

- **Status:** accepted
- **Date:** 2026-08-08
- **Decision and execution issue:** [#1009](https://github.com/Knosence9/project-vela/issues/1009)
- **Related:** ADR-0088, ADR-0094, ADR-0101

## Context

ADR-0101 provides bounded exact-ID discovery of complete one-shot schedule lifecycle histories in one read snapshot. Operators otherwise must page current inventory and issue one exact-history command per schedule, splitting selection and audit evidence across snapshots and duplicating continuation and corruption handling.

The CLI adapter must preserve the kernel's read-only authority and complete-history validation. It must not turn bounded audit discovery into event-level paging or lifecycle authority.

## Decision

Add `vela-dev schedule histories DATABASE PAGE_SIZE [AFTER]`. The command validates a positive page size of at most 1024 and the optional non-blank exact schedule ID before storage access, then opens only the selected existing database through `ScheduleStore::open_read_only`.

The adapter delegates to `ScheduleStore::histories_page`. Success emits compact deterministic JSON with `histories` ordered by exact schedule ID and `next_after` naming the last emitted ID only when validated lookahead proves another history exists. Each result contains its exact ID and complete revision-ordered tagged creation, claim, release, cancellation, and materialization evidence. Exact caller-authored strings remain JSON escaped. Exact and paged history commands share one fail-closed typed-entry conversion.

The cursor is exclusive and need not identify an existing schedule. Empty stores and beyond-end cursors emit an empty array and `null` cursor. Invalid sizes and cursors emit `invalid_schedule_page_size` and `invalid_schedule_id`; missing storage, replay or projection corruption in the selected window or lookahead, unsupported future history variants, and serialization failures emit `schedule_history_page_inspection_failed`, non-zero, with no partial stdout.

The command never creates storage, persists a cursor, reads ambient time, pages inside an individual lifecycle, mutates schedule state, dispatches, retries, grants permission, or executes work.

## Alternatives considered

### Page inventory and repeat exact history commands

Rejected because selection and histories would not share one read snapshot and callers would duplicate fail-closed lookahead policy.

### Return raw event-log rows

Rejected because callers could consume malformed or impossible lifecycle prefixes and would duplicate canonical projection.

### Page events inside each history

Rejected because safe continuation requires prior lifecycle state or a separate validated snapshot proof. Output and work would otherwise have different bounds.

## Consequences

- Operators can audit bounded exact-ID windows of complete schedule histories through deterministic JSON.
- Selected and lookahead corruption fail before output; corruption before the cursor, beyond lookahead, and in unrelated stream families remains isolated.
- Page size bounds selected schedules, not events within each complete lifecycle.
- No schema, event, write, clock, worker, permission, or execution authority is added.

## Verification

RED→GREEN CLI integration tests prove complete repeated lifecycle evidence, exact escaping and ordering, non-overlapping continuation, nonexistent and beyond-end cursors, empty and terminal behavior, pre-storage validation, missing-storage non-creation, selected and lookahead corruption failure, and bounded corruption isolation. The complete repository quality gate must remain green.

## Revisit when

Reconsider before adding partial per-stream history paging, snapshot tokens, durable cursors, lifecycle indexes, destructive deletion, clocks, workers, leases, dispatch, retries, permissions, or execution.
