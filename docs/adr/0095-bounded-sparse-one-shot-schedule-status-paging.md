# ADR-0095: Bounded sparse one-shot schedule status paging

- **Status:** accepted
- **Date:** 2026-08-08
- **Decision and execution issue:** [#993](https://github.com/Knosence9/project-vela/issues/993)
- **Related:** ADR-0089, ADR-0093, ADR-0094

## Context

The complete one-shot schedule status projection validates and filters the entire authoritative inventory. ADR-0093 adds bounded exact-ID inventory pages but deliberately does not compose filtering with paging: filling a requested number of matches could scan an unbounded number of nonmatching schedules. Its last-emitted cursor also cannot advance when a bounded window contains no matches.

Callers need bounded storage work more than dense output. Existing exact schedule IDs can identify scan progress without a persisted cursor, index, snapshot token, or new identity.

## Decision

Add `ScheduleStatusPage`, containing complete matching schedules and an optional `next_after` exact schedule ID. Add `ScheduleStore::list_by_status_page(status, after, scan_size)`, reusing validated `SchedulePageSize` values from 1 through 1024 as an explicit bound on inspected authoritative schedules.

The event log selects at most `scan_size + 1` authoritative `schedule.created` streams after the optional caller-owned exclusive exact-ID cursor. It fully replays that exact-ID-ordered window in one read snapshot. Canonical projection validates every selected history, including lookahead, before filtering. The first at most `scan_size` projections are filtered by exact persisted `ScheduleStatus` and matching schedules retain exact-ID order.

When validated lookahead proves more inventory exists, `next_after` is the last **inspected** schedule ID, even when no schedule matched. Otherwise it is absent. The cursor need not identify an existing schedule. Empty stores and cursors beyond the end return empty terminal pages.

This sparse scan cursor is intentionally distinct from `SchedulePage::next_after`, which identifies the last emitted schedule. The operation works through read-only storage, reads no ambient clock, mutates nothing, persists no cursor, and grants no lifecycle, worker, lease, dispatch, retry, permission, or execution authority.

## Alternatives considered

### Filter `ScheduleStore::list_page` results in each caller

Rejected because callers could accidentally resume from a matching item rather than the last inspected coordinate, repeat nonmatches, or fail to advance after an empty page. The kernel owns storage selection and corruption boundaries.

### Fill a dense page of matching schedules

Rejected because a sparse inventory can require unbounded stream selection, replay, and allocation to find one match.

### Change `SchedulePage` cursor semantics

Rejected because its established cursor means the last emitted schedule. Overloading it with scan progress would make identical fields carry incompatible continuation contracts.

### Add a durable status index

Rejected for this slice because sparse bounded inspection satisfies incremental callers without schema, migration, index-maintenance, or repair authority.

## Consequences

- Storage selection, replay, and allocation are bounded by one explicit scan window plus lookahead; result density may range from zero through `scan_size`.
- Empty nonterminal pages can advance safely and continuation windows do not overlap.
- Corruption in the inspected window or lookahead fails closed before any partial page is returned. Corruption before the cursor, beyond lookahead, or in unrelated stream families remains isolated.
- Complete status inventory retains its global fail-closed contract, and ordinary inventory paging retains its last-emitted cursor contract.
- No schema, event, write, clock, worker, or execution authority is added.

## Verification

Strict RED→GREEN tests prove sparse mixed-status selection, an all-nonmatching advancing page, exact ordering, non-overlapping continuation, nonexistent and beyond-end cursors, terminal and empty read-only behavior, unrelated-stream exclusion, selected and lookahead corruption failures, and corruption isolation outside the bounded window. The complete repository quality gate must remain green.

## Revisit when

Reconsider before adding a CLI adapter, due-filtered pages, dense matching pages, durable indexes or cursors, snapshot tokens across calls, destructive deletion, clocks, workers, leases, dispatch, permissions, retries, or execution.
