# ADR-0057: Read-only due recurrence occurrence CLI paging

- **Status:** accepted
- **Date:** 2026-08-04

## Context

ADR-0056 defines allocation-bounded due-occurrence projection for one exact finite recurrence through an inclusive caller-owned cutoff. Operators otherwise need a custom kernel adapter to use that boundary. Reading ambient time, selecting across definitions, persisting a catch-up cursor, or generating task identities would combine separate policy and authority decisions.

## Decision

`vela-dev recurrence due DATABASE RECURRENCE_ID START_OFFSET PAGE_SIZE CUTOFF_UNIX_MILLIS` validates `RECURRENCE_ID` through `RecurrenceId` and `PAGE_SIZE` through `OccurrencePageSize` before storage access. Clap parses the authored start offset and caller-owned cutoff as non-negative `u64` values. The command opens only the selected existing database through `RecurrenceStore::open_read_only` and delegates projection to `RecurrenceStore::due_occurrences_page`.

Success emits one compact JSON page. `occurrences` are ordered by authored offset and preserve exact `recurrence_id`, `goal`, `offset`, `unix_millis`, and `definition_revision`. `next_offset` advances after a full bounded page, identifies the first future authored coordinate when the cutoff stops selection, and is `null` only at the finite definition end. An empty page with a non-null unchanged cursor means that the selected coordinate is beyond the current cutoff and may be retried with a later caller-owned cutoff.

Invalid identities and page sizes emit `invalid_recurrence_id` and `invalid_occurrence_page_size` before storage access. Missing definitions emit `recurrence_not_found`; invalid starts emit `recurrence_occurrence_out_of_range`. Storage open, strict selected-definition replay, due projection, and serialization failures emit `due_recurrence_occurrence_lookup_failed`. Every failure is non-zero, emits one escaped diagnostic, and emits no partial stdout. Missing storage remains missing, and corruption in unrelated streams cannot block the selected definition.

The command reads no ambient clock and persists no cursor. It grants no global discovery, missed-run or catch-up choice, generated identity, occurrence persistence, materialization, cancellation, claim, lease, dispatch, workflow, provider/tool, permission, retry, or execution authority.

## Alternatives considered

### Read the system clock in the CLI

Rejected because time authority must remain explicit and reproducible. The cutoff is caller-owned input.

### Select due occurrences across every recurrence

Rejected because global discovery broadens work, corruption, ordering, and catch-up policy beyond the exact recurrence boundary established by ADR-0056.

### Persist the returned cursor automatically

Rejected because a projection coordinate is not lifecycle state. Cursor ownership, retries, and catch-up policy require separate contracts.

## Consequences

- Operators can inspect one recurrence's bounded due projection without embedding the kernel.
- CLI JSON preserves the kernel distinction between a temporary cutoff horizon and permanent finite completion.
- Validation and strict replay remain owned by existing value types and the recurrence store.
- Callers remain responsible for cutoff selection, cursor retention, catch-up policy, identity generation, and any later mutation or execution.
