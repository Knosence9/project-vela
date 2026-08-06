# ADR-0077: Read-only available recurrence occurrence CLI paging

- **Status:** accepted
- **Date:** 2026-08-05
- **Decision and execution issue:** [#949](https://github.com/Knosence9/project-vela/issues/949)
- **Related:** ADR-0049, ADR-0055, ADR-0068, ADR-0069, ADR-0075, ADR-0076

## Context

ADR-0076 defines bounded authored-offset paging for recurrence occurrences whose current lifecycle remains available. Exact lookup and writable lifecycle commands require callers to know each coordinate and its current revision, leaving recovery and operator tooling without a deterministic recurrence-local view of coordinates that can be claimed or directly materialized.

The smallest responsible adapter exposes the accepted kernel boundary without adding global availability discovery, claim-next selection, worker identity, leases, or lifecycle authority.

## Decision

`vela-dev recurrence available DATABASE RECURRENCE_ID START_OFFSET PAGE_SIZE` validates the exact recurrence identity through `RecurrenceId` and the allocation bound through `OccurrencePageSize` before storage access. It opens only the selected existing database through `RecurrenceStore::open_read_only` and delegates the selected authored-offset window to `RecurrenceStore::available_occurrences_page`.

Success emits one compact JSON object. `occurrences` contains complete current available coordinates in increasing authored-offset order. Each preserves exact `recurrence_id`, `goal`, `offset`, `unix_millis`, `definition_revision`, `occurrence_revision`, and `latest_release`. Persisted-only coordinates emit `latest_release: null`; explicitly released coordinates preserve the exact latest caller-authored reason. Missing, claimed, and materialized coordinates are omitted. `next_offset` advances by inspected authored coordinates, including all-gap windows, and is `null` at the finite definition end.

Invalid identities and page sizes emit `invalid_recurrence_id` and `invalid_occurrence_page_size` before storage access. Missing definitions emit `recurrence_not_found`; starts outside the finite definition emit `recurrence_occurrence_out_of_range`. Open, strict selected-window replay, provenance, paging, and serialization failures emit `available_recurrence_occurrence_lookup_failed`, return non-zero, and emit no stdout. Missing storage remains missing. Corruption outside the selected recurrence or authored window cannot block the page.

The command is read-only and inert. It reads no ambient time, mutates no lifecycle, persists no cursor, performs no global inventory, and grants no claim-next selection, generated identity, worker identity, lease, dispatch, workflow, provider/tool, permission, retry, or execution authority.

## Alternatives considered

### Derive availability from persisted CLI output

Rejected because persisted provenance does not expose current occurrence revisions or canonical release evidence and includes claimed and materialized coordinates. Reconstructing lifecycle in clients would duplicate strict kernel replay.

### Return a generic lifecycle status page

Rejected because this command promises exact current availability. Existing persisted, claimed, and materialized commands retain distinct evidence and authority contracts.

### Add global available-occurrence inventory

Rejected because no bounded cross-recurrence ordering or cursor contract exists. One exact caller-selected recurrence is the narrower responsible boundary.

## Consequences

- Operators can page sparse claimable or directly materializable coordinates with the exact revisions later commands require.
- CLI allocation, cursor, ordering, lifecycle filtering, recovery evidence, and corruption isolation remain aligned with the kernel boundary.
- Claims and materializations disappear from later available views; releases reappear with exact latest recovery evidence while authored-window progress remains deterministic.
- Global discovery, claim-next selection, workers, leases, dispatch, retries, and execution remain deferred.

## Verification

RED→GREEN CLI integration tests prove deterministic escaped JSON, exact current revisions, persisted-only null release evidence, latest release evidence, claimed and materialized omission, all-gap progress, final-page termination, validation before storage access, typed missing and bounds failures, read-only missing-path behavior, and selected-window corruption isolation. The complete repository quality gate must remain green.

## Revisit when

Reconsider before adding cross-recurrence available inventory, claim-next selection, durable cursors, generated task identity, workers, leases or expiry, ambient clocks, dispatch, retries, permissions, or execution.
