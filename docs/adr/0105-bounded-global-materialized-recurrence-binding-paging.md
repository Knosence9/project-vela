# ADR-0105: Bounded global materialized recurrence binding paging

- **Status:** accepted
- **Date:** 2026-08-08
- **Decision and execution issue:** [#1017](https://github.com/Knosence9/project-vela/issues/1017)
- **Related:** ADR-0045, ADR-0050, ADR-0052, ADR-0054, ADR-0091

## Context

`RecurrenceStore::materialized_occurrences_page` exposes a bounded authored-offset window for one known recurrence, while `find_materialized_by_task_id` resolves one known task identity. An auditor or recovery adapter that knows neither coordinate must otherwise page every recurrence definition and inspect every authored offset, including absent and non-materialized coordinates. Those calls do not share one storage snapshot and duplicate discovery and corruption handling.

Materialized occurrence streams already carry an authoritative `recurrence.occurrence_materialized` marker and use collision-free canonical stream keys. Those keys are deterministic but intentionally encode the recurrence ID byte length and decimal offset. Their lexical order is therefore not recurrence-ID order or numeric-offset order. Reinterpreting JSON payload values in SQL would let malformed coordinate payloads move outside the selected validation window and would not preserve the full `u64` offset domain safely.

## Decision

Add `MaterializedRecurrenceOccurrenceCursor`, an owned typed recurrence ID and authored offset that never exposes the internal stream key, and `GlobalMaterializedRecurrenceOccurrencePage`, containing complete bindings plus an optional exclusive cursor.

`RecurrenceStore::materialized_occurrences_global_page(after, page_size)` accepts the existing validated `OccurrencePageSize` bound from 1 through 1024. It derives the optional cursor's canonical stream key internally, selects at most `page_size + 1` streams carrying materialization markers in lexical canonical stream-key order, and completely replays those selected streams in one read snapshot.

Every selected stream, including lookahead, must have a canonical occurrence stream ID, an authoritative recurrence definition, a complete valid occurrence lifecycle, provenance matching that definition and coordinate, and a final materialized task binding. Only after all selected evidence validates does the method remove lookahead and return the last emitted coordinate as `next_after`. Empty inventories and cursors beyond the final key return empty terminal pages.

The order is deliberately opaque. Callers may retain and return the typed cursor, but must not infer recurrence-ID or numeric-offset ordering from page position. The page reports each complete validated binding but does not prove that its task identity is unique outside the selected window; exact reverse lookup retains that global ambiguity check.

The boundary works through read-only storage, reads no clock, validates no task lifecycle, mutates nothing, persists no cursor, and grants no recurrence or task lifecycle, worker, lease, dispatch, retry, permission, or execution authority.

## Alternatives considered

### Order by recurrence ID and numeric offset extracted from JSON

Rejected because malformed coordinate payloads could be filtered or reordered before canonical Rust validation, and SQLite JSON numbers do not preserve exact ordering across the complete `u64` offset domain.

### Expose raw stream IDs as cursors

Rejected because internal namespace and encoding details are not caller authority and should remain changeable behind a typed coordinate.

### Page every recurrence and authored offset

Rejected because sparse materialized evidence could require work proportional to every absent coordinate, and callers would split discovery and validation across snapshots.

### Claim global task-identity uniqueness while paging

Rejected because a bounded page cannot prove uniqueness outside its selected window. `find_materialized_by_task_id` remains the exact global ambiguity boundary.

## Consequences

- Global materialized-binding discovery and returned allocation are bounded by one explicit page plus lookahead.
- Complete selected occurrence histories and their authoritative recurrence definitions are still replayed proportionally to those selected streams.
- Canonical lexical stream-key order is deterministic and resumable but intentionally opaque.
- Selected and lookahead corruption fail before output; evidence at or before the cursor, beyond lookahead, and unrelated stream families remains isolated.
- Separator-containing recurrence IDs and every `u64` authored offset remain collision-free without SQL numeric reinterpretation.
- No event, schema, write, clock, task execution, or permission authority is added.

## Verification

Strict RED→GREEN tests prove deterministic multi-page traversal, typed exclusive cursors, empty and terminal pages, separator-containing IDs, multi-digit offsets, selected and lookahead failure, before-cursor and beyond-lookahead isolation, invalid stream rejection, missing-definition failure, and read-only reopen. The complete repository quality gate must remain green.

## Revisit when

Reconsider before exposing this cursor through a serialization boundary, changing occurrence stream encoding, adding a durable ordering index, claiming task uniqueness across pages, validating task lifecycle, or adding clocks, workers, leases, dispatch, retries, permissions, or execution.
