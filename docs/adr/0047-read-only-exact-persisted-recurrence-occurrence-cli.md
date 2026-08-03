# ADR-0047: Read-only exact persisted recurrence occurrence CLI lookup

- **Status:** accepted
- **Date:** 2026-08-03
- **Decision and execution issue:** [#881](https://github.com/Knosence9/project-vela/issues/881)
- **Related:** ADR-0045, ADR-0046

## Context

ADR-0045 establishes canonical durable provenance for one exact finite recurrence coordinate, and ADR-0046 exposes its explicit write boundary through the developer CLI. Operators still cannot verify one persisted coordinate through a supported read-only adapter. Projecting the authored coordinate again is insufficient because a projection does not prove that provenance was persisted.

Occurrence inventory, catch-up selection, materialization, and execution each require broader and independent authority. The smallest responsible follow-on is exact read-only lookup using the caller-owned recurrence ID and offset.

## Decision

`vela-dev recurrence occurrence DATABASE RECURRENCE_ID OFFSET` validates `RECURRENCE_ID` through `RecurrenceId` before storage access. Clap parses the zero-based offset as a non-negative `u64` before command execution. The command opens only the caller-selected existing database through `RecurrenceStore::open_read_only` and delegates canonical coordinate lookup and complete provenance validation to `RecurrenceStore::load_occurrence`.

Success emits one compact JSON object preserving exact `recurrence_id`, `goal`, `offset`, `unix_millis`, and `definition_revision`. Exact caller-authored strings are JSON escaped without trimming or normalization.

An invalid ID emits `invalid_recurrence_id` before storage access. A valid absent coordinate emits `recurrence_occurrence_not_found`. Storage open, strict selected-definition or selected-occurrence replay, provenance validation, and serialization failures emit `recurrence_occurrence_lookup_failed`. Every failure returns non-zero and emits no stdout. A missing database remains missing, and corruption in unrelated streams cannot block the exact lookup.

The command reads no ambient time and persists nothing. It cannot enumerate occurrences, choose catch-up or due policy, generate identities, create schedules or tasks, claim, cancel, dispatch, retry, grant permission, or execute anything.

## Alternatives considered

### Reuse projected occurrence paging

Rejected because projection proves only what an immutable definition authors. It does not prove that the exact occurrence provenance stream exists and passes strict replay.

### Return a nullable success object

Rejected because this command selects one required durable coordinate. A typed not-found diagnostic keeps absence distinct from successful provenance and from invalid durable state.

### Add persisted occurrence inventory

Rejected because inventory requires a separate discovery, ordering, allocation-bounding, and corruption-domain contract. Exact canonical lookup already exists and grants narrower authority.

## Consequences

- Operators can verify one exact persisted recurrence coordinate through deterministic machine-readable output.
- Invalid identities and missing storage cannot create a database.
- Canonical stream lookup and strict kernel replay remain authoritative rather than being reimplemented in the CLI.
- Persisted occurrence inventory, catch-up policy, schedule/task materialization, dispatch, and execution remain deferred.

## Verification

Strict RED→GREEN CLI integration tests prove deterministic exact JSON after reopen, pre-storage identity validation, missing storage preservation, distinct absent-coordinate failure, and fail-closed selected-coordinate corruption. The complete repository quality gate must remain green.

## Revisit when

Reconsider before adding persisted occurrence inventory or paging, catch-up or missed-run policy, mutable recurrence lifecycle, generated identities, schedule/task materialization, claims, retries, workers, ambient clocks, dispatch, or execution.
