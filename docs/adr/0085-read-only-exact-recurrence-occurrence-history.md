# ADR-0085: Read-only exact recurrence occurrence history

- **Status:** accepted
- **Date:** 2026-08-07
- **Decision and execution issue:** [#969](https://github.com/Knosence9/project-vela/issues/969)
- **Related:** ADR-0045, ADR-0068, ADR-0069, ADR-0072, ADR-0082, ADR-0084

## Context

Exact recurrence occurrence lookups project current persisted, claimed, released, or materialized state. They intentionally do not expose prior claim and release transitions. Consequently, complete recovery evidence remains available only as raw event-log rows even though exact recurrence aggregate history already has a typed read-only boundary.

Cancellation preserves historical occurrence evidence, so auditability should not depend on mutable eligibility or require raw payload decoding. This boundary must remain narrower than cross-recurrence discovery, worker, lease, dispatch, retry, or execution authority.

## Decision

Add `RecurrenceStore::occurrence_history(&RecurrenceId, offset)`. It replays only the selected recurrence aggregate and exact occurrence streams. A missing occurrence stream returns `None`; an occurrence stream without its authoritative recurrence definition is invalid history.

A present valid stream yields revision-bearing `RecurrenceOccurrenceHistoryEntry` values in persisted order. Its non-exhaustive typed `RecurrenceOccurrenceHistoryEvent` preserves:

- `Persisted`: the complete canonical `RecurrenceOccurrence`, including exact recurrence ID, goal, offset, instant, and immutable definition revision
- `Claimed`
- `Released`: the exact caller-authored recovery reason
- `Materialized`: the exact caller-owned task ID

Before returning any entry, the method strictly decodes the authoritative recurrence definition and complete occurrence stream, then reuses the canonical occurrence lifecycle projector. Valid grammar remains `persisted -> (claimed -> released)*` followed by an optional claim or materialization; direct available-state materialization remains valid. Aggregate cancellation does not erase or hide a previously persisted coordinate's history.

The method works through writable or read-only recurrence stores. It reads no ambient clock, mutates no state, scans no unrelated occurrence coordinates, and grants no persistence, cancellation, claim, release, materialization, discovery, worker, lease, dispatch, permission, retry, or execution authority.

## Alternatives considered

### Return raw event-log envelopes

Rejected because callers would duplicate payload-version, provenance, and lifecycle validation and could consume partial or impossible histories.

### Synthesize history from current projections

Rejected because current state intentionally omits prior claim/release cycles and cannot reproduce durable revision order.

### Scan every occurrence for aggregate history

Rejected because this contract is exact-coordinate inspection. Cross-occurrence discovery needs a separate bounded design.

### Add a CLI adapter in the same slice

Rejected because the smallest reusable boundary is the kernel query. A deterministic read-only CLI adapter can follow independently without changing durable schema.

## Consequences

- Exact persistence, recovery, reservation, and materialization evidence is auditable with persisted revisions.
- Missing occurrence streams remain distinct from present histories.
- Malformed payloads, unsupported events or versions, divergent provenance, impossible ordering, or missing authoritative definitions fail closed before a partial prefix is returned.
- Corruption in unrelated occurrence streams cannot block one exact-coordinate query.
- No durable event, database migration, clock, or execution authority is added.

## Verification

RED→GREEN tests prove repeated claim/release history, claimed and direct materialization, exact reasons, task IDs and revisions, read-only reopen, post-cancellation inspection, missing-stream behavior, complete corruption rejection, and unrelated-stream isolation. The complete repository quality gate must remain green.

## Revisit when

Reconsider before adding an occurrence history CLI, cross-occurrence history discovery, destructive deletion, claim interruption, undo or resume semantics, clocks, workers, leases, dispatch, permissions, retries, or execution.
