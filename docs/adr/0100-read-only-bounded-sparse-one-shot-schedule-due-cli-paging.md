# ADR-0100: Read-only bounded sparse one-shot schedule due CLI paging

- **Status:** accepted
- **Date:** 2026-08-08
- **Decision and execution issue:** [#1003](https://github.com/Knosence9/project-vela/issues/1003)
- **Related:** ADR-0034, ADR-0094, ADR-0099

## Context

ADR-0099 exposes bounded sparse pending-due schedule paging through `ScheduleStore::list_due_page`. Operators otherwise need custom kernel code to use it. The existing `schedule due` command intentionally performs complete due discovery in due-instant then exact-ID order, while client-side filtering of ordinary inventory pages can misinterpret empty sparse pages or resume from the last emitted match instead of the last inspected schedule.

The CLI adapter must preserve the kernel's exact-ID storage-work bound, inclusive cutoff, and scan-progress cursor without adding dense-fill behavior, ambient time, or mutation authority.

## Decision

Add `vela-dev schedule due-page DATABASE CUTOFF_UNIX_MILLIS SCAN_SIZE [AFTER]`. Clap parses the explicit cutoff as a non-negative `u64`. `SCAN_SIZE` must form a positive, at-most-1024 `SchedulePageSize`. Optional `AFTER` must form one exact non-blank `ScheduleId`; it is an exclusive caller-owned scan cursor and need not identify an existing schedule. All adapter validation completes before storage access.

The command opens only the selected existing database through `ScheduleStore::open_read_only` and delegates bounded selection, complete selected-window replay, pending/due filtering, lookahead validation, and continuation to `ScheduleStore::list_due_page`. Success emits compact `{"schedules":[...],"next_after":...}` JSON using the existing complete schedule representation. Matching schedules retain exact-ID scan order, including when their due instants differ, and exact caller-authored content is JSON escaped.

`next_after` preserves the kernel's last-inspected semantics. A page may contain no matches and still return a non-null cursor when validated lookahead proves more inventory exists. Terminal, empty, and beyond-end windows return `null`.

Invalid scan sizes and cursors emit `invalid_schedule_page_size` and `invalid_schedule_id`; malformed cutoff syntax is rejected by clap. Missing or incompatible storage, selected or lookahead corruption, projection failures, and serialization failures emit `schedule_due_page_inspection_failed`, return non-zero, and emit no partial stdout. Missing storage is never created.

The command reads no ambient clock, mutates nothing, persists no cursor, and grants no dense-fill scan, lifecycle, worker, claim, materialization, dispatch, retry, permission, or execution authority.

## Alternatives considered

### Add paging flags to `schedule due`

Rejected because the existing command has a complete fail-closed inventory contract, due-instant ordering, and an output shape without continuation evidence. An explicit command keeps that behavior compatible.

### Filter `schedule page` output in callers

Rejected because ordinary paging does not own the pending/due projection contract, and callers can mishandle all-nonmatching windows or corruption boundaries.

### Fill a dense page of due matches

Rejected because future or non-pending schedules could require unbounded storage selection, replay, and allocation.

## Consequences

- Operators can traverse pending-due schedules with explicitly bounded storage work.
- Empty nonterminal pages remain distinguishable from terminal pages.
- CLI continuation, inclusive cutoff, exact-ID ordering, and corruption boundaries remain identical to the kernel contract.
- Complete due discovery and ordinary inventory and status paging remain unchanged.
- No schema, event, write, clock, or execution authority is added.

## Verification

Strict RED→GREEN CLI tests prove sparse mixed-due and lifecycle selection, inclusive cutoff behavior, all-nonmatching progress, exact-ID ordering and escaping, non-overlapping continuation, nonexistent and beyond-end cursors, terminal and empty output, validation before storage access, missing-storage non-creation, selected and lookahead corruption failures, and isolation outside the bounded window. The complete repository quality gate must remain green.

## Revisit when

Reconsider before adding dense due-result pages, durable due-instant indexes or cursors, snapshot tokens across calls, destructive deletion, clocks, workers, leases, dispatch, permissions, retries, or execution.
