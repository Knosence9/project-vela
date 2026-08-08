# ADR-0106: Read-only global materialized recurrence binding CLI paging

- **Status:** accepted
- **Date:** 2026-08-08
- **Decision and execution issue:** [#1019](https://github.com/Knosence9/project-vela/issues/1019)
- **Related:** ADR-0053, ADR-0055, ADR-0105

## Context

ADR-0105 provides bounded global discovery of complete materialized recurrence bindings through a typed opaque cursor. Recovery and audit automation still has to embed the kernel to use that projection. Exposing the internal canonical occurrence stream key would leak storage encoding, while accepting independently optional cursor fields could silently restart traversal or resume from an unintended coordinate.

## Decision

Add `vela-dev recurrence materialized-page DATABASE PAGE_SIZE [--after-recurrence-id ID --after-offset OFFSET]` as a thin read-only adapter over `RecurrenceStore::materialized_occurrences_global_page`.

`PAGE_SIZE` uses the existing `OccurrencePageSize` bound from 1 through 1024. The continuation is an all-or-none pair: both recurrence ID and authored offset must be supplied, or neither. A supplied ID is validated through `RecurrenceId`; partial and invalid cursors fail before storage access. The CLI reconstructs the typed kernel cursor and never accepts or emits an internal stream key.

Success emits one compact JSON object. `occurrences` contains complete bindings exactly as returned in deterministic opaque kernel order. `next_after` is either `null` or `{ "recurrence_id": ..., "offset": ... }`; callers may round-trip that coordinate but must not infer recurrence-ID or numeric-offset ordering from it.

The command opens only existing storage through `RecurrenceStore::open_read_only`. Open, selected/lookahead replay, provenance, and serialization failures emit `global_materialized_recurrence_occurrence_lookup_failed`, return non-zero, and emit no stdout. Missing storage remains missing.

The adapter reads no clock, mutates nothing, persists no cursor, does not claim task-identity uniqueness across pages, and grants no recurrence or task lifecycle, worker, lease, dispatch, retry, permission, or execution authority.

## Alternatives considered

### Serialize the internal stream key

Rejected because storage namespace and encoding are not caller authority and must remain changeable behind the typed cursor.

### Encode the cursor as one delimiter-separated string

Rejected because recurrence IDs may contain separators. Separate named fields preserve the complete coordinate without another escaping grammar.

### Allow either cursor field independently

Rejected because an offset without an identity is meaningless, while silently ignoring a lone identity could restart traversal and duplicate audit work.

## Consequences

- Operators can discover complete materialized bindings globally without custom Rust code.
- Allocation and selected validation remain bounded by the kernel page plus lookahead.
- The JSON continuation is explicit, lossless across the full `u64` offset domain, and opaque in ordering semantics.
- Selected and lookahead corruption fail before output; unrelated evidence outside the kernel window remains isolated.
- No new storage, event, write, clock, lifecycle, or execution authority is introduced.

## Verification

Strict RED→GREEN CLI tests cover deterministic multi-page traversal, separator-containing identities, multi-digit offsets, exact cursor round-trip, terminal pages, and page-size, identity, and all-or-none cursor validation before storage access. Existing kernel tests retain empty inventory, selected/lookahead corruption, unrelated-window isolation, malformed stream, missing-definition, and read-only coverage. The complete repository quality gate must remain green.

## Revisit when

Reconsider before changing cursor serialization, exposing internal ordering, claiming global task uniqueness, persisting consumer checkpoints, or adding clocks, workers, leases, dispatch, retries, permissions, or execution.
