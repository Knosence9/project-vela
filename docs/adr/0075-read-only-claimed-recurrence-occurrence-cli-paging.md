# ADR-0075: Read-only claimed recurrence occurrence CLI paging

- **Status:** accepted
- **Date:** 2026-08-05
- **Decision and execution issue:** [#945](https://github.com/Knosence9/project-vela/issues/945)
- **Related:** ADR-0049, ADR-0055, ADR-0068, ADR-0069, ADR-0072, ADR-0074

## Context

ADR-0074 defines bounded authored-offset paging for current recurrence occurrence claims. Exact lookup and writable lifecycle commands require callers to know each coordinate, leaving recovery and operator tooling without a deterministic recurrence-local view of current reservations.

The smallest responsible adapter exposes the accepted kernel boundary without adding global claim discovery, claim-next selection, worker identity, leases, or lifecycle authority.

## Decision

`vela-dev recurrence claimed DATABASE RECURRENCE_ID START_OFFSET PAGE_SIZE` validates the exact recurrence identity through `RecurrenceId` and the allocation bound through `OccurrencePageSize` before storage access. It opens only the selected existing database through `RecurrenceStore::open_read_only` and delegates the selected authored-offset window to `RecurrenceStore::claimed_occurrences_page`.

Success emits one compact JSON object. `occurrences` contains complete current claims in increasing authored-offset order; each preserves exact `recurrence_id`, `goal`, `offset`, `unix_millis`, `definition_revision`, and `occurrence_revision`. Missing, persisted-only, released, and materialized coordinates are omitted. `next_offset` advances by inspected authored coordinates, including all-gap windows, and is `null` at the finite definition end.

Invalid identities and page sizes emit `invalid_recurrence_id` and `invalid_occurrence_page_size` before storage access. Missing definitions emit `recurrence_not_found`; starts outside the finite definition emit `recurrence_occurrence_out_of_range`. Open, strict selected-window replay, provenance, paging, and serialization failures emit `claimed_recurrence_occurrence_lookup_failed`, return non-zero, and emit no stdout. Missing storage remains missing. Corruption outside the selected recurrence or authored window cannot block the page.

The command is read-only and inert. It reads no ambient time, mutates no lifecycle, persists no cursor, performs no global inventory, and grants no claim-next selection, generated identity, worker identity, lease, dispatch, workflow, provider/tool, permission, retry, or execution authority.

## Alternatives considered

### Filter claim events in the CLI

Rejected because claim events alone are not authoritative current state. The CLI would duplicate canonical lifecycle replay, miss release and materialization transitions, and broaden the corruption domain.

### Return every occurrence lifecycle with a status

Rejected because this command promises complete current claims. Existing persisted and materialized commands retain their distinct evidence contracts.

### Add global claimed-occurrence inventory

Rejected because no bounded cross-recurrence ordering or cursor contract exists. One exact caller-selected recurrence is the narrower responsible boundary.

## Consequences

- Operators can page sparse current reservations through deterministic machine-readable output.
- CLI allocation, cursor, ordering, lifecycle filtering, and corruption isolation remain aligned with the kernel boundary.
- Released and materialized coordinates disappear from later claimed views while authored-window progress remains deterministic.
- Global discovery, claim-next selection, workers, leases, dispatch, retries, and execution remain deferred.

## Verification

RED→GREEN CLI integration tests prove deterministic escaped JSON, exact claim revisions, released and materialized omission, all-gap progress, final-page termination, validation before storage access, typed missing and bounds failures, read-only missing-path behavior, and selected-window corruption isolation. The complete repository quality gate must remain green.

## Revisit when

Reconsider before adding cross-recurrence claimed inventory, claim-next selection, durable cursors, generated task identity, workers, leases or expiry, ambient clocks, dispatch, retries, permissions, or execution.
