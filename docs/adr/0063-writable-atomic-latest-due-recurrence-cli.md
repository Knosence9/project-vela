# ADR-0063: Writable atomic latest-due recurrence CLI adapter

- **Status:** accepted
- **Date:** 2026-08-05
- **Decision and execution issue:** [#919](https://github.com/Knosence9/project-vela/issues/919)

## Context

ADR-0062 provides an atomic kernel mutation for persisting only the latest occurrence due from one caller-owned authored coordinate and inclusive cutoff. Operators otherwise need custom code or a read-only selection followed by exact persistence, which would separate the consequential catch-up choice from the revision-bound atomic write.

The responsible adapter must remain thin: one exact recurrence, one observed immutable definition revision, one authored start, and one explicit cutoff. It must not add an ambient clock, discovery, durable cursor or skip semantics, generated identity, lifecycle policy, or execution authority.

## Decision

`vela-dev recurrence persist-latest-due DATABASE RECURRENCE_ID EXPECTED_REVISION START_OFFSET CUTOFF_UNIX_MILLIS` validates `RECURRENCE_ID` through `RecurrenceId` before storage access. Clap parses the revision, authored start, and cutoff as non-negative `u64` values. The command opens only the selected database through `RecurrenceStore::open` and delegates selection, revision validation, duplicate protection, and atomic persistence to `RecurrenceStore::persist_latest_due_occurrence`.

Success emits the same compact JSON shape as read-only latest-due selection. `occurrence` contains the complete selected persisted provenance or `null`; `next_offset` contains the following authored coordinate, the unchanged future coordinate, or `null` at finite completion. A future horizon writes nothing. Skipped coordinates remain uninspected and unpersisted.

Invalid identity emits `invalid_recurrence_id` before storage access. Open, missing-definition, stale-revision, bounds, duplicate, selected-corruption, concurrency, persistence, and serialization failures emit `latest_due_recurrence_occurrence_persistence_failed`, return non-zero, and emit no stdout. Kernel atomicity ensures every failure appends no selected provenance.

The adapter reads no ambient clock, persists no cursor or skipped-coordinate evidence, discovers no unrelated recurrence, generates no identity, and grants no materialization, task lifecycle, claim, lease, dispatch, retry, workflow, provider/tool, permission, or execution authority.

## Alternatives considered

### Select with `latest-due`, then call `persist`

Rejected because definition revision and selected-stream absence would not be rechecked in one transaction with the policy selection.

### Persist every skipped coordinate

Rejected because that changes latest-only catch-up into all-due persistence and fabricates provenance for work the caller did not select.

### Read the current system clock by default

Rejected because cutoff selection is caller authority and deterministic evidence must not depend on ambient time.

## Consequences

- Operators can atomically persist one explicit latest-only catch-up choice without custom code.
- Writable and read-only latest-due commands share one deterministic JSON projection.
- Future horizons remain write-free and resumable; finite completion remains explicit.
- Callers retain cutoff choice, cursor retention, retry policy, later materialization, identity generation, and execution authority.

## Verification

RED→GREEN CLI integration tests cover deterministic escaped JSON, between-instant selection, skipped-coordinate absence after reopen, future write-free resumption, finite completion, `u64::MAX`, validation before storage access, missing and stale definitions, invalid starts, and duplicate rejection. The complete repository quality gate must remain green.

## Revisit when

Reconsider before adding durable cursor or skip evidence, global recurrence discovery, idempotent persistence, generated task identities, recurrence lifecycle, claims or leases, ambient clocks, materialization policy, dispatch, retries, or execution.
