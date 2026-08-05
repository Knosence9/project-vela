# ADR-0067: Writable atomic due recurrence task materialization CLI

- **Status:** accepted
- **Date:** 2026-08-05
- **Decision and execution issue:** [#927](https://github.com/Knosence9/project-vela/issues/927)
- **Related:** ADR-0056, ADR-0058, ADR-0066

## Context

ADR-0066 atomically selects one bounded due page, records canonical persisted-to-materialized provenance for every selected coordinate, and binds each coordinate to one ordered caller-owned inert task. Callers otherwise need custom Rust code or a non-atomic composition of due selection, page persistence, and repeated exact materialization.

The smallest responsible adapter must preserve the kernel's exact recurrence, observed definition revision, authored start, bounded page size, explicit cutoff, and ordered task identities. It must not add an ambient clock, global discovery, generated identity, durable cursor, sparse recovery policy, dispatch, or execution authority.

## Decision

Add:

```text
vela-dev recurrence materialize-due DATABASE RECURRENCE_ID EXPECTED_REVISION START_OFFSET PAGE_SIZE CUTOFF_UNIX_MILLIS [TASK_IDS]...
```

Clap parses the definition revision, authored start, page size, and inclusive cutoff as non-negative `u64` values before command execution. The command validates `RECURRENCE_ID`, the positive at-most-1024 `PAGE_SIZE`, and every supplied `TASK_ID` before opening the exact caller-selected database with `RecurrenceStore::open`.

The command delegates bounded due selection, exact definition-revision validation, task-count and duplicate validation, selected occurrence and task-stream uniqueness, contention classification, and the atomic page append to `RecurrenceStore::materialize_due_occurrences_page`. It does not duplicate those rules. Zero task IDs are accepted syntactically so an empty future-horizon page can remain write-free; a non-empty selection requires exactly one ordered task ID per occurrence.

Success emits one compact deterministic JSON object. `occurrences` contains complete materialized bindings in authored-offset order—exact `recurrence_id`, `goal`, `offset`, `unix_millis`, `definition_revision`, resulting `occurrence_revision`, and `task_id`. `next_offset` is the kernel-returned following authored coordinate, unchanged future cursor, or `null` at finite completion. Exact caller-authored strings remain JSON escaped.

Invalid identity or page size emits `invalid_recurrence_id`, `invalid_occurrence_page_size`, or `invalid_task_id` before storage access. Missing or stale definitions, invalid starts, task-count mismatch, duplicate identities, existing or malformed selected provenance, task collisions, concurrency, open, replay, append, and serialization failures emit one escaped `due_recurrence_occurrence_materialization_failed` diagnostic, return non-zero, and emit no stdout. Kernel mutation failures leave no selected prefix or orphan task. Serialization happens only after a successful atomic commit and therefore cannot roll that commit back; the current fixed-field JSON projection has no data-dependent serialization failure.

A future horizon writes nothing and preserves its resumable cursor. The adapter reads no ambient clock, discovers no unrelated recurrence, generates no identity, persists no cursor, and grants no claim, lease, dispatch, retry, workflow, provider/tool, permission, or execution authority.

## Alternatives considered

### Persist the due page, then materialize each coordinate

Rejected because process failure or contention can leave a task-bound prefix, contrary to the caller's page-level consequential operation.

### Generate task identities in the CLI

Rejected because identity allocation, collision handling, and retry policy remain caller authorities.

### Require at least one task identity syntactically

Rejected because a valid future-horizon selection is empty and must be representable without a placeholder identity or write.

## Consequences

- Scripts can atomically commit one explicit bounded all-due page and its inert task bindings without custom code.
- Read-only due paging, persistence-only due paging, exact recovery materialization, latest-only materialization, and bounded all-due materialization retain separate authority boundaries.
- Future horizons remain write-free and resumable; final-page and maximum-instant behavior remain explicit.
- Callers retain cutoff choice, cursor retention, ordered task identity allocation, retry and recovery policy, and execution authority.

## Verification

Strict RED→GREEN CLI integration tests cover deterministic escaped JSON, ordered multi-coordinate materialization, canonical persisted-to-materialized provenance, authoritative task goals, reopen lookup, future write-free resumption, count mismatch, duplicate identities, invalid input before storage access, stale definitions, task collision atomicity, and final-page materialization through `u64::MAX`. The complete repository quality gate must remain green.

## Revisit when

Reconsider before adding generated task identities, durable due cursors, sparse idempotent recovery, persisted-only automatic recovery, global recurrence discovery, ambient clocks, claims or leases, dispatch, retries, or execution.
