# ADR-0045: Durable exact finite recurrence occurrence provenance

- **Status:** accepted
- **Date:** 2026-08-03
- **Decision and execution issue:** [#875](https://github.com/Knosence9/project-vela/issues/875)
- **Related:** ADR-0037, ADR-0038, ADR-0039

## Context

ADR-0037 through ADR-0044 establish immutable finite recurrence definitions and bounded read-only occurrence projection. The exact `(recurrence ID, offset)` coordinate is stable projection provenance, but callers cannot durably record that one authored occurrence has crossed into scheduler state without inventing another identity or an ad hoc schema.

Task or one-shot schedule materialization would additionally decide identity ownership, catch-up, selection, and lifecycle policy. The smaller responsible boundary is durable occurrence provenance that remains inert.

## Decision

`RecurrenceStore::persist_occurrence(id, expected_revision, offset)` loads and strictly validates the exact recurrence definition, requires the caller's exact observed definition revision, and projects the authored offset through `FixedIntervalRecurrence::occurrence_at`. Missing definitions, stale revisions, and out-of-range offsets fail before occurrence persistence.

The exact `(RecurrenceId, offset)` coordinate is the durable occurrence identity. Its internal stream name uses a byte-length-prefixed recurrence ID followed by the exact decimal offset, making the encoding collision-free for every accepted UTF-8 ID without exposing another caller-owned identity. Each coordinate accepts exactly one version-1 `recurrence.occurrence_persisted` event with `ExpectedVersion::NoStream`. The payload preserves exact recurrence ID, definition revision, offset, validated goal, and Unix-millisecond instant. A duplicate coordinate returns typed `OccurrenceAlreadyPersisted` evidence and cannot replace the first event.

`RecurrenceStore::load_occurrence(id, offset)` replays only the exact coordinate stream. Absence returns `None`. Existing evidence must contain exactly one supported event whose payload coordinate and every projected provenance field agree with the authoritative recurrence definition; malformed payloads, unsupported types or versions, invalid definitions, divergent fields, and other history shapes fail closed.

Persistence and lookup read no ambient clock and perform no inventory scan. Durable occurrence evidence is inert provenance only. It is not a due or catch-up decision, cursor, one-shot schedule, task, claim, permission grant, dispatch request, retry, or execution outcome.

## Alternatives considered

### Let callers assign an occurrence ID

Rejected because the recurrence ID and authored offset already form a canonical identity. A second caller-owned identity would require duplicate-coordinate detection across unrelated streams and permit ambiguous provenance.

### Append occurrences to the recurrence definition stream

Rejected because it would turn immutable definition replay into an ever-growing lifecycle, serialize unrelated occurrence writers behind one revision, and make exact occurrence lookup scan definition history.

### Materialize directly into one-shot schedules or tasks

Rejected because schedule/task identity, catch-up selection, atomic provenance binding, and execution lifecycle are separate policy decisions. Durable provenance can be established without granting those authorities.

### Treat duplicate persistence as success

Rejected because explicit typed evidence lets callers distinguish a first durable transition from an already-recorded coordinate without rewriting history.

## Consequences

- Every persisted occurrence has one canonical, collision-free durable identity and complete immutable definition provenance.
- Exact lookup is isolated from unrelated occurrence streams and validates evidence against its authoritative definition.
- Recurrence definitions remain one-event immutable histories.
- Occurrence inventory and lifecycle, catch-up policy, schedule/task materialization, claims, dispatch, retries, and execution remain deferred.

## Verification

Strict RED→GREEN tests prove first and final persistence, exact `u64::MAX` provenance, UTF-8/separator coordinate isolation, duplicate preservation, missing/stale/out-of-range rejection, exact missing lookup, reopen, and fail-closed event, payload, coordinate, and history corruption. The complete repository quality gate must remain green.

## Revisit when

Reconsider before adding occurrence inventory, mutable definitions, cancellation, catch-up or missed-run policy, generated schedule/task identities, materialization, claims, retries, workers, calendar/time-zone semantics, ambient clocks, dispatch, or execution.
