# ADR-0090: Read-only finite recurrence status CLI filtering

- **Status:** accepted
- **Date:** 2026-08-07
- **Decision and execution issue:** [#981](https://github.com/Knosence9/project-vela/issues/981)
- **Related:** ADR-0041, ADR-0089

## Context

ADR-0089 exposes fail-closed exact lifecycle filtering through `RecurrenceStore::list_by_status`. Operators otherwise need custom kernel code or must retrieve the complete recurrence inventory and duplicate status parsing, filtering, diagnostics, and storage authority policy.

The smallest responsible adapter should preserve the kernel operation's canonical discovery and complete lifecycle validation while reusing the existing deterministic inventory representation.

## Decision

Add `vela-dev recurrence status DATABASE STATUS`. The command accepts only exact lowercase `active` or `cancelled`, validates that caller-owned status before storage access, opens only the selected existing database through `RecurrenceStore::open_read_only`, and delegates filtering to `RecurrenceStore::list_by_status`.

Success emits the same compact `{"recurrences":[...]}` document and complete recurrence objects as `recurrence inspect`. Results retain exact recurrence-ID ordering, exact JSON-escaped caller-authored strings, lifecycle status, immutable definition revision, aggregate revision, and nullable cancellation evidence. Empty and unmatched results emit an empty array.

Invalid input emits `invalid_recurrence_status` before storage access. Missing or incompatible storage, malformed recurrence histories, projection failures, and serialization failures emit `recurrence_status_inspection_failed`, return non-zero, and emit no partial stdout. Complete canonical discovery remains fail closed: corruption in a recurrence outside the requested status cannot be hidden by filtering.

The command reads no ambient clock, mutates nothing, and grants no cancellation, occurrence lifecycle, worker, lease, dispatch, retry, permission, or execution authority.

## Alternatives considered

### Filter the `recurrence inspect` JSON externally

Rejected because callers would duplicate an exact status vocabulary and could incorrectly treat partially decoded or corrupted inventory as valid.

### Accept case-insensitive or multiple status values

Rejected because the existing CLI uses exact lowercase lifecycle names and the kernel contract accepts one exact typed status. Multi-status predicates add no value while only two exhaustive states exist.

### Query cancellation events directly

Rejected because event presence is not the canonical lifecycle projection and bypasses complete history validation.

## Consequences

- Operators can request active or cancelled recurrences through a deterministic read-only command.
- Full and filtered recurrence inventory share one JSON serializer.
- Invalid filters cannot create or inspect storage.
- No schema, event, lifecycle transition, clock, cursor, or execution authority is added.

## Verification

Strict RED→GREEN CLI tests prove mixed active and cancelled filtering, exact ID ordering, JSON escaping, empty results, case-sensitive pre-storage validation, missing-storage non-creation, and fail-closed malformed nonmatching histories. The complete repository quality gate must remain green.

## Revisit when

Reconsider before adding pagination, multi-status predicates, destructive deletion, resume or undo semantics, clocks, workers, leases, dispatch, permissions, retries, or execution.
