# ADR-0059: Writable atomic due recurrence page CLI

- **Status:** accepted
- **Date:** 2026-08-04
- **Decision and execution issue:** [#909](https://github.com/Knosence9/project-vela/issues/909)

## Context

ADR-0058 provides an atomic, allocation-bounded kernel mutation for persisting one exact recurrence's caller-selected due page. Operators otherwise need a custom adapter or repeated exact-coordinate commands, which can lose the page-level atomicity established by the kernel contract.

The responsible CLI boundary remains a thin adapter over one exact recurrence, one observed immutable definition revision, one authored start, one bounded page size, and one explicit inclusive cutoff. The CLI must not read ambient time or add catch-up, identity, lifecycle, or execution policy.

## Decision

`vela-dev recurrence persist-due DATABASE RECURRENCE_ID EXPECTED_REVISION START_OFFSET PAGE_SIZE CUTOFF_UNIX_MILLIS` validates `RECURRENCE_ID` through `RecurrenceId` and `PAGE_SIZE` through `OccurrencePageSize` before storage access. Clap parses the revision, authored offset, page size, and cutoff as non-negative `u64` values. The command opens only the selected database through `RecurrenceStore::open` and delegates directly to `RecurrenceStore::persist_due_occurrences_page`.

Success emits one compact JSON page. `occurrences` are ordered by authored offset and preserve exact `recurrence_id`, `goal`, `offset`, `unix_millis`, and `definition_revision`. `next_offset` identifies the first uninspected or future coordinate and is `null` only at finite completion. A cutoff-truncated non-empty page resumes at the first future coordinate; an empty future-horizon page preserves its unchanged cursor.

Invalid identities and page sizes fail before storage access as `invalid_recurrence_id` and `invalid_occurrence_page_size`. Open, missing-definition, stale-revision, bounds, duplicate, selected-corruption, concurrency, persistence, and serialization failures emit `due_recurrence_occurrence_persistence_failed`, return non-zero, and emit no stdout. Kernel page atomicity ensures failures persist no selected prefix.

The command reads no ambient clock, persists no cursor, generates no identity, discovers no unrelated recurrence, and grants no global catch-up, materialization, task lifecycle, claim, lease, dispatch, retry, workflow, provider/tool, permission, or execution authority.

## Alternatives considered

### Script repeated `recurrence persist` commands

Rejected because process failure or a later conflict can durably record only a prefix of the selected page.

### Let the CLI select due coordinates itself

Rejected because duplicating inclusive cutoff and cursor policy would create a second scheduler contract and risk divergence from the kernel.

### Read the current system clock by default

Rejected because cutoff choice is caller authority and deterministic evidence must not depend on ambient time.

## Consequences

- Operators can persist one bounded due page while retaining kernel atomicity and typed validation.
- Exact JSON and cursor semantics match read-only due paging.
- Callers still own cutoff choice, cursor retention, retries, later materialization, identity generation, and execution.

## Verification

RED→GREEN CLI integration tests cover deterministic bounded persistence, cutoff truncation, empty future horizons, later-cutoff resumption, finite completion, `u64::MAX`, validation before storage access, missing/stale/out-of-range input, duplicate and corrupted selected provenance, and no partial page on failure. The complete repository quality gate must remain green.

## Revisit when

Reconsider before adding global recurrence discovery, idempotent sparse catch-up, durable cursors, ambient clocks, generated task identities, recurrence lifecycle, claims or leases, dispatch, retries, or execution.
