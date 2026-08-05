# ADR-0065: Writable atomic latest-due recurrence task materialization CLI

- **Status:** accepted
- **Date:** 2026-08-05
- **Decision and execution issue:** [#923](https://github.com/Knosence9/project-vela/issues/923)
- **Related:** ADR-0060, ADR-0061, ADR-0064

## Context

ADR-0064 atomically selects one latest due recurrence coordinate, records its canonical persisted-to-materialized provenance, and binds it to one caller-owned inert task. Callers otherwise need custom Rust code or a non-atomic composition of read-only selection, persistence, and exact materialization.

The smallest responsible adapter must preserve the kernel's exact recurrence, observed definition revision, caller-owned authored start, explicit cutoff, and caller-owned task identity. It must not add an ambient clock, global discovery, generated identity, durable cursor or skip semantics, recovery policy, dispatch, or execution authority.

## Decision

Add:

```text
vela-dev recurrence materialize-latest-due DATABASE RECURRENCE_ID EXPECTED_REVISION START_OFFSET CUTOFF_UNIX_MILLIS TASK_ID
```

Clap parses the definition revision, authored start, and inclusive cutoff as non-negative `u64` values before command execution. The command validates `RECURRENCE_ID` through `RecurrenceId` and `TASK_ID` through `TaskId` before opening the exact caller-selected database with `RecurrenceStore::open`.

The command delegates constant-space latest-only projection, exact definition-revision validation, selected occurrence and task-stream uniqueness, contention classification, and the atomic three-event append to `RecurrenceStore::materialize_latest_due_occurrence`. It does not duplicate those rules.

Success emits one compact deterministic JSON object. `occurrence` contains the complete selected materialized binding—exact `recurrence_id`, `goal`, `offset`, `unix_millis`, `definition_revision`, resulting `occurrence_revision`, and `task_id`—or `null` when the starting coordinate remains future. `next_offset` is the following authored coordinate, the unchanged future cursor, or `null` at finite completion. Exact caller-authored strings remain JSON escaped.

Invalid identities emit `invalid_recurrence_id` or `invalid_task_id` before storage access. Missing or stale definitions, invalid starts, existing or malformed selected provenance, task collisions, concurrency, open, replay, append, and serialization failures emit one escaped `latest_due_recurrence_occurrence_materialization_failed` diagnostic, return non-zero, and emit no stdout. Kernel atomicity leaves no partial occurrence history or orphan task.

A future horizon writes nothing. Skipped coordinates remain uninspected and unpersisted, and the returned cursor is not durable acceptance, waiver, or lifecycle evidence. The adapter reads no ambient clock, discovers no unrelated recurrence, generates no identity, and grants no claim, lease, dispatch, retry, workflow, provider/tool, permission, or execution authority.

## Alternatives considered

### Select latest due, persist it, then materialize it

Rejected because the selected occurrence can race or the process can fail between persistence and task binding, leaving provenance without its intended caller-owned task.

### Generate a task identity in the CLI

Rejected because identity generation, collision handling, and retry policy remain caller authorities.

### Consume persisted-only latest-due evidence automatically

Rejected because this would add recovery policy to catch-up selection. Exact `recurrence materialize` remains the explicit recovery boundary for an observed persisted occurrence revision.

## Consequences

- Scripts can atomically commit one explicit latest-only catch-up choice and inert task binding without custom code.
- Read-only selection, persistence-only selection, exact persisted-occurrence materialization, and combined latest-only materialization retain separate authority boundaries.
- Future horizons remain write-free and resumable; finite completion remains explicit.
- Callers retain cutoff choice, cursor retention, task identity, retry and recovery policy, and execution authority.

## Verification

Strict RED→GREEN CLI integration tests cover deterministic escaped JSON, between-instant latest-only selection, canonical persisted-to-materialized provenance, authoritative task goal, skipped-coordinate absence, future write-free resumption, finite completion at `u64::MAX`, identity validation before storage access, missing and stale definitions, invalid starts, task collision atomicity, and duplicate selected provenance. The complete repository quality gate must remain green.

## Revisit when

Reconsider before adding generated task identities, durable catch-up cursors or skipped-run evidence, persisted-only recovery, idempotent materialization, global recurrence discovery, ambient clocks, claims or leases, dispatch, retries, or execution.
