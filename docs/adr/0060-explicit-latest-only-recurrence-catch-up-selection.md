# ADR-0060: Explicit latest-only finite recurrence catch-up selection

- **Status:** accepted
- **Date:** 2026-08-04
- **Decision and execution issue:** [#913](https://github.com/Knosence9/project-vela/issues/913)

## Context

ADR-0056 defines allocation-bounded all-due paging for one exact finite recurrence. A caller that deliberately wants to collapse an accumulated backlog to only its latest due authored occurrence would otherwise need to page every due coordinate and discard earlier results. That both scales with backlog size and obscures a consequential missed-run policy inside adapter code.

The smallest responsible policy boundary is explicit, read-only, and exact-recurrence scoped. The caller must continue to own the starting coordinate and inclusive time cutoff, while persistence, durable skip evidence, materialization, identity, dispatch, and execution remain separate decisions.

## Decision

`RecurrenceStore::latest_due_occurrence(id, start_offset, cutoff)` names the latest-only catch-up choice directly. It strictly replays one exact immutable recurrence and validates `start_offset` through the existing exact occurrence projection. If the starting occurrence is in the future, the result contains no occurrence and preserves `Some(start_offset)` as its resumable cursor.

When the starting occurrence is due, the method calculates the latest authored offset at or before the inclusive caller-owned cutoff with checked recurrence invariants, integer division, saturating addition, and a finite-count cap. It projects only that exact coordinate. The typed `LatestDueOccurrenceSelection` returns the complete occurrence plus the following authored offset when the definition continues, or `None` at finite completion. Selection therefore uses constant space and constant arithmetic work regardless of skipped-backlog size.

The projection reads no ambient clock, persists no cursor or skip evidence, scans no unrelated recurrence, generates no identity, and grants no occurrence persistence, materialization, task lifecycle, dispatch, retry, workflow, provider/tool, permission, or execution authority.

## Alternatives considered

### Add a generic catch-up policy enum now

Rejected under YAGNI. All-due paging already has a stable method, while this slice adds one materially different latest-only projection. An enum would imply additional policy variants and a shared result shape before they are needed.

### Page all due occurrences and retain the final result

Rejected because work scales with backlog size and the latest-only policy becomes implicit in each caller rather than visible in the kernel API.

### Persist skipped-coordinate evidence automatically

Rejected because a read-only policy selection is not durable proof that the caller accepted, persisted, or acted on the skip. Durable cursor and skip semantics require their own revision-bound lifecycle contract.

### Read the current clock inside the store

Rejected because the due horizon remains caller authority and deterministic replay must not depend on ambient time.

## Consequences

- Callers can select an explicit latest-only catch-up result without allocating or iterating over skipped backlog.
- Future-horizon and finite-completion cursors compose with later caller-owned cutoffs.
- Missing definitions and invalid starts preserve existing typed failures.
- All-due paging remains unchanged for callers that require every due coordinate.
- No skipped occurrence becomes durable merely because it was bypassed by this projection.

## Verification

RED→GREEN integration tests cover exact and between-instant inclusive selection, nonzero starts, future-horizon resumption, finite completion at `u64::MAX`, a `u64::MAX` occurrence count selected in constant space, missing definitions, and out-of-range starts. The complete repository quality gate must remain green.

## Revisit when

Reconsider before adding more catch-up modes, a CLI adapter, persisted skip evidence or cursors, recurrence cancellation, global due discovery, ambient clocks, generated task identities, claims or leases, materialization policy, dispatch, retries, or execution.
