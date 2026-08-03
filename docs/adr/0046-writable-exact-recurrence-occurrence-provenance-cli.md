# ADR-0046: Writable exact recurrence occurrence provenance CLI

- **Status:** accepted
- **Date:** 2026-08-03
- **Decision and execution issue:** [#879](https://github.com/Knosence9/project-vela/issues/879)
- **Related:** ADR-0044, ADR-0045

## Context

ADR-0045 establishes a narrow kernel boundary for durably recording one exact authored recurrence coordinate. Operators can create and inspect finite recurrence definitions and page their inert projections through the developer CLI, but cannot cross the existing occurrence-provenance boundary without a custom adapter.

Selection, catch-up policy, schedule or task materialization, and execution each require additional authority. The smallest responsible adapter accepts one caller-selected recurrence coordinate and observed definition revision, then delegates the complete persistence contract to the kernel.

## Decision

`vela-dev recurrence persist DATABASE RECURRENCE_ID EXPECTED_REVISION OFFSET` validates `RECURRENCE_ID` through `RecurrenceId` before storage access. Clap parses the expected revision and zero-based offset as non-negative `u64` values before command execution. The command opens only the caller-selected database through `RecurrenceStore::open` and calls `RecurrenceStore::persist_occurrence` with the exact caller-owned coordinate and observed revision.

Success emits one compact JSON object preserving exact `recurrence_id`, `goal`, `offset`, `unix_millis`, and `definition_revision`. Exact caller-authored strings are JSON escaped without trimming or normalization.

An invalid ID emits `invalid_recurrence_id` before storage access. Missing definitions, stale revisions, out-of-range offsets, duplicate coordinates, invalid durable history, storage open, replay, append, and serialization failures emit `recurrence_occurrence_persistence_failed`. Every failure returns non-zero and emits no partial stdout. Kernel validation ensures failed persistence cannot replace an existing coordinate.

The command reads no ambient time and makes no selection or due decision. It does not choose catch-up policy, generate identities, create schedules or tasks, claim or cancel work, dispatch, retry, grant permission, or execute anything.

## Alternatives considered

### Persist every projected page

Rejected because paging is a read-only inspection boundary. Implicitly persisting a page would combine projection with a bulk lifecycle decision and make partial failure semantics necessary.

### Omit the expected definition revision

Rejected because the caller must identify the exact durable definition observation that authorized the transition. The kernel already rejects stale provenance and preserves this revision in the occurrence payload.

### Materialize a one-shot schedule or task directly

Rejected because identity ownership, atomic provenance binding, catch-up selection, and downstream lifecycle policy are independent decisions. This command records only immutable occurrence provenance.

## Consequences

- Operators can durably record one exact recurrence coordinate through deterministic machine-readable CLI output.
- Invalid identities are rejected before the selected path can be created.
- Kernel duplicate, revision, bounds, and fail-closed replay guarantees remain authoritative rather than being reimplemented in the adapter.
- Occurrence inventory, catch-up policy, schedule/task materialization, claims, dispatch, retries, and execution remain deferred.

## Verification

Strict RED→GREEN CLI integration tests prove deterministic exact JSON, reopen persistence, pre-open identity validation, missing, stale, out-of-range, and duplicate rejection, and preservation of the original coordinate. The complete repository quality gate must remain green.

## Revisit when

Reconsider before adding bulk occurrence transitions, occurrence inventory or cancellation, catch-up or missed-run policy, generated schedule/task identities, materialization, claims, retries, workers, ambient clocks, dispatch, or execution.
