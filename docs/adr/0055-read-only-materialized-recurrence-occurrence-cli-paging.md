# ADR-0055: Read-only materialized recurrence occurrence CLI paging

- **Status:** accepted
- **Date:** 2026-08-03

## Context

ADR-0054 defines bounded authored-offset paging for complete materialized recurrence occurrence bindings. Operators can inspect exact bindings by task identity, but cannot page one selected recurrence's materialized bindings without writing a custom adapter.

The smallest responsible next slice exposes the existing kernel projection without adding global discovery, catch-up selection, or lifecycle authority.

## Decision

`vela-dev recurrence materialized DATABASE RECURRENCE_ID START_OFFSET PAGE_SIZE` validates the exact recurrence identity through `RecurrenceId` and the allocation bound through `OccurrencePageSize` before storage access. It opens only the selected existing database through `RecurrenceStore::open_read_only` and delegates the selected authored-offset window to `RecurrenceStore::materialized_occurrences_page`.

Success emits one compact JSON object. `occurrences` contains complete materialized bindings in ascending offset order; each preserves exact `recurrence_id`, `goal`, `offset`, `unix_millis`, `definition_revision`, `occurrence_revision`, and `task_id`. Persisted-only and missing coordinates are omitted. `next_offset` advances by inspected authored coordinates, including empty windows, and is `null` at the finite definition end.

Invalid identities and page sizes emit `invalid_recurrence_id` and `invalid_occurrence_page_size` before storage access. Missing definitions emit `recurrence_not_found`; starts outside the finite definition emit `recurrence_occurrence_out_of_range`. Open, strict selected-window replay, provenance, paging, and serialization failures emit `materialized_recurrence_occurrence_lookup_failed`, return non-zero, and emit no stdout. Missing storage remains missing. Corruption outside the selected recurrence or authored window cannot block the page.

The command is read-only and inert. It reads no ambient time, persists no cursor, performs no global inventory, and grants no catch-up, due-selection, identity generation, lifecycle mutation, claim, dispatch, workflow, provider/tool, permission, retry, or execution authority.

## Alternatives considered

### Filter all materialization events in the CLI

Rejected because it duplicates canonical stream decoding and provenance validation, expands the corruption domain, and bypasses the kernel's bounded exact-recurrence contract.

### Return persisted-only coordinates with a null task

Rejected because the command promises complete materialized bindings. Mixing lifecycle states would weaken the output contract and duplicate the existing persisted-provenance command.

### Add global materialized-binding inventory

Rejected because no global discovery contract or bounded cursor exists. One exact caller-selected recurrence is the narrower responsible authority boundary.

## Consequences

- Operators can page sparse materialized bindings through deterministic machine-readable output.
- CLI allocation, cursor, ordering, and corruption behavior remain aligned with the kernel boundary.
- Persisted-only gaps make deterministic progress without being mistaken for bindings.
- Global discovery, catch-up policy, generated identities, lifecycle mutation, dispatch, retries, and execution remain deferred.

## Verification

RED→GREEN CLI integration tests prove deterministic sparse JSON, persisted-only omission, empty-gap progress, final-page termination, validation before storage access, typed missing and bounds failures, read-only missing-path behavior, and selected-window corruption isolation. The complete repository quality gate must remain green.

## Revisit when

Reconsider before adding global materialized-occurrence discovery, catch-up or missed-run selection, generated identities, recurrence cancellation, claims or leases, workers, ambient clocks, dispatch, retries, or execution.
