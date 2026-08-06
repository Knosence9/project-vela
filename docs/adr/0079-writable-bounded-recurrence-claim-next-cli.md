# ADR-0079: Writable bounded recurrence claim-next CLI

- **Status:** accepted
- **Date:** 2026-08-06
- **Decision and execution issue:** [#953](https://github.com/Knosence9/project-vela/issues/953)
- **Related:** ADR-0070, ADR-0076, ADR-0077, ADR-0078

## Context

ADR-0078 establishes one bounded kernel mutation that selects and claims the earliest available due coordinate in an exact caller-selected recurrence window. The CLI exposes exact claiming and read-only availability paging, but automation would otherwise have to compose those boundaries and recreate cursor, cutoff, ordering, and contention policy.

## Decision

Add `vela-dev recurrence claim-next DATABASE RECURRENCE_ID START_OFFSET PAGE_SIZE CUTOFF_UNIX_MILLIS`.

The adapter validates the exact recurrence identity and positive, at-most-1024 page size before storage access. It interprets the cutoff as an inclusive caller-owned `ScheduleInstant`, opens only the selected database through writable `RecurrenceStore::open`, and delegates all selection, complete-window race protection, strict replay, and mutation to `RecurrenceStore::claim_next_available_occurrence`.

Success emits one compact JSON object with `occurrence` and `next_offset`. A claimed result preserves exact recurrence ID, goal, authored offset, instant, definition revision, resulting occurrence revision, and nullable latest release provenance. No eligible coordinate emits `"occurrence":null`; its nullable kernel-owned cursor distinguishes future resumption, window progress, and finite completion.

Invalid identity and page size use `invalid_recurrence_id` and `invalid_occurrence_page_size`. Missing definitions and out-of-range starts use `recurrence_not_found` and `recurrence_occurrence_out_of_range`. Open, strict replay, provenance, contention exhaustion, append, read-only, and serialization failures use `recurrence_occurrence_claim_next_failed`. Every failure returns non-zero and emits no stdout.

The adapter reads no ambient clock, persists no cursor, scans no unrelated recurrence, generates no identity, and grants no worker, lease, expiry, dispatch, retry-of-work, permission, provider/tool, workflow, materialization, or execution authority.

## Alternatives considered

### Compose available paging and exact claim in the CLI

Rejected because it duplicates the kernel's ordering, future-cursor, complete-window race, and contention policy.

### Select across all recurrence definitions

Rejected because no bounded cross-recurrence ordering or cursor contract exists and the command's exact identity is an intentional authority boundary.

### Read ambient time or persist the returned cursor

Rejected because cutoff and resumption policy belong to the caller; neither is required to expose the accepted kernel mutation.

## Consequences

- Automation can reserve one deterministic due coordinate with one bounded command.
- Released coordinates preserve the latest recovery evidence that informed their eligibility.
- Sparse, future, consumed, and finite windows retain the kernel cursor contract.
- Cross-recurrence discovery and worker/execution semantics remain deferred.

## Verification

RED→GREEN CLI integration tests cover deterministic escaping and projection, sparse earliest selection, released provenance, future cursor preservation, all-gap and finite-end progress, validation before storage, missing and out-of-range definitions, selected-window corruption, read-only failure, and empty stdout on every error. The complete repository quality gate must remain green.

## Revisit when

Reconsider before adding cross-recurrence selection, durable cursors, ambient clocks, generated identities, worker ownership, leases or expiry, dispatch, retries, permissions, materialization, or execution.
