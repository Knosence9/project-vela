# ADR-0101: Bounded one-shot schedule lifecycle-history paging

- **Status:** accepted
- **Date:** 2026-08-08
- **Decision and execution issue:** [#1007](https://github.com/Knosence9/project-vela/issues/1007)
- **Related:** ADR-0034, ADR-0093

## Context

`ScheduleStore::history` exposes one known one-shot schedule's complete typed lifecycle evidence. `ScheduleStore::list_page` bounds global inventory discovery but returns only each schedule's current projection. An auditor that needs lifecycle evidence for discovered schedules must therefore page inventory and issue a separate exact replay for every result, which can split one logical page across storage snapshots and duplicate cursor and corruption handling in callers.

A schedule can repeat claim and release transitions, so its exact lifecycle can grow independently of inventory size. Returning partial per-stream event windows would require a separate continuation contract that carries enough lifecycle state to validate later events without accepting an invalid prefix. The smallest responsible boundary instead bounds the number of authoritative schedule streams selected while preserving complete fail-closed replay for each selected stream.

## Decision

Add `ScheduleHistory`, containing one exact `ScheduleId` and its complete revision-ordered `ScheduleHistoryEntry` values. Add `ScheduleHistoryPage`, containing exact-ID-ordered histories and an optional `next_after` exact schedule ID.

`ScheduleStore::histories_page(after, page_size)` accepts the existing validated `SchedulePageSize` bound from 1 through 1024. It selects at most `page_size + 1` authoritative streams marked by `schedule.created` after the optional caller-owned exclusive exact-ID cursor and fully replays those streams in one read snapshot. Every selected lifecycle, including lookahead, is canonically projected before any output is returned.

When validated lookahead proves another schedule exists, the page removes lookahead and sets `next_after` to the last returned schedule ID. Otherwise the cursor is absent. The cursor need not identify an existing schedule. Empty stores and cursors beyond the end return empty terminal pages.

The operation works through read-only storage, reads no ambient clock, mutates nothing, persists no cursor, and grants no lifecycle, worker, lease, dispatch, retry, permission, or execution authority.

## Alternatives considered

### Page inventory and call exact history in the caller

Rejected because page selection and exact histories would not share one read snapshot, and every caller would have to reproduce continuation and fail-closed aggregation behavior.

### Page individual lifecycle events by revision

Rejected for this slice because validating a non-initial event page requires prior lifecycle state. Replaying the complete prefix would bound output but not work, while trusting caller-supplied state could accept impossible histories. A safe partial-history protocol needs a separate snapshot and state-proof decision.

### Add a materialized schedule-history index

Rejected because the existing canonical event streams already support bounded authoritative selection. A new index would add schema, migration, consistency, and repair authority without reducing per-schedule lifecycle replay.

## Consequences

- Global schedule selection and returned-history count are bounded by one explicit page plus lookahead.
- Replay and aggregate allocation within each selected schedule remain proportional to that schedule's complete lifecycle history.
- Exact goals, due instants, claims, releases, cancellation reasons, task bindings, and one-based revisions remain inspectable without a second storage snapshot.
- Corruption in the returned window or lookahead fails closed before partial output. Corruption before the cursor, beyond lookahead, and in unrelated stream families remains isolated.
- Existing exact history, current inventory, status, and due projections remain unchanged.
- No event, schema, write, clock, worker, or execution authority is added.

## Verification

Strict RED→GREEN tests prove complete multi-transition evidence, exact-ID ordering, non-overlapping continuation, nonexistent and beyond-end cursors, empty and terminal behavior, selected and lookahead corruption failures, and isolation from corruption before the cursor, after lookahead, and in unrelated streams. The complete repository quality gate must remain green.

## Revisit when

Reconsider before adding CLI exposure, partial per-stream history paging, snapshot tokens, durable cursors, lifecycle indexes, destructive deletion, clocks, workers, leases, dispatch, retries, permissions, or execution.
